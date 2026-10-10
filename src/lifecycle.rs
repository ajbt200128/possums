//! Process-local admission and cleanup proof, independent of telemetry/delivery.
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};
use tokio::sync::Notify;

#[derive(Clone, Default)]
pub struct Lifecycle(Arc<Inner>);

#[derive(Default)]
struct Inner {
    state: Mutex<State>,
    changed: Notify,
}

#[derive(Default)]
struct State {
    quiescing: bool,
    roots: usize,
    accounting_failed: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub struct DrainFailed;

/// Descendants share one admitted root, never register new work after the fence.
#[derive(Clone)]
pub struct Ticket(Arc<Root>);
struct Root(Lifecycle);

impl Lifecycle {
    pub fn try_admit(&self) -> Option<Ticket> {
        let mut state = self.0.state.lock().unwrap();
        if state.quiescing {
            return None;
        }
        state.roots += 1;
        Some(Ticket(Arc::new(Root(self.clone()))))
    }

    pub fn is_serving(&self) -> bool {
        !self.0.state.lock().unwrap().quiescing
    }

    pub fn quiesce(&self) {
        self.0.state.lock().unwrap().quiescing = true;
        self.0.changed.notify_waiters();
    }

    pub(crate) fn accounting_failed(&self) {
        self.0.state.lock().unwrap().accounting_failed = true;
    }

    pub async fn wait_drained(&self) -> Result<(), DrainFailed> {
        loop {
            // Enable before the locked predicate: notify_waiters must wake every
            // waiter even when the final root retires between check and await.
            let changed = self.0.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            {
                let state = self.0.state.lock().unwrap();
                if state.quiescing && state.roots == 0 {
                    return if state.accounting_failed {
                        Err(DrainFailed)
                    } else {
                        Ok(())
                    };
                }
            }
            changed.await;
        }
    }
}

impl Ticket {
    pub(crate) fn lifecycle(&self) -> Lifecycle {
        self.0 .0.clone()
    }

    /// Explicit envelope drop order, also on cancellation/unwind/pre-poll drop.
    pub(crate) fn track<F: Future>(self, work: F) -> Tracked<F> {
        Tracked {
            work: Box::pin(work),
            _ticket: self,
        }
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        {
            let mut state = self.0 .0.state.lock().unwrap();
            state.roots -= 1;
        }
        self.0 .0.changed.notify_waiters();
    }
}

pub(crate) struct Tracked<F> {
    work: Pin<Box<F>>,
    _ticket: Ticket,
}
impl<F: Future> Future for Tracked<F> {
    type Output = F::Output;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.get_mut().work.as_mut().poll(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::poll;

    #[tokio::test]
    async fn fence_descendants_and_multiple_waiters() {
        let lifecycle = Lifecycle::default();
        let root = lifecycle.try_admit().unwrap();
        lifecycle.quiesce();
        lifecycle.quiesce();
        assert!(lifecycle.try_admit().is_none());
        let child = root.clone();
        let first = lifecycle.wait_drained();
        let second = lifecycle.wait_drained();
        tokio::pin!(first, second);
        assert!(poll!(&mut first).is_pending());
        assert!(poll!(&mut second).is_pending());
        drop(root);
        assert!(poll!(&mut first).is_pending());
        drop(child);
        assert_eq!(first.await, Ok(()));
        assert_eq!(second.await, Ok(()));
        assert_eq!(lifecycle.wait_drained().await, Ok(()));
    }

    #[tokio::test]
    async fn sticky_failure_waits_for_every_root() {
        let lifecycle = Lifecycle::default();
        let first = lifecycle.try_admit().unwrap();
        let other = lifecycle.try_admit().unwrap();
        lifecycle.quiesce();
        lifecycle.accounting_failed();
        drop(first);
        let drain = lifecycle.wait_drained();
        tokio::pin!(drain);
        assert!(poll!(&mut drain).is_pending());
        drop(other);
        assert_eq!(drain.await, Err(DrainFailed));
        assert_eq!(lifecycle.wait_drained().await, Err(DrainFailed));
    }

    #[tokio::test]
    async fn admission_races_fence_without_uncounted_roots() {
        for _ in 0..128 {
            let lifecycle = Lifecycle::default();
            let barrier = Arc::new(std::sync::Barrier::new(2));
            let other = lifecycle.clone();
            let ready = barrier.clone();
            let thread = std::thread::spawn(move || {
                ready.wait();
                other.try_admit()
            });
            barrier.wait();
            lifecycle.quiesce();
            let ticket = thread.join().unwrap();
            assert!(lifecycle.try_admit().is_none());
            let drain = lifecycle.wait_drained();
            tokio::pin!(drain);
            if ticket.is_some() {
                assert!(poll!(&mut drain).is_pending());
            }
            drop(ticket);
            assert_eq!(drain.await, Ok(()));
        }
    }

    #[tokio::test]
    async fn serving_zero_is_not_drained() {
        let lifecycle = Lifecycle::default();
        let drain = lifecycle.wait_drained();
        tokio::pin!(drain);
        assert!(poll!(&mut drain).is_pending());
        lifecycle.quiesce();
        assert_eq!(drain.await, Ok(()));
    }
}
