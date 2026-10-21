//! Ownership tests use a scripted service, not real credentials, network or inference.
use super::*;
use crate::accounting::remote_fixture::{receipt, reservation, script, Script};
use crate::intake::TerminalOutcome;
use futures_util::poll;

#[path = "generation_owner_renewal_fault_tests.rs"]
mod renewal_fault_tests;
#[path = "generation_owner_renewal_tests.rs"]
mod renewal_tests;

fn owner(f: &Fixture, script: &Script) -> ReservedGeneration {
    assert_eq!(
        script.accounting.reserve(
            ACCOUNT,
            [1; 32],
            BINDING,
            quote(),
            Instant::now() + Duration::from_secs(60)
        ),
        Ok(ReserveResult::Claimed)
    );
    ReservedGeneration::new(
        script.accounting.clone(),
        [1; 32],
        f.generations.clone().try_acquire_owned().unwrap(),
        Lease::from(f.resources.clone().try_acquire_owned().unwrap()),
    )
    .tracked(f.lifecycle.try_admit().unwrap())
}
fn duplicate(accounting: &Accounting, outcome: Outcome) {
    assert_eq!(
        accounting.reserve(
            ACCOUNT,
            [1; 32],
            BINDING,
            quote(),
            Instant::now() + Duration::from_secs(60)
        ),
        Ok(ReserveResult::Duplicate(outcome))
    );
}

#[tokio::test]
async fn lost_observer_and_owner_during_reserve_wait_for_late_hold_then_refund_once() {
    let f = Fixture::new();
    let script = script();
    let mut owner = owner(&f, &script);
    let ready = owner.start_remote().unwrap();
    let (key, maximum) = bounded(script.reserve_started).await.unwrap();
    assert_eq!(maximum, 52);
    duplicate(&script.accounting, Outcome::InFlight);
    owner.expected_interruption();
    drop((ready, owner));
    f.lifecycle.quiesce();
    let drain = f.lifecycle.wait_drained();
    tokio::pin!(drain);
    assert!(poll!(&mut drain).is_pending());
    assert_eq!(f.generations.available_permits(), 3);
    assert_eq!(f.resources.available_permits(), 3);
    assert!(script
        .reserve
        .send(Ok(reservation(&key, 1801, "active")))
        .is_ok());
    assert_eq!(bounded(script.finish_started).await.unwrap(), None);
    assert!(poll!(&mut drain).is_pending());
    assert!(script
        .finish
        .send(receipt(TerminalOutcome::Refunded, true))
        .is_ok());
    assert_eq!(bounded(drain).await, Ok(()));
    duplicate(&script.accounting, Outcome::Refunded);
    assert_eq!(script.service.reserves.load(Ordering::SeqCst), 1);
    assert_eq!(script.service.finishes.load(Ordering::SeqCst), 1);
    f.leases_returned();
}

#[tokio::test]
async fn unknown_reserve_and_explicit_refusals_never_start_work_or_replay() {
    for error in [
        AccountingError::StateUnavailable,
        AccountingError::InsufficientCredit,
        AccountingError::UnknownAccount,
        AccountingError::AlteredDuplicate,
    ] {
        let f = Fixture::new();
        let script = script();
        let mut owner = owner(&f, &script);
        let ready = owner.start_remote().unwrap();
        bounded(script.reserve_started).await.unwrap();
        assert!(script.reserve.send(Err(error)).is_ok());
        assert_eq!(bounded(ready).await.unwrap(), Err(error));
        // The admission gate rejects before constructing any prompt worker.
        owner.expected_interruption();
        bounded(owner.cancel()).await;
        duplicate(&script.accounting, Outcome::Unknown);
        assert_eq!(script.service.reserves.load(Ordering::SeqCst), 1);
        assert_eq!(script.service.finishes.load(Ordering::SeqCst), 0);
        f.lifecycle.quiesce();
        assert_eq!(
            bounded(f.lifecycle.wait_drained()).await.is_err(),
            error.reserve_uncertain()
        );
        f.leases_returned();
    }
}

