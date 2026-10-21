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

/// A linear terminal capability. Synthetic cleanup is synchronous; production
/// cleanup sends once to the already-rooted reserve/terminal actor, never I/O in Drop.
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
    remote: Option<oneshot::Sender<TerminalCommand>>,
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
        self.observe(&result, usage, true, true);
        result
    }

    fn observe(
        &mut self,
        result: &Result<Outcome, AccountingError>,
        usage: Option<FinalUsage>,
        matches_requested: bool,
        terminal_attempted: bool,
    ) {
        let accounting_failed = terminal_attempted && (result.is_err() || !matches_requested);
        if accounting_failed {
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
            if accounting_failed {
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
        let terminal = if !terminal_attempted {
            self.failure.unwrap_or(GenerationTerminal::Admission)
        } else {
            match (result, usage.is_some() && matches_requested, self.failure) {
                (Ok(Outcome::Settled { .. }), _, Some(failure)) => failure,
                (Ok(Outcome::Settled { .. }), true, None) => GenerationTerminal::Success,
                (Ok(Outcome::Refunded), false, Some(failure)) => failure,
                (Ok(Outcome::Refunded), false, None) => GenerationTerminal::Unknown,
                _ => GenerationTerminal::Settlement,
            }
        };
        if let Some(mut observation) = self.observation.take() {
            observation.finish_generation(terminal);
        }
    }

    fn usage(&self, fallback: Option<FinalUsage>) -> Option<FinalUsage> {
        self.evidence
            .as_ref()
            .map(|e| FinalUsage {
                input_tokens: e.usage.input_tokens,
                output_tokens: e.usage.output_tokens,
                total_tokens: e.usage.total_tokens,
            })
            .or(fallback)
    }

    fn send_remote(
        &mut self,
        usage: Option<FinalUsage>,
        reply: Option<oneshot::Sender<TerminalResult>>,
    ) {
        self.armed = false;
        let sender = self.remote.take().expect("remote terminal capability");
        // Only the numeric evidence and closed observations cross to the actor.
        let evidence = self.evidence.as_ref().map(|e| {
            let mut numeric = crate::inference::forward::Evidence::sse(e.usage);
            numeric.cached_tokens = e.cached_tokens;
            numeric.cache_usage_available = e.cache_usage_available;
            numeric
        });
        let report = Terminal {
            accounting: self.accounting.clone(),
            id: self.id,
            armed: false,
            evidence,
            observation: self.observation.take(),
            failure: self.failure,
            primary: self.primary,
            expected_interruption: self.expected_interruption,
            worker_handoff: self.worker_handoff,
            model: self.model,
            lifecycle: self.lifecycle.clone(),
            remote: None,
        };
        if let Err(mut command) = sender.send(TerminalCommand {
            report,
            usage,
            reply,
        }) {
            // Actor loss is not a receipt. Preserve usage and classify locally.
            command
                .report
                .observe(&Err(AccountingError::StateUnavailable), usage, false, true);
        }
    }

    async fn finish_owned(&mut self, usage: Option<FinalUsage>) -> TerminalResult {
        if self.remote.is_none() {
            return TerminalResult {
                outcome: self.finish(usage),
                matches_requested: true,
                diagnostic: None,
            };
        }
        let usage = self.usage(usage);
        let (reply, receipt) = oneshot::channel();
        self.send_remote(usage, Some(reply));
        receipt.await.unwrap_or(TerminalResult {
            outcome: Err(AccountingError::StateUnavailable),
            matches_requested: false,
            diagnostic: None,
        })
    }
}

struct TerminalCommand {
    report: Terminal,
    usage: Option<FinalUsage>,
    reply: Option<oneshot::Sender<TerminalResult>>,
}
struct TerminalResult {
    outcome: Result<Outcome, AccountingError>,
    matches_requested: bool,
    diagnostic: Option<crate::intake::diagnostics::Diagnostic>,
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
            if self.remote.is_some() {
                self.send_remote(self.usage(None), None);
            } else {
                let _ = self.finish(None);
            }
        }
    }
}

