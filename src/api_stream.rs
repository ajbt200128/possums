//! Bounded native SSE forwarding; completion and money are independent.
use crate::{
    accounting::Outcome,
    catalog::Model,
    generation::{PreparedGeneration, PreparedStructuredGeneration},
    generation_owner::{ReservedGeneration, Settlement},
    inference::{
        forward::{Evidence, ForwardSink},
        tools::ToolInvocation,
        InferenceError, InferenceFailure, Message, SharedInference,
    },
    stream_owner::{delivery, DeliveryBody, DeliveryTx, Limits},
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

const CHUNK_BYTES: usize = crate::inference::stream::MAX_FRAME_BYTES + 512;

pub(crate) fn compose(
    owner: ReservedGeneration,
    heavy: Arc<crate::telemetry::hooks::Lease>,
    mut input: PreparedGeneration,
    inference: SharedInference,
    usage_metrics: bool,
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
        usage_metrics,
    )
}
pub(crate) fn compose_structured(
    owner: ReservedGeneration,
    heavy: Arc<crate::telemetry::hooks::Lease>,
    input: PreparedStructuredGeneration,
    inference: SharedInference,
    usage_metrics: bool,
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
        usage_metrics,
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
    usage_metrics: bool,
) -> DeliveryBody {
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
    let observer = owner.spawn_settling(tx, move |tx, mut settlement| async move {
        let mut output = Output { tx, model: &model.id };
        let (result, evidence) = {
            let mut sink = Sink { output: &mut output, settlement: &mut settlement, evidence: None };
            let result = tokio::select! {
                biased;
                _ = disconnect.cancelled() => Err(InferenceFailure::StreamTransportFailed.into()),
                result = inference.generate_forwarded_request(&model, (&messages, invocation.as_ref(), &options), &scope, heavy, &mut sink) => result,
            };
            (result, sink.evidence)
        };
        let usage = result.as_ref().map(|c| c.usage).map_err(|e: &InferenceError| InferenceError::from(e.failure()));
        let receipt = settlement.finish(&usage);
        let complete = result.is_ok() && matches!(receipt.outcome(), Ok(Outcome::Settled { .. }));
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
                if !complete { let _ = output.failure(result.as_ref().err().map_or(InferenceFailure::SettlementFailed, |e|e.failure()), "unknown").await; }
                let _ = output.tx.send(b"data: [DONE]\n\n").await;
            }
            _ => { let _ = output.failure(InferenceFailure::SettlementFailed, "unknown").await; }
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
}
impl Output<'_> {
    async fn event(&mut self, mut event: Value) -> Result<(), InferenceError> {
        event["object"] = json!("chat.completion.chunk");
        event["model"] = json!(self.model);
        let bytes = crate::bounded_json::to_vec(&event, CHUNK_BYTES - 8)
            .map_err(|_| InferenceFailure::StreamEventSchemaInvalid)?;
        let mut frame = b"data: ".to_vec();
        frame.extend_from_slice(&bytes);
        frame.extend_from_slice(b"\n\n");
        self.tx
            .send(&frame)
            .await
            .map_err(|_| InferenceFailure::StreamTransportFailed.into())
    }
    async fn failure(
        &mut self,
        failure: InferenceFailure,
        billing: &'static str,
    ) -> Result<(), InferenceError> {
        let bytes = serde_json::to_vec(&json!({"error":{"code":"generation_failed","detail":failure,"message":failure.to_string(),"billing":billing}})).map_err(|_| InferenceFailure::RequestEncodingFailed)?;
        let mut frame = b"data: ".to_vec();
        frame.extend_from_slice(&bytes);
        frame.extend_from_slice(b"\n\n");
        self.tx
            .send(&frame)
            .await
            .map_err(|_| InferenceFailure::StreamTransportFailed.into())
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
        self.output.event(event).await
    }
    fn evidence(&mut self, evidence: Evidence) {
        self.settlement.evidence(evidence.clone());
        self.evidence = Some(evidence);
    }
}
