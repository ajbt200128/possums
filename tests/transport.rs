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
    inference::{Generation, Inference, InferenceError, Message},
    web::{router_with_body_deadline, serve_with_header_deadline, AppState},
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
fn accepted_gateway_evidence_exceeds_proposed_control_lane() {
    use possums::attestation::{load_evidence, validate_evidence};

    // 520 chains of 62 singleton objects: legal depth 64 including the root
    // object and quote array; 65,011 lexical nodes, below the 65,536 guard.
    // BTreeMap storage, not serialized length, dominates this accepted input.
    // load_evidence + validate_evidence exercise the same structural scan,
    // deserialization and metadata checks as the production helper-output path.
    // This blocks the proposed 16-MiB control lane before response allocation;
    // it is not a whole-process memory measurement or a safe partition proof.
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
    let (evidence, retained, peak) =
        allocation_probe::measure(|| validate_evidence(load_evidence(&path).unwrap(), 1).unwrap());
    std::fs::remove_file(path).unwrap();
    assert_eq!(evidence.quote.as_array().unwrap().len(), 520);
    println!(
        "accepted evidence: wire={} retained={retained} peak={peak} bytes",
        document.len()
    );
    assert!(retained > 16 * 1024 * 1024, "retained {retained} bytes");
    assert!(peak >= retained);
    // This is an accepted Rust evidence-boundary counterexample, not a claim
    // that synthetic evidence passes cryptographic appraisal by the helper.
}

struct UnusedInference;

#[async_trait]
impl Inference for UnusedInference {
    async fn catalog(&self) -> Result<Vec<u8>, InferenceError> {
        Err(InferenceError::Unavailable)
    }

    async fn count_tokens(&self, _: &str, _: &[Message]) -> Result<u64, InferenceError> {
        unreachable!()
    }

    async fn generate(&self, _: &Model, _: &[Message]) -> Result<Generation, InferenceError> {
        unreachable!()
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
                .uri("/login")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
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
