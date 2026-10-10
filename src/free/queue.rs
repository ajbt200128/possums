use super::{budget::Budget, Core, Diagnostic, Failure, Stage};
use crate::{
    api_stream::Output,
    catalog::Catalog,
    inference::{
        tools::{present, Tool, ToolChoice, ToolInvocation, ToolMessage},
        InferenceFailure, Message,
    },
    stream_owner::DeliveryTx,
    telemetry::hooks::Lease,
};
use axum::body::Bytes;
use futures_util::FutureExt;
use serde::Deserialize;
use std::{
    collections::VecDeque,
    panic::AssertUnwindSafe,
    sync::{Arc, Mutex, Weak},
};
use tokio::{sync::Notify, time::Instant};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Queued,
    Started,
    Cancelled,
}

/// Cancellation and the first prompt-bearing dispatch share one linearization point.
/// After Started, only the worker can retire ownership, never the downstream body.
pub(super) struct Position {
    phase: Mutex<Phase>,
    queue: Weak<Queue>,
    cancelled: Notify,
    pub(super) deadline: Instant,
}
impl Position {
    pub(super) fn new(deadline: Instant, queue: Weak<Queue>) -> Self {
        Self {
            phase: Mutex::new(Phase::Queued),
            queue,
            cancelled: Notify::new(),
            deadline,
        }
    }
    pub(super) fn cancel(&self) {
        {
            let mut phase = self.phase.lock().unwrap_or_else(|e| e.into_inner());
            if *phase != Phase::Queued {
                return;
            }
            *phase = Phase::Cancelled;
        }
        // Never acquire the queue lock while holding phase. Dispatch holds queue
        // then phase; either cancellation wins, or only the worker owns the job.
        if let Some(queue) = self.queue.upgrade() {
            queue.remove(self);
        }
        self.cancelled.notify_one();
    }
    fn start(&self) -> bool {
        let mut phase = self.phase.lock().unwrap_or_else(|e| e.into_inner());
        if *phase != Phase::Queued || Instant::now() >= self.deadline {
            return false;
        }
        *phase = Phase::Started;
        true
    }
}

#[derive(Default)]
pub(super) struct Queue {
    pending: Mutex<VecDeque<Job>>,
    ready: Arc<Notify>,
}
impl Queue {
    pub(super) fn push(&self, job: Job) -> Result<(), ()> {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        if pending.len() >= super::CONNECTIONS
            || *job.position.phase.lock().unwrap_or_else(|e| e.into_inner()) != Phase::Queued
            || Instant::now() >= job.position.deadline
        {
            return Err(());
        }
        pending.push_back(job);
        self.ready.notify_one();
        Ok(())
    }

    fn remove(&self, position: &Position) -> Option<Job> {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        let index = pending
            .iter()
            .position(|job| std::ptr::eq(job.position.as_ref(), position))?;
        pending.remove(index)
    }

    fn dispatch(&self, position: &Position) -> Option<Job> {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        if !pending
            .front()
            .is_some_and(|job| std::ptr::eq(job.position.as_ref(), position))
            || !position.start()
        {
            return None;
        }
        pending.pop_front()
    }

    #[cfg(test)]
    pub(super) fn occupancy(&self) -> (usize, usize) {
        let pending = self.pending.lock().unwrap();
        (pending.len(), pending.iter().map(|job| job.raw.len()).sum())
    }
}
impl Drop for Queue {
    fn drop(&mut self) {
        self.ready.notify_one();
    }
}

