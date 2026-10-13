//! Synthetic source/loopback checks, not attestation, live catalog or export evidence.
use super::*;
use crate::catalog::CatalogError;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::oneshot,
};

const HOSTILE: &str = "private-prompt credential-canary https://private.invalid/account";
const VALID: &[u8] = br#"{"object":"list","data":[{"id":"m","type":"chat","context_window":10,"endpoints":["/v1/chat/completions"],"pricing":{"inputTokenPricePer1M":1,"outputTokenPricePer1M":2,"requestPrice":0}}]}"#;

fn client() -> reqwest::Client {
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder().no_proxy().build().unwrap()
}

async fn response_from(
    raw: &'static [u8],
    keep_open: bool,
) -> (
    reqwest::Response,
    oneshot::Sender<()>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (release, held) = oneshot::channel();
    let peer = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0; 1024];
        let _ = socket.read(&mut request).await.unwrap();
        socket.write_all(raw).await.unwrap();
        if keep_open {
            let _ = held.await;
        }
    });
    let response = client()
        .get(format!("http://{address}/"))
        .send()
        .await
        .unwrap();
    (response, release, peer)
}

#[tokio::test]
async fn http_refusal_keeps_status_and_does_not_wait_for_hostile_body() {
    let (response, release, peer) = response_from(
        b"HTTP/1.1 503 Unavailable\r\nContent-Length: 999999\r\n\r\nprivate-prompt credential-canary https://private.invalid/account",
        true,
    ).await;
    let failure = collect_catalog_response(response, 5, Instant::now())
        .await
        .unwrap_err();
    assert_eq!(failure, CatalogFailure::HttpRefused(503));
    assert_eq!(failure.stage(), "catalog_http");
    assert!(failure.message().contains("503"));
    assert!(!failure.message().contains("underlying cause is unknown"));
    assert_private(failure);
    let _ = release.send(());
    peer.await.unwrap();
}

#[tokio::test]
async fn refusal_never_polls_the_response_body() {
    let polls = Arc::new(AtomicUsize::new(0));
    let observed = polls.clone();
    let body = reqwest::Body::wrap_stream(futures_util::stream::poll_fn(move |_| {
        observed.fetch_add(1, Ordering::SeqCst);
        std::task::Poll::Ready(Some(Err::<Vec<u8>, _>(std::io::Error::other(HOSTILE))))
    }));
    let response = http::Response::builder()
        .status(403)
        .body(body)
        .unwrap()
        .into();
    assert_eq!(
        collect_catalog_response(response, 5, Instant::now())
            .await
            .unwrap_err(),
        CatalogFailure::HttpRefused(403)
    );
    assert_eq!(polls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn declared_and_chunked_limits_preserve_source_and_helper_projections() {
    for raw in [
        b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n".as_slice(),
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n6\r\n123456\r\n0\r\n\r\n".as_slice(),
    ] {
        let (response, release, peer) = response_from(raw, true).await;
        let failure =
            collect_catalog_response(response, 5, Instant::now() + Duration::from_secs(1))
                .await
                .unwrap_err();
        assert_eq!(failure, CatalogFailure::BodyLimit);
        assert_eq!(failure.wire(), InferenceFailure::CatalogFailed);
        let _ = release.send(());
        peer.await.unwrap();
        let (response, release, peer) = response_from(raw, true).await;
        assert!(matches!(
            collect_bounded_response(response, 5, Instant::now() + Duration::from_secs(1)).await,
            Err(InferenceError::InvalidResponse)
        ));
        let _ = release.send(());
        peer.await.unwrap();
    }
}

#[tokio::test]
async fn stalled_body_preserves_deadline_and_legacy_unavailable() {
    for checked in [true, false] {
        let (response, release, peer) = response_from(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1\r\na\r\n",
            true,
        )
        .await;
        let deadline = Instant::now() + Duration::from_millis(30);
        if checked {
            assert_eq!(
                collect_catalog_response(response, 5, deadline)
                    .await
                    .unwrap_err(),
                CatalogFailure::BodyDeadline
            );
        } else {
            assert!(matches!(
                collect_bounded_response(response, 5, deadline).await,
                Err(InferenceError::Unavailable)
            ));
        }
        let _ = release.send(());
        peer.await.unwrap();
    }
}

#[tokio::test]
async fn premature_closed_body_is_unknown_not_a_guessed_cause() {
    for checked in [true, false] {
        let (response, release, peer) =
            response_from(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\na", false).await;
        let deadline = Instant::now() + Duration::from_secs(1);
        if checked {
            let failure = collect_catalog_response(response, 5, deadline)
                .await
                .unwrap_err();
            assert_eq!(failure, CatalogFailure::BodyUnknown);
            assert!(failure.message().contains("underlying cause is unknown"));
        } else {
            assert!(matches!(
                collect_bounded_response(response, 5, deadline).await,
                Err(InferenceError::Unavailable)
            ));
        }
        drop(release);
        peer.await.unwrap();
    }
}

#[tokio::test]
async fn synthetic_hostile_sdk_body_error_is_not_retained() {
    let response = http::Response::new(reqwest::Body::wrap_stream(futures_util::stream::iter([
        Err::<Vec<u8>, _>(std::io::Error::other(HOSTILE)),
    ])))
    .into();
    let failure = collect_catalog_response(response, 5, Instant::now() + Duration::from_secs(1))
        .await
        .unwrap_err();
    assert_eq!(failure, CatalogFailure::BodyUnknown);
    assert_private(failure);
}

#[tokio::test]
async fn supported_reqwest_body_timeout_is_a_deadline_not_unknown() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (release, held) = oneshot::channel::<()>();
    let peer = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0; 1024];
        let _ = socket.read(&mut request).await.unwrap();
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\n")
            .await
            .unwrap();
        let _ = held.await;
    });
    let response = client()
        .get(format!("http://{address}/"))
        .timeout(Duration::from_millis(100))
        .send()
        .await
        .unwrap();
    assert_eq!(
        collect_catalog_response(response, 5, Instant::now() + Duration::from_secs(5))
            .await
            .unwrap_err(),
        CatalogFailure::BodyDeadline
    );
    let _ = release.send(());
    peer.await.unwrap();
}

