use async_trait::async_trait;
use axum::{
    body::Body,
    http::{header, Request, StatusCode},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
// Drain and release each frame like a transport. BodyExt::collect retains frame
// owners until EOF, intentionally exhausting the eight-outstanding-frame budget.
trait DrainBody {
    async fn collect(self) -> Result<Drained, axum::Error>;
    async fn frame(&mut self) -> Option<Result<http_body::Frame<axum::body::Bytes>, axum::Error>>;
}
struct Drained(axum::body::Bytes);
impl Drained {
    fn to_bytes(&self) -> axum::body::Bytes {
        self.0.clone()
    }
}
impl DrainBody for Body {
    async fn collect(mut self) -> Result<Drained, axum::Error> {
        let mut bytes = Vec::new();
        while let Some(frame) = http_body_util::BodyExt::frame(&mut self).await {
            if let Ok(data) = frame?.into_data() {
                bytes.extend_from_slice(&data);
            }
        }
        Ok(Drained(bytes.into()))
    }
    async fn frame(&mut self) -> Option<Result<http_body::Frame<axum::body::Bytes>, axum::Error>> {
        http_body_util::BodyExt::frame(self).await
    }
}
use possums::{
    attestation::{load_evidence, EvidenceError, EvidenceVerifier, GatewayEvidence},
    auth::{session_cookie, Auth},
    catalog::Model,
    inference::{stream, Inference, InferenceError, Message},
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

#[path = "support/stream.rs"]
mod stream_support;

// Count requested, live application allocation bytes on this test thread only.
// This deliberately excludes allocator overhead, RSS, helper/SDK and transport
// memory. Keep fixture construction outside the measured parser interval.
mod allocation_probe {
    use std::{
        alloc::{GlobalAlloc, Layout, System},
        cell::Cell,
    };

    thread_local! {
        static LIVE: Cell<isize> = const { Cell::new(0) };
        static PEAK: Cell<isize> = const { Cell::new(0) };
    }

    pub struct Allocator;

    fn change(delta: isize) {
        let _ = LIVE.try_with(|live| {
            let current = live.get() + delta;
            live.set(current);
            let _ = PEAK.try_with(|peak| peak.set(peak.get().max(current)));
        });
    }

    unsafe impl GlobalAlloc for Allocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let pointer = unsafe { System.alloc(layout) };
            if !pointer.is_null() {
                change(layout.size() as isize);
            }
            pointer
        }

        unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
            change(-(layout.size() as isize));
            unsafe { System.dealloc(pointer, layout) };
        }

        unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
            let pointer = unsafe { System.realloc(pointer, layout, size) };
            if !pointer.is_null() {
                change(size as isize - layout.size() as isize);
            }
            pointer
        }
    }

    pub async fn measure<T>(operation: impl std::future::Future<Output = T>) -> (T, usize, usize) {
        let baseline = LIVE.with(Cell::get);
        PEAK.with(|peak| peak.set(baseline));
        let result = operation.await;
        let live = LIVE.with(|live| (live.get() - baseline) as usize);
        let peak = PEAK.with(|peak| (peak.get() - baseline) as usize);
        (result, live, peak)
    }
}

