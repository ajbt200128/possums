use async_trait::async_trait;
use axum::{
    body::Body,
    http::{header, Request, StatusCode},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use http_body_util::BodyExt;
use possums::{
    auth::{session_cookie, Auth},
    catalog::Model,
    inference::{Generation, Inference, InferenceError, Message},
    web::{router, AppState},
};
use sha2::{Digest, Sha256};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tower::ServiceExt;

struct FakeInference {
    tokenizations: AtomicUsize,
    generations: AtomicUsize,
}

#[async_trait]
impl Inference for FakeInference {
    async fn catalog(&self) -> Result<Vec<u8>, InferenceError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Ok(format!(r#"{{"issued_at_unix":{now},"models":[{{"id":"m","context_tokens":20,"max_output_tokens":10,"input_microunits_per_token":1,"output_microunits_per_token":1}}]}}"#).into_bytes())
    }

    async fn count_tokens(&self, _: &str, _: &[Message]) -> Result<u64, InferenceError> {
        self.tokenizations.fetch_add(1, Ordering::SeqCst);
        Ok(1)
    }

    async fn generate(&self, _: &Model, _: &[Message]) -> Result<Generation, InferenceError> {
        self.generations.fetch_add(1, Ordering::SeqCst);
        Ok(Generation {
            content: "**safe**".into(),
            input_tokens: 1,
            output_tokens: 1,
        })
    }

    fn verification_document(&self) -> Result<serde_json::Value, InferenceError> {
        Ok(serde_json::json!({"verified": true}))
    }
}

fn fixture_with_budget(
    evidence: &str,
    budget: u64,
) -> (AppState, String, String, Arc<FakeInference>) {
    let credential = URL_SAFE_NO_PAD.encode([7_u8; 32]);
    let hash = URL_SAFE_NO_PAD.encode(Sha256::digest(credential.as_bytes()));
    let auth = Auth::from_json(&format!(
        r#"[{{"id":"a","credential_sha256":"{hash}","demo_microunits":{budget}}}]"#
    ))
    .unwrap();
    let (session_id, session) = auth.authenticate(&credential).unwrap();
    let inference = Arc::new(FakeInference {
        tokenizations: AtomicUsize::new(0),
        generations: AtomicUsize::new(0),
    });
    let state = AppState::new(auth, inference.clone(), Arc::<str>::from(evidence));
    (state, session_cookie(&session_id), session.csrf, inference)
}

fn fixture(evidence: &str) -> (AppState, String, String, Arc<FakeInference>) {
    fixture_with_budget(evidence, 100)
}

#[tokio::test]
async fn no_javascript_chat_sets_security_headers_and_settles() {
    let path = std::env::temp_dir().join(format!("possums-web-evidence-{}", std::process::id()));
    std::fs::write(
        &path,
        format!(
            r#"{{"quote":{{}},"release_digest":"release","endpoint_key_sha256":"{}"}}"#,
            "a".repeat(64)
        ),
    )
    .unwrap();
    let (state, cookie, csrf, inference) = fixture(path.to_str().unwrap());
    let body = format!(
        "csrf={csrf}&token={}&model=m&history=%5B%5D&prompt=hello",
        "t".repeat(32)
    );
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/chat")
                .header(header::COOKIE, cookie.split(';').next().unwrap())
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert!(response.headers()[header::CONTENT_SECURITY_POLICY]
        .to_str()
        .unwrap()
        .contains("default-src 'none'"));
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert!(String::from_utf8_lossy(&body).contains("<strong>safe</strong>"));
    assert_eq!(inference.generations.load(Ordering::SeqCst), 1);
    assert_eq!(state.accounting.available("a"), Some(97));
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn missing_gateway_evidence_stops_before_upstream_calls() {
    let (state, cookie, csrf, inference) = fixture("/missing/evidence");
    let body = format!(
        "csrf={csrf}&token={}&model=m&history=%5B%5D&prompt=canary",
        "t".repeat(32)
    );
    let response = router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/chat")
                .header(header::COOKIE, cookie.split(';').next().unwrap())
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(inference.tokenizations.load(Ordering::SeqCst), 0);
    assert_eq!(inference.generations.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn failed_reservation_stops_before_prompt_tokenization() {
    let path = std::env::temp_dir().join(format!(
        "possums-low-budget-evidence-{}",
        std::process::id()
    ));
    std::fs::write(
        &path,
        format!(
            r#"{{"quote":{{}},"release_digest":"release","endpoint_key_sha256":"{}"}}"#,
            "a".repeat(64)
        ),
    )
    .unwrap();
    let (state, cookie, csrf, inference) = fixture_with_budget(path.to_str().unwrap(), 1);
    let body = format!(
        "csrf={csrf}&token={}&model=m&history=%5B%5D&prompt=canary",
        "t".repeat(32)
    );
    let response = router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/chat")
                .header(header::COOKIE, cookie.split(';').next().unwrap())
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYMENT_REQUIRED);
    assert_eq!(inference.tokenizations.load(Ordering::SeqCst), 0);
    assert_eq!(inference.generations.load(Ordering::SeqCst), 0);
    std::fs::remove_file(path).unwrap();
}
