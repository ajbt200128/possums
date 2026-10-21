//! Bounded native SSE forwarding; completion and money are independent.
use crate::{
    accounting::Outcome,
    catalog::Model,
    failures::InterfaceError,
    generation::{PreparedGeneration, PreparedStructuredGeneration},
    generation_owner::{ReservedGeneration, Settlement},
    inference::{
        forward::{Evidence, ForwardSink},
        tools::ToolInvocation,
        InferenceError, InferenceFailure, Message, SharedInference,
    },
    stream_owner::{delivery, DeliveryBody, DeliveryError, DeliveryTx, Disconnect, Limits},
};
use async_trait::async_trait;
use futures_util::FutureExt;
use serde_json::{json, Value};
use std::sync::Arc;

const CHUNK_BYTES: usize = crate::inference::stream::MAX_FRAME_BYTES + 512;

#[derive(Clone, Copy)]
pub(crate) struct Options {
    pub(crate) usage_metrics: bool,
    intake_diagnostics: bool,
}
impl Options {
    pub(crate) fn parse(headers: &axum::http::HeaderMap) -> Self {
        let mut values = headers.get_all("x-tinfoil-request-usage-metrics").iter();
        Self {
            usage_metrics: values.next().is_some_and(|value| value == "true")
                && values.next().is_none(),
            intake_diagnostics: crate::intake::diagnostics::requested(headers),
        }
    }
}

