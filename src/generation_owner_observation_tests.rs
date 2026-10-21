use super::*;
use crate::{
    failures::InterfaceError as E,
    inference::{forward::Evidence, InferenceFailure as I},
    telemetry::{AggregateMetrics, Deployment, Endpoint, SystemClock},
};

fn metrics() -> Arc<AggregateMetrics> {
    let metrics = Arc::new(AggregateMetrics::new(
        Deployment::IsolatedSynthetic,
        SystemClock::default(),
    ));
    metrics.prime_synthetic_application_window();
    metrics
}

fn observed(owner: ReservedGeneration, metrics: &Arc<AggregateMetrics>) -> ReservedGeneration {
    let model = QualifiedModel::from_authenticated_quote(&quote());
    let http = metrics.http(Endpoint::ChatApi);
    let observation = metrics.generation(&http, model).into_owned(metrics);
    owner.observed(Some(observation), model)
}

fn assert_usage(metrics: &AggregateMetrics, counts: [Option<u64>; 3]) {
    assert_eq!(
        metrics.usage_counts(QualifiedModel::from_authenticated_quote(&quote())),
        counts
    );
}

#[tokio::test]
async fn terminal_numeric_observations_do_not_require_an_eligible_original_pool_key() {
    let f = Fixture::new();
    let m = Arc::new(AggregateMetrics::new(
        Deployment::IsolatedSynthetic,
        SystemClock::default(),
    ));
    // Start during synthetic warmup: HTTP/generation records are inert.
    let owner = observed(f.pending(1), &m);
    assert_eq!(m.application_counts()[4], 0);
    // Advance eligibility only in this synthetic fixture. The terminal must use
    // the global current window rather than requiring its old record to be live.
    m.prime_synthetic_application_window();
    let (tx, body) = f.delivery();
    let completion = owner.spawn_settling(tx, |_, mut settlement| async move {
        settlement.evidence(Evidence::sse(usage()));
        settlement
            .finish(&Err(I::SdkStreamDecodeFailed.into()))
            .await
    });
    assert_eq!(
        bounded(completion).await.unwrap(),
        Ok(Outcome::Settled { charged: 29 })
    );
    assert_eq!(m.application_counts()[5], 0);
    assert_eq!(m.interface_error_count(E::StreamDecodeFailed), Some(1));
    assert_usage(&m, [Some(5), Some(4), None]);
    drop(body);
    f.leases_returned();
}

#[tokio::test]
async fn primary_and_independent_accounting_failure_are_once_only_and_keep_usage() {
    let f = Fixture::new();
    let m = metrics();
    let mut owner = observed(f.pending(1), &m);
    owner.terminal.accounting = Arc::new(Accounting::new([]));
    owner.failure(GenerationTerminal::Decoding, E::StreamDecodeFailed);
    let (tx, body) = f.delivery();
    let completion = owner.spawn_settling(tx, |_, mut settlement| async move {
        let mut evidence = Evidence::sse(usage());
        evidence.cache_usage_available = true;
        evidence.cached_tokens = 0;
        settlement.evidence(evidence);
        settlement.expected_interruption();
        settlement
            .finish(&Err(I::StreamTransportFailed.into()))
            .await
    });
    assert_eq!(
        bounded(completion).await.unwrap(),
        Err(OwnerError::Accounting(AccountingError::InvalidTransition))
    );
    assert_eq!(m.interface_error_count(E::StreamDecodeFailed), Some(1));
    assert_eq!(m.interface_error_count(E::SettlementFailed), Some(1));
    assert_eq!(m.interface_error_count(E::StreamTransportFailed), None);
    assert_eq!(m.interface_error_count(E::GenerationUnknown), None);
    assert_usage(&m, [Some(5), Some(4), Some(0)]);
    drop(body);
    f.leases_returned();
}

#[tokio::test]
async fn terminal_uses_final_or_last_cumulative_not_sum_and_distinguishes_cache_zero() {
    for (complete, cache_available, fallback) in [
        (true, true, false),
        (false, true, false),
        (false, false, false),
        (true, false, true),
    ] {
        let f = Fixture::new();
        let m = metrics();
        let owner = observed(f.pending(1), &m);
        let (tx, body) = f.delivery();
        let completion = owner.spawn_settling(tx, move |_, mut settlement| async move {
            if !fallback {
                settlement.evidence(Evidence::sse(StreamUsage {
                    input_tokens: 7,
                    output_tokens: 6,
                    total_tokens: 13,
                }));
                let mut last = Evidence::sse(usage());
                last.cache_usage_available = cache_available;
                last.cached_tokens = 0;
                settlement.evidence(last);
            }
            if complete {
                // Stored authenticated evidence overrides a different fallback.
                settlement
                    .finish(&Ok(if fallback {
                        usage()
                    } else {
                        StreamUsage {
                            input_tokens: 8,
                            output_tokens: 8,
                            total_tokens: 16,
                        }
                    }))
                    .await
            } else {
                settlement.expected_interruption();
                settlement
                    .finish(&Err(I::StreamTransportFailed.into()))
                    .await
            }
        });
        assert_eq!(
            bounded(completion).await.unwrap(),
            Ok(Outcome::Settled { charged: 29 })
        );
        assert_usage(&m, [Some(5), Some(4), cache_available.then_some(0)]);
        assert!(E::ALL
            .into_iter()
            .all(|error| m.interface_error_count(error).is_none()));
        drop(body);
        f.leases_returned();
    }
}

