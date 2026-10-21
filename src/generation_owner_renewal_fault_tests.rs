//! Local scheduling/loss qualification, not real service lifetime evidence.
use super::*;
use crate::{
    accounting::remote_fixture::renewing_script as script, intake::cutoff_fixture::renewal,
};

#[tokio::test(start_paused = true)]
async fn confirmation_is_mapped_from_dispatch_not_delayed_receipt() {
    let f = Fixture::new();
    let mut script = script();
    let mut owned = owner(&f, &script);
    let ready = owned.start_remote().unwrap();
    let (key, _) = bounded(&mut script.reserve_started).await.unwrap();
    let held = reservation(&key, 1800, "active");
    let renewed = renewal(&held, 600, None, false);
    assert!(script.reserve.send(Ok(held)).is_ok());
    assert!(bounded(ready).await.unwrap().is_ok());
    tokio::time::advance(Duration::from_secs(600)).await;
    tokio::task::yield_now().await;
    assert!(script.renew_started.try_recv().is_ok());
    // Receipt has four seconds of latency; it must not add four seconds to
    // the lease. Stall the actor again until the dispatch-based horizon.
    tokio::time::advance(Duration::from_secs(4)).await;
    assert!(script.renew.pop_front().unwrap().send(Ok(renewed)).is_ok());
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(1796)).await;
    tokio::task::yield_now().await;
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

#[tokio::test(start_paused = true)]
async fn actor_panic_during_renewal_is_unknown_and_never_fabricates_terminal_success() {
    let f = Fixture::new();
    let mut script = script();
    let mut owned = owner(&f, &script);
    let ready = owned.start_remote().unwrap();
    let (key, _) = bounded(&mut script.reserve_started).await.unwrap();
    assert!(script
        .reserve
        .send(Ok(reservation(&key, 1800, "active")))
        .is_ok());
    assert!(bounded(ready).await.unwrap().is_ok());
    script.service.panic_renew.store(true, Ordering::SeqCst);
    tokio::time::advance(Duration::from_secs(600)).await;
    tokio::task::yield_now().await;
    let completion = owned.spawn_settling((), |(), settlement| async move {
        let receipt = settlement.finish(&Ok(usage())).await;
        assert!(!receipt.matches_requested());
        receipt
    });
    assert_eq!(
        bounded(completion).await.unwrap(),
        Err(OwnerError::Accounting(AccountingError::StateUnavailable))
    );
    assert_eq!(script.service.finishes.load(Ordering::SeqCst), 0);
    f.lifecycle.quiesce();
    assert!(bounded(f.lifecycle.wait_drained()).await.is_err());
    f.leases_returned();
}

#[tokio::test(start_paused = true)]
async fn known_and_unknown_renewal_diagnostics_survive_without_changing_success_or_masking_terminal(
) {
    for known in [true, false] {
        for terminal_failed in [true, false] {
            let f = Fixture::new();
            let mut script = script();
            let mut owned = owner(&f, &script);
            let ready = owned.start_remote().unwrap();
            let (key, _) = bounded(&mut script.reserve_started).await.unwrap();
            assert!(script
                .reserve
                .send(Ok(reservation(&key, 1800, "active")))
                .is_ok());
            assert!(bounded(ready).await.unwrap().is_ok());
            tokio::time::advance(Duration::from_secs(600)).await;
            tokio::task::yield_now().await;
            let error = if known {
                AccountingError::Intake(crate::intake::cutoff_fixture::renewal_commit_unknown())
            } else {
                AccountingError::StateUnavailable
            };
            assert!(script.renew.pop_front().unwrap().send(Err(error)).is_ok());
            tokio::task::yield_now().await;
            tokio::time::advance(Duration::from_secs(7200)).await;
            let completion = owned.spawn_settling((), move |(), settlement| async move {
                let receipt = settlement.finish(&Ok(usage())).await;
                let diagnostic = serde_json::to_value(receipt.diagnostic().unwrap()).unwrap();
                assert_eq!(
                    diagnostic["operation"],
                    if terminal_failed {
                        "intake_settle"
                    } else {
                        "intake_renew"
                    }
                );
                assert_eq!(
                    diagnostic["constraint"],
                    if known || terminal_failed {
                        "commit_evidence"
                    } else {
                        "remote_cause_unknown"
                    }
                );
                assert_eq!(
                    diagnostic["status"],
                    if known || terminal_failed {
                        serde_json::json!(503)
                    } else {
                        serde_json::Value::Null
                    }
                );
                assert_eq!(receipt.matches_requested(), !terminal_failed);
                receipt
            });
            assert_eq!(bounded(script.finish_started).await.unwrap(), Some(29));
            let terminal = if terminal_failed {
                Err(AccountingError::Intake(
                    crate::intake::cutoff_fixture::commit_unknown(),
                ))
            } else {
                receipt(TerminalOutcome::Settled { charged: 29 }, true)
            };
            assert!(script.finish.send(terminal).is_ok());
            assert_eq!(bounded(completion).await.unwrap().is_ok(), !terminal_failed);
            assert_eq!(script.service.renews.load(Ordering::SeqCst), 1);
            f.lifecycle.quiesce();
            assert_eq!(
                bounded(f.lifecycle.wait_drained()).await.is_ok(),
                !terminal_failed
            );
            f.leases_returned();
        }
    }
}

#[test]
fn runtime_loss_during_renewal_retires_roots_without_fabricating_a_receipt_or_clean_drain() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let f = Fixture::new();
    let mut script = script();
    let service = script.service.clone();
    let owned = runtime.block_on(async {
        tokio::time::pause();
        let mut owned = owner(&f, &script);
        let ready = owned.start_remote().unwrap();
        let (key, _) = bounded(&mut script.reserve_started).await.unwrap();
        assert!(script
            .reserve
            .send(Ok(reservation(&key, 1800, "active")))
            .is_ok());
        assert!(bounded(ready).await.unwrap().is_ok());
        tokio::time::advance(Duration::from_secs(600)).await;
        tokio::task::yield_now().await;
        assert!(bounded(script.renew_started.recv()).await.is_some());
        owned
    });
    drop(runtime);
    assert_eq!(service.finishes.load(Ordering::SeqCst), 0);
    assert_eq!(service.renews.load(Ordering::SeqCst), 1);
    // Dropping the remaining capability discovers actor loss; no runtime exists
    // to recover or dispatch another attempt. Unknown accounting cannot drain clean.
    drop(owned);
    f.lifecycle.quiesce();
    let check = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    assert!(check.block_on(f.lifecycle.wait_drained()).is_err());
    f.leases_returned();
}
