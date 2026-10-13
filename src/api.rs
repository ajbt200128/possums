//! Cookie-independent, streaming-only Phase 0.1 API.
use crate::{
    accounting::{AccountingError, Outcome},
    auth::{AdmissionError, AuthError, Session},
    bounded_json,
    catalog::{Catalog, MAX_CATALOG_BYTES},
    failures::InterfaceError,
    generation::{
        now_unix, FailureProjection, GenerationInput, Rejection, StructuredGenerationInput,
        Submission,
    },
    inference::{
        authenticated_catalog_checked,
        tools::{Tool, ToolChoice, ToolInvocation, ToolMessage},
        CatalogFailure, InferenceFailure,
    },
    server::AppState,
    telemetry::{hooks::RequestContext, AdmissionModel, Rejection as MetricRejection},
};
use axum::{
    body::Bytes,
    extract::{Extension, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::sync::Arc;

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/auth/challenge", get(challenge))
        .route("/v1/sessions", post(session))
        .route("/v1/submissions", post(submission))
        .route("/v1/sessions/current", delete(logout))
        .route("/v1/models", get(models))
        .route("/v1/balance", post(balance))
        .route("/v1/chat/completions", post(chat))
}

pub(crate) fn is_api(path: &str) -> bool {
    path == "/v1" || path.starts_with("/v1/")
}

// Fixed codes only: no parser, provider, input, or authentication diagnostics.
#[derive(Clone)]
pub(crate) struct SafeError;

/// The original operation owner handles failure observation, including when
/// telemetry is off/unavailable. HTTP propagation must not count it again.
#[derive(Clone)]
pub(crate) struct FailureOwned;

#[derive(Clone, Copy, Debug)]
struct ApiFailure {
    status: StatusCode,
    rejection: MetricRejection,
    cause: InterfaceError,
}
impl ApiFailure {
    fn input(status: StatusCode) -> Self {
        Self {
            status,
            rejection: MetricRejection::Input,
            cause: InterfaceError::InputRefused,
        }
    }
    fn respond(self, observation: &RequestContext) -> Response {
        observation.reject(AdmissionModel::Unknown, self.rejection);
        failure(self.status, self.cause)
    }
}

/// Keep existing code/detail vocabulary and use only the already-supported
/// message field; older strict shipped decoders reject added metadata fields.
/// Classification and observation remain independent of telemetry availability.
pub(crate) fn failure(status: StatusCode, cause: InterfaceError) -> Response {
    let code = match status {
        StatusCode::BAD_REQUEST => "invalid_request",
        StatusCode::UNAUTHORIZED => "unauthorized",
        StatusCode::NOT_FOUND => "not_found",
        StatusCode::METHOD_NOT_ALLOWED => "method_not_allowed",
        StatusCode::REQUEST_TIMEOUT => "request_timeout",
        StatusCode::CONFLICT => "conversation_conflict",
        StatusCode::PAYMENT_REQUIRED => "insufficient_credit",
        StatusCode::TOO_MANY_REQUESTS => "account_limit",
        StatusCode::PAYLOAD_TOO_LARGE => "body_too_large",
        StatusCode::UNSUPPORTED_MEDIA_TYPE => "unsupported_media_type",
        StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE => "headers_too_large",
        _ => "unavailable",
    };
    let mut response = (
        status,
        axum::Json(serde_json::json!({"error": {"code": code, "message": cause.message()}})),
    )
        .into_response();
    response.extensions_mut().insert(SafeError);
    response.extensions_mut().insert(cause);
    response
}

pub(crate) fn interrupted() -> Response {
    let mut response = (
        StatusCode::SERVICE_UNAVAILABLE,
        axum::Json(serde_json::json!({"error": {"code": "unavailable", "message": crate::server::REQUEST_INTERRUPTED_MESSAGE}})),
    ).into_response();
    response.extensions_mut().insert(SafeError);
    response.extensions_mut().insert(FailureOwned);
    response
}

pub(crate) fn quiescing() -> Response {
    let mut response = (
        StatusCode::SERVICE_UNAVAILABLE,
        axum::Json(serde_json::json!({"error": {
            "code": "service_quiescing",
            "stage": "admission",
            "constraint": "service_quiescing",
            "billing": "not_submitted",
            "message": "Service is shutting down; this request was not submitted and no prompt was sent. Submit deliberately after service is available."
        }})),
    ).into_response();
    response.extensions_mut().insert(SafeError);
    response
}

fn reject(observation: &RequestContext, status: StatusCode, reason: MetricRejection) -> Response {
    ApiFailure {
        status,
        rejection: reason,
        cause: match reason {
            MetricRejection::Input => InterfaceError::InputRefused,
            MetricRejection::Auth => InterfaceError::AuthenticationRefused,
            MetricRejection::Credit => InterfaceError::CreditRefused,
            MetricRejection::AccountLimit => InterfaceError::AccountLimitRefused,
            MetricRejection::Verification => InterfaceError::VerificationFailed,
            MetricRejection::Catalog => InterfaceError::CatalogFailed,
            _ => InterfaceError::RequestDispatchUnknown,
        },
    }
    .respond(observation)
}

#[cfg(test)]
#[tokio::test]
async fn unsupported_reasoning_keeps_main_wire_and_unowned_typed_refusal() {
    use http_body_util::BodyExt as _;
    let response = rejection_response(Rejection::Failure {
        failure: InterfaceError::InputRefused,
        projection: FailureProjection::InvalidReasoning,
        owned: false,
    });
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        response.extensions().get::<InterfaceError>(),
        Some(&InterfaceError::InputRefused)
    );
    assert!(response.extensions().get::<FailureOwned>().is_none());
    assert!(response.extensions().get::<SafeError>().is_some());
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        value,
        serde_json::json!({"error": {
            "code": "invalid_request", "detail": "reasoning_parameters_unsupported",
            "message": "Stage: request; constraint: authenticated_catalog_configuration. Reasoning controls are unsupported. Omit controls or refresh model metadata; this attempt sent no upstream prompt."
        }})
    );
}

