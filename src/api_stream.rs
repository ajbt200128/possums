//! Bounded SSE delivery. The detached owner, never the HTTP body, settles usage.
use crate::{
    accounting::Outcome,
    catalog::Model,
    generation::{PreparedGeneration, PreparedStructuredGeneration},
    generation_owner::{ReservedGeneration, Settlement},
    inference::{
        stream::FinishReason,
        tools::{CompletionDelta, ToolInvocation},
        DeltaSink, InferenceFailure, Message, SharedInference, TextDeltaSink,
    },
    stream_owner::{delivery, DeliveryBody, DeliveryTx, Limits},
};
use serde_json::{json, Value};
use std::sync::Arc;

const CHUNK_BYTES: usize = 8 * 1024;

pub(crate) fn compose(
    owner: ReservedGeneration,
    heavy: Arc<crate::telemetry::hooks::Lease>,
    input: PreparedGeneration,
    inference: SharedInference,
) -> DeliveryBody {
    compose_core(
        owner,
        heavy,
        input.model,
        input.reserved_microunits,
        RenderInput::Text {
            history: input.history,
            prompt: input.prompt,
        },
        inference,
    )
}

pub(crate) fn compose_structured(
    owner: ReservedGeneration,
    heavy: Arc<crate::telemetry::hooks::Lease>,
    input: PreparedStructuredGeneration,
    inference: SharedInference,
) -> DeliveryBody {
    compose_core(
        owner,
        heavy,
        input.model,
        input.reserved_microunits,
        RenderInput::Structured(input.invocation),
        inference,
    )
}

enum RenderInput {
    Text {
        history: Vec<Message>,
        prompt: String,
    },
    Structured(ToolInvocation),
}

