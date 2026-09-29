use async_trait::async_trait;
use axum::{
    body::Body,
    http::{header, Request, StatusCode},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use http_body_util::BodyExt;
use possums::{
    attestation::{load_evidence, EvidenceError, EvidenceVerifier, GatewayEvidence},
    auth::{session_cookie, Auth},
    catalog::Model,
    inference::{Generation, Inference, InferenceError, Message},
    web::{router, serve, AppState},
};
use sha2::{Digest, Sha256};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Notify,
    time::{sleep, Duration},
};
use tower::ServiceExt;

const EXPECTED_RELEASE: &str = "release";
const EXPECTED_KEY: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

struct TestEvidenceVerifier;

#[async_trait]
impl EvidenceVerifier for TestEvidenceVerifier {
    async fn verify(
        &self,
        evidence_path: &str,
        now: u64,
    ) -> Result<GatewayEvidence, EvidenceError> {
        let evidence = load_evidence(evidence_path)?;
        if evidence.quote != serde_json::json!({"verified": true})
            || evidence.release_digest != EXPECTED_RELEASE
            || evidence.endpoint_key_sha256 != EXPECTED_KEY
            || evidence.issued_at_unix > now
            || now - evidence.issued_at_unix > 300
            || evidence.freshness_expires_at_unix <= now
        {
            return Err(EvidenceError::Invalid);
        }
        Ok(evidence)
    }
}

fn catalog_bytes() -> Vec<u8> {
    br#"{"object":"list","data":[{"id":"m","type":"chat","context_window":20,"endpoints":["/v1/chat/completions"],"pricing":{"inputTokenPricePer1M":1,"outputTokenPricePer1M":1,"requestPrice":0}}]}"#.to_vec()
}

struct BlockingInference {
    started: Arc<Notify>,
}