fn rejection_response(rejection: Rejection) -> Response {
    let cause = rejection.interface_error();
    let owned = rejection.owned_failure();
    let mut response = match rejection {
        Rejection::Failure {
            projection,
            failure: cause,
            ..
        } => match projection {
            FailureProjection::Upstream(wire) => inference_error_with_cause(wire, Some(cause)),
            FailureProjection::Catalog(source) => {
                inference_error_message(source.wire(), Some(cause), source.message())
            }
            FailureProjection::Unavailable => {
                inference_error_with_cause(InferenceFailure::InferenceUnavailable, Some(cause))
            }
            FailureProjection::InvalidReasoning => {
                let mut response = (
                    StatusCode::BAD_REQUEST,
                    axum::Json(serde_json::json!({"error": {
                        "code": "invalid_request", "detail": "reasoning_parameters_unsupported",
                        "message": "Stage: request; constraint: authenticated_catalog_configuration. Reasoning controls are unsupported. Omit controls or refresh model metadata; this attempt sent no upstream prompt."
                    }})),
                )
                    .into_response();
                response.extensions_mut().insert(SafeError);
                response
            }
            FailureProjection::InvalidModel | FailureProjection::Context => {
                failure(StatusCode::BAD_REQUEST, cause)
            }
        },
        Rejection::Admission(AdmissionError::Auth(auth_failure)) => failure(
            match auth_failure {
                AuthError::Invalid | AuthError::Conflict => StatusCode::BAD_REQUEST,
                AuthError::Configuration | AuthError::StateUnavailable | AuthError::Capacity => {
                    StatusCode::SERVICE_UNAVAILABLE
                }
            },
            cause.expect("admission has a typed cause"),
        ),
        Rejection::Admission(AdmissionError::Accounting(accounting_failure)) => failure(
            match accounting_failure {
                AccountingError::InsufficientCredit => StatusCode::PAYMENT_REQUIRED,
                AccountingError::Concurrency => StatusCode::TOO_MANY_REQUESTS,
                AccountingError::UnknownAccount
                | AccountingError::AlteredDuplicate
                | AccountingError::Capacity
                | AccountingError::InvalidTransition
                | AccountingError::StateUnavailable
                | AccountingError::Cost => StatusCode::SERVICE_UNAVAILABLE,
            },
            cause.expect("admission has a typed cause"),
        ),
        Rejection::Cancelled => {
            inference_error_with_cause(InferenceFailure::InferenceUnavailable, None)
        }
        Rejection::Duplicate(outcome) => {
            let outcome = match outcome {
                Outcome::InFlight => "in_flight",
                Outcome::Settled { .. } => "settled",
                Outcome::Refunded => "refunded",
            };
            let mut response = (
                StatusCode::CONFLICT,
                axum::Json(
                    serde_json::json!({"error":{"code":"duplicate_request","outcome":outcome}}),
                ),
            )
                .into_response();
            response.extensions_mut().insert(SafeError);
            response
        }
    };
    if let Some(cause) = cause {
        response.extensions_mut().insert(cause);
    } else {
        response.extensions_mut().remove::<InterfaceError>();
    }
    if owned || cause.is_none() {
        response.extensions_mut().insert(FailureOwned);
    }
    response
}

