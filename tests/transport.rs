use async_trait::async_trait;
use axum::{
    body::{Body, Bytes},
    http::{header, Request, StatusCode},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use futures_util::stream;
use possums::{
    attestation::UnavailableEvidenceVerifier,
    auth::Auth,
    catalog::Model,
    inference::{Inference, InferenceError, Message},
    server::{router_with_body_deadline, serve_with_header_deadline, AppState},
};
use sha2::{Digest, Sha256};
use std::{convert::Infallible, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tower::ServiceExt;

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

    pub fn measure<T>(operation: impl FnOnce() -> T) -> (T, usize, usize) {
        let baseline = LIVE.with(Cell::get);
        PEAK.with(|peak| peak.set(baseline));
        let result = operation();
        let live = LIVE.with(|live| (live.get() - baseline) as usize);
        let peak = PEAK.with(|peak| (peak.get() - baseline) as usize);
        (result, live, peak)
    }
}

#[global_allocator]
static ALLOCATOR: allocation_probe::Allocator = allocation_probe::Allocator;

#[test]
fn oversized_nested_gateway_evidence_is_rejected() {
    use possums::attestation::load_evidence;

    // Previously accepted: 520 chains of 62 singleton objects, depth 64 and
    // 65,011 lexical nodes. Reject before constructing BTreeMap-heavy Values.
    let chain = format!("{}0{}", "{\"a\":".repeat(62), "}".repeat(62));
    let quote = vec![chain; 520].join(",");
    let document = format!(
        r#"{{"quote":[{quote}],"issued_at_unix":1,"release_digest":"{}","endpoint_key_sha256":"{}","freshness_expires_at_unix":2}}"#,
        "a".repeat(64),
        "b".repeat(64),
    );
    assert_eq!(document.len(), 194_713);
    assert!(document.len() < 1024 * 1024);
    let path = std::env::temp_dir().join(format!(
        "possums-control-envelope-{}.json",
        std::process::id()
    ));
    std::fs::write(&path, &document).unwrap();
    assert!(matches!(
        load_evidence(&path),
        Err(possums::attestation::EvidenceError::Invalid)
    ));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn near_cap_nested_gateway_evidence_stays_below_control_lane_for_tested_shape() {
    use possums::attestation::{load_evidence, validate_evidence};

    // Depth 64 including root object and quote array. Each chain contributes
    // 125 lexical nodes; each singleton object contributes 3. With 11 envelope
    // nodes, 32 chains and 28 singletons total 4,095 (cap: 4,096).
    let chain = format!("{}0{}", "{\"a\":".repeat(62), "}".repeat(62));
    let quote = std::iter::repeat_n(chain, 32)
        .chain(std::iter::repeat_n("{\"a\":0}".to_owned(), 28))
        .collect::<Vec<_>>()
        .join(",");
    let document = format!(
        r#"{{"quote":[{quote}],"issued_at_unix":1,"release_digest":"{}","endpoint_key_sha256":"{}","freshness_expires_at_unix":2}}"#,
        "a".repeat(64),
        "b".repeat(64),
    );
    let path = std::env::temp_dir().join(format!(
        "possums-control-envelope-near-cap-{}.json",
        std::process::id()
    ));
    std::fs::write(&path, &document).unwrap();
    // Fixture construction is outside this measurement; add raw document bytes
    // explicitly. Probe counts requested allocations on this thread, not RSS,
    // allocator overhead, transport, helper or response serialization.
    let (evidence, retained, peak) =
        allocation_probe::measure(|| validate_evidence(load_evidence(&path).unwrap(), 1).unwrap());
    std::fs::remove_file(path).unwrap();
    assert_eq!(evidence.quote.as_array().unwrap().len(), 60);
    println!(
        "near-cap evidence: wire={} retained={retained} peak={peak} bytes",
        document.len()
    );
    assert!(retained <= peak);
    assert!(document.len() + peak < 16 * 1024 * 1024);
    // This synthetic Rust parsing/schema fixture is not cryptographically
    // appraised; the measured shape is not a universal allocation bound.
}

#[test]
fn near_byte_cap_scalar_gateway_evidence_stays_below_control_lane_for_tested_shape() {
    use possums::attestation::{load_evidence, validate_evidence};

    let envelope = |quote: &str| {
        format!(
            r#"{{"quote":{quote},"issued_at_unix":1,"release_digest":"{}","endpoint_key_sha256":"{}","freshness_expires_at_unix":2}}"#,
            "a".repeat(64),
            "b".repeat(64),
        )
    };
    let empty = envelope("\"\"");
    let document = envelope(&format!("\"{}\"", "x".repeat(1024 * 1024 - empty.len())));
    assert_eq!(document.len(), 1024 * 1024);
    let path = std::env::temp_dir().join(format!(
        "possums-control-envelope-byte-cap-{}.json",
        std::process::id()
    ));
    std::fs::write(&path, &document).unwrap();
    let (_, retained, peak) =
        allocation_probe::measure(|| validate_evidence(load_evidence(&path).unwrap(), 1).unwrap());
    std::fs::remove_file(path).unwrap();
    println!(
        "byte-cap evidence: wire={} retained={retained} peak={peak} bytes",
        document.len()
    );
    assert!(retained <= peak);
    assert!(document.len() + peak < 16 * 1024 * 1024);
}

struct UnusedInference;

#[async_trait]
impl Inference for UnusedInference {
    async fn catalog(&self) -> Result<Vec<u8>, InferenceError> {
        Err(InferenceError::Unavailable)
    }

    async fn count_tokens(
        &self,
        _: &str,
        _: &[Message],
        _heavy: std::sync::Arc<possums::telemetry::hooks::Lease>,
    ) -> Result<u64, InferenceError> {
        unreachable!()
    }

    async fn generate_stream(
        &self,
        _: &Model,
        _: &[Message],
        _heavy: std::sync::Arc<possums::telemetry::hooks::Lease>,
        _: &mut (dyn for<'d> FnMut(&'d str) + Send),
    ) -> Result<possums::inference::stream::StreamUsage, InferenceError> {
        unreachable!("transport rejection must precede generation")
    }

    fn verification_document(&self) -> Result<serde_json::Value, InferenceError> {
        Err(InferenceError::Unavailable)
    }
}

fn state() -> AppState {
    let credential = URL_SAFE_NO_PAD.encode([7_u8; 32]);
    let hash = URL_SAFE_NO_PAD.encode(Sha256::digest(credential.as_bytes()));
    let auth = Auth::from_json(&format!(
        r#"[{{"id":"a","credential_sha256":"{hash}","demo_microunits":100}}]"#
    ))
    .unwrap();
    AppState::new(
        auth,
        Arc::new(UnusedInference),
        Arc::<str>::from("/missing"),
        Arc::new(UnavailableEvidenceVerifier),
    )
}

#[tokio::test]
async fn malformed_deep_dense_and_fragmented_api_json_never_reaches_inference() {
    let state = state();
    let credential = URL_SAFE_NO_PAD.encode([7_u8; 32]);
    let (bearer, _) = state
        .auth
        .authenticate_api(&credential, &state.auth.issue_api_challenge().unwrap())
        .unwrap();
    let app = possums::server::router(state);
    let dense = format!("[{}]", "0,".repeat(100_000));
    let deep = format!("{}0{}", "[".repeat(140), "]".repeat(140));
    for wire in ["{".to_owned(), deep, dense, "\u{0000}".to_owned()] {
        let fragments = stream::iter(
            wire.into_bytes()
                .into_iter()
                .map(|byte| Ok::<Bytes, Infallible>(Bytes::from(vec![byte]))),
        );
        let response = app
            .clone()
            .oneshot(
                Request::post("/v1/chat/completions")
                    .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from_stream(fragments))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    }
}

#[tokio::test]
async fn removed_browser_routes_are_bounded_non_html_and_never_infer() {
    use http_body_util::BodyExt;
    for path in [
        "/",
        "/app",
        "/login",
        "/logout",
        "/chat",
        "/chat/new",
        "/recovery",
        "/recovery/download",
        "/claims",
    ] {
        for method in ["GET", "POST"] {
            let response = possums::server::router(state())
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(path)
                        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                        .body(Body::from("csrf=secret&prompt=hostile-prompt-canary"))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert!(!response.status().is_success(), "{method} {path}");
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
            assert_ne!(
                response
                    .headers()
                    .get(header::CONTENT_TYPE)
                    .map(|v| v.as_bytes()),
                Some(b"text/html; charset=utf-8".as_slice())
            );
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            assert!(bytes.len() < 4096);
            assert!(!bytes
                .windows(b"hostile-prompt-canary".len())
                .any(|v| v == b"hostile-prompt-canary"));
            assert!(!bytes.windows(b"<html".len()).any(|v| v == b"<html"));
        }
    }
}

#[tokio::test(start_paused = true)]
async fn request_body_has_one_total_deadline_despite_trickling() {
    let chunks = stream::unfold((), |_| async {
        tokio::time::sleep(Duration::from_secs(1)).await;
        Some((Ok::<Bytes, Infallible>(Bytes::from_static(b"x")), ()))
    });
    let response = router_with_body_deadline(state(), Duration::from_secs(3))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sessions")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from_stream(chunks))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
}

#[tokio::test]
async fn incomplete_headers_are_closed_at_the_total_header_deadline() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(serve_with_header_deadline(
        listener,
        state(),
        Duration::from_millis(50),
    ));
    let mut client = TcpStream::connect(address).await.unwrap();
    client.write_all(b"GET / HTTP/1.1\r\nHost:").await.unwrap();

    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(1), client.read_to_end(&mut response))
        .await
        .expect("header deadline did not close the connection")
        .unwrap();
    server.abort();
}

