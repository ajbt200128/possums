use super::resource_streaming_tests::{checkpoint, Pause};
use super::*;
use crate::{
    attestation::{EvidenceError, EvidenceVerifier, GatewayEvidence},
    catalog::Model,
    inference::{
        stream::{FinishReason, StreamCompletion, StreamUsage},
        tools::{CompletionDelta, ToolInvocation, ToolProfile},
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
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tower::ServiceExt;

#[derive(Default)]
struct Probe {
    evidence: Mutex<Option<Pause>>,
    tokenizer: Mutex<Option<Pause>>,
    calls: AtomicUsize,
}
#[async_trait]
impl EvidenceVerifier for Probe {
    async fn verify(&self, _: &str, now: u64) -> Result<GatewayEvidence, EvidenceError> {
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
#[async_trait]
impl Inference for Probe {
    async fn catalog(&self) -> Result<Vec<u8>, InferenceError> {
        Ok(br#"{"object":"list","data":[{"id":"m","type":"chat","context_window":20,"endpoints":["/v1/chat/completions"],"pricing":{"inputTokenPricePer1M":1,"outputTokenPricePer1M":1,"requestPrice":0}}]}"#.to_vec())
    }
    async fn count_tokens(
        &self,
        _: &str,
        _: &[Message],
        _: Arc<Lease>,
    ) -> Result<u64, InferenceError> {
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
        _: &mut (dyn for<'d> FnMut(&'d str) + Send),
    ) -> Result<StreamCompletion, InferenceError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(completion())
    }
    async fn generate_invocation_stream(
        &self,
        _: &Model,
        _: &ToolInvocation,
        _: Arc<Lease>,
        _: &mut (dyn for<'d> FnMut(CompletionDelta<'d>) + Send),
    ) -> Result<StreamCompletion, InferenceError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(completion())
    }
    fn verification_document(&self) -> Result<serde_json::Value, InferenceError> {
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
        drop(bounded(handler).await.unwrap().unwrap());
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
            while state.generation_headroom() != CHAT_LANES {
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
    let (state, probe, _) = fixture();
    let control = state.control_memory.clone().acquire_owned().await.unwrap();
    let request = || Request::get("/healthz").body(Body::empty()).unwrap();
    let response = router(state.clone()).oneshot(request()).await.unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .is_empty());
    assert_eq!(probe.calls.load(Ordering::SeqCst), 0);
    drop(control);
    state.lifecycle.quiesce();
    let response = router(state.clone()).oneshot(request()).await.unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .is_empty());
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
async fn shutdown_waits_for_detached_worker_after_connection_finishes() {
    let (state, _, session) = fixture();
    let (pause, gate) = checkpoint();
    *state.preflight_hooks.after.lock().unwrap() = Some(pause);
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
    bounded(async {
        while state.lifecycle.is_serving() {
            tokio::task::yield_now().await;
        }
    })
    .await;
    assert!(!server.is_finished());
    state.lifecycle.quiesce(); // A repeated shutdown intent cannot skip owner cleanup.
    assert!(!server.is_finished());
    gate.release.send(()).unwrap();
    assert!(bounded(server).await.unwrap().is_ok());
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