struct ControlFailure {
    wire: InferenceFailure,
    cause: InterfaceError,
    catalog: Option<CatalogFailure>,
}
fn control_inference_error(observation: &RequestContext, failure: ControlFailure) -> Response {
    let reason = match failure.wire {
        InferenceFailure::VerificationFailed | InferenceFailure::EndpointBindingFailed => {
            MetricRejection::Verification
        }
        InferenceFailure::CatalogFailed => MetricRejection::Catalog,
        _ => MetricRejection::Internal,
    };
    observation.reject(AdmissionModel::Unknown, reason);
    match failure.catalog {
        Some(source) => {
            inference_error_message(failure.wire, Some(failure.cause), source.message())
        }
        None => inference_error_with_cause(failure.wire, Some(failure.cause)),
    }
}

fn inference_error_with_cause(
    failure: InferenceFailure,
    cause: Option<InterfaceError>,
) -> Response {
    let message = cause.map(|cause| format!("{failure} {}", cause.message())).unwrap_or_else(||
        "Preflight interrupted after an observed disconnect or observer removal. Not replayed. Billing/outcome is unknown without an authenticated receipt; a new submission may incur another charge.".to_owned());
    inference_error_message(failure, cause, message)
}

fn inference_error_message(
    failure: InferenceFailure,
    cause: Option<InterfaceError>,
    message: String,
) -> Response {
    let mut response = (
        StatusCode::SERVICE_UNAVAILABLE,
        axum::Json(serde_json::json!({"error": {
            "code": "unavailable", "detail": failure,
            "message": message, "billing": "unknown"
        }})),
    )
        .into_response();
    response.extensions_mut().insert(SafeError);
    if let Some(cause) = cause {
        response.extensions_mut().insert(cause);
    }
    response
}

fn auth_error(observation: &RequestContext, error: AuthError) -> Response {
    auth_rejection(error).respond(observation)
}

// A poisoned session lock is unavailable authentication state (cause unknown),
// never evidence of bad credentials; only a missing/expired session is Invalid.
fn auth_rejection(error: AuthError) -> ApiFailure {
    let (status, rejection, cause) = match error {
        AuthError::Invalid => (
            StatusCode::UNAUTHORIZED,
            MetricRejection::Auth,
            InterfaceError::AuthenticationRefused,
        ),
        AuthError::Conflict => (
            StatusCode::CONFLICT,
            MetricRejection::Auth,
            InterfaceError::ConflictRefused,
        ),
        AuthError::Configuration => (
            StatusCode::SERVICE_UNAVAILABLE,
            MetricRejection::Internal,
            InterfaceError::ConfigurationInvalid,
        ),
        AuthError::StateUnavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            MetricRejection::Internal,
            InterfaceError::AuthStateUnknown,
        ),
        AuthError::Capacity => (
            StatusCode::SERVICE_UNAVAILABLE,
            MetricRejection::Internal,
            InterfaceError::StateCapacityRefused,
        ),
    };
    ApiFailure {
        status,
        rejection,
        cause,
    }
}

