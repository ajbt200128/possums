//! Detached settlement ownership for accepted `/v1/chat/completions` reservations.
//!
//! Admission supplies a generation lifetime marker and the existing defensive
//! heavy resource lease shared with delivery. No separate generation quota exists.
//! This is staged lifetime plumbing, not an aggregate memory bound. The process
//! must install a content-suppressing panic hook (as `main` does): Tokio catches
//! unwinds only AFTER the hook runs.

use crate::telemetry::{hooks::Lease, GenerationTerminal, OwnedObservation, QualifiedModel};
use crate::{
    accounting::{Accounting, AccountingError, FinalUsage, Outcome},
    failures::InterfaceError,
    inference::{stream::StreamUsage, InferenceError, InferenceFailure},
    stream_owner::DeliveryTx,
};
use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tokio::{sync::oneshot, task::JoinHandle};

/// One synchronous terminal attempt, including when the task is never polled.
/// Only prompt-free accounting state is retained here; no inference data/errors.
struct Terminal {
    accounting: Arc<Accounting>,
    id: [u8; 32],
    armed: bool,
    evidence: Option<crate::inference::forward::Evidence>,
    observation: Option<OwnedObservation>,
    failure: Option<GenerationTerminal>,
    primary: Option<InterfaceError>,
    expected_interruption: bool,
    worker_handoff: bool,
    model: Option<QualifiedModel>,
    lifecycle: Option<crate::lifecycle::Lifecycle>,
}

impl Terminal {
    fn finish(&mut self, usage: Option<FinalUsage>) -> Result<Outcome, AccountingError> {
        // Even an accounting error must not cause a second, conflicting attempt.
        // Such errors indicate a broken reservation/ledger invariant, not retry.
        self.armed = false;
        let usage = self
            .evidence
            .as_ref()
            .map(|e| FinalUsage {
                input_tokens: e.usage.input_tokens,
                output_tokens: e.usage.output_tokens,
                total_tokens: e.usage.total_tokens,
            })
            .or(usage);
        let cached = self.evidence.as_ref().map_or(0, |e| e.cached_tokens);
        let result = self.accounting.finish_cached(self.id, usage, cached);
        if result.is_err() {
            if let Some(lifecycle) = &self.lifecycle {
                lifecycle.accounting_failed();
            }
        }
        // Accounting has returned and released its lock. Numeric observations
        // belong to the terminal window, even when the original record retired.
        if let Some(observation) = &self.observation {
            let metrics = observation.metrics();
            if let Some(primary) = self.primary {
                metrics.interface_error(primary);
            }
            if result.is_err() {
                metrics.interface_error(InterfaceError::SettlementFailed);
            }
            if let (Some(model), Some(usage)) = (self.model, usage) {
                metrics.observed_usage(
                    model,
                    usage.input_tokens,
                    usage.output_tokens,
                    usage.total_tokens,
                    self.evidence
                        .as_ref()
                        .and_then(|e| e.cache_usage_available.then_some(e.cached_tokens)),
                );
            }
        }
        let terminal = match (&result, usage.is_some(), self.failure) {
            (Ok(Outcome::Settled { .. }), _, Some(failure)) => failure,
            (Ok(Outcome::Settled { .. }), true, None) => GenerationTerminal::Success,
            (Ok(Outcome::Refunded), false, Some(failure)) => failure,
            (Ok(Outcome::Refunded), false, None) => GenerationTerminal::Unknown,
            _ => GenerationTerminal::Settlement,
        };
        if let Some(mut observation) = self.observation.take() {
            observation.finish_generation(terminal);
        }
        result
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        if self.armed {
            self.failure.get_or_insert(GenerationTerminal::Unknown);
            if !self.expected_interruption {
                self.primary.get_or_insert(if self.worker_handoff {
                    InterfaceError::GenerationUnknown
                } else {
                    InterfaceError::PreflightUnknown
                });
            }
            let _ = self.finish(None);
        }
    }
}

/// Linear, synchronous access to the original reservation's terminal guard.
/// Dropping without `finish` settles committed usage or refunds if none exists.
/// This owns no resource leases/lifetime markers: those stay with the worker until ALL
/// captured input and rendering state is destroyed, even after settlement.
pub(crate) struct Settlement {
    terminal: Terminal,
    first_output_seen: bool,
    local_failure: Option<InferenceFailure>,
}

impl Settlement {
    pub(crate) fn expected_interruption(&mut self) {
        self.terminal.expected_interruption = true;
    }

    /// The sink owns this failure; its legacy upstream projection must not count
    /// again at settlement. A different upstream failure remains independent.
    pub(crate) fn local_failure(&mut self, failure: InferenceFailure) {
        self.local_failure.get_or_insert(failure);
    }

    pub(crate) fn evidence(&mut self, evidence: crate::inference::forward::Evidence) {
        self.terminal.evidence = Some(evidence);
    }
    pub(crate) fn first_output(&mut self) {
        // Later stream fragments need no global aggregation lock.
        if std::mem::replace(&mut self.first_output_seen, true) {
            return;
        }
        if let Some(o) = &mut self.terminal.observation {
            o.first_output();
        }
    }

