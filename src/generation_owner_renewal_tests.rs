//! Paused-clock owner tests; scripted replies pass the real renewal validator.
//! Tokio time does not advance wall time or claim real remote expiry.
use super::*;
use crate::{
    accounting::remote_fixture::renewing_script as script, intake::cutoff_fixture::renewal,
};

async fn tick(seconds: u64) {
    tokio::time::advance(Duration::from_secs(seconds)).await;
    tokio::task::yield_now().await;
}

#[tokio::test(start_paused = true)]
async fn renews_serially_then_stops_permanently_after_one_unknown_without_stopping_work() {
    let f = Fixture::new();
    let mut script = script();
    let mut owned = owner(&f, &script);
    let ready = owned.start_remote().unwrap();
    let (key, _) = bounded(&mut script.reserve_started).await.unwrap();
    let held = reservation(&key, 1800, "active");
    let first = renewal(&held, 600, None, false);
    let second = renewal(&first.reservation, 1200, None, false);
    let first_expiry = held.expires_at();
    let second_expiry = first.reservation.expires_at();
    assert!(script.reserve.send(Ok(held)).is_ok());
    assert!(bounded(ready).await.unwrap().is_ok());
    let (end, ended) = oneshot::channel();
    let completion = owned.spawn_settling((), |(), mut settlement| async move {
        settlement.evidence(crate::inference::forward::Evidence::sse(StreamUsage {
            input_tokens: 7,
            output_tokens: 7,
            total_tokens: 14,
        }));
        settlement.evidence(crate::inference::forward::Evidence::sse(usage()));
        ended.await.unwrap();
        settlement.finish(&Ok(usage())).await
    });
    tick(600).await;
    assert_eq!(script.renew_started.try_recv().unwrap(), first_expiry);
    assert!(script.renew.pop_front().unwrap().send(Ok(first)).is_ok());
    tokio::task::yield_now().await;
    tick(600).await;
    assert_eq!(script.renew_started.try_recv().unwrap(), second_expiry);
    assert!(script.renew.pop_front().unwrap().send(Ok(second)).is_ok());
    tokio::task::yield_now().await;
    tick(600).await;
    assert!(script.renew_started.try_recv().is_ok());
    assert!(script
        .renew
        .pop_front()
        .unwrap()
        .send(Err(AccountingError::StateUnavailable))
        .is_ok());
    tokio::task::yield_now().await;
    tick(7200).await;
    assert_eq!(script.service.renews.load(Ordering::SeqCst), 3);
    assert_eq!(script.service.finishes.load(Ordering::SeqCst), 0);
    assert_eq!(f.resources.available_permits(), 3);
    end.send(()).unwrap();
    assert_eq!(bounded(script.finish_started).await.unwrap(), Some(29));
    assert!(script
        .finish
        .send(receipt(TerminalOutcome::Settled { charged: 29 }, true))
        .is_ok());
    assert_eq!(
        bounded(completion).await.unwrap(),
        Ok(Outcome::Settled { charged: 29 })
    );
    f.lifecycle.quiesce();
    assert_eq!(bounded(f.lifecycle.wait_drained()).await, Ok(()));
    assert_eq!(script.service.finishes.load(Ordering::SeqCst), 1);
    f.leases_returned();
}

#[tokio::test(start_paused = true)]
async fn completion_during_renew_waits_for_its_evidence_and_never_overlaps_mutations() {
    let f = Fixture::new();
    let mut script = script();
    let mut owned = owner(&f, &script);
    let ready = owned.start_remote().unwrap();
    let (key, _) = bounded(&mut script.reserve_started).await.unwrap();
    let held = reservation(&key, 1800, "active");
    let reply = renewal(&held, 600, None, false);
    assert!(script.reserve.send(Ok(held)).is_ok());
    assert!(bounded(ready).await.unwrap().is_ok());
    let (end, ended) = oneshot::channel();
    let completion = owned.spawn_settling((), |(), settlement| async move {
        ended.await.unwrap();
        settlement.finish(&Ok(usage())).await
    });
    tick(600).await;
    assert!(script.renew_started.try_recv().is_ok());
    end.send(()).unwrap();
    tokio::task::yield_now().await;
    tick(4).await;
    assert_eq!(script.service.finishes.load(Ordering::SeqCst), 0);
    assert_eq!(script.service.renews.load(Ordering::SeqCst), 1);
    assert!(script.renew.pop_front().unwrap().send(Ok(reply)).is_ok());
    assert_eq!(bounded(script.finish_started).await.unwrap(), Some(29));
    assert!(script
        .finish
        .send(receipt(TerminalOutcome::Settled { charged: 29 }, true))
        .is_ok());
    assert_eq!(
        bounded(completion).await.unwrap(),
        Ok(Outcome::Settled { charged: 29 })
    );
    f.lifecycle.quiesce();
    assert_eq!(bounded(f.lifecycle.wait_drained()).await, Ok(()));
    f.leases_returned();
}

