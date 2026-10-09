use super::*;
use crate::{
    accounting::ReserveResult,
    catalog::{Model, Quote},
    stream_owner::{delivery, DeliveryBody, DeliveryError, Limits},
};
use http_body_util::BodyExt;
use std::{
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;

const ACCOUNT: &str = "a";
const BINDING: [u8; 32] = [99; 32];

fn quote() -> Quote {
    Quote {
        model: Model {
            id: "m".into(),
            context_tokens: 100,
            max_output_tokens: 10,
            input_microunits_per_million_tokens: 2_000_000,
            output_microunits_per_million_tokens: 3_000_000,
        },
        input_tokens: 5,
        reserved_microunits: 52,
    }
}

fn usage() -> StreamUsage {
    StreamUsage {
        input_tokens: 5,
        output_tokens: 4,
        total_tokens: 9,
    }
}

fn final_usage() -> FinalUsage {
    FinalUsage {
        input_tokens: 5,
        output_tokens: 4,
        total_tokens: 9,
    }
}

struct Fixture {
    ledger: Arc<Accounting>,
    generations: Arc<Semaphore>,
    resources: Arc<Semaphore>,
    lanes: Arc<Semaphore>,
}

impl Fixture {
    fn new() -> Self {
        Self {
            ledger: Arc::new(Accounting::new([("a".into(), 1000), ("b".into(), 1000)])),
            generations: Arc::new(Semaphore::new(4)),
            resources: Arc::new(Semaphore::new(4)),
            lanes: Arc::new(Semaphore::new(4)),
        }
    }

    fn reserve(&self, account: &str, id: u8) -> Result<ReserveResult, AccountingError> {
        self.ledger.reserve(
            account,
            [id; 32],
            BINDING,
            quote(),
            Instant::now() + Duration::from_secs(60),
        )
    }

    fn pending_for(&self, account: &str, id: u8) -> ReservedGeneration {
        let generation = self.generations.clone().try_acquire_owned().unwrap();
        let resources = self.resources.clone().try_acquire_owned().unwrap();
        assert_eq!(self.reserve(account, id).unwrap(), ReserveResult::Reserved);
        ReservedGeneration::new(
            self.ledger.clone(),
            [id; 32],
            generation,
            Lease::from(resources),
        )
    }

    fn pending(&self, id: u8) -> ReservedGeneration {
        self.pending_for(ACCOUNT, id)
    }

    fn delivery(&self) -> (DeliveryTx, DeliveryBody) {
        let (startup, body) = delivery(
            Lease::from(self.lanes.clone().try_acquire_owned().unwrap()),
            Limits {
                frames: 1,
                payload_bytes: 8,
                chunk_bytes: 8,
            },
            Duration::from_secs(1),
        );
        (startup.into_streaming(), body)
    }

    fn outcome(&self, id: u8, outcome: Outcome, balance: u64) {
        assert_eq!(
            self.reserve(ACCOUNT, id).unwrap(),
            ReserveResult::Duplicate(outcome)
        );
        assert_eq!(self.ledger.available(ACCOUNT), Some(balance));
    }

    fn leases_returned(&self) {
        assert_eq!(self.generations.available_permits(), 4);
        assert_eq!(self.resources.available_permits(), 4);
    }

    // Credit-backed reservations remain usable after cleanup without an account
    // request-count quota. Refunding the probes restores the original balance.
    fn account_slots_returned(&self, balance: u64) {
        for id in 240..244 {
            assert_eq!(self.reserve(ACCOUNT, id).unwrap(), ReserveResult::Reserved);
        }
        assert_eq!(self.ledger.available(ACCOUNT), Some(balance - 4 * 52));
        for id in 240..244 {
            assert_eq!(
                self.ledger.finish([id; 32], None).unwrap(),
                Outcome::Refunded
            );
        }
        assert_eq!(self.ledger.available(ACCOUNT), Some(balance));
    }
}

async fn bounded<T>(future: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(5), future)
        .await
        .unwrap()
}

