//! Offline transport feasibility only. No network entry point, trust contract or gateway routes.
use std::{future::Future, num::NonZeroUsize};

use axum::{routing::get, Router};
use futures::{Stream, StreamExt, TryStreamExt};
use hyper_util::rt::TokioIo;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::sync::CancellationToken;
use tower::Service;

#[cfg(feature = "arti")]
pub mod embedding;

/// Closed categories; never retain or format an SDK/parser/I/O cause.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    IdentityRead,
    IdentityMissing,
    IdentityMismatch,
    Launch,
    Disabled,
    RendezvousAccept,
    StreamAccept,
    StreamReject,
    HttpServe,
    OpaqueCopy,
}

impl Failure {
    pub fn diagnostic(self) -> &'static str {
        match self {
            Self::IdentityRead => "arti_identity_read: key manager read failed; cause unknown; check protected state offline before launch.",
            Self::IdentityMissing => "arti_identity_missing: approved identity key material absent; restore protected state; do not generate a replacement.",
            Self::IdentityMismatch => "arti_identity_mismatch: stored key pair/public key/approved identity disagree; stop and review protected state offline.",
            Self::Launch => "arti_launch: service launch failed; cause unknown; stop and review configuration offline.",
            Self::Disabled => "arti_disabled: service configuration disabled; review configuration before deliberate launch.",
            Self::RendezvousAccept => "arti_rendezvous_accept: rendezvous establishment failed; cause unknown; stop this prototype; no replay.",
            Self::StreamAccept => "arti_stream_accept: stream acceptance failed; cause unknown; stop this prototype; no replay.",
            Self::StreamReject => "arti_stream_reject: stream rejection failed; cause unknown; stop this prototype.",
            Self::HttpServe => "arti_http_serve: HTTP connection failed; cause unknown; no gateway submission is implemented here.",
            Self::OpaqueCopy => "arti_opaque_copy: byte forwarding failed; cause unknown; remote outcome unknown; no replay.",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {
    Exhausted,
    Cancelled,
}

/// One owner, no spawned/detached tasks or application queue. Dropping this future
/// drops all admitted work. The bound includes pending handshakes and active sessions.
/// First failure stops this prototype, preserving its category (not a production policy).
pub async fn run_bounded<S, H, F>(
    requests: S,
    limit: NonZeroUsize,
    cancel: CancellationToken,
    handle: H,
) -> Result<End, Failure>
where
    S: Stream,
    H: FnMut(S::Item) -> F,
    F: Future<Output = Result<(), Failure>>,
{
    // A pre-existing stop must not poll the request source or call the handler.
    if cancel.is_cancelled() {
        return Ok(End::Cancelled);
    }
    let work = requests
        .take_until(cancel.cancelled())
        .map(Ok)
        .try_for_each_concurrent(limit.get(), handle);
    tokio::pin!(work);
    tokio::select! {
        biased;
        result = &mut work => result.map(|()| {
            if cancel.is_cancelled() { End::Cancelled } else { End::Exhausted }
        }),
        _ = cancel.cancelled() => Ok(End::Cancelled),
    }
}

/// Same TokioIo -> Hyper -> Axum adaptation as the official Arti sample.
/// Synthetic fixed response only, never the production authenticated /v1 API.
pub async fn serve_http<I>(io: I) -> Result<(), Failure>
where
    I: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let router = Router::new().route("/", get(|| async { "synthetic-only" }));
    let service = hyper::service::service_fn(move |request| router.clone().call(request));
    hyper::server::conn::http1::Builder::new()
        .keep_alive(false)
        .serve_connection(TokioIo::new(io), service)
        .await
        .map_err(|_| Failure::HttpServe)
}

/// Blind byte bridge: no TLS termination, HTTP parsing, DNS or dialer. The caller
/// would supply an independently approved endpoint transport; this spike supplies
/// only synthetic streams. Tokio uses fixed 8 KiB buffers in each direction.
pub async fn forward_opaque<A, B>(mut onion: A, mut endpoint: B) -> Result<(), Failure>
where
    A: AsyncRead + AsyncWrite + Unpin,
    B: AsyncRead + AsyncWrite + Unpin,
{
    tokio::io::copy_bidirectional(&mut onion, &mut endpoint)
        .await
        .map(|_| ())
        .map_err(|_| Failure::OpaqueCopy)
}
