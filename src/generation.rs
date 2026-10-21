//! Shared admission and detached context preflight, independent of HTTP/rendering.
//! Adapters supply bounded, decoded owned input and a synchronous response handoff.
//! Locked admission claims an attempt. Intake must confirm a hold before prompt work.

use crate::telemetry::{
    self,
    hooks::{Lease, RequestContext},
    AdmissionModel, Disposition, GenerationTerminal, QualifiedModel,
};
use crate::{
    accounting::{Accounting, Outcome, ReserveResult},
    attestation::{EvidenceError, EvidenceVerifier, GatewayEvidence},
    auth::{AdmissionError, Auth},
    catalog::Model,
    failures::InterfaceError,
    generation_owner::ReservedGeneration,
    inference::{
        authenticated_catalog_checked,
        tools::{ToolInvocation, ToolProfile},
        InferenceFailure, Message, SharedInference,
    },
};
use std::{
    future::Future,
    pin::Pin,
    sync::{atomic::AtomicUsize, Arc},
    task::{Context, Poll},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::oneshot;

const PREFLIGHT_DEADLINE: Duration = crate::inference::stream::RESPONSE_TIMEOUT;

/// Borrowed shared services; no transport or response state.
pub(crate) struct Generation<'a> {
    pub auth: &'a Arc<Auth>,
    pub accounting: &'a Arc<Accounting>,
    pub inference: &'a SharedInference,
    pub evidence_path: &'a str,
    pub evidence_verifier: &'a Arc<dyn EvidenceVerifier>,
    pub metrics: Option<&'a Arc<telemetry::AggregateMetrics>>,
    pub observation: RequestContext,
    pub activity: &'a Arc<AtomicUsize>,
    pub disconnect: crate::stream_owner::Disconnect,
    #[cfg(test)]
    pub hooks: &'a Arc<crate::server::resource_streaming_tests::PreflightHooks>,
}

// No Debug: input and admission material must never enter diagnostics.
// The adapter's bounded parser validates shape/size before constructing these;
// this core neither decodes wire input nor copies complete histories.
pub(crate) struct GenerationInput {
    pub model: String,
    pub reasoning_options: crate::catalog::ReasoningOptions,
    pub history: Vec<Message>,
    pub prompt: String,
}

pub(crate) struct Submission<'a> {
    pub session_id: &'a str,
    pub admission_binding: &'a str,
    pub token: &'a str,
}

pub(crate) struct PreparedGeneration {
    pub model: Model,
    pub reasoning_options: crate::catalog::ReasoningOptions,
    pub history: Vec<Message>,
    pub prompt: String,
    /// Original maximum, not the context-legal allowance or a refreshed price.
    pub reserved_microunits: u64,
    pub cache_scope: crate::auth::CacheScope,
}

pub(crate) struct StructuredGenerationInput {
    pub model: String,
    pub reasoning_options: crate::catalog::ReasoningOptions,
    pub invocation: ToolInvocation,
}

