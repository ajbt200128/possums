use crate::{
    accounting::{Accounting, Outcome, ReserveResult},
    attestation::load_evidence,
    auth::{clear_session_cookie, session_cookie, Auth, Session},
    catalog::actual_cost,
    inference::{authenticated_catalog, Message, SharedInference},
    render,
};
use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Form, State},
    http::{header, HeaderMap, HeaderValue, Request, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Router,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::Semaphore;
use tower::limit::ConcurrencyLimitLayer;
use tower_http::catch_panic::CatchPanicLayer;

const BODY_LIMIT: usize = 8 * 1024 * 1024;

#[derive(Clone)]
pub struct AppState {
    pub auth: Arc<Auth>,
    pub accounting: Arc<Accounting>,
    pub inference: SharedInference,
    pub gateway_evidence_path: Arc<str>,
    generation_slots: Arc<Semaphore>,
}

impl AppState {
    pub fn new(auth: Auth, inference: SharedInference, evidence_path: impl Into<Arc<str>>) -> Self {
        let accounting = Accounting::new(auth.account_budgets());
        Self {
            auth: Arc::new(auth),
            accounting: Arc::new(accounting),
            inference,
            gateway_evidence_path: evidence_path.into(),
            generation_slots: Arc::new(Semaphore::new(4)),
        }
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(home))
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/chat", post(chat))
        .route("/recovery", get(recovery))
        .route("/claims", get(claims))
        .route("/attestation", get(attestation))
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .layer(ConcurrencyLimitLayer::new(32))
        .layer(CatchPanicLayer::new())
        .layer(middleware::from_fn(security_headers))
        .with_state(state)
}

async fn security_headers(request: Request<Body>, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'none'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'",
        ),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}

fn session_from_headers(state: &AppState, headers: &HeaderMap) -> Option<(String, Session)> {
    let cookies = headers.get(header::COOKIE)?.to_str().ok()?;
    let id = cookies.split(';').find_map(|cookie| {
        let (name, value) = cookie.trim().split_once('=')?;
        (name == "possums_session").then(|| value.to_owned())
    })?;
    state.auth.session(&id).map(|session| (id, session))
}

async fn home(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let Some((_, session)) = session_from_headers(&state, &headers) else {
        return Html(render::login_page(None)).into_response();
    };
    match current_catalog(&state).await {
        Ok(catalog) => {
            Html(render::chat_page(&catalog.models, &session.csrf, &[], None)).into_response()
        }
        Err(response) => response,
    }
}

#[derive(Deserialize)]
struct LoginForm {
    credential: String,
}

