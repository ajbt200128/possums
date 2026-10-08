//! Direct OTLP transport. Hyper/rustls own HTTP/TLS, OTel/prost own encoding.
//! No pool, retry, redirect, proxy, compression or detached HTTP/TLS driver.
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
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    task::{Context, Poll},
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::TcpStream,
    sync::{watch, Mutex},
};

pub(in crate::telemetry) const RESPONSE_LIMIT: usize = 64 * 1024;
pub(in crate::telemetry) const MAX_OUTBOUND: usize = 6_576_128;

// Closed diagnostics: never retain or format an underlying error or metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(in crate::telemetry) enum Failure {
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

#[cfg(test)]
#[derive(Default)]
pub(in crate::telemetry) struct Evidence {
    pub(in crate::telemetry) connections: std::sync::atomic::AtomicUsize,
    pub(in crate::telemetry) live_io: std::sync::atomic::AtomicUsize,
    pub(in crate::telemetry) write_polls: std::sync::atomic::AtomicUsize,
    pub(in crate::telemetry) written: std::sync::atomic::AtomicUsize,
    pub(in crate::telemetry) pending_writes: std::sync::atomic::AtomicUsize,
    pub(in crate::telemetry) connecting: std::sync::atomic::AtomicUsize,
    pub(in crate::telemetry) received: std::sync::atomic::AtomicUsize,
}

struct State {
    stopped: watch::Sender<bool>,
    active: Mutex<()>,
    #[cfg(test)]
    evidence: Arc<Evidence>,
}

#[derive(Clone)]
pub(in crate::telemetry) struct Client {
    address: Option<SocketAddr>,
    tls: Option<Arc<rustls::ClientConfig>>,
    server_name: rustls::pki_types::ServerName<'static>,
    credential: Option<http::HeaderValue>,
    uri: Uri,
    state: Arc<State>,
    // Sole seam: deterministic pending connect, rather than non-loopback routing
    // or platform-dependent backlog exhaustion. Normal tests use real TCP.
    #[cfg(test)]
    pub(in crate::telemetry) stall_connect: bool,
    authority: Option<(Arc<AtomicU64>, u64, tokio::time::Instant)>,
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TelemetryClient")
    }
}

impl Client {
    #[cfg(test)]
    pub(in crate::telemetry) fn new(address: SocketAddr) -> Result<Self, Failure> {
        if !address.ip().is_loopback() || address.port() == 0 {
            return Err(Failure::Configuration);
        }
        Ok(Self {
            address: Some(address),
            tls: None,
            server_name: "api.honeycomb.io".try_into().unwrap(),
            credential: None,
            uri: format!("http://{address}/v1/metrics")
                .parse()
                .map_err(|_| Failure::Configuration)?,
            state: Arc::new(State {
                stopped: watch::channel(false).0,
                active: Mutex::new(()),
                #[cfg(test)]
                evidence: Arc::new(Evidence::default()),
            }),
            stall_connect: false,
            authority: None,
        })
    }

