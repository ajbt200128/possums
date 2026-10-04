//! Bounded SSE delivery. The detached owner, never the HTTP body, settles usage.
use crate::{
    accounting::Outcome,
    generation::PreparedGeneration,
    generation_owner::ReservedGeneration,
    inference::{stream::FinishReason, Message, SharedInference},
    stream_owner::{delivery, DeliveryBody, DeliveryTx, Limits},
};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tokio::sync::OwnedSemaphorePermit;

const CHUNK_BYTES: usize = 8 * 1024;

pub(crate) fn compose(
    owner: ReservedGeneration,
    heavy: Arc<OwnedSemaphorePermit>,
    mut input: PreparedGeneration,
    inference: SharedInference,
) -> DeliveryBody {
    let (tx, body) = delivery(
        heavy.clone(),
        Limits {
            frames: 8,
            payload_bytes: 64 * 1024,
            chunk_bytes: CHUNK_BYTES as u32,
        },
        Duration::from_secs(30),
    );
    let tx = tx.into_streaming();
    let observer = owner.spawn_settling(tx, move |tx, settlement| async move {
        let mut output = Output {
            tx,
            model: &input.model.id,
        };
        output.event(
            json!({"choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}),
        );
        input.history.push(Message {
            role: "user".into(),
            content: std::mem::take(&mut input.prompt),
        });
        let result = inference
            .generate_completion_stream(&input.model, &input.history, heavy, &mut |delta| {
                output.delta(delta);
            })
            .await;
        let usage = result
            .as_ref()
            .map(|completion| completion.usage)
            .map_err(|_| crate::inference::InferenceError::InvalidResponse);
        // Only validated upstream EOF can produce success. Commit the ledger BEFORE
        // publishing finish/usage/DONE; delivery failure never changes this receipt.
        let receipt = settlement.finish(&usage);
        match (result, receipt.outcome()) {
            (Ok(completion), Ok(Outcome::Settled { charged })) => {
                output.finish(completion.finish_reason);
                output.event(json!({"choices":[],"usage":{
                    "prompt_tokens":completion.usage.input_tokens,
                    "completion_tokens":completion.usage.output_tokens,
                    "total_tokens":completion.usage.total_tokens,
                },"possums":{
                    "outcome":"settled",
                    "charged_microunits":charged.to_string(),
                    "refunded_microunits":(input.reserved_microunits - charged).to_string(),
                }}));
                let _ = output.tx.try_send(b"data: [DONE]\n\n");
            }
            _ => {
                let _ = output
                    .tx
                    .try_send(b"data: {\"error\":{\"code\":\"generation_failed\"}}\n\n");
            }
        }
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
    fn event(&mut self, mut value: Value) {
        if self.tx.failure().is_some() {
            return;
        }
        value["object"] = json!("chat.completion.chunk");
        value["model"] = json!(self.model);
        // Small per-event serialization only, never a collected answer.
        let Ok(bytes) = crate::bounded_json::to_vec(&value, CHUNK_BYTES - 8) else {
            // Permanently detach on an encoder bound failure, just like queue overflow.
            self.tx.detach();
            return;
        };
        let mut frame = Vec::with_capacity(bytes.len() + 8);
        frame.extend_from_slice(b"data: ");
        frame.extend_from_slice(&bytes);
        frame.extend_from_slice(b"\n\n");
        let _ = self.tx.try_send(&frame);
    }

    fn delta(&mut self, mut delta: &str) {
        // JSON may expand one byte to six; split on UTF-8 boundaries before escaping.
        while !delta.is_empty() && self.tx.failure().is_none() {
            let mut end = delta.len().min(1024);
            while !delta.is_char_boundary(end) {
                end -= 1;
            }
            let (text, remaining) = delta.split_at(end);
            self.event(
                json!({"choices":[{"index":0,"delta":{"content":text},"finish_reason":null}]}),
            );
            delta = remaining;
        }
    }

    fn finish(&mut self, reason: FinishReason) {
        self.event(json!({"choices":[{"index":0,"delta":{},"finish_reason":reason}]}));
    }
}
