use async_trait::async_trait;
use axum::{
    body::Body,
    http::{header, Request, StatusCode},
    response::Response,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use futures_util::StreamExt;
use http_body_util::BodyExt;
use possums::{
    attestation::{EvidenceError, EvidenceVerifier, GatewayEvidence},
    auth::Auth,
    catalog::Model,
    inference::{
        stream::{ProtocolParser, SdkToolValidator, StreamCompletion},
        tools::{CompletionDelta, ToolInvocation, ToolProfile},
        Inference, InferenceError, InferenceFailure, Message,
    },
    server::{router, AppState, BODY_LIMIT},
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::sync::Notify;
use tower::ServiceExt;

struct Provider {
    qualified: AtomicBool,
    revoke_qualification: AtomicBool,
    qualification_checks: AtomicUsize,
    invocation: Mutex<Option<Value>>,
    tokens: AtomicU64,
    tokenizer_calls: AtomicUsize,
    generation_calls: AtomicUsize,
    output_allowance: AtomicU64,
    price: AtomicU64,
    valid_catalog: AtomicBool,
    catalog_has_m: AtomicBool,
    valid_evidence: AtomicBool,
    upstream_error: AtomicBool,
    invalid_usage: AtomicBool,
    missing_final_usage: AtomicBool,
    overflow_delivery: AtomicBool,
    hold_tokenizer: AtomicBool,
    tokenizer_error: AtomicBool,
    invalid_receipt: AtomicBool,
    panic_generation: AtomicBool,
    tokenizer_started: Notify,
    tokenizer_release: Notify,
    started: Notify,
    finish: Notify,
    completed: Notify,
}
#[async_trait]
impl EvidenceVerifier for Provider {
    async fn verify(&self, _: &str, now: u64) -> Result<GatewayEvidence, EvidenceError> {
        if !self.valid_evidence.load(Ordering::SeqCst) {
            return Err(EvidenceError::Invalid);
        }
        Ok(GatewayEvidence {
            quote: json!({}),
            issued_at_unix: now,
            release_digest: String::new(),
            endpoint_key_sha256: String::new(),
            freshness_expires_at_unix: now + 60,
        })
    }
}
#[async_trait]
impl Inference for Provider {
    async fn catalog(&self) -> Result<Vec<u8>, InferenceError> {
        if !self.valid_catalog.load(Ordering::SeqCst) {
            return Err(InferenceError::Unavailable);
        }
        let price = self.price.load(Ordering::SeqCst);
        let id = if self.catalog_has_m.load(Ordering::SeqCst) {
            "m"
        } else {
            "other"
        };
        Ok(json!({"object":"list","data":[{"id":id,"type":"chat","context_window":20,"endpoints":["/v1/chat/completions"],"pricing":{"inputTokenPricePer1M":price,"outputTokenPricePer1M":price,"requestPrice":0}}]}).to_string().into_bytes())
    }
    async fn count_tokens(
        &self,
        _: &str,
        _: &[Message],
        _: Arc<possums::telemetry::hooks::Lease>,
    ) -> Result<u64, InferenceError> {
        self.tokenizer_calls.fetch_add(1, Ordering::SeqCst);
        self.tokenizer_started.notify_one();
        if self.hold_tokenizer.load(Ordering::SeqCst) {
            self.tokenizer_release.notified().await;
        }
        if self.tokenizer_error.load(Ordering::SeqCst) {
            return Err(InferenceFailure::TokenizerResponseInvalid.into());
        }
        Ok(self.tokens.load(Ordering::SeqCst))
    }
    async fn generate_completion_stream(
        &self,
        model: &Model,
        _: &[Message],
        _: Arc<possums::telemetry::hooks::Lease>,
        on_delta: &mut (dyn for<'d> FnMut(&'d str) + Send),
    ) -> Result<StreamCompletion, InferenceError> {
        self.generation_calls.fetch_add(1, Ordering::SeqCst);
        if self.panic_generation.load(Ordering::SeqCst) {
            panic!("synthetic generation panic; no request content");
        }
        self.output_allowance
            .store(model.max_output_tokens, Ordering::SeqCst);
        let mut parser = ProtocolParser::default();
        let content = "<script>synthetic</script>🐾";
        let event = format!(
            "data: {}\n\n",
            json!({"choices":[{"index":0,"delta":{"content":content},"finish_reason":null}]})
        );
        let repeats = if self.overflow_delivery.load(Ordering::SeqCst) {
            100
        } else {
            1
        };
        for _ in 0..repeats {
            parser
                .feed(event.as_bytes(), &mut *on_delta)
                .map_err(|_| InferenceError::InvalidResponse)?;
        }
        self.started.notify_one();
        self.finish.notified().await;
        let result = if self.invalid_receipt.load(Ordering::SeqCst) {
            Ok(StreamCompletion {
                usage: possums::inference::stream::StreamUsage {
                    input_tokens: 2,
                    output_tokens: 3,
                    total_tokens: 999,
                },
                finish_reason: possums::inference::stream::FinishReason::Stop,
            })
        } else if self.upstream_error.load(Ordering::SeqCst) {
            Err(InferenceFailure::StreamTransportFailed.into())
        } else if self.invalid_usage.load(Ordering::SeqCst) {
            Err(InferenceError::InvalidResponse)
        } else {
            parser.feed(b"data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"length\"}]}\n\ndata: {\"choices\":[],\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":3,\"total_tokens\":5}}\n\ndata: [DONE]\n\n", &mut *on_delta).map_err(|_| InferenceError::InvalidResponse)?;
            parser
                .eof_completion()
                .map_err(|_| InferenceError::InvalidResponse)
        };
        self.completed.notify_one();
        result
    }
    fn tool_profile(&self, model: &str) -> Option<ToolProfile> {
        let check = self.qualification_checks.fetch_add(1, Ordering::SeqCst);
        (model == "m"
            && self.qualified.load(Ordering::SeqCst)
            && !(check > 0 && self.revoke_qualification.load(Ordering::SeqCst)))
        .then_some(ToolProfile::OpenAiFunctionsV1)
    }
    async fn count_invocation_tokens(
        &self,
        model: &str,
        invocation: &ToolInvocation,
        heavy: Arc<possums::telemetry::hooks::Lease>,
    ) -> Result<u64, InferenceError> {
        *self.invocation.lock().unwrap() = Some(serde_json::to_value(invocation).unwrap());
        self.count_tokens(model, &[], heavy).await
    }
    async fn generate_invocation_stream(
        &self,
        model: &Model,
        invocation: &ToolInvocation,
        _: Arc<possums::telemetry::hooks::Lease>,
        on_delta: &mut (dyn for<'d> FnMut(CompletionDelta<'d>) + Send),
    ) -> Result<StreamCompletion, InferenceError> {
        assert_eq!(
            self.invocation.lock().unwrap().as_ref(),
            Some(&serde_json::to_value(invocation).unwrap())
        );
        self.generation_calls.fetch_add(1, Ordering::SeqCst);
        self.output_allowance
            .store(model.max_output_tokens, Ordering::SeqCst);
        let mut validator = SdkToolValidator::default();
        // Controllable HTTP-byte fixture through the production SDK decoder.
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Result<Vec<u8>, std::io::Error>>();
        let bytes = futures_util::stream::unfold(rx, |mut rx| async {
            rx.recv().await.map(|b| (b.map(Into::into), rx))
        });
        let mut events = tinfoil::sse::parse_event_stream(bytes);
        let arguments = json!({"q":"🐾\n".repeat(260)}).to_string();
        let event = format!(
            "data: {}\n\n",
            json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"generated_0","type":"function","function":{"name":"lookup","arguments":arguments}}]},"finish_reason":null}],
                "usage":{"prompt_tokens":2,"completion_tokens":1,"total_tokens":3}})
        );
        tx.send(Ok(event.into_bytes())).unwrap();
        validator
            .accept(
                events
                    .next()
                    .await
                    .unwrap()
                    .map_err(|_| InferenceError::InvalidResponse)?,
                &mut *on_delta,
            )
            .map_err(|_| InferenceError::InvalidResponse)?;
        if self.overflow_delivery.load(Ordering::SeqCst) {
            let event = format!(
                "data: {}\n\n",
                json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":" ".repeat(256)}}]},"finish_reason":null}]})
            );
            for _ in 0..100 {
                tx.send(Ok(event.as_bytes().to_vec())).unwrap();
                validator
                    .accept(
                        events
                            .next()
                            .await
                            .unwrap()
                            .map_err(|_| InferenceError::InvalidResponse)?,
                        &mut *on_delta,
                    )
                    .map_err(|_| InferenceError::InvalidResponse)?;
            }
        }
        self.started.notify_one();
        self.finish.notified().await;
        if self.upstream_error.load(Ordering::SeqCst) {
            tx.send(Err(std::io::Error::other("fixture transport failure")))
                .unwrap();
        } else {
            tx.send(Ok(format!(
                "data: {}\n\n",
                json!({"choices":[{"index":0,"delta":{},"finish_reason":"stop"}],
                "usage":{"prompt_tokens":2,"completion_tokens":2,"total_tokens":4}})
            )
            .into_bytes()))
                .unwrap();
            if !self.missing_final_usage.load(Ordering::SeqCst) {
                let total = if self.invalid_usage.load(Ordering::SeqCst) {
                    6
                } else {
                    5
                };
                tx.send(Ok(format!("data: {}\n\n", json!({"choices":[],"usage":{"prompt_tokens":2,"completion_tokens":3,"total_tokens":total}})).into_bytes())).unwrap();
            }
            tx.send(Ok(b"data: [DONE]\n\n".to_vec())).unwrap();
        }
        drop(tx);
        let result = async {
            while let Some(event) = events.next().await {
                validator
                    .accept(
                        event.map_err(|_| InferenceError::InvalidResponse)?,
                        &mut *on_delta,
                    )
                    .map_err(|_| InferenceError::InvalidResponse)?;
            }
            validator
                .eof_completion()
                .map_err(|_| InferenceError::InvalidResponse)
        }
        .await;
        self.completed.notify_one();
        result
    }
    fn verification_document(&self) -> Result<Value, InferenceError> {
        Err(InferenceError::Unavailable)
    }
}
fn fixture(credit: u64) -> (AppState, Arc<Provider>, String, String) {
    let credential = URL_SAFE_NO_PAD.encode([8; 32]);
    let auth=Auth::from_json(&json!([{"id":"demo","credential_sha256":URL_SAFE_NO_PAD.encode(Sha256::digest(credential.as_bytes())),"demo_microunits":credit}]).to_string()).unwrap();
    let (bearer, _) = auth
        .authenticate_api(&credential, &auth.issue_api_challenge().unwrap())
        .unwrap();
    let submission = auth.issue_api_submission(&bearer, "m", false).unwrap();
    let provider = Arc::new(Provider {
        qualified: false.into(),
        revoke_qualification: false.into(),
        qualification_checks: 0.into(),
        invocation: Mutex::new(None),
        tokens: 2.into(),
        tokenizer_calls: 0.into(),
        generation_calls: 0.into(),
        output_allowance: 0.into(),
        price: 1.into(),
        valid_catalog: true.into(),
        catalog_has_m: true.into(),
        valid_evidence: true.into(),
        upstream_error: false.into(),
        invalid_usage: false.into(),
        missing_final_usage: false.into(),
        overflow_delivery: false.into(),
        hold_tokenizer: false.into(),
        tokenizer_error: false.into(),
        invalid_receipt: false.into(),
        panic_generation: false.into(),
        tokenizer_started: Notify::new(),
        tokenizer_release: Notify::new(),
        started: Notify::new(),
        finish: Notify::new(),
        completed: Notify::new(),
    });
    (
        AppState::new(auth, provider.clone(), "unused", provider.clone()),
        provider,
        bearer,
        submission,
    )
}
fn request(bearer: &str, submission: &str) -> Request<Body> {
    json_request(
        bearer,
        json!({"model":"m","stream":true,"stream_options":{"include_usage":true},"n":1,"submission":submission,"messages":[{"role":"system","content":"synthetic context"},{"role":"user","content":"synthetic request"}]}),
    )
}
fn json_request(bearer: &str, input: Value) -> Request<Body> {
    Request::post("/v1/chat/completions")
        .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(input.to_string()))
        .unwrap()
}
async fn send(state: &AppState, bearer: &str, submission: &str) -> Response {
    router(state.clone())
        .oneshot(request(bearer, submission))
        .await
        .unwrap()
}
async fn wait(notify: &Notify) {
    tokio::time::timeout(Duration::from_secs(3), notify.notified())
        .await
        .expect("synthetic worker deadline");
}
async fn terminal(state: &AppState, balance: u64) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while state.accounting.available("demo") != Some(balance) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("ledger terminal deadline");
}
async fn duplicate(state: &AppState, bearer: &str, submission: &str, expected: &str) {
    let response = send(state, bearer, submission).await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(value["error"]["code"] == "duplicate_request");
    assert!(value["error"]["outcome"] == expected);
}

