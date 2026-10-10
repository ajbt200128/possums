use super::*;
use crate::{
    attestation::EvidenceError,
    catalog::Model,
    inference::{
        stream::{FinishReason, StreamCompletion, StreamUsage},
        tools::{CompletionDelta, ToolInvocation, ToolProfile},
        Inference, InferenceError, Message,
    },
    telemetry::hooks::Lease,
};
use async_trait::async_trait;
use axum::{
    body::{Body, Bytes},
    http::{header, HeaderMap, Request},
    response::Response,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    Mutex,
};
use tokio::{
    sync::{mpsc, Semaphore},
    time::{timeout, Instant},
};
use tower::ServiceExt;

fn key(n: u8) -> String {
    URL_SAFE_NO_PAD.encode([n; 32])
}
fn keys() -> SharedKeys {
    SharedKeys::from_json(&json!([key(1), key(2)]).to_string()).unwrap()
}
struct Evidence;
#[async_trait]
impl EvidenceVerifier for Evidence {
    async fn verify(&self, _: &str, now: u64) -> Result<GatewayEvidence, EvidenceError> {
        Ok(GatewayEvidence {
            quote: json!({"synthetic":true}),
            issued_at_unix: now,
            release_digest: "a".repeat(64),
            endpoint_key_sha256: "b".repeat(64),
            freshness_expires_at_unix: now + 300,
        })
    }
}

struct Event {
    stage: &'static str,
    input: Value,
    allowance: u64,
}
struct Mock {
    events: mpsc::UnboundedSender<Event>,
    count_gate: Semaphore,
    generation_gate: Semaphore,
    count: AtomicU64,
    usage: Mutex<StreamUsage>,
    count_error: Mutex<Option<InferenceFailure>>,
    generation_error: Mutex<Option<InferenceFailure>>,
    panic: AtomicBool,
    count_panic: AtomicBool,
    catalog_gate: Semaphore,
    catalog_entered: tokio::sync::Notify,
    flood: AtomicBool,
    catalog_error: AtomicBool,
    active: AtomicUsize,
    high: AtomicUsize,
    generations: AtomicUsize,
    tokenizers: AtomicUsize,
}
struct Active<'a>(&'a Mock);
impl Drop for Active<'_> {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::SeqCst);
    }
}
impl Mock {
    fn enter(&self) -> Active<'_> {
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.high.fetch_max(active, Ordering::SeqCst);
        Active(self)
    }
    async fn count(&self, input: Value) -> Result<u64, InferenceError> {
        let _active = self.enter();
        self.tokenizers.fetch_add(1, Ordering::SeqCst);
        self.events
            .send(Event {
                stage: "tokenizer",
                input,
                allowance: 0,
            })
            .unwrap();
        self.count_gate.acquire().await.unwrap().forget();
        assert!(
            !self.count_panic.load(Ordering::SeqCst),
            "synthetic-private-panic-marker"
        );
        if let Some(error) = *self.count_error.lock().unwrap() {
            return Err(error.into());
        }
        Ok(self.count.load(Ordering::SeqCst))
    }
    async fn generate(
        &self,
        model: &Model,
        input: Value,
    ) -> Result<StreamCompletion, InferenceError> {
        let _active = self.enter();
        self.generations.fetch_add(1, Ordering::SeqCst);
        self.events
            .send(Event {
                stage: "generation",
                input,
                allowance: model.max_output_tokens,
            })
            .unwrap();
        self.generation_gate.acquire().await.unwrap().forget();
        assert!(
            !self.panic.load(Ordering::SeqCst),
            "synthetic-private-panic-marker"
        );
        if let Some(error) = *self.generation_error.lock().unwrap() {
            return Err(error.into());
        }
        Ok(StreamCompletion {
            usage: *self.usage.lock().unwrap(),
            finish_reason: FinishReason::Stop,
        })
    }
}
#[async_trait]
impl Inference for Mock {
    async fn catalog(&self) -> Result<Vec<u8>, InferenceError> {
        self.catalog_entered.notify_one();
        self.catalog_gate.acquire().await.unwrap().forget();
        if self.catalog_error.load(Ordering::SeqCst) {
            return Err(InferenceFailure::CatalogFailed.into());
        }
        Ok(json!({"object":"list","data":[{"id":"test","type":"chat","context_window":10_000_000,
            "endpoints":["/v1/chat/completions"],"pricing":{"inputTokenPricePer1M":1,"outputTokenPricePer1M":1,"requestPrice":0}}]}).to_string().into_bytes())
    }
    async fn count_tokens(
        &self,
        _: &str,
        messages: &[Message],
        _: Arc<Lease>,
    ) -> Result<u64, InferenceError> {
        self.count(json!({"messages":messages})).await
    }
    async fn generate_completion_stream(
        &self,
        model: &Model,
        messages: &[Message],
        _: Arc<Lease>,
        on_delta: &mut (dyn for<'a> FnMut(&'a str) + Send),
    ) -> Result<StreamCompletion, InferenceError> {
        let completion = self.generate(model, json!({"messages":messages})).await?;
        if self.flood.load(Ordering::SeqCst) {
            for _ in 0..100 {
                on_delta(&"x".repeat(8192));
            }
        } else {
            on_delta("synthetic answer");
        }
        Ok(completion)
    }
    fn tool_profile(&self, _: &str) -> Option<ToolProfile> {
        Some(ToolProfile::OpenAiFunctionsV1)
    }
    async fn count_invocation_tokens(
        &self,
        _: &str,
        invocation: &ToolInvocation,
        _: Arc<Lease>,
    ) -> Result<u64, InferenceError> {
        self.count(serde_json::to_value(invocation).unwrap()).await
    }
    async fn generate_invocation_stream(
        &self,
        model: &Model,
        invocation: &ToolInvocation,
        _: Arc<Lease>,
        on_delta: &mut (dyn for<'a> FnMut(CompletionDelta<'a>) + Send),
    ) -> Result<StreamCompletion, InferenceError> {
        let completion = self
            .generate(model, serde_json::to_value(invocation).unwrap())
            .await?;
        on_delta(CompletionDelta::Text("synthetic answer"));
        Ok(completion)
    }
    fn verification_document(&self) -> Result<Value, InferenceError> {
        Ok(json!({"synthetic":true}))
    }
}