fn compose_core(
    owner: ReservedGeneration,
    heavy: Arc<crate::telemetry::hooks::Lease>,
    model: Model,
    reserved_microunits: u64,
    mut input: RenderInput,
    inference: SharedInference,
) -> DeliveryBody {
    let (tx, body) = delivery(
        heavy.clone(),
        Limits {
            frames: 8,
            payload_bytes: 64 * 1024,
            chunk_bytes: CHUNK_BYTES as u32,
        },
    );
    let body = body.observed(owner.delivery_observation());
    let observer = owner.spawn_settling(tx, move |tx, mut settlement| async move {
        let mut output = Output {
            tx,
            model: &model.id,
        };
        output.event(
            json!({"choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}),
        ).await;
        let mut sink = OutputSink { output: &mut output, settlement: &mut settlement };
        let result = match &mut input {
            RenderInput::Text { history, prompt } => {
                history.push(Message { role: "user".into(), content: std::mem::take(prompt) });
                inference.generate_completion_stream(&model, history, heavy, &mut sink).await
            }
            RenderInput::Structured(invocation) => inference.generate_invocation_stream(
                &model, invocation, heavy, &mut sink,
            ).await,
        };
        let failure = result.as_ref().err().map(|error| error.failure());
        let usage = result
            .as_ref()
            .map(|completion| completion.usage)
            .map_err(|error| crate::inference::InferenceError::from(error.failure()));
        // Only validated upstream EOF can produce success. Commit the ledger BEFORE
        // publishing finish/usage/DONE; delivery failure never changes this receipt.
        let receipt = settlement.finish(&usage);
        match (result, receipt.outcome()) {
            (Ok(completion), Ok(Outcome::Settled { charged })) => {
                output.finish(completion.finish_reason).await;
                output.event(json!({"choices":[],"usage":{
                    "prompt_tokens":completion.usage.input_tokens,
                    "completion_tokens":completion.usage.output_tokens,
                    "total_tokens":completion.usage.total_tokens,
                },"possums":{
                    "outcome":"settled",
                    "quoted_input_microunits_per_million_tokens":model.input_microunits_per_million_tokens.to_string(),
                    "quoted_output_microunits_per_million_tokens":model.output_microunits_per_million_tokens.to_string(),
                    "charged_microunits":charged.to_string(),
                    "refunded_microunits":(reserved_microunits - charged).to_string(),
                }})).await;
                let _ = output.tx.send(b"data: [DONE]\n\n").await;
            }
            (Err(_), Ok(Outcome::Refunded)) => {
                output.failure(failure.unwrap_or(InferenceFailure::InferenceUnavailable), "refunded").await;
            }
            _ => output.failure(InferenceFailure::SettlementFailed, "unknown").await,
        }
        output.tx.finish();
        receipt
    });
    drop(observer); // Observation has no cancellation authority.
    body
}

struct Output<'a> {
    tx: DeliveryTx,
    model: &'a str,
}

impl Output<'_> {
    async fn event(&mut self, mut value: Value) {
        if self.tx.failure().is_some() {
            return;
        }
        value["object"] = json!("chat.completion.chunk");
        value["model"] = json!(self.model);
        // Small per-event serialization only, never a collected answer.
        let Ok(bytes) = crate::bounded_json::to_vec(&value, CHUNK_BYTES - 8) else {
            // Permanently detach on an encoder bound failure, without cancelling inference.
            self.tx.detach();
            return;
        };
        let mut frame = Vec::with_capacity(bytes.len() + 8);
        frame.extend_from_slice(b"data: ");
        frame.extend_from_slice(&bytes);
        frame.extend_from_slice(b"\n\n");
        let _ = self.tx.send(&frame).await;
    }

    async fn failure(&mut self, failure: InferenceFailure, billing: &'static str) {
        // Keep the legacy code first for older prefix-based consumers. No model,
        // SDK text, or response payload enters the diagnostic frame.
        #[derive(serde::Serialize)]
        struct ErrorBody {
            code: &'static str,
            detail: InferenceFailure,
            message: String,
            billing: &'static str,
        }
        #[derive(serde::Serialize)]
        struct Envelope {
            error: ErrorBody,
        }
        let error = Envelope {
            error: ErrorBody {
                code: "generation_failed",
                detail: failure,
                message: failure.to_string(),
                billing,
            },
        };
        if let Ok(bytes) = crate::bounded_json::to_vec(&error, CHUNK_BYTES - 8) {
            let mut frame = b"data: ".to_vec();
            frame.extend_from_slice(&bytes);
            frame.extend_from_slice(b"\n\n");
            let _ = self.tx.send(&frame).await;
        } else {
            self.tx.detach();
        }
    }

    async fn delta(&mut self, mut delta: &str) {
        // JSON may expand one byte to six; split on UTF-8 boundaries before escaping.
        while !delta.is_empty() && self.tx.failure().is_none() {
            let mut end = delta.len().min(1024);
            while !delta.is_char_boundary(end) {
                end -= 1;
            }
            let (text, remaining) = delta.split_at(end);
            self.event(
                json!({"choices":[{"index":0,"delta":{"content":text},"finish_reason":null}]}),
            )
            .await;
            delta = remaining;
        }
    }

    async fn structured_delta(&mut self, delta: CompletionDelta<'_>) {
        match delta {
            CompletionDelta::Text(text) => self.delta(text).await,
            CompletionDelta::ToolCall {
                index,
                id,
                name,
                arguments,
            } => {
                let mut remaining = arguments.unwrap_or_default();
                let mut first = true;
                loop {
                    if self.tx.failure().is_some() {
                        return;
                    }
                    let mut end = remaining.len().min(1024);
                    while !remaining.is_char_boundary(end) {
                        end -= 1;
                    }
                    let (fragment, rest) = remaining.split_at(end);
                    let mut call = json!({"index":index});
                    if first {
                        if let Some(id) = id {
                            call["id"] = json!(id);
                            call["type"] = json!("function");
                        }
                        if let Some(name) = name {
                            call["function"] = json!({"name":name});
                        }
                    }
                    if arguments.is_some() {
                        if call.get("function").is_none() {
                            call["function"] = json!({});
                        }
                        call["function"]["arguments"] = json!(fragment);
                    }
                    self.event(json!({"choices":[{"index":0,"delta":{"tool_calls":[call]},"finish_reason":null}]})).await;
                    if rest.is_empty() {
                        break;
                    }
                    remaining = rest;
                    first = false;
                }
            }
        }
    }

    async fn finish(&mut self, reason: FinishReason) {
        self.event(json!({"choices":[{"index":0,"delta":{},"finish_reason":reason}]}))
            .await;
    }
}

// The sink borrows both owners only for the upstream consuming call. Settlement
// is committed after EOF, before any terminal delivery capacity wait.
struct OutputSink<'a, 'model> {
    output: &'a mut Output<'model>,
    settlement: &'a mut Settlement,
}

#[async_trait::async_trait]
impl TextDeltaSink for OutputSink<'_, '_> {
    async fn emit(&mut self, text: &str) {
        if !text.is_empty() {
            self.settlement.first_output();
        }
        self.output.delta(text).await;
    }
}

#[async_trait::async_trait]
impl DeltaSink for OutputSink<'_, '_> {
    async fn emit(&mut self, delta: CompletionDelta<'_>) {
        if match &delta {
            CompletionDelta::Text(text) => !text.is_empty(),
            CompletionDelta::ToolCall { .. } => true,
        } {
            self.settlement.first_output();
        }
        self.output.structured_delta(delta).await;
    }
}
