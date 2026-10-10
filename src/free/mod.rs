//! Isolated free API. Paid authentication, accounting and routes are not constructed.
mod auth;
mod budget;
mod info;
mod queue;
#[cfg(test)]
mod tests;
mod transport;

pub use auth::{KeyConfigurationError, SharedKeys};
pub use info::build_identity_available;
pub use transport::{router, serve};

use crate::{
    attestation::{EvidenceVerifier, GatewayEvidence},
    catalog::Catalog,
    inference::{authenticated_catalog, InferenceFailure, SharedInference},
};
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use std::{sync::Arc, time::Duration};
use tokio::sync::Semaphore;

pub const INDEX: &str = include_str!("../../index.html");
pub(crate) const CONNECTIONS: usize = 64;
pub(crate) const LIFETIME: Duration = Duration::from_secs(12 * 60 * 60);

#[derive(Clone)]
pub struct AppState {
    keys: Arc<SharedKeys>,
    core: Arc<Core>,
    queue: Arc<queue::Queue>,
    connections: Arc<Semaphore>,
}

struct Core {
    inference: SharedInference,
    evidence_path: Arc<str>,
    verifier: Arc<dyn EvidenceVerifier>,
    index: Arc<str>,
    info: Arc<info::RuntimeInfo>,
}

impl AppState {
    /// Starts one worker and one process-local budget. Construct once per process run.
    pub fn new(
        keys: SharedKeys,
        inference: SharedInference,
        evidence_path: impl Into<Arc<str>>,
        verifier: Arc<dyn EvidenceVerifier>,
    ) -> Self {
        Self::with_index(
            keys,
            inference,
            evidence_path.into(),
            verifier,
            Arc::from(INDEX),
        )
    }

    fn with_index(
        keys: SharedKeys,
        inference: SharedInference,
        evidence_path: Arc<str>,
        verifier: Arc<dyn EvidenceVerifier>,
        index: Arc<str>,
    ) -> Self {
        let core = Arc::new(Core {
            inference,
            evidence_path,
            verifier,
            index,
            info: Arc::default(),
        });
        let queue = Arc::new(queue::Queue::default());
        queue::spawn(core.clone(), &queue);
        Self {
            keys: Arc::new(keys),
            core,
            queue,
            connections: Arc::new(Semaphore::new(CONNECTIONS)),
        }
    }
}

impl Core {
    async fn evidence(&self) -> Result<GatewayEvidence, Diagnostic> {
        tokio::time::timeout(
            Duration::from_secs(45),
            self.verifier
                .verify(&self.evidence_path, crate::generation::now_unix()),
        )
        .await
        .map_err(|_| {
            Diagnostic::upstream(
                InferenceFailure::VerificationFailed,
                Stage::Verification,
                false,
            )
        })?
        .map_err(|_| {
            Diagnostic::upstream(
                InferenceFailure::VerificationFailed,
                Stage::Verification,
                false,
            )
        })
    }

    async fn catalog(&self) -> Result<Catalog, Diagnostic> {
        self.evidence().await?;
        tokio::time::timeout(
            Duration::from_secs(30),
            authenticated_catalog(self.inference.as_ref(), crate::generation::now_unix()),
        )
        .await
        .map_err(|_| Diagnostic::upstream(InferenceFailure::CatalogFailed, Stage::Catalog, false))?
        .map_err(|e| Diagnostic::upstream(e.failure(), Stage::Catalog, false))
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum Stage {
    Admission,
    Authentication,
    Decode,
    Verification,
    Catalog,
    Tokenizer,
    Context,
    Generation,
    Settlement,
    Worker,
}

#[derive(Clone, Copy, Serialize, thiserror::Error, Debug)]
#[serde(rename_all = "snake_case")]
enum Failure {
    #[error("Supply exactly one valid shared Bearer key.")]
    Unauthorized,
    #[error("Use the documented method and path; queries, cookies and content encodings are not accepted.")]
    InvalidRequest,
    #[error("Submit a streaming JSON chat request with supported fields and complete history.")]
    InvalidChat,
    #[error("Use exactly one application/json content type.")]
    ContentType,
    #[error("Request headers exceed the 32 KiB or 64-header bound.")]
    HeadersTooLarge,
    #[error("The body exceeds its transport bound or could not be read. Chat permits 8 MiB; control requests require an empty body.")]
    BodyTooLarge,
    #[error("Request upload exceeded its deadline.")]
    UploadTimeout,
    #[error("The connection or queue is at capacity; no inference was started.")]
    Capacity,
    #[error("This endpoint is not available.")]
    NotFound,
    #[error("The shared process-run budget is exhausted; operator intervention is required.")]
    BudgetExhausted,
    #[error("An upstream charge is uncertain; further inference is blocked pending operator intervention.")]
    BudgetUnknown,
    #[error("Select a model from the authenticated model catalog.")]
    UnknownModel,
    #[error("The conversation leaves no output space in the model context window.")]
    ContextExceeded,
    #[error("The worker failed; upstream billing is unknown and further inference is blocked.")]
    WorkerFailed,
}

#[derive(Serialize)]
#[serde(untagged)]
enum Detail {
    Local(Failure),
    Upstream(InferenceFailure),
}

#[derive(Serialize)]
struct Diagnostic {
    code: &'static str,
    detail: Detail,
    stage: Stage,
    message: String,
    billing: &'static str,
    replayed: bool,
}

impl Diagnostic {
    fn local(failure: Failure, stage: Stage) -> Self {
        Self {
            code: "free_request_failed",
            detail: Detail::Local(failure),
            stage,
            message: failure.to_string(),
            billing: "generation_not_started",
            replayed: false,
        }
    }
    fn upstream(failure: InferenceFailure, stage: Stage, uncertain: bool) -> Self {
        Self {
            code: "free_request_failed",
            detail: Detail::Upstream(failure),
            stage,
            message: if uncertain {
                format!(
                    "{failure} Not replayed. A new submission may incur another upstream charge."
                )
            } else {
                failure.to_string()
            },
            billing: if uncertain {
                "unknown"
            } else {
                "generation_not_started"
            },
            replayed: false,
        }
    }
    fn response(self, status: StatusCode) -> Response {
        (status, axum::Json(serde_json::json!({"error":self}))).into_response()
    }
}