    pub(in crate::telemetry) fn honeycomb(credential: http::HeaderValue) -> Result<Self, Failure> {
        let roots =
            rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let mut tls = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|_| Failure::Configuration)?
        .with_root_certificates(roots)
        .with_no_client_auth();
        tls.alpn_protocols = vec![b"http/1.1".to_vec()];
        // No resumption cache or TLS early-data replay.
        tls.resumption = rustls::client::Resumption::disabled();
        Ok(Self {
            address: None,
            uri: Uri::from_static("https://api.honeycomb.io/v1/metrics"),
            tls: Some(Arc::new(tls)),
            server_name: "api.honeycomb.io"
                .try_into()
                .map_err(|_| Failure::Configuration)?,
            credential: Some(credential),
            state: Arc::new(State {
                stopped: watch::channel(false).0,
                active: Mutex::new(()),
                #[cfg(test)]
                evidence: Arc::new(Evidence::default()),
            }),
            #[cfg(test)]
            stall_connect: false,
            authority: None,
        })
    }

    #[cfg(test)]
    pub(in crate::telemetry) fn tls_fixture(
        address: SocketAddr,
        tls: rustls::ClientConfig,
        name: &'static str,
    ) -> Self {
        assert!(address.ip().is_loopback());
        let mut client =
            Self::honeycomb(http::HeaderValue::from_static("synthetic-key-canary")).unwrap();
        client.address = Some(address);
        client.tls = Some(Arc::new(tls));
        client.server_name = name.try_into().unwrap();
        client
    }

    pub(in crate::telemetry) fn fresh(&self) -> Self {
        let mut client = self.clone();
        client.state = Arc::new(State {
            stopped: watch::channel(false).0,
            active: Mutex::new(()),
            #[cfg(test)]
            evidence: self.evidence(),
        });
        client.authority = None;
        client
    }

    pub(in crate::telemetry) fn bind_epoch(
        &mut self,
        epoch: Arc<AtomicU64>,
        expected: u64,
        deadline: tokio::time::Instant,
    ) {
        self.authority = Some((epoch, expected, deadline));
    }

    pub(in crate::telemetry) fn endpoint(&self) -> String {
        self.uri.to_string()
    }

    #[cfg(test)]
    pub(in crate::telemetry) fn evidence(&self) -> Arc<Evidence> {
        self.state.evidence.clone()
    }

    // Latch BEFORE waiting. All clones share both the latch and sole attempt
    // guard. Acquiring it certifies the owned attempt future/I/O has been dropped.
    // Callers must poll/join their attempt; this is not runtime kill wiring.
    pub(in crate::telemetry) fn stop(&self) {
        self.state.stopped.send_replace(true);
    }

    pub(in crate::telemetry) async fn cancel(&self) {
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
        let mut builder = Request::builder()
            .method(http::Method::POST)
            .uri("/v1/metrics") // origin-form on the one direct connection, never proxy-form
            .header(http::header::HOST, self.uri.authority().unwrap().as_str())
            .header(http::header::CONTENT_TYPE, "application/x-protobuf")
            .header(http::header::CONNECTION, "close");
        if let Some(credential) = &self.credential {
            builder = builder.header("x-honeycomb-team", credential);
        }
        let request = builder
            .body(Full::new(request.into_body()))
            .map_err(|_| Failure::Configuration)?;
        #[cfg(test)]
        self.state
            .evidence
            .connecting
            .fetch_add(1, Ordering::SeqCst);
        #[cfg(test)]
        if self.stall_connect {
            std::future::pending::<()>().await;
        }
        if !authorized(&self.authority) {
            return Err(Failure::Unavailable);
        }
        let stream = match self.address {
            Some(address) => TcpStream::connect(address).await,
            // Unqualified for rollout: Tokio resolves on its blocking pool, whose
            // OS lookup can outlive cancellation. Production release is closed
            // in runtime.rs until owned DNS is qualified; see verification.
            // Never accept a configured host or pass credentials to DNS.
            None => TcpStream::connect(("api.honeycomb.io", 443)).await,
        }
        .map_err(|_| Failure::Transport)?;
        let mut io = ObservedIo::new(
            stream,
            #[cfg(test)]
            self.evidence(),
        );
        io.authority = self.authority.clone();
        let io: Box<dyn Io> = if let Some(tls) = &self.tls {
            let mut stream = tokio_rustls::TlsConnector::from(tls.clone())
                .connect(self.server_name.clone(), io)
                .await
                .map_err(|_| Failure::Transport)?;
            stream.get_mut().1.set_buffer_limit(Some(32 * 1024));
            Box::new(stream)
        } else {
            Box::new(io)
        };
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
                #[cfg(test)]
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
    #[cfg(test)]
    evidence: Arc<Evidence>,
    authority: Option<(Arc<AtomicU64>, u64, tokio::time::Instant)>,
}

impl ObservedIo {
    fn new(stream: TcpStream, #[cfg(test)] evidence: Arc<Evidence>) -> Self {
        #[cfg(test)]
        evidence.connections.fetch_add(1, Ordering::SeqCst);
        #[cfg(test)]
        evidence.live_io.fetch_add(1, Ordering::SeqCst);
        Self {
            stream,
            #[cfg(test)]
            evidence,
            authority: None,
        }
    }

    #[cfg(test)]
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

#[cfg(test)]
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
        #[cfg(test)]
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
        #[cfg(test)]
        self.record(&result);
        result
    }

    fn is_write_vectored(&self) -> bool {
        true
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        if !authorized(&self.authority) {
            return Poll::Ready(Err(std::io::ErrorKind::ConnectionAborted.into()));
        }
        Pin::new(&mut self.stream).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}

trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}
