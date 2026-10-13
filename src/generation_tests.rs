use super::*;
use crate::{
    inference::{Inference, InferenceError},
    telemetry::{AggregateMetrics, Deployment, Endpoint, SystemClock},
};
use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::sync::atomic::Ordering;
use tokio::sync::Notify;

#[tokio::test]
async fn catalog_source_reaches_preflight_projection_before_prompt_or_reservation() {
    use crate::inference::CatalogFailure as C;
    for source in [
        C::SendUnknown,
        C::SendDeadline,
        C::HttpRefused(403),
        C::BodyUnknown,
        C::BodyDeadline,
        C::BodyLimit,
        C::Validation(crate::catalog::CatalogError::Invalid),
        C::Legacy(InferenceFailure::UpstreamResponseInvalid),
    ] {
        let fixture = Fixture::new(EvidenceMode::Good, Tokens::Count(1), 20);
        *fixture.provider.catalog_failure.lock().unwrap() = Some(source);
        let failure = fixture.submit("m").await.unwrap_err();
        assert!(
            matches!(failure, Rejection::Failure { projection: FailureProjection::Catalog(actual), .. } if actual == source)
        );
        assert_eq!(failure.interface_error(), Some(source.into()));
        assert!(!failure.owned_failure());
        assert_eq!(fixture.provider.calls.load(Ordering::SeqCst), 0);
        assert_eq!(fixture.accounting.snapshot("a").unwrap().in_flight, 0);
        assert_eq!(fixture.accounting.available("a"), Some(1000));
    }
}

#[derive(Clone, Copy)]
enum EvidenceMode {
    Good,
    Unavailable,
    Invalid,
}
#[derive(Clone, Copy)]
enum Tokens {
    Count(u64),
    Failure(InferenceFailure),
    Panic,
    Pending,
}
struct Provider {
    evidence: EvidenceMode,
    tokens: Tokens,
    context: u64,
    catalog_failure: std::sync::Mutex<Option<crate::inference::CatalogFailure>>,
    calls: AtomicUsize,
    started: Notify,
}
#[async_trait]
impl EvidenceVerifier for Provider {
    async fn verify(&self, _: &str, now: u64) -> Result<GatewayEvidence, EvidenceError> {
        match self.evidence {
            EvidenceMode::Unavailable => Err(EvidenceError::Unavailable),
            EvidenceMode::Invalid => Err(EvidenceError::Invalid),
            EvidenceMode::Good => Ok(GatewayEvidence {
                quote: json!({}),
                issued_at_unix: now,
                release_digest: String::new(),
                endpoint_key_sha256: String::new(),
                freshness_expires_at_unix: now + 60,
            }),
        }
    }
}
#[async_trait]
impl Inference for Provider {
    async fn catalog(&self) -> Result<Vec<u8>, InferenceError> {
        Ok(serde_json::to_vec(&json!({"object":"list","data":[{"id":"m","type":"chat","context_window":self.context,"endpoints":["/v1/chat/completions"],"pricing":{"inputTokenPricePer1M":1,"outputTokenPricePer1M":1,"requestPrice":0}}]})).unwrap())
    }
    async fn catalog_checked(&self) -> Result<Vec<u8>, crate::inference::CatalogFailure> {
        let failure = *self.catalog_failure.lock().unwrap();
        match failure {
            Some(failure) => Err(failure),
            None => self.catalog().await.map_err(Into::into),
        }
    }
    async fn count_tokens(
        &self,
        _: &str,
        _: &[Message],
        _: Arc<Lease>,
    ) -> Result<u64, InferenceError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.started.notify_one();
        match self.tokens {
            Tokens::Count(count) => Ok(count),
            Tokens::Failure(failure) => Err(failure.into()),
            Tokens::Panic => panic!("synthetic preflight worker loss"),
            Tokens::Pending => std::future::pending().await,
        }
    }
    fn verification_document(&self) -> Result<serde_json::Value, InferenceError> {
        Err(InferenceError::Unavailable)
    }
}
struct Fixture {
    auth: Arc<Auth>,
    accounting: Arc<Accounting>,
    provider: Arc<Provider>,
    inference: SharedInference,
    evidence: Arc<dyn EvidenceVerifier>,
    metrics: Arc<AggregateMetrics>,
    activity: Arc<AtomicUsize>,
    lifecycle: crate::lifecycle::Lifecycle,
    hooks: Arc<crate::server::resource_streaming_tests::PreflightHooks>,
    session: String,
    binding: String,
    token: String,
    disconnect: crate::stream_owner::Disconnect,
}
impl Fixture {
    fn new(evidence: EvidenceMode, tokens: Tokens, context: u64) -> Self {
        let credential = URL_SAFE_NO_PAD.encode([8; 32]);
        let auth = Arc::new(Auth::from_json(&json!([{"id":"a","credential_sha256":URL_SAFE_NO_PAD.encode(Sha256::digest(credential.as_bytes())),"demo_microunits":1000}]).to_string()).unwrap());
        let (session, details) = auth
            .authenticate_api(&credential, &auth.issue_api_challenge().unwrap())
            .unwrap();
        let token = auth.issue_api_submission(&session, "m", false).unwrap();
        let provider = Arc::new(Provider {
            evidence,
            tokens,
            context,
            catalog_failure: std::sync::Mutex::new(None),
            calls: AtomicUsize::new(0),
            started: Notify::new(),
        });
        let metrics = Arc::new(AggregateMetrics::new(
            Deployment::IsolatedSynthetic,
            SystemClock::default(),
        ));
        metrics.prime_synthetic_application_window();
        Self {
            accounting: Arc::new(Accounting::new([("a".into(), 1000)])),
            auth,
            inference: provider.clone(),
            evidence: provider.clone(),
            provider,
            metrics,
            activity: Arc::new(AtomicUsize::new(0)),
            lifecycle: Default::default(),
            hooks: Default::default(),
            session,
            binding: details.admission_binding,
            token,
            disconnect: Default::default(),
        }
    }
    async fn submit(&self, model: &str) -> Result<(), Rejection> {
        let (_http, observation) =
            crate::telemetry::hooks::HttpObservation::new(Some(&self.metrics), Endpoint::ChatApi);
        Generation {
            auth: &self.auth,
            accounting: &self.accounting,
            inference: &self.inference,
            evidence_path: "synthetic-unused",
            evidence_verifier: &self.evidence,
            metrics: Some(&self.metrics),
            observation,
            activity: &self.activity,
            disconnect: self.disconnect.clone(),
            hooks: &self.hooks,
        }
        .submit(
            self.lifecycle.try_admit().unwrap(),
            Submission {
                session_id: &self.session,
                admission_binding: &self.binding,
                token: &self.token,
            },
            GenerationInput {
                model: model.into(),
                history: Vec::new(),
                prompt: "synthetic input".into(),
                reasoning_options: Default::default(),
            },
            Arc::new(Lease::tracking(
                Arc::new(AtomicUsize::new(0)),
                None,
                telemetry::Lane::Heavy,
            )),
            |_, _| panic!("unexpected successful handoff in rejection fixture"),
        )
        .await
    }
    fn refunded(&self) {
        assert_eq!(self.accounting.available("a"), Some(1000));
        assert_eq!(self.activity.load(Ordering::SeqCst), 0);
    }
}
async fn bounded<T>(future: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(5), future)
        .await
        .unwrap()
}

