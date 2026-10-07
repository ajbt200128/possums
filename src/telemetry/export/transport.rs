//! One test-only, numeric-loopback OTLP transport candidate. No runtime wiring.
//! Hyper owns HTTP framing; OTel/prost still own OTLP. There is no executor,
//! pool, retry, redirect, proxy, compression, DNS or detached connection driver.
use super::ATTEMPT_TIMEOUT;
use async_trait::async_trait;
use http::{Request, Response, Uri};
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::Bytes;
use hyper_util::rt::TokioIo;
use opentelemetry_http::{HttpClient, HttpError};
use std::{
    fmt,
    net::SocketAddr,
    pin::Pin,
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
    task::{Context, Poll},
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::TcpStream,
    sync::{watch, Mutex},
};

pub(super) const RESPONSE_LIMIT: usize = 64 * 1024;
pub(super) const MAX_OUTBOUND: usize = 6_576_128;

// Closed diagnostics: never retain or format an underlying error or metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(super) enum Failure {
    #[error("telemetry configuration rejected")]
    Configuration,
    #[error("telemetry attempt unavailable")]
    Unavailable,
    #[error("telemetry transport rejected")]
    Transport,
    #[error("telemetry response rejected")]
    Response,
    #[error("telemetry attempt expired")]
    Expired,
}

#[derive(Default)]
pub(super) struct Evidence {
    pub(super) connections: AtomicUsize,
    pub(super) live_io: AtomicUsize,
    pub(super) write_polls: AtomicUsize,
    pub(super) written: AtomicUsize,
    pub(super) pending_writes: AtomicUsize,
    pub(super) connecting: AtomicUsize,
    pub(super) received: AtomicUsize,
}

struct State {
    stopped: watch::Sender<bool>,
    active: Mutex<()>,
    evidence: Arc<Evidence>,
}

#[derive(Clone)]
pub(super) struct Client {
    address: SocketAddr,
    uri: Uri,
    state: Arc<State>,
    // Sole seam: deterministic pending connect, rather than non-loopback routing
    // or platform-dependent backlog exhaustion. Normal tests use real TCP.
    pub(super) stall_connect: bool,
    authority: Option<(Arc<AtomicU64>, u64, tokio::time::Instant)>,
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LoopbackTelemetryClient")
    }
}

impl Client {
    pub(super) fn new(address: SocketAddr) -> Result<Self, Failure> {
        if !address.ip().is_loopback() || address.port() == 0 {
            return Err(Failure::Configuration);
        }
        Ok(Self {
            address,
            uri: format!("http://{address}/v1/metrics")
                .parse()
                .map_err(|_| Failure::Configuration)?,
            state: Arc::new(State {
                stopped: watch::channel(false).0,
                active: Mutex::new(()),
                evidence: Arc::new(Evidence::default()),
            }),
            stall_connect: false,
            authority: None,
        })
    }

    pub(super) fn bind_epoch(
        &mut self,
        epoch: Arc<AtomicU64>,
        expected: u64,
        deadline: tokio::time::Instant,
    ) {
        self.authority = Some((epoch, expected, deadline));
    }

    pub(super) fn endpoint(&self) -> String {
        self.uri.to_string()
    }

    pub(super) fn evidence(&self) -> Arc<Evidence> {
        self.state.evidence.clone()
    }

    // Latch BEFORE waiting. All clones share both the latch and sole attempt
    // guard. Acquiring it certifies the owned attempt future/I/O has been dropped.
    // Callers must poll/join their attempt; this is not runtime kill wiring.
    pub(super) fn stop(&self) {
        self.state.stopped.send_replace(true);
    }

    pub(super) async fn cancel(&self) {
        self.stop();
        let _quiescent = self.state.active.lock().await;
    }