#[test]
fn generation_tracking_retires_before_memory_handoff_in_both_owner_envelopes() {
    struct Handoff {
        activity: Arc<AtomicUsize>,
        observed: AtomicUsize,
    }
    impl std::task::Wake for Handoff {
        fn wake(self: Arc<Self>) {
            self.observed
                .store(self.activity.load(Ordering::SeqCst), Ordering::SeqCst);
        }
    }

    for worker_envelope in [false, true] {
        let f = Fixture::new();
        let resources = Arc::new(Semaphore::new(1));
        let activity = Arc::new(AtomicUsize::new(0));
        assert_eq!(f.reserve(ACCOUNT, 1).unwrap(), ReserveResult::Reserved);
        let owner = ReservedGeneration::new(
            f.ledger.clone(),
            [1; 32],
            Lease::tracking(activity.clone(), None, crate::telemetry::Lane::Generation),
            Lease::from(resources.clone().try_acquire_owned().unwrap()),
        );
        let handoff = Arc::new(Handoff {
            activity: activity.clone(),
            observed: AtomicUsize::new(usize::MAX),
        });
        let waker = std::task::Waker::from(handoff.clone());
        let mut cx = Context::from_waker(&waker);
        let mut next = Box::pin(resources.clone().acquire_owned());
        assert!(next.as_mut().poll(&mut cx).is_pending());
        if worker_envelope {
            let ReservedGeneration {
                terminal,
                _generation,
                _resources,
            } = owner;
            let settlement = Settlement {
                terminal,
                first_output_seen: false,
            };
            drop(Worker {
                work: Box::pin(async move {
                    let _settlement = settlement;
                    std::future::pending::<SettledReceipt>().await
                }),
                _generation,
                _resources,
            });
        } else {
            drop(owner);
        }
        // Semaphore release wakes the replacement synchronously, exposing the
        // precise retirement order rather than depending on scheduler timing.
        assert_eq!(handoff.observed.load(Ordering::SeqCst), 0);
        assert_eq!(activity.load(Ordering::SeqCst), 0);
        assert!(next.as_mut().poll(&mut cx).is_ready());
        f.outcome(1, Outcome::Refunded, 1000);
    }
}

#[tokio::test]
async fn settling_handoff_accepts_owned_startup_before_any_blocking_write() {
    let f = Fixture::new();
    let pending = f.pending(1);
    let (startup, mut body) = delivery(
        Lease::from(f.lanes.clone().try_acquire_owned().unwrap()),
        Limits {
            frames: 1,
            payload_bytes: 8,
            chunk_bytes: 8,
        },
        Duration::from_secs(1),
    );
    let (finish_tx, finish) = oneshot::channel();
    let completion = pending.spawn_settling(startup, move |mut startup, settlement| async move {
        let _delivery = tokio::task::spawn_blocking(move || {
            startup.send_blocking(b"startup").unwrap();
            startup.into_streaming()
        })
        .await
        .unwrap();
        finish.await.unwrap();
        settlement.finish(&Ok(usage()))
    });
    let frame = bounded(body.frame()).await.unwrap().unwrap();
    assert_eq!(frame.into_data().unwrap(), "startup");
    f.outcome(1, Outcome::InFlight, 948);
    finish_tx.send(()).unwrap();
    assert_eq!(
        bounded(completion).await.unwrap(),
        Ok(Outcome::Settled { charged: 29 })
    );
    f.leases_returned();
}

// Staged composition only: the buffered /chat handler does not spawn this owner.
#[tokio::test]
async fn shared_heavy_admission_returns_only_after_worker_body_and_slices_release() {
    for last_owner in ["worker", "body", "slice"] {
        let f = Fixture::new();
        // Leave exactly one heavy slot: constructing BOTH owners must not need
        // a second permit. The independent four-slot work lease is unchanged.
        let other_heavy = f.resources.clone().try_acquire_many_owned(3).unwrap();
        let heavy: Arc<crate::telemetry::hooks::Lease> =
            Arc::new(f.resources.clone().try_acquire_owned().unwrap().into());
        let generation = f.generations.clone().try_acquire_owned().unwrap();
        assert_eq!(f.reserve(ACCOUNT, 1).unwrap(), ReserveResult::Reserved);
        let pending = ReservedGeneration::new(f.ledger.clone(), [1; 32], generation, heavy.clone());
        let (startup, mut body) = delivery(
            heavy,
            Limits {
                frames: 1,
                payload_bytes: 8,
                chunk_bytes: 8,
            },
            Duration::from_secs(1),
        );
        let (started_tx, started) = oneshot::channel();
        let (finish_tx, finish) = oneshot::channel();
        let completion = pending.spawn(startup.into_streaming(), move |mut tx| async move {
            tx.try_send(b"partial").unwrap();
            // Detach sender ownership, so only the worker/body/frame can pin it.
            assert_eq!(tx.try_send(b"tail"), Err(DeliveryError::Full));
            started_tx.send(()).unwrap();
            finish.await.unwrap();
            Ok(usage())
        });
        bounded(started).await.unwrap();
        let frame = bounded(body.frame())
            .await
            .unwrap()
            .unwrap()
            .into_data()
            .unwrap();
        let clone = frame.clone();
        let slice = clone.slice(1..2);
        drop(frame);
        drop(clone);
        assert!(bounded(body.frame()).await.is_none());
        assert_eq!(f.resources.available_permits(), 0);
        assert_eq!(f.generations.available_permits(), 3);
        let mut body = Some(body);
        let mut slice = Some(slice);
        if last_owner == "worker" {
            drop(body.take());
            drop(slice.take());
            assert_eq!(f.resources.available_permits(), 0);
            f.outcome(1, Outcome::InFlight, 948);
        }
        finish_tx.send(()).unwrap();
        assert_eq!(
            bounded(completion).await.unwrap(),
            Ok(Outcome::Settled { charged: 29 })
        );
        assert_eq!(f.generations.available_permits(), 4);
        if last_owner == "body" {
            drop(slice.take());
            assert_eq!(f.resources.available_permits(), 0);
            drop(body.take());
        } else if last_owner == "slice" {
            drop(body.take());
            assert_eq!(f.resources.available_permits(), 0);
            drop(slice.take());
        }
        assert_eq!(f.resources.available_permits(), 1);
        assert_eq!(f.lanes.available_permits(), 4); // No second delivery lane.
        drop(other_heavy);
        f.leases_returned();
        f.account_slots_returned(971);
    }
}

