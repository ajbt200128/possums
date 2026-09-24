use crate::{
    accounting::{Accounting, Outcome, ReserveResult},
    attestation::{load_evidence, EvidenceVerifier},
    auth::{clear_session_cookie, login_challenge_cookie, session_cookie, Auth, Session},
    catalog::actual_cost,
    inference::{authenticated_catalog, Message, SharedInference},
    render,
};
use axum::{
    body::{to_bytes, Body, Bytes, HttpBody},
    extract::{DefaultBodyLimit, Form, State},
    http::{header, HeaderMap, HeaderValue, Request, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Router,
};
use hyper::server::conn::http1;
use hyper_util::{
    rt::{TokioIo, TokioTimer},
    service::TowerToHyperService,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    net::TcpListener,
    sync::{OwnedSemaphorePermit, Semaphore},
};
use tower_http::catch_panic::CatchPanicLayer;

const BODY_LIMIT: usize = 8 * 1024 * 1024;
const BODY_DEADLINE: Duration = Duration::from_secs(30);
const HEADER_DEADLINE: Duration = Duration::from_secs(10);
const CONNECTION_DEADLINE: Duration = Duration::from_secs(10 * 60);
const MAX_CONNECTIONS: usize = 64;
const MAX_HEADERS: usize = 64;
const MAX_HEADER_BYTES: usize = 32 * 1024;
const REQUEST_MEMORY_BYTES: usize = 128 * 1024 * 1024;
const SHARED_MEMORY_BYTES: usize = 512 * 1024 * 1024;
const MAX_RENDERED_RESPONSE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone)]
pub struct AppState {
    pub auth: Arc<Auth>,
    pub accounting: Arc<Accounting>,
    pub inference: SharedInference,
    pub gateway_evidence_path: Arc<str>,
    gateway_evidence_verifier: Arc<dyn EvidenceVerifier>,
    request_memory: Arc<Semaphore>,
    generation_slots: Arc<Semaphore>,
}

impl AppState {
    pub fn new(
        auth: Auth,
        inference: SharedInference,
        evidence_path: impl Into<Arc<str>>,
        gateway_evidence_verifier: Arc<dyn EvidenceVerifier>,
    ) -> Self {
        let accounting = Accounting::new(auth.account_budgets());
        Self {
            auth: Arc::new(auth),
            accounting: Arc::new(accounting),
            inference,
            gateway_evidence_path: evidence_path.into(),
            gateway_evidence_verifier,
            request_memory: Arc::new(Semaphore::new(SHARED_MEMORY_BYTES / REQUEST_MEMORY_BYTES)),
            generation_slots: Arc::new(Semaphore::new(4)),
        }
    }
}

pub fn router(state: AppState) -> Router {
    router_with_body_deadline(state, BODY_DEADLINE)
}

#[doc(hidden)]
pub fn router_with_body_deadline(state: AppState, body_deadline: Duration) -> Router {
    Router::new()
        .route("/", get(home))
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/chat", post(chat))
        .route("/confirm", post(confirm_delivery))
        .route("/recovery", get(recovery))
        .route("/recovery/download", get(recovery_download))
        .route("/claims", get(claims))
        .route("/attestation", get(attestation))
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .layer(middleware::from_fn(move |request, next| {
            total_body_deadline(request, next, body_deadline)
        }))
        .layer(CatchPanicLayer::new())
        .layer(middleware::from_fn(header_size_limit))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            request_admission,
        ))
        .layer(middleware::from_fn(security_headers))
        .with_state(state)
}

pub async fn serve(listener: TcpListener, state: AppState) -> std::io::Result<()> {
    serve_with_header_deadline(listener, state, HEADER_DEADLINE).await
}