#[tokio::test]
async fn supplied_primary_survives_drop_and_expected_cleanup_without_reclassification() {
    let f = Fixture::new();
    let m = metrics();
    let mut owner = observed(f.pending(1), &m);
    owner.failure(GenerationTerminal::Transport, E::TokenizerTransportFailed);
    owner.expected_interruption();
    drop(owner);
    assert_eq!(
        m.interface_error_count(E::TokenizerTransportFailed),
        Some(1)
    );
    assert_eq!(m.interface_error_count(E::PreflightUnknown), None);
    assert_eq!(m.interface_error_count(E::GenerationUnknown), None);
    f.outcome(1, Outcome::Refunded, 1000);
    f.leases_returned();
}

#[tokio::test]
async fn default_drop_and_pre_poll_worker_loss_use_the_actual_owner_stage() {
    for worker_handoff in [false, true] {
        let f = Fixture::new();
        let m = metrics();
        let owner = observed(f.pending(1), &m);
        if worker_handoff {
            let (tx, body) = f.delivery();
            let worker = owner.spawn_settling_worker(tx, |_, _settlement| async move {
                std::future::pending::<SettledReceipt>().await
            });
            worker.abort();
            assert_eq!(
                bounded(supervise(worker)).await.unwrap(),
                Err(OwnerError::Cancelled)
            );
            drop(body);
        } else {
            drop(owner);
        }
        assert_eq!(
            m.interface_error_count(E::PreflightUnknown),
            (!worker_handoff).then_some(1)
        );
        assert_eq!(
            m.interface_error_count(E::GenerationUnknown),
            worker_handoff.then_some(1)
        );
        assert_usage(&m, [None; 3]);
        f.outcome(1, Outcome::Refunded, 1000);
        f.leases_returned();
    }
}

#[tokio::test]
async fn expected_disconnect_does_not_invent_fault_or_usage() {
    let f = Fixture::new();
    let m = metrics();
    let owner = observed(f.pending(1), &m);
    let (tx, body) = f.delivery();
    let completion = owner.spawn_settling(tx, |_, mut settlement| async move {
        settlement.expected_interruption();
        settlement
            .finish(&Err(I::StreamTransportFailed.into()))
            .await
    });
    assert_eq!(bounded(completion).await.unwrap(), Ok(Outcome::Refunded));
    assert!(E::ALL
        .into_iter()
        .all(|error| m.interface_error_count(error).is_none()));
    assert_usage(&m, [None; 3]);
    drop(body);
    f.leases_returned();
}

#[tokio::test]
async fn panic_cleanup_observes_unknown_once_but_post_settlement_panic_never_recounts() {
    for settled in [false, true] {
        let f = Fixture::new();
        let m = metrics();
        let owner = observed(f.pending(1), &m);
        let (tx, body) = f.delivery();
        let completion = owner.spawn_settling(tx, move |_, mut settlement| async move {
            settlement.evidence(Evidence::sse(usage()));
            if settled {
                let receipt = settlement
                    .finish(&Err(I::SdkStreamDecodeFailed.into()))
                    .await;
                assert_eq!(receipt.outcome(), &Ok(Outcome::Settled { charged: 29 }));
            }
            panic!("synthetic owner cleanup");
        });
        assert_eq!(
            bounded(completion).await.unwrap(),
            Err(OwnerError::Panicked)
        );
        assert_eq!(
            m.interface_error_count(E::GenerationUnknown),
            (!settled).then_some(1)
        );
        assert_eq!(
            m.interface_error_count(E::StreamDecodeFailed),
            settled.then_some(1)
        );
        assert_usage(&m, [Some(5), Some(4), None]);
        f.outcome(1, Outcome::Settled { charged: 29 }, 971);
        drop(body);
        f.leases_returned();
    }
}

#[tokio::test]
async fn output_owned_projection_does_not_recount_but_other_upstream_fault_survives() {
    for same in [false, true] {
        let f = Fixture::new();
        let m = metrics();
        let owner = observed(f.pending(1), &m);
        let (tx, body) = f.delivery();
        let completion = owner.spawn_settling(tx, move |_, mut settlement| async move {
            settlement.local_failure(I::StreamEventSchemaInvalid);
            let failure = if same {
                I::StreamEventSchemaInvalid
            } else {
                I::SdkStreamDecodeFailed
            };
            settlement.finish(&Err(failure.into())).await
        });
        assert_eq!(bounded(completion).await.unwrap(), Ok(Outcome::Refunded));
        assert_eq!(m.interface_error_count(E::StreamValidationFailed), None);
        assert_eq!(
            m.interface_error_count(E::StreamDecodeFailed),
            (!same).then_some(1)
        );
        drop(body);
        f.leases_returned();
    }
}
