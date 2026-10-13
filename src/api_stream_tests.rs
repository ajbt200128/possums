use super::*;
use crate::{
    accounting::{Accounting, AccountingError, ReserveResult},
    catalog::Quote,
    generation_owner::OwnerError,
    inference::stream::StreamUsage,
    telemetry::{
        hooks::Lease, AggregateMetrics, Deployment, Endpoint, QualifiedModel, SystemClock,
    },
};
use std::{
    sync::atomic::AtomicUsize,
    time::{Duration, Instant},
};

fn quote() -> Quote {
    Quote {
        model: Model {
            id: "m".into(),
            public_name: None,
            reasoning: None,
            reasoning_params: None,
            context_tokens: 100,
            max_output_tokens: 10,
            input_microunits_per_million_tokens: 2_000_000,
            output_microunits_per_million_tokens: 3_000_000,
            cached_input_microunits_per_million_tokens: None,
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
fn lease() -> Arc<Lease> {
    Arc::new(Lease::tracking(
        Arc::new(AtomicUsize::new(0)),
        None,
        crate::telemetry::Lane::Heavy,
    ))
}
fn metrics() -> Arc<AggregateMetrics> {
    let m = Arc::new(AggregateMetrics::new(
        Deployment::IsolatedSynthetic,
        SystemClock::default(),
    ));
    m.prime_synthetic_application_window();
    m
}
fn owner(
    m: &Arc<AggregateMetrics>,
    accounting_fails: bool,
) -> (ReservedGeneration, Arc<Accounting>) {
    let accounting = Arc::new(Accounting::new([("a".into(), 1000)]));
    assert_eq!(
        accounting
            .reserve(
                "a",
                [1; 32],
                [2; 32],
                quote(),
                Instant::now() + Duration::from_secs(60)
            )
            .unwrap(),
        ReserveResult::Reserved
    );
    let model = QualifiedModel::from_authenticated_quote(&quote());
    let http = m.http(Endpoint::ChatApi);
    let observation = m.generation(&http, model).into_owned(m);
    let owner = ReservedGeneration::new(
        if accounting_fails {
            Arc::new(Accounting::new([]))
        } else {
            accounting.clone()
        },
        [1; 32],
        Lease::tracking(
            Arc::new(AtomicUsize::new(0)),
            None,
            crate::telemetry::Lane::Generation,
        ),
        lease(),
    )
    .observed(Some(observation), model);
    (owner, accounting)
}
fn output(
    tx: DeliveryTx,
    metrics: Option<Arc<AggregateMetrics>>,
    disconnect: Disconnect,
) -> Output<'static> {
    Output {
        tx,
        model: "m",
        metrics,
        disconnect,
        encoding_failure: None,
        delivery_failed: false,
    }
}
fn channel() -> (DeliveryTx, DeliveryBody) {
    delivery(
        lease(),
        Limits {
            frames: 8,
            payload_bytes: (2 * CHUNK_BYTES) as u32,
            chunk_bytes: CHUNK_BYTES as u32,
        },
    )
}
async fn bounded<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(5), future)
        .await
        .unwrap()
}
fn oversized() -> Value {
    json!({"choices":[{"delta":{"content":"SYNTHETIC_PRIVATE_CONTENT".repeat(CHUNK_BYTES)}}]})
}

#[tokio::test]
async fn local_encoding_then_settlement_failure_is_counted_by_each_owner_once() {
    let m = metrics();
    let (owner, _) = owner(&m, true);
    let (tx, body) = channel();
    let global = owner.metrics();
    let completion = owner.spawn_settling(tx, move |tx, mut settlement| async move {
        let mut output = output(tx, global, Disconnect::default());
        let result = {
            let mut sink = Sink {
                output: &mut output,
                settlement: &mut settlement,
                evidence: None,
            };
            sink.evidence(Evidence::sse(usage()));
            let error = sink.event(oversized()).await.unwrap_err();
            assert_eq!(error.failure(), InferenceFailure::StreamEventSchemaInvalid);
            Err(error)
        };
        // Subsequent encoding failure is the same component, not another event.
        assert!(output.event(oversized()).await.is_err());
        let receipt = settlement.finish(&result);
        assert_eq!(
            output.encoding_failure,
            Some(InterfaceError::ResponseEncodingLimit)
        );
        receipt
    });
    assert_eq!(
        bounded(completion).await.unwrap(),
        Err(OwnerError::Accounting(AccountingError::InvalidTransition))
    );
    assert_eq!(
        m.interface_error_count(InterfaceError::ResponseEncodingLimit),
        Some(1)
    );
    assert_eq!(
        m.interface_error_count(InterfaceError::SettlementFailed),
        Some(1)
    );
    assert_eq!(
        m.interface_error_count(InterfaceError::StreamValidationFailed),
        None
    );
    assert_eq!(
        m.usage_counts(QualifiedModel::from_authenticated_quote(&quote())),
        [Some(5), Some(4), None]
    );
    drop(body);
}