#[tokio::test]
async fn evidence_failures_preserve_source_before_reservation_and_wire_projection() {
    for (mode, expected) in [
        (
            EvidenceMode::Unavailable,
            InterfaceError::EvidenceUnavailable,
        ),
        (EvidenceMode::Invalid, InterfaceError::EvidenceInvalid),
    ] {
        let f = Fixture::new(mode, Tokens::Count(2), 20);
        let rejection = bounded(f.submit("m")).await.unwrap_err();
        assert_eq!(rejection.interface_error(), Some(expected));
        assert!(!rejection.owned_failure());
        assert!(matches!(
            rejection,
            Rejection::Failure {
                projection: FailureProjection::Upstream(InferenceFailure::VerificationFailed),
                ..
            }
        ));
        assert_eq!(f.provider.calls.load(Ordering::SeqCst), 0);
        assert_eq!(f.metrics.interface_error_count(expected), None); // route owns pre-reservation failure
        f.refunded();
    }
}

#[tokio::test]
async fn quote_selection_and_overflow_are_typed_before_reservation() {
    for (context, model, expected) in [
        (20, "unknown", InterfaceError::ModelUnavailable),
        (u64::MAX, "m", InterfaceError::QuoteOverflow),
    ] {
        let f = Fixture::new(EvidenceMode::Good, Tokens::Count(2), context);
        let rejection = bounded(f.submit(model)).await.unwrap_err();
        assert_eq!(rejection.interface_error(), Some(expected));
        assert!(!rejection.owned_failure());
        assert!(matches!(
            rejection,
            Rejection::Failure {
                projection: FailureProjection::InvalidModel,
                ..
            }
        ));
        assert_eq!(f.provider.calls.load(Ordering::SeqCst), 0);
        assert_eq!(f.metrics.interface_error_count(expected), None);
        f.refunded();
    }
}

