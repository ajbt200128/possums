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
        ReservedGeneration::new(self.ledger.clone(), [id; 32], generation, resources)
    }

    fn pending(&self, id: u8) -> ReservedGeneration {
        self.pending_for(ACCOUNT, id)
    }

    fn delivery(&self) -> (DeliveryTx, DeliveryBody) {
        let (startup, body) = delivery(
            self.lanes.clone().try_acquire_owned().unwrap(),
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

    // Probe the real account counter without adding inspection APIs: exactly
    // three new reservations fit, a fourth does not, and cleanup restores credit.
    fn account_slots_returned(&self, balance: u64) {
        for id in 240..243 {
            assert_eq!(self.reserve(ACCOUNT, id).unwrap(), ReserveResult::Reserved);
        }
        assert_eq!(
            self.reserve(ACCOUNT, 243),
            Err(AccountingError::Concurrency)
        );
        assert_eq!(self.ledger.available(ACCOUNT), Some(balance - 3 * 52));
        for id in 240..243 {
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
async fn three_per_account_four_global_remain_held_after_disconnect() {
    let f = Fixture::new();
    let mut finishers = Vec::new();
    let mut completions = Vec::new();
    for (account, id) in [("a", 1), ("a", 2), ("a", 3), ("b", 4)] {
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
    assert_eq!(f.reserve("a", 5), Err(AccountingError::Concurrency));
    assert_eq!(f.ledger.available("a"), Some(844));
    assert_eq!(f.ledger.available("b"), Some(948));
    for finish in finishers {
        finish.send(()).unwrap();
    }
    for completion in completions {
        assert_eq!(
            bounded(completion).await.unwrap(),
            Ok(Outcome::Settled { charged: 29 })
        );
    }
    assert_eq!(f.ledger.available("a"), Some(913));
    assert_eq!(f.ledger.available("b"), Some(971));
    f.leases_returned();
    assert_eq!(f.lanes.available_permits(), 4);
    f.account_slots_returned(913);
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
