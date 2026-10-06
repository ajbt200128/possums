//! Shared admission and detached context preflight, independent of HTTP/rendering.
//! Adapters supply bounded, decoded owned input and a synchronous renderer handoff.
//! Acceptance is the locked reservation, not receipt of the handoff result.

use crate::{
    accounting::{Accounting, Outcome, ReserveResult},
    attestation::{EvidenceError, EvidenceVerifier, GatewayEvidence},
    auth::{AdmissionError, Auth, ConversationId},
    catalog::Model,
    generation_owner::ReservedGeneration,
    inference::{
        authenticated_catalog,
        tools::{ToolInvocation, ToolProfile},
        InferenceFailure, Message, SharedInference,
    },
};
use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{oneshot, OwnedSemaphorePermit, Semaphore};

const PREFLIGHT_DEADLINE: Duration = Duration::from_secs(30);

/// Borrowed shared services; no transport, response or renderer state.
pub(crate) struct Generation<'a> {
    pub auth: &'a Arc<Auth>,
    pub accounting: &'a Arc<Accounting>,
    pub inference: &'a SharedInference,
    pub evidence_path: &'a str,
    pub evidence_verifier: &'a Arc<dyn EvidenceVerifier>,
    pub slots: &'a Arc<Semaphore>,
    #[cfg(test)]
    pub hooks: &'a Arc<crate::web::resource_streaming_tests::PreflightHooks>,
}

// No Debug: input and admission material must never enter diagnostics.
// The adapter's bounded parser validates shape/size before constructing these;
// this core neither decodes wire input nor copies complete histories.
pub(crate) struct GenerationInput {
    pub model: String,
    pub history: Vec<Message>,
    pub prompt: String,
}

pub(crate) struct Submission<'a> {
    pub session_id: &'a str,
    pub csrf: &'a str,
    pub token: &'a str,
}

pub(crate) struct PreparedGeneration {
    pub model: Model,
    pub history: Vec<Message>,
    pub prompt: String,
    pub conversation: ConversationId,
    /// Original maximum, not the context-legal allowance or a refreshed price.
    pub reserved_microunits: u64,
}

pub(crate) struct StructuredGenerationInput {
    pub model: String,
    pub invocation: ToolInvocation,
}

pub(crate) struct PreparedStructuredGeneration {
    pub model: Model,
    pub invocation: ToolInvocation,
    pub reserved_microunits: u64,
}

enum RequestInput {
    Text(GenerationInput),
    Structured(StructuredGenerationInput),
}
impl RequestInput {
    fn model(&self) -> &str {
        match self {
            Self::Text(input) => &input.model,
            Self::Structured(input) => &input.model,
        }
    }
    fn profile(&self, inference: &SharedInference) -> Result<Option<ToolProfile>, Rejection> {
        match self {
            Self::Text(_) => Ok(None),
            Self::Structured(input) => {
                inference
                    .tool_profile(&input.model)
                    .map(Some)
                    .ok_or(Rejection::Upstream(
                        InferenceFailure::ToolProfileUnqualified,
                    ))
            }
        }
    }
}
enum Prepared {
    Text(PreparedGeneration),
    Structured(PreparedStructuredGeneration),
}

/// Fixed, content-free failures; adapters own their protocol/status mappings.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Rejection {
    Unavailable,
    Upstream(InferenceFailure),
    InvalidModel,
    Busy,
    Admission(AdmissionError),
    Duplicate(Outcome),
    Context,
}

