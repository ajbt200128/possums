//! Production `/chat` composition. Call AFTER authenticated
//! catalog verification, a new reservation, THEN detached context preflight;
//! this module does no admission. Accounting acceptance is the successful reserve,
//! not this synchronous generation handoff. Detached route preflight passes
//! its original ReservedGeneration here even if its HTTP observer has disappeared.
//! Synthetic tests are not authenticated provider/network or aggregate RSS proof.
//! As with generation_owner, the service must suppress content in its panic hook.

use crate::{
    accounting::Outcome,
    auth::{Auth, ConversationId},
    catalog::Model,
    generation_owner::{OwnerError, ReservedGeneration},
    inference::{InferenceError, Message, SharedInference},
    render::{IncrementalRenderer, RenderOutcome},
    stream_owner::{delivery, DeliveryBody, DeliveryTx, Limits, StartupTx},
};
use std::{sync::Arc, time::Duration};
use tokio::sync::{oneshot, OwnedSemaphorePermit};

const LIMITS: Limits = Limits {
    frames: 8,
    payload_bytes: 64 * 1024,
    chunk_bytes: 8 * 1024,
};
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);

type Observer = oneshot::Receiver<Result<Outcome, OwnerError>>;

// No Debug: all of these strings are sensitive, including rejected inputs.
pub(crate) struct AcceptedChat {
    pub model: Model,
    pub history: Vec<Message>,
    pub prompt: String,
    pub session_id: String,
    pub csrf: String,
    pub conversation: ConversationId,
    #[cfg(test)]
    pub resource_hooks: Option<Arc<crate::web::resource_streaming_tests::ResourceHooks>>,
}

/// `heavy` MUST be the same Arc supplied to `owner`, not a second acquisition.
/// Synchronous handoff: neither startup nor inference runs in the request future.
/// The optional observer has no cancellation authority over accepted work.
pub(crate) fn compose(
    owner: ReservedGeneration,
    heavy: Arc<OwnedSemaphorePermit>,
    input: AcceptedChat,
    auth: Arc<Auth>,
    inference: SharedInference,
) -> (DeliveryBody, Observer) {
    compose_with_startup(owner, heavy, input, auth, inference, STARTUP_TIMEOUT, start)
}

// Keep the blocking boundary injectable privately for panic/shutdown tests.
fn compose_with_startup(
    owner: ReservedGeneration,
    heavy: Arc<OwnedSemaphorePermit>,
    input: AcceptedChat,
    auth: Arc<Auth>,
    inference: SharedInference,
    timeout: Duration,
    startup: impl FnOnce(Startup) -> Started + Send + 'static,
) -> (DeliveryBody, Observer) {
    let (tx, body) = delivery(heavy.clone(), LIMITS, timeout);
    #[cfg(test)]
    if let Some(hooks) = &input.resource_hooks {
        hooks.delivery(&body);
    }
    let observer = owner.spawn_settling(tx, move |tx, settlement| async move {
        // spawn_blocking outlives cancellation of its async waiter. Carry the
        // SAME heavy lease with both its input AND its unclaimed return value,
        // even after a failed sender has detached and the body has disappeared.
        let job = Startup {
            input,
            tx,
            _heavy: heavy,
        };
        #[cfg(test)]
        let hooks = job.input.resource_hooks.clone();
        let started = tokio::task::spawn_blocking(move || startup(job));
        #[cfg(test)]
        if let Some(hooks) = &hooks {
            hooks.queued(&started).await;
        }
        let Ok(started) = started.await else {
            // Never format/resume a panic payload; refund without sending input.
            return settlement.finish(&Err(InferenceError::Unavailable));
        };
        let Started {
            renderer,
            mut input,
            mut tx,
            _heavy,
        } = started;
        let Ok(mut renderer) = renderer else {
            return settlement.finish(&Err(InferenceError::InvalidResponse));
        };
        // Reuse the owned history allocation and move, rather than copy, prompt.
        input.history.push(Message {
            role: "user".into(),
            content: std::mem::take(&mut input.prompt),
        });
        let result = inference
            .generate_stream(&input.model, &input.history, &mut |delta| {
                renderer.delta(delta, |html| {
                    tx.try_send(html.as_bytes())
                        .map_err(|_| RenderOutcome::DeliveryFailed)
                });
            })
            .await;
        // Only the adapter's terminal result reaches accounting, once. A failed
        // delivery never cancels/replays upstream or changes its billing result.
        let receipt = settlement.finish(&result);
        let settled = matches!(receipt.outcome(), Ok(Outcome::Settled { .. }));
        // A valid upstream EOF can still refund on invalid usage arithmetic.
        // Render that as a failed generation, not a changed conversation.
        let display_result = if settled {
            result
        } else {
            Err(InferenceError::InvalidResponse)
        };
        renderer.complete(
            display_result,
            || {
                // complete calls this only while continuation remains usable.
                // finish has already released the accounting lock before auth.
                // If final enqueue fails after issuance, the unused token remains
                // bounded by auth capacity and its 15-minute expiry. Do not tie
                // settlement to delivery or add compensating token cleanup here.
                settled
                    .then(|| {
                        auth.issue_submission_for(
                            &input.session_id,
                            input.conversation,
                            Some(&input.model.id),
                        )
                        .ok()
                    })
                    .flatten()
            },
            |html| {
                tx.try_send(html.as_bytes())
                    .map_err(|_| RenderOutcome::DeliveryFailed)
            },
        );
        receipt
    });
    (body, observer)
}

// Explicit field order: input/rendering storage is destroyed before admission
// credit, including a queued blocking task or an unclaimed blocking result.
struct Startup {
    input: AcceptedChat,
    tx: StartupTx,
    _heavy: Arc<OwnedSemaphorePermit>,
}

struct Started {
    renderer: Result<IncrementalRenderer<'static>, RenderOutcome>,
    input: AcceptedChat,
    tx: DeliveryTx,
    _heavy: Arc<OwnedSemaphorePermit>,
}

fn start(mut job: Startup) -> Started {
    #[cfg(test)]
    if let Some(hooks) = &job.input.resource_hooks {
        tokio::runtime::Handle::current().block_on(hooks.at("startup-input"));
    }
    let renderer = (|| {
        let mut sink = |html: &str| {
            job.tx
                .send_blocking(html.as_bytes())
                .map_err(|_| RenderOutcome::DeliveryFailed)
        };
        let mut renderer = IncrementalRenderer::start(
            &job.input.history,
            &job.input.prompt,
            &job.input.csrf,
            &job.input.model.id,
            &mut sink,
        )?;
        // Delivery failure is renderer state, not invalid input: still finish
        // the staged transition and run accepted inference without output.
        while renderer.emit_next_history(&mut sink)? {}
        renderer.finish_start(&mut sink)?;
        renderer.into_streaming()
    })();
    #[cfg(test)]
    if let Some(hooks) = &job.input.resource_hooks {
        tokio::runtime::Handle::current().block_on(hooks.at("startup-result"));
    }
    Started {
        renderer,
        input: job.input,
        tx: job.tx.into_streaming(),
        _heavy: job._heavy,
    }
}

#[cfg(test)]
#[path = "streaming_chat_tests.rs"]
mod tests;