#[global_allocator]
static ALLOCATOR: allocation_probe::Allocator = allocation_probe::Allocator;

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

    async fn count_tokens(
        &self,
        _: &str,
        _: &[Message],
        _heavy: std::sync::Arc<tokio::sync::OwnedSemaphorePermit>,
    ) -> Result<u64, InferenceError> {
        self.started.notify_one();
        std::future::pending().await
    }

    async fn generate_stream(
        &self,
        _: &Model,
        _: &[Message],
        _heavy: std::sync::Arc<tokio::sync::OwnedSemaphorePermit>,
        _: &mut (dyn for<'d> FnMut(&'d str) + Send),
    ) -> Result<stream::StreamUsage, InferenceError> {
        unreachable!("preflight must time out")
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

    async fn count_tokens(
        &self,
        _: &str,
        _: &[Message],
        _heavy: std::sync::Arc<tokio::sync::OwnedSemaphorePermit>,
    ) -> Result<u64, InferenceError> {
        Ok(1)
    }

    async fn generate_stream(
        &self,
        _: &Model,
        _: &[Message],
        _heavy: std::sync::Arc<tokio::sync::OwnedSemaphorePermit>,
        on_delta: &mut (dyn for<'d> FnMut(&'d str) + Send),
    ) -> Result<stream::StreamUsage, InferenceError> {
        let mut parser = stream::ProtocolParser::default();
        for _ in 0..200 {
            parser
                .feed(
                    &stream_support::event(stream_support::choice(Some(&"x".repeat(1024)), None)),
                    &mut *on_delta,
                )
                .map_err(|_| InferenceError::InvalidResponse)?;
            tokio::task::yield_now().await;
        }
        for event in [
            stream_support::event(stream_support::choice(None, Some("stop"))),
            stream_support::event(stream_support::usage(1, 1)),
            b"data: [DONE]\n\n".to_vec(),
        ] {
            parser
                .feed(&event, &mut *on_delta)
                .map_err(|_| InferenceError::InvalidResponse)?;
        }
        parser.eof().map_err(|_| InferenceError::InvalidResponse)
    }

    fn verification_document(&self) -> Result<serde_json::Value, InferenceError> {
        Ok(serde_json::json!({"verified": true}))
    }
}

struct OversizedVerificationInference;

#[async_trait]
impl Inference for OversizedVerificationInference {
    async fn catalog(&self) -> Result<Vec<u8>, InferenceError> {
        unreachable!()
    }

    async fn count_tokens(
        &self,
        _: &str,
        _: &[Message],
        _heavy: std::sync::Arc<tokio::sync::OwnedSemaphorePermit>,
    ) -> Result<u64, InferenceError> {
        unreachable!()
    }

    async fn generate_stream(
        &self,
        _: &Model,
        _: &[Message],
        _heavy: std::sync::Arc<tokio::sync::OwnedSemaphorePermit>,
        _: &mut (dyn for<'d> FnMut(&'d str) + Send),
    ) -> Result<stream::StreamUsage, InferenceError> {
        unreachable!("verification must reject before generation")
    }

    fn verification_document(&self) -> Result<serde_json::Value, InferenceError> {
        Ok(serde_json::json!({"oversized": "x".repeat(2 * 1024 * 1024)}))
    }
}

struct PanicInference;

#[async_trait]
impl Inference for PanicInference {
    async fn catalog(&self) -> Result<Vec<u8>, InferenceError> {
        Ok(catalog_bytes())
    }

    async fn count_tokens(
        &self,
        _: &str,
        _: &[Message],
        _heavy: std::sync::Arc<tokio::sync::OwnedSemaphorePermit>,
    ) -> Result<u64, InferenceError> {
        panic!("injected post-reservation panic")
    }