#[tokio::test]
async fn checked_collector_accepts_chunked_and_absent_length_at_exact_limit() {
    for raw in [
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nhe\r\n3\r\nllo\r\n0\r\n\r\n"
            .as_slice(),
        b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nhello".as_slice(),
    ] {
        let (response, release, peer) = response_from(raw, false).await;
        assert_eq!(
            collect_catalog_response(response, 5, Instant::now() + Duration::from_secs(1))
                .await
                .unwrap(),
            b"hello"
        );
        drop(release);
        peer.await.unwrap();
    }
}

// Only the old required trait methods are implemented: external fixtures need no edits.
struct LegacyFixture {
    bytes: &'static [u8],
    failure: Option<InferenceFailure>,
    calls: AtomicUsize,
}
impl LegacyFixture {
    fn new(bytes: &'static [u8], failure: Option<InferenceFailure>) -> Self {
        Self {
            bytes,
            failure,
            calls: AtomicUsize::new(0),
        }
    }
}
#[async_trait]
impl Inference for LegacyFixture {
    async fn catalog(&self) -> Result<Vec<u8>, InferenceError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match self.failure {
            Some(InferenceFailure::InferenceUnavailable) => Err(InferenceError::Unavailable),
            Some(InferenceFailure::UpstreamResponseInvalid) => Err(InferenceError::InvalidResponse),
            Some(failure) => Err(failure.into()),
            None => Ok(self.bytes.to_vec()),
        }
    }
    async fn count_tokens(
        &self,
        _: &str,
        _: &[Message],
        _: Arc<crate::telemetry::hooks::Lease>,
    ) -> Result<u64, InferenceError> {
        panic!("catalog diagnostics must not initiate inference")
    }
    fn verification_document(&self) -> Result<serde_json::Value, InferenceError> {
        panic!("catalog diagnostics must not initiate verification")
    }
}

#[tokio::test]
async fn trait_default_preserves_legacy_failures_and_wire_codes() {
    for (failure, code) in [
        (
            InferenceFailure::InferenceUnavailable,
            "inference_unavailable",
        ),
        (
            InferenceFailure::UpstreamResponseInvalid,
            "upstream_response_invalid",
        ),
        (InferenceFailure::VerificationFailed, "verification_failed"),
        (InferenceFailure::CatalogFailed, "catalog_failed"),
    ] {
        let fixture = LegacyFixture::new(&[], Some(failure));
        assert_eq!(
            fixture.catalog_checked().await.unwrap_err(),
            CatalogFailure::Legacy(failure)
        );
        assert_eq!(
            authenticated_catalog_checked(&fixture, 0)
                .await
                .unwrap_err(),
            CatalogFailure::Legacy(failure)
        );
        let wire = authenticated_catalog(&fixture, 0)
            .await
            .unwrap_err()
            .failure();
        assert_eq!(wire, failure);
        assert_eq!(serde_json::to_value(wire).unwrap(), code);
        assert_eq!(fixture.calls.load(Ordering::SeqCst), 3);
    }
}

