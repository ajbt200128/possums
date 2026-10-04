use async_trait::async_trait;
use axum::{
    body::Body,
    http::{header, Request, StatusCode},
    response::Response,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use http_body_util::BodyExt;
use possums::{
    attestation::{EvidenceError, EvidenceVerifier, GatewayEvidence},
    auth::Auth,
    catalog::Model,
    inference::{
        stream::{ProtocolParser, StreamCompletion},
        Inference, InferenceError, Message,
    },
    web::{router, AppState, BODY_LIMIT},
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::{Notify, OwnedSemaphorePermit};
use tower::ServiceExt;

struct Provider {
    tokens: AtomicU64,
    tokenizer_calls: AtomicUsize,
    generation_calls: AtomicUsize,
    output_allowance: AtomicU64,
    price: AtomicU64,
    valid_catalog: AtomicBool,
    valid_evidence: AtomicBool,
    upstream_error: AtomicBool,
    invalid_usage: AtomicBool,
    overflow_delivery: AtomicBool,
    hold_tokenizer: AtomicBool,
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
        Ok(json!({"object":"list","data":[{"id":"m","type":"chat","context_window":20,"endpoints":["/v1/chat/completions"],"pricing":{"inputTokenPricePer1M":price,"outputTokenPricePer1M":price,"requestPrice":0}}]}).to_string().into_bytes())
    }
    async fn count_tokens(
        &self,
        _: &str,
        _: &[Message],
        _: Arc<OwnedSemaphorePermit>,
    ) -> Result<u64, InferenceError> {
        self.tokenizer_calls.fetch_add(1, Ordering::SeqCst);
        self.tokenizer_started.notify_one();
        if self.hold_tokenizer.load(Ordering::SeqCst) {
            self.tokenizer_release.notified().await;
        }
        Ok(self.tokens.load(Ordering::SeqCst))
    }
    async fn generate_completion_stream(
        &self,
        model: &Model,
        _: &[Message],
        _: Arc<OwnedSemaphorePermit>,
        on_delta: &mut (dyn for<'d> FnMut(&'d str) + Send),
    ) -> Result<StreamCompletion, InferenceError> {
        self.generation_calls.fetch_add(1, Ordering::SeqCst);
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
        let result = if self.upstream_error.load(Ordering::SeqCst) {
            Err(InferenceError::Unavailable)
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
        tokens: 2.into(),
        tokenizer_calls: 0.into(),
        generation_calls: 0.into(),
        output_allowance: 0.into(),
        price: 1.into(),
        valid_catalog: true.into(),
        valid_evidence: true.into(),
        upstream_error: false.into(),
        invalid_usage: false.into(),
        overflow_delivery: false.into(),
        hold_tokenizer: false.into(),
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
    let (state, provider, bearer, submission) = fixture(5_000_000);
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
    provider.price.store(100, Ordering::SeqCst); // Settlement must use the original rate.
    provider.finish.notify_one();
    let bytes = body.collect().await.unwrap().to_bytes();
    terminal(&state, 5_000_000 - 7).await;
    let text = std::str::from_utf8(&bytes).unwrap();
    assert!(text.contains("\"finish_reason\":\"length\""));
    assert!(text.contains("\"total_tokens\":5"));
    assert!(text.contains("\"charged_microunits\":\"7\""));
    assert!(text.ends_with("data: [DONE]\n\n"));
    duplicate(&state, &bearer, &submission, "settled").await;
    assert_eq!(provider.tokenizer_calls.load(Ordering::SeqCst), 1);
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
        json!({"model":"m","stream":true,"submission":submission,"messages":vec![json!({"role":"user","content":"synthetic"});4097]}),
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