#[tokio::test]
async fn pre_handoff_request_cancellation_refunds_and_never_starts_work() {
    let f = Fixture::new();
    let pending = f.pending(1);
    let (held, holding) = oneshot::channel();
    let caller = tokio::spawn(async move {
        let _pending = pending;
        held.send(()).unwrap();
        std::future::pending::<()>().await;
    });
    bounded(holding).await.unwrap();
    f.outcome(1, Outcome::InFlight, 948);
    assert_eq!(f.generations.available_permits(), 3);
    assert_eq!(f.resources.available_permits(), 3);
    caller.abort();
    assert!(bounded(caller).await.unwrap_err().is_cancelled());
    f.outcome(1, Outcome::Refunded, 1000);
    f.leases_returned();
    f.account_slots_returned(1000);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn caller_cancel_body_and_observer_drop_cannot_cancel_handed_off_generation() {
    let f = Fixture::new();
    let pending = f.pending(1);
    let (tx, body) = f.delivery();
    let (started_tx, started) = oneshot::channel();
    let (finish_tx, finish) = oneshot::channel();
    let (handoff_tx, handoff) = oneshot::channel();
    let caller = tokio::spawn(async move {
        let completion = pending.spawn(tx, move |mut tx| async move {
            tx.try_send(b"partial").unwrap();
            started_tx.send(()).unwrap();
            finish.await.unwrap();
            assert_eq!(tx.try_send(b"tail"), Err(DeliveryError::Closed));
            Ok(usage())
        });
        handoff_tx.send(completion).unwrap();
        std::future::pending::<()>().await;
    });
    let observer = bounded(handoff).await.unwrap();
    bounded(started).await.unwrap();
    caller.abort();
    assert!(bounded(caller).await.unwrap_err().is_cancelled());
    drop(observer);
    drop(body);
    f.outcome(1, Outcome::InFlight, 948);
    assert_eq!(f.generations.available_permits(), 3);
    assert_eq!(f.resources.available_permits(), 3);
    finish_tx.send(()).unwrap();
    // Slot return is a deterministic task-destruction barrier, even without an
    // observer/receipt. No sleep or client-lifetime cancellation token is used.
    let all = bounded(f.generations.clone().acquire_many_owned(4))
        .await
        .unwrap();
    f.outcome(1, Outcome::Settled { charged: 29 }, 971);
    drop(all);
    f.leases_returned();
    assert_eq!(f.lanes.available_permits(), 4);
    f.account_slots_returned(971);
}

#[tokio::test]
async fn slow_sink_detaches_without_blocking_terminal_and_keeps_delivery_credit_separate() {
    let f = Fixture::new();
    let pending = f.pending(1);
    let (tx, mut body) = f.delivery();
    let (detached_tx, detached) = oneshot::channel();
    let (finish_tx, finish) = oneshot::channel();
    let completion = pending.spawn(tx, move |mut tx| async move {
        tx.try_send(b"first").unwrap();
        assert_eq!(tx.try_send(b"second"), Err(DeliveryError::Full));
        detached_tx.send(tx.usage()).unwrap();
        finish.await.unwrap();
        // Delivery stays detached even if capacity could later become free.
        assert_eq!(tx.try_send(b"third"), Err(DeliveryError::Full));
        Ok(usage())
    });
    let high = bounded(detached).await.unwrap();
    assert_eq!((high.high_frames, high.high_bytes), (1, 5));
    f.outcome(1, Outcome::InFlight, 948);
    assert_eq!(f.generations.available_permits(), 3);
    finish_tx.send(()).unwrap();
    assert_eq!(
        bounded(completion).await.unwrap(),
        Ok(Outcome::Settled { charged: 29 })
    );
    f.outcome(1, Outcome::Settled { charged: 29 }, 971);
    f.leases_returned();
    assert_eq!(f.lanes.available_permits(), 3);
    let frame = bounded(body.frame())
        .await
        .unwrap()
        .unwrap()
        .into_data()
        .unwrap();
    assert_eq!(frame.len(), 5);
    assert!(bounded(body.frame()).await.is_none());
    drop(body);
    assert_eq!(f.lanes.available_permits(), 3); // Retained frame owns only delivery.
    drop(frame);
    assert_eq!(f.lanes.available_permits(), 4);
    f.account_slots_returned(971);
}

#[tokio::test]
async fn error_missing_or_invalid_usage_after_partial_output_refunds_without_replay() {
    for terminal in [
        Err(InferenceError::Unavailable),
        Err(InferenceError::InvalidResponse), // Missing finish/DONE/EOF/usage.
        Ok(StreamUsage {
            total_tokens: 8,
            ..usage()
        }),
        Ok(StreamUsage {
            input_tokens: u64::MAX,
            output_tokens: 1,
            total_tokens: 0,
        }),
    ] {
        let f = Fixture::new();
        let pending = f.pending(1);
        let (tx, mut body) = f.delivery();
        let calls = Arc::new(AtomicUsize::new(0));
        let called = calls.clone();
        let (finish_tx, finish) = oneshot::channel();
        let completion = pending.spawn(tx, move |mut tx| async move {
            called.fetch_add(1, Ordering::SeqCst);
            tx.try_send(b"partial").unwrap();
            finish.await.unwrap();
            terminal
        });
        let partial = bounded(body.frame()).await.unwrap().unwrap();
        f.outcome(1, Outcome::InFlight, 948);
        finish_tx.send(()).unwrap();
        assert_eq!(bounded(completion).await.unwrap(), Ok(Outcome::Refunded));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        f.outcome(1, Outcome::Refunded, 1000);
        f.leases_returned();
        drop((partial, body));
        assert_eq!(f.lanes.available_permits(), 4);
        f.account_slots_returned(1000);
    }
}

#[tokio::test]
async fn accepted_usage_over_quote_caps_charge_at_original_reservation() {
    let f = Fixture::new();
    let pending = f.pending(1);
    let (tx, body) = f.delivery();
    drop(body);
    let completion = pending.spawn(tx, |_| async {
        Ok(StreamUsage {
            input_tokens: 100,
            output_tokens: 100,
            total_tokens: 200,
        })
    });
    assert_eq!(
        bounded(completion).await.unwrap(),
        Ok(Outcome::Settled { charged: 52 })
    );
    f.outcome(1, Outcome::Settled { charged: 52 }, 948);
    f.leases_returned();
    f.account_slots_returned(948);
}

// Checks destruction order without panicking from Drop (which could mask the
// original panic). Captured resource data must die before either lease returns.
struct ResourceProbe {
    generations: Arc<Semaphore>,
    resources: Arc<Semaphore>,
    held_on_drop: Arc<AtomicBool>,
    drops: Arc<AtomicUsize>,
}

impl Drop for ResourceProbe {
    fn drop(&mut self) {
        self.held_on_drop.store(
            self.generations.available_permits() == 3 && self.resources.available_permits() == 3,
            Ordering::SeqCst,
        );
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}

fn probe(f: &Fixture) -> (ResourceProbe, Arc<AtomicBool>, Arc<AtomicUsize>) {
    let held = Arc::new(AtomicBool::new(false));
    let drops = Arc::new(AtomicUsize::new(0));
    (
        ResourceProbe {
            generations: f.generations.clone(),
            resources: f.resources.clone(),
            held_on_drop: held.clone(),
            drops: drops.clone(),
        },
        held,
        drops,
    )
}

#[test]
fn failed_spawn_outside_runtime_refunds_without_invoking_work() {
    let f = Fixture::new();
    let pending = f.pending(1);
    let (tx, body) = f.delivery();
    let (probe, held, drops) = probe(&f);
    let calls = Arc::new(AtomicUsize::new(0));
    let called = calls.clone();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pending.spawn(tx, move |_| {
            called.fetch_add(1, Ordering::SeqCst);
            async move {
                let _resource = probe;
                Ok(usage())
            }
        })
    }));
    assert!(result.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert!(held.load(Ordering::SeqCst));
    f.outcome(1, Outcome::Refunded, 1000);
    f.leases_returned();
    drop(body);
    f.account_slots_returned(1000);
}