#[tokio::test]
async fn owned_tokenizer_and_context_failures_are_observed_before_rejection_not_recounted() {
    for (tokens, expected) in [
        (
            Tokens::Failure(InferenceFailure::RequestEncodingFailed),
            InterfaceError::RequestEncodingFailed,
        ),
        (
            Tokens::Failure(InferenceFailure::TokenizerSendFailed),
            InterfaceError::TokenizerTransportFailed,
        ),
        (
            Tokens::Failure(InferenceFailure::TokenizerHttpFailed),
            InterfaceError::TokenizerHttpRefused,
        ),
        (
            Tokens::Failure(InferenceFailure::TokenizerResponseInvalid),
            InterfaceError::TokenizerResponseInvalid,
        ),
        (
            Tokens::Failure(InferenceFailure::TokenizerUploadIncomplete),
            InterfaceError::TokenizerUploadIncomplete,
        ),
        (Tokens::Count(20), InterfaceError::ContextRefused),
    ] {
        let f = Fixture::new(EvidenceMode::Good, tokens, 20);
        let rejection = bounded(f.submit("m")).await.unwrap_err();
        assert_eq!(rejection.interface_error(), Some(expected));
        assert!(rejection.owned_failure());
        assert_eq!(f.provider.calls.load(Ordering::SeqCst), 1);
        assert_eq!(f.metrics.interface_error_count(expected), Some(1));
        assert_eq!(
            f.metrics
                .interface_error_count(InterfaceError::PreflightUnknown),
            None
        );
        f.refunded();
    }
}

#[tokio::test(start_paused = true)]
async fn preflight_deadline_has_explicit_type_and_refunds_once() {
    let f = Fixture::new(EvidenceMode::Good, Tokens::Pending, 20);
    let rejection = f.submit("m").await.unwrap_err();
    assert_eq!(
        rejection.interface_error(),
        Some(InterfaceError::PreflightDeadline)
    );
    assert!(rejection.owned_failure());
    assert_eq!(
        f.metrics
            .interface_error_count(InterfaceError::PreflightDeadline),
        Some(1)
    );
    assert_eq!(
        f.metrics
            .interface_error_count(InterfaceError::PreflightUnknown),
        None
    );
    f.refunded();
}

#[tokio::test]
async fn lost_preflight_worker_is_unknown_not_guessed_panic_at_the_boundary() {
    let f = Fixture::new(EvidenceMode::Good, Tokens::Panic, 20);
    let rejection = bounded(f.submit("m")).await.unwrap_err();
    assert_eq!(
        rejection.interface_error(),
        Some(InterfaceError::PreflightUnknown)
    );
    assert!(rejection.owned_failure());
    assert_eq!(
        f.metrics
            .interface_error_count(InterfaceError::PreflightUnknown),
        Some(1)
    );
    f.refunded();
}

#[tokio::test]
async fn observed_disconnect_and_closed_preflight_observer_are_expected_lifecycle() {
    for observer_closed in [false, true] {
        let f = Arc::new(Fixture::new(EvidenceMode::Good, Tokens::Pending, 20));
        let work_fixture = f.clone();
        let work = tokio::spawn(async move { work_fixture.submit("m").await });
        bounded(f.provider.started.notified()).await;
        if observer_closed {
            work.abort();
            assert!(bounded(work).await.unwrap_err().is_cancelled());
        } else {
            f.disconnect.cancel();
            assert_eq!(bounded(work).await.unwrap(), Err(Rejection::Cancelled));
        }
        f.lifecycle.quiesce();
        bounded(f.lifecycle.wait_drained()).await.unwrap();
        assert!(InterfaceError::ALL
            .into_iter()
            .all(|error| f.metrics.interface_error_count(error).is_none()));
        f.refunded();
    }
}

#[test]
fn duplicate_and_cancellation_have_no_fault_class() {
    for rejection in [
        Rejection::Duplicate(Outcome::InFlight),
        Rejection::Duplicate(Outcome::Refunded),
        Rejection::Duplicate(Outcome::Settled { charged: 0 }),
        Rejection::Cancelled,
    ] {
        assert_eq!(rejection.interface_error(), None);
        assert!(!rejection.owned_failure());
    }
}