pub(crate) fn compose(
    owner: ReservedGeneration,
    heavy: Arc<crate::telemetry::hooks::Lease>,
    mut input: PreparedGeneration,
    inference: SharedInference,
    options: Options,
) -> DeliveryBody {
    input.history.push(Message {
        role: "user".into(),
        content: input.prompt,
    });
    compose_core(
        owner,
        heavy,
        input.model,
        input.reserved_microunits,
        (
            input.cache_scope,
            input.history,
            None,
            input.reasoning_options,
        ),
        inference,
        options,
    )
}
pub(crate) fn compose_structured(
    owner: ReservedGeneration,
    heavy: Arc<crate::telemetry::hooks::Lease>,
    input: PreparedStructuredGeneration,
    inference: SharedInference,
    options: Options,
) -> DeliveryBody {
    compose_core(
        owner,
        heavy,
        input.model,
        input.reserved_microunits,
        (
            input.cache_scope,
            Vec::new(),
            Some(input.invocation),
            input.reasoning_options,
        ),
        inference,
        options,
    )
}
fn compose_core(
    owner: ReservedGeneration,
    heavy: Arc<crate::telemetry::hooks::Lease>,
    model: Model,
    reserved: u64,
    input: (
        crate::auth::CacheScope,
        Vec<Message>,
        Option<ToolInvocation>,
        crate::catalog::ReasoningOptions,
    ),
    inference: SharedInference,
    stream_options: Options,
) -> DeliveryBody {
    let Options {
        usage_metrics,
        intake_diagnostics,
    } = stream_options;
    let (scope, messages, invocation, options) = input;
    let disconnect = owner.disconnect();
    let (tx, body) = delivery(
        heavy.clone(),
        Limits {
            frames: 8,
            payload_bytes: (2 * CHUNK_BYTES) as u32,
            chunk_bytes: CHUNK_BYTES as u32,
        },
    );
    let body = body
        .observed(owner.delivery_observation())
        .disconnected(disconnect.clone());
    // This Arc retains only the aggregator, not a reservation/observation lease.
    // Terminal delivery can fail after settlement consumes its observation.
    let metrics = owner.metrics();
    let observer = owner.spawn_settling(tx, move |tx, mut settlement| async move {
        let mut output = Output {
            tx,
            model: &model.id,
            metrics,
            disconnect: disconnect.clone(),
            encoding_failure: None,
            delivery_failed: false,
            intake: None,
        };
        let (result, evidence) = {
            let mut sink = Sink { output: &mut output, settlement: &mut settlement, evidence: None };
            let result = tokio::select! {
                biased;
                _ = disconnect.cancelled() => {
                    sink.settlement.expected_interruption();
                    Err(InferenceFailure::StreamTransportFailed.into())
                },
                result = inference.generate_forwarded_request(&model, (&messages, invocation.as_ref(), &options), &scope, heavy, &mut sink) => result,
            };
            (result, sink.evidence)
        };
        let usage = result.as_ref().map(|c| c.usage).map_err(|e: &InferenceError| InferenceError::from(e.failure()));
        let receipt = settlement.finish(&usage).await;
        let complete = result.is_ok() && receipt.matches_requested() && matches!(receipt.outcome(), Ok(Outcome::Settled { .. }));
        if intake_diagnostics {
            // Preserve authoritative terminal evidence independently of any
            // primary provider or delivery failure.
            output.intake = receipt.diagnostic().or_else(|| match receipt.outcome() {
                Err(crate::generation_owner::OwnerError::Accounting(crate::accounting::AccountingError::Intake(error))) => Some(error.diagnostic()),
                Err(crate::generation_owner::OwnerError::Accounting(crate::accounting::AccountingError::HoldUnavailable)) => Some(crate::intake::diagnostics::Diagnostic::hold_unusable()),
                _ => None,
            });
        }
        let accounting_message = match receipt.outcome() {
            Err(crate::generation_owner::OwnerError::Accounting(error)) => Some(error.to_string()),
            Ok(Outcome::Expired) => Some("code=INTAKE_HOLD_EXPIRED stage=settlement constraint=immutable_hold_expiry. Intake returned an expired reservation, not the requested settlement. This is not successful completion; no replay occurred. Ask the operator to check accounting privately.".to_owned()),
            Ok(_) if !receipt.matches_requested() => Some("code=INTAKE_TERMINAL_MISMATCH stage=settlement constraint=first_terminal_wins. Intake returned an authoritative terminal outcome different from the requested charge or refund. The receipt describes that observed outcome, not successful generation. No replay occurred; check accounting privately.".to_owned()),
            _ => None,
        };
        let message = accounting_message.map(|accounting| match result.as_ref().err() {
            Some(error) => format!("{} {} {accounting}", error.failure(), InterfaceError::from(error.failure()).message()),
            None => accounting,
        });
        match receipt.outcome() {
            Ok(outcome @ (Outcome::Settled { .. } | Outcome::Refunded)) => {
                let charged = match outcome { Outcome::Settled { charged } => *charged, _ => 0 };
                let counts = evidence.as_ref().map(|e| {
                    let mut counts = json!({"prompt_tokens":e.usage.input_tokens,"completion_tokens":e.usage.output_tokens,"total_tokens":e.usage.total_tokens});
                    if e.cache_usage_available { counts["cached_tokens"] = json!(e.cached_tokens); }
                    counts
                });
                let _ = output.event(json!({"choices":[],"possums":{
                    "outcome":if matches!(outcome, Outcome::Refunded) { "refunded" } else { "settled" },
                    "status":if complete { "complete" } else { "incomplete" },
                    "reserved_microunits":reserved.to_string(), "charged_microunits":charged.to_string(),
                    "refunded_microunits":(reserved - charged).to_string(), "markup_percent":130,
                    "quoted_input_microunits_per_million_tokens":model.input_microunits_per_million_tokens.to_string(),
                    "quoted_output_microunits_per_million_tokens":model.output_microunits_per_million_tokens.to_string(),
                    "quoted_cached_input_microunits_per_million_tokens":model.cached_input_microunits_per_million_tokens.map(|r|r.to_string()),
                    "evidence":evidence.as_ref().map_or("none", |e|e.source), "usage":counts,
                    "provider_cost_usd":evidence.as_ref().and_then(|e| e.provider_cost_usd.as_deref()),
                }})).await;
                if !complete { let _ = output.failure(result.as_ref().err().map_or(InferenceFailure::SettlementFailed, |e|e.failure()), "unknown", message.as_deref()).await; }
                let _ = output.send(b"data: [DONE]\n\n").await;
            }
            _ => { let _ = output.failure(result.as_ref().err().map_or(InferenceFailure::SettlementFailed, |e| e.failure()), "unknown", message.as_deref()).await; }
        }
        // Clean EOF metrics can settle incomplete work, but only a complete
        // receipt permits the downstream trailer. Keep accounting evidence intact.
        let metrics = evidence.as_ref()
            .filter(|e| complete && usage_metrics && e.source == "final_metrics")
            .and_then(|e| e.final_metrics.clone());
        output.tx.finish_with_metrics(metrics);
        receipt
    });
    drop(observer);
    body
}
struct Output<'a> {
    tx: DeliveryTx,
    model: &'a str,
    metrics: Option<Arc<crate::telemetry::AggregateMetrics>>,
    disconnect: Disconnect,
    encoding_failure: Option<InterfaceError>,
    delivery_failed: bool,
    intake: Option<crate::intake::diagnostics::Diagnostic>,
}
impl Output<'_> {
    fn disconnected(&self) -> bool {
        self.disconnect.cancelled().now_or_never().is_some()
    }

    fn encoding_error(
        &mut self,
        failure: crate::bounded_json::EncodingFailure,
        projection: InferenceFailure,
    ) -> InferenceError {
        if self.encoding_failure.is_none() {
            let failure = match failure {
                crate::bounded_json::EncodingFailure::LimitExceeded => {
                    InterfaceError::ResponseEncodingLimit
                }
                crate::bounded_json::EncodingFailure::CauseUnknown => {
                    InterfaceError::ResponseEncodingUnknown
                }
            };
            self.encoding_failure = Some(failure);
            if let Some(metrics) = &self.metrics {
                metrics.interface_error(failure);
            }
        }
        projection.into()
    }

    async fn send(&mut self, frame: &[u8]) -> Result<(), InferenceError> {
        if let Err(error) = self.tx.send(frame).await {
            if error == DeliveryError::TooLarge {
                return Err(self.encoding_error(
                    crate::bounded_json::EncodingFailure::LimitExceeded,
                    InferenceFailure::StreamTransportFailed,
                ));
            }
            if !self.delivery_failed && !self.disconnected() {
                self.delivery_failed = true;
                if let Some(metrics) = &self.metrics {
                    metrics.interface_error(InterfaceError::ResponseDeliveryUnknown);
                }
            }
            return Err(InferenceFailure::StreamTransportFailed.into());
        }
        Ok(())
    }

    async fn event(&mut self, mut event: Value) -> Result<(), InferenceError> {
        event["object"] = json!("chat.completion.chunk");
        event["model"] = json!(self.model);
        let bytes = crate::bounded_json::to_vec(&event, CHUNK_BYTES - 8).map_err(|error| {
            self.encoding_error(error, InferenceFailure::StreamEventSchemaInvalid)
        })?;
        let mut frame = b"data: ".to_vec();
        frame.extend_from_slice(&bytes);
        frame.extend_from_slice(b"\n\n");
        self.send(&frame).await
    }
    async fn failure(
        &mut self,
        failure: InferenceFailure,
        billing: &'static str,
        primary_message: Option<&str>,
    ) -> Result<(), InferenceError> {
        let cause = self
            .encoding_failure
            .or_else(|| {
                self.delivery_failed
                    .then_some(InterfaceError::ResponseDeliveryUnknown)
            })
            .unwrap_or_else(|| failure.into());
        let message = primary_message
            .map(str::to_owned)
            .unwrap_or_else(|| format!("{failure} {}", cause.message()));
        let mut value = json!({"error":{"code":"generation_failed","detail":failure,"message":message,"billing":billing}});
        if let Some(diagnostic) = self.intake {
            value["error"]["intake"] = json!(diagnostic);
        }
        let bytes = crate::bounded_json::to_vec(&value, CHUNK_BYTES - 8)
            .map_err(|error| self.encoding_error(error, InferenceFailure::RequestEncodingFailed))?;
        let mut frame = b"data: ".to_vec();
        frame.extend_from_slice(&bytes);
        frame.extend_from_slice(b"\n\n");
        self.send(&frame).await
    }
}
struct Sink<'a, 'b> {
    output: &'a mut Output<'b>,
    settlement: &'a mut Settlement,
    evidence: Option<Evidence>,
}
#[async_trait]
impl ForwardSink for Sink<'_, '_> {
    async fn event(&mut self, event: Value) -> Result<(), InferenceError> {
        if event
            .pointer("/choices/0/delta/content")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty())
            || event
                .pointer("/choices/0/delta/tool_calls")
                .and_then(Value::as_array)
                .is_some_and(|calls| !calls.is_empty())
        {
            self.settlement.first_output();
        }
        let result = self.output.event(event).await;
        if let Err(error) = &result {
            self.settlement.local_failure(error.failure());
            if self.output.disconnected() {
                self.settlement.expected_interruption();
            }
        }
        result
    }
    fn evidence(&mut self, evidence: Evidence) {
        self.settlement.evidence(evidence.clone());
        self.evidence = Some(evidence);
    }
}

#[cfg(test)]
#[path = "api_stream_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "api_stream_intake_continued_work_tests.rs"]
mod intake_continued_work_tests;