#[tokio::test(start_paused = true)]
async fn expired_confirmation_skips_renewal_but_never_cuts_off_accepted_work() {
    for (remaining, delay) in [(1, 600), (1800, 1900)] {
        let f = Fixture::new();
        let mut script = script();
        let mut owned = owner(&f, &script);
        let ready = owned.start_remote().unwrap();
        let (key, _) = bounded(&mut script.reserve_started).await.unwrap();
        assert!(script
            .reserve
            .send(Ok(reservation(&key, remaining, "active")))
            .is_ok());
        assert!(bounded(ready).await.unwrap().is_ok());
        tick(delay).await;
        assert_eq!(script.service.renews.load(Ordering::SeqCst), 0);
        assert_eq!(script.service.finishes.load(Ordering::SeqCst), 0);
        owned.expected_interruption();
        let cancel = tokio::spawn(owned.cancel());
        assert_eq!(bounded(script.finish_started).await.unwrap(), None);
        assert!(script
            .finish
            .send(receipt(TerminalOutcome::Expired, false))
            .is_ok());
        bounded(cancel).await.unwrap();
        f.lifecycle.quiesce();
        assert!(bounded(f.lifecycle.wait_drained()).await.is_err());
        f.leases_returned();
    }
}

#[tokio::test(start_paused = true)]
async fn closed_renewal_evidence_is_absorbing_but_not_early_billing_or_completion() {
    for terminal in [
        TerminalOutcome::Settled { charged: 29 },
        TerminalOutcome::Settled { charged: 7 },
        TerminalOutcome::Refunded,
        TerminalOutcome::Expired,
    ] {
        let f = Fixture::new();
        let mut script = script();
        let mut owned = owner(&f, &script);
        let ready = owned.start_remote().unwrap();
        let (key, _) = bounded(&mut script.reserve_started).await.unwrap();
        let held = reservation(&key, 1800, "active");
        let closed = renewal(
            &held,
            if terminal == TerminalOutcome::Expired {
                1800
            } else {
                600
            },
            Some(terminal),
            false,
        );
        assert!(script.reserve.send(Ok(held)).is_ok());
        assert!(bounded(ready).await.unwrap().is_ok());
        let (end, ended) = oneshot::channel();
        let completion = owned.spawn_settling((), move |(), settlement| async move {
            ended.await.unwrap();
            let receipt = settlement.finish(&Ok(usage())).await;
            assert_eq!(
                receipt.matches_requested(),
                terminal.matches_requested(Some(29))
            );
            if !receipt.matches_requested() {
                assert!(receipt.diagnostic().is_some());
            }
            receipt
        });
        tick(600).await;
        assert!(script.renew.pop_front().unwrap().send(Ok(closed)).is_ok());
        tokio::task::yield_now().await;
        tick(7200).await;
        assert_eq!(script.service.renews.load(Ordering::SeqCst), 1);
        assert_eq!(script.service.finishes.load(Ordering::SeqCst), 0);
        assert_eq!(f.resources.available_permits(), 3);
        end.send(()).unwrap();
        assert_eq!(bounded(completion).await.unwrap(), Ok(terminal.into()));
        assert_eq!(script.service.finishes.load(Ordering::SeqCst), 0);
        f.lifecycle.quiesce();
        assert_eq!(
            bounded(f.lifecycle.wait_drained()).await.is_ok(),
            terminal.matches_requested(Some(29))
        );
        f.leases_returned();
    }
}

#[tokio::test(start_paused = true)]
async fn disabled_refusal_stops_all_renewals_and_does_not_release_or_stop_work() {
    let f = Fixture::new();
    let mut script = script();
    let mut owned = owner(&f, &script);
    let ready = owned.start_remote().unwrap();
    let (key, _) = bounded(&mut script.reserve_started).await.unwrap();
    let held = reservation(&key, 1800, "active");
    let refusal = renewal(&held, 600, None, true);
    assert!(script.reserve.send(Ok(held)).is_ok());
    assert!(bounded(ready).await.unwrap().is_ok());
    tick(600).await;
    assert!(script.renew.pop_front().unwrap().send(Ok(refusal)).is_ok());
    tokio::task::yield_now().await;
    tick(7200).await;
    assert_eq!(script.service.renews.load(Ordering::SeqCst), 1);
    assert_eq!(script.service.finishes.load(Ordering::SeqCst), 0);
    let cancel = tokio::spawn(owned.cancel());
    assert_eq!(bounded(script.finish_started).await.unwrap(), None);
    assert!(script
        .finish
        .send(receipt(TerminalOutcome::Refunded, true))
        .is_ok());
    bounded(cancel).await.unwrap();
    f.lifecycle.quiesce();
    assert_eq!(bounded(f.lifecycle.wait_drained()).await, Ok(()));
    f.leases_returned();
}