    async fn attempt(&self, request: Request<Bytes>) -> Result<Response<Bytes>, Failure> {
        if request.method() != http::Method::POST
            || request.uri() != &self.uri
            || request.body().len() > MAX_OUTBOUND
        {
            return Err(Failure::Configuration);
        }
        // Rebuild, do not inherit SDK headers/extensions (including User-Agent).
        // Full takes ownership of Bytes without cloning/copying its data.
        let request = Request::builder()
            .method(http::Method::POST)
            .uri("/v1/metrics") // origin-form on the one direct connection, never proxy-form
            .header(http::header::HOST, self.uri.authority().unwrap().as_str())
            .header(http::header::CONTENT_TYPE, "application/x-protobuf")
            .header(http::header::CONNECTION, "close")
            .body(Full::new(request.into_body()))
            .map_err(|_| Failure::Configuration)?;
        self.state
            .evidence
            .connecting
            .fetch_add(1, Ordering::SeqCst);
        if self.stall_connect {
            std::future::pending::<()>().await;
        }
        if !authorized(&self.authority) {
            return Err(Failure::Unavailable);
        }
        let stream = TcpStream::connect(self.address)
            .await
            .map_err(|_| Failure::Transport)?;
        let mut io = ObservedIo::new(stream, self.evidence());
        io.authority = self.authority.clone();
        let (mut sender, connection) = hyper::client::conn::http1::Builder::new()
            .max_buf_size(32 * 1024)
            .max_headers(64)
            .writev(true)
            .handshake(TokioIo::new(io))
            .await
            .map_err(|_| Failure::Transport)?;
        let exchange = async move {
            let response = sender
                .send_request(request)
                .await
                .map_err(|_| Failure::Transport)?;
            // OTel 0.29 ignores status unless HttpClient returns Err. Reject
            // before consuming hostile error bodies; never follow Location.
            if !response.status().is_success()
                || response.headers().len() > 64
                // Hyper's max_buf_size is not an exact allocation/header-length
                // certificate: a read into spare capacity can complete a larger
                // header before its incomplete-header limit check. Independently
                // reject oversized parsed fields; measure allocations separately.
                || response.headers().iter().try_fold(0_usize, |total, (name, value)| {
                    total.checked_add(name.as_str().len() + value.as_bytes().len() + 4)
                }).is_none_or(|bytes| bytes > 32 * 1024)
            {
                return Err(Failure::Response);
            }
            let status = response.status();
            let mut body = Limited::new(response.into_body(), RESPONSE_LIMIT);
            let mut bytes = Vec::new(); // NEVER reserve from Content-Length.
            while let Some(frame) = body.frame().await {
                let frame = frame.map_err(|_| Failure::Response)?;
                let data = frame.into_data().map_err(|_| Failure::Response)?;
                self.state
                    .evidence
                    .received
                    .fetch_add(data.len(), Ordering::SeqCst);
                bytes.extend_from_slice(&data);
            }
            Response::builder()
                .status(status)
                .body(Bytes::from(bytes))
                .map_err(|_| Failure::Response)
        };
        tokio::pin!(exchange);
        tokio::pin!(connection);
        // The driver and request/body are polled by THIS owned attempt. A clean
        // connection EOF can precede consumption of already-delivered body data.
        tokio::select! {
            result = &mut exchange => result,
            result = &mut connection => {
                result.map_err(|_| Failure::Transport)?;
                exchange.await
            }
        }
    }
}

#[async_trait]
impl HttpClient for Client {
    async fn send_bytes(&self, request: Request<Bytes>) -> Result<Response<Bytes>, HttpError> {
        let deadline = tokio::time::Instant::now() + ATTEMPT_TIMEOUT;
        let _guard = self
            .state
            .active
            .try_lock()
            .map_err(|_| Failure::Unavailable)?;
        let mut stopped = self.state.stopped.subscribe();
        if *stopped.borrow() || !authorized(&self.authority) {
            return Err(Failure::Unavailable.into());
        }
        // Box owns the actual future, not merely a pinned reference. Drop it
        // explicitly BEFORE releasing the guard or returning any acknowledgement.
        // Retain the SAME serialized allocation through response/disposal. This
        // is a shallow reference, not a body copy; composed peak tests keep it
        // live alongside the SDK tables, frozen slots and bounded response.
        let payload = request.body().clone();
        let mut attempt = Box::pin(self.attempt(request));
        let result = tokio::select! {
            biased;
            _ = stopped.wait_for(|stopped| *stopped) => Err(Failure::Unavailable),
            _ = tokio::time::sleep_until(deadline) => Err(Failure::Expired),
            result = &mut attempt => result,
        };
        drop(attempt);
        drop(payload);
        result.map_err(Into::into)
    }
}

// Test-only observation at the actual I/O boundary, not peer-buffer inference.
// Also witnesses that cancellation drops the real stream before acknowledging.
fn authorized(authority: &Option<(Arc<AtomicU64>, u64, tokio::time::Instant)>) -> bool {
    authority
        .as_ref()
        .is_none_or(|(epoch, expected, deadline)| {
            epoch.load(Ordering::SeqCst) == *expected && tokio::time::Instant::now() < *deadline
        })
}

struct ObservedIo {
    stream: TcpStream,
    evidence: Arc<Evidence>,
    authority: Option<(Arc<AtomicU64>, u64, tokio::time::Instant)>,
}

impl ObservedIo {
    fn new(stream: TcpStream, evidence: Arc<Evidence>) -> Self {
        evidence.connections.fetch_add(1, Ordering::SeqCst);
        evidence.live_io.fetch_add(1, Ordering::SeqCst);
        Self {
            stream,
            evidence,
            authority: None,
        }
    }

    fn record(&self, result: &Poll<std::io::Result<usize>>) {
        self.evidence.write_polls.fetch_add(1, Ordering::SeqCst);
        if result.is_pending() {
            self.evidence.pending_writes.fetch_add(1, Ordering::SeqCst);
        }
        if let Poll::Ready(Ok(n)) = result {
            self.evidence.written.fetch_add(*n, Ordering::SeqCst);
        }
    }
}

impl Drop for ObservedIo {
    fn drop(&mut self) {
        self.evidence.live_io.fetch_sub(1, Ordering::SeqCst);
    }
}

impl AsyncRead for ObservedIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_read(cx, buf)
    }
}

impl AsyncWrite for ObservedIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        if !authorized(&self.authority) {
            return Poll::Ready(Err(std::io::ErrorKind::ConnectionAborted.into()));
        }
        let result = Pin::new(&mut self.stream).poll_write(cx, buf);
        self.record(&result);
        result
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[std::io::IoSlice<'_>],
    ) -> Poll<std::io::Result<usize>> {
        if !authorized(&self.authority) {
            return Poll::Ready(Err(std::io::ErrorKind::ConnectionAborted.into()));
        }
        let result = Pin::new(&mut self.stream).poll_write_vectored(cx, bufs);
        self.record(&result);
        result
    }

    fn is_write_vectored(&self) -> bool {
        true
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}
