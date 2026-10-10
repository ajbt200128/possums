//! Cookie-independent, streaming-only Phase 0.1 API.
use crate::{
    accounting::{AccountingError, Outcome},
    auth::{AdmissionError, AuthError, Session},
    bounded_json,
    catalog::{Catalog, MAX_CATALOG_BYTES},
    generation::{now_unix, GenerationInput, Rejection, StructuredGenerationInput, Submission},
    inference::{
        authenticated_catalog,
        tools::{Tool, ToolChoice, ToolInvocation, ToolMessage},
        InferenceFailure,
    },
    telemetry::{hooks::RequestContext, AdmissionModel, Rejection as MetricRejection},
    web::AppState,
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
use std::{sync::Arc, time::Duration};

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

pub(crate) fn error(status: StatusCode) -> Response {
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
        axum::Json(serde_json::json!({"error": {"code": code}})),
    )
        .into_response();
    response.extensions_mut().insert(SafeError);
    response
}

fn reject(observation: &RequestContext, status: StatusCode, reason: MetricRejection) -> Response {
    observation.reject(AdmissionModel::Unknown, reason);
    error(status)
}

fn control_inference_error(observation: &RequestContext, failure: InferenceFailure) -> Response {
    let reason = match failure {
        InferenceFailure::VerificationFailed | InferenceFailure::EndpointBindingFailed => {
            MetricRejection::Verification
        }
        InferenceFailure::CatalogFailed => MetricRejection::Catalog,
        _ => MetricRejection::Internal,
    };
    observation.reject(AdmissionModel::Unknown, reason);
    inference_error(failure)
}

fn inference_error(failure: InferenceFailure) -> Response {
    let mut response = (
        StatusCode::SERVICE_UNAVAILABLE,
        axum::Json(serde_json::json!({"error": {
            "code": "unavailable", "detail": failure,
            "message": failure.to_string(), "billing": "unknown"
        }})),
    )
        .into_response();
    response.extensions_mut().insert(SafeError);
    response
}

fn auth_error(observation: &RequestContext, error: AuthError) -> Response {
    let reason = match error {
        AuthError::Invalid => MetricRejection::Auth,
        AuthError::Capacity | AuthError::Configuration => MetricRejection::Internal,
    };
    reject(
        observation,
        match error {
            AuthError::Invalid => StatusCode::UNAUTHORIZED,
            _ => StatusCode::SERVICE_UNAVAILABLE,
        },
        reason,
    )
}

fn json(observation: &RequestContext, value: &impl Serialize, cap: usize) -> Response {
    match bounded_json::to_vec(value, cap) {
        Ok(bytes) => ([(header::CONTENT_TYPE, "application/json")], bytes).into_response(),
        Err(_) => reject(
            observation,
            StatusCode::SERVICE_UNAVAILABLE,
            MetricRejection::Internal,
        ),
    }
}

fn token(value: &str) -> bool {
    value.len() == 43
        && URL_SAFE_NO_PAD
            .decode(value)
            .is_ok_and(|bytes| bytes.len() == 32)
}

