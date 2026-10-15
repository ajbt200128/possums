//! Compile-only real Arti boundary. Tests never construct a TorClient or launch a service.
use std::num::NonZeroUsize;

use arti_client::TorClient;
use futures::StreamExt;
use tokio_util::sync::CancellationToken;
use tor_cell::relaycell::msg::{Connected, End as RelayEnd, EndReason};
use tor_hscrypto::pk::{HsId, HsIdKey, HsIdKeypair};
use tor_hsservice::{HsIdKeypairSpecifier, HsIdPublicKeySpecifier, HsNickname, OnionServiceConfig};
use tor_keymgr::KeyMgr;
use tor_proto::stream::IncomingStreamRequest;

use crate::{run_bounded, serve_http, End, Failure};

/// Read-only preflight. No get_or_generate, fallback or mutation. The host must
/// exclusively own this state through launch: this is not a cross-process lock.
pub fn require_identity(
    manager: &KeyMgr,
    nickname: &HsNickname,
    approved: &HsId,
) -> Result<(), Failure> {
    let pair = manager
        .get::<HsIdKeypair>(&HsIdKeypairSpecifier::new(nickname.clone()))
        .map_err(|_| Failure::IdentityRead)?
        .ok_or(Failure::IdentityMissing)?;
    let public = manager
        .get::<HsIdKey>(&HsIdPublicKeySpecifier::new(nickname.clone()))
        .map_err(|_| Failure::IdentityRead)?
        .ok_or(Failure::IdentityMissing)?;
    let derived = HsIdKey::from(&pair);
    if derived.id() != public.id() || &public.id() != approved {
        return Err(Failure::IdentityMismatch);
    }
    Ok(())
}

/// Intentionally has no caller/executable in this spike. NOT deployment-ready:
/// Arti 0.47.0 can exit the entire host on a later unsupported consensus.
/// PreferredRuntime uses Tokio/rustls with this manifest; no separate Tor process is used.
/// Each bounded rendezvous handles streams serially, sacrificing multiplexing
/// throughput to keep a single auditable bound on handshake/circuit/handler owners.
pub async fn compile_only_http_embedding(
    client: &TorClient<tor_rtcompat::PreferredRuntime>,
    config: OnionServiceConfig,
    approved: &HsId,
    sessions: NonZeroUsize,
    cancel: CancellationToken,
) -> Result<End, Failure> {
    if cancel.is_cancelled() {
        return Ok(End::Cancelled);
    }
    require_identity(
        client.keymgr().map_err(|_| Failure::IdentityRead)?,
        config.nickname(),
        approved,
    )?;
    let (service, requests) = client
        .launch_onion_service(config)
        .map_err(|_| Failure::Launch)?
        .ok_or(Failure::Disabled)?;
    let result = run_bounded(requests, sessions, cancel, |request| async move {
        let mut streams = request
            .accept()
            .await
            .map_err(|_| Failure::RendezvousAccept)?;
        while let Some(stream) = streams.next().await {
            match stream.request() {
                IncomingStreamRequest::Begin(begin) if begin.port() == 80 => {
                    let io = stream
                        .accept(Connected::new_empty())
                        .await
                        .map_err(|_| Failure::StreamAccept)?;
                    serve_http(io).await?;
                }
                _ => stream
                    .reject(RelayEnd::new_with_reason(EndReason::DONE))
                    .await
                    .map_err(|_| Failure::StreamReject)?,
            }
        }
        Ok(())
    })
    .await;
    // Dropping the last service handle stops publication/new introductions, not
    // proof of instant network disappearance. All application futures are owned.
    drop(service);
    result
}