#[tokio::test]
async fn worker_cancel_before_first_poll_or_while_held_refunds_and_frees_inputs_before_leases() {
    for poll_first in [false, true] {
        let f = Fixture::new();
        let pending = f.pending(1);
        let (tx, body) = f.delivery();
        let (probe, held, drops) = probe(&f);
        let (started_tx, started) = oneshot::channel();
        let calls = Arc::new(AtomicUsize::new(0));
        let called = calls.clone();
        let worker = pending.spawn_worker(tx, move |_| {
            called.fetch_add(1, Ordering::SeqCst);
            async move {
                let _resource = probe;
                started_tx.send(()).unwrap();
                std::future::pending::<Result<StreamUsage, InferenceError>>().await
            }
        });
        // current_thread runtime: no poll can occur before the first await.
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        if poll_first {
            bounded(started).await.unwrap();
        }
        let abort = worker.abort_handle();
        let completion = supervise(worker);
        abort.abort();
        assert_eq!(
            bounded(completion).await.unwrap(),
            Err(OwnerError::Cancelled)
        );
        assert_eq!(calls.load(Ordering::SeqCst), usize::from(poll_first));
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert!(held.load(Ordering::SeqCst));
        f.outcome(1, Outcome::Refunded, 1000);
        f.leases_returned();
        drop(body);
        assert_eq!(f.lanes.available_permits(), 4);
        f.account_slots_returned(1000);
    }
}