#[tokio::test]
async fn panic_and_abort_keep_last_trustworthy_usage_and_actor_drain_root() {
    for abort in [false, true] {
        let f = Fixture::new();
        let script = script();
        let mut owner = owner(&f, &script);
        let ready = owner.start_remote().unwrap();
        let (key, _) = bounded(script.reserve_started).await.unwrap();
        assert!(script
            .reserve
            .send(Ok(reservation(&key, 1801, "active")))
            .is_ok());
        assert!(bounded(ready).await.unwrap().is_ok());
        let (captured, evidence) = oneshot::channel();
        let worker = owner.spawn_settling_worker((), move |(), mut settlement| async move {
            settlement.evidence(crate::inference::forward::Evidence::sse(StreamUsage {
                input_tokens: 6,
                output_tokens: 6,
                total_tokens: 12,
            }));
            settlement.evidence(crate::inference::forward::Evidence::sse(usage()));
            captured.send(()).unwrap();
            if abort {
                std::future::pending::<()>().await;
            }
            panic!("synthetic ownership fault");
        });
        bounded(evidence).await.unwrap();
        if abort {
            worker.abort();
        }
        assert_eq!(
            bounded(supervise(worker)).await.unwrap(),
            Err(if abort {
                OwnerError::Cancelled
            } else {
                OwnerError::Panicked
            })
        );
        assert_eq!(bounded(script.finish_started).await.unwrap(), Some(29));
        f.lifecycle.quiesce();
        let drain = f.lifecycle.wait_drained();
        tokio::pin!(drain);
        assert!(poll!(&mut drain).is_pending());
        assert_eq!(f.resources.available_permits(), 3);
        assert!(script
            .finish
            .send(receipt(TerminalOutcome::Settled { charged: 29 }, true))
            .is_ok());
        assert_eq!(bounded(drain).await, Ok(()));
        duplicate(&script.accounting, Outcome::Settled { charged: 29 });
        assert_eq!(script.service.finishes.load(Ordering::SeqCst), 1);
        f.leases_returned();
    }
}

#[tokio::test]
async fn first_terminal_mismatch_expiry_and_unknown_are_not_requested_success() {
    for (terminal, expected) in [
        (
            receipt(TerminalOutcome::Settled { charged: 7 }, false),
            Ok(Outcome::Settled { charged: 7 }),
        ),
        (
            receipt(TerminalOutcome::Refunded, false),
            Ok(Outcome::Refunded),
        ),
        (
            receipt(TerminalOutcome::Expired, false),
            Ok(Outcome::Expired),
        ),
        (
            Err(AccountingError::StateUnavailable),
            Err(OwnerError::Accounting(AccountingError::StateUnavailable)),
        ),
    ] {
        let f = Fixture::new();
        let script = script();
        let mut owner = owner(&f, &script);
        let ready = owner.start_remote().unwrap();
        let (key, _) = bounded(script.reserve_started).await.unwrap();
        assert!(script
            .reserve
            .send(Ok(reservation(&key, 1801, "active")))
            .is_ok());
        assert!(bounded(ready).await.unwrap().is_ok());
        let completion = owner.spawn_settling((), |(), settlement| async move {
            let receipt = settlement.finish(&Ok(usage())).await;
            assert!(!receipt.matches_requested());
            receipt
        });
        assert_eq!(bounded(script.finish_started).await.unwrap(), Some(29));
        assert!(script.finish.send(terminal).is_ok());
        assert_eq!(bounded(completion).await.unwrap(), expected);
        duplicate(&script.accounting, expected.unwrap_or(Outcome::Unknown));
        f.lifecycle.quiesce();
        assert_eq!(
            bounded(f.lifecycle.wait_drained()).await,
            Err(crate::lifecycle::DrainFailed)
        );
        assert_eq!(script.service.finishes.load(Ordering::SeqCst), 1);
        f.leases_returned();
    }
}

#[tokio::test]
async fn terminal_replay_or_elapsed_hold_never_admits_work() {
    for (state, remaining) in [
        ("refunded", 1801),
        ("expired", 0),
        ("settled", 1801),
        ("active", 0),
    ] {
        let f = Fixture::new();
        let script = script();
        let mut owner = owner(&f, &script);
        let ready = owner.start_remote().unwrap();
        let (key, _) = bounded(script.reserve_started).await.unwrap();
        assert!(script
            .reserve
            .send(Ok(reservation(&key, remaining, state)))
            .is_ok());
        assert_eq!(
            bounded(ready).await.unwrap(),
            Err(AccountingError::HoldUnavailable)
        );
        owner.expected_interruption();
        let cancellation = tokio::spawn(owner.cancel());
        if state == "active" {
            assert_eq!(bounded(script.finish_started).await.unwrap(), None);
            assert!(script
                .finish
                .send(receipt(TerminalOutcome::Expired, false))
                .is_ok());
        }
        bounded(cancellation).await.unwrap();
        assert_eq!(
            script.service.finishes.load(Ordering::SeqCst),
            usize::from(state == "active")
        );
        f.lifecycle.quiesce();
        assert_eq!(
            bounded(f.lifecycle.wait_drained()).await.is_err(),
            state == "active"
        );
        f.leases_returned();
    }
}