    async fn generate_stream(
        &self,
        _: &Model,
        _: &[Message],
        _heavy: std::sync::Arc<tokio::sync::OwnedSemaphorePermit>,
        _: &mut (dyn for<'d> FnMut(&'d str) + Send),
    ) -> Result<stream::StreamUsage, InferenceError> {
        unreachable!("preflight must panic")
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

    async fn count_tokens(
        &self,
        _: &str,
        _: &[Message],
        _heavy: std::sync::Arc<tokio::sync::OwnedSemaphorePermit>,
    ) -> Result<u64, InferenceError> {
        self.tokenizations.fetch_add(1, Ordering::SeqCst);
        self.pause_at(HoldPoint::Tokenization).await;
        Ok(1)
    }

    async fn generate_stream(
        &self,
        _: &Model,
        _: &[Message],
        _heavy: std::sync::Arc<tokio::sync::OwnedSemaphorePermit>,
        on_delta: &mut (dyn for<'d> FnMut(&'d str) + Send),
    ) -> Result<stream::StreamUsage, InferenceError> {
        self.generations.fetch_add(1, Ordering::SeqCst);
        stream_support::terminal("**safe**", 1, 1, on_delta)
            .map_err(|_| InferenceError::InvalidResponse)
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
    let body = continuation_body(format!(
        "csrf={csrf}&token={token}&model=m&history=%5B%5D&prompt=hello"
    ));
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
    assert!(html.contains("**safe**"));
    assert!(!html.contains("<strong>"));
    assert!(!html.contains("Confirm response delivery"));
    assert_eq!(inference.generations.load(Ordering::SeqCst), 1);
    assert_eq!(state.accounting.available("a"), Some(97));

    let carried_history = "%5B%7B%22role%22%3A%22user%22%2C%22content%22%3A%22hello%22%7D%2C%7B%22role%22%3A%22assistant%22%2C%22content%22%3A%22%2A%2Asafe%2A%2A%22%7D%5D";
    let marker = "name=token value=\"";
    let start = html.find(marker).unwrap() + marker.len();
    let next_token = &html[start..html[start..].find('\"').unwrap() + start];
    let next_body = continuation_body(format!(
        "csrf={csrf}&token={next_token}&model=m&history={carried_history}&prompt=again"
    ));
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
    response.into_body().collect().await.unwrap();
    assert_eq!(inference.generations.load(Ordering::SeqCst), 2);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn dropping_unconsumed_response_settles_usage() {
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
                .body(Body::from(continuation_body(format!(
                    "csrf={csrf}&token={token}&model=m&history=%5B%5D&prompt=hello"
                ))))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(state.accounting.available("a"), Some(48));
    drop(response);
    wait_balance(&state, "a", 97).await;
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
    drop(response); // The independent ordinary-control lane is response-held.

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
async fn control_admission_is_fail_fast_and_held_until_response_drop() {
    let (state, cookie, _, _, _) = fixture("/unused/evidence");
    let app = router(state);
    let mut responses = Vec::new();
    for _ in 0..1 {
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
    let request = || post_form("/chat", "", String::new());
    let overloaded = app.clone().oneshot(request()).await.unwrap();
    assert_eq!(overloaded.status(), StatusCode::SERVICE_UNAVAILABLE);
    let control = app
        .clone()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(control.status(), StatusCode::OK);

    // Dequeuing data and dropping the body must not free its heavy lease while
    // a clone/slice still owns response storage. This also covers failed forms.
    let mut body = responses.pop().unwrap().into_body();
    let frame = body.frame().await.unwrap().unwrap().into_data().unwrap();
    let clone = frame.clone();
    let slice = clone.slice(..1);
    drop(frame);
    drop(clone);
    assert!(body.frame().await.is_none());
    drop(body);
    assert_eq!(
        app.clone().oneshot(request()).await.unwrap().status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    drop(slice);
    assert_eq!(
        app.oneshot(request()).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
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
    let body = continuation_body(format!(
        "csrf={}&token={token}&model=m&history=%5B%5D&prompt=canary",
        new_session.csrf
    ));
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
    let body = continuation_body(format!(
        "csrf={csrf}&token={token}&model=m&history=%5B%5D&prompt=canary"
    ));
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
        let body = continuation_body(format!(
            "csrf={csrf}&token={token}&model=m&history=%5B%5D&prompt=canary"
        ));
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

// Keep human-readable JSON in fixture recipes, but submit only the canonical
// production continuation wire format. No compatibility decoder exists in /chat.
fn continuation_body(body: String) -> String {
    let mut fields: Vec<&str> = body.split('&').collect();
    let Some(index) = fields
        .iter()
        .position(|field| field.starts_with("history="))
    else {
        return body;
    };
    let encoded = fields.remove(index).strip_prefix("history=").unwrap();
    let mut json = Vec::new();
    let mut bytes = encoded.bytes();
    while let Some(byte) = bytes.next() {
        json.push(match byte {
            b'%' => {
                (char::from(bytes.next().unwrap()).to_digit(16).unwrap() * 16
                    + char::from(bytes.next().unwrap()).to_digit(16).unwrap()) as u8
            }
            b'+' => b' ',
            byte => byte,
        });
    }
    let mut wire = fields.join("&");
    for (index, block) in json
        .chunks(possums::render::HISTORY_BLOCK_BYTES)
        .enumerate()
    {
        wire.push_str(&format!("&h{index:06}={}", URL_SAFE_NO_PAD.encode(block)));
    }
    wire.push_str(&format!(
        "&history_manifest=1.{:06}.{:08}",
        json.len().div_ceil(possums::render::HISTORY_BLOCK_BYTES),
        json.len()
    ));
    wire
}

fn post_form(uri: &str, cookie: &str, body: String) -> Request<Body> {
    let body = if uri.split('?').next() == Some("/chat") {
        continuation_body(body)
    } else {
        body
    };
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
async fn new_chat_requires_csrf_and_renders_empty_selector_in_same_session() {
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
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!response.headers().contains_key(header::LOCATION));
    assert!(!response.headers().contains_key(header::SET_COOKIE));
    assert_ne!(
        state.auth.session(id).unwrap().conversation,
        original.conversation
    );
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8_lossy(&body);
    assert!(html.contains("name=h000000 value=\"W10\""));
    assert!(html.contains("<select name=model>"));
    assert!(html.contains("<option value=\"m\">m</option>"));
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
        assert_eq!(
            reset.status(),
            if control == "/chat/new" {
                StatusCode::OK
            } else {
                StatusCode::SEE_OTHER
            }
        );
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
        if control == "/logout" {
            // Home and logout intentionally share the one ordinary-control
            // lane. Preserve the stale-auth race coverage through the auth API.
            assert_eq!(reset.status(), StatusCode::SERVICE_UNAVAILABLE);
            state.auth.logout(session_id(&cookie), &csrf).unwrap();
        } else {
            assert_eq!(reset.status(), StatusCode::OK);
        }
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
        assert_eq!(
            reset.status(),
            if control == "/chat/new" {
                StatusCode::OK
            } else {
                StatusCode::SEE_OTHER
            }
        );
        drop(reset);
        assert_eq!(state.accounting.available("a"), Some(48));
        gate.release.notify_one();
        let response = request.await.unwrap().unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert!(!String::from_utf8_lossy(&body).contains("name=token"));
        assert_eq!(inference.tokenizations.load(Ordering::SeqCst), 1);
        assert_eq!(inference.generations.load(Ordering::SeqCst), 1);
        // Accepted usage settles even when reset/logout suppresses continuation.
        assert_eq!(state.accounting.available("a"), Some(97));
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
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(state.accounting.available("a"), Some(97));
        let reset = router(state.clone())
            .oneshot(post_form(control, &cookie, format!("csrf={csrf}")))
            .await
            .unwrap();
        assert_eq!(
            reset.status(),
            if control == "/chat/new" {
                StatusCode::OK
            } else {
                StatusCode::SEE_OTHER
            }
        );
        drop(reset);
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
            "%5B%7B%22role%22%3A%22user%22%2C%22content%22%3A%22changed-history%22%7D%2C%7B%22role%22%3A%22assistant%22%2C%22content%22%3A%22prior%22%7D%5D";
        for _ in 0..6 {
            let response = router(state.clone()).oneshot(post_form("/chat", &cookie, format!("csrf={csrf}&token={token}&model=m&history={changed_history}&prompt=different"))).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let body = response.into_body().collect().await.unwrap().to_bytes();
            let html = String::from_utf8_lossy(&body);
            assert!(html.contains("already in progress"));
            assert!(!html.contains("changed-history"));
            assert!(!html.contains("name=token"));
            assert_eq!(state.accounting.available("a"), Some(48));
        }
        gate.release.notify_one();
        let response = first.await.unwrap().unwrap();
        if deliver {
            response.into_body().collect().await.unwrap();
        } else {
            drop(response);
        }
        let expected = 97;
        wait_balance(&state, "a", expected).await;
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
            StatusCode::BAD_REQUEST,
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
    assert_eq!(response.status(), StatusCode::OK);
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
    assert!(html.contains("name=h000000 value=\"W10\""));
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
    assert_eq!(response.status(), StatusCode::OK);
    drop(response);
    assert_eq!(state.accounting.available("a"), Some(844));
    let token = state.auth.issue_submission(session_id(&cookie)).unwrap();
    // Repeated rejection cannot leak the fourth generation permit.
    for _ in 0..6 {
        let response = router(state.clone())
            .oneshot(chat_request(&cookie, &csrf, &token))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
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
    assert_eq!(response.status(), StatusCode::OK);
    response.into_body().collect().await.unwrap();
    assert_eq!(state.accounting.available("a"), Some(893));
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
        let response = task.await.unwrap().unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        response.into_body().collect().await.unwrap();
    }
    assert_eq!(state.accounting.available("a"), Some(990));
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

async fn wait_balance(state: &AppState, account: &str, expected: u64) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while state.accounting.available(account) != Some(expected) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[tokio::test(start_paused = true)]
async fn cancelled_preflight_waiter_keeps_reservation_until_deadline() {
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
    let body = continuation_body(format!(
        "csrf={}&token={token}&model=m&history=%5B%5D&prompt=hello",
        session.csrf
    ));
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
    assert_eq!(state.accounting.available("a"), Some(48));
    tokio::time::advance(Duration::from_secs(31)).await;
    wait_balance(&state, "a", 100).await;
    std::fs::remove_file(path).unwrap();
}

#[tokio::test(start_paused = true)]
async fn socket_disconnect_during_preflight_keeps_work_until_deadline() {
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
    let body = continuation_body(format!(
        "csrf={}&token={token}&model=m&history=%5B%5D&prompt=hello",
        session.csrf
    ));
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
    assert_eq!(state.accounting.available("a"), Some(48));
    tokio::time::advance(Duration::from_secs(31)).await;
    wait_balance(&state, "a", 100).await;
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
    let body = continuation_body(format!(
        "csrf={}&token={token}&model=m&history=%5B%5D&prompt=hello",
        session.csrf
    ));
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
        if state.accounting.available("a") == Some(97) {
            break;
        }
        sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(state.accounting.available("a"), Some(97));
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
    let body = continuation_body(format!(
        "csrf={}&token={token}&model=m&history=%5B%5D&prompt=hello",
        session.csrf
    ));
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
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
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
    let body = continuation_body(format!(
        "csrf={csrf}&token={token}&model=m&history=%5B%5D&prompt=canary"
    ));
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
async fn attestation_response_rejects_oversized_upstream_document() {
    let path = std::env::temp_dir().join(format!(
        "possums-large-document-evidence-{}",
        std::process::id()
    ));
    write_evidence(
        &path,
        serde_json::json!({"verified": true}),
        now(),
        EXPECTED_RELEASE,
        EXPECTED_KEY,
    );
    let (mut state, ..) = fixture(path.to_str().unwrap());
    state.inference = Arc::new(OversizedVerificationInference);
    let response = router(state)
        .oneshot(
            Request::builder()
                .uri("/attestation")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
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
                    .body(Body::from(continuation_body(format!(
                        "csrf={csrf}&token={token}&model=m&history=%5B%5D&prompt=canary"
                    ))))
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

#[tokio::test]
async fn four_active_chats_leave_new_chat_and_logout_independent_and_keep_account_limit() {
    let path = valid_evidence("partition");
    let (_, _, _, _, inference) = fixture(path.to_str().unwrap());
    two_model_catalog(&inference);
    let credentials = [
        URL_SAFE_NO_PAD.encode([7_u8; 32]),
        URL_SAFE_NO_PAD.encode([8_u8; 32]),
    ];
    let accounts: Vec<_> = credentials.iter().enumerate().map(|(index, credential)| {
        serde_json::json!({"id": index.to_string(), "credential_sha256": URL_SAFE_NO_PAD.encode(Sha256::digest(credential.as_bytes())), "demo_microunits": 1000})
    }).collect();
    let auth = Auth::from_json(&serde_json::to_string(&accounts).unwrap()).unwrap();
    let sessions: Vec<_> = credentials
        .iter()
        .map(|credential| {
            auth.authenticate(credential, &auth.issue_login_challenge().unwrap())
                .unwrap()
        })
        .collect();
    let state = AppState::new(
        auth,
        inference.clone(),
        path.to_str().unwrap(),
        Arc::new(TestEvidenceVerifier),
    );
    let app = router(state.clone());
    let mut held = Vec::new();
    for index in 0..4 {
        let (id, session) = &sessions[index / 3];
        let token = state.auth.issue_submission(id).unwrap();
        let gate = inference.hold(HoldPoint::Tokenization);
        let request = if index < 2 {
            post_form(
                "/chat",
                &session_cookie(id),
                maximum_chat_form(&session.csrf, &token, index == 1),
            )
        } else {
            chat_request(&session_cookie(id), &session.csrf, &token)
        };
        let task = tokio::spawn(app.clone().oneshot(request));
        gate.entered.notified().await;
        held.push((task, gate));
        if index == 2 {
            // Three per account still applies while the fourth global lane is free.
            let token = state.auth.issue_submission(id).unwrap();
            let rejected = app
                .clone()
                .oneshot(chat_request(&session_cookie(id), &session.csrf, &token))
                .await
                .unwrap();
            assert_eq!(rejected.status(), StatusCode::TOO_MANY_REQUESTS);
            assert_eq!(inference.tokenizations.load(Ordering::SeqCst), 3);
            drop(rejected);
        }
    }
    assert_eq!(inference.tokenizations.load(Ordering::SeqCst), 4);
    assert_eq!(state.accounting.available("0"), Some(844));
    assert_eq!(state.accounting.available("1"), Some(948));
    // Saturated heavy admission must reject before polling even a pending body.
    let pending = Body::from_stream(futures_util::stream::pending::<
        Result<axum::body::Bytes, std::io::Error>,
    >());
    let overloaded = tokio::time::timeout(
        Duration::from_secs(1),
        app.clone().oneshot(
            Request::builder()
                .method("POST")
                .uri("/chat?alias=1")
                .body(pending)
                .unwrap(),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(overloaded.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(overloaded.headers()[header::CACHE_CONTROL], "no-store");

    let control = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/claims")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(control.status(), StatusCode::OK);
    let (id, session) = &sessions[0];
    let cookie = session_cookie(id);
    let original = state.auth.session(id).unwrap().conversation;
    let forged = app
        .clone()
        .oneshot(post_form("/chat/new", &cookie, "csrf=forged".into()))
        .await
        .unwrap();
    assert_eq!(forged.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(state.auth.session(id).unwrap().conversation, original);
    drop(forged);
    let new_chat = app
        .clone()
        .oneshot(post_form(
            "/chat/new?native=1",
            &cookie,
            format!("csrf={}", session.csrf),
        ))
        .await
        .unwrap();
    assert_eq!(new_chat.status(), StatusCode::OK);
    assert_ne!(state.auth.session(id).unwrap().conversation, original);
    // All six lanes are now occupied (four chats, New chat, ordinary controls).
    for route in ["/chat/new", "/logout"] {
        let rejected = app
            .clone()
            .oneshot(post_form(route, &cookie, format!("csrf={}", session.csrf)))
            .await
            .unwrap();
        assert_eq!(rejected.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
    let mut new_body = new_chat.into_body();
    let new_frame = new_body
        .frame()
        .await
        .unwrap()
        .unwrap()
        .into_data()
        .unwrap();
    assert!(String::from_utf8_lossy(&new_frame).contains("<option value=\"n\">n</option>"));
    let retained_new = new_frame.slice(..1);
    drop(new_frame);
    drop(new_body);
    assert_eq!(
        app.clone()
            .oneshot(post_form(
                "/chat/new",
                &cookie,
                format!("csrf={}", session.csrf)
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    drop(retained_new);
    let new_chat = app
        .clone()
        .oneshot(post_form(
            "/chat/new",
            &cookie,
            format!("csrf={}", session.csrf),
        ))
        .await
        .unwrap();
    assert_eq!(new_chat.status(), StatusCode::OK);
    let selector = new_chat.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8_lossy(&selector);
    assert!(html.contains("<select name=model>"));
    assert!(html.contains("<option value=\"m\">m</option>"));
    assert!(html.contains("<option value=\"n\">n</option>"));
    assert!(html.contains("name=h000000 value=\"W10\""));

    // Ordinary controls also remain charged through dequeued/retained frames.
    let mut body = control.into_body();
    let frame = body.frame().await.unwrap().unwrap().into_data().unwrap();
    let retained = frame.slice(..1);
    drop(frame);
    drop(body);
    let (id, session) = &sessions[1];
    let logout = || {
        post_form(
            "/logout",
            &session_cookie(id),
            format!("csrf={}", session.csrf),
        )
    };
    assert_eq!(
        app.clone().oneshot(logout()).await.unwrap().status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    drop(retained);
    assert_eq!(
        app.clone().oneshot(logout()).await.unwrap().status(),
        StatusCode::SEE_OTHER
    );
    assert!(state.auth.session(id).is_none());

    for (task, gate) in held {
        gate.release.notify_one();
        let response = task.await.unwrap().unwrap();
        // Reset/logout invalidate continuation, but not already admitted work.
        assert_eq!(response.status(), StatusCode::OK);
        drop(response);
    }
    wait_balance(&state, "0", 991).await;
    wait_balance(&state, "1", 997).await;
    assert_eq!(inference.generations.load(Ordering::SeqCst), 4);
    // All heavy slots return after accepted work, not after observer loss.
    let mut returned = Vec::new();
    for _ in 0..4 {
        let response = app
            .clone()
            .oneshot(post_form("/chat", "", String::new()))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        returned.push(response);
    }
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn control_body_bounds_and_method_path_aliases_cannot_evade_lanes() {
    let (state, cookie, csrf, _, inference) = fixture("/unused/evidence");
    let app = router(state.clone());
    let original = state
        .auth
        .session(session_id(&cookie))
        .unwrap()
        .conversation;
    for (method, path) in [
        ("POST", "/chat/new"),
        ("POST", "/logout"),
        ("POST", "/login"),
        ("GET", "/"),
        ("HEAD", "/"),
        ("GET", "/attestation"),
        ("GET", "/chat"),
        ("POST", "/chat/"),
        ("POST", "/chat%2fnew"),
        ("POST", "//chat"),
        ("PUT", "/chat"),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header(header::COOKIE, cookie.split(';').next().unwrap())
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from(vec![b'x'; 4097]))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::PAYLOAD_TOO_LARGE,
            "{method} {path}"
        );
    }
    assert_eq!(
        state
            .auth
            .session(session_id(&cookie))
            .unwrap()
            .conversation,
        original
    );
    assert_eq!(inference.tokenizations.load(Ordering::SeqCst), 0);
    let mut boundary = format!("csrf={csrf}&padding=");
    boundary.extend(std::iter::repeat_n('x', 4096 - boundary.len()));
    assert_eq!(
        app.clone()
            .oneshot(post_form("/chat/new", &cookie, boundary))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let held_control = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/claims")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    for (method, path) in [
        ("HEAD", "/"),
        ("POST", "/chat/"),
        ("GET", "/chat/new"),
        ("POST", "/chat%2Fnew"),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
    // The canonical handlers remain independent even when ordinary controls fill.
    assert_eq!(
        app.clone()
            .oneshot(post_form("/chat/new", &cookie, format!("csrf={csrf}")))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let chat = app
        .clone()
        .oneshot(post_form("/chat?native=1", &cookie, String::new()))
        .await
        .unwrap();
    assert_eq!(chat.status(), StatusCode::BAD_REQUEST); // Reached bounded continuation decoding.
    drop(chat);
    drop(held_control);
    assert_eq!(
        app.oneshot(post_form("/logout", &cookie, format!("csrf={csrf}")))
            .await
            .unwrap()
            .status(),
        StatusCode::SEE_OTHER
    );
}

fn maximum_chat_form(csrf: &str, token: &str, dense: bool) -> String {
    use possums::{render::max_history_decoded_bytes, web::BODY_LIMIT};
    let pair = r#"{"role":"user","content":"x"},{"role":"assistant","content":""},"#;
    let json = if dense {
        format!(
            "[{}]",
            pair.repeat((max_history_decoded_bytes() - 2) / pair.len())
                .trim_end_matches(',')
        )
    } else {
        format!(
            r#"[{{"role":"user","content":"{}"}},{{"role":"assistant","content":""}}]"#,
            "x".repeat(max_history_decoded_bytes() - 80)
        )
    };
    let mut payload = continuation_body(format!(
        "csrf={csrf}&token={token}&model=m&history={json}&prompt=x"
    ));
    // Prompt padding reaches the exact raw boundary; no noncanonical fields.
    payload = payload.replacen(
        "prompt=x",
        &format!("prompt=x{}", "x".repeat(BODY_LIMIT - payload.len())),
        1,
    );
    assert_eq!(payload.len(), BODY_LIMIT);
    payload
}

#[tokio::test]
async fn exact_eight_mib_and_dense_chat_inputs_stay_within_tested_heavy_envelope() {
    use axum::body::Bytes;

    let path = valid_evidence("partition-inputs");
    for dense in [false, true] {
        let (state, cookie, csrf, token, inference) = fixture(path.to_str().unwrap());
        let payload = maximum_chat_form(&csrf, &token, dense);
        let (response, _, peak) = allocation_probe::measure(async {
            // Include the submitted raw allocation as well as collector storage
            // and decoded strings/Vec. Tiny frames must never be queued en masse.
            let bytes = Bytes::from(payload.clone());
            let chunks = futures_util::stream::unfold(bytes, |mut remaining| async move {
                if remaining.is_empty() {
                    return None;
                }
                let chunk = remaining.split_to(remaining.len().min(97));
                Some((Ok::<_, std::io::Error>(chunk), remaining))
            });
            router(state.clone())
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/chat")
                        .header(header::COOKIE, cookie.split(';').next().unwrap())
                        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                        .body(Body::from_stream(chunks))
                        .unwrap(),
                )
                .await
                .unwrap()
        })
        .await;
        println!(
            "streaming route preflight dense={dense}: raw={} thread-local peak={peak}",
            payload.len()
        );
        assert!(peak < 104 * 1024 * 1024, "application peak={peak}");
        // Fixture-only route/decoder measurement: inference is fake, so this is
        // not a universal 104-MiB envelope or SDK/TLS/inference measurement.
        // Both forms pass input parsing, trust, reservation and prompt calls.
        // Generation/startup now run on detached workers: this old thread-local
        // measurement covers only preflight, NOT the aggregate resource gate.
        assert_eq!(inference.tokenizations.load(Ordering::SeqCst), 1);
        assert_eq!(response.status(), StatusCode::OK);
        drop(response);
        wait_balance(&state, "a", 97).await;
        assert_eq!(inference.generations.load(Ordering::SeqCst), 1);
    }
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn full_supported_catalog_fits_tested_control_render_envelope() {
    let (state, cookie, _, _, inference) = fixture("/unused/evidence");
    let mut catalog: serde_json::Value = serde_json::from_slice(&catalog_bytes()).unwrap();
    let template = catalog["data"][0].clone();
    catalog["data"] = (0..256)
        .map(|index| {
            let mut model = template.clone();
            model["id"] = serde_json::json!(format!("{index:04}{}", "x".repeat(124)));
            model
        })
        .collect();
    *inference.catalog_override.lock().unwrap() = Some(serde_json::to_vec(&catalog).unwrap());
    let (html, _, peak) = allocation_probe::measure(async {
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
        response.into_body().collect().await.unwrap().to_bytes()
    })
    .await;
    assert_eq!(
        String::from_utf8_lossy(&html).matches("<option ").count(),
        256
    );
    println!(
        "full catalog control: response={} application peak={peak}",
        html.len()
    );
    assert!(peak < 16 * 1024 * 1024);
}