#[tokio::test]
async fn factory_or_polled_future_panic_is_supervised_and_refunds() {
    for factory_panic in [true, false] {
        let f = Fixture::new();
        let pending = f.pending(1);
        let (tx, body) = f.delivery();
        let (probe, held, drops) = probe(&f);
        let completion = pending.spawn(tx, move |mut tx| {
            assert!(!factory_panic, "synthetic owner factory failure");
            async move {
                let _resource = probe;
                tx.try_send(b"partial").unwrap();
                panic!("synthetic owner poll failure");
            }
        });
        assert_eq!(
            bounded(completion).await.unwrap(),
            Err(OwnerError::Panicked)
        );
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert!(held.load(Ordering::SeqCst));
        f.outcome(1, Outcome::Refunded, 1000);
        f.leases_returned();
        drop(body);
        f.account_slots_returned(1000);
    }
}

#[test]
fn runtime_shutdown_drops_worker_and_refunds_without_supervisor_progress() {
    let f = Fixture::new();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let pending = f.pending(1);
    let (tx, body) = f.delivery();
    let (probe, held, drops) = probe(&f);
    let (started_tx, started) = oneshot::channel();
    let completion = {
        let _entered = runtime.enter();
        pending.spawn(tx, move |_| async move {
            let _resource = probe;
            started_tx.send(()).unwrap();
            std::future::pending::<Result<StreamUsage, InferenceError>>().await
        })
    };
    runtime.block_on(bounded(started)).unwrap();
    f.outcome(1, Outcome::InFlight, 948);
    drop(runtime);
    drop(completion); // The refund does not require a live supervisor or observer.
    assert!(held.load(Ordering::SeqCst));
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    f.outcome(1, Outcome::Refunded, 1000);
    f.leases_returned();
    drop(body);
    f.account_slots_returned(1000);
}