#[tokio::test]
async fn active_unexpired_hold_admits_without_minimum_work_budget() {
    let f = Fixture::new();
    let script = script();
    let mut owner = owner(&f, &script);
    let ready = owner.start_remote().unwrap();
    let (key, _) = bounded(script.reserve_started).await.unwrap();
    let active = reservation(&key, 29 * 60 + 5, "active");
    let expires_at = active.expires_at();
    assert!(script.reserve.send(Ok(active)).is_ok());
    assert_eq!(bounded(ready).await.unwrap(), Ok(expires_at));
    owner.expected_interruption();
    let cancellation = tokio::spawn(owner.cancel());
    assert_eq!(bounded(script.finish_started).await.unwrap(), None);
    assert!(script
        .finish
        .send(receipt(TerminalOutcome::Refunded, true))
        .is_ok());
    bounded(cancellation).await.unwrap();
    assert_eq!(script.service.reserves.load(Ordering::SeqCst), 1);
    assert_eq!(script.service.finishes.load(Ordering::SeqCst), 1);
    f.lifecycle.quiesce();
    assert_eq!(bounded(f.lifecycle.wait_drained()).await, Ok(()));
    f.leases_returned();
}

#[tokio::test]
async fn remote_terminal_owns_usage_primary_and_independent_settlement_failure_once() {
    use crate::telemetry::{AggregateMetrics, Deployment, Endpoint, SystemClock};
    let f = Fixture::new();
    let script = script();
    let metrics = Arc::new(AggregateMetrics::new(
        Deployment::IsolatedSynthetic,
        SystemClock::default(),
    ));
    metrics.prime_synthetic_application_window();
    let model = QualifiedModel::from_authenticated_quote(&quote());
    let http = metrics.http(Endpoint::ChatApi);
    let observation = metrics.generation(&http, model).into_owned(&metrics);
    let mut owner = owner(&f, &script).observed(Some(observation), model);
    let ready = owner.start_remote().unwrap();
    let (key, _) = bounded(script.reserve_started).await.unwrap();
    assert!(script
        .reserve
        .send(Ok(reservation(&key, 1801, "active")))
        .is_ok());
    assert!(bounded(ready).await.unwrap().is_ok());
    let completion = owner.spawn_settling((), |(), mut settlement| async move {
        settlement.evidence(crate::inference::forward::Evidence::sse(StreamUsage {
            input_tokens: 7,
            output_tokens: 7,
            total_tokens: 14,
        }));
        let mut last = crate::inference::forward::Evidence::sse(usage());
        last.cache_usage_available = true;
        last.cached_tokens = 0;
        settlement.evidence(last);
        settlement
            .finish(&Err(InferenceFailure::SdkStreamDecodeFailed.into()))
            .await
    });
    assert_eq!(bounded(script.finish_started).await.unwrap(), Some(29));
    assert!(script
        .finish
        .send(Err(AccountingError::StateUnavailable))
        .is_ok());
    assert_eq!(
        bounded(completion).await.unwrap(),
        Err(OwnerError::Accounting(AccountingError::StateUnavailable))
    );
    f.lifecycle.quiesce();
    assert!(bounded(f.lifecycle.wait_drained()).await.is_err());
    assert_eq!(metrics.usage_counts(model), [Some(5), Some(4), Some(0)]);
    assert_eq!(
        metrics.interface_error_count(InterfaceError::StreamDecodeFailed),
        Some(1)
    );
    assert_eq!(
        metrics.interface_error_count(InterfaceError::SettlementFailed),
        Some(1)
    );
    assert_eq!(
        metrics.interface_error_count(InterfaceError::GenerationUnknown),
        None
    );
    assert_eq!(script.service.finishes.load(Ordering::SeqCst), 1);
    f.leases_returned();
}

#[tokio::test]
async fn pre_poll_worker_abort_refunds_and_finish_future_loss_does_not_cancel_rpc() {
    for during_finish in [false, true] {
        let f = Fixture::new();
        let script = script();
        let mut owner = owner(&f, &script);
        let ready = owner.start_remote().unwrap();
        let (key, _) = bounded(script.reserve_started).await.unwrap();
        assert!(script
            .reserve
            .send(Ok(reservation(&key, 1801, "active")))
            .is_ok());
        assert!(bounded(ready).await.unwrap().is_ok());
        let worker = owner.spawn_settling_worker((), |(), settlement| async move {
            settlement.finish(&Ok(usage())).await
        });
        let expected = if during_finish { Some(29) } else { None };
        if !during_finish {
            worker.abort();
        }
        assert_eq!(bounded(script.finish_started).await.unwrap(), expected);
        worker.abort();
        assert_eq!(
            bounded(supervise(worker)).await.unwrap(),
            Err(OwnerError::Cancelled)
        );
        f.lifecycle.quiesce();
        let drain = f.lifecycle.wait_drained();
        tokio::pin!(drain);
        assert!(poll!(&mut drain).is_pending());
        let outcome = expected.map_or(TerminalOutcome::Refunded, |charged| {
            TerminalOutcome::Settled { charged }
        });
        assert!(script.finish.send(receipt(outcome, true)).is_ok());
        assert_eq!(bounded(drain).await, Ok(()));
        assert_eq!(script.service.finishes.load(Ordering::SeqCst), 1);
        f.leases_returned();
    }
}
