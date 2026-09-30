//! Detached ownership for accepted `/chat` reservations.
//!
//! Admission supplies the existing global-four generation permit and a separate
//! heavy resource lease shared with delivery. Delivery owns no generation slot.
//! This is staged lifetime plumbing, not an aggregate memory bound. The process
//! must install a content-suppressing panic hook (as `main` does): Tokio catches
//! unwinds only AFTER the hook runs.

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

/// Linear, synchronous access to the original reservation's terminal guard.
/// Dropping without `finish` refunds, including pre-poll cancellation and unwind.
/// This owns no resource/generation leases: those stay with the worker until ALL
/// captured input and rendering state is destroyed, even after settlement.
pub(crate) struct Settlement {
    terminal: Terminal,
}

impl Settlement {
    /// Consume verified adapter completion, not an intermediate usage event.
    /// `Ok` is trusted ONLY after successful finish, DONE and EOF with no upstream
    /// error; this capability cannot establish stream authenticity itself.
    /// Accounting rechecks arithmetic and refunds invalid usage. No error text
    /// is retained or formatted. Even accounting failure is a single attempt.
    pub(crate) fn finish(mut self, result: &Result<StreamUsage, InferenceError>) -> SettledReceipt {
        SettledReceipt {
            completion: self
                .terminal
                .finish(result.as_ref().ok().map(|usage| FinalUsage {
                    input_tokens: usage.input_tokens,
                    output_tokens: usage.output_tokens,
                    total_tokens: usage.total_tokens,
                }))
                .map_err(OwnerError::Accounting),
        }
    }
}

/// Evidence of a terminal accounting attempt, including refunds and errors.
/// Only `Settlement::finish` constructs this non-cloneable receipt. Work can
/// inspect but cannot replace its completion; only the owner unwraps it. Thus a
/// normal work return cannot report a fabricated success while its guard refunds.
/// This seal does NOT authenticate usage supplied to `finish`.
#[must_use = "return the receipt to the generation owner after inspecting its outcome"]
pub(crate) struct SettledReceipt {
    completion: Completion,
}

impl SettledReceipt {
    pub(crate) fn outcome(&self) -> &Completion {
        &self.completion
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
/// The route's first spawn is DETACHED PREFLIGHT,
/// before tokenization, not compose. Its explicit owner envelope drops owned
/// prompt-bearing work BEFORE this guard/leases. Preflight failure/deadline drops
/// this sole guard (no explicit refund plus guard); success moves this same value
/// synchronously into streaming_chat::compose, even if observation was dropped.
/// Neither a closed result channel nor reset/logout can veto accepted work.
///
/// Field order keeps the resource and generation leases until terminal cleanup.
pub(crate) struct ReservedGeneration {
    terminal: Terminal,
    _resources: Arc<OwnedSemaphorePermit>,
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
        resources: impl Into<Arc<OwnedSemaphorePermit>>,
    ) -> Self {
        Self {
            terminal: Terminal {
                accounting,
                id,
                armed: true,
            },
            _resources: resources.into(),
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

    /// Like `spawn`, but lets the owned future settle before completing a renderer
    /// that borrows its captured history. Consume the verified adapter to EOF,
    /// call `settlement.finish(&result)` synchronously, then ONLY when the
    /// receipt's `outcome()` is `Ok(Outcome::Settled { .. })` complete rendering /
    /// issue a next-turn token bound to the snapshotted model and conversation.
    /// Return that same receipt, not a work-supplied completion.
    /// Refunds and accounting errors must not mint tokens. Accounting releases
    /// its lock before `finish` returns; token issuance must not run under it.
    ///
    /// No lease is transferred to the capability or observer. Dropping delivery
    /// or observation cannot abort work. Dropping an unused capability refunds;
    /// after a terminal attempt, panic/cancel cannot undo it or attempt a refund.
    /// The owned delivery input may still be in its bounded startup phase; work
    /// (including startup) begins only after handoff to the detached worker.
    pub(crate) fn spawn_settling<T, F, Fut>(
        self,
        delivery: T,
        work: F,
    ) -> oneshot::Receiver<Completion>
    where
        T: Send + 'static,
        F: FnOnce(T, Settlement) -> Fut + Send + 'static,
        Fut: Future<Output = SettledReceipt> + Send + 'static,
    {
        supervise(self.spawn_settling_worker(delivery, work))
    }

    fn spawn_worker<F, Fut>(self, delivery: DeliveryTx, work: F) -> JoinHandle<Completion>
    where
        F: FnOnce(DeliveryTx) -> Fut + Send + 'static,
        Fut: Future<Output = Result<StreamUsage, InferenceError>> + Send + 'static,
    {
        self.spawn_settling_worker(delivery, move |delivery, settlement| async move {
            let result = work(delivery).await;
            settlement.finish(&result)
        })
    }

    fn spawn_settling_worker<T, F, Fut>(self, delivery: T, work: F) -> JoinHandle<Completion>
    where
        T: Send + 'static,
        F: FnOnce(T, Settlement) -> Fut + Send + 'static,
        Fut: Future<Output = SettledReceipt> + Send + 'static,
    {
        let Self {
            terminal,
            _resources,
            _generation,
        } = self;
        let settlement = Settlement { terminal };
        tokio::spawn(Worker {
            // The SAME guard moves into Worker before the factory can run.
            work: Box::pin(async move { work(delivery, settlement).await }),
            _resources,
            _generation,
        })
    }
}

// Explicit field drop order is essential, including before the first poll:
// destroy ALL work/captured input state before returning resource/slot credit.
// Do not rely on the unspecified field order of an async block's captures.
struct Worker<Fut> {
    work: Pin<Box<Fut>>,
    _resources: Arc<OwnedSemaphorePermit>,
    _generation: OwnedSemaphorePermit,
}

impl<Fut> Future for Worker<Fut>
where
    Fut: Future<Output = SettledReceipt>,
{
    type Output = Completion;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let receipt = std::task::ready!(self.get_mut().work.as_mut().poll(cx));
        Poll::Ready(receipt.completion)
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