#[doc(hidden)]
pub async fn serve_with_header_deadline(
    listener: TcpListener,
    state: AppState,
    header_deadline: Duration,
) -> std::io::Result<()> {
    let connections = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    let app = router(state);
    loop {
        let (stream, _) = listener.accept().await?;
        let Ok(permit) = connections.clone().try_acquire_owned() else {
            drop(stream);
            continue;
        };
        let service = TowerToHyperService::new(app.clone());
        tokio::spawn(async move {
            let mut builder = http1::Builder::new();
            builder
                .timer(TokioTimer::new())
                .header_read_timeout(header_deadline)
                .max_headers(MAX_HEADERS)
                .max_buf_size(MAX_HEADER_BYTES);
            let connection = builder.serve_connection(TokioIo::new(stream), service);
            let _ = tokio::time::timeout(CONNECTION_DEADLINE, connection).await;
            drop(permit);
        });
    }
}

async fn request_admission(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let Ok(permit) = state.request_memory.clone().try_acquire_owned() else {
        return (StatusCode::SERVICE_UNAVAILABLE, "service busy").into_response();
    };
    let response = next.run(request).await;
    let (parts, body) = response.into_parts();
    Response::from_parts(
        parts,
        Body::new(AdmissionBody {
            inner: body,
            _permit: permit,
        }),
    )
}

struct AdmissionBody {
    inner: Body,
    _permit: OwnedSemaphorePermit,
}

impl HttpBody for AdmissionBody {
    type Data = Bytes;
    type Error = axum::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        Pin::new(&mut self.inner).poll_frame(context)
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> http_body::SizeHint {
        self.inner.size_hint()
    }
}

async fn total_body_deadline(request: Request<Body>, next: Next, deadline: Duration) -> Response {
    let (parts, body) = request.into_parts();
    let bytes = match tokio::time::timeout(deadline, to_bytes(body, BODY_LIMIT)).await {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(_)) => {
            return (StatusCode::PAYLOAD_TOO_LARGE, "request body too large").into_response()
        }
        Err(_) => return (StatusCode::REQUEST_TIMEOUT, "request body timed out").into_response(),
    };
    next.run(Request::from_parts(parts, Body::from(bytes)))
        .await
}

async fn header_size_limit(request: Request<Body>, next: Next) -> Response {
    let header_bytes = request
        .headers()
        .iter()
        .fold(0_usize, |total, (name, value)| {
            total
                .saturating_add(name.as_str().len())
                .saturating_add(value.as_bytes().len())
        });
    if header_bytes > MAX_HEADER_BYTES {
        return (
            StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE,
            "request headers too large",
        )
            .into_response();
    }
    next.run(request).await
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

fn cookie_value(headers: &HeaderMap, wanted: &str) -> Option<String> {
    let cookies = headers.get(header::COOKIE)?.to_str().ok()?;
    cookies.split(';').find_map(|cookie| {
        let (name, value) = cookie.trim().split_once('=')?;
        (name == wanted).then(|| value.to_owned())
    })
}

fn session_from_headers(state: &AppState, headers: &HeaderMap) -> Option<(String, Session)> {
    let id = cookie_value(headers, "possums_session")?;
    state.auth.session(&id).map(|session| (id, session))
}

async fn home(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let Some((session_id, session)) = session_from_headers(&state, &headers) else {
        return login_page_response(&state, None);
    };
    match current_catalog(&state).await {
        Ok(catalog) => match state.auth.issue_submission(&session_id) {
            Ok(token) => render::chat_page(
                &catalog.models,
                &session.csrf,
                &token,
                &[],
                None,
                None,
                MAX_RENDERED_RESPONSE_BYTES,
            )
            .map_or_else(unavailable, |html| Html(html).into_response()),
            Err(_) => unavailable(),
        },
        Err(response) => response,
    }
}

#[derive(Deserialize)]
struct LoginForm {
    csrf: String,
    credential: String,
}

async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<LoginForm>,
) -> Response {
    let Some(cookie_challenge) = cookie_value(&headers, "possums_login_csrf") else {
        return unauthorized();
    };
    if !constant_time_equal(cookie_challenge.as_bytes(), form.csrf.as_bytes()) {
        return unauthorized();
    }
    match state.auth.authenticate(&form.credential, &form.csrf) {
        Ok((id, _)) => {
            let mut response = Redirect::to("/").into_response();
            if let Ok(value) = HeaderValue::from_str(&session_cookie(&id)) {
                response.headers_mut().insert(header::SET_COOKIE, value);
                response
            } else {
                internal_error()
            }
        }
        Err(_) => (StatusCode::UNAUTHORIZED, "invalid credential").into_response(),
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
        "<h1>Recovery credential</h1><p>Copy or download this credential now. Anyone holding it controls this demo account.</p><pre>{}</pre><p><a href=/recovery/download download=possums-recovery.txt>Download recovery credential</a></p><p><a href=/>Return</a></p>",
        render::escape(&session.recovery_credential)
    )))
    .into_response()
}