fn json(observation: &RequestContext, value: &impl Serialize, cap: usize) -> Response {
    match bounded_json::to_vec(value, cap) {
        Ok(bytes) => ([(header::CONTENT_TYPE, "application/json")], bytes).into_response(),
        Err(error) => ApiFailure {
            status: StatusCode::SERVICE_UNAVAILABLE,
            rejection: MetricRejection::Internal,
            cause: match error {
                bounded_json::EncodingFailure::LimitExceeded => {
                    InterfaceError::ResponseEncodingLimit
                }
                bounded_json::EncodingFailure::CauseUnknown => {
                    InterfaceError::ResponseEncodingUnknown
                }
            },
        }
        .respond(observation),
    }
}

fn token(value: &str) -> bool {
    value.len() == 43
        && URL_SAFE_NO_PAD
            .decode(value)
            .is_ok_and(|bytes| bytes.len() == 32)
}

fn no_cookie_or_encoding(headers: &HeaderMap) -> Result<(), ApiFailure> {
    if headers.contains_key(header::COOKIE) || headers.contains_key(header::CONTENT_ENCODING) {
        return Err(ApiFailure::input(StatusCode::BAD_REQUEST));
    }
    Ok(())
}

fn anonymous(headers: &HeaderMap) -> Result<(), ApiFailure> {
    no_cookie_or_encoding(headers)?;
    if headers.contains_key(header::AUTHORIZATION) {
        return Err(ApiFailure::input(StatusCode::BAD_REQUEST));
    }
    Ok(())
}

// Exact one-header grammar, no comma folding, alternate scheme, whitespace or
// credential fallback. Only authenticated API sessions are usable here.
fn authenticated(state: &AppState, headers: &HeaderMap) -> Result<(String, Session), ApiFailure> {
    no_cookie_or_encoding(headers)?;
    let mut values = headers.get_all(header::AUTHORIZATION).iter();
    let value = values.next().and_then(|value| value.to_str().ok());
    let id = value.and_then(|value| value.strip_prefix("Bearer "));
    let Some(id) = id
        .filter(|id| token(id))
        .filter(|_| values.next().is_none())
    else {
        return Err(auth_rejection(AuthError::Invalid));
    };
    let session = state.auth.checked_api_session(id).map_err(auth_rejection)?;
    Ok((id.to_owned(), session))
}

fn empty(headers: &HeaderMap, bytes: &Bytes) -> Result<(), ApiFailure> {
    if !bytes.is_empty() || headers.contains_key(header::CONTENT_TYPE) {
        return Err(ApiFailure::input(StatusCode::BAD_REQUEST));
    }
    Ok(())
}

fn parse<T: DeserializeOwned>(headers: &HeaderMap, bytes: &Bytes) -> Result<T, ApiFailure> {
    let mut types = headers.get_all(header::CONTENT_TYPE).iter();
    if types.next().is_none_or(|value| value != "application/json") || types.next().is_some() {
        return Err(ApiFailure::input(StatusCode::UNSUPPORTED_MEDIA_TYPE));
    }
    if bytes.iter().find(|byte| !byte.is_ascii_whitespace()) != Some(&b'{') {
        return Err(ApiFailure::input(StatusCode::BAD_REQUEST));
    }
    // Middleware has already bounded collection under its deadline and retained
    // request lease. Typed structs reject unknown fields; the guard also rejects
    // duplicate keys inside opaque tool schemas before Value can discard them.
    crate::inference::stream::validate_request_json(bytes)
        .map_err(|_| ApiFailure::input(StatusCode::BAD_REQUEST))?;
    serde_json::from_slice(bytes).map_err(|_| ApiFailure::input(StatusCode::BAD_REQUEST))
}