#[tokio::test]
async fn one_account_uses_all_supplied_resource_leases_after_disconnect() {
    let f = Fixture::new();
    let mut finishers = Vec::new();
    let mut completions = Vec::new();
    for (account, id) in [("a", 1), ("a", 2), ("a", 3), ("a", 4)] {
        let pending = f.pending_for(account, id);
        let (tx, body) = f.delivery();
        drop(body);
        let (started_tx, started) = oneshot::channel();
        let (finish_tx, finish) = oneshot::channel();
        completions.push(pending.spawn(tx, move |_| async move {
            started_tx.send(()).unwrap();
            finish.await.unwrap();
            Ok(usage())
        }));
        finishers.push(finish_tx);
        bounded(started).await.unwrap();
    }
    assert_eq!(f.generations.available_permits(), 0);
    assert_eq!(f.resources.available_permits(), 0);
    assert!(f.generations.clone().try_acquire_owned().is_err());
    assert_eq!(f.reserve("a", 5), Ok(ReserveResult::Reserved));
    assert_eq!(f.ledger.available("a"), Some(740));
    assert_eq!(f.ledger.finish([5; 32], None), Ok(Outcome::Refunded));
    assert_eq!(f.ledger.available("a"), Some(792));
    assert_eq!(f.ledger.available("b"), Some(1000));
    for finish in finishers {
        finish.send(()).unwrap();
    }
    for completion in completions {
        assert_eq!(
            bounded(completion).await.unwrap(),
            Ok(Outcome::Settled { charged: 29 })
        );
    }
    assert_eq!(f.ledger.available("a"), Some(884));
    assert_eq!(f.ledger.available("b"), Some(1000));
    f.leases_returned();
    assert_eq!(f.lanes.available_permits(), 4);
    f.account_slots_returned(884);
}

#[tokio::test]
async fn conflicting_terminal_orders_are_absorbing_but_do_not_release_active_worker_slots() {
    for first in [None, Some(final_usage())] {
        let f = Fixture::new();
        let pending = f.pending(1);
        let (tx, body) = f.delivery();
        let (started_tx, started) = oneshot::channel();
        let (finish_tx, finish) = oneshot::channel();
        let completion = pending.spawn(tx, move |_| async move {
            started_tx.send(()).unwrap();
            finish.await.unwrap();
            Ok(usage())
        });
        bounded(started).await.unwrap();
        drop(body);
        f.outcome(1, Outcome::InFlight, 948); // Duplicate reserve cannot spend again.
        let expected = if first.is_some() {
            Outcome::Settled { charged: 29 }
        } else {
            Outcome::Refunded
        };
        let balance = if first.is_some() { 971 } else { 1000 };
        // Adversarial external terminal contenders, not normal owner callers.
        assert_eq!(f.ledger.finish([1; 32], first).unwrap(), expected);
        let barrier = Arc::new(std::sync::Barrier::new(5));
        let contenders: Vec<_> = [None, Some(final_usage()), None, Some(final_usage())]
            .into_iter()
            .map(|usage| {
                let ledger = f.ledger.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    ledger.finish([1; 32], usage).unwrap()
                })
            })
            .collect();
        barrier.wait();
        for contender in contenders {
            assert_eq!(contender.join().unwrap(), expected);
        }
        f.outcome(1, expected.clone(), balance);
        assert_eq!(f.generations.available_permits(), 3);
        assert_eq!(f.resources.available_permits(), 3);
        finish_tx.send(()).unwrap();
        assert_eq!(bounded(completion).await.unwrap(), Ok(expected.clone()));
        assert_eq!(f.ledger.finish([1; 32], None).unwrap(), expected);
        f.outcome(1, expected, balance);
        f.leases_returned();
        f.account_slots_returned(balance);
    }
}

// Synthetic sequencing only: this is not a production renderer/adapter or a
// route integration claim. In particular, these fabricated usages are not proof
// of upstream verification. The eventual route must use only the verified adapter.
struct BorrowedRenderer<'a> {
    model: &'a str,
    history: &'a str,
}

impl BorrowedRenderer<'_> {
    fn complete(self, issue_token: impl FnOnce(&str, &str)) {
        issue_token(self.model, self.history);
    }
}

#[tokio::test]
async fn settling_callback_observes_charge_before_token_and_keeps_inputs_leased() {
    let f = Fixture::new();
    let pending = f.pending(1);
    let (tx, body) = f.delivery();
    let (probe, held, drops) = probe(&f);
    let ledger = f.ledger.clone();
    let tokens = Arc::new(AtomicUsize::new(0));
    let issued = tokens.clone();
    let (rendered_tx, rendered) = oneshot::channel();
    let (release_tx, release) = oneshot::channel();
    let completion = pending.spawn_settling(tx, move |_, settlement| async move {
        let _resource = probe;
        let model = String::from("m");
        let history = String::from("synthetic conversation snapshot");
        let renderer = BorrowedRenderer {
            model: &model,
            history: &history,
        };
        let result = Ok(usage()); // Synthetic stand-in for verified adapter EOF.
        let receipt = settlement.finish(&result);
        if matches!(receipt.outcome(), Ok(Outcome::Settled { .. })) {
            renderer.complete(|model, history| {
                // Re-entering accounting here also checks that its lock has
                // been released before a continuation/token callback runs.
                assert_eq!(ledger.available(ACCOUNT), Some(971));
                assert_eq!(
                    ledger.reserve(
                        ACCOUNT,
                        [1; 32],
                        BINDING,
                        quote(),
                        Instant::now() + Duration::from_secs(60),
                    ),
                    Ok(ReserveResult::Duplicate(Outcome::Settled { charged: 29 }))
                );
                assert_eq!(model, "m");
                assert_eq!(history, "synthetic conversation snapshot");
                issued.fetch_add(1, Ordering::SeqCst); // Token issuance simulation.
            });
        }
        rendered_tx.send(()).unwrap();
        release.await.unwrap();
        receipt
    });
    bounded(rendered).await.unwrap();
    f.outcome(1, Outcome::Settled { charged: 29 }, 971);
    assert_eq!(tokens.load(Ordering::SeqCst), 1);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    assert_eq!(f.generations.available_permits(), 3);
    assert_eq!(f.resources.available_permits(), 3);
    release_tx.send(()).unwrap();
    assert_eq!(
        bounded(completion).await.unwrap(),
        Ok(Outcome::Settled { charged: 29 })
    );
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert!(held.load(Ordering::SeqCst));
    f.leases_returned();
    drop(body);
    f.account_slots_returned(971);
}