async fn recovery_download(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let Some((_, session)) = session_from_headers(&state, &headers) else {
        return unauthorized();
    };
    let mut response = session.recovery_credential.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=\"possums-recovery.txt\""),
    );
    response
}

async fn claims() -> Html<String> {
    Html(render::page(
        "<h1>Phase 0 claims</h1><p>The gateway and Tinfoil see prompt plaintext in memory. Request content is not intentionally persisted or exported. Attestation identifies measured code and configuration; it does not prevent runtime compromise or traffic correlation.</p><p>Demo accounting is memory-only and resets on restart. No payments, Tor, replicas, public API, or audit are included.</p><p><a href=/>Return</a></p>",
    ))
}

async fn attestation(State(state): State<AppState>) -> Response {
    let gateway = match verified_gateway_evidence(&state) {
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
    let Some((session_id, session)) = session_from_headers(&state, &headers) else {
        return unauthorized();
    };
    if !Auth::verify_csrf(&session, &form.csrf) || form.prompt.is_empty() {
        return bad_request();
    }
    let mut history = match parse_history(&form.history) {
        Some(value) => value,
        None => return bad_request(),
    };
    history.push(Message {
        role: "user".into(),
        content: form.prompt,
    });

    // Gateway provenance and serving-key evidence is a mandatory pre-prompt gate.
    if verified_gateway_evidence(&state).is_err() {
        return unavailable();
    }
    let catalog = match current_catalog(&state).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let model = match catalog.models.iter().find(|model| model.id == form.model) {
        Some(model) => model,
        None => return bad_request(),
    };
    let maximum_input = model.context_tokens - model.max_output_tokens;
    let reservation_quote = match catalog.quote(&form.model, maximum_input) {
        Ok(value) => value,
        Err(_) => return bad_request(),
    };
    let submission =
        match state
            .auth
            .bind_submission(&session_id, &session.account_id, &form.token, &form.model)
        {
            Ok(value) => value,
            Err(_) => return bad_request(),
        };
    let submission_id = submission.id;
    let request_digest: [u8; 32] = Sha256::digest(form.model.as_bytes()).into();
    match state.accounting.reserve(
        &session.account_id,
        submission_id,
        request_digest,
        reservation_quote,
        submission.expires_at,
    ) {
        Ok(ReserveResult::Reserved) => {}
        Ok(ReserveResult::Duplicate(outcome)) => {
            let notice = match outcome {
                Outcome::InFlight | Outcome::AwaitingConfirmation => {
                    "This submission is already in progress."
                }
                Outcome::Settled { .. } => {
                    "This submission already completed and was not regenerated."
                }
                Outcome::Refunded => {
                    "This submission failed and was refunded; use the new form to retry."
                }
            };
            let token = match state.auth.issue_submission(&session_id) {
                Ok(value) => value,
                Err(_) => return unavailable(),
            };
            return render::chat_page(
                &catalog.models,
                &session.csrf,
                &token,
                &history,
                Some(notice),
                None,
                MAX_RENDERED_RESPONSE_BYTES,
            )
            .map_or_else(unavailable, |html| Html(html).into_response());
        }
        Err(_) => return (StatusCode::PAYMENT_REQUIRED, "request unavailable").into_response(),
    }

    let mut reservation = ReservationGuard::new(state.accounting.clone(), submission_id);
    let input_tokens = match state.inference.count_tokens(&form.model, &history).await {
        Ok(value) => value,
        Err(_) => {
            let _ = state.accounting.refund(submission_id);
            return unavailable();
        }
    };
    let quote = match catalog.quote(&form.model, input_tokens) {
        Ok(value) => value,
        Err(_) => {
            let _ = state.accounting.refund(submission_id);
            return bad_request();
        }
    };
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
            .prepare_settlement(
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
    let token = match state.auth.issue_submission(&session_id) {
        Ok(value) => value,
        Err(_) => return unavailable(),
    };
    let Some(html) = render::chat_page(
        &catalog.models,
        &session.csrf,
        &token,
        &history,
        Some("Confirm delivery to finalize the charge."),
        Some((&form.token, &form.model)),
        MAX_RENDERED_RESPONSE_BYTES,
    ) else {
        return unavailable();
    };
    reservation.disarm();
    Html(html).into_response()
}

#[derive(Deserialize)]
struct ConfirmationForm {
    csrf: String,
    token: String,
    model: String,
    history: String,
}

async fn confirm_delivery(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<ConfirmationForm>,
) -> Response {
    let Some((session_id, session)) = session_from_headers(&state, &headers) else {
        return unauthorized();
    };
    if !Auth::verify_csrf(&session, &form.csrf) {
        return bad_request();
    }
    let submission_id =
        match state
            .auth
            .bind_submission(&session_id, &session.account_id, &form.token, &form.model)
        {
            Ok(value) => value.id,
            Err(_) => return bad_request(),
        };
    let history = match parse_history(&form.history) {
        Some(value) => value,
        None => return bad_request(),
    };
    let catalog = match current_catalog(&state).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let token = match state.auth.issue_submission(&session_id) {
        Ok(value) => value,
        Err(_) => return unavailable(),
    };
    let Some(html) = render::chat_page(
        &catalog.models,
        &session.csrf,
        &token,
        &history,
        Some("Delivery confirmed."),
        None,
        MAX_RENDERED_RESPONSE_BYTES,
    ) else {
        return unavailable();
    };
    if state.accounting.settle(submission_id).is_err() {
        return unavailable();
    }
    Html(html).into_response()
}

fn parse_history(encoded: &str) -> Option<Vec<Message>> {
    let history: Vec<Message> = serde_json::from_str(encoded).ok()?;
    if history.iter().any(|message| {
        !matches!(message.role.as_str(), "user" | "assistant") || message.content.is_empty()
    }) {
        return None;
    }
    Some(history)
}

struct ReservationGuard {
    accounting: Arc<Accounting>,
    submission_id: [u8; 32],
    armed: bool,
}

impl ReservationGuard {
    fn new(accounting: Arc<Accounting>, submission_id: [u8; 32]) -> Self {
        Self {
            accounting,
            submission_id,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ReservationGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.accounting.refund(self.submission_id);
        }
    }
}

fn verified_gateway_evidence(
    state: &AppState,
) -> Result<crate::attestation::GatewayEvidence, crate::attestation::EvidenceError> {
    let evidence = load_evidence(state.gateway_evidence_path.as_ref())?;
    state
        .gateway_evidence_verifier
        .verify(&evidence, now_unix())?;
    Ok(evidence)
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

fn login_page_response(state: &AppState, error: Option<&str>) -> Response {
    let challenge = match state.auth.issue_login_challenge() {
        Ok(value) => value,
        Err(_) => return unavailable(),
    };
    let cookie = match HeaderValue::from_str(&login_challenge_cookie(&challenge)) {
        Ok(value) => value,
        Err(_) => return internal_error(),
    };
    let mut response = Html(render::login_page(&challenge, error)).into_response();
    response.headers_mut().insert(header::SET_COOKIE, cookie);
    response
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    use subtle::ConstantTimeEq;
    left.len() == right.len() && bool::from(left.ct_eq(right))
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