async fn challenge(
    State(state): State<AppState>,
    Extension(observation): Extension<RequestContext>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Response {
    if let Err(failure) = anonymous(&headers).and_then(|()| empty(&headers, &bytes)) {
        return failure.respond(&observation);
    }
    match state.auth.issue_api_challenge() {
        Ok(challenge) => json(
            &observation,
            &serde_json::json!({"challenge": challenge, "expires_in": 600}),
            4096,
        ),
        Err(error) => auth_error(&observation, error),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionRequest {
    challenge: String,
    credential: String,
}

async fn session(
    State(state): State<AppState>,
    Extension(observation): Extension<RequestContext>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Response {
    if let Err(failure) = anonymous(&headers) {
        return failure.respond(&observation);
    }
    let input: SessionRequest = match parse(&headers, &bytes) {
        Ok(input) => input,
        Err(failure) => return failure.respond(&observation),
    };
    if !token(&input.challenge) {
        return reject(
            &observation,
            StatusCode::BAD_REQUEST,
            MetricRejection::Input,
        );
    }
    match state
        .auth
        .authenticate_api(&input.credential, &input.challenge)
    {
        Ok((id, _)) => json(
            &observation,
            &serde_json::json!({"token": id, "token_type": "Bearer", "expires_in": 43200}),
            4096,
        ),
        Err(error) => auth_error(&observation, error),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SubmissionRequest {
    model: String,
    new_conversation: bool,
}

async fn live_catalog(state: &AppState) -> Result<Catalog, ControlFailure> {
    tokio::time::timeout(crate::inference::stream::RESPONSE_TIMEOUT, async {
        state
            .generation()
            .evidence()
            .await
            .map_err(|error| ControlFailure {
                wire: InferenceFailure::VerificationFailed,
                cause: error.into(),
                catalog: None,
            })?;
        authenticated_catalog_checked(state.inference.as_ref(), now_unix())
            .await
            .map_err(|error| ControlFailure {
                wire: error.wire(),
                cause: error.into(),
                catalog: Some(error),
            })
    })
    .await
    .map_err(|_| ControlFailure {
        wire: InferenceFailure::InferenceUnavailable,
        cause: InterfaceError::PreflightDeadline,
        catalog: None,
    })?
}

async fn submission(
    State(state): State<AppState>,
    Extension(observation): Extension<RequestContext>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Response {
    let (id, _) = match authenticated(&state, &headers) {
        Ok(auth) => auth,
        Err(failure) => return failure.respond(&observation),
    };
    let input: SubmissionRequest = match parse(&headers, &bytes) {
        Ok(input) => input,
        Err(failure) => return failure.respond(&observation),
    };
    if input.model.is_empty() || input.model.len() > 128 {
        return reject(
            &observation,
            StatusCode::BAD_REQUEST,
            MetricRejection::Input,
        );
    }
    let catalog = match live_catalog(&state).await {
        Ok(catalog) => catalog,
        Err(failure) => return control_inference_error(&observation, failure),
    };
    if let Err(error) = catalog.reservation_quote(&input.model) {
        return ApiFailure {
            status: StatusCode::BAD_REQUEST,
            rejection: MetricRejection::Input,
            cause: error.into(),
        }
        .respond(&observation);
    }
    match state
        .auth
        .issue_api_submission(&id, &input.model, input.new_conversation)
    {
        Ok(submission) => json(
            &observation,
            &serde_json::json!({"submission": submission}),
            4096,
        ),
        Err(error) => auth_error(&observation, error),
    }
}

async fn logout(
    State(state): State<AppState>,
    Extension(observation): Extension<RequestContext>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Response {
    let (id, _) = match authenticated(&state, &headers) {
        Ok(auth) => auth,
        Err(failure) => return failure.respond(&observation),
    };
    if let Err(failure) = empty(&headers, &bytes) {
        return failure.respond(&observation);
    }
    match state.auth.logout_api(&id) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => auth_error(&observation, error),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BalanceRequest {}

async fn balance(
    State(state): State<AppState>,
    Extension(observation): Extension<RequestContext>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Response {
    let (_, session) = match authenticated(&state, &headers) {
        Ok(auth) => auth,
        Err(failure) => return failure.respond(&observation),
    };
    if let Err(failure) = parse::<BalanceRequest>(&headers, &bytes) {
        return failure.respond(&observation);
    }
    let snapshot = match state.accounting.snapshot(&session.account_id) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            return ApiFailure {
                status: StatusCode::SERVICE_UNAVAILABLE,
                rejection: MetricRejection::Internal,
                cause: match error {
                    AccountingError::UnknownAccount => InterfaceError::AccountUnavailable,
                    _ => InterfaceError::AccountingStateUnknown,
                },
            }
            .respond(&observation)
        }
    };
    // Fixed-size, account-local control response. No inference or catalog calls;
    // failure observations contain only the closed category, never this balance.
    axum::Json(serde_json::json!({
        "available_microunits": snapshot.available_microunits.to_string(),
        "in_flight": snapshot.in_flight,
        "completed_requests": snapshot.completed_requests.to_string(),
    }))
    .into_response()
}

#[derive(Serialize)]
struct ModelEntry {
    object: &'static str,
    id: String,
    // Public display name is presentation-only, non-authorizing metadata. It is
    // exposed only under the same opt-in as reasoning metadata so the default
    // catalog stays byte-for-byte identical for older strict decoders.
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    // All u64 values are decimal strings, including context/output limits, so
    // catalog changes cannot silently lose precision in JavaScript consumers.
    context_tokens: String,
    max_output_tokens: String,
    input_microunits_per_million_tokens: String,
    output_microunits_per_million_tokens: String,
    maximum_reservation_microunits: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    cached_input_microunits_per_million_tokens: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_protocol: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_params: Option<crate::catalog::ReasoningParams>,
}

async fn models(
    State(state): State<AppState>,
    Extension(observation): Extension<RequestContext>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Response {
    let (id, _) = match authenticated(&state, &headers) {
        Ok(auth) => auth,
        Err(failure) => return failure.respond(&observation),
    };
    if let Err(failure) = empty(&headers, &bytes) {
        return failure.respond(&observation);
    }
    let catalog = match live_catalog(&state).await {
        Ok(catalog) => catalog,
        Err(failure) => return control_inference_error(&observation, failure),
    };
    let mut data = Vec::with_capacity(catalog.models.len());
    for model in &catalog.models {
        let quote = match catalog.reservation_quote(&model.id) {
            Ok(quote) => quote,
            Err(error) => {
                return control_inference_error(
                    &observation,
                    ControlFailure {
                        wire: InferenceFailure::CatalogFailed,
                        cause: error.into(),
                        catalog: None,
                    },
                )
            }
        };
        data.push(ModelEntry {
            object: "model",
            name: reasoning_metadata_requested(&headers)
                .then(|| model.public_name.clone())
                .flatten(),
            reasoning: reasoning_metadata_requested(&headers)
                .then_some(model.reasoning)
                .flatten(),
            reasoning_params: reasoning_metadata_requested(&headers)
                .then(|| model.reasoning_params.clone())
                .flatten(),
            tool_protocol: state
                .inference
                .tool_profile(&model.id)
                .map(|p| p.protocol()),
            id: model.id.clone(),
            context_tokens: model.context_tokens.to_string(),
            max_output_tokens: model.max_output_tokens.to_string(),
            input_microunits_per_million_tokens: model
                .input_microunits_per_million_tokens
                .to_string(),
            output_microunits_per_million_tokens: model
                .output_microunits_per_million_tokens
                .to_string(),
            maximum_reservation_microunits: quote.reserved_microunits.to_string(),
            cached_input_microunits_per_million_tokens: model
                .cached_input_microunits_per_million_tokens
                .map(|rate| rate.to_string()),
        });
    }
    if let Err(error) = state.auth.checked_api_session(&id) {
        return auth_error(&observation, error);
    }
    #[derive(Serialize)]
    struct List {
        object: &'static str,
        data: Vec<ModelEntry>,
    }
    json(
        &observation,
        &List {
            object: "list",
            data,
        },
        MAX_CATALOG_BYTES,
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChatRequest {
    model: String,
    stream: bool,
    submission: String,
    stream_options: Option<StreamOptions>,
    n: Option<u8>,
    messages: Vec<ToolMessage>,
    #[serde(default, deserialize_with = "present")]
    tools: Option<Vec<Tool>>,
    #[serde(default, deserialize_with = "present")]
    tool_choice: Option<ToolChoice>,
    #[serde(default, deserialize_with = "present")]
    reasoning_effort: Option<String>,
    #[serde(default, deserialize_with = "present")]
    chat_template_kwargs: Option<std::collections::BTreeMap<String, serde_json::Value>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StreamOptions {
    include_usage: bool,
}

// Presence is significant: explicit null is not absence/capability bypass.
fn present<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    decoder: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(decoder).map(Some)
}

fn reasoning_metadata_requested(headers: &HeaderMap) -> bool {
    let mut values = headers.get_all("x-possums-reasoning-metadata").iter();
    values.next().is_some_and(|value| value == "true") && values.next().is_none()
}

fn usage_metrics_requested(headers: &HeaderMap) -> bool {
    let mut values = headers.get_all("x-tinfoil-request-usage-metrics").iter();
    values.next().is_some_and(|value| value == "true") && values.next().is_none()
}

async fn chat(
    State(state): State<AppState>,
    Extension(ticket): Extension<crate::lifecycle::Ticket>,
    Extension(disconnect): Extension<crate::stream_owner::Disconnect>,
    Extension(heavy): Extension<Arc<crate::telemetry::hooks::Lease>>,
    Extension(observation): Extension<RequestContext>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Response {
    let reject = |status, reason| reject(&observation, status, reason);
    let (session_id, session) = match authenticated(&state, &headers) {
        Ok(auth) => auth,
        Err(failure) => return failure.respond(&observation),
    };
    let mut input: ChatRequest = match parse(&headers, &bytes) {
        Ok(input) => input,
        Err(failure) => return failure.respond(&observation),
    };
    drop(bytes);
    if !input.stream
        || input.n.is_some_and(|n| n != 1)
        || input
            .stream_options
            .is_some_and(|options| !options.include_usage)
        || !token(&input.submission)
        || input.model.is_empty()
        || input.model.len() > 128
        || input.messages.is_empty()
    {
        return reject(StatusCode::BAD_REQUEST, MetricRejection::Input);
    }
    let reasoning_options = crate::catalog::ReasoningOptions {
        reasoning_effort: input.reasoning_effort.take(),
        chat_template_kwargs: input.chat_template_kwargs.take(),
    };
    let usage_metrics = usage_metrics_requested(&headers);
    let inference = state.inference.clone();
    let delivery_heavy = heavy.clone();
    let submission = Submission {
        session_id: &session_id,
        admission_binding: &session.admission_binding,
        token: &input.submission,
    };
    let structured = input.tools.is_some()
        || input.tool_choice.is_some()
        || input.messages.iter().any(ToolMessage::is_structured);
    let result = if structured {
        let invocation = match ToolInvocation::new(input.messages, input.tools, input.tool_choice) {
            Ok(invocation) => invocation,
            Err(_) => return reject(StatusCode::BAD_REQUEST, MetricRejection::Input),
        };
        state
            .generation()
            .observed(observation.clone())
            .disconnected(disconnect.clone())
            .submit_structured(
                ticket,
                submission,
                StructuredGenerationInput {
                    model: input.model,
                    reasoning_options,
                    invocation,
                },
                heavy,
                move |owner, prepared| {
                    crate::api_stream::compose_structured(
                        owner,
                        delivery_heavy,
                        prepared,
                        inference,
                        usage_metrics,
                    )
                },
            )
            .await
    } else {
        let Some(ToolMessage::User { content: prompt }) = input.messages.pop() else {
            return reject(StatusCode::BAD_REQUEST, MetricRejection::Input);
        };
        if prompt.is_empty() {
            return reject(StatusCode::BAD_REQUEST, MetricRejection::Input);
        }
        let history = match input
            .messages
            .into_iter()
            .map(ToolMessage::into_text)
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(history) => history,
            Err(_) => return reject(StatusCode::BAD_REQUEST, MetricRejection::Input),
        };
        state
            .generation()
            .observed(observation.clone())
            .disconnected(disconnect)
            .submit(
                ticket,
                submission,
                GenerationInput {
                    model: input.model,
                    reasoning_options,
                    history,
                    prompt,
                },
                heavy,
                move |owner, prepared| {
                    crate::api_stream::compose(
                        owner,
                        delivery_heavy,
                        prepared,
                        inference,
                        usage_metrics,
                    )
                },
            )
            .await
    };
    match result {
        Ok(body) => {
            let mut response = axum::body::Body::new(body).into_response();
            if usage_metrics {
                response
                    .headers_mut()
                    .insert(header::TRAILER, "X-Tinfoil-Usage-Metrics".parse().unwrap());
            }
            response
                .headers_mut()
                .insert(header::CONTENT_TYPE, "text/event-stream".parse().unwrap());
            response
                .headers_mut()
                .insert("x-accel-buffering", "no".parse().unwrap());
            response
        }
        Err(rejection) => rejection_response(rejection),
    }
}
