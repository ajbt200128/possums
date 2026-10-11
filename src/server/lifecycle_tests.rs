use super::resource_streaming_tests::{checkpoint, Pause};
use super::*;
use crate::{
    attestation::{EvidenceError, EvidenceVerifier, GatewayEvidence},
    catalog::Model,
    inference::{
        stream::{FinishReason, StreamCompletion, StreamUsage},
        tools::{ToolInvocation, ToolProfile},
        Inference, InferenceError, Message,
    },
};
use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use futures_util::poll;
use http_body_util::BodyExt;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::sync::{atomic::Ordering, Mutex};

#[derive(Default)]
pub(super) struct ServerHooks {
    pub(super) accept_failure: Mutex<Option<tokio::sync::oneshot::Receiver<()>>>,
    pub(super) connection_start: Mutex<Option<Pause>>,
    pub(super) connection_finished: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    pub(super) shutdown_published: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tower::ServiceExt;

#[derive(Default)]
struct Probe {
    evidence: Mutex<Option<Pause>>,
    tokenizer: Mutex<Option<Pause>>,
    generation: Mutex<Option<Pause>>,
    calls: AtomicUsize,
    emit_progress: std::sync::atomic::AtomicBool,
    verify_calls: AtomicUsize,
    catalog_calls: AtomicUsize,
    tokenize_calls: AtomicUsize,
    document_calls: AtomicUsize,
}
#[async_trait]
impl EvidenceVerifier for Probe {
    async fn verify(&self, _: &str, now: u64) -> Result<GatewayEvidence, EvidenceError> {
        self.verify_calls.fetch_add(1, Ordering::SeqCst);
        let pause = self.evidence.lock().unwrap().take();
        if let Some(pause) = pause {
            pause.wait().await;
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
impl Probe {
    async fn complete_generation(&self) -> StreamCompletion {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let pause = self.generation.lock().unwrap().take();
        if let Some(pause) = pause {
            pause.wait().await;
        }
        completion()
    }
}
#[async_trait]
impl Inference for Probe {
    async fn catalog(&self) -> Result<Vec<u8>, InferenceError> {
        self.catalog_calls.fetch_add(1, Ordering::SeqCst);
        Ok(br#"{"object":"list","data":[{"id":"m","type":"chat","context_window":20,"endpoints":["/v1/chat/completions"],"pricing":{"inputTokenPricePer1M":1,"outputTokenPricePer1M":1,"requestPrice":0}}]}"#.to_vec())
    }
    async fn count_tokens(
        &self,
        _: &str,
        _: &[Message],
        _: Arc<Lease>,
    ) -> Result<u64, InferenceError> {
        self.tokenize_calls.fetch_add(1, Ordering::SeqCst);
        let pause = self.tokenizer.lock().unwrap().take();
        if let Some(pause) = pause {
            pause.wait().await;
        }
        Ok(2)
    }
    fn tool_profile(&self, _: &str) -> Option<ToolProfile> {
        Some(ToolProfile::OpenAiFunctionsV1)
    }
    async fn count_invocation_tokens(
        &self,
        model: &str,
        _: &ToolInvocation,
        heavy: Arc<Lease>,
    ) -> Result<u64, InferenceError> {
        self.count_tokens(model, &[], heavy).await
    }
    async fn generate_completion_stream(
        &self,
        _: &Model,
        _: &[Message],
        _: Arc<Lease>,
        on_delta: &mut dyn crate::inference::TextDeltaSink,
    ) -> Result<StreamCompletion, InferenceError> {
        if self.emit_progress.load(Ordering::SeqCst) {
            on_delta.emit("synthetic progress").await;
        }
        Ok(self.complete_generation().await)
    }
    async fn generate_invocation_stream(
        &self,
        _: &Model,
        _: &ToolInvocation,
        _: Arc<Lease>,
        _: &mut dyn crate::inference::DeltaSink,
    ) -> Result<StreamCompletion, InferenceError> {
        Ok(self.complete_generation().await)
    }
    fn verification_document(&self) -> Result<serde_json::Value, InferenceError> {
        self.document_calls.fetch_add(1, Ordering::SeqCst);
        Ok(json!({}))
    }
}
fn completion() -> StreamCompletion {
    StreamCompletion {
        usage: StreamUsage {
            input_tokens: 2,
            output_tokens: 3,
            total_tokens: 5,
        },
        finish_reason: FinishReason::Stop,
    }
}
fn fixture() -> (AppState, Arc<Probe>, String) {
    let credential = URL_SAFE_NO_PAD.encode([8; 32]);
    let auth = Auth::from_json(&json!([{"id":"demo","credential_sha256":URL_SAFE_NO_PAD.encode(Sha256::digest(credential.as_bytes())),"demo_microunits":1000}]).to_string()).unwrap();
    let (session, _) = auth
        .authenticate_api(&credential, &auth.issue_api_challenge().unwrap())
        .unwrap();
    let probe = Arc::new(Probe::default());
    (
        AppState::new(auth, probe.clone(), "unused", probe.clone()),
        probe,
        session,
    )
}
fn chat(state: &AppState, session: &str, structured: bool) -> Request<Body> {
    let token = state
        .auth
        .issue_api_submission(session, "m", false)
        .unwrap();
    let mut input = json!({"model":"m","submission":token,"stream":true,"messages":[{"role":"user","content":"synthetic"}]});
    if structured {
        input["tools"] = json!([{"type":"function","function":{"name":"lookup","parameters":{"type":"object","properties":{}}}}]);
    }
    Request::post("/v1/chat/completions")
        .header(header::AUTHORIZATION, format!("Bearer {session}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(input.to_string()))
        .unwrap()
}
async fn bounded<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(5), future)
        .await
        .unwrap()
}

#[tokio::test]
async fn unread_model_and_submission_responses_do_not_admit_or_block_controls() {
    let (state, probe, session) = fixture();
    let mut unread = Vec::new();
    for _ in 0..3 {
        for (method, path, body) in [
            (Method::GET, "/v1/models", ""),
            (
                Method::POST,
                "/v1/submissions",
                r#"{"model":"m","new_conversation":false}"#,
            ),
        ] {
            let mut request = Request::builder()
                .method(method.clone())
                .uri(path)
                .header(header::AUTHORIZATION, format!("Bearer {session}"));
            if method == Method::POST {
                request = request.header(header::CONTENT_TYPE, "application/json");
            }
            let response = router(state.clone())
                .oneshot(request.body(Body::from(body)).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            unread.push(response);
        }
    }
    assert_eq!(state.active_lanes(), [0, 0, 0]);
    let response = router(state.clone())
        .oneshot(chat(&state, &session, false))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    drop(response);
    drop(unread);
    state.lifecycle.quiesce();
    bounded(state.lifecycle.wait_drained()).await.unwrap();
    assert_eq!(probe.calls.load(Ordering::SeqCst), 1);
    assert_eq!(state.active_lanes(), [0, 0, 0]);
}

#[tokio::test]
async fn accepted_before_evidence_fence_can_reserve_but_new_chat_cannot() {
    for structured in [false, true] {
        let (state, probe, session) = fixture();
        let request = chat(&state, &session, structured);
        let rejected = chat(&state, &session, structured);
        let (pause, gate) = checkpoint();
        *probe.evidence.lock().unwrap() = Some(pause);
        let handler = tokio::spawn(router(state.clone()).oneshot(request));
        bounded(gate.reached).await.unwrap();
        state.lifecycle.quiesce();
        let drain = state.lifecycle.wait_drained();
        tokio::pin!(drain);
        assert!(poll!(&mut drain).is_pending());
        assert_eq!(state.accounting.snapshot("demo").unwrap().in_flight, 0);
        let response = router(state.clone()).oneshot(rejected).await.unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["error"]["code"], "service_quiescing");
        assert_eq!(value["error"]["billing"], "not_submitted");
        assert_eq!(probe.calls.load(Ordering::SeqCst), 0);
        gate.release.send(()).unwrap();
        bounded(handler)
            .await
            .unwrap()
            .unwrap()
            .into_body()
            .collect()
            .await
            .unwrap();
        assert_eq!(bounded(drain).await, Ok(()));
        assert_eq!(probe.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            state
                .accounting
                .snapshot("demo")
                .unwrap()
                .completed_requests,
            1
        );
    }
}

#[tokio::test]
async fn control_handler_remains_counted_across_fence_and_mutates_only_if_admitted() {
    let (state, probe, session) = fixture();
    let (pause, gate) = checkpoint();
    *probe.evidence.lock().unwrap() = Some(pause);
    let request = || {
        Request::post("/v1/submissions")
            .header(header::AUTHORIZATION, format!("Bearer {session}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"model":"m","new_conversation":false}"#))
            .unwrap()
    };
    let handler = tokio::spawn(router(state.clone()).oneshot(request()));
    bounded(gate.reached).await.unwrap();
    state.lifecycle.quiesce();
    let drain = state.lifecycle.wait_drained();
    tokio::pin!(drain);
    assert!(poll!(&mut drain).is_pending());
    let logout = Request::delete("/v1/sessions/current")
        .header(header::AUTHORIZATION, format!("Bearer {session}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        router(state.clone())
            .oneshot(logout)
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert!(state.auth.api_session(&session).is_some());
    gate.release.send(()).unwrap();
    assert_eq!(
        bounded(handler).await.unwrap().unwrap().status(),
        StatusCode::OK
    );
    assert_eq!(bounded(drain).await, Ok(()));
    assert_eq!(probe.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn lost_handler_and_finished_worker_still_wait_for_whole_preflight() {
    for structured in [false, true] {
        let (state, probe, session) = fixture();
        let (pause, gate) = checkpoint();
        *state.preflight_hooks.after.lock().unwrap() = Some(pause);
        let handler =
            tokio::spawn(router(state.clone()).oneshot(chat(&state, &session, structured)));
        bounded(gate.reached).await.unwrap();
        handler.abort();
        assert!(bounded(handler).await.unwrap_err().is_cancelled());
        // Worker returns generation marker before memory; preflight retains its
        // own heavy ref. Yield until the worker cleanup marker returns.
        bounded(async {
            while state.active_lanes()[0] != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await;
        assert_eq!(probe.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            state
                .accounting
                .snapshot("demo")
                .unwrap()
                .completed_requests,
            1
        );
        state.lifecycle.quiesce();
        let drain = state.lifecycle.wait_drained();
        tokio::pin!(drain);
        assert!(poll!(&mut drain).is_pending());
        gate.release.send(()).unwrap();
        assert_eq!(bounded(drain).await, Ok(()));
    }
}

#[tokio::test]
async fn health_is_credential_free_unobserved_and_independent_of_control_capacity() {
    let (state, probe, session) = fixture();
    let metrics = Arc::new(crate::telemetry::AggregateMetrics::new(
        crate::telemetry::Deployment::IsolatedSynthetic,
        crate::telemetry::SystemClock::default(),
    ));
    // TEST ONLY: bypass the synthetic fixture's warmup, not the production
    // release gate. These assertions inspect active counts, not exported windows.
    metrics.prime_synthetic_application_window();
    let state = state.with_telemetry(Some(metrics.clone()));
    // Positive control through the same oneshot router: a real application
    // request must create HTTP observations before absence can be meaningful.
    let response = router(state.clone())
        .oneshot(
            Request::get("/v1/models")
                .header(header::AUTHORIZATION, format!("Bearer {session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    response.into_body().collect().await.unwrap();
    let observations = metrics.application_counts();
    assert_eq!(observations[0], 1); // HTTP start
    assert_eq!(observations[1], 1); // HTTP terminal
    assert_eq!(observations[2], 1); // HTTP duration bucket
    assert_eq!(probe.verify_calls.load(Ordering::SeqCst), 1);
    assert_eq!(probe.catalog_calls.load(Ordering::SeqCst), 1);
    let dependency_calls = || {
        (
            probe.calls.load(Ordering::SeqCst),
            probe.verify_calls.load(Ordering::SeqCst),
            probe.catalog_calls.load(Ordering::SeqCst),
            probe.tokenize_calls.load(Ordering::SeqCst),
            probe.document_calls.load(Ordering::SeqCst),
        )
    };
    let dependencies = dependency_calls();
    let balance = state.accounting.snapshot("demo").unwrap();
    let auth = state.auth.api_session(&session).unwrap();
    for status in [StatusCode::NO_CONTENT, StatusCode::SERVICE_UNAVAILABLE] {
        if status == StatusCode::SERVICE_UNAVAILABLE {
            state.lifecycle.quiesce();
        }
        for method in [Method::GET, Method::HEAD] {
            let response = router(state.clone())
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri("/healthz")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), status);
            assert!(response
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .is_empty());
            assert_eq!(metrics.application_counts(), observations);
            assert_eq!(dependency_calls(), dependencies);
            assert_eq!(state.accounting.snapshot("demo").unwrap(), balance);
            let current = state.auth.api_session(&session).unwrap();
            assert!(
                current.account_id == auth.account_id
                    && current.admission_binding == auth.admission_binding
                    && current.selected_model == auth.selected_model
                    && current.conversation == auth.conversation
            );
        }
    }
    // oneshot does not exercise the real network connection lease.
}

#[tokio::test]
async fn shutdown_retains_admitted_connection_and_fences_new_work() {
    let (state, probe, session) = fixture();
    let (pause, gate) = checkpoint();
    *probe.evidence.lock().unwrap() = Some(pause);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopping) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(serve_until(listener, state.clone(), async move {
        let _ = stopping.await;
    }));
    let mut client = tokio::net::TcpStream::connect(address).await.unwrap();
    client.write_all(format!("GET /v1/models HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {session}\r\n\r\n").as_bytes()).await.unwrap();
    bounded(gate.reached).await.unwrap();
    stop.send(()).unwrap();
    bounded(async {
        while state.lifecycle.is_serving() {
            tokio::task::yield_now().await;
        }
    })
    .await;
    assert!(!server.is_finished());
    assert!(state.lifecycle.try_admit().is_none());
    gate.release.send(()).unwrap();
    let mut received = Vec::new();
    bounded(client.read_to_end(&mut received)).await.unwrap();
    assert!(received.starts_with(b"HTTP/1.1 200"));
    assert!(bounded(server).await.unwrap().is_ok());
    assert_eq!(probe.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn actual_disconnect_during_tokenization_prevents_generation_and_refunds() {
    let (state, probe, session) = fixture();
    let (pause, gate) = checkpoint();
    *probe.tokenizer.lock().unwrap() = Some(pause);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopping) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(serve_until(listener, state.clone(), async move {
        let _ = stopping.await;
    }));
    let body = chat(&state, &session, false)
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let mut client = tokio::net::TcpStream::connect(address).await.unwrap();
    client.write_all(format!("POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {session}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n", body.len()).as_bytes()).await.unwrap();
    client.write_all(&body).await.unwrap();
    bounded(gate.reached).await.unwrap();
    assert_eq!(state.accounting.snapshot("demo").unwrap().in_flight, 1);
    drop(client);
    bounded(async {
        while state.accounting.snapshot("demo").unwrap().in_flight != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await;
    assert_eq!(probe.calls.load(Ordering::SeqCst), 0);
    assert_eq!(probe.tokenize_calls.load(Ordering::SeqCst), 1);
    assert_eq!(state.accounting.available("demo"), Some(1000));
    assert!(gate.release.send(()).is_err());
    stop.send(()).unwrap();
    assert!(bounded(server).await.unwrap().is_ok());
}

#[tokio::test]
async fn shutdown_before_accept_and_repeated_intent_is_idempotent() {
    let (state, _, _) = fixture();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    assert!(
        bounded(serve_until(listener, state.clone(), std::future::ready(())))
            .await
            .is_ok()
    );
    state.lifecycle.quiesce();
    assert!(state.lifecycle.try_admit().is_none());
    assert_eq!(bounded(state.lifecycle.wait_drained()).await, Ok(()));
}

#[tokio::test]
async fn disconnect_cancels_original_owner_before_shutdown_finishes() {
    let (state, probe, session) = fixture();
    let (pause, gate) = checkpoint();
    *probe.generation.lock().unwrap() = Some(pause);
    let (finished, connection_finished) = tokio::sync::oneshot::channel();
    *state.server_hooks.connection_finished.lock().unwrap() = Some(finished);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopping) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(serve_until(listener, state.clone(), async move {
        let _ = stopping.await;
    }));
    let request = chat(&state, &session, false);
    let body = request.into_body().collect().await.unwrap().to_bytes();
    let mut client = tokio::net::TcpStream::connect(address).await.unwrap();
    client.write_all(format!("POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {session}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n", body.len()).as_bytes()).await.unwrap();
    client.write_all(&body).await.unwrap();
    bounded(gate.reached).await.unwrap();
    drop(client);
    stop.send(()).unwrap();
    bounded(connection_finished).await.unwrap(); // The retained connection future actually retired.
    assert!(!state.lifecycle.is_serving());
    assert!(state.lifecycle.try_admit().is_none());
    // No draining/releasing the fixture: connection closure cancels generation.
    state.lifecycle.quiesce();
    assert!(bounded(server).await.unwrap().is_ok());
    assert!(gate.release.send(()).is_err());
    assert_eq!(state.accounting.available("demo"), Some(1000));
    assert_eq!(probe.calls.load(Ordering::SeqCst), 1);
    let balance = state.accounting.snapshot("demo").unwrap();
    assert_eq!(balance.in_flight, 0);
    assert_eq!(balance.completed_requests, 1);
}

#[tokio::test]
async fn disconnect_after_progress_cancels_without_shutdown_or_another_write() {
    let (state, probe, session) = fixture();
    probe.emit_progress.store(true, Ordering::SeqCst);
    let (pause, gate) = checkpoint();
    *probe.generation.lock().unwrap() = Some(pause);
    let (finished, connection_finished) = tokio::sync::oneshot::channel();
    *state.server_hooks.connection_finished.lock().unwrap() = Some(finished);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopping) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(serve_until(listener, state.clone(), async move {
        let _ = stopping.await;
    }));
    let body = chat(&state, &session, false)
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let mut client = tokio::net::TcpStream::connect(address).await.unwrap();
    client.write_all(format!("POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {session}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n", body.len()).as_bytes()).await.unwrap();
    client.write_all(&body).await.unwrap();
    bounded(gate.reached).await.unwrap();
    let mut received = Vec::new();
    bounded(async {
        while !received
            .windows(b"synthetic progress".len())
            .any(|window| window == b"synthetic progress")
        {
            let mut bytes = [0; 512];
            let length = client.read(&mut bytes).await.unwrap();
            assert!(length > 0);
            received.extend_from_slice(&bytes[..length]);
            assert!(received.len() < 4096);
        }
    })
    .await;
    drop(client);
    bounded(connection_finished).await.unwrap();
    assert!(state.lifecycle.is_serving());
    assert!(gate.release.send(()).is_err());
    stop.send(()).unwrap();
    assert!(bounded(server).await.unwrap().is_ok());
    assert_eq!(state.accounting.available("demo"), Some(1000));
    let balance = state.accounting.snapshot("demo").unwrap();
    assert_eq!((balance.in_flight, balance.completed_requests), (0, 1));
}

#[tokio::test]
async fn fatal_accept_failure_fences_but_waits_for_original_owner() {
    let (state, probe, session) = fixture();
    let (pause, gate) = checkpoint();
    *probe.generation.lock().unwrap() = Some(pause);
    let (fail, failure) = tokio::sync::oneshot::channel();
    *state.server_hooks.accept_failure.lock().unwrap() = Some(failure);
    let (published, shutdown_published) = tokio::sync::oneshot::channel();
    *state.server_hooks.shutdown_published.lock().unwrap() = Some(published);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(serve_until(listener, state.clone(), std::future::pending()));
    let request = chat(&state, &session, false);
    let body = request.into_body().collect().await.unwrap().to_bytes();
    let mut client = tokio::net::TcpStream::connect(address).await.unwrap();
    client.write_all(format!("POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {session}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n", body.len()).as_bytes()).await.unwrap();
    client.write_all(&body).await.unwrap();
    bounded(gate.reached).await.unwrap(); // Real accept, reserve, and adapter dispatch.
    fail.send(()).unwrap();
    bounded(shutdown_published).await.unwrap();
    assert!(state.lifecycle.try_admit().is_none());
    assert_eq!(state.accounting.snapshot("demo").unwrap().in_flight, 1);
    assert!(!server.is_finished());
    drop(client);
    gate.release.send(()).unwrap();
    let result = bounded(server).await.unwrap().unwrap_err();
    assert_eq!(result.to_string(), "injected listener failure");
    let balance = state.accounting.snapshot("demo").unwrap();
    assert_eq!(balance.in_flight, 0);
    assert_eq!(balance.completed_requests, 1);
    assert_eq!(probe.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn accepted_connection_observes_shutdown_before_its_first_poll() {
    let (state, probe, session) = fixture();
    let (pause, gate) = checkpoint();
    *state.server_hooks.connection_start.lock().unwrap() = Some(pause);
    let (published, shutdown_published) = tokio::sync::oneshot::channel();
    *state.server_hooks.shutdown_published.lock().unwrap() = Some(published);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopping) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(serve_until(listener, state.clone(), async move {
        let _ = stopping.await;
    }));
    let mut client = tokio::net::TcpStream::connect(address).await.unwrap();
    client.write_all(format!("GET /v1/models HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {session}\r\n\r\n").as_bytes()).await.unwrap();
    bounded(gate.reached).await.unwrap(); // Accepted, but no connection future was polled.
    stop.send(()).unwrap();
    bounded(shutdown_published).await.unwrap(); // Fence and sticky watch both published.
    assert!(state.lifecycle.try_admit().is_none());
    assert!(!server.is_finished());
    gate.release.send(()).unwrap();
    let mut received = Vec::new();
    if let Err(error) = bounded(client.read_to_end(&mut received)).await {
        assert_eq!(error.kind(), std::io::ErrorKind::ConnectionReset);
    }
    assert!(!received.starts_with(b"HTTP/1.1 200"));
    assert!(bounded(server).await.unwrap().is_ok());
    assert_eq!(probe.verify_calls.load(Ordering::SeqCst), 0);
    assert_eq!(probe.catalog_calls.load(Ordering::SeqCst), 0);
    assert_eq!(probe.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn duplicate_does_not_add_an_accounting_owner() {
    let (state, probe, session) = fixture();
    let request = chat(&state, &session, false);
    let (parts, body) = request.into_parts();
    let bytes = body.collect().await.unwrap().to_bytes();
    let (pause, gate) = checkpoint();
    *probe.tokenizer.lock().unwrap() = Some(pause);
    let handler = tokio::spawn(router(state.clone()).oneshot(Request::from_parts(
        parts.clone(),
        Body::from(bytes.clone()),
    )));
    bounded(gate.reached).await.unwrap();
    let duplicate = router(state.clone())
        .oneshot(Request::from_parts(parts, Body::from(bytes)))
        .await
        .unwrap();
    assert_eq!(duplicate.status(), StatusCode::CONFLICT);
    state.lifecycle.quiesce();
    let drain = state.lifecycle.wait_drained();
    tokio::pin!(drain);
    assert!(poll!(&mut drain).is_pending());
    gate.release.send(()).unwrap();
    drop(bounded(handler).await.unwrap().unwrap());
    assert_eq!(bounded(drain).await, Ok(()));
    assert_eq!(
        state
            .accounting
            .snapshot("demo")
            .unwrap()
            .completed_requests,
        1
    );
}

#[tokio::test]
async fn overlapping_chat_has_no_admission_gate_and_disconnect_drains_original_owners() {
    for structured in [false, true] {
        let (state, probe, session) = fixture();
        let mut gates = Vec::new();
        let mut responses = Vec::new();
        for n in 1..=9 {
            let (pause, gate) = checkpoint();
            *probe.generation.lock().unwrap() = Some(pause);
            let response = router(state.clone())
                .oneshot(chat(&state, &session, structured))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            bounded(gate.reached).await.unwrap();
            responses.push(response); // Keep connected workers paused concurrently.
            gates.push(gate.release);
            assert_eq!(state.active_lanes(), [n, n, 0]);
            assert_eq!(
                state.accounting.snapshot("demo").unwrap().in_flight,
                n as u32
            );
        }
        assert_eq!(probe.calls.load(Ordering::SeqCst), 9);
        state.lifecycle.quiesce();
        let drain = state.lifecycle.wait_drained();
        tokio::pin!(drain);
        assert!(poll!(&mut drain).is_pending());
        drop(responses); // Actual disconnect cancels, without releasing fixture gates.
        assert_eq!(bounded(drain).await, Ok(()));
        for release in gates {
            assert!(release.send(()).is_err());
        }
        assert_eq!(state.active_lanes(), [0, 0, 0]);
        assert_eq!(probe.calls.load(Ordering::SeqCst), 9); // No replay.
        let balance = state.accounting.snapshot("demo").unwrap();
        assert_eq!(balance.in_flight, 0);
        assert_eq!(balance.completed_requests, 9);
        assert_eq!(balance.available_microunits, 1000); // No trustworthy usage.
    }
}