#[tokio::test]
async fn pipelined_http1_rejects_oversized_declared_body_without_reading_its_length() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(serve_with_header_deadline(
        listener,
        state(),
        Duration::from_secs(5),
    ));
    let mut client = TcpStream::connect(address).await.unwrap();
    let request = format!(
        "GET /v1/auth/challenge HTTP/1.1\r\nHost: local\r\nX-Fill: {}\r\n\r\nPOST /v1/sessions HTTP/1.1\r\nHost: local\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
        "h".repeat(10 * 1024),
        8 * 1024,
        "x".repeat(4 * 1024 + 1),
    );
    client.write_all(request.as_bytes()).await.unwrap();
    let mut response = Vec::new();
    let mut block = [0; 4096];
    while !response
        .windows(b"HTTP/1.1 413".len())
        .any(|part| part == b"HTTP/1.1 413")
    {
        let read = tokio::time::timeout(Duration::from_secs(5), client.read(&mut block))
            .await
            .expect("pipelined response timed out")
            .unwrap();
        assert!(
            read > 0,
            "server closed before rejecting the oversized body"
        );
        response.extend_from_slice(&block[..read]);
        assert!(response.len() <= 64 * 1024);
    }
    assert!(response.starts_with(b"HTTP/1.1 200"));
    server.abort();
}