#[tokio::test]
async fn settling_failed_or_invalid_usage_refunds_without_token() {
    for result in [
        Err(InferenceError::Unavailable),
        Err(InferenceError::InvalidResponse),
        Ok(StreamUsage {
            total_tokens: 8,
            ..usage()
        }),
        Ok(StreamUsage {
            input_tokens: u64::MAX,
            output_tokens: 1,
            total_tokens: 0,
        }),
        Ok(StreamUsage {
            input_tokens: u64::MAX,
            output_tokens: 0,
            total_tokens: u64::MAX,
        }),
    ] {
        let f = Fixture::new();
        let pending = f.pending(1);
        let (tx, body) = f.delivery();
        let tokens = Arc::new(AtomicUsize::new(0));
        let issued = tokens.clone();
        let completion = pending.spawn_settling(tx, move |_, settlement| async move {
            let receipt = settlement.finish(&result);
            assert_eq!(receipt.outcome(), &Ok(Outcome::Refunded));
            if matches!(receipt.outcome(), Ok(Outcome::Settled { .. })) {
                issued.fetch_add(1, Ordering::SeqCst);
            }
            receipt
        });
        assert_eq!(bounded(completion).await.unwrap(), Ok(Outcome::Refunded));
        assert_eq!(tokens.load(Ordering::SeqCst), 0);
        f.outcome(1, Outcome::Refunded, 1000);
        f.leases_returned();
        drop(body);
        f.account_slots_returned(1000);
    }
}

#[tokio::test]
async fn settling_pre_poll_or_held_cancellation_drops_inputs_before_leases_and_refunds() {
    for poll_first in [false, true] {
        let f = Fixture::new();
        let pending = f.pending(1);
        let (tx, body) = f.delivery();
        let (probe, held, drops) = probe(&f);
        let calls = Arc::new(AtomicUsize::new(0));
        let called = calls.clone();
        let (started_tx, started) = oneshot::channel();
        let worker = pending.spawn_settling_worker(tx, move |_, settlement| {
            called.fetch_add(1, Ordering::SeqCst);
            async move {
                let _resource = probe;
                started_tx.send(()).unwrap();
                let result = std::future::pending::<Result<StreamUsage, InferenceError>>().await;
                settlement.finish(&result)
            }
        });
        // No await yet on this current-thread runtime: the guard must already
        // belong to Worker, even though the factory has not run.
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        if poll_first {
            bounded(started).await.unwrap();
        }
        let abort = worker.abort_handle();
        let completion = supervise(worker);
        abort.abort();
        assert_eq!(
            bounded(completion).await.unwrap(),
            Err(OwnerError::Cancelled)
        );
        assert_eq!(calls.load(Ordering::SeqCst), usize::from(poll_first));
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert!(held.load(Ordering::SeqCst));
        f.outcome(1, Outcome::Refunded, 1000);
        f.leases_returned();
        drop(body);
        f.account_slots_returned(1000);
    }
}

