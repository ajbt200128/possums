//! Staged, detached generation ownership; deliberately not wired to `/chat`.
//!
//! Admission supplies the existing global-four generation permit and a separate
//! resource lease. Delivery owns neither. This is a lifetime guarantee, not an
//! aggregate memory bound. The process must install a content-suppressing panic
//! hook (as `main` does): Tokio catches unwinds only AFTER the hook runs.

use crate::{
    accounting::{Accounting, AccountingError, FinalUsage, Outcome},
    inference::{stream::StreamUsage, InferenceError},
    stream_owner::DeliveryTx,
};
use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tokio::{
    sync::{oneshot, OwnedSemaphorePermit},
    task::JoinHandle,
};

/// One synchronous terminal attempt, including when the task is never polled.
/// Only prompt-free accounting state is retained here; no inference data/errors.
struct Terminal {
    accounting: Arc<Accounting>,
    id: [u8; 32],
    armed: bool,
}

impl Terminal {
    fn finish(&mut self, usage: Option<FinalUsage>) -> Result<Outcome, AccountingError> {
        // Even an accounting error must not cause a second, conflicting attempt.
        // Such errors indicate a broken reservation/ledger invariant, not retry.
        self.armed = false;
        self.accounting.finish(self.id, usage)
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.finish(None);
        }
    }
}

/// Construct immediately after a *new* successful `Accounting::reserve`, without
/// an intervening await. Never construct for Duplicate or share refund ownership
/// with another guard. No second reservation is created here.
///
/// Before spawn, dropping this value refunds. At successful `tokio::spawn`, this
/// SAME guard and both leases belong to the worker. There is no disarm/ack gap in
/// which cancellation can refund an already-running generation.
///
/// Field order keeps the resource and generation leases until terminal cleanup.
pub(crate) struct ReservedGeneration {
    terminal: Terminal,
    _resources: OwnedSemaphorePermit,
    _generation: OwnedSemaphorePermit,
}

/// Fixed, content-free supervision result. Never carry a JoinError/panic payload
/// or upstream response/error text across this boundary.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum OwnerError {
    Accounting(AccountingError),
    Panicked,
    Cancelled,
}

type Completion = Result<Outcome, OwnerError>;

impl ReservedGeneration {
    pub(crate) fn new(
        accounting: Arc<Accounting>,
        id: [u8; 32],
        generation: OwnedSemaphorePermit,
        resources: OwnedSemaphorePermit,
    ) -> Self {
        Self {
            terminal: Terminal {
                accounting,
                id,
                armed: true,
            },
            _resources: resources,
            _generation: generation,
        }
    }

    /// Synchronous handoff. The work factory is invoked ONLY inside the spawned
    /// worker, after reservation and lease ownership have moved. Capture owned
    /// inference inputs in it, not an already-running task. It must consume the
    /// authenticated stream to terminal even when nonblocking delivery fails.
    ///
    /// `Ok` must come only from the verified adapter after successful finish,
    /// DONE and EOF, never from a parser/usage event alone. Missing/invalid usage
    /// and upstream errors are `Err`; no output-derived token counts or replay.
    /// The owner rechecks usage arithmetic through Accounting::finish.
    ///
    /// The returned receiver is observation only: dropping it or the HTTP body
    /// cannot abort the worker or release either lease. Only runtime/service
    /// shutdown may cancel the task; unwind/cancel cleanup refunds synchronously.
    pub(crate) fn spawn<F, Fut>(
        self,
        delivery: DeliveryTx,
        work: F,
    ) -> oneshot::Receiver<Completion>
    where
        F: FnOnce(DeliveryTx) -> Fut + Send + 'static,
        Fut: Future<Output = Result<StreamUsage, InferenceError>> + Send + 'static,
    {
        supervise(self.spawn_worker(delivery, work))
    }

    fn spawn_worker<F, Fut>(self, delivery: DeliveryTx, work: F) -> JoinHandle<Completion>
    where
        F: FnOnce(DeliveryTx) -> Fut + Send + 'static,
        Fut: Future<Output = Result<StreamUsage, InferenceError>> + Send + 'static,
    {
        tokio::spawn(Worker {
            work: Box::pin(async move { work(delivery).await }),
            owner: self,
        })
    }
}

// Explicit field drop order is essential, including before the first poll:
// destroy ALL work/captured input state before returning resource/slot credit.
// Do not rely on the unspecified field order of an async block's captures.
struct Worker<Fut> {
    work: Pin<Box<Fut>>,
    owner: ReservedGeneration,
}

impl<Fut> Future for Worker<Fut>
where
    Fut: Future<Output = Result<StreamUsage, InferenceError>>,
{
    type Output = Completion;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let result = std::task::ready!(this.work.as_mut().poll(cx));
        Poll::Ready(
            this.owner
                .terminal
                .finish(result.ok().map(|usage| FinalUsage {
                    input_tokens: usage.input_tokens,
                    output_tokens: usage.output_tokens,
                    total_tokens: usage.total_tokens,
                }))
                .map_err(OwnerError::Accounting),
        )
    }
}

fn supervise(worker: JoinHandle<Completion>) -> oneshot::Receiver<Completion> {
    let (tx, rx) = oneshot::channel();
    // Dropping this supervisor's handle detaches it. It always joins the worker,
    // even with no observer. Neither supervisor nor observer owns worker leases.
    // Cancelling the supervisor itself also only drops/detaches the worker handle.
    tokio::spawn(async move {
        let completion = match worker.await {
            Ok(result) => result,
            Err(error) if error.is_cancelled() => Err(OwnerError::Cancelled),
            Err(_) => Err(OwnerError::Panicked), // Never format the panic payload.
        };
        let _ = tx.send(completion);
    });
    rx
}

#[cfg(test)]
#[path = "generation_owner_tests.rs"]
mod tests;