pub(crate) struct PreparedStructuredGeneration {
    pub model: Model,
    pub reasoning_options: crate::catalog::ReasoningOptions,
    pub invocation: ToolInvocation,
    pub reserved_microunits: u64,
    pub cache_scope: crate::auth::CacheScope,
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
    fn reasoning_options(&self) -> &crate::catalog::ReasoningOptions {
        match self {
            Self::Text(input) => &input.reasoning_options,
            Self::Structured(input) => &input.reasoning_options,
        }
    }
    fn profile(&self, inference: &SharedInference) -> Result<Option<ToolProfile>, Rejection> {
        match self {
            Self::Text(_) => Ok(None),
            Self::Structured(input) => {
                inference
                    .tool_profile(&input.model)
                    .map(Some)
                    .ok_or_else(|| {
                        Rejection::upstream(InferenceFailure::ToolProfileUnqualified, false)
                    })
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
    Failure {
        failure: InterfaceError,
        projection: FailureProjection,
        owned: bool,
    },
    Admission(AdmissionError),
    OwnedAdmission(AdmissionError),
    Duplicate(Outcome),
    Cancelled,
}

/// Legacy adapter projection, separate from the preserved typed cause/owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FailureProjection {
    Unavailable,
    Upstream(InferenceFailure),
    Catalog(crate::inference::CatalogFailure),
    InvalidModel,
    InvalidReasoning,
    ImagesUnsupported,
    Context,
}

impl Rejection {
    fn upstream(failure: InferenceFailure, owned: bool) -> Self {
        let cause = match failure {
            InferenceFailure::InferenceUnavailable if owned => InterfaceError::PreflightUnknown,
            InferenceFailure::InferenceUnavailable => InterfaceError::CatalogAcquisitionUnknown,
            _ => failure.into(),
        };
        Self::Failure {
            failure: cause,
            projection: FailureProjection::Upstream(failure),
            owned,
        }
    }

    pub(crate) fn interface_error(&self) -> Option<InterfaceError> {
        use crate::auth::AuthError as H;
        Some(match self {
            Self::Failure { failure, .. } => *failure,
            Self::Admission(AdmissionError::Auth(error))
            | Self::OwnedAdmission(AdmissionError::Auth(error)) => match error {
                H::Invalid => InterfaceError::AuthenticationRefused,
                H::Conflict => InterfaceError::ConflictRefused,
                H::Configuration => InterfaceError::ConfigurationInvalid,
                H::StateUnavailable => InterfaceError::AuthStateUnknown,
                H::Capacity => InterfaceError::StateCapacityRefused,
                H::Intake(error) => match error.kind {
                    crate::intake::ErrorKind::Authentication
                    | crate::intake::ErrorKind::MfaUnsupported
                    | crate::intake::ErrorKind::BindingChanged => {
                        InterfaceError::AuthenticationRefused
                    }
                    _ => InterfaceError::AuthStateUnknown,
                },
            },
            Self::Admission(AdmissionError::Accounting(error))
            | Self::OwnedAdmission(AdmissionError::Accounting(error)) => error.interface_error(),
            Self::Duplicate(_) | Self::Cancelled => return None,
        })
    }

    pub(crate) fn owned_failure(&self) -> bool {
        matches!(
            self,
            Self::Failure { owned: true, .. } | Self::OwnedAdmission(_)
        )
    }
}

impl Generation<'_> {
    pub(crate) fn disconnected(mut self, signal: crate::stream_owner::Disconnect) -> Self {
        self.disconnect = signal;
        self
    }
    pub(crate) fn observed(mut self, observation: RequestContext) -> Self {
        self.observation = observation;
        self
    }
    fn reject(&self, rejection: Rejection, model: AdmissionModel) -> Rejection {
        use telemetry::Rejection as R;
        let reason = match &rejection {
            Rejection::Failure {
                projection:
                    FailureProjection::InvalidModel
                    | FailureProjection::InvalidReasoning
                    | FailureProjection::ImagesUnsupported,
                ..
            } => R::Input,
            Rejection::Admission(AdmissionError::Auth(_)) => R::Auth,
            Rejection::Admission(AdmissionError::Accounting(
                crate::accounting::AccountingError::InsufficientCredit,
            )) => R::Credit,
            Rejection::Admission(AdmissionError::Accounting(
                crate::accounting::AccountingError::Concurrency,
            )) => R::AccountLimit,
            Rejection::Failure {
                projection:
                    FailureProjection::Upstream(
                        InferenceFailure::VerificationFailed
                        | InferenceFailure::EndpointBindingFailed,
                    ),
                ..
            } => R::Verification,
            Rejection::Failure {
                projection: FailureProjection::Upstream(InferenceFailure::CatalogFailed),
                ..
            } => R::Catalog,
            Rejection::Failure {
                projection: FailureProjection::Catalog(failure),
                ..
            } => match failure.wire() {
                InferenceFailure::VerificationFailed | InferenceFailure::EndpointBindingFailed => {
                    R::Verification
                }
                InferenceFailure::CatalogFailed => R::Catalog,
                _ => R::Internal,
            },
            _ => R::Internal,
        };
        self.observation.reject(model, reason);
        rejection
    }
    pub(crate) async fn evidence(&self) -> Result<GatewayEvidence, EvidenceError> {
        self.evidence_verifier
            .verify(self.evidence_path, now_unix())
            .await
    }

    /// The result observes the sole accounting owner. Losing this observer or
    /// the downstream connection cancels prompt-bearing preflight work, not
    /// settlement. Successful preflight synchronously transfers that same owner
    /// to the API worker; never spawn a second accounting owner.
    pub(crate) async fn submit<T, F>(
        self,
        ticket: crate::lifecycle::Ticket,
        submission: Submission<'_>,
        input: GenerationInput,
        heavy: Arc<crate::telemetry::hooks::Lease>,
        handoff: F,
    ) -> Result<T, Rejection>
    where
        T: Send + 'static,
        F: FnOnce(ReservedGeneration, PreparedGeneration) -> T + Send + 'static,
    {
        self.submit_core(
            ticket,
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
        ticket: crate::lifecycle::Ticket,
        submission: Submission<'_>,
        input: StructuredGenerationInput,
        heavy: Arc<crate::telemetry::hooks::Lease>,
        handoff: F,
    ) -> Result<T, Rejection>
    where
        T: Send + 'static,
        F: FnOnce(ReservedGeneration, PreparedStructuredGeneration) -> T + Send + 'static,
    {
        self.submit_core(
            ticket,
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
        ticket: crate::lifecycle::Ticket,
        submission: Submission<'_>,
        input: RequestInput,
        heavy: Arc<crate::telemetry::hooks::Lease>,
        handoff: F,
    ) -> Result<T, Rejection>
    where
        T: Send + 'static,
        F: FnOnce(ReservedGeneration, Prepared) -> T + Send + 'static,
    {
        let profile = input
            .profile(self.inference)
            .map_err(|error| self.reject(error, AdmissionModel::Unknown))?;
        self.evidence().await.map_err(|error| {
            self.reject(
                Rejection::Failure {
                    failure: error.into(),
                    projection: FailureProjection::Upstream(InferenceFailure::VerificationFailed),
                    owned: false,
                },
                AdmissionModel::Unknown,
            )
        })?;
        let catalog = authenticated_catalog_checked(self.inference.as_ref(), now_unix())
            .await
            .map_err(|error| {
                self.reject(
                    Rejection::Failure {
                        failure: error.into(),
                        projection: FailureProjection::Catalog(error),
                        owned: false,
                    },
                    AdmissionModel::Unknown,
                )
            })?;
        let quote = catalog.reservation_quote(input.model()).map_err(|error| {
            self.reject(
                Rejection::Failure {
                    failure: error.into(),
                    projection: FailureProjection::InvalidModel,
                    owned: false,
                },
                AdmissionModel::Unknown,
            )
        })?;
        if matches!(&input, RequestInput::Structured(input) if input.invocation.has_images())
            && !crate::inference::tools::supports_images(&quote.model.id)
        {
            return Err(self.reject(
                Rejection::Failure {
                    failure: InterfaceError::InputRefused,
                    projection: FailureProjection::ImagesUnsupported,
                    owned: false,
                },
                AdmissionModel::Unknown,
            ));
        }
        if !quote.model.allows_reasoning(input.reasoning_options()) {
            return Err(self.reject(
                Rejection::Failure {
                    failure: InterfaceError::InputRefused,
                    projection: FailureProjection::InvalidReasoning,
                    owned: false,
                },
                AdmissionModel::Unknown,
            ));
        }
        let model = QualifiedModel::from_authenticated_quote(&quote);
        let admission_model = AdmissionModel::Qualified(model);
        // The caller already owns the defensive heavy-memory lease. Generation
        // observation adds no independent request-count quota.
        let permit = Lease::tracking(
            self.activity.clone(),
            self.metrics,
            telemetry::Lane::Generation,
        );
        let reserved_microunits = quote.reserved_microunits;
        // Qualification is independent of catalog data and must still match before
        // reservation. There are no prompt-bearing calls before this point.
        if input
            .profile(self.inference)
            .map_err(|error| self.reject(error, admission_model))?
            != profile
        {
            return Err(self.reject(
                Rejection::upstream(InferenceFailure::ToolProfileUnqualified, false),
                admission_model,
            ));
        }
        // Session -> submission -> accounting locks remain inside Auth. No await
        // or prompt-bearing operation may separate a new claim from its owner.
        let admission = self
            .auth
            .admit_submission(
                self.accounting,
                submission.session_id,
                submission.admission_binding,
                submission.token,
                quote,
            )
            .map_err(|error| self.reject(Rejection::Admission(error), admission_model))?;
        if let ReserveResult::Duplicate(outcome) = admission.result {
            self.observation.disposition(Disposition::Duplicate);
            return Err(Rejection::Duplicate(outcome));
        }
        let mut owner = ReservedGeneration::new(
            self.accounting.clone(),
            admission.submission.id,
            permit,
            heavy.clone(),
        )
        .disconnected(self.disconnect.clone())
        .tracked(ticket.clone())
        .observed(self.observation.generation(model), model);
        self.observation.disposition(Disposition::NewGeneration);
        let remote_ready = owner.start_remote();
        let remote = remote_ready.is_some();
        let auth = self.auth.clone();
        let session_id = submission.session_id.to_owned();
        let admission_binding = submission.admission_binding.to_owned();
        let token = submission.token.to_owned();
        let (sender, receiver) = oneshot::channel();
        // Construct BEFORE creating the future: even a pre-poll drop destroys
        // prompt/response captures before the sole refund owner and its leases.
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
        let disconnect = self.disconnect.clone();
        tokio::spawn(ChargedPreflight {
            work: Box::pin(async move {
                let result = tokio::select! {
                    biased;
                    _ = disconnect.cancelled() => Err(Rejection::Cancelled),
                    _ = job.sender.as_mut().expect("preflight observer").closed() => Err(Rejection::Cancelled),
                    result = tokio::time::timeout(PREFLIGHT_DEADLINE, async {
                    if let Some(ready) = remote_ready {
                        let expires_at = ready.await.unwrap_or(Err(crate::accounting::AccountingError::StateUnavailable))
                            .map_err(|error| Rejection::OwnedAdmission(AdmissionError::Accounting(error)))?;
                        auth.validate_admission(&session_id, &admission_binding, &token, job.input.model())
                            .await.map_err(|error| Rejection::OwnedAdmission(AdmissionError::Auth(error)))?;
                        if expires_at <= SystemTime::now() {
                            return Err(Rejection::OwnedAdmission(AdmissionError::Accounting(crate::accounting::AccountingError::HoldUnavailable)));
                        }
                    }
                    // Accepted work starts here, before prompt-bearing tokenization.
                    // Hold expiry is no longer a cancellation condition after this point.
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
                                .count_request_tokens(&input.model, (&input.history, None, &input.reasoning_options), work_heavy.clone())
                                .await
                                .map_err(|error| Rejection::upstream(error.failure(), true))?;
                            input.prompt = input.history.pop().expect("preflight prompt").content;
                            tokens
                        }
                        RequestInput::Structured(input) => inference
                            .count_request_tokens(
                                &input.model,
                                (&[], Some(&input.invocation), &input.reasoning_options),
                                work_heavy.clone(),
                            )
                            .await
                            .map_err(|error| Rejection::upstream(error.failure(), true))?,
                    };
                    let quote = catalog.quote(job.input.model(), tokens).map_err(|error| Rejection::Failure {
                        failure: error.into(), projection: FailureProjection::Context, owned: true,
                    })?;
                    // Tokenization may have yielded through logout/reset. Recheck
                    // current authority before handing off prompt-bearing inference.
                    if remote {
                        auth.validate_admission(&session_id, &admission_binding, &token, job.input.model())
                            .await.map_err(|error| Rejection::OwnedAdmission(AdmissionError::Auth(error)))?;
                    }
                    Ok(quote)
                    }) => result.unwrap_or(Err(Rejection::Failure {
                        failure: InterfaceError::PreflightDeadline,
                        projection: FailureProjection::Unavailable,
                        owned: true,
                    })),
                };
                let quote = match result {
                    Ok(quote) => quote,
                    Err(rejection) => {
                        let sender = job.sender.take().expect("preflight observer");
                        if let Some(owner) = &mut job.owner {
                            if let Some(primary) = rejection.interface_error() {
                                let terminal = match &rejection {
                                    Rejection::Failure {
                                        projection: FailureProjection::Upstream(failure),
                                        ..
                                    } => (*failure).into(),
                                    Rejection::Failure {
                                        projection: FailureProjection::Context,
                                        ..
                                    } => GenerationTerminal::Admission,
                                    _ => GenerationTerminal::Internal,
                                };
                                owner.failure(terminal, primary);
                            } else {
                                owner.expected_interruption();
                            }
                        }
                        if let Some(owner) = job.owner.take() {
                            owner.cancel().await; // Same actor; await its one terminal attempt.
                        }
                        drop(job);
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
                        reasoning_options: input.reasoning_options,
                        history: input.history,
                        prompt: input.prompt,
                        reserved_microunits,
                        cache_scope: admission.cache_scope,
                    }),
                    RequestInput::Structured(input) => {
                        Prepared::Structured(PreparedStructuredGeneration {
                            model: quote.model,
                            reasoning_options: input.reasoning_options,
                            invocation: input.invocation,
                            reserved_microunits,
                            cache_scope: admission.cache_scope,
                        })
                    }
                };
                // Synchronous owner transfer; compose observes the same cancellation.
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
            _ticket: ticket,
        });
        receiver.await.unwrap_or(Err(Rejection::Failure {
            failure: InterfaceError::PreflightUnknown,
            projection: FailureProjection::Unavailable,
            owned: true,
        }))
    }
}

// Explicit field order covers captured response data as well as inference input.
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
    _heavy: Arc<crate::telemetry::hooks::Lease>,
    _ticket: crate::lifecycle::Ticket,
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

#[cfg(test)]
#[path = "generation_tests.rs"]
mod tests;
