//! Test-only ownership boundary. Two existing boxes per family, no backlog.
//! A missing frozen box is in flight (including disposal); its metadata cannot
//! be replaced. Return mail holds only that same box, never another window.
use super::*;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use tokio::sync::watch;

pub(super) enum Table {
    Request(Box<RequestTables>),
    Infrastructure(Box<Infrastructure>),
}

pub(super) struct Exchange {
    pub(super) busy: AtomicBool,
    pub(super) changed: watch::Sender<()>,
    returned: SyncSender<Table>,
    receiver: Mutex<Receiver<Table>>,
}
impl Default for Exchange {
    fn default() -> Self {
        let (returned, receiver) = sync_channel(1);
        Self {
            busy: AtomicBool::new(false),
            changed: watch::channel(()).0,
            returned,
            receiver: Mutex::new(receiver),
        }
    }
}
impl Exchange {
    pub(super) fn reclaim(&self, state: &mut State) {
        // Only the state owner consumes the one return cell. No other caller
        // locks this receiver; never wait behind a sender/serializer here.
        let Ok(receiver) = self.receiver.try_lock() else {
            return;
        };
        if let Ok(table) = receiver.try_recv() {
            match table {
                Table::Request(mut table) => {
                    *table = RequestTables::default();
                    debug_assert!(state.requests.frozen.is_none());
                    state.requests.frozen = Some(table);
                }
                Table::Infrastructure(mut table) => {
                    *table = Infrastructure::default();
                    debug_assert!(state.infrastructure.frozen.is_none());
                    state.infrastructure.frozen = Some(table);
                }
            }
        }
    }
}

/// Non-cloneable sole send authority, never a StateGuard or a borrowed view.
/// Drop discards, returning the same allocation without locking aggregation.
/// Epoch/watermark/pending metadata are deliberately NOT restored on return.
pub(super) struct Permit<'a, C: Clock> {
    pub(super) owner: &'a AggregateMetrics<C>,
    pub(super) table: Option<Table>,
    pub(super) window: Window,
    pub(super) epoch: u64,
}
impl<C: Clock> Permit<'_, C> {
    pub(super) fn valid(&self) -> bool {
        let Some(mut state) = self.owner.lock() else {
            return false;
        };
        let Some((_, mapped)) = self.owner.time(&mut state) else {
            return false;
        };
        if !self.owner.advance(&mut state, mapped) {
            return false;
        }
        let (missing, watermark) = match self.table.as_ref().unwrap() {
            Table::Request(_) => (state.requests.frozen.is_none(), state.requests.attempted),
            Table::Infrastructure(_) => (
                state.infrastructure.frozen.is_none(),
                state.infrastructure.attempted,
            ),
        };
        missing
            && watermark == self.window.end_ns
            && mapped >= self.window.end_ns
            && mapped - self.window.end_ns < 2 * SECOND
            && self.owner.eligible_token(self.epoch)
    }
}
impl<C: Clock> Drop for Permit<'_, C> {
    fn drop(&mut self) {
        // Exactly one permit and one return cell. Handoff drains it before
        // selecting another permit. Failure cannot restore/replay stale data;
        // a lost box leaves that family permanently unavailable, not allocated.
        let _ = self
            .owner
            .handoff
            .returned
            .try_send(self.table.take().unwrap());
        self.owner.handoff.busy.store(false, Ordering::SeqCst);
        self.owner.handoff.changed.send_replace(());
    }
}
fn claim<T: Default>(slots: &mut Slots<T>, expected: u64, mapped: u64) -> Option<(Window, Box<T>)> {
    let (window, epoch) = slots.pending.take()?;
    if epoch != expected
        || slots.attempted != window.end_ns
        || mapped < window.end_ns
        || mapped - window.end_ns >= 2 * SECOND
    {
        if let Some(table) = &mut slots.frozen {
            **table = T::default();
        }
        return None;
    }
    Some((window, slots.frozen.take()?))
}

impl<C: Clock> AggregateMetrics<C> {
    pub(super) fn take_window(&self) -> Option<Permit<'_, C>> {
        // Ordinary observations don't acquire the sender latch.
        let mut state = self.lock()?;
        let (_, mapped) = self.time(&mut state)?;
        if !self.advance(&mut state, mapped) || !self.eligible_token(state.epoch) {
            return None;
        }
        if self.handoff.busy.load(Ordering::SeqCst) {
            return None;
        }
        // Drop may have published AFTER lock()'s first reclaim but before
        // releasing busy. Drain again after observing that release, otherwise
        // selecting the other family could overfill the one return cell.
        self.handoff.reclaim(&mut state);
        let epoch = state.epoch;
        let (window, table) =
            if let Some((window, table)) = claim(&mut state.requests, epoch, mapped) {
                (window, Table::Request(table))
            } else if let Some((window, table)) = claim(&mut state.infrastructure, epoch, mapped) {
                (window, Table::Infrastructure(table))
            } else {
                return None;
            };
        self.handoff.busy.store(true, Ordering::SeqCst);
        let permit = Permit {
            owner: self,
            table: Some(table),
            window,
            epoch,
        };
        // A losing observation can invalidate us even while this guard exists.
        // The bridge must revalidate after unlock and before any attempt.
        Some(permit)
    }

    /// No tasks are spawned here. Acknowledgement follows the bridge's actual
    /// future/data/connection disposal, never just an SDK shutdown call. Failure
    /// at the deadline is NOT acknowledgement; the epoch stays disabled.
    pub(super) async fn stop_export(&self) -> bool {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(1);
        let mut changed = self.handoff.changed.subscribe();
        loop {
            if self.off() {
                return true;
            }
            tokio::select! {
                _ = tokio::time::sleep_until(deadline) => return false,
                _ = changed.changed() => {},
                // A non-exporting state owner need not send a notification.
                _ = tokio::time::sleep(std::time::Duration::from_millis(1)) => {},
            }
            if tokio::time::Instant::now() >= deadline {
                return false;
            }
        }
    }
}