#[tokio::test]
async fn oversized_headers_are_rejected_before_routing() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(serve_with_header_deadline(
        listener,
        state(),
        Duration::from_secs(1),
    ));
    let mut client = TcpStream::connect(address).await.unwrap();
    let request = format!(
        "GET / HTTP/1.1\r\nHost: local\r\nX-Fill: {}\r\n\r\n",
        "a".repeat(40 * 1024)
    );
    client.write_all(request.as_bytes()).await.unwrap();

    let mut response = [0_u8; 1024];
    let read = tokio::time::timeout(Duration::from_secs(1), client.read(&mut response))
        .await
        .unwrap()
        .unwrap();
    assert!(read == 0 || !response[..read].starts_with(b"HTTP/1.1 200"));
    server.abort();
}

#[tokio::test]
async fn zero_idle_pool_opens_a_new_http1_connection_for_each_request() {
    // Runtime check of the pinned reqwest/Hyper setting used by the SDK's TLS
    // builder. Plain loopback cannot establish production certificate pinning.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let peer = tokio::spawn(async move {
        let mut held = Vec::new();
        for _ in 0..2 {
            let (mut socket, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
                .await
                .unwrap()
                .unwrap();
            let mut headers = Vec::new();
            let mut block = [0_u8; 1024];
            while !headers.windows(4).any(|part| part == b"\r\n\r\n") {
                let read = socket.read(&mut block).await.unwrap();
                assert!(read > 0 && headers.len() + read <= 8192);
                headers.extend_from_slice(&block[..read]);
            }
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                .await
                .unwrap();
            held.push(socket); // Keep the first peer alive to expose reuse.
        }
    });
    let client = reqwest::Client::builder()
        .no_proxy()
        .http1_only()
        .pool_max_idle_per_host(0)
        .build()
        .unwrap();
    for _ in 0..2 {
        let response = tokio::time::timeout(Duration::from_secs(5), client.get(&url).send())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        drop(response);
    }
    peer.await.unwrap();
}