#[tokio::test]
async fn settling_unused_capability_refunds_on_drop_but_cannot_return_success() {
    let f = Fixture::new();
    let pending = f.pending(1);
    let (tx, body) = f.delivery();
    let (dropped_tx, dropped) = oneshot::channel();
    let worker = pending.spawn_settling_worker(tx, move |_, settlement| async move {
        drop(settlement);
        dropped_tx.send(()).unwrap();
        // Without finish there is no receipt to return. An arbitrary Completion
        // is no longer a valid output; this future can only stall or diverge.
        std::future::pending::<SettledReceipt>().await
    });
    let abort = worker.abort_handle();
    let mut completion = supervise(worker);
    bounded(dropped).await.unwrap();
    f.outcome(1, Outcome::Refunded, 1000);
    assert_eq!(
        completion.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    );
    assert_eq!(f.generations.available_permits(), 3);
    assert_eq!(f.resources.available_permits(), 3);
    abort.abort();
    assert_eq!(
        bounded(completion).await.unwrap(),
        Err(OwnerError::Cancelled)
    );
    f.outcome(1, Outcome::Refunded, 1000);
    f.leases_returned();
    drop(body);
    f.account_slots_returned(1000);
}

#[tokio::test]
async fn settling_factory_or_future_panic_refunds_but_post_finish_panic_keeps_charge() {
    for stage in 0..3 {
        let f = Fixture::new();
        let pending = f.pending(1);
        let (tx, body) = f.delivery();
        let (probe, held, drops) = probe(&f);
        let completion = pending.spawn_settling(tx, move |_, settlement| {
            assert_ne!(stage, 0, "synthetic settling factory failure");
            async move {
                let _resource = probe;
                assert_ne!(stage, 1, "synthetic settling poll failure");
                assert_eq!(
                    settlement.finish(&Ok(usage())).outcome(),
                    &Ok(Outcome::Settled { charged: 29 })
                );
                panic!("synthetic post-settlement failure");
            }
        });
        assert_eq!(
            bounded(completion).await.unwrap(),
            Err(OwnerError::Panicked)
        );
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert!(held.load(Ordering::SeqCst));
        let (outcome, balance) = if stage == 2 {
            (Outcome::Settled { charged: 29 }, 971)
        } else {
            (Outcome::Refunded, 1000)
        };
        f.outcome(1, outcome, balance);
        f.leases_returned();
        drop(body);
        f.account_slots_returned(balance);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn settling_observer_and_body_loss_while_held_never_abort_work() {
    let f = Fixture::new();
    let pending = f.pending(1);
    let (tx, body) = f.delivery();
    let (probe, held, drops) = probe(&f);
    let (started_tx, started) = oneshot::channel();
    let (finish_tx, finish) = oneshot::channel();
    let (dropped_tx, dropped) = oneshot::channel();
    let completion = pending.spawn_settling(tx, move |mut tx, settlement| async move {
        let resource = probe;
        started_tx.send(()).unwrap();
        finish.await.unwrap();
        assert_eq!(tx.try_send(b"tail"), Err(DeliveryError::Closed));
        let receipt = settlement.finish(&Ok(usage()));
        drop(resource);
        dropped_tx.send(()).unwrap();
        receipt
    });
    bounded(started).await.unwrap();
    drop((completion, body));
    f.outcome(1, Outcome::InFlight, 948);
    assert_eq!(f.generations.available_permits(), 3);
    assert_eq!(f.resources.available_permits(), 3);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    finish_tx.send(()).unwrap();
    // Wait for the probe before reserving permits for this barrier: a queued
    // acquire_many itself reduces available_permits and would distort the probe.
    bounded(dropped).await.unwrap();
    let all = bounded(f.generations.clone().acquire_many_owned(4))
        .await
        .unwrap();
    f.outcome(1, Outcome::Settled { charged: 29 }, 971);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert!(held.load(Ordering::SeqCst));
    drop(all);
    f.leases_returned();
    f.account_slots_returned(971);
}

#[test]
fn panic_payload_is_not_exported_with_the_service_panic_hook() {
    const CHILD: &str = "POSSUMS_OWNER_PANIC_CHILD";
    const CANARY: &str = "private-owner-panic-content-canary";
    if std::env::var_os(CHILD).is_some() {
        // Match main's hook in an isolated process, never mutate the parallel
        // test harness's global hook. Catching a JoinError alone is insufficient.
        std::panic::set_hook(Box::new(|_| {}));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let f = Fixture::new();
            let pending = f.pending(1);
            let (tx, body) = f.delivery();
            let completion = pending.spawn(tx, |_| async {
                std::panic::panic_any(CANARY.to_owned());
            });
            assert_eq!(
                bounded(completion).await.unwrap(),
                Err(OwnerError::Panicked)
            );
            f.outcome(1, Outcome::Refunded, 1000);
            f.leases_returned();
            drop(body);
        });
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "generation_owner::tests::panic_payload_is_not_exported_with_the_service_panic_hook",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains(CANARY));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(CANARY));
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
}