/// Linear access to the original reservation's terminal guard.
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
    pub(crate) async fn finish(
        mut self,
        result: &Result<StreamUsage, InferenceError>,
    ) -> SettledReceipt {
        if let Err(error) = result {
            let failure = error.failure();
            self.terminal.failure.get_or_insert(failure.into());
            let expected_disconnect = self.terminal.expected_interruption
                && failure == InferenceFailure::StreamTransportFailed;
            if !expected_disconnect && self.local_failure != Some(failure) {
                self.terminal.primary.get_or_insert(failure.into());
            }
        }
        let terminal = self
            .terminal
            .finish_owned(result.as_ref().ok().map(|usage| FinalUsage {
                input_tokens: usage.input_tokens,
                output_tokens: usage.output_tokens,
                total_tokens: usage.total_tokens,
            }))
            .await;
        SettledReceipt {
            completion: terminal.outcome.map_err(OwnerError::Accounting),
            matches_requested: terminal.matches_requested,
            diagnostic: terminal.diagnostic,
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
    matches_requested: bool,
    diagnostic: Option<crate::intake::diagnostics::Diagnostic>,
}

impl SettledReceipt {
    pub(crate) fn diagnostic(&self) -> Option<crate::intake::diagnostics::Diagnostic> {
        self.diagnostic
    }
    pub(crate) fn matches_requested(&self) -> bool {
        self.matches_requested
    }
    pub(crate) fn outcome(&self) -> &Completion {
        &self.completion
    }
}

/// Construct immediately after a new local claim, never for duplicates. For
/// intake, call `start_remote` after tracking/observation and before any await:
/// it roots the sole reserve/terminal actor, not a second monetary reservation.
/// Preflight only observes its readiness and transfers this capability to work.
/// Cancellation drops prompt work but cannot discard a late hold or terminal RPC.
/// Shared leases keep generation/memory ownership until BOTH work and actor end;
/// generation tracking retires before the final memory lease.
pub(crate) struct ReservedGeneration {
    terminal: Terminal,
    _generation: Arc<Lease>,
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
                remote: None,
            },
            _resources: resources.into(),
            _generation: Arc::new(generation.into()),
            _ticket: None,
            disconnect: Default::default(),
        }
    }

    /// Root the one reserve attempt AND its terminal continuation before dispatch.
    /// Readiness carries the confirmed expiry for the post-await admission check,
    /// not an accepted-work cutoff. Dropping it cannot cancel either RPC.
    pub(crate) fn start_remote(
        &mut self,
    ) -> Option<oneshot::Receiver<Result<std::time::SystemTime, AccountingError>>> {
        if !self.terminal.accounting.is_remote() {
            return None;
        }
        let (sender, command) = oneshot::channel();
        let (ready, receiver) = oneshot::channel();
        let attempt = self.terminal.accounting.take_attempt(self.terminal.id);
        let root = ActorRoot {
            complete: false,
            _generation: self._generation.clone(),
            _resources: self._resources.clone(),
            ticket: self
                ._ticket
                .as_ref()
                .expect("root before remote dispatch")
                .clone(),
        };
        self.terminal.remote = Some(sender);
        let accounting = self.terminal.accounting.clone();
        let id = self.terminal.id;
        tokio::spawn(async move {
            remote_terminal(root, accounting, id, attempt, ready, command).await
        });
        Some(receiver)
    }

    /// Await cleanup on ordinary preflight rejection. Panic/cancellation instead
    /// uses the same pre-existing actor through Terminal::drop.
    pub(crate) async fn cancel(mut self) {
        let _ = self.terminal.finish_owned(None).await;
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
    /// Await `settlement.finish(&result)` and return that same receipt,
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
            settlement.finish(&result).await
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
    _generation: Arc<Lease>,
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

// Kept outside work's future: actor loss cannot silently prove clean drain.
struct ActorRoot {
    complete: bool,
    _generation: Arc<Lease>,
    _resources: Arc<Lease>,
    ticket: crate::lifecycle::Ticket,
}
impl Drop for ActorRoot {
    fn drop(&mut self) {
        if !self.complete {
            self.ticket.lifecycle().accounting_failed();
        }
    }
}

fn usable_hold(reservation: &crate::intake::Reservation) -> bool {
    // Admission needs confirmed, currently active credit, not a minimum work
    // budget. Accepted work may continue beyond the last confirmed lease expiry.
    reservation.is_active() && reservation.expires_at() > std::time::SystemTime::now()
}

async fn remote_terminal(
    mut root: ActorRoot,
    accounting: Arc<Accounting>,
    id: [u8; 32],
    attempt: Result<crate::accounting::Attempt, AccountingError>,
    ready: oneshot::Sender<Result<std::time::SystemTime, AccountingError>>,
    command: oneshot::Receiver<TerminalCommand>,
) {
    // This single bounded attempt is deliberately not selected against caller
    // cancellation. A late confirmed active hold still belongs to this actor.
    let dispatched = tokio::time::Instant::now();
    let mut reserved = match &attempt {
        Ok(attempt) => attempt.reserve().await,
        Err(error) => Err(*error),
    };
    let admission = match &reserved {
        Ok(reservation) if usable_hold(reservation) => Ok(reservation.expires_at()),
        Ok(_) => Err(AccountingError::HoldUnavailable),
        Err(error) => Err(*error),
    };
    if admission.is_err_and(AccountingError::reserve_uncertain) {
        root.ticket.lifecycle().accounting_failed();
    }
    let _ = ready.send(admission);
    // The capability sends once on explicit finish OR drop, including pre-poll
    // work loss. Only numeric evidence/closed observations enter this channel.
    let (mut command, closed, renewal_failure) = match (&attempt, &mut reserved) {
        (Ok(attempt), Ok(reservation)) if admission.is_ok() => {
            await_remote_terminal(attempt, reservation, dispatched, command).await
        }
        _ => (command.await.ok(), None, None),
    };
    let usage = command.as_ref().and_then(|c| c.usage);
    let cached = command
        .as_ref()
        .and_then(|c| c.report.evidence.as_ref())
        .map_or(0, |e| e.cached_tokens);
    let mut terminal_attempted = false;
    let mut result = match (&attempt, reserved) {
        (Ok(attempt), Ok(reservation)) if reservation.is_active() => {
            terminal_attempted = true;
            // Ineligible admissions never produce billable prompt work.
            let charge = if admission.is_ok() {
                attempt.charge(usage, cached)
            } else {
                None
            };
            // A renewal may already have returned an authoritative absorbing
            // terminal row. Compare it only at normal end; never bill early or
            // issue another mutation against a known terminal reservation.
            let receipt = match closed {
                Some(reply) => Ok(reply
                    .terminal_receipt(charge)
                    .expect("validated closed row")),
                None => attempt.finish(&reservation, charge).await,
            };
            match receipt {
                Ok(receipt) => TerminalResult {
                    outcome: Ok(receipt.outcome.into()),
                    matches_requested: receipt.matches_requested,
                    diagnostic: receipt.diagnostic,
                },
                Err(error) => TerminalResult {
                    outcome: Err(error),
                    matches_requested: false,
                    diagnostic: None,
                },
            }
        }
        (_, Ok(reservation)) => TerminalResult {
            outcome: reservation
                .terminal_outcome()
                .map(Outcome::from)
                .ok_or(AccountingError::HoldUnavailable),
            matches_requested: false,
            diagnostic: None,
        },
        (_, Err(error)) => TerminalResult {
            outcome: Err(error),
            matches_requested: false,
            diagnostic: None,
        },
    };
    // Preserve renewal classification transiently, but terminal evidence/failure
    // has precedence. A recovered final settlement remains successful. This adds
    // no telemetry or wire fields; only the existing opt-in diagnostic can use it.
    result.diagnostic = result.diagnostic.or_else(|| match result.outcome {
        Err(AccountingError::Intake(error)) => Some(error.diagnostic()),
        _ => renewal_failure,
    });
    if terminal_attempted && (result.outcome.is_err() || !result.matches_requested) {
        root.ticket.lifecycle().accounting_failed();
    }
    // A bookkeeping failure must not erase an already authenticated receipt.
    if accounting
        .record_remote(
            id,
            result.outcome.as_ref().cloned().unwrap_or(Outcome::Unknown),
        )
        .is_err()
    {
        root.ticket.lifecycle().accounting_failed();
    }
    if let Some(command) = &mut command {
        if let Err(error) = admission {
            command.report.primary = Some(error.interface_error());
            command.report.failure = Some(GenerationTerminal::Admission);
        }
        command.report.observe(
            &result.outcome,
            usage,
            result.matches_requested,
            terminal_attempted,
        );
    }
    root.complete = true;
    // Leases and lifecycle roots retire only after the RPC, observations and
    // tombstone commit; work/delivery retain their independent root until done.
    drop(root);
    if let Some(TerminalCommand {
        reply: Some(reply), ..
    }) = command
    {
        let _ = reply.send(result);
    }
}

/// Renew within the already-rooted monetary owner, never in prompt work or
/// delivery. Terminal commands win a scheduling tie. An in-flight bounded RPC
/// is not cancelled by completion: its committed evidence still belongs here.
async fn await_remote_terminal(
    attempt: &crate::accounting::Attempt,
    reservation: &mut crate::intake::Reservation,
    dispatched: tokio::time::Instant,
    mut command: oneshot::Receiver<TerminalCommand>,
) -> (
    Option<TerminalCommand>,
    Option<crate::intake::RenewalReply>,
    Option<crate::intake::diagnostics::Diagnostic>,
) {
    use std::time::{Duration, SystemTime};
    use tokio::time::{sleep_until, Instant};
    const CADENCE: Duration = Duration::from_secs(10 * 60);
    let remaining = reservation
        .expires_at()
        .duration_since(SystemTime::now())
        .unwrap_or_default();
    // Receipt/delivery latency must never extend the local confirmation horizon.
    let mut confirmed_until =
        dispatched + remaining.min(Duration::from_secs(u64::from(reservation.timeout_seconds)));
    let mut next = dispatched + CADENCE;
    loop {
        tokio::select! {
            biased;
            terminal = &mut command => return (terminal.ok(), None, None),
            _ = sleep_until(next) => {}
        }
        if Instant::now() >= confirmed_until {
            // A delayed poll cannot resurrect an expired confirmation.
            return (command.await.ok(), None, None);
        }
        let dispatched = Instant::now();
        let reply = match attempt.renew(reservation).await {
            Ok(reply) => reply,
            // One unconfirmed attempt permanently ends renewal, not work.
            // No retry, lookup, replacement hold or inference replay.
            Err(error) => {
                let diagnostic = match error {
                    AccountingError::Intake(error) => error.diagnostic(),
                    _ => crate::intake::diagnostics::Diagnostic::renewal_unknown(),
                };
                return (command.await.ok(), None, Some(diagnostic));
            }
        };
        if !reply.reservation.is_active() {
            return (command.await.ok(), Some(reply), None);
        }
        let horizon = reply
            .reservation
            .expires_at()
            .duration_since(reply.observed_at)
            .unwrap_or_default();
        let confirmed = dispatched + horizon;
        let failure = reply.refusal.map(|error| error.diagnostic());
        let stop = failure.is_some() || confirmed <= Instant::now();
        *reservation = reply.reservation;
        if stop {
            return (command.await.ok(), None, failure);
        }
        confirmed_until = confirmed;
        next = dispatched + CADENCE;
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
#[cfg(test)]
pub(crate) use tests::completed_cached_usage_fixture;