#[tokio::test]
async fn progressive_stream_preserves_finish_and_settles_before_success_markers_without_replay() {
    async fn balance(state: &AppState, bearer: &str) -> Value {
        let response = router(state.clone())
            .oneshot(
                Request::post("/v1/balance")
                    .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }
    let (state, provider, bearer, submission) = fixture(5_000_000);
    assert_eq!(
        balance(&state, &bearer).await,
        json!({
            "available_microunits": "5000000", "in_flight": 0, "completed_requests": "0"
        })
    );
    let response = send(&state, &bearer, &submission).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "text/event-stream"
    );
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let mut body = response.into_body();
    let first = body.frame().await.unwrap().unwrap().into_data().unwrap();
    assert!(std::str::from_utf8(&first).unwrap().contains("assistant"));
    drop(first);
    let next = body.frame().await.unwrap().unwrap().into_data().unwrap();
    assert!(std::str::from_utf8(&next).unwrap().contains("synthetic"));
    drop(next);
    wait(&provider.started).await;
    assert_eq!(provider.output_allowance.load(Ordering::SeqCst), 18);
    assert_eq!(state.accounting.available("demo"), Some(5_000_000 - 52));
    duplicate(&state, &bearer, &submission, "in_flight").await;
    assert_eq!(
        balance(&state, &bearer).await,
        json!({
            "available_microunits": "4999948", "in_flight": 1, "completed_requests": "0"
        })
    );
    provider.price.store(100, Ordering::SeqCst); // Settlement must use the original rate.
    provider.finish.notify_one();
    let bytes = body.collect().await.unwrap().to_bytes();
    terminal(&state, 5_000_000 - 7).await;
    let text = std::str::from_utf8(&bytes).unwrap();
    assert!(text.contains("\"finish_reason\":\"length\""));
    assert!(text.contains("\"total_tokens\":5"));
    assert!(text.contains("\"charged_microunits\":\"7\""));
    assert!(text.contains("\"quoted_input_microunits_per_million_tokens\":\"1000000\""));
    assert!(text.contains("\"quoted_output_microunits_per_million_tokens\":\"1000000\""));
    assert!(text.ends_with("data: [DONE]\n\n"));
    duplicate(&state, &bearer, &submission, "settled").await;
    assert_eq!(
        balance(&state, &bearer).await,
        json!({
            "available_microunits": "4999993", "in_flight": 0, "completed_requests": "1"
        })
    );
    assert_eq!(provider.tokenizer_calls.load(Ordering::SeqCst), 1);
    assert_eq!(provider.generation_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn disconnect_near_authenticated_eof_keeps_single_settlement() {
    let (state, provider, bearer, submission) = fixture(5_000_000);
    let response = send(&state, &bearer, &submission).await;
    wait(&provider.started).await;
    let completed = provider.completed.notified();
    provider.finish.notify_one();
    tokio::time::timeout(Duration::from_secs(3), completed)
        .await
        .unwrap();
    drop(response);
    terminal(&state, 5_000_000 - 7).await;
    duplicate(&state, &bearer, &submission, "settled").await;
    assert_eq!(provider.generation_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn detached_generation_panic_refunds_once_without_replay() {
    let (state, provider, bearer, submission) = fixture(5_000_000);
    provider.panic_generation.store(true, Ordering::SeqCst);
    let response = send(&state, &bearer, &submission).await;
    drop(response);
    terminal(&state, 5_000_000).await;
    duplicate(&state, &bearer, &submission, "refunded").await;
    assert_eq!(provider.generation_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn disconnect_and_backpressure_never_cancel_generation_or_change_settlement() {
    for overflow in [false, true] {
        let (state, provider, bearer, submission) = fixture(5_000_000);
        provider.overflow_delivery.store(overflow, Ordering::SeqCst);
        let response = send(&state, &bearer, &submission).await;
        wait(&provider.started).await;
        if !overflow {
            drop(response);
        } else {
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            assert!(!std::str::from_utf8(&bytes).unwrap().contains("[DONE]"));
            assert!(bytes.len() < 64 * 1024);
        }
        assert_eq!(state.accounting.available("demo"), Some(5_000_000 - 52));
        provider.finish.notify_one();
        terminal(&state, 5_000_000 - 7).await;
        duplicate(&state, &bearer, &submission, "settled").await;
        assert_eq!(provider.generation_calls.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn upstream_failure_or_missing_usage_refunds_once_after_disconnect() {
    for invalid_usage in [false, true] {
        let (state, provider, bearer, submission) = fixture(5_000_000);
        provider
            .invalid_usage
            .store(invalid_usage, Ordering::SeqCst);
        provider
            .upstream_error
            .store(!invalid_usage, Ordering::SeqCst);
        let response = send(&state, &bearer, &submission).await;
        wait(&provider.started).await;
        drop(response);
        provider.finish.notify_one();
        terminal(&state, 5_000_000).await;
        duplicate(&state, &bearer, &submission, "refunded").await;
        assert_eq!(provider.generation_calls.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn cancelled_request_during_tokenization_keeps_accepted_work_and_refunds_context_failure() {
    for context_failure in [false, true] {
        let (state, provider, bearer, submission) = fixture(5_000_000);
        provider.hold_tokenizer.store(true, Ordering::SeqCst);
        if context_failure {
            provider.tokens.store(20, Ordering::SeqCst);
        }
        let app = router(state.clone());
        let task = tokio::spawn(app.oneshot(request(&bearer, &submission)));
        wait(&provider.tokenizer_started).await;
        task.abort();
        let _ = task.await;
        provider.tokenizer_release.notify_one();
        if context_failure {
            terminal(&state, 5_000_000).await;
            assert_eq!(provider.generation_calls.load(Ordering::SeqCst), 0);
            duplicate(&state, &bearer, &submission, "refunded").await;
        } else {
            wait(&provider.started).await;
            provider.finish.notify_one();
            terminal(&state, 5_000_000 - 7).await;
            duplicate(&state, &bearer, &submission, "settled").await;
        }
    }
}

#[tokio::test]
async fn trust_catalog_credit_and_context_fail_before_upstream_generation() {
    for failure in ["evidence", "catalog", "credit", "context", "stale"] {
        let (state, provider, bearer, submission) = fixture(5_000_000);
        let expected = match failure {
            "evidence" => {
                provider.valid_evidence.store(false, Ordering::SeqCst);
                StatusCode::SERVICE_UNAVAILABLE
            }
            "catalog" => {
                provider.valid_catalog.store(false, Ordering::SeqCst);
                StatusCode::SERVICE_UNAVAILABLE
            }
            "credit" => {
                provider.price.store(100_000, Ordering::SeqCst);
                StatusCode::PAYMENT_REQUIRED
            }
            "context" => {
                provider.tokens.store(20, Ordering::SeqCst);
                StatusCode::BAD_REQUEST
            }
            _ => {
                state.auth.issue_api_submission(&bearer, "m", true).unwrap();
                StatusCode::BAD_REQUEST
            }
        };
        let response = send(&state, &bearer, &submission).await;
        assert_eq!(response.status(), expected);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
        if failure == "credit" {
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            let value: Value = serde_json::from_slice(&bytes).unwrap();
            assert!(value["error"]["code"] == "insufficient_credit");
        } else {
            drop(response);
        }
        assert_eq!(
            provider.tokenizer_calls.load(Ordering::SeqCst),
            usize::from(failure == "context")
        );
        assert_eq!(provider.generation_calls.load(Ordering::SeqCst), 0);
        assert_eq!(state.accounting.available("demo"), Some(5_000_000));
    }
}

#[tokio::test]
async fn reject_buffered_mode_tools_malformed_history_oversize_and_old_restart_auth_before_prompts()
{
    let (state, provider, bearer, submission) = fixture(5_000_000);
    for input in [
        json!({"model":"m","stream":false,"submission":submission,"messages":[{"role":"user","content":"synthetic"}]}),
        json!({"model":"m","stream":true,"stream_options":{"include_usage":false},"submission":submission,"messages":[{"role":"user","content":"synthetic"}]}),
        json!({"model":"m","stream":true,"n":2,"submission":submission,"messages":[{"role":"user","content":"synthetic"}]}),
        json!({"model":"m","stream":true,"submission":submission,"messages":[]}),
        json!({"model":"m","stream":true,"submission":submission,"messages":[{"role":"tool","content":"synthetic"}]}),
        json!({"model":"m","stream":true,"submission":submission,"messages":[{"role":"assistant","content":"synthetic"}]}),
        json!({"model":"m","stream":true,"submission":submission,"messages":[{"role":"user","content":"synthetic","tool_calls":[]}]}),
        json!({"model":"m","stream":true,"submission":submission,"messages":[{"role":"user","content":"synthetic"}],"tools":[]}),
        json!({"model":"m","stream":true,"submission":submission,"messages":null}),
        json!({"model":"m","stream":true,"submission":submission,"messages":[{"role":"user","content":"synthetic"}],"tools":null}),
    ] {
        let response = router(state.clone())
            .oneshot(json_request(&bearer, input))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        drop(response);
    }
    let response = router(state.clone())
        .oneshot(
            Request::post("/v1/chat/completions")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(" ".repeat(BODY_LIMIT + 1)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    drop(response);
    assert_eq!(provider.tokenizer_calls.load(Ordering::SeqCst), 0);
    assert_eq!(provider.generation_calls.load(Ordering::SeqCst), 0);
    let (restarted, _, _, _) = fixture(5_000_000);
    assert_eq!(
        send(&restarted, &bearer, &submission).await.status(),
        StatusCode::UNAUTHORIZED
    );
}

fn structured_input(submission: &str) -> Value {
    json!({"model":"m","stream":true,"submission":submission,
        "tools":[{"type":"function","function":{"name":"lookup","description":"fixture","parameters":{"type":"object"}}}],
        "tool_choice":"auto",
        "messages":[{"role":"user","content":"fixture"},
            {"role":"assistant","content":null,"tool_calls":[{"id":"prior_0","type":"function","function":{"name":"lookup","arguments":"{}"}}]},
            {"role":"tool","tool_call_id":"prior_0","content":"fixture result"}]})
}

#[tokio::test]
async fn long_tool_descriptions_reach_tokenization_and_generation_unchanged() {
    for description in [
        "x".repeat(16 * 1024),
        "x".repeat(16 * 1024 + 1),
        "🐾".repeat(16 * 1024 + 1),
    ] {
        let (state, provider, bearer, submission) = fixture(5_000_000);
        provider.qualified.store(true, Ordering::SeqCst);
        let mut input = structured_input(&submission);
        input["tools"][0]["function"]["description"] = json!(description);
        let response = router(state.clone())
            .oneshot(json_request(&bearer, input))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        wait(&provider.started).await;
        assert_eq!(provider.tokenizer_calls.load(Ordering::SeqCst), 1);
        assert_eq!(provider.generation_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            provider.invocation.lock().unwrap().as_ref().unwrap()["tools"][0]["function"]
                ["description"],
            description
        );
        provider.finish.notify_one();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert!(bytes.ends_with(b"data: [DONE]\n\n"));
        terminal(&state, 5_000_000 - 7).await;
    }
}

#[tokio::test]
async fn large_synthetic_history_and_catalog_reach_upstream_within_body_bound() {
    let (state, provider, bearer, submission) = fixture(5_000_000);
    provider.qualified.store(true, Ordering::SeqCst);
    let mut input = structured_input(&submission);
    let calls: Vec<_> = (0..66).map(|i| json!({"id":format!("prior_{i}"),"type":"function","function":{"name":"lookup","arguments":serde_json::to_string(&json!({"x":"a".repeat(65 * 1024)})).unwrap()}})).collect();
    let mut messages = vec![json!({"role":"assistant","content":null,"tool_calls":calls})];
    messages.extend(
        (0..66).map(
            |i| json!({"role":"tool","tool_call_id":format!("prior_{i}"),"content":"synthetic"}),
        ),
    );
    messages.extend((0..4097).map(|_| json!({"role":"user","content":"synthetic"})));
    input["messages"] = json!(messages);
    input["tools"] = json!((0..65).map(|i| json!({"type":"function","function":{"name":format!("tool_{i}"),"parameters":{"type":"object"}}})).collect::<Vec<_>>());
    assert!(serde_json::to_vec(&input).unwrap().len() < BODY_LIMIT);
    let response = router(state.clone())
        .oneshot(json_request(&bearer, input.clone()))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    wait(&provider.started).await;
    {
        let observed = provider.invocation.lock().unwrap();
        assert_eq!(observed.as_ref().unwrap()["messages"], input["messages"]);
        assert_eq!(observed.as_ref().unwrap()["tools"], input["tools"]);
    }
    provider.finish.notify_one();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert!(bytes.ends_with(b"data: [DONE]\n\n"));
    terminal(&state, 5_000_000 - 7).await;
}

#[tokio::test]
async fn near_eight_mib_api_history_is_not_product_capped_or_replayed() {
    let (state, provider, bearer, submission) = fixture(5_000_000);
    let mut input = json!({"model":"m","stream":true,"stream_options":{"include_usage":true},
        "submission": submission, "messages":[{"role":"user","content":""}]});
    input["messages"][0]["content"] = json!("x".repeat(BODY_LIMIT - 512));
    let wire = input.to_string();
    assert!(wire.len() < BODY_LIMIT && wire.len() > BODY_LIMIT - 1024);
    let response = router(state.clone())
        .oneshot(
            Request::post("/v1/chat/completions")
                .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(wire))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    wait(&provider.started).await;
    drop(response);
    provider.finish.notify_one();
    terminal(&state, 5_000_000 - 7).await;
    duplicate(&state, &bearer, &submission, "settled").await;
    assert_eq!(provider.tokenizer_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn oversized_tool_description_still_hits_aggregate_body_limit() {
    let (state, provider, bearer, submission) = fixture(5_000_000);
    provider.qualified.store(true, Ordering::SeqCst);
    let mut input = structured_input(&submission);
    input["tools"][0]["function"]["description"] = json!("x".repeat(BODY_LIMIT + 1));
    let response = router(state.clone())
        .oneshot(json_request(&bearer, input))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(provider.tokenizer_calls.load(Ordering::SeqCst), 0);
    assert_eq!(provider.generation_calls.load(Ordering::SeqCst), 0);
    assert_eq!(state.accounting.available("demo"), Some(5_000_000));
}

#[tokio::test]
async fn tools_require_independent_qualification_rechecked_before_reservation() {
    for revoked in [false, true] {
        let (state, provider, bearer, submission) = fixture(5_000_000);
        provider.qualified.store(revoked, Ordering::SeqCst);
        provider
            .revoke_qualification
            .store(revoked, Ordering::SeqCst);
        let response = router(state.clone())
            .oneshot(json_request(&bearer, structured_input(&submission)))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            provider.qualification_checks.load(Ordering::SeqCst),
            if revoked { 2 } else { 1 }
        );
        assert_eq!(provider.tokenizer_calls.load(Ordering::SeqCst), 0);
        assert_eq!(provider.generation_calls.load(Ordering::SeqCst), 0);
        assert_eq!(state.accounting.available("demo"), Some(5_000_000));
    }
}

#[tokio::test]
async fn structured_history_exact_preflight_bounded_sse_and_original_price_receipt() {
    for choice in [
        json!("none"),
        json!("required"),
        json!({"type":"function","function":{"name":"named"}}),
    ] {
        let (state, provider, bearer, submission) = fixture(5_000_000);
        provider.qualified.store(true, Ordering::SeqCst);
        let mut input = structured_input(&submission);
        // Neither historical nor returned calls need belong to today's declarations.
        // Policy is enforced only at the caller's execution boundary.
        input["tools"][0]["function"]["name"] = json!("current");
        input["tool_choice"] = choice;
        let expected_messages = input["messages"].clone();
        let response = router(state.clone())
            .oneshot(json_request(&bearer, input))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        wait(&provider.started).await;
        assert_eq!(
            provider.invocation.lock().unwrap().as_ref().unwrap()["messages"],
            expected_messages
        );
        assert_eq!(provider.output_allowance.load(Ordering::SeqCst), 18);
        assert_eq!(state.accounting.available("demo"), Some(5_000_000 - 52));
        provider.price.store(100, Ordering::SeqCst);
        provider.finish.notify_one();
        let mut body = response.into_body();
        let mut arguments = String::new();
        let mut fragments = 0;
        let mut finished = false;
        let mut receipt = false;
        let mut done = false;
        while let Some(frame) = body.frame().await {
            let frame = frame.unwrap().into_data().unwrap();
            assert!(frame.len() <= 8192);
            let text = std::str::from_utf8(&frame).unwrap();
            if text == "data: [DONE]\n\n" {
                assert!(finished && receipt);
                done = true;
                continue;
            }
            let value: Value =
                serde_json::from_str(text.strip_prefix("data: ").unwrap().trim()).unwrap();
            if let Some(delta) =
                value["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"].as_str()
            {
                assert!(!finished);
                fragments += 1;
                arguments.push_str(delta);
            }
            if let Some(reason) = value["choices"][0]["finish_reason"].as_str() {
                assert_eq!(reason, "stop"); // Never normalize stop-with-calls to tool_calls.
                finished = true;
            }
            if value.get("possums").is_some() {
                assert!(finished);
                assert_eq!(state.accounting.available("demo"), Some(5_000_000 - 7));
                assert_eq!(value["possums"]["charged_microunits"], "7");
                assert_eq!(value["possums"]["refunded_microunits"], "45");
                assert_eq!(
                    value["possums"]["quoted_input_microunits_per_million_tokens"],
                    "1000000"
                );
                assert_eq!(
                    value["possums"]["quoted_output_microunits_per_million_tokens"],
                    "1000000"
                );
                receipt = true;
            }
        }
        assert!(done && fragments > 1);
        assert_eq!(
            serde_json::from_str::<Value>(&arguments).unwrap(),
            json!({"q":"🐾\n".repeat(260)})
        );
        duplicate(&state, &bearer, &submission, "settled").await;
    }
}

#[tokio::test]
async fn structured_disconnect_overflow_and_failure_keep_sole_settling_owner() {
    for mode in [
        "disconnect",
        "overflow",
        "failure",
        "invalid_usage",
        "missing_final_usage",
    ] {
        let (state, provider, bearer, submission) = fixture(5_000_000);
        provider.qualified.store(true, Ordering::SeqCst);
        provider
            .overflow_delivery
            .store(mode == "overflow", Ordering::SeqCst);
        provider
            .upstream_error
            .store(mode == "failure", Ordering::SeqCst);
        provider
            .invalid_usage
            .store(mode == "invalid_usage", Ordering::SeqCst);
        provider
            .missing_final_usage
            .store(mode == "missing_final_usage", Ordering::SeqCst);
        let failed = matches!(mode, "failure" | "invalid_usage" | "missing_final_usage");
        let response = router(state.clone())
            .oneshot(json_request(&bearer, structured_input(&submission)))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        wait(&provider.started).await;
        if mode == "overflow" {
            let body = response.into_body().collect().await.unwrap().to_bytes();
            assert!(body.len() <= 64 * 1024);
            assert!(!std::str::from_utf8(&body).unwrap().contains("[DONE]"));
        } else {
            drop(response);
        }
        provider.finish.notify_one();
        terminal(&state, 5_000_000 - if failed { 0 } else { 7 }).await;
        duplicate(
            &state,
            &bearer,
            &submission,
            if failed { "refunded" } else { "settled" },
        )
        .await;
        assert_eq!(provider.generation_calls.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn structured_preflight_survives_request_cancellation_and_context_rejection() {
    for context_failure in [false, true] {
        let (state, provider, bearer, submission) = fixture(5_000_000);
        provider.qualified.store(true, Ordering::SeqCst);
        provider.hold_tokenizer.store(true, Ordering::SeqCst);
        if context_failure {
            provider.tokens.store(20, Ordering::SeqCst);
        }
        let request = json_request(&bearer, structured_input(&submission));
        let task = tokio::spawn(router(state.clone()).oneshot(request));
        wait(&provider.tokenizer_started).await;
        task.abort();
        let _ = task.await;
        provider.tokenizer_release.notify_one();
        if context_failure {
            terminal(&state, 5_000_000).await;
            assert_eq!(provider.generation_calls.load(Ordering::SeqCst), 0);
            duplicate(&state, &bearer, &submission, "refunded").await;
        } else {
            wait(&provider.started).await;
            provider.finish.notify_one();
            terminal(&state, 5_000_000 - 7).await;
            duplicate(&state, &bearer, &submission, "settled").await;
        }
    }
}

#[tokio::test]
async fn structured_model_absent_from_authenticated_catalog_stops_before_upstream() {
    let (state, provider, bearer, submission) = fixture(5_000_000);
    provider.qualified.store(true, Ordering::SeqCst);
    provider.catalog_has_m.store(false, Ordering::SeqCst);
    // The fixture explicitly offers a wire profile for "m", but the live
    // authenticated catalog now contains only "other".
    assert_eq!(
        provider.tool_profile("m"),
        Some(ToolProfile::OpenAiFunctionsV1)
    );
    let response = router(state.clone())
        .oneshot(json_request(&bearer, structured_input(&submission)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(provider.tokenizer_calls.load(Ordering::SeqCst), 0);
    assert_eq!(provider.generation_calls.load(Ordering::SeqCst), 0);
    assert_eq!(state.accounting.available("demo"), Some(5_000_000));
}

#[tokio::test]
async fn catalog_advertises_only_explicit_fixture_profile() {
    let (state, provider, bearer, _) = fixture(5_000_000);
    for qualified in [false, true] {
        provider.qualified.store(qualified, Ordering::SeqCst);
        let response = router(state.clone())
            .oneshot(
                Request::get("/v1/models")
                    .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            value["data"][0].get("tool_protocol"),
            qualified.then_some(&json!("openai-functions-v1"))
        );
    }
}

#[tokio::test]
async fn safe_details_survive_middleware_and_only_actual_refunds_are_reported() {
    for preflight in ["tokenizer", "evidence", "catalog"] {
        let (state, provider, bearer, submission) = fixture(5_000_000);
        let detail = match preflight {
            "tokenizer" => {
                provider.tokenizer_error.store(true, Ordering::SeqCst);
                "tokenizer_response_invalid"
            }
            "evidence" => {
                provider.valid_evidence.store(false, Ordering::SeqCst);
                "verification_failed"
            }
            _ => {
                provider.valid_catalog.store(false, Ordering::SeqCst);
                "inference_unavailable"
            }
        };
        let response = send(&state, &bearer, &submission).await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["error"]["code"], "unavailable");
        assert_eq!(value["error"]["detail"], detail);
        assert_eq!(value["error"]["billing"], "unknown"); // Drop is not a receipt.
        assert!(value["error"]["message"].is_string());
        assert!(!std::str::from_utf8(&bytes).unwrap().contains("synthetic"));
        assert_eq!(provider.generation_calls.load(Ordering::SeqCst), 0);
        assert_eq!(state.accounting.available("demo"), Some(5_000_000));
    }
    for invalid_receipt in [false, true] {
        let (state, provider, bearer, submission) = fixture(5_000_000);
        provider
            .upstream_error
            .store(!invalid_receipt, Ordering::SeqCst);
        provider
            .invalid_receipt
            .store(invalid_receipt, Ordering::SeqCst);
        let response = send(&state, &bearer, &submission).await;
        wait(&provider.started).await;
        provider.finish.notify_one();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let text = std::str::from_utf8(&bytes).unwrap();
        let error = text
            .lines()
            .find(|line| line.starts_with("data: {\"error\":{\"code\":\"generation_failed\""))
            .unwrap();
        let value: Value = serde_json::from_str(error.strip_prefix("data: ").unwrap()).unwrap();
        assert_eq!(
            value["error"]["detail"],
            if invalid_receipt {
                "settlement_failed"
            } else {
                "stream_transport_failed"
            }
        );
        assert_eq!(
            value["error"]["billing"],
            if invalid_receipt {
                "unknown"
            } else {
                "refunded"
            }
        );
        assert!(!error.contains("synthetic"));
        assert!(!text.contains("[DONE]"));
        terminal(&state, 5_000_000).await;
        duplicate(&state, &bearer, &submission, "refunded").await;
        assert_eq!(provider.generation_calls.load(Ordering::SeqCst), 1);
    }
}