#[tokio::test]
async fn authenticated_checked_parser_preserves_invalid_and_overflow() {
    for (bytes, error) in [
        (HOSTILE.as_bytes(), CatalogError::Invalid),
        (br#"{"object":"list","data":[{"id":"private-prompt","type":"chat","context_window":10,"endpoints":["/v1/chat/completions"],"pricing":{"inputTokenPricePer1M":1e100,"outputTokenPricePer1M":1,"requestPrice":0}}]}"#.as_slice(), CatalogError::Overflow),
    ] {
        let fixture = LegacyFixture::new(bytes, None);
        let failure = authenticated_catalog_checked(&fixture, 0).await.unwrap_err();
        assert_eq!(failure, CatalogFailure::Validation(error));
        assert_private(failure);
        assert_eq!(authenticated_catalog(&fixture, 0).await.unwrap_err().failure(), InferenceFailure::CatalogFailed);
    }
    let fixture = LegacyFixture::new(VALID, None);
    assert_eq!(
        authenticated_catalog_checked(&fixture, 0).await.unwrap(),
        authenticated_catalog(&fixture, 0).await.unwrap()
    );
}

struct CheckedFixture(CatalogFailure);
#[async_trait]
impl Inference for CheckedFixture {
    async fn catalog(&self) -> Result<Vec<u8>, InferenceError> {
        self.catalog_checked()
            .await
            .map_err(|failure| failure.wire().into())
    }
    async fn catalog_checked(&self) -> Result<Vec<u8>, CatalogFailure> {
        Err(self.0)
    }
    async fn count_tokens(
        &self,
        _: &str,
        _: &[Message],
        _: Arc<crate::telemetry::hooks::Lease>,
    ) -> Result<u64, InferenceError> {
        panic!("catalog diagnostics must not initiate inference")
    }
    fn verification_document(&self) -> Result<serde_json::Value, InferenceError> {
        Err(InferenceError::Unavailable)
    }
}

#[tokio::test]
async fn checked_override_and_legacy_wrapper_are_one_way_and_do_not_recurse() {
    for failure in failures() {
        let fixture = CheckedFixture(failure);
        assert_eq!(
            authenticated_catalog_checked(&fixture, 0)
                .await
                .unwrap_err(),
            failure
        );
        assert_eq!(
            fixture.catalog().await.unwrap_err().failure(),
            failure.wire()
        );
        assert_eq!(
            authenticated_catalog(&fixture, 0)
                .await
                .unwrap_err()
                .failure(),
            failure.wire()
        );
    }
}

fn failures() -> [CatalogFailure; 13] {
    [
        CatalogFailure::Legacy(InferenceFailure::InferenceUnavailable),
        CatalogFailure::Legacy(InferenceFailure::UpstreamResponseInvalid),
        CatalogFailure::Legacy(InferenceFailure::VerificationFailed),
        CatalogFailure::Validation(CatalogError::Invalid),
        CatalogFailure::Validation(CatalogError::UnknownModel),
        CatalogFailure::Validation(CatalogError::ContextExceeded),
        CatalogFailure::Validation(CatalogError::Overflow),
        CatalogFailure::SendUnknown,
        CatalogFailure::SendDeadline,
        CatalogFailure::HttpRefused(503),
        CatalogFailure::BodyUnknown,
        CatalogFailure::BodyDeadline,
        CatalogFailure::BodyLimit,
    ]
}

fn assert_private(failure: CatalogFailure) {
    let diagnostic = format!(
        "{failure:?} {failure} {} {} {}",
        failure.code(),
        failure.stage(),
        failure.message()
    );
    assert!(diagnostic.len() < 1024);
    for forbidden in [
        "private-prompt",
        "credential-canary",
        "private.invalid",
        "https://",
        "refunded",
        "billing=settled",
        "replayed=true",
        "charged=",
    ] {
        assert!(!diagnostic.contains(forbidden));
    }
    assert!(!serde_json::to_string(&failure.wire())
        .unwrap()
        .contains(HOSTILE));
}

#[test]
fn source_diagnostics_are_copy_closed_actionable_and_keep_wire_details_frozen() {
    fn require_copy<T: Copy>() {}
    require_copy::<CatalogFailure>();
    for failure in failures() {
        assert_private(failure);
        assert!(!failure.code().is_empty() && !failure.stage().is_empty());
        assert!(failure.message().contains("constraint:"));
        assert!(failure.message().contains("Not replayed."));
        assert!(failure.message().contains("Billing/outcome unknown"));
        assert!(failure.message().contains("may incur another charge"));
        assert!(
            failure.message().contains("Report")
                || failure.message().contains("report")
                || failure.message().contains("Reduce")
                || failure.message().contains("Refresh")
        );
        if !matches!(failure, CatalogFailure::Legacy(_)) {
            assert_eq!(failure.wire(), InferenceFailure::CatalogFailed);
            assert_eq!(
                serde_json::to_value(failure.wire()).unwrap(),
                "catalog_failed"
            );
            assert_eq!(
                InferenceError::from(failure.wire()).to_string(),
                "The authenticated model catalog is unavailable or invalid."
            );
        }
    }
}