impl Generation<'_> {
    pub(crate) async fn evidence(&self) -> Result<GatewayEvidence, EvidenceError> {
        self.evidence_verifier
            .verify(self.evidence_path, now_unix())
            .await
    }

    /// The result observes accepted work; dropping this future after admission
    /// cannot cancel preflight/inference. `handoff` MUST synchronously transfer
    /// the same owner to the renderer's detached settling worker, even when
    /// delivery/observation has disappeared. Never spawn a second accounting owner.
    pub(crate) async fn submit<T, F>(
        self,
        submission: Submission<'_>,
        input: GenerationInput,
        heavy: Arc<OwnedSemaphorePermit>,
        handoff: F,
    ) -> Result<T, Rejection>
    where
        T: Send + 'static,
        F: FnOnce(ReservedGeneration, PreparedGeneration) -> T + Send + 'static,
    {
        self.submit_core(
            submission,
            RequestInput::Text(input),
            heavy,
            move |owner, prepared| {
                let Prepared::Text(prepared) = prepared else {
                    unreachable!("text handoff")
                };
                handoff(owner, prepared)
            },
        )
        .await
    }

    pub(crate) async fn submit_structured<T, F>(
        self,
        submission: Submission<'_>,
        input: StructuredGenerationInput,
        heavy: Arc<OwnedSemaphorePermit>,
        handoff: F,
    ) -> Result<T, Rejection>
    where
        T: Send + 'static,
        F: FnOnce(ReservedGeneration, PreparedStructuredGeneration) -> T + Send + 'static,
    {
        self.submit_core(
            submission,
            RequestInput::Structured(input),
            heavy,
            move |owner, prepared| {
                let Prepared::Structured(prepared) = prepared else {
                    unreachable!("structured handoff")
                };
                handoff(owner, prepared)
            },
        )
        .await
    }

    async fn submit_core<T, F>(
        self,
        submission: Submission<'_>,
        input: RequestInput,
        heavy: Arc<OwnedSemaphorePermit>,
        handoff: F,
    ) -> Result<T, Rejection>
    where
        T: Send + 'static,
        F: FnOnce(ReservedGeneration, Prepared) -> T + Send + 'static,
    {
        let profile = input.profile(self.inference)?;
        self.evidence()
            .await
            .map_err(|_| Rejection::Upstream(InferenceFailure::VerificationFailed))?;
        let catalog = authenticated_catalog(self.inference.as_ref(), now_unix())
            .await
            .map_err(|error| Rejection::Upstream(error.failure()))?;
        let quote = catalog
            .reservation_quote(input.model())
            .map_err(|_| Rejection::InvalidModel)?;
        let permit = self
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| Rejection::Busy)?;
        let reserved_microunits = quote.reserved_microunits;
        // Qualification is independent of catalog data and must still match before
        // reservation. There are no prompt-bearing calls before this point.
        if input.profile(self.inference)? != profile {
            return Err(Rejection::Upstream(
                InferenceFailure::ToolProfileUnqualified,
            ));
        }
        // Session -> submission -> accounting locks remain inside Auth. No await
        // or prompt-bearing operation may separate a new reserve from its guard.
        let admission = self
            .auth
            .admit_submission(
                self.accounting,
                submission.session_id,
                submission.csrf,
                submission.token,
                quote,
            )
            .map_err(Rejection::Admission)?;
        if let ReserveResult::Duplicate(outcome) = admission.result {
            return Err(Rejection::Duplicate(outcome));
        }
        let owner = ReservedGeneration::new(
            self.accounting.clone(),
            admission.submission.id,
            permit,
            heavy.clone(),
        );
        let (sender, receiver) = oneshot::channel();
        // Construct BEFORE creating the future: even a pre-poll drop destroys
        // prompt/renderer captures before the sole refund owner and its leases.
        let mut job = PreflightInput {
            input,
            handoff,
            owner: Some(owner),
            sender: Some(sender),
        };
        let inference = self.inference.clone();
        #[cfg(test)]
        let hooks = self.hooks.clone();
        let work_heavy = heavy.clone();
        tokio::spawn(ChargedPreflight {
            work: Box::pin(async move {
                let result = tokio::time::timeout(PREFLIGHT_DEADLINE, async {
                    let tokens = match &mut job.input {
                        RequestInput::Text(input) => {
                            input.history.push(Message {
                                role: "user".into(),
                                content: std::mem::take(&mut input.prompt),
                            });
                            #[cfg(test)]
                            hooks
                                .resources
                                .input("preflight", &input.history, &input.prompt);
                            let tokens = inference
                                .count_tokens(&input.model, &input.history, work_heavy.clone())
                                .await
                                .map_err(|error| Rejection::Upstream(error.failure()))?;
                            input.prompt = input.history.pop().expect("preflight prompt").content;
                            tokens
                        }
                        RequestInput::Structured(input) => inference
                            .count_invocation_tokens(
                                &input.model,
                                &input.invocation,
                                work_heavy.clone(),
                            )
                            .await
                            .map_err(|error| Rejection::Upstream(error.failure()))?,
                    };
                    catalog
                        .quote(job.input.model(), tokens)
                        .map_err(|_| Rejection::Context)
                })
                .await;
                let quote = match result {
                    Ok(Ok(quote)) => quote,
                    failure => {
                        let rejection = match failure {
                            Ok(Err(error)) => error,
                            _ => Rejection::Unavailable,
                        };
                        let sender = job.sender.take().expect("preflight observer");
                        drop(job); // Refund once BEFORE reporting rejection.
                        let _ = sender.send(Err(rejection));
                        return;
                    }
                };
                #[cfg(test)]
                hooks.before_compose().await;
                let owner = job.owner.take().expect("sole reservation owner");
                let prepared = match job.input {
                    RequestInput::Text(input) => Prepared::Text(PreparedGeneration {
                        model: quote.model,
                        history: input.history,
                        prompt: input.prompt,
                        conversation: admission.submission.conversation,
                        reserved_microunits,
                    }),
                    RequestInput::Structured(input) => {
                        Prepared::Structured(PreparedStructuredGeneration {
                            model: quote.model,
                            invocation: input.invocation,
                            reserved_microunits,
                        })
                    }
                };
                // Unconditional synchronous transfer, never gated by sender state.
                let output = (job.handoff)(owner, prepared);
                #[cfg(test)]
                hooks.after_compose().await;
                // Failure discards delivery only.
                let _ = job
                    .sender
                    .take()
                    .expect("preflight observer")
                    .send(Ok(output));
            }),
            _heavy: heavy,
        });
        receiver.await.unwrap_or(Err(Rejection::Unavailable))
    }
}

// Explicit field order covers captured renderer data as well as inference input.
// Close observation AFTER refund on panic/pre-poll drop, not in unspecified
// async capture order where another worker could observe failure before cleanup.
struct PreflightInput<F, T> {
    input: RequestInput,
    handoff: F,
    owner: Option<ReservedGeneration>,
    sender: Option<oneshot::Sender<Result<T, Rejection>>>,
}

// All work/input/unclaimed output dies before returning the last heavy lease.
struct ChargedPreflight<F> {
    work: Pin<Box<F>>,
    _heavy: Arc<OwnedSemaphorePermit>,
}

impl<F: Future<Output = ()>> Future for ChargedPreflight<F> {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        self.get_mut().work.as_mut().poll(cx)
    }
}

pub(crate) fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}