pub(super) struct Job {
    pub(super) raw: Bytes,
    pub(super) position: Arc<Position>,
    pub(super) lease: Arc<Lease>,
    pub(super) tx: DeliveryTx,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Chat {
    model: String,
    stream: bool,
    stream_options: Option<StreamOptions>,
    n: Option<u8>,
    messages: Vec<ToolMessage>,
    #[serde(default, deserialize_with = "present")]
    tools: Option<Vec<Tool>>,
    #[serde(default, deserialize_with = "present")]
    tool_choice: Option<ToolChoice>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StreamOptions {
    include_usage: bool,
}

enum Input {
    Text(Vec<Message>),
    Structured(ToolInvocation),
}

fn decode(bytes: &[u8], prefix: &str) -> Result<(String, Input), Diagnostic> {
    let invalid = || Diagnostic::local(Failure::InvalidChat, Stage::Decode);
    crate::inference::stream::validate_request_json(bytes).map_err(|_| invalid())?;
    let mut chat: Chat = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    if !chat.stream
        || chat.n.is_some_and(|n| n != 1)
        || chat.stream_options.is_some_and(|o| !o.include_usage)
        || chat.model.is_empty()
        || chat.model.len() > 128
        || chat.messages.is_empty()
    {
        return Err(invalid());
    }
    let structured = chat.tools.is_some()
        || chat.tool_choice.is_some()
        || chat.messages.iter().any(ToolMessage::is_structured);
    // No trimming, templating, normalization or concatenation with caller messages.
    chat.messages.insert(
        0,
        ToolMessage::System {
            content: prefix.to_owned(),
        },
    );
    let invocation =
        ToolInvocation::new(chat.messages, chat.tools, chat.tool_choice).map_err(|_| invalid())?;
    if structured {
        Ok((chat.model, Input::Structured(invocation)))
    } else {
        // Reconstruct text only after validating the same supported history grammar.
        let messages = invocation
            .messages()
            .iter()
            .map(|message| match message {
                ToolMessage::System { content } => Message {
                    role: "system".into(),
                    content: content.clone(),
                },
                ToolMessage::User { content } => Message {
                    role: "user".into(),
                    content: content.clone(),
                },
                ToolMessage::Assistant {
                    content: Some(content),
                    tool_calls: None,
                } => Message {
                    role: "assistant".into(),
                    content: content.clone(),
                },
                _ => unreachable!("validated text invocation"),
            })
            .collect();
        Ok((chat.model, Input::Text(messages)))
    }
}

pub(super) fn spawn(core: Arc<Core>, queue: &Arc<Queue>) {
    tokio::spawn(run(core, Arc::downgrade(queue), queue.ready.clone()));
}

async fn run(core: Arc<Core>, queue: Weak<Queue>, ready: Arc<Notify>) {
    let mut budget = Budget::default();
    loop {
        let Some(pending) = queue.upgrade() else {
            break;
        };
        let position = pending
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .front()
            .map(|job| job.position.clone());
        drop(pending);
        let Some(position) = position else {
            ready.notified().await;
            continue;
        };
        // Only public preflight data is worker-owned here. The removable queue
        // retains the raw body, sender and lease until the atomic dispatch point.
        let preflight = tokio::select! {
            biased;
            _ = position.cancelled.notified() => continue,
            _ = tokio::time::sleep_until(position.deadline) => {
                position.cancel();
                continue;
            }
            result = AssertUnwindSafe(async {
                if !budget.admits() {
                    return Err(Diagnostic::local(
                        if budget.blocked { Failure::BudgetUnknown } else { Failure::BudgetExhausted },
                        Stage::Admission,
                    ));
                }
                let prefix = core.info.snapshot().append_to(&core.index)?;
                Ok((prefix, core.catalog().await?))
            }).catch_unwind() => result,
        };
        let Some(pending) = queue.upgrade() else {
            break;
        };
        if preflight.is_err() {
            budget.blocked = true;
        }
        let job = pending.dispatch(&position);
        drop(pending);
        let Some(mut job) = job else {
            position.cancel();
            continue;
        };
        let result = match preflight {
            Ok(Ok((prefix, catalog))) => {
                AssertUnwindSafe(process(&core, &mut budget, &mut job, &prefix, catalog))
                    .catch_unwind()
                    .await
            }
            Ok(Err(error)) => Ok(Err(error)),
            Err(payload) => Err(payload),
        };
        let error = match result {
            Ok(result) => result.err(),
            Err(_) => {
                // The process hook suppresses payloads. Do not inspect/store/log them.
                budget.blocked = true;
                let mut error = Diagnostic::local(Failure::WorkerFailed, Stage::Worker);
                error.billing = "unknown";
                Some(error)
            }
        };
        core.info
            .spent
            .store(budget.settled, std::sync::atomic::Ordering::SeqCst);
        if let Some(error) = error {
            send_error(&mut job.tx, error);
        }
        job.tx.finish();
    }
}

fn send_error(tx: &mut DeliveryTx, error: Diagnostic) {
    if let Ok(bytes) = crate::bounded_json::to_vec(&serde_json::json!({"error":error}), 4096) {
        let mut frame = b"data: ".to_vec();
        frame.extend(bytes);
        frame.extend(b"\n\n");
        let _ = tx.try_send(&frame);
    } else {
        tx.detach();
    }
}

// These closed adapter failures occur before send. All other tokenizer failures
// are conservatively charge-uncertain: its response has no billable usage receipt.
// A successful input_tokens operation is a count, not a generated completion;
// the authenticated catalog rejects nonzero per-request pricing.
fn definitely_predispatch(failure: InferenceFailure) -> bool {
    matches!(
        failure,
        InferenceFailure::RequestEncodingFailed
            | InferenceFailure::VerificationFailed
            | InferenceFailure::ToolProfileUnqualified
    )
}

async fn process(
    core: &Core,
    budget: &mut Budget,
    job: &mut Job,
    prefix: &str,
    catalog: Catalog,
) -> Result<(), Diagnostic> {
    let (model_id, input) = decode(&job.raw, prefix)?;
    job.raw = Bytes::new();
    if matches!(input, Input::Structured(_)) && core.inference.tool_profile(&model_id).is_none() {
        return Err(Diagnostic::upstream(
            InferenceFailure::ToolProfileUnqualified,
            Stage::Decode,
            false,
        ));
    }
    if !catalog.models.iter().any(|m| m.id == model_id) {
        return Err(Diagnostic::local(Failure::UnknownModel, Stage::Catalog));
    }
    budget.blocked = true;
    let counted = match &input {
        Input::Text(messages) => {
            core.inference
                .count_tokens(&model_id, messages, job.lease.clone())
                .await
        }
        Input::Structured(invocation) => {
            core.inference
                .count_invocation_tokens(&model_id, invocation, job.lease.clone())
                .await
        }
    };
    let count = match counted {
        Ok(count) => {
            budget.blocked = false;
            count
        }
        Err(error) => {
            let failure = error.failure();
            budget.blocked = !definitely_predispatch(failure);
            return Err(Diagnostic::upstream(
                failure,
                Stage::Tokenizer,
                budget.blocked,
            ));
        }
    };
    let model = catalog
        .context_model(&model_id, count)
        .map_err(|_| Diagnostic::local(Failure::ContextExceeded, Stage::Context))?;
    let mut output = Output {
        tx: &mut job.tx,
        model: &model.id,
    };
    output.event(serde_json::json!({"choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}));
    budget.blocked = true;
    let result = match &input {
        Input::Text(messages) => {
            core.inference
                .generate_completion_stream(&model, messages, job.lease.clone(), &mut |delta| {
                    output.delta(delta)
                })
                .await
        }
        Input::Structured(invocation) => {
            core.inference
                .generate_invocation_stream(&model, invocation, job.lease.clone(), &mut |delta| {
                    output.structured_delta(delta)
                })
                .await
        }
    };
    match result {
        Ok(completion) => {
            budget.blocked = false;
            match budget.settle(&model, completion.usage) {
                Ok(cost) => {
                    output.finish(completion.finish_reason);
                    output.event(serde_json::json!({"choices":[],"usage":{
                        "prompt_tokens":completion.usage.input_tokens,
                        "completion_tokens":completion.usage.output_tokens,
                        "total_tokens":completion.usage.total_tokens
                    },"possums_free":{
                        "outcome":"settled", "upstream_cost_microusd":cost.to_string(),
                        "quoted_input_microunits_per_million_tokens":model.input_microunits_per_million_tokens.to_string(),
                        "quoted_output_microunits_per_million_tokens":model.output_microunits_per_million_tokens.to_string()
                    }}));
                    let _ = output.tx.try_send(b"data: [DONE]\n\n");
                    Ok(())
                }
                Err(()) => Err(Diagnostic::upstream(
                    InferenceFailure::SettlementFailed,
                    Stage::Settlement,
                    true,
                )),
            }
        }
        Err(error) => {
            let failure = error.failure();
            budget.blocked = !definitely_predispatch(failure);
            Err(Diagnostic::upstream(
                failure,
                Stage::Generation,
                budget.blocked,
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancellation_drops_raw_owner_and_sender_without_worker_or_tombstones() {
        use crate::stream_owner::{delivery, Limits};
        use http_body_util::BodyExt;
        use std::sync::atomic::{AtomicBool, Ordering};
        use tokio::sync::Semaphore;

        struct Raw {
            bytes: Vec<u8>,
            dropped: Arc<AtomicBool>,
        }
        impl AsRef<[u8]> for Raw {
            fn as_ref(&self) -> &[u8] {
                &self.bytes
            }
        }
        impl Drop for Raw {
            fn drop(&mut self) {
                self.dropped.store(true, Ordering::SeqCst);
            }
        }
        let queue = Arc::new(Queue::default());
        let permits = Arc::new(Semaphore::new(1));
        let lease = Arc::new(Lease::from(permits.clone().try_acquire_owned().unwrap()));
        let weak_lease = Arc::downgrade(&lease);
        let position = Arc::new(Position::new(
            Instant::now() + super::super::LIFETIME,
            Arc::downgrade(&queue),
        ));
        let dropped = Arc::new(AtomicBool::new(false));
        let (tx, mut body) = delivery(
            lease.clone(),
            Limits {
                frames: 1,
                payload_bytes: 8,
                chunk_bytes: 8,
            },
        );
        queue
            .push(Job {
                raw: Bytes::from_owner(Raw {
                    bytes: vec![b'x'; crate::server::BODY_LIMIT],
                    dropped: dropped.clone(),
                }),
                position: position.clone(),
                lease,
                tx,
            })
            .unwrap();
        position.cancel();
        assert!(dropped.load(Ordering::SeqCst));
        assert_eq!(queue.occupancy(), (0, 0));
        assert!(body.frame().await.is_none()); // sender was dropped, not merely detached from raw
        assert_eq!(permits.available_permits(), 0); // downstream still owns its lease
        drop(body);
        assert!(weak_lease.upgrade().is_none());
        assert_eq!(permits.available_permits(), 1);
        assert!(queue.dispatch(&position).is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn cancellation_and_start_have_one_atomic_winner_and_expiry_is_inclusive() {
        let deadline = Instant::now() + std::time::Duration::from_secs(1);
        let cancelled = Position::new(deadline, Weak::new());
        cancelled.cancel();
        assert!(!cancelled.start());
        let started = Position::new(deadline, Weak::new());
        assert!(started.start());
        started.cancel();
        assert!(*started.phase.lock().unwrap() == Phase::Started);
        let expired = Position::new(deadline, Weak::new());
        tokio::time::advance(std::time::Duration::from_secs(1)).await;
        assert!(!expired.start());
    }

    #[test]
    fn concurrent_cancel_start_never_revives_a_cancelled_position() {
        for _ in 0..100 {
            let queue = Arc::new(Queue::default());
            let permits = Arc::new(tokio::sync::Semaphore::new(1));
            let lease = Arc::new(Lease::from(permits.clone().try_acquire_owned().unwrap()));
            let position = Arc::new(Position::new(
                Instant::now() + std::time::Duration::from_secs(10),
                Arc::downgrade(&queue),
            ));
            let (tx, body) = crate::stream_owner::delivery(
                lease.clone(),
                crate::stream_owner::Limits {
                    frames: 1,
                    payload_bytes: 8,
                    chunk_bytes: 8,
                },
            );
            queue
                .push(Job {
                    raw: Bytes::from_static(b"private"),
                    position: position.clone(),
                    lease,
                    tx,
                })
                .unwrap();
            drop(body);
            let barrier = Arc::new(std::sync::Barrier::new(2));
            let other = position.clone();
            let sync = barrier.clone();
            let cancel = std::thread::spawn(move || {
                sync.wait();
                other.cancel();
            });
            barrier.wait();
            let job = queue.dispatch(&position);
            let started = job.is_some();
            cancel.join().unwrap();
            assert_eq!(queue.occupancy(), (0, 0));
            assert_eq!(permits.available_permits(), usize::from(!started));
            drop(job);
            assert_eq!(permits.available_permits(), 1);
            assert!(
                *position.phase.lock().unwrap()
                    == if started {
                        Phase::Started
                    } else {
                        Phase::Cancelled
                    }
            );
            assert!(!position.start());
        }
    }
}