    /// Consume verified adapter completion, not an intermediate usage event.
    /// `Ok` is trusted ONLY after validated finish, final usage and actual EOF.
    /// Committed partial evidence may settle money on `Err`, never completion.
    /// This capability cannot establish stream authenticity itself.
    /// Accounting rechecks arithmetic and refunds invalid usage. No error text
    /// is retained or formatted. Even accounting failure is a single attempt.
    pub(crate) fn finish(mut self, result: &Result<StreamUsage, InferenceError>) -> SettledReceipt {
        if let Err(error) = result {
            let failure = error.failure();
            self.terminal.failure.get_or_insert(failure.into());
            let expected_disconnect = self.terminal.expected_interruption
                && failure == InferenceFailure::StreamTransportFailed;
            if !expected_disconnect && self.local_failure != Some(failure) {
                self.terminal.primary.get_or_insert(failure.into());
            }
        }
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
/// SAME guard, resource lease and generation marker belong to the worker. There is no disarm/ack gap in
/// which cancellation can refund an already-running generation.
///
/// The route's first spawn is DETACHED PREFLIGHT,
/// before tokenization, not compose. Its explicit owner envelope drops owned
/// prompt-bearing work BEFORE this guard/leases. Preflight failure/deadline drops
/// this sole guard (no explicit refund plus guard); success moves this same value
/// synchronously into API streaming composition, even if observation was dropped.
/// Observation loss cannot cancel this owner. The shared disconnect signal may
/// close its upstream operation while the same guard still settles once.
///
/// Retire generation tracking after terminal cleanup but before returning memory
/// admission, so a replacement cannot overlap the old generation observation.
pub(crate) struct ReservedGeneration {
    terminal: Terminal,
    _generation: Lease,
    _resources: Arc<crate::telemetry::hooks::Lease>,
    _ticket: Option<crate::lifecycle::Ticket>,
    disconnect: crate::stream_owner::Disconnect,
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
        generation: impl Into<Lease>,
        resources: impl Into<Arc<crate::telemetry::hooks::Lease>>,
    ) -> Self {
        Self {
            terminal: Terminal {
                accounting,
                id,
                armed: true,
                evidence: None,
                observation: None,
                failure: None,
                primary: None,
                expected_interruption: false,
                worker_handoff: false,
                model: None,
                lifecycle: None,
            },
            _resources: resources.into(),
            _generation: generation.into(),
            _ticket: None,
            disconnect: Default::default(),
        }
    }

    pub(crate) fn disconnected(mut self, signal: crate::stream_owner::Disconnect) -> Self {
        self.disconnect = signal;
        self
    }
    pub(crate) fn disconnect(&self) -> crate::stream_owner::Disconnect {
        self.disconnect.clone()
    }

    pub(crate) fn tracked(mut self, ticket: crate::lifecycle::Ticket) -> Self {
        self.terminal.lifecycle = Some(ticket.lifecycle());
        self._ticket = Some(ticket);
        self
    }

    pub(crate) fn observed(
        mut self,
        observation: Option<OwnedObservation>,
        model: QualifiedModel,
    ) -> Self {
        if let Some(observation) = &observation {
            self._resources.observe_generation(observation);
        }
        self.terminal.observation = observation;
        self.terminal.model = Some(model);
        self
    }
    pub(crate) fn failure(&mut self, failure: GenerationTerminal, primary: InterfaceError) {
        self.terminal.failure.get_or_insert(failure);
        self.terminal.primary.get_or_insert(primary);
    }
    pub(crate) fn expected_interruption(&mut self) {
        self.terminal.expected_interruption = true;
    }
    pub(crate) fn metrics(&self) -> Option<Arc<crate::telemetry::AggregateMetrics>> {
        self.terminal
            .observation
            .as_ref()
            .map(OwnedObservation::metrics)
    }
    pub(crate) fn delivery_observation(&self) -> Option<OwnedObservation> {
        Some(
            self.terminal
                .observation
                .as_ref()?
                .delivery(self.terminal.model?),
        )
    }

    /// Synchronous handoff. The work factory is invoked ONLY inside the spawned
    /// worker, after reservation and lease ownership have moved. Capture owned
    /// inference inputs in it, not an already-running task. Work must observe
    /// disconnect without cancelling this settlement owner.
    ///
    /// `Ok` must come only from the verified adapter after validated finish,
    /// final usage and EOF, never from a parser/usage event alone. Missing/invalid
    /// final usage and upstream errors are `Err`; no text-derived counts or replay.
    /// The owner rechecks usage arithmetic through Accounting::finish.
    ///
    /// The returned receiver is observation only: dropping it or the HTTP body
    /// cannot abort the settlement worker or release either lease. Body drop
    /// signals upstream cancellation; cleanup uses committed usage or refunds.
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

    /// Like `spawn`, but lets the owned future settle before terminal delivery.
    /// Call `settlement.finish(&result)` synchronously and return that same receipt,
    /// not a work-supplied outcome. Settled money alone is not successful inference
    /// or tool authorization. Accounting releases its lock before `finish` returns.
    ///
    /// No lease is transferred to the capability or observer. Dropping delivery
    /// or observation cannot abort this owner. Dropping the capability settles
    /// committed evidence or refunds; a terminal attempt is always absorbing.
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
            _ticket,
            disconnect: _,
        } = self;
        let mut terminal = terminal;
        terminal.worker_handoff = true;
        let settlement = Settlement {
            terminal,
            first_output_seen: false,
            local_failure: None,
        };
        tokio::spawn(Worker {
            // The SAME guard moves into Worker before the factory can run.
            work: Box::pin(async move { work(delivery, settlement).await }),
            _resources,
            _generation,
            _ticket,
        })
    }
}

// Explicit field drop order is essential, including before the first poll:
// destroy ALL work/captured input state before returning resource/slot credit.
// Do not rely on the unspecified field order of an async block's captures.
struct Worker<Fut> {
    work: Pin<Box<Fut>>,
    _generation: Lease,
    _resources: Arc<crate::telemetry::hooks::Lease>,
    _ticket: Option<crate::lifecycle::Ticket>,
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