fn no_cookie_or_encoding(headers: &HeaderMap) -> Result<(), StatusCode> {
    if headers.contains_key(header::COOKIE) || headers.contains_key(header::CONTENT_ENCODING) {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(())
}

fn anonymous(headers: &HeaderMap) -> Result<(), StatusCode> {
    no_cookie_or_encoding(headers)?;
    if headers.contains_key(header::AUTHORIZATION) {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(())
}

// Exact one-header grammar, no comma folding, alternate scheme, whitespace or
// recovery-credential fallback. Only API-kind sessions are usable here.
fn authenticated(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(String, Session), (StatusCode, MetricRejection)> {
    no_cookie_or_encoding(headers).map_err(|status| (status, MetricRejection::Input))?;
    let mut values = headers.get_all(header::AUTHORIZATION).iter();
    let value = values.next().and_then(|value| value.to_str().ok());
    let id = value.and_then(|value| value.strip_prefix("Bearer "));
    let Some(id) = id
        .filter(|id| token(id))
        .filter(|_| values.next().is_none())
    else {
        return Err((StatusCode::UNAUTHORIZED, MetricRejection::Auth));
    };
    let session = state
        .auth
        .api_session(id)
        .ok_or((StatusCode::UNAUTHORIZED, MetricRejection::Auth))?;
    Ok((id.to_owned(), session))
}

fn empty(headers: &HeaderMap, bytes: &Bytes) -> Result<(), StatusCode> {
    if !bytes.is_empty() || headers.contains_key(header::CONTENT_TYPE) {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(())
}

fn parse<T: DeserializeOwned>(headers: &HeaderMap, bytes: &Bytes) -> Result<T, StatusCode> {
    let mut types = headers.get_all(header::CONTENT_TYPE).iter();
    if types.next().is_none_or(|value| value != "application/json") || types.next().is_some() {
        return Err(StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }
    if bytes.iter().find(|byte| !byte.is_ascii_whitespace()) != Some(&b'{') {
        return Err(StatusCode::BAD_REQUEST);
    }
    // Middleware has already bounded collection under its deadline and retained
    // request lease. Typed structs reject unknown fields; the guard also rejects
    // duplicate keys inside opaque tool schemas before Value can discard them.
    crate::inference::stream::validate_request_json(bytes).map_err(|_| StatusCode::BAD_REQUEST)?;
    serde_json::from_slice(bytes).map_err(|_| StatusCode::BAD_REQUEST)
}

async fn challenge(
    State(state): State<AppState>,
    Extension(observation): Extension<RequestContext>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Response {
    if let Err(status) = anonymous(&headers).and_then(|()| empty(&headers, &bytes)) {
        return reject(&observation, status, MetricRejection::Input);
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
    if let Err(status) = anonymous(&headers) {
        return reject(&observation, status, MetricRejection::Input);
    }
    let input: SessionRequest = match parse(&headers, &bytes) {
        Ok(input) => input,
        Err(status) => return reject(&observation, status, MetricRejection::Input),
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

async fn live_catalog(state: &AppState) -> Result<Catalog, InferenceFailure> {
    tokio::time::timeout(Duration::from_secs(30), async {
        state
            .generation()
            .evidence()
            .await
            .map_err(|_| InferenceFailure::VerificationFailed)?;
        authenticated_catalog(state.inference.as_ref(), now_unix())
            .await
            .map_err(|error| error.failure())
    })
    .await
    .map_err(|_| InferenceFailure::InferenceUnavailable)?
}

async fn submission(
    State(state): State<AppState>,
    Extension(observation): Extension<RequestContext>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Response {
    let (id, _) = match authenticated(&state, &headers) {
        Ok(auth) => auth,
        Err((status, reason)) => return reject(&observation, status, reason),
    };
    let input: SubmissionRequest = match parse(&headers, &bytes) {
        Ok(input) => input,
        Err(status) => return reject(&observation, status, MetricRejection::Input),
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
    if catalog.reservation_quote(&input.model).is_err() {
        return reject(
            &observation,
            StatusCode::BAD_REQUEST,
            MetricRejection::Input,
        );
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
        Err(AuthError::Invalid) => {
            if state.auth.api_session(&id).is_none() {
                reject(
                    &observation,
                    StatusCode::UNAUTHORIZED,
                    MetricRejection::Auth,
                )
            } else {
                reject(&observation, StatusCode::CONFLICT, MetricRejection::Auth)
            }
        }
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
        Err((status, reason)) => return reject(&observation, status, reason),
    };
    if let Err(status) = empty(&headers, &bytes) {
        return reject(&observation, status, MetricRejection::Input);
    }
    match state.auth.logout_api(&id) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => auth_error(&observation, error),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BalanceRequest {}

async fn balance(State(state): State<AppState>, headers: HeaderMap, bytes: Bytes) -> Response {
    let (_, session) = match authenticated(&state, &headers) {
        Ok(auth) => auth,
        Err((status, _)) => return error(status),
    };
    if let Err(status) = parse::<BalanceRequest>(&headers, &bytes) {
        return error(status);
    }
    let snapshot = match state.accounting.snapshot(&session.account_id) {
        Ok(snapshot) => snapshot,
        Err(_) => return error(StatusCode::SERVICE_UNAVAILABLE),
    };
    // Fixed-size, account-local control response. No identifiers, inference,
    // catalog lookup or telemetry observations are needed for reconciliation.
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
    // All u64 values are decimal strings, including context/output limits, so
    // catalog changes cannot silently lose precision in JavaScript consumers.
    context_tokens: String,
    max_output_tokens: String,
    input_microunits_per_million_tokens: String,
    output_microunits_per_million_tokens: String,
    maximum_reservation_microunits: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_protocol: Option<&'static str>,
}

async fn models(
    State(state): State<AppState>,
    Extension(observation): Extension<RequestContext>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Response {
    let (id, _) = match authenticated(&state, &headers) {
        Ok(auth) => auth,
        Err((status, reason)) => return reject(&observation, status, reason),
    };
    if let Err(status) = empty(&headers, &bytes) {
        return reject(&observation, status, MetricRejection::Input);
    }
    let catalog = match live_catalog(&state).await {
        Ok(catalog) => catalog,
        Err(failure) => return control_inference_error(&observation, failure),
    };
    let mut data = Vec::with_capacity(catalog.models.len());
    for model in &catalog.models {
        let quote = match catalog.reservation_quote(&model.id) {
            Ok(quote) => quote,
            Err(_) => {
                return control_inference_error(&observation, InferenceFailure::CatalogFailed)
            }
        };
        data.push(ModelEntry {
            object: "model",
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
        });
    }
    if state.auth.api_session(&id).is_none() {
        return reject(
            &observation,
            StatusCode::UNAUTHORIZED,
            MetricRejection::Auth,
        );
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

async fn chat(
    State(state): State<AppState>,
    Extension(heavy): Extension<Arc<crate::telemetry::hooks::Lease>>,
    Extension(observation): Extension<RequestContext>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Response {
    let reject = |status, reason| reject(&observation, status, reason);
    let (session_id, session) = match authenticated(&state, &headers) {
        Ok(auth) => auth,
        Err((status, reason)) => return reject(status, reason),
    };
    let mut input: ChatRequest = match parse(&headers, &bytes) {
        Ok(input) => input,
        Err(status) => return reject(status, MetricRejection::Input),
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
    let inference = state.inference.clone();
    let delivery_heavy = heavy.clone();
    let submission = Submission {
        session_id: &session_id,
        csrf: &session.csrf,
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
            .submit_structured(
                submission,
                StructuredGenerationInput {
                    model: input.model,
                    invocation,
                },
                heavy,
                move |owner, prepared| {
                    crate::api_stream::compose_structured(
                        owner,
                        delivery_heavy,
                        prepared,
                        inference,
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
            .submit(
                submission,
                GenerationInput {
                    model: input.model,
                    history,
                    prompt,
                },
                heavy,
                move |owner, prepared| {
                    crate::api_stream::compose(owner, delivery_heavy, prepared, inference)
                },
            )
            .await
    };
    match result {
        Ok(body) => {
            let mut response = axum::body::Body::new(body).into_response();
            response
                .headers_mut()
                .insert(header::CONTENT_TYPE, "text/event-stream".parse().unwrap());
            response
                .headers_mut()
                .insert("x-accel-buffering", "no".parse().unwrap());
            response
        }
        Err(Rejection::Duplicate(outcome)) => {
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
        Err(Rejection::Upstream(failure)) => inference_error(failure),
        Err(Rejection::Unavailable) => inference_error(InferenceFailure::InferenceUnavailable),
        Err(rejection) => error(match rejection {
            Rejection::InvalidModel
            | Rejection::Context
            | Rejection::Admission(AdmissionError::Auth(_)) => StatusCode::BAD_REQUEST,
            Rejection::Admission(AdmissionError::Accounting(
                AccountingError::InsufficientCredit,
            )) => StatusCode::PAYMENT_REQUIRED,
            Rejection::Admission(AdmissionError::Accounting(AccountingError::Concurrency)) => {
                StatusCode::TOO_MANY_REQUESTS
            }
            _ => StatusCode::SERVICE_UNAVAILABLE,
        }),
    }
}
