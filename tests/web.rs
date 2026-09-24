use async_trait::async_trait;
use axum::{
    body::Body,
    http::{header, Request, StatusCode},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use http_body_util::BodyExt;
use possums::{
    attestation::{EvidenceError, EvidenceVerifier, GatewayEvidence},
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
use tokio::sync::Notify;
use tower::ServiceExt;

const EXPECTED_RELEASE: &str = "release";
const EXPECTED_KEY: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

struct TestEvidenceVerifier;

impl EvidenceVerifier for TestEvidenceVerifier {
    fn verify(&self, evidence: &GatewayEvidence, now: u64) -> Result<(), EvidenceError> {
        if evidence.quote != serde_json::json!({"verified": true})
            || evidence.release_digest != EXPECTED_RELEASE
            || evidence.endpoint_key_sha256 != EXPECTED_KEY
            || evidence.issued_at_unix > now
            || now - evidence.issued_at_unix > 300
        {
            return Err(EvidenceError::Invalid);
        }
        Ok(())
    }
}

struct BlockingInference {
    started: Arc<Notify>,
}

#[async_trait]
impl Inference for BlockingInference {
    async fn catalog(&self) -> Result<Vec<u8>, InferenceError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Ok(format!(r#"{{"issued_at_unix":{now},"models":[{{"id":"m","context_tokens":20,"max_output_tokens":10,"input_microunits_per_token":1,"output_microunits_per_token":1}}]}}"#).into_bytes())
    }

    async fn count_tokens(&self, _: &str, _: &[Message]) -> Result<u64, InferenceError> {
        self.started.notify_one();
        std::future::pending().await
    }

    async fn generate(&self, _: &Model, _: &[Message]) -> Result<Generation, InferenceError> {
        unreachable!()
    }

    fn verification_document(&self) -> Result<serde_json::Value, InferenceError> {
        Ok(serde_json::json!({"verified": true}))
    }
}

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
) -> (AppState, String, String, String, Arc<FakeInference>) {
    let credential = URL_SAFE_NO_PAD.encode([7_u8; 32]);
    let hash = URL_SAFE_NO_PAD.encode(Sha256::digest(credential.as_bytes()));
    let auth = Auth::from_json(&format!(
        r#"[{{"id":"a","credential_sha256":"{hash}","demo_microunits":{budget}}}]"#
    ))
    .unwrap();
    let challenge = auth.issue_login_challenge().unwrap();
    let (session_id, session) = auth.authenticate(&credential, &challenge).unwrap();
    let submission_token = auth.issue_submission(&session_id).unwrap();
    let inference = Arc::new(FakeInference {
        tokenizations: AtomicUsize::new(0),
        generations: AtomicUsize::new(0),
    });
    let state = AppState::new(
        auth,
        inference.clone(),
        Arc::<str>::from(evidence),
        Arc::new(TestEvidenceVerifier),
    );
    (
        state,
        session_cookie(&session_id),
        session.csrf,
        submission_token,
        inference,
    )
}

fn fixture(evidence: &str) -> (AppState, String, String, String, Arc<FakeInference>) {
    fixture_with_budget(evidence, 100)
}

fn write_evidence(
    path: &std::path::Path,
    quote: serde_json::Value,
    issued_at_unix: u64,
    release: &str,
    key: &str,
) {
    std::fs::write(
        path,
        serde_json::to_vec(&serde_json::json!({
            "quote": quote,
            "issued_at_unix": issued_at_unix,
            "release_digest": release,
            "endpoint_key_sha256": key,
        }))
        .unwrap(),
    )
    .unwrap();
}

#[tokio::test]
async fn no_javascript_chat_sets_security_headers_and_settles() {
    let path = std::env::temp_dir().join(format!("possums-web-evidence-{}", std::process::id()));
    write_evidence(
        &path,
        serde_json::json!({"verified": true}),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        EXPECTED_RELEASE,
        EXPECTED_KEY,
    );
    let (state, cookie, csrf, token, inference) = fixture(path.to_str().unwrap());
    let body = format!("csrf={csrf}&token={token}&model=m&history=%5B%5D&prompt=hello");
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
    assert_eq!(state.accounting.available("a"), Some(74));

    let carried_history = "%5B%7B%22role%22%3A%22user%22%2C%22content%22%3A%22hello%22%7D%2C%7B%22role%22%3A%22assistant%22%2C%22content%22%3A%22%2A%2Asafe%2A%2A%22%7D%5D";
    let confirmation = format!("csrf={csrf}&token={token}&model=m&history={carried_history}");
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/confirm")
                .header(header::COOKIE, cookie.split(';').next().unwrap())
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(confirmation))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(state.accounting.available("a"), Some(97));
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8_lossy(&body);
    assert!(html.contains("<strong>safe</strong>"));
    let marker = "name=token value=\"";
    let start = html.find(marker).unwrap() + marker.len();
    let next_token = &html[start..html[start..].find('\"').unwrap() + start];
    let next_body =
        format!("csrf={csrf}&token={next_token}&model=m&history={carried_history}&prompt=again");
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/chat")
                .header(header::COOKIE, cookie.split(';').next().unwrap())
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(next_body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(inference.generations.load(Ordering::SeqCst), 2);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn recovery_download_is_authenticated_and_never_cached() {
    let (state, cookie, _, _, _) = fixture("/unused/evidence");
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .uri("/recovery/download")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = router(state)
        .oneshot(
            Request::builder()
                .uri("/recovery/download")
                .header(header::COOKIE, cookie.split(';').next().unwrap())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(
        response.headers()[header::CONTENT_DISPOSITION],
        "attachment; filename=\"possums-recovery.txt\""
    );
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body.as_ref(), URL_SAFE_NO_PAD.encode([7_u8; 32]).as_bytes());
}

#[tokio::test]
async fn oversized_request_body_is_rejected_before_upstream_calls() {
    let (state, cookie, _, _, inference) = fixture("/unused/evidence");
    let response = router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/chat")
                .header(header::COOKIE, cookie.split(';').next().unwrap())
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(vec![b'x'; 8 * 1024 * 1024 + 1]))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(inference.tokenizations.load(Ordering::SeqCst), 0);
    assert_eq!(inference.generations.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn missing_gateway_evidence_stops_before_upstream_calls() {
    let (state, cookie, csrf, token, inference) = fixture("/missing/evidence");
    let body = format!("csrf={csrf}&token={token}&model=m&history=%5B%5D&prompt=canary");
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
async fn unverified_gateway_evidence_stops_before_prompt_calls() {
    let cases = [
        (serde_json::json!({}), now(), EXPECTED_RELEASE, EXPECTED_KEY),
        (
            serde_json::json!({"verified": true}),
            now() - 301,
            EXPECTED_RELEASE,
            EXPECTED_KEY,
        ),
        (
            serde_json::json!({"verified": true}),
            now(),
            "wrong-release",
            EXPECTED_KEY,
        ),
        (
            serde_json::json!({"verified": true}),
            now(),
            EXPECTED_RELEASE,
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        ),
    ];

    for (index, (quote, issued_at, release, key)) in cases.into_iter().enumerate() {
        let path = std::env::temp_dir().join(format!(
            "possums-invalid-evidence-{}-{index}",
            std::process::id()
        ));
        write_evidence(&path, quote, issued_at, release, key);
        let (state, cookie, csrf, token, inference) = fixture(path.to_str().unwrap());
        let body = format!("csrf={csrf}&token={token}&model=m&history=%5B%5D&prompt=canary");
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
        std::fs::remove_file(path).unwrap();
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[tokio::test]
async fn cancelled_request_refunds_reservation_and_releases_concurrency() {
    let path = std::env::temp_dir().join(format!("possums-cancel-evidence-{}", std::process::id()));
    write_evidence(
        &path,
        serde_json::json!({"verified": true}),
        now(),
        EXPECTED_RELEASE,
        EXPECTED_KEY,
    );
    let credential = URL_SAFE_NO_PAD.encode([7_u8; 32]);
    let hash = URL_SAFE_NO_PAD.encode(Sha256::digest(credential.as_bytes()));
    let auth = Auth::from_json(&format!(
        r#"[{{"id":"a","credential_sha256":"{hash}","demo_microunits":100}}]"#
    ))
    .unwrap();
    let challenge = auth.issue_login_challenge().unwrap();
    let (session_id, session) = auth.authenticate(&credential, &challenge).unwrap();
    let token = auth.issue_submission(&session_id).unwrap();
    let started = Arc::new(Notify::new());
    let inference = Arc::new(BlockingInference {
        started: started.clone(),
    });
    let state = AppState::new(
        auth,
        inference,
        Arc::<str>::from(path.to_str().unwrap()),
        Arc::new(TestEvidenceVerifier),
    );
    let body = format!(
        "csrf={}&token={token}&model=m&history=%5B%5D&prompt=hello",
        session.csrf
    );
    let request = Request::builder()
        .method("POST")
        .uri("/chat")
        .header(
            header::COOKIE,
            session_cookie(&session_id).split(';').next().unwrap(),
        )
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(body))
        .unwrap();
    let task = tokio::spawn(router(state.clone()).oneshot(request));
    started.notified().await;
    assert_eq!(state.accounting.available("a"), Some(74));
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert_eq!(state.accounting.available("a"), Some(100));
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn failed_reservation_stops_before_prompt_tokenization() {
    let path = std::env::temp_dir().join(format!(
        "possums-low-budget-evidence-{}",
        std::process::id()
    ));
    write_evidence(
        &path,
        serde_json::json!({"verified": true}),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        EXPECTED_RELEASE,
        EXPECTED_KEY,
    );
    let (state, cookie, csrf, token, inference) = fixture_with_budget(path.to_str().unwrap(), 1);
    let body = format!("csrf={csrf}&token={token}&model=m&history=%5B%5D&prompt=canary");
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