#[async_trait]
impl Inference for BlockingInference {
    async fn catalog(&self) -> Result<Vec<u8>, InferenceError> {
        Ok(catalog_bytes())
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

struct LargeResponseInference;

#[async_trait]
impl Inference for LargeResponseInference {
    async fn catalog(&self) -> Result<Vec<u8>, InferenceError> {
        Ok(catalog_bytes())
    }

    async fn count_tokens(&self, _: &str, _: &[Message]) -> Result<u64, InferenceError> {
        Ok(1)
    }

    async fn generate(&self, _: &Model, _: &[Message]) -> Result<Generation, InferenceError> {
        Ok(Generation {
            content: "x".repeat(200 * 1024),
            input_tokens: 1,
            output_tokens: 1,
        })
    }

    fn verification_document(&self) -> Result<serde_json::Value, InferenceError> {
        Ok(serde_json::json!({"verified": true}))
    }
}

struct PanicInference;

#[async_trait]
impl Inference for PanicInference {
    async fn catalog(&self) -> Result<Vec<u8>, InferenceError> {
        Ok(catalog_bytes())
    }

    async fn count_tokens(&self, _: &str, _: &[Message]) -> Result<u64, InferenceError> {
        panic!("injected post-reservation panic")
    }

    async fn generate(&self, _: &Model, _: &[Message]) -> Result<Generation, InferenceError> {
        unreachable!()
    }

    fn verification_document(&self) -> Result<serde_json::Value, InferenceError> {
        Ok(serde_json::json!({"verified": true}))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HoldPoint {
    Catalog,
    Tokenization,
}

#[derive(Default)]
struct Gate {
    entered: Notify,
    release: Notify,
}

struct FakeInference {
    hold: std::sync::Mutex<Option<(HoldPoint, Arc<Gate>)>>,
    tokenizations: AtomicUsize,
    generations: AtomicUsize,
    catalog_failures: AtomicUsize,
    catalog_override: std::sync::Mutex<Option<Vec<u8>>>,
}

impl FakeInference {
    fn hold(&self, point: HoldPoint) -> Arc<Gate> {
        let gate = Arc::new(Gate::default());
        *self.hold.lock().unwrap() = Some((point, gate.clone()));
        gate
    }

    async fn pause_at(&self, point: HoldPoint) {
        let gate = {
            let mut hold = self.hold.lock().unwrap();
            if hold
                .as_ref()
                .is_some_and(|(expected, _)| *expected == point)
            {
                hold.take().map(|(_, gate)| gate)
            } else {
                None
            }
        };
        if let Some(gate) = gate {
            gate.entered.notify_one();
            gate.release.notified().await;
        }
    }
}

#[async_trait]
impl Inference for FakeInference {
    async fn catalog(&self) -> Result<Vec<u8>, InferenceError> {
        self.pause_at(HoldPoint::Catalog).await;
        if self
            .catalog_failures
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                remaining.checked_sub(1)
            })
            .is_ok()
        {
            return Err(InferenceError::Unavailable);
        }
        Ok(self
            .catalog_override
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(catalog_bytes))
    }

    async fn count_tokens(&self, _: &str, _: &[Message]) -> Result<u64, InferenceError> {
        self.tokenizations.fetch_add(1, Ordering::SeqCst);
        self.pause_at(HoldPoint::Tokenization).await;
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
        hold: std::sync::Mutex::new(None),
        tokenizations: AtomicUsize::new(0),
        generations: AtomicUsize::new(0),
        catalog_failures: AtomicUsize::new(0),
        catalog_override: std::sync::Mutex::new(None),
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
            "freshness_expires_at_unix": issued_at_unix.saturating_add(300),
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
    let html = String::from_utf8_lossy(&body);
    assert!(html.contains("<strong>safe</strong>"));
    assert!(!html.contains("Confirm response delivery"));
    assert_eq!(inference.generations.load(Ordering::SeqCst), 1);
    assert_eq!(state.accounting.available("a"), Some(97));

    let carried_history = "%5B%7B%22role%22%3A%22user%22%2C%22content%22%3A%22hello%22%7D%2C%7B%22role%22%3A%22assistant%22%2C%22content%22%3A%22%2A%2Asafe%2A%2A%22%7D%5D";
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
async fn dropping_unconsumed_response_refunds_reservation() {
    let path = std::env::temp_dir().join(format!(
        "possums-abandoned-response-evidence-{}",
        std::process::id()
    ));
    write_evidence(
        &path,
        serde_json::json!({"verified": true}),
        now(),
        EXPECTED_RELEASE,
        EXPECTED_KEY,
    );
    let (state, cookie, csrf, token, inference) = fixture(path.to_str().unwrap());
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/chat")
                .header(header::COOKIE, cookie.split(';').next().unwrap())
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(format!(
                    "csrf={csrf}&token={token}&model=m&history=%5B%5D&prompt=hello"
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(state.accounting.available("a"), Some(48));
    drop(response);
    assert_eq!(state.accounting.available("a"), Some(100));
    assert_eq!(inference.generations.load(Ordering::SeqCst), 1);
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
async fn shared_memory_admission_is_fail_fast_and_held_until_response_drop() {
    let (state, cookie, _, _, _) = fixture("/unused/evidence");
    let app = router(state);
    let mut responses = Vec::new();
    for _ in 0..4 {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header(header::COOKIE, cookie.split(';').next().unwrap())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        responses.push(response);
    }

    let overloaded = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/")
                .header(header::COOKIE, cookie.split(';').next().unwrap())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(overloaded.status(), StatusCode::SERVICE_UNAVAILABLE);

    drop(responses.pop());
    let admitted = app
        .oneshot(
            Request::builder()
                .uri("/")
                .header(header::COOKIE, cookie.split(';').next().unwrap())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(admitted.status(), StatusCode::OK);
}

#[tokio::test]
async fn concurrent_maximum_bodies_are_bounded_and_release_memory_on_drop() {
    let (state, _, _, _, _) = fixture("/unused/evidence");
    let app = router(state);
    let mut responses = Vec::new();
    for _ in 0..4 {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/chat")
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from(vec![b'x'; 8 * 1024 * 1024]))
                    .unwrap(),
            )
            .await
            .unwrap();
        responses.push(response);
    }
    let overloaded = app
        .clone()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(overloaded.status(), StatusCode::SERVICE_UNAVAILABLE);

    drop(responses.pop());
    let admitted = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(admitted.status(), StatusCode::OK);
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
async fn replay_after_reauthentication_stops_before_prompt_calls() {
    let path = std::env::temp_dir().join(format!("possums-replay-evidence-{}", std::process::id()));
    write_evidence(
        &path,
        serde_json::json!({"verified": true}),
        now(),
        EXPECTED_RELEASE,
        EXPECTED_KEY,
    );
    let (state, _, _, token, inference) = fixture(path.to_str().unwrap());
    let credential = URL_SAFE_NO_PAD.encode([7_u8; 32]);
    let challenge = state.auth.issue_login_challenge().unwrap();
    let (new_session_id, new_session) = state.auth.authenticate(&credential, &challenge).unwrap();
    let body = format!(
        "csrf={}&token={token}&model=m&history=%5B%5D&prompt=canary",
        new_session.csrf
    );
    let response = router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/chat")
                .header(
                    header::COOKIE,
                    session_cookie(&new_session_id).split(';').next().unwrap(),
                )
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(inference.tokenizations.load(Ordering::SeqCst), 0);
    assert_eq!(inference.generations.load(Ordering::SeqCst), 0);
    std::fs::remove_file(path).unwrap();
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

fn post_form(uri: &str, cookie: &str, body: String) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::COOKIE, cookie.split(';').next().unwrap())
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(body))
        .unwrap()
}

fn session_id(cookie: &str) -> &str {
    cookie.split(';').next().unwrap().split_once('=').unwrap().1
}

#[tokio::test]
async fn new_chat_requires_csrf_and_redirects_to_empty_history_in_same_session() {
    let (state, cookie, csrf, _, inference) = fixture("/unused/evidence");
    let id = session_id(&cookie);
    let original = state.auth.session(id).unwrap();
    for (body, expected) in [
        (String::new(), StatusCode::UNPROCESSABLE_ENTITY),
        ("csrf=forged".into(), StatusCode::UNAUTHORIZED),
    ] {
        let response = router(state.clone())
            .oneshot(post_form("/chat/new", &cookie, body))
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        assert_eq!(
            state.auth.session(id).unwrap().conversation,
            original.conversation
        );
    }
    let response = router(state.clone())
        .oneshot(post_form("/chat/new", &cookie, format!("csrf={csrf}")))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers()[header::LOCATION], "/");
    assert!(!response.headers().contains_key(header::SET_COOKIE));
    assert_ne!(
        state.auth.session(id).unwrap().conversation,
        original.conversation
    );
    drop(response);
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .uri("/")
                .header(header::COOKIE, cookie.split(';').next().unwrap())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8_lossy(&body);
    assert!(html.contains("name=history value=\"[]\""));
    assert_eq!(state.accounting.available("a"), Some(100));
    assert_eq!(inference.tokenizations.load(Ordering::SeqCst), 0);
    assert_eq!(inference.generations.load(Ordering::SeqCst), 0);
}

fn valid_evidence(label: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("possums-{label}-{}", std::process::id()));
    write_evidence(
        &path,
        serde_json::json!({"verified": true}),
        now(),
        EXPECTED_RELEASE,
        EXPECTED_KEY,
    );
    path
}

fn two_model_catalog(inference: &FakeInference) {
    let mut catalog: serde_json::Value = serde_json::from_slice(&catalog_bytes()).unwrap();
    let mut other = catalog["data"][0].clone();
    other["id"] = serde_json::json!("n");
    other["pricing"]["inputTokenPricePer1M"] = serde_json::json!(0.1);
    other["pricing"]["outputTokenPricePer1M"] = serde_json::json!(0.1);
    catalog["data"].as_array_mut().unwrap().push(other);
    *inference.catalog_override.lock().unwrap() = Some(serde_json::to_vec(&catalog).unwrap());
}

fn chat_request(cookie: &str, csrf: &str, token: &str) -> Request<Body> {
    post_form(
        "/chat",
        cookie,
        format!("csrf={csrf}&token={token}&model=m&history=%5B%5D&prompt=hello"),
    )
}

#[tokio::test]
async fn reset_and_logout_before_admission_prevent_all_prompt_transmission() {
    let path = valid_evidence("reset-preflight");
    for control in ["/chat/new", "/logout"] {
        let (state, cookie, csrf, token, inference) = fixture(path.to_str().unwrap());
        let gate = inference.hold(HoldPoint::Catalog);
        let request =
            tokio::spawn(router(state.clone()).oneshot(chat_request(&cookie, &csrf, &token)));
        gate.entered.notified().await;
        let reset = router(state.clone())
            .oneshot(post_form(control, &cookie, format!("csrf={csrf}")))
            .await
            .unwrap();
        assert_eq!(reset.status(), StatusCode::SEE_OTHER);
        drop(reset);
        gate.release.notify_one();
        assert_eq!(
            request.await.unwrap().unwrap().status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(state.accounting.available("a"), Some(100));
        assert_eq!(inference.tokenizations.load(Ordering::SeqCst), 0);
        assert_eq!(inference.generations.load(Ordering::SeqCst), 0);
    }
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn home_snapshot_cannot_issue_for_a_conversation_reset_during_catalog_fetch() {
    for control in ["/chat/new", "/logout"] {
        let (state, cookie, csrf, _, inference) = fixture("/unused/evidence");
        let gate = inference.hold(HoldPoint::Catalog);
        let home = tokio::spawn(
            router(state.clone()).oneshot(
                Request::builder()
                    .uri("/")
                    .header(header::COOKIE, cookie.split(';').next().unwrap())
                    .body(Body::empty())
                    .unwrap(),
            ),
        );
        gate.entered.notified().await;
        let reset = router(state.clone())
            .oneshot(post_form(control, &cookie, format!("csrf={csrf}")))
            .await
            .unwrap();
        assert_eq!(reset.status(), StatusCode::SEE_OTHER);
        drop(reset);
        gate.release.notify_one();
        let response = home.await.unwrap().unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert!(!String::from_utf8_lossy(&body).contains("name=token"));
    }
}

#[tokio::test]
async fn accepted_work_continues_after_reset_or_logout_but_cannot_issue_stale_completion() {
    let path = valid_evidence("reset-accepted");
    for control in ["/chat/new", "/logout"] {
        let (state, cookie, csrf, token, inference) = fixture(path.to_str().unwrap());
        let gate = inference.hold(HoldPoint::Tokenization);
        let request =
            tokio::spawn(router(state.clone()).oneshot(chat_request(&cookie, &csrf, &token)));
        gate.entered.notified().await;
        assert_eq!(state.accounting.available("a"), Some(48));
        let reset = router(state.clone())
            .oneshot(post_form(control, &cookie, format!("csrf={csrf}")))
            .await
            .unwrap();
        assert_eq!(reset.status(), StatusCode::SEE_OTHER);
        drop(reset);
        assert_eq!(state.accounting.available("a"), Some(48));
        gate.release.notify_one();
        let response = request.await.unwrap().unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert!(!String::from_utf8_lossy(&body).contains("name=token"));
        assert_eq!(inference.tokenizations.load(Ordering::SeqCst), 1);
        assert_eq!(inference.generations.load(Ordering::SeqCst), 1);
        // Packet 3 intentionally preserves the buffered delivery policy: failed
        // continuation delivery refunds. Packet 6 replaces this with usage settlement.
        assert_eq!(state.accounting.available("a"), Some(100));
        if control == "/chat/new" {
            assert_eq!(
                state
                    .auth
                    .session(session_id(&cookie))
                    .unwrap()
                    .selected_model,
                None
            );
        }
    }
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn reset_after_continuation_insertion_invalidates_token_without_cancelling_delivery() {
    let path = valid_evidence("reset-delivery");
    for control in ["/chat/new", "/logout"] {
        let (state, cookie, csrf, token, inference) = fixture(path.to_str().unwrap());
        let response = router(state.clone())
            .oneshot(chat_request(&cookie, &csrf, &token))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(state.accounting.available("a"), Some(48));
        let reset = router(state.clone())
            .oneshot(post_form(control, &cookie, format!("csrf={csrf}")))
            .await
            .unwrap();
        assert_eq!(reset.status(), StatusCode::SEE_OTHER);
        drop(reset);
        assert_eq!(state.accounting.available("a"), Some(48));
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(state.accounting.available("a"), Some(97));
        let html = String::from_utf8_lossy(&body);
        let continuation = html
            .split("name=token value=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        let stale = router(state.clone())
            .oneshot(chat_request(&cookie, &csrf, continuation))
            .await
            .unwrap();
        assert_eq!(
            stale.status(),
            if control == "/logout" {
                StatusCode::UNAUTHORIZED
            } else {
                StatusCode::BAD_REQUEST
            }
        );
        assert_eq!(inference.generations.load(Ordering::SeqCst), 1);
        assert_eq!(state.accounting.available("a"), Some(97));
    }
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn changed_prompt_and_history_duplicates_never_regenerate_or_reserve_again() {
    let path = valid_evidence("changed-duplicate");
    for deliver in [false, true] {
        let (state, cookie, csrf, token, inference) = fixture(path.to_str().unwrap());
        let gate = inference.hold(HoldPoint::Tokenization);
        let first =
            tokio::spawn(router(state.clone()).oneshot(chat_request(&cookie, &csrf, &token)));
        gate.entered.notified().await;
        let changed_history =
            "%5B%7B%22role%22%3A%22user%22%2C%22content%22%3A%22changed-history%22%7D%5D";
        for _ in 0..6 {
            let response = router(state.clone()).oneshot(post_form("/chat", &cookie, format!("csrf={csrf}&token={token}&model=m&history={changed_history}&prompt=different"))).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let body = response.into_body().collect().await.unwrap().to_bytes();
            assert!(String::from_utf8_lossy(&body).contains("already in progress"));
            assert_eq!(state.accounting.available("a"), Some(48));
        }
        gate.release.notify_one();
        let response = first.await.unwrap().unwrap();
        if deliver {
            response.into_body().collect().await.unwrap();
        } else {
            drop(response);
        }
        let expected = if deliver { 97 } else { 100 };
        for _ in 0..6 {
            let response = router(state.clone()).oneshot(post_form("/chat", &cookie, format!("csrf={csrf}&token={token}&model=m&history={changed_history}&prompt=changed-again"))).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            response.into_body().collect().await.unwrap();
            assert_eq!(state.accounting.available("a"), Some(expected));
        }
        assert_eq!(inference.tokenizations.load(Ordering::SeqCst), 1);
        assert_eq!(inference.generations.load(Ordering::SeqCst), 1);
    }
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn same_session_model_switch_needs_new_chat_and_old_tabs_stay_invalid() {
    let path = valid_evidence("model-switch");
    let (state, cookie, csrf, token, inference) = fixture(path.to_str().unwrap());
    two_model_catalog(&inference);
    for (body, expected) in [
        (
            format!("token={token}&model=m&history=%5B%5D&prompt=hello"),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            format!("csrf=forged&token={token}&model=m&history=%5B%5D&prompt=hello"),
            StatusCode::BAD_REQUEST,
        ),
    ] {
        let response = router(state.clone())
            .oneshot(post_form("/chat", &cookie, body))
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        assert_eq!(inference.tokenizations.load(Ordering::SeqCst), 0);
        assert_eq!(
            state
                .auth
                .session(session_id(&cookie))
                .unwrap()
                .selected_model,
            None
        );
    }
    let response = router(state.clone())
        .oneshot(chat_request(&cookie, &csrf, &token))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8_lossy(&body);
    let stale = html
        .split("name=token value=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    let switch = format!("csrf={csrf}&token={stale}&model=n&history=%5B%5D&prompt=switch");
    let response = router(state.clone())
        .oneshot(post_form("/chat", &cookie, switch.clone()))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    drop(response);
    let response = router(state.clone())
        .oneshot(post_form("/chat/new", &cookie, format!("csrf={csrf}")))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    drop(response);
    let response = router(state.clone())
        .oneshot(post_form("/chat", &cookie, switch))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    drop(response);
    assert_eq!(inference.generations.load(Ordering::SeqCst), 1);
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .uri("/")
                .header(header::COOKIE, cookie.split(';').next().unwrap())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8_lossy(&body);
    assert!(html.contains("name=history value=\"[]\""));
    let fresh = html
        .split("name=token value=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    let response = router(state.clone())
        .oneshot(post_form(
            "/chat",
            &cookie,
            format!("csrf={csrf}&token={fresh}&model=n&history=%5B%5D&prompt=switched"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    response.into_body().collect().await.unwrap();
    assert_eq!(
        state
            .auth
            .session(session_id(&cookie))
            .unwrap()
            .selected_model
            .as_deref(),
        Some("n")
    );
    assert_eq!(inference.generations.load(Ordering::SeqCst), 2);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn account_capacity_rejection_after_reset_commits_no_binding_and_sends_no_prompt() {
    let path = valid_evidence("reset-account-capacity");
    let (state, cookie, csrf, _, inference) = fixture_with_budget(path.to_str().unwrap(), 1000);
    two_model_catalog(&inference);
    let mut pending = Vec::new();
    for _ in 0..3 {
        let token = state.auth.issue_submission(session_id(&cookie)).unwrap();
        let gate = inference.hold(HoldPoint::Tokenization);
        let task =
            tokio::spawn(router(state.clone()).oneshot(chat_request(&cookie, &csrf, &token)));
        gate.entered.notified().await;
        pending.push((gate, task));
    }
    assert_eq!(state.accounting.available("a"), Some(844));
    let response = router(state.clone())
        .oneshot(post_form("/chat/new", &cookie, format!("csrf={csrf}")))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    drop(response);
    assert_eq!(state.accounting.available("a"), Some(844));
    let token = state.auth.issue_submission(session_id(&cookie)).unwrap();
    // Repeated rejection cannot leak the fourth generation permit.
    for _ in 0..6 {
        let response = router(state.clone())
            .oneshot(chat_request(&cookie, &csrf, &token))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PAYMENT_REQUIRED);
        assert_eq!(
            state
                .auth
                .session(session_id(&cookie))
                .unwrap()
                .selected_model,
            None
        );
        assert_eq!(state.accounting.available("a"), Some(844));
        assert_eq!(inference.tokenizations.load(Ordering::SeqCst), 3);
    }
    let (gate, task) = pending.pop().unwrap();
    gate.release.notify_one();
    let response = task.await.unwrap().unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    drop(response);
    assert_eq!(state.accounting.available("a"), Some(896));
    let response = router(state.clone())
        .oneshot(post_form(
            "/chat",
            &cookie,
            format!("csrf={csrf}&token={token}&model=n&history=%5B%5D&prompt=new-conversation"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    response.into_body().collect().await.unwrap();
    for (gate, task) in pending {
        gate.release.notify_one();
        assert_eq!(
            task.await.unwrap().unwrap().status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
    }
    assert_eq!(state.accounting.available("a"), Some(999));
    assert_eq!(
        state
            .auth
            .session(session_id(&cookie))
            .unwrap()
            .selected_model
            .as_deref(),
        Some("n")
    );
    assert_eq!(inference.generations.load(Ordering::SeqCst), 4);
    std::fs::remove_file(path).unwrap();
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
    assert_eq!(state.accounting.available("a"), Some(48));
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert_eq!(state.accounting.available("a"), Some(100));
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn socket_disconnect_during_inference_refunds_reservation() {
    let path = std::env::temp_dir().join(format!(
        "possums-disconnect-evidence-{}",
        std::process::id()
    ));
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
    let state = AppState::new(
        auth,
        Arc::new(BlockingInference {
            started: started.clone(),
        }),
        Arc::<str>::from(path.to_str().unwrap()),
        Arc::new(TestEvidenceVerifier),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(serve(listener, state.clone()));
    let body = format!(
        "csrf={}&token={token}&model=m&history=%5B%5D&prompt=hello",
        session.csrf
    );
    let request = format!(
        "POST /chat HTTP/1.1\r\nHost: local\r\nCookie: {}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n{}",
        session_cookie(&session_id).split(';').next().unwrap(),
        body.len(),
        body
    );
    let mut client = TcpStream::connect(address).await.unwrap();
    client.write_all(request.as_bytes()).await.unwrap();
    started.notified().await;
    assert_eq!(state.accounting.available("a"), Some(48));
    drop(client);

    for _ in 0..100 {
        if state.accounting.available("a") == Some(100) {
            break;
        }
        sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(state.accounting.available("a"), Some(100));
    server.abort();
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn disconnect_near_transport_completion_reaches_one_terminal_balance() {
    let path = std::env::temp_dir().join(format!(
        "possums-rendered-disconnect-evidence-{}",
        std::process::id()
    ));
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
    let state = AppState::new(
        auth,
        Arc::new(LargeResponseInference),
        Arc::<str>::from(path.to_str().unwrap()),
        Arc::new(TestEvidenceVerifier),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(serve(listener, state.clone()));
    let body = format!(
        "csrf={}&token={token}&model=m&history=%5B%5D&prompt=hello",
        session.csrf
    );
    let request = format!(
        "POST /chat HTTP/1.1\r\nHost: local\r\nCookie: {}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n{}",
        session_cookie(&session_id).split(';').next().unwrap(),
        body.len(),
        body
    );
    let mut client = TcpStream::connect(address).await.unwrap();
    client.write_all(request.as_bytes()).await.unwrap();
    let mut received = Vec::new();
    while !received.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
        let mut chunk = [0; 1024];
        let count = client.read(&mut chunk).await.unwrap();
        assert_ne!(count, 0);
        received.extend_from_slice(&chunk[..count]);
    }
    assert!(matches!(state.accounting.available("a"), Some(48 | 97)));
    drop(client);

    for _ in 0..100 {
        if matches!(state.accounting.available("a"), Some(97 | 100)) {
            break;
        }
        sleep(Duration::from_millis(10)).await;
    }
    assert!(matches!(state.accounting.available("a"), Some(97 | 100)));
    server.abort();
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn panic_after_reservation_refunds_before_error_response() {
    let path = std::env::temp_dir().join(format!("possums-panic-evidence-{}", std::process::id()));
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
    let state = AppState::new(
        auth,
        Arc::new(PanicInference),
        Arc::<str>::from(path.to_str().unwrap()),
        Arc::new(TestEvidenceVerifier),
    );
    let body = format!(
        "csrf={}&token={token}&model=m&history=%5B%5D&prompt=hello",
        session.csrf
    );
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/chat")
                .header(
                    header::COOKIE,
                    session_cookie(&session_id).split(';').next().unwrap(),
                )
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
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
    // Old endpoint-only quote was 26; the full operational reservation is 52.
    let (state, cookie, csrf, token, inference) = fixture_with_budget(path.to_str().unwrap(), 51);
    let body = format!("csrf={csrf}&token={token}&model=m&history=%5B%5D&prompt=canary");
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
    assert_eq!(response.status(), StatusCode::PAYMENT_REQUIRED);
    assert_eq!(state.accounting.available("a"), Some(51));
    assert_eq!(inference.tokenizations.load(Ordering::SeqCst), 0);
    assert_eq!(inference.generations.load(Ordering::SeqCst), 0);
    assert_eq!(
        state
            .auth
            .session(session_id(&cookie))
            .unwrap()
            .selected_model,
        None
    );
    drop(response);
    // An explicit user-selected cheaper model can use the same uncommitted
    // token. The gateway did not substitute or reduce the original request.
    two_model_catalog(&inference);
    let response = router(state.clone())
        .oneshot(post_form(
            "/chat",
            &cookie,
            format!("csrf={csrf}&token={token}&model=n&history=%5B%5D&prompt=retry"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    response.into_body().collect().await.unwrap();
    assert_eq!(
        state
            .auth
            .session(session_id(&cookie))
            .unwrap()
            .selected_model
            .as_deref(),
        Some("n")
    );
    assert_eq!(inference.generations.load(Ordering::SeqCst), 1);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn invalid_catalog_and_unrepresentable_quote_stop_before_prompt_calls() {
    let path = std::env::temp_dir().join(format!("possums-quote-evidence-{}", std::process::id()));
    write_evidence(
        &path,
        serde_json::json!({"verified": true}),
        now(),
        EXPECTED_RELEASE,
        EXPECTED_KEY,
    );
    for (context, extra_fee, expected) in [
        (
            serde_json::Value::Null,
            false,
            StatusCode::SERVICE_UNAVAILABLE,
        ),
        (serde_json::json!(0), false, StatusCode::SERVICE_UNAVAILABLE),
        (serde_json::json!(20), true, StatusCode::SERVICE_UNAVAILABLE),
        (serde_json::json!(u64::MAX), false, StatusCode::BAD_REQUEST),
    ] {
        let (state, cookie, csrf, token, inference) = fixture(path.to_str().unwrap());
        let mut catalog: serde_json::Value = serde_json::from_slice(&catalog_bytes()).unwrap();
        catalog["data"][0]["context_window"] = context;
        if extra_fee {
            catalog["data"][0]["pricing"]["unknownFee"] = serde_json::json!(1);
        }
        *inference.catalog_override.lock().unwrap() = Some(serde_json::to_vec(&catalog).unwrap());
        let response = router(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/chat")
                    .header(header::COOKIE, cookie.split(';').next().unwrap())
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from(format!(
                        "csrf={csrf}&token={token}&model=m&history=%5B%5D&prompt=canary"
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        assert_eq!(state.accounting.available("a"), Some(100));
        assert_eq!(inference.tokenizations.load(Ordering::SeqCst), 0);
        assert_eq!(inference.generations.load(Ordering::SeqCst), 0);
        assert_eq!(
            state
                .auth
                .session(session_id(&cookie))
                .unwrap()
                .selected_model,
            None
        );
        drop(response);
        two_model_catalog(&inference);
        let response = router(state.clone())
            .oneshot(post_form(
                "/chat",
                &cookie,
                format!("csrf={csrf}&token={token}&model=n&history=%5B%5D&prompt=retry"),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        response.into_body().collect().await.unwrap();
        assert_eq!(inference.generations.load(Ordering::SeqCst), 1);
    }
    std::fs::remove_file(path).unwrap();
}
