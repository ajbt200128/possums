//! Unwired local bridge: one owned permit, one owned attempt, no background task.
use super::{
    candidate, materialize,
    transport::{Client, Failure},
    ATTEMPT_TIMEOUT,
};
use crate::telemetry::{
    handoff::{Permit, Table},
    Clock,
};
use opentelemetry_sdk::metrics::exporter::PushMetricExporter;
use std::sync::atomic::Ordering;

struct StopOnDrop(Client);
impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.stop();
    }
}

// Fixed phase barriers for deterministic kill/ownership tests, not callbacks or
// sinks. None in ordinary bridge tests; no serving build includes this module.
#[derive(Default)]
pub(super) struct Pauses {
    pub(super) before: Option<tokio::sync::oneshot::Receiver<()>>,
    pub(super) materialized: Option<tokio::sync::oneshot::Receiver<()>>,
    pub(super) during: Option<materialize::Checkpoint>,
}

struct Ownership<'a, C: Clock> {
    client: StopOnDrop,
    permit: Permit<'a, C>,
}

pub(super) fn send<'a, C: Clock>(
    permit: Permit<'a, C>,
    mut client: Client,
    pauses: Pauses,
) -> impl std::future::Future<Output = Result<(), Failure>> + 'a {
    // Bind and arm drop even if the returned future is never polled. Fields
    // drop in order: stop all client clones before returning the frozen box.
    let deadline = tokio::time::Instant::now() + ATTEMPT_TIMEOUT;
    client.bind_epoch(permit.owner.epoch.clone(), permit.epoch, deadline);
    let ownership = Ownership {
        client: StopOnDrop(client),
        permit,
    };
    async move {
        let owner = ownership;
        let permit = &owner.permit;
        let client = &owner.client;
        let mut changed = permit.owner.handoff.changed.subscribe();
        let mut attempt = Box::pin(async {
            if !permit.valid() {
                return Err(Failure::Unavailable);
            }
            if let Some(before) = pauses.before {
                let _ = before.await;
            }
            if !permit.valid() {
                return Err(Failure::Unavailable);
            }
            // No state guard exists here. Supported conversion only; table remains
            // exclusively owned until SDK and serialized bytes have been disposed.
            let mut metrics = match permit.table.as_ref().unwrap() {
                Table::Request(table) => {
                    materialize::request_paused(table, permit.window, pauses.during)
                }
                Table::Infrastructure(table) => materialize::infrastructure(table, permit.window),
            }
            .ok_or(Failure::Unavailable)?;
            if let Some(materialized) = pauses.materialized {
                let _ = materialized.await;
            }
            // Synchronous materialization/SDK encoding cannot be preempted. Never
            // acknowledge until it returns; test the bound, don't detach on timeout.
            if tokio::time::Instant::now() >= deadline || !permit.valid() {
                return Err(Failure::Unavailable);
            }
            let exporter = candidate(client.0.clone())?;
            exporter
                .export(&mut metrics)
                .await
                .map_err(|_| Failure::Transport)
        });
        let result = tokio::select! {
            biased;
            _ = async {
                loop {
                    if permit.owner.epoch.load(Ordering::SeqCst) != permit.epoch { break; }
                    if changed.changed().await.is_err() { break; }
                }
            } => Err(Failure::Unavailable),
            _ = tokio::time::sleep_until(deadline) => Err(Failure::Expired),
            result = &mut attempt => result,
        };
        client.0.stop();
        drop(attempt); // request, connection, serialized body and SDK data
        client.0.cancel().await; // actual sole transport ownership, not SDK shutdown
        drop(owner);
        result
    }
}

#[cfg(test)]
mod tests;