async fn login(State(state): State<AppState>, Form(form): Form<LoginForm>) -> Response {
    match state.auth.authenticate(&form.credential) {
        Ok((id, _)) => {
            let mut response = Redirect::to("/").into_response();
            if let Ok(value) = HeaderValue::from_str(&session_cookie(&id)) {
                response.headers_mut().insert(header::SET_COOKIE, value);
                response
            } else {
                internal_error()
            }
        }
        Err(_) => (
            StatusCode::UNAUTHORIZED,
            Html(render::login_page(Some("Invalid credential"))),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct CsrfForm {
    csrf: String,
}

async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<CsrfForm>,
) -> Response {
    let Some((id, _)) = session_from_headers(&state, &headers) else {
        return unauthorized();
    };
    if state.auth.logout(&id, &form.csrf).is_err() {
        return unauthorized();
    }
    let mut response = Redirect::to("/").into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_static(clear_session_cookie()),
    );
    response
}

async fn recovery(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let Some((_, session)) = session_from_headers(&state, &headers) else {
        return unauthorized();
    };
    Html(render::page(&format!(
        "<h1>Recovery credential</h1><p>Copy or download this credential now. Anyone holding it controls this demo account.</p><pre>{}</pre><p><a href=/>Return</a></p>",
        render::escape(&session.recovery_credential)
    )))
    .into_response()
}

async fn claims() -> Html<String> {
    Html(render::page(
        "<h1>Phase 0 claims</h1><p>The gateway and Tinfoil see prompt plaintext in memory. Request content is not intentionally persisted or exported. Attestation identifies measured code and configuration; it does not prevent runtime compromise or traffic correlation.</p><p>Demo accounting is memory-only and resets on restart. No payments, Tor, replicas, public API, or audit are included.</p><p><a href=/>Return</a></p>",
    ))
}

async fn attestation(State(state): State<AppState>) -> Response {
    let gateway = match load_evidence(state.gateway_evidence_path.as_ref()) {
        Ok(value) => value,
        Err(_) => return unavailable(),
    };
    let upstream = match state.inference.verification_document() {
        Ok(value) => value,
        Err(_) => return unavailable(),
    };
    axum::Json(serde_json::json!({"gateway": gateway, "upstream": upstream})).into_response()
}

#[derive(Deserialize)]
struct ChatForm {
    csrf: String,
    token: String,
    model: String,
    history: String,
    prompt: String,
}

async fn chat(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<ChatForm>,
) -> Response {
    let Some((_, session)) = session_from_headers(&state, &headers) else {
        return unauthorized();
    };
    if !Auth::verify_csrf(&session, &form.csrf) || form.token.len() < 32 || form.prompt.is_empty() {
        return bad_request();
    }
    let mut history: Vec<Message> = match serde_json::from_str(&form.history) {
        Ok(value) => value,
        Err(_) => return bad_request(),
    };
    if history.iter().any(|message| {
        !matches!(message.role.as_str(), "user" | "assistant") || message.content.is_empty()
    }) {
        return bad_request();
    }
    history.push(Message {
        role: "user".into(),
        content: form.prompt,
    });

    // Gateway provenance and serving-key evidence is a mandatory pre-prompt gate.
    if load_evidence(state.gateway_evidence_path.as_ref()).is_err() {
        return unavailable();
    }
    let catalog = match current_catalog(&state).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let input_tokens = match state.inference.count_tokens(&form.model, &history).await {
        Ok(value) => value,
        Err(_) => return unavailable(),
    };
    let quote = match catalog.quote(&form.model, input_tokens) {
        Ok(value) => value,
        Err(_) => return bad_request(),
    };
    let submission_id = state.auth.bind_submission(&session.account_id, &form.token);
    let request_digest: [u8; 32] = Sha256::digest(form.model.as_bytes()).into();
    match state.accounting.reserve(
        &session.account_id,
        submission_id,
        request_digest,
        quote.clone(),
    ) {
        Ok(ReserveResult::Reserved) => {}
        Ok(ReserveResult::Duplicate(outcome)) => {
            let notice = match outcome {
                Outcome::InFlight => "This submission is already in progress.",
                Outcome::Settled { .. } => {
                    "This submission already completed and was not regenerated."
                }
                Outcome::Refunded => {
                    "This submission failed and was refunded; use the new form to retry."
                }
            };
            return Html(render::chat_page(
                &catalog.models,
                &session.csrf,
                &history,
                Some(notice),
            ))
            .into_response();
        }
        Err(_) => return (StatusCode::PAYMENT_REQUIRED, "request unavailable").into_response(),
    }

    let permit = match state.generation_slots.clone().try_acquire_owned() {
        Ok(value) => value,
        Err(_) => {
            let _ = state.accounting.refund(submission_id);
            return (StatusCode::SERVICE_UNAVAILABLE, "service busy").into_response();
        }
    };
    let generation = state.inference.generate(&quote.model, &history).await;
    drop(permit);
    let generation = match generation {
        Ok(value) => value,
        Err(_) => {
            let _ = state.accounting.refund(submission_id);
            return unavailable();
        }
    };
    if generation.input_tokens != input_tokens
        || actual_cost(&quote, generation.output_tokens).is_err()
        || state
            .accounting
            .settle(
                submission_id,
                generation.input_tokens,
                generation.output_tokens,
            )
            .is_err()
    {
        let _ = state.accounting.refund(submission_id);
        return unavailable();
    }
    history.push(Message {
        role: "assistant".into(),
        content: generation.content,
    });
    Html(render::chat_page(
        &catalog.models,
        &session.csrf,
        &history,
        None,
    ))
    .into_response()
}

async fn current_catalog(state: &AppState) -> Result<crate::catalog::Catalog, Response> {
    authenticated_catalog(state.inference.as_ref(), now_unix())
        .await
        .map_err(|_| unavailable())
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn unauthorized() -> Response {
    (StatusCode::UNAUTHORIZED, "authentication required").into_response()
}
fn bad_request() -> Response {
    (StatusCode::BAD_REQUEST, "invalid request").into_response()
}
fn unavailable() -> Response {
    (StatusCode::SERVICE_UNAVAILABLE, "service unavailable").into_response()
}
fn internal_error() -> Response {
    (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
}