fn fixture(prefix: &str, gated: bool) -> (AppState, Arc<Mock>, mpsc::UnboundedReceiver<Event>) {
    fixture_with_evidence(prefix, gated, Arc::new(Evidence))
}
fn fixture_with_evidence(
    prefix: &str,
    gated: bool,
    verifier: Arc<dyn EvidenceVerifier>,
) -> (AppState, Arc<Mock>, mpsc::UnboundedReceiver<Event>) {
    let (events, receiver) = mpsc::unbounded_channel();
    let mock = Arc::new(Mock {
        events,
        count_gate: Semaphore::new(if gated { 0 } else { 100 }),
        generation_gate: Semaphore::new(if gated { 0 } else { 100 }),
        count: AtomicU64::new(10),
        usage: Mutex::new(StreamUsage {
            input_tokens: 10,
            output_tokens: 5,
            total_tokens: 15,
        }),
        count_error: Mutex::new(None),
        generation_error: Mutex::new(None),
        panic: AtomicBool::new(false),
        count_panic: AtomicBool::new(false),
        catalog_gate: Semaphore::new(100),
        catalog_entered: tokio::sync::Notify::new(),
        flood: AtomicBool::new(false),
        catalog_error: AtomicBool::new(false),
        active: AtomicUsize::new(0),
        high: AtomicUsize::new(0),
        generations: AtomicUsize::new(0),
        tokenizers: AtomicUsize::new(0),
    });
    let state = AppState::with_index(
        keys(),
        mock.clone(),
        Arc::from("synthetic"),
        verifier,
        Arc::from(prefix),
    );
    (state, mock, receiver)
}
fn chat(id: &str) -> Value {
    json!({"model":"test","stream":true,"messages":[{"role":"user","content":id}]})
}
async fn submit(state: &AppState, input: Value) -> Response {
    router(state.clone())
        .oneshot(
            Request::post("/v1/chat/completions")
                .header(header::AUTHORIZATION, format!("Bearer {}", key(1)))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(input.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}
async fn get(state: &AppState, path: &str, auth: bool) -> Response {
    let mut request = Request::get(path);
    if auth {
        request = request.header(header::AUTHORIZATION, format!("Bearer {}", key(1)));
    }
    router(state.clone())
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}
async fn text(response: Response) -> String {
    let bytes = timeout(Duration::from_secs(5), response.into_body().collect())
        .await
        .unwrap()
        .unwrap()
        .to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}
async fn event(events: &mut mpsc::UnboundedReceiver<Event>, stage: &str) -> Event {
    let event = timeout(Duration::from_secs(5), events.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(event.stage, stage);
    event
}
#[test]
fn shared_keys_exact_grammar_rotation_and_configuration_bounds() {
    let keys = keys();
    for n in [1, 2, 3] {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {}", key(n)).parse().unwrap(),
        );
        assert_eq!(keys.accepts(&headers), n != 3);
        headers.append(
            header::AUTHORIZATION,
            format!("Bearer {}", key(1)).parse().unwrap(),
        );
        assert!(!keys.accepts(&headers));
    }
    for value in [
        "bearer ".to_owned() + &key(1),
        "Bearer ".to_owned() + &key(1) + " ",
        "Bearer x".into(),
        "Bearer ".to_owned() + &key(1) + "," + &key(2),
    ] {
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, value.parse().unwrap());
        assert!(!keys.accepts(&headers));
    }
    for config in [
        "[]".into(),
        "[\"short\"]".into(),
        json!([key(1), key(1)]).to_string(),
        json!(vec![key(1); 9]).to_string(),
        "x".repeat(1025),
    ] {
        assert!(SharedKeys::from_json(&config).is_err());
    }
}

#[tokio::test]
async fn info_public_index_and_routes_are_content_free_without_upstream_calls() {
    let (state, mock, _events) = fixture(INDEX, false);
    assert_eq!(INDEX.as_bytes(), b"speak like a possum girl\n");
    let root = text(get(&state, "/", false).await).await;
    let alias = text(get(&state, "/index.html", false).await).await;
    assert_eq!(root, alias);
    assert!(root.starts_with(&format!("{INDEX}\n{{\n  \"live_connections\"")));
    let public: Value =
        serde_json::from_str(root.strip_prefix(&format!("{INDEX}\n")).unwrap()).unwrap();
    let info = text(get(&state, "/v1/info", true).await).await;
    assert_eq!(serde_json::from_str::<Value>(&info).unwrap(), public);
    assert_eq!(public["live_connections"], 1);
    assert_eq!(public["limit_period"], "run");
    assert_eq!(public["limit_usd"], "10.000000");
    assert_eq!(public["spent_usd"], "0.000000");
    assert_eq!(public.as_object().unwrap().len(), 6);
    for sentinel in [
        key(1),
        "synthetic".into(),
        "account".into(),
        "request_id".into(),
        "ip".into(),
    ] {
        assert!(!info.contains(&sentinel));
    }
    assert_eq!(
        get(&state, "/v1/info", false).await.status(),
        StatusCode::UNAUTHORIZED
    );
    for path in [
        "/v1/balance",
        "/v1/sessions",
        "/v1/auth/challenge",
        "/login",
        "/recovery",
        "/chat",
        "/v1/info?key=private",
    ] {
        assert!(!get(&state, path, true).await.status().is_success());
    }
    assert_eq!(mock.tokenizers.load(Ordering::SeqCst), 0);
    assert_eq!(mock.generations.load(Ordering::SeqCst), 0);
    assert!(super::info::source("unavailable").is_none());
    assert!(super::info::source(&"0".repeat(40)).is_none());
    assert_eq!(
        super::info::source(&"a".repeat(40)).unwrap(),
        format!(
            "https://github.com/ajbt200128/possums/tree/{}",
            "a".repeat(40)
        )
    );
}

#[tokio::test]
async fn prefix_is_exact_once_at_head_and_shared_by_tokenizer_generation_including_empty_source() {
    for original in [INDEX, "", "private fixture\n\n"] {
        let (state, mock, mut events) = fixture(original, true);
        let input = json!({"model":"test","stream":true,"messages":[{"role":"system","content":"caller system"},{"role":"assistant","content":"old"},{"role":"user","content":"new"}]});
        let response = submit(&state, input.clone()).await;
        let count = event(&mut events, "tokenizer").await;
        let prefix = count.input["messages"][0]["content"].as_str().unwrap();
        assert!(prefix.starts_with(&format!("{original}\n{{\n")));
        let snapshot: Value =
            serde_json::from_str(prefix.strip_prefix(&format!("{original}\n")).unwrap()).unwrap();
        assert_eq!(snapshot["live_connections"], 1);
        assert_eq!(
            &count.input["messages"].as_array().unwrap()[1..],
            input["messages"].as_array().unwrap()
        );
        // Change live count between tokenizer and generator. Prefix must NOT change.
        let extra = get(&state, "/v1/info", true).await;
        mock.count_gate.add_permits(1);
        let generation = event(&mut events, "generation").await;
        assert_eq!(generation.input, count.input);
        assert_eq!(generation.allowance, 9_999_990);
        mock.generation_gate.add_permits(1);
        assert!(text(response).await.contains("[DONE]"));
        drop(extra);
    }
}

#[tokio::test]
async fn tools_and_history_preserved_after_augmented_system_context() {
    let (state, _, mut events) = fixture(INDEX, false);
    let input = json!({"model":"test","stream":true,"tools":[{"type":"function","function":{"name":"read","parameters":{"type":"object"}}}],"tool_choice":"auto",
        "messages":[{"role":"system","content":"caller"},{"role":"user","content":"before"},
        {"role":"assistant","content":null,"tool_calls":[{"id":"a","type":"function","function":{"name":"read","arguments":"{\"file\":\"test\"}"}}]},
        {"role":"tool","tool_call_id":"a","content":"tool result"}]});
    assert!(text(submit(&state, input.clone()).await)
        .await
        .contains("[DONE]"));
    let count = event(&mut events, "tokenizer").await;
    let generation = event(&mut events, "generation").await;
    assert_eq!(count.input, generation.input);
    assert_eq!(count.input["tools"], input["tools"]);
    assert_eq!(count.input["tool_choice"], input["tool_choice"]);
    assert_eq!(
        &count.input["messages"].as_array().unwrap()[1..],
        input["messages"].as_array().unwrap()
    );
}

#[tokio::test]
async fn fifo_more_than_four_raw_positions_single_flight_through_disconnect() {
    let (state, mock, mut events) = fixture(INDEX, true);
    let first = submit(&state, chat("0")).await;
    event(&mut events, "tokenizer").await;
    let mut queued = Vec::new();
    for i in 1..9 {
        let response = submit(&state, chat(&i.to_string())).await;
        assert_eq!(response.status(), StatusCode::OK);
        queued.push(response);
    }
    assert_eq!(mock.tokenizers.load(Ordering::SeqCst), 1);
    // Invalid JSON is not decoded at admission, even with >4 held positions.
    let invalid = submit(&state, json!({"private_hostile_field":"secret-marker"})).await;
    assert_eq!(invalid.status(), StatusCode::OK);
    drop(first); // dispatched tokenizer owns the lane despite body cancellation
    let info: Value =
        serde_json::from_str(&text(get(&state, "/v1/info", true).await).await).unwrap();
    assert_eq!(info["live_connections"], 10); // nine queued + this request; detached first excluded
    mock.count_gate.add_permits(1);
    event(&mut events, "generation").await;
    assert_eq!(mock.tokenizers.load(Ordering::SeqCst), 1);
    mock.generation_gate.add_permits(1);
    for (i, response) in queued.into_iter().enumerate() {
        let count = event(&mut events, "tokenizer").await;
        assert_eq!(count.input["messages"][1]["content"], (i + 1).to_string());
        mock.count_gate.add_permits(1);
        event(&mut events, "generation").await;
        mock.generation_gate.add_permits(1);
        assert!(text(response).await.contains("[DONE]"));
    }
    let error = text(invalid).await;
    assert!(error.contains("invalid_chat"));
    assert!(!error.contains("secret-marker"));
    assert_eq!(mock.high.load(Ordering::SeqCst), 1);
    assert_eq!(mock.generations.load(Ordering::SeqCst), 9);
}

#[tokio::test]
async fn queued_drop_skips_inference_and_active_delivery_drop_does_not_replay() {
    let (state, mock, mut events) = fixture(INDEX, true);
    let first = submit(&state, chat("first")).await;
    event(&mut events, "tokenizer").await;
    let cancelled = submit(&state, chat("cancelled")).await;
    drop(cancelled);
    let last = submit(&state, chat("last")).await;
    mock.count_gate.add_permits(1);
    event(&mut events, "generation").await;
    drop(first);
    assert_eq!(mock.active.load(Ordering::SeqCst), 1);
    mock.generation_gate.add_permits(1);
    let count = event(&mut events, "tokenizer").await;
    assert_eq!(count.input["messages"][1]["content"], "last");
    mock.count_gate.add_permits(1);
    event(&mut events, "generation").await;
    mock.generation_gate.add_permits(1);
    assert!(text(last).await.contains("[DONE]"));
    assert_eq!(mock.generations.load(Ordering::SeqCst), 2);
    assert_eq!(mock.high.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn cancelled_full_queue_releases_raw_bodies_and_all_credits_before_active_finishes() {
    for expire in [false, true] {
        let (state, mock, mut events) = fixture(INDEX, true);
        let first = submit(&state, chat("active")).await;
        event(&mut events, "tokenizer").await;
        mock.count_gate.add_permits(1);
        event(&mut events, "generation").await;
        // The first upstream remains gated throughout capacity and reconnect checks.
        for _ in 0..3 {
            let mut queued = Vec::new();
            for _ in 1..CONNECTIONS {
                let response = submit(&state, chat("cancelled")).await;
                assert_eq!(response.status(), StatusCode::OK);
                queued.push(response);
            }
            let (entries, bytes) = state.queue.occupancy();
            assert_eq!(entries, CONNECTIONS - 1);
            assert!(bytes > 0);
            assert_eq!(state.connections.available_permits(), 0);
            assert_eq!(
                get(&state, "/v1/info", true).await.status(),
                StatusCode::SERVICE_UNAVAILABLE
            );
            if expire {
                tokio::time::advance(LIFETIME).await;
                for response in queued {
                    assert!(text(response).await.is_empty());
                }
            } else {
                drop(queued);
            }
            assert_eq!(state.queue.occupancy(), (0, 0));
            assert_eq!(state.connections.available_permits(), CONNECTIONS - 1);
            let info = get(&state, "/v1/info", true).await;
            assert_eq!(info.status(), StatusCode::OK);
            drop(info);
            let replacement = submit(&state, chat("replacement")).await;
            assert_eq!(replacement.status(), StatusCode::OK);
            assert_eq!(state.queue.occupancy().0, 1);
            drop(replacement);
            assert_eq!(state.queue.occupancy(), (0, 0));
            assert_eq!(mock.active.load(Ordering::SeqCst), 1);
            assert_eq!(mock.tokenizers.load(Ordering::SeqCst), 1);
        }
        let survivor = submit(&state, chat("survivor")).await;
        mock.generation_gate.add_permits(1);
        let count = event(&mut events, "tokenizer").await;
        assert_eq!(count.input["messages"][1]["content"], "survivor");
        mock.count_gate.add_permits(1);
        event(&mut events, "generation").await;
        mock.generation_gate.add_permits(1);
        assert!(text(survivor).await.contains("[DONE]"));
        drop(first);
        assert_eq!(mock.generations.load(Ordering::SeqCst), 2);
        assert_eq!(mock.high.load(Ordering::SeqCst), 1);
        assert_eq!(state.connections.available_permits(), CONNECTIONS);
    }
}

struct GatedEvidence {
    entered: tokio::sync::Notify,
    gate: Semaphore,
}
#[async_trait]
impl EvidenceVerifier for GatedEvidence {
    async fn verify(&self, path: &str, now: u64) -> Result<GatewayEvidence, EvidenceError> {
        self.entered.notify_one();
        self.gate.acquire().await.unwrap().forget();
        Evidence.verify(path, now).await
    }
}

#[tokio::test]
async fn predispatch_evidence_and_catalog_cancellation_releases_payload_without_tokenizing() {
    for evidence in [false, true] {
        let verifier = Arc::new(GatedEvidence {
            entered: tokio::sync::Notify::new(),
            gate: Semaphore::new(if evidence { 0 } else { 100 }),
        });
        let (state, mock, mut events) = fixture_with_evidence(INDEX, false, verifier.clone());
        if !evidence {
            mock.catalog_gate.acquire_many(100).await.unwrap().forget();
        }
        let entered = if evidence {
            &verifier.entered
        } else {
            &mock.catalog_entered
        };
        let cancelled = submit(&state, chat("cancelled preflight")).await;
        timeout(Duration::from_secs(5), entered.notified())
            .await
            .unwrap();
        assert_eq!(state.queue.occupancy().0, 1);
        drop(cancelled);
        assert_eq!(state.queue.occupancy(), (0, 0));
        assert_eq!(state.connections.available_permits(), CONNECTIONS);
        let next = submit(&state, chat("live preflight")).await;
        // The cancelled preflight is abandoned without releasing its gate first.
        timeout(Duration::from_secs(5), entered.notified())
            .await
            .unwrap();
        assert_eq!(mock.tokenizers.load(Ordering::SeqCst), 0);
        if evidence {
            verifier.gate.add_permits(1);
        } else {
            mock.catalog_gate.add_permits(1);
        }
        assert!(text(next).await.contains("[DONE]"));
        let count = event(&mut events, "tokenizer").await;
        assert_eq!(count.input["messages"][1]["content"], "live preflight");
        event(&mut events, "generation").await;
        assert_eq!(mock.tokenizers.load(Ordering::SeqCst), 1);
        assert_eq!(mock.generations.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn soft_budget_overshoots_once_then_closes_using_final_usage_without_markup() {
    let (state, mock, _events) = fixture(INDEX, false);
    *mock.usage.lock().unwrap() = StreamUsage {
        input_tokens: 10,
        output_tokens: 9_989_990,
        total_tokens: 9_990_000,
    };
    let first = text(submit(&state, chat("first")).await).await;
    assert!(first.contains("\"upstream_cost_microusd\":\"9990000\""));
    *mock.usage.lock().unwrap() = StreamUsage {
        input_tokens: 20,
        output_tokens: 6_499_980,
        total_tokens: 6_500_000,
    };
    let second = text(submit(&state, chat("second")).await).await;
    assert!(second.contains("\"upstream_cost_microusd\":\"6500000\""));
    let info: Value =
        serde_json::from_str(&text(get(&state, "/v1/info", true).await).await).unwrap();
    assert_eq!(info["spent_usd"], "16.490000");
    let third = text(submit(&state, chat("third")).await).await;
    assert!(third.contains("budget_exhausted"));
    assert_eq!(mock.generations.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn uncertain_generation_tokenizer_and_panic_block_future_calls() {
    for scenario in 0..6 {
        let (state, mock, _events) = fixture(INDEX, false);
        match scenario {
            0 => {
                *mock.generation_error.lock().unwrap() = Some(InferenceFailure::StreamUsageMissing)
            }
            1 => *mock.count_error.lock().unwrap() = Some(InferenceFailure::TokenizerSendFailed),
            2 | 5 => mock.panic.store(true, Ordering::SeqCst),
            4 => mock.count_panic.store(true, Ordering::SeqCst),
            _ => {
                *mock.usage.lock().unwrap() = StreamUsage {
                    input_tokens: u64::MAX,
                    output_tokens: 1,
                    total_tokens: 0,
                }
            }
        }
        let mut input = chat("private-sentinel");
        if scenario == 5 {
            input["tools"] = json!([{"type":"function","function":{"name":"read","parameters":{"type":"object"}}}]);
        }
        let first = text(submit(&state, input).await).await;
        assert!(!first.contains("[DONE]"));
        assert!(!first.contains("private-sentinel"));
        assert!(!first.contains("synthetic-private-panic-marker"));
        assert!(first.contains("\"billing\":\"unknown\""));
        assert!(!first.contains("\"outcome\":\"settled\""));
        if matches!(scenario, 2 | 4 | 5) {
            let diagnostic: Value = first
                .lines()
                .filter_map(|line| line.strip_prefix("data: "))
                .filter_map(|json| serde_json::from_str::<Value>(json).ok())
                .find_map(|event| event.get("error").cloned())
                .expect("connected client receives a panic diagnostic");
            assert_eq!(diagnostic["detail"], "worker_failed");
            assert_eq!(diagnostic["stage"], "worker");
            assert_eq!(diagnostic["billing"], "unknown");
            assert_eq!(diagnostic["replayed"], false);
        }
        let second = text(submit(&state, chat("second")).await).await;
        assert!(second.contains("budget_unknown"));
        assert_eq!(mock.tokenizers.load(Ordering::SeqCst), 1);
        assert_eq!(
            mock.generations.load(Ordering::SeqCst),
            usize::from(!matches!(scenario, 1 | 4))
        );
    }
}

#[tokio::test]
async fn definite_predispatch_failure_and_catalog_failure_do_not_spend_or_latch() {
    let (state, mock, _events) = fixture(INDEX, false);
    mock.catalog_error.store(true, Ordering::SeqCst);
    assert!(text(submit(&state, chat("a")).await)
        .await
        .contains("catalog_failed"));
    assert_eq!(mock.tokenizers.load(Ordering::SeqCst), 0);
    mock.catalog_error.store(false, Ordering::SeqCst);
    *mock.count_error.lock().unwrap() = Some(InferenceFailure::RequestEncodingFailed);
    assert!(text(submit(&state, chat("a")).await)
        .await
        .contains("request_encoding_failed"));
    *mock.count_error.lock().unwrap() = None;
    assert!(text(submit(&state, chat("b")).await)
        .await
        .contains("[DONE]"));
}

#[tokio::test]
async fn slow_delivery_remains_bounded_and_settles_before_next_inference() {
    let (state, mock, _events) = fixture(INDEX, false);
    mock.flood.store(true, Ordering::SeqCst);
    let response = submit(&state, chat("slow")).await;
    // Poll worker completion via its subsequent FIFO submission, not a timer.
    let next = submit(&state, json!({"invalid":true})).await;
    assert!(text(next).await.contains("invalid_chat"));
    let output = text(response).await;
    assert!(output.len() <= 64 * 1024);
    assert!(!output.contains("[DONE]"));
    let info: Value =
        serde_json::from_str(&text(get(&state, "/v1/info", true).await).await).unwrap();
    assert_eq!(info["spent_usd"], "0.000015");
}

#[test]
fn budget_equality_rounding_usage_and_arithmetic_fail_closed() {
    use super::budget::{Budget, THRESHOLD};
    let mut model = Model {
        id: "test".into(),
        context_tokens: u64::MAX,
        max_output_tokens: u64::MAX,
        input_microunits_per_million_tokens: 1_000_000,
        output_microunits_per_million_tokens: 1_000_000,
    };
    let mut budget = Budget {
        settled: THRESHOLD - 1,
        blocked: false,
    };
    assert!(budget.admits());
    assert_eq!(
        budget.settle(
            &model,
            StreamUsage {
                input_tokens: 1,
                output_tokens: 0,
                total_tokens: 1
            }
        ),
        Ok(1)
    );
    assert!(!budget.admits());
    let mut budget = Budget::default();
    model.input_microunits_per_million_tokens = 1;
    assert_eq!(
        budget.settle(
            &model,
            StreamUsage {
                input_tokens: 1,
                output_tokens: 0,
                total_tokens: 1
            }
        ),
        Ok(1)
    );
    model.input_microunits_per_million_tokens = u64::MAX;
    assert!(budget
        .settle(
            &model,
            StreamUsage {
                input_tokens: u64::MAX,
                output_tokens: 0,
                total_tokens: u64::MAX
            }
        )
        .is_err());
    assert!(budget.blocked);
    let mut budget = Budget {
        settled: u64::MAX,
        blocked: false,
    };
    model.input_microunits_per_million_tokens = 1_000_000;
    assert!(budget
        .settle(
            &model,
            StreamUsage {
                input_tokens: 1,
                output_tokens: 0,
                total_tokens: 1
            }
        )
        .is_err());
    assert!(budget.blocked);
}

#[tokio::test(start_paused = true)]
async fn queued_twelve_hour_expiry_does_not_dispatch_but_started_owner_finishes() {
    let (state, mock, mut events) = fixture(INDEX, true);
    let first = submit(&state, chat("started")).await;
    event(&mut events, "tokenizer").await;
    let expired = submit(&state, chat("expired")).await;
    tokio::time::advance(LIFETIME).await;
    assert!(text(expired).await.is_empty());
    assert!(text(first).await.is_empty());
    let next = submit(&state, chat("fresh")).await;
    mock.count_gate.add_permits(1);
    event(&mut events, "generation").await;
    mock.generation_gate.add_permits(1);
    let count = event(&mut events, "tokenizer").await;
    assert_eq!(count.input["messages"][1]["content"], "fresh");
    mock.count_gate.add_permits(1);
    event(&mut events, "generation").await;
    mock.generation_gate.add_permits(1);
    assert!(text(next).await.contains("[DONE]"));
    assert_eq!(mock.tokenizers.load(Ordering::SeqCst), 2);
}

#[tokio::test(start_paused = true)]
async fn upload_timeout_and_header_body_parser_bounds_are_closed() {
    let (state, mock, _events) = fixture(INDEX, false);
    let request = |body: Body| {
        Request::post("/v1/chat/completions")
            .header(header::AUTHORIZATION, format!("Bearer {}", key(1)))
            .header(header::CONTENT_TYPE, "application/json")
            .body(body)
            .unwrap()
    };
    let pending =
        Body::from_stream(futures_util::stream::pending::<Result<Bytes, std::io::Error>>());
    let before = Instant::now();
    let response = router(state.clone())
        .oneshot(request(pending))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
    assert_eq!(Instant::now() - before, Duration::from_secs(30));
    let response = router(state.clone())
        .oneshot(request(Body::from(vec![
            b'x';
            crate::server::BODY_LIMIT + 1
        ])))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let mut oversized = request(Body::empty());
    oversized
        .headers_mut()
        .insert("x-synthetic", "x".repeat(32 * 1024).parse().unwrap());
    assert_eq!(
        router(state.clone())
            .oneshot(oversized)
            .await
            .unwrap()
            .status(),
        StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE
    );
    let mut headers = request(Body::empty());
    for i in 0..65 {
        headers.headers_mut().insert(
            format!("x-synthetic-{i}")
                .parse::<header::HeaderName>()
                .unwrap(),
            "x".parse().unwrap(),
        );
    }
    assert_eq!(
        router(state.clone())
            .oneshot(headers)
            .await
            .unwrap()
            .status(),
        StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE
    );
    for raw in [
        r#"{"model":"private","model":"test","stream":true,"messages":[]}"#.to_owned(),
        format!("{}0{}", "[".repeat(200), "]".repeat(200)),
    ] {
        let response = router(state.clone())
            .oneshot(request(Body::from(raw)))
            .await
            .unwrap();
        let error = text(response).await;
        assert!(error.contains("invalid_chat"));
        assert!(!error.contains("private"));
    }
    for name in [header::COOKIE, header::CONTENT_ENCODING] {
        let mut invalid = request(Body::empty());
        invalid
            .headers_mut()
            .insert(name, "private-sentinel".parse().unwrap());
        let response = router(state.clone()).oneshot(invalid).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(!text(response).await.contains("private-sentinel"));
    }
    assert_eq!(mock.tokenizers.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn sixty_four_resource_owners_bound_requests_without_four_lane_admission() {
    let (state, _, _events) = fixture(INDEX, false);
    let mut held = Vec::new();
    for _ in 0..64 {
        let response = get(&state, "/v1/info", true).await;
        assert_eq!(response.status(), StatusCode::OK);
        held.push(response);
    }
    assert_eq!(
        get(&state, "/v1/info", true).await.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    drop(held.pop());
    assert_eq!(get(&state, "/v1/info", true).await.status(), StatusCode::OK);
}

#[tokio::test]
async fn backend_http1_closes_after_one_response_and_counts_only_live_sockets() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (state, mock, mut events) = fixture(INDEX, true);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(serve(listener, state.clone()));
    let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
    socket
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: keep-alive\r\n\r\n")
        .await
        .unwrap();
    let mut wire = Vec::new();
    timeout(Duration::from_secs(5), socket.read_to_end(&mut wire))
        .await
        .unwrap()
        .unwrap();
    let wire = String::from_utf8(wire).unwrap();
    assert!(wire.to_ascii_lowercase().contains("connection: close\r\n"));
    assert!(wire.contains("speak like a possum girl\n\n"));
    let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
    let body = chat("socket").to_string();
    socket.write_all(format!("POST /v1/chat/completions HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",key(1),body.len(),body).as_bytes()).await.unwrap();
    event(&mut events, "tokenizer").await;
    let mut queued = tokio::net::TcpStream::connect(address).await.unwrap();
    let body = chat("queued-socket").to_string();
    queued.write_all(format!("POST /v1/chat/completions HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",key(1),body.len(),body).as_bytes()).await.unwrap();
    let mut byte = [0];
    timeout(Duration::from_secs(5), queued.read_exact(&mut byte))
        .await
        .unwrap()
        .unwrap();
    drop(queued);
    drop(socket);
    // The HTTP task sees the closed transport; detached tokenization remains held.
    timeout(Duration::from_secs(5), async {
        loop {
            let info: Value =
                serde_json::from_slice(&state.core.info.snapshot().bytes().ok().unwrap()).unwrap();
            if info["live_connections"] == 0 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(mock.active.load(Ordering::SeqCst), 1);
    mock.count_gate.add_permits(1);
    event(&mut events, "generation").await;
    mock.generation_gate.add_permits(1);
    let barrier = submit(&state, json!({"invalid":true})).await;
    assert!(text(barrier).await.contains("invalid_chat"));
    assert_eq!(mock.generations.load(Ordering::SeqCst), 1);
    assert_eq!(mock.tokenizers.load(Ordering::SeqCst), 1);
    server.abort();
}

#[tokio::test]
async fn admission_order_is_upload_completion_not_connection_arrival() {
    let (state, mock, mut events) = fixture(INDEX, true);
    let (sender, receiver) = mpsc::channel::<Result<Bytes, std::io::Error>>(1);
    let polled = Arc::new(tokio::sync::Notify::new());
    let seen = polled.clone();
    let stream = futures_util::stream::unfold(receiver, move |mut receiver| {
        let seen = seen.clone();
        async move {
            let bytes = receiver.recv().await?;
            seen.notify_one();
            Some((bytes, receiver))
        }
    });
    let request = Request::post("/v1/chat/completions")
        .header(header::AUTHORIZATION, format!("Bearer {}", key(1)))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from_stream(stream))
        .unwrap();
    let app = router(state.clone());
    let upload = tokio::spawn(async move { app.oneshot(request).await.unwrap() });
    sender
        .send(Ok(Bytes::from(chat("slow upload").to_string())))
        .await
        .unwrap();
    polled.notified().await; // Complete bytes but no upload EOF yet: not queued.
    let fast = submit(&state, chat("fast upload")).await;
    let count = event(&mut events, "tokenizer").await;
    assert_eq!(count.input["messages"][1]["content"], "fast upload");
    drop(sender);
    let slow = upload.await.unwrap();
    mock.count_gate.add_permits(1);
    event(&mut events, "generation").await;
    mock.generation_gate.add_permits(1);
    assert!(text(fast).await.contains("[DONE]"));
    let count = event(&mut events, "tokenizer").await;
    assert_eq!(count.input["messages"][1]["content"], "slow upload");
    mock.count_gate.add_permits(1);
    event(&mut events, "generation").await;
    mock.generation_gate.add_permits(1);
    assert!(text(slow).await.contains("[DONE]"));
}

#[tokio::test]
async fn retained_frames_hold_resource_credit_but_not_live_connection_count() {
    let (state, _, _events) = fixture(INDEX, false);
    let mut body = get(&state, "/", false).await.into_body();
    let frame = body.frame().await.unwrap().unwrap();
    drop(body);
    assert_eq!(state.connections.available_permits(), 63);
    let snapshot = serde_json::to_value(state.core.info.snapshot()).unwrap();
    assert_eq!(snapshot["live_connections"], 0);
    drop(frame);
    assert_eq!(state.connections.available_permits(), 64);
}