#[tokio::test]
async fn terminal_receipt_error_and_done_delivery_after_settlement_count_once() {
    let m = metrics();
    let (owner, accounting) = owner(&m, false);
    let (tx, body) = channel();
    // Synthetic closed observer without an observed downstream disconnect.
    drop(body);
    let global = owner.metrics();
    let completion = owner.spawn_settling(tx, move |tx, settlement| async move {
        let mut output = output(tx, global, Disconnect::default());
        let receipt = settlement.finish(&Ok(usage()));
        assert_eq!(receipt.outcome(), &Ok(Outcome::Settled { charged: 29 }));
        assert!(output
            .event(json!({"choices":[],"possums":{"outcome":"settled"}}))
            .await
            .is_err());
        assert!(output
            .failure(InferenceFailure::SettlementFailed, "unknown")
            .await
            .is_err());
        assert!(output.send(b"data: [DONE]\n\n").await.is_err());
        receipt
    });
    assert_eq!(
        bounded(completion).await.unwrap(),
        Ok(Outcome::Settled { charged: 29 })
    );
    assert_eq!(accounting.available("a"), Some(971));
    assert_eq!(
        accounting.finish([1; 32], None),
        Ok(Outcome::Settled { charged: 29 })
    );
    assert_eq!(
        m.interface_error_count(InterfaceError::ResponseDeliveryUnknown),
        Some(1)
    );
    assert_eq!(
        m.interface_error_count(InterfaceError::SettlementFailed),
        None
    );
    assert_eq!(
        m.interface_error_count(InterfaceError::StreamTransportFailed),
        None
    );
    assert_eq!(
        m.interface_error_count(InterfaceError::GenerationUnknown),
        None
    );
}

#[tokio::test]
async fn primary_upstream_survives_independent_late_encoding_and_delivery_failures() {
    let m = metrics();
    let (owner, accounting) = owner(&m, false);
    let (tx, body) = channel();
    drop(body);
    let global = owner.metrics();
    let completion = owner.spawn_settling(tx, move |tx, mut settlement| async move {
        let mut output = output(tx, global, Disconnect::default());
        settlement.evidence(Evidence::sse(usage()));
        let receipt = settlement.finish(&Err(InferenceFailure::SdkStreamDecodeFailed.into()));
        assert!(output.event(oversized()).await.is_err());
        assert!(output
            .failure(InferenceFailure::SdkStreamDecodeFailed, "unknown")
            .await
            .is_err());
        assert!(output.send(b"data: [DONE]\n\n").await.is_err());
        receipt
    });
    assert_eq!(
        bounded(completion).await.unwrap(),
        Ok(Outcome::Settled { charged: 29 })
    );
    assert_eq!(accounting.available("a"), Some(971));
    for error in [
        InterfaceError::StreamDecodeFailed,
        InterfaceError::ResponseEncodingLimit,
        InterfaceError::ResponseDeliveryUnknown,
    ] {
        assert_eq!(m.interface_error_count(error), Some(1));
    }
    assert_eq!(
        m.interface_error_count(InterfaceError::StreamTransportFailed),
        None
    );
}

#[tokio::test]
async fn observed_disconnect_sink_failure_is_expected_and_keeps_partial_usage() {
    let m = metrics();
    let (owner, accounting) = owner(&m, false);
    let (tx, body) = channel();
    let disconnect = Disconnect::default();
    drop(body.disconnected(disconnect.clone()));
    let global = owner.metrics();
    let completion = owner.spawn_settling(tx, move |tx, mut settlement| async move {
        let mut output = output(tx, global, disconnect);
        let result = {
            let mut sink = Sink {
                output: &mut output,
                settlement: &mut settlement,
                evidence: None,
            };
            sink.evidence(Evidence::sse(usage()));
            Err(sink.event(json!({"choices":[]})).await.unwrap_err())
        };
        let receipt = settlement.finish(&result);
        assert!(output
            .failure(InferenceFailure::StreamTransportFailed, "unknown")
            .await
            .is_err());
        assert!(output.send(b"data: [DONE]\n\n").await.is_err());
        receipt
    });
    assert_eq!(
        bounded(completion).await.unwrap(),
        Ok(Outcome::Settled { charged: 29 })
    );
    assert_eq!(accounting.available("a"), Some(971));
    assert!(InterfaceError::ALL
        .into_iter()
        .all(|error| m.interface_error_count(error).is_none()));
    assert_eq!(
        m.usage_counts(QualifiedModel::from_authenticated_quote(&quote())),
        [Some(5), Some(4), None]
    );
}

#[tokio::test]
async fn encoder_unknown_is_local_closed_and_does_not_replace_earlier_component_failure() {
    let m = metrics();
    let (tx, body) = channel();
    let mut output = output(tx, Some(m.clone()), Disconnect::default());
    let projection = InferenceFailure::RequestEncodingFailed;
    assert_eq!(
        output
            .encoding_error(
                crate::bounded_json::EncodingFailure::CauseUnknown,
                projection
            )
            .failure(),
        projection
    );
    assert_eq!(
        output
            .encoding_error(
                crate::bounded_json::EncodingFailure::LimitExceeded,
                projection
            )
            .failure(),
        projection
    );
    assert_eq!(
        output.encoding_failure,
        Some(InterfaceError::ResponseEncodingUnknown)
    );
    assert_eq!(
        m.interface_error_count(InterfaceError::ResponseEncodingUnknown),
        Some(1)
    );
    assert_eq!(
        m.interface_error_count(InterfaceError::ResponseEncodingLimit),
        None
    );
    drop(body);
}
