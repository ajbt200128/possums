use crate::telemetry::{
    hooks::{HttpObservation, Lease, RequestContext},
    AdmissionModel, Endpoint, HttpTerminal, Lane, Rejection as MetricRejection,
};
use crate::{
    accounting::{Accounting, AccountingError, Outcome},
    attestation::{EvidenceVerifier, GatewayEvidence},
    auth::{
        clear_session_cookie, login_challenge_cookie, session_cookie, AdmissionError, Auth,
        AuthError, Session,
    },
    generation::{now_unix, Generation, GenerationInput, Rejection, Submission},
    inference::{authenticated_catalog, Message, SharedInference},
    render,
    streaming_chat::{self, AcceptedChat},
};
use axum::{
    body::{Body, Bytes, HttpBody},
    extract::{rejection::FormRejection, DefaultBodyLimit, Extension, Form, State},
    http::{header, HeaderMap, HeaderValue, Method, Request, StatusCode},
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
use serde::{Deserialize, Serialize};
use std::{
    future::poll_fn,
    pin::Pin,
    sync::{atomic::AtomicUsize, Arc},
    task::{Context, Poll},
    time::Duration,
};
use tokio::{net::TcpListener, sync::Semaphore};
use tower_http::catch_panic::CatchPanicLayer;

#[cfg(test)]
pub(crate) mod resource_streaming_tests;

pub const BODY_LIMIT: usize = 8 * 1024 * 1024;
const BODY_DEADLINE: Duration = Duration::from_secs(30);
const HEADER_DEADLINE: Duration = Duration::from_secs(10);
const CONNECTION_DEADLINE: Duration = Duration::from_secs(10 * 60);
const MAX_CONNECTIONS: usize = 64;
const MAX_HEADERS: usize = 64;
const MAX_HEADER_BYTES: usize = 32 * 1024;
// Scoped admission targets, not measured RSS or a whole-process memory proof:
// 4 * (104 MiB heavy + 16 MiB ingress) + 16 MiB New chat + 16 MiB controls.
// Heavy is acquired BEFORE raw collection/decoding and covers decoded form/JSON,
// tokenizer, streaming inference and rendering overlap. Ingress covers raw input
// storage; it is NOT a standalone allowance for decoded history. SDK/TLS, shared
// auth/accounting state and allocator overhead are outside these envelopes.
const CHAT_LANES: usize = 4;
const NEW_CHAT_LANES: usize = 1;
const CONTROL_LANES: usize = 1;
// Native control forms contain only CSRF/credentials (43/128 bytes), never
// conversation history. Apply this ceiling to fallbacks/wrong methods as well.
const CONTROL_BODY_LIMIT: usize = 4 * 1024;
const MAX_CONTROL_RENDERED_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const MAX_ATTESTATION_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

/// Fixed configured resource bounds, not currently available permits.
/// Generation inherits the heavy-memory bound; it has no separate count quota.
pub fn admission_capacities() -> [(Lane, u64); 6] {
    [
        (Lane::Connection, MAX_CONNECTIONS as u64),
        (Lane::Generation, CHAT_LANES as u64),
        (Lane::Heavy, CHAT_LANES as u64),
        (Lane::Ingress, CHAT_LANES as u64),
        (Lane::NewChat, NEW_CHAT_LANES as u64),
        (Lane::Control, CONTROL_LANES as u64),
    ]
}

#[derive(Clone)]
pub struct AppState {
    pub auth: Arc<Auth>,
    pub accounting: Arc<Accounting>,
    pub inference: SharedInference,
    pub gateway_evidence_path: Arc<str>,
    gateway_evidence_verifier: Arc<dyn EvidenceVerifier>,
    chat_memory: Arc<Semaphore>,
    chat_ingress: Arc<Semaphore>,
    new_chat_memory: Arc<Semaphore>,
    control_memory: Arc<Semaphore>,
    generation_activity: Arc<AtomicUsize>,
    telemetry: Option<Arc<crate::telemetry::AggregateMetrics>>,
    #[cfg(test)]
    preflight_hooks: Arc<resource_streaming_tests::PreflightHooks>,
}

impl AppState {
    pub fn with_telemetry(
        mut self,
        metrics: Option<Arc<crate::telemetry::AggregateMetrics>>,
    ) -> Self {
        self.telemetry = metrics;
        self
    }
    // Test-only observation of lifetime headroom, not an admission semaphore.
    #[cfg(test)]
    pub(crate) fn generation_headroom(&self) -> usize {
        CHAT_LANES.saturating_sub(
            self.generation_activity
                .load(std::sync::atomic::Ordering::Relaxed),
        )
    }
    #[cfg(test)]
    pub(crate) fn available_lanes(&self) -> [usize; 5] {
        [
            self.generation_headroom(),
            self.chat_memory.available_permits(),
            self.chat_ingress.available_permits(),
            self.new_chat_memory.available_permits(),
            self.control_memory.available_permits(),
        ]
    }
    pub(crate) fn generation(&self) -> Generation<'_> {
        Generation {
            auth: &self.auth,
            accounting: &self.accounting,
            inference: &self.inference,
            evidence_path: &self.gateway_evidence_path,
            evidence_verifier: &self.gateway_evidence_verifier,
            metrics: self.telemetry.as_ref(),
            observation: Default::default(),
            activity: &self.generation_activity,
            #[cfg(test)]
            hooks: &self.preflight_hooks,
        }
    }

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
            chat_memory: Arc::new(Semaphore::new(CHAT_LANES)),
            chat_ingress: Arc::new(Semaphore::new(CHAT_LANES)),
            new_chat_memory: Arc::new(Semaphore::new(NEW_CHAT_LANES)),
            control_memory: Arc::new(Semaphore::new(CONTROL_LANES)),
            generation_activity: Arc::default(),
            telemetry: None,
            #[cfg(test)]
            preflight_hooks: Arc::default(),
        }
    }
}

pub fn router(state: AppState) -> Router {
    router_with_body_deadline(state, BODY_DEADLINE)
}

#[doc(hidden)]
pub fn router_with_body_deadline(state: AppState, body_deadline: Duration) -> Router {
    Router::new()
        .merge(crate::api::routes())
        .route("/", get(home))
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/chat", post(chat))
        .route("/chat/new", post(new_chat))
        .route("/recovery", get(recovery))
        .route("/recovery/download", get(recovery_download))
        .route("/claims", get(claims))
        .route("/attestation", get(attestation))
        .method_not_allowed_fallback(wrong_method)
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            move |state, request, next| total_body_deadline(state, request, next, body_deadline),
        ))
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
    let telemetry = state.telemetry.clone();
    let app = router(state);
    loop {
        let (stream, _) = listener.accept().await?;
        let Ok(permit) = connections.clone().try_acquire_owned() else {
            if let Some(metrics) = &telemetry {
                metrics.reject(
                    None,
                    AdmissionModel::NotApplicable,
                    MetricRejection::ConnectionCapacity,
                );
            }
            drop(stream);
            continue;
        };
        let permit = Lease::observed(permit, telemetry.as_ref(), Lane::Connection);
        let entered = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let seen = entered.clone();
        let service = TowerToHyperService::new(app.clone());
        let service = hyper::service::service_fn(move |request| {
            seen.store(true, std::sync::atomic::Ordering::Release);
            let service = service.clone();
            async move { hyper::service::Service::call(&service, request).await }
        });
        let metrics = telemetry.clone();
        tokio::spawn(async move {
            let mut builder = http1::Builder::new();
            builder
                .timer(TokioTimer::new())
                .header_read_timeout(header_deadline)
                .max_headers(MAX_HEADERS)
                .max_buf_size(MAX_HEADER_BYTES);
            let connection = builder.serve_connection(TokioIo::new(stream), service);
            let result = tokio::time::timeout(CONNECTION_DEADLINE, connection).await;
            if !entered.load(std::sync::atomic::Ordering::Acquire) {
                let reason = match result {
                    Err(_) => Some(MetricRejection::ConnectionDeadline),
                    Ok(Err(error)) if error.is_timeout() => {
                        Some(MetricRejection::ConnectionDeadline)
                    }
                    Ok(Err(error)) if error.is_parse() => Some(MetricRejection::HeaderProtocol),
                    Ok(Err(_)) => Some(MetricRejection::TransportUnknown),
                    Ok(Ok(())) => None, // clean idle close is not a rejection
                };
                if let (Some(metrics), Some(reason)) = (&metrics, reason) {
                    metrics.reject(None, AdmissionModel::NotApplicable, reason);
                }
            }
            drop(permit);
        });
    }
}

async fn wrong_method(Extension(observation): Extension<RequestContext>) -> StatusCode {
    observation.reject(AdmissionModel::NotApplicable, MetricRejection::Input);
    StatusCode::METHOD_NOT_ALLOWED
}

async fn request_admission(
    State(state): State<AppState>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    // Match the router's exact method/path semantics; query strings do not
    // affect routing. Wrong methods, trailing slashes and encoded aliases stay
    // in the bounded control lane and cannot reach a heavy handler.
    let endpoint = Endpoint::route(request.method().as_str(), request.uri().path());
    let (observation, context) = HttpObservation::new(state.telemetry.as_ref(), endpoint);
    request.extensions_mut().insert(context.clone());
    let (lane, label) = match (request.method(), request.uri().path()) {
        (&Method::POST, "/chat" | "/v1/chat/completions") => (&state.chat_memory, Lane::Heavy),
        (&Method::POST, "/chat/new") => (&state.new_chat_memory, Lane::NewChat),
        _ => (&state.control_memory, Lane::Control),
    };
    let api = crate::api::is_api(request.uri().path());
    let Ok(permit) = lane.clone().try_acquire_owned() else {
        context.reject(AdmissionModel::Unknown, MetricRejection::RequestCapacity);
        let response = if api {
            crate::api::error(StatusCode::SERVICE_UNAVAILABLE)
        } else {
            (StatusCode::SERVICE_UNAVAILABLE, "service busy").into_response()
        };
        return observed_response(response, None, observation);
    };
    let lease = Arc::new(Lease::observed(permit, state.telemetry.as_ref(), label));
    if request.method() == Method::POST
        && matches!(request.uri().path(), "/chat" | "/v1/chat/completions")
    {
        // Detached preflight, generation and delivery share THIS heavy admission.
        request.extensions_mut().insert(lease.clone());
    }
    let response = if api && (request.uri().query().is_some() || request.method() == Method::HEAD) {
        context.reject(AdmissionModel::Unknown, MetricRejection::Input);
        crate::api::error(StatusCode::BAD_REQUEST)
    } else {
        next.run(request).await
    };
    // Normalize even framework, panic, header and body-limit failures, while
    // retaining the SAME admission lease through the replacement response.
    let response = if api
        && !response.status().is_success()
        && response
            .extensions()
            .get::<crate::api::SafeError>()
            .is_none()
    {
        crate::api::error(response.status())
    } else {
        response
    };
    observed_response(response, Some(lease), observation)
}

pub(crate) fn observed_response(
    response: Response,
    lease: Option<Arc<Lease>>,
    mut observation: HttpObservation,
) -> Response {
    observation.status(response.status().as_u16());
    let (parts, body) = response.into_parts();
    if body.is_end_stream() {
        observation.finish(HttpTerminal::Eof);
    }
    Response::from_parts(
        parts,
        Body::new(AdmissionBody {
            inner: body,
            lease,
            observation,
        }),
    )
}

struct AdmissionBody {
    inner: Body,
    lease: Option<Arc<Lease>>,
    observation: HttpObservation,
}

// Data drops BEFORE its lease. from_owner preserves the lease across frame
// dequeue, body/EOF drop, Bytes clones and even one-byte slices, without copying
// payload. The same owner is used for raw request bytes through Form decoding.
struct AdmittedBytes {
    data: Bytes,
    _lease: Arc<crate::telemetry::hooks::Lease>,
}

impl AsRef<[u8]> for AdmittedBytes {
    fn as_ref(&self) -> &[u8] {
        &self.data
    }
}

impl HttpBody for AdmissionBody {
    type Data = Bytes;
    type Error = axum::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        let frame = std::task::ready!(Pin::new(&mut self.inner).poll_frame(context));
        match &frame {
            None => self.observation.finish(HttpTerminal::Eof),
            Some(Err(_)) => self.observation.finish(HttpTerminal::Error),
            _ if self.inner.is_end_stream() => self.observation.finish(HttpTerminal::Eof),
            _ => {}
        }
        Poll::Ready(frame.map(|result| {
            result.map(|frame| {
                frame.map_data(|data| match &self.lease {
                    Some(lease) => Bytes::from_owner(AdmittedBytes {
                        data,
                        _lease: lease.clone(),
                    }),
                    None => data,
                })
            })
        }))
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> http_body::SizeHint {
        self.inner.size_hint()
    }
}

async fn total_body_deadline(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
    deadline: Duration,
) -> Response {
    let observation = request
        .extensions()
        .get::<RequestContext>()
        .cloned()
        .unwrap_or_default();
    let chat = request.method() == Method::POST
        && matches!(request.uri().path(), "/chat" | "/v1/chat/completions");
    // The outer heavy permit is already owned. Failure here rolls it back
    // through the bounded error response, with no waiter or body polling.
    let ingress = if chat {
        match state.chat_ingress.clone().try_acquire_owned() {
            Ok(permit) => Some(Arc::new(Lease::observed(
                permit,
                state.telemetry.as_ref(),
                Lane::Ingress,
            ))),
            Err(_) => {
                observation.reject(AdmissionModel::Unknown, MetricRejection::IngressCapacity);
                return (StatusCode::SERVICE_UNAVAILABLE, "service busy").into_response();
            }
        }
    } else {
        None
    };
    let limit = if chat { BODY_LIMIT } else { CONTROL_BODY_LIMIT };
    let (parts, body) = request.into_parts();
    let bytes = match tokio::time::timeout(deadline, collect_request(body, limit)).await {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(())) => {
            observation.reject(AdmissionModel::Unknown, MetricRejection::Input);
            return (StatusCode::PAYLOAD_TOO_LARGE, "request body too large").into_response();
        }
        Err(_) => {
            observation.reject(AdmissionModel::Unknown, MetricRejection::Input);
            return (StatusCode::REQUEST_TIMEOUT, "request body timed out").into_response();
        }
    };
    let bytes = match ingress {
        Some(lease) => Bytes::from_owner(AdmittedBytes {
            data: bytes,
            _lease: lease,
        }),
        None => bytes,
    };
    #[cfg(test)]
    if chat {
        state.preflight_hooks.resources.raw(&bytes);
        state.preflight_hooks.resources.at("ingress").await;
    }
    next.run(Request::from_parts(parts, Body::from(bytes)))
        .await
}

async fn collect_request(mut body: Body, limit: usize) -> Result<Bytes, ()> {
    // Do not collect a list of frames: an 8-MiB one-byte-fragmented request
    // otherwise retains millions of frame headers before coalescing. One fixed
    // allocation, one borrowed incoming frame, no growth/copy-overlap or queue.
    let mut bytes = Vec::with_capacity(limit);
    while let Some(frame) = poll_fn(|cx| Pin::new(&mut body).poll_frame(cx)).await {
        let frame = frame.map_err(|_| ())?;
        if let Ok(data) = frame.into_data() {
            if data.len() > limit - bytes.len() {
                return Err(());
            }
            bytes.extend_from_slice(&data);
        }
    }
    Ok(Bytes::from(bytes))
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
    if header_bytes > MAX_HEADER_BYTES || request.headers().len() > MAX_HEADERS {
        if let Some(observation) = request.extensions().get::<RequestContext>() {
            observation.reject(AdmissionModel::Unknown, MetricRejection::Input);
        }
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

async fn home(
    State(state): State<AppState>,
    Extension(observation): Extension<RequestContext>,
    headers: HeaderMap,
) -> Response {
    let Some((session_id, session)) = session_from_headers(&state, &headers) else {
        return login_page_response(&state, None, &observation);
    };
    let catalog = match current_catalog(&state, &observation).await {
        Ok(catalog) => catalog,
        Err(response) => return response,
    };
    // Quote the complete authenticated catalog with admission's exact formula.
    // One unusable quote fails the whole display, never hides a supported model.
    let quotes = match catalog
        .models
        .iter()
        .map(|model| catalog.reservation_quote(&model.id))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(quotes) => quotes,
        Err(_) => {
            observation.reject(AdmissionModel::NotApplicable, MetricRejection::Catalog);
            return unavailable();
        }
    };
    let Some(available_microunits) = state.accounting.available(&session.account_id) else {
        observation.reject(AdmissionModel::NotApplicable, MetricRejection::Internal);
        return unavailable();
    };
    match state.auth.issue_submission_for(
        &session_id,
        session.conversation,
        session.selected_model.as_deref(),
    ) {
        Ok(token) => render::chat_page(
            render::CatalogSnapshot {
                quotes: &quotes,
                available_microunits,
            },
            &session.csrf,
            &token,
            &[],
            session.selected_model.as_deref(),
            None,
            // Validated catalog: <=256 IDs of <=128 safe ASCII bytes;
            // empty history. The renderer preflight bounds intermediates
            // before allocation without excluding any supported model.
            MAX_CONTROL_RENDERED_RESPONSE_BYTES,
        )
        .map_or_else(
            || {
                observation.reject(AdmissionModel::NotApplicable, MetricRejection::Internal);
                unavailable()
            },
            |html| Html(html).into_response(),
        ),
        Err(error) => {
            observe_auth_rejection(&observation, error);
            unavailable()
        }
    }
}

#[derive(Deserialize)]
struct LoginForm {
    csrf: String,
    credential: String,
}

async fn login(
    State(state): State<AppState>,
    Extension(observation): Extension<RequestContext>,
    headers: HeaderMap,
    form: Result<Form<LoginForm>, FormRejection>,
) -> Response {
    let form = match control_form(form, &observation) {
        Ok(form) => form,
        Err(rejection) => return rejection.into_response(),
    };
    let Some(cookie_challenge) = cookie_value(&headers, "possums_login_csrf") else {
        observation.reject(AdmissionModel::NotApplicable, MetricRejection::Auth);
        return unauthorized();
    };
    if !constant_time_equal(cookie_challenge.as_bytes(), form.csrf.as_bytes()) {
        observation.reject(AdmissionModel::NotApplicable, MetricRejection::Auth);
        return unauthorized();
    }
    match state.auth.authenticate(&form.credential, &form.csrf) {
        Ok((id, _)) => {
            let mut response = Redirect::to("/").into_response();
            if let Ok(value) = HeaderValue::from_str(&session_cookie(&id)) {
                response.headers_mut().insert(header::SET_COOKIE, value);
                response
            } else {
                observation.reject(AdmissionModel::NotApplicable, MetricRejection::Internal);
                internal_error()
            }
        }
        Err(error) => {
            observe_auth_rejection(&observation, error);
            (StatusCode::UNAUTHORIZED, "invalid credential").into_response()
        }
    }
}

#[derive(Deserialize)]
struct CsrfForm {
    csrf: String,
}

async fn logout(
    State(state): State<AppState>,
    Extension(observation): Extension<RequestContext>,
    headers: HeaderMap,
    form: Result<Form<CsrfForm>, FormRejection>,
) -> Response {
    let form = match control_form(form, &observation) {
        Ok(form) => form,
        Err(rejection) => return rejection.into_response(),
    };
    let Some((id, _)) = session_from_headers(&state, &headers) else {
        observation.reject(AdmissionModel::NotApplicable, MetricRejection::Auth);
        return unauthorized();
    };
    if let Err(error) = state.auth.logout(&id, &form.csrf) {
        observe_auth_rejection(&observation, error);
        return unauthorized();
    }
    let mut response = Redirect::to("/").into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_static(clear_session_cookie()),
    );
    response
}

// Render the complete selector in this lane, without entering GET / admission.
// No availability promise applies while the New chat lane itself is retained.
async fn new_chat(
    State(state): State<AppState>,
    Extension(observation): Extension<RequestContext>,
    headers: HeaderMap,
    form: Result<Form<CsrfForm>, FormRejection>,
) -> Response {
    let form = match control_form(form, &observation) {
        Ok(form) => form,
        Err(rejection) => return rejection.into_response(),
    };
    let Some(id) = cookie_value(&headers, "possums_session") else {
        observation.reject(AdmissionModel::NotApplicable, MetricRejection::Auth);
        return unauthorized();
    };
    if let Err(error) = state.auth.new_chat(&id, &form.csrf) {
        observe_auth_rejection(&observation, error);
        return unauthorized();
    }
    home(State(state), Extension(observation), headers).await
}

fn control_form<T>(
    form: Result<Form<T>, FormRejection>,
    observation: &RequestContext,
) -> Result<T, FormRejection> {
    form.map(|Form(form)| form).inspect_err(|_| {
        observation.reject(AdmissionModel::NotApplicable, MetricRejection::Input);
    })
}

fn observe_auth_rejection(observation: &RequestContext, error: AuthError) {
    observation.reject(
        AdmissionModel::NotApplicable,
        match error {
            AuthError::Invalid => MetricRejection::Auth,
            _ => MetricRejection::Internal,
        },
    );
}

async fn recovery(
    State(state): State<AppState>,
    Extension(observation): Extension<RequestContext>,
    headers: HeaderMap,
) -> Response {
    let Some((_, session)) = session_from_headers(&state, &headers) else {
        observation.reject(AdmissionModel::NotApplicable, MetricRejection::Auth);
        return unauthorized();
    };
    Html(render::page(&format!(
        "<h1>Recovery credential</h1><p>Copy or download this credential now. Anyone holding it controls this demo account.</p><pre>{}</pre><p><a href=/recovery/download download=possums-recovery.txt>Download recovery credential</a></p><p><a href=/>Return</a></p>",
        render::escape(&session.recovery_credential)
    )))
    .into_response()
}

async fn recovery_download(
    State(state): State<AppState>,
    Extension(observation): Extension<RequestContext>,
    headers: HeaderMap,
) -> Response {
    let Some((_, session)) = session_from_headers(&state, &headers) else {
        observation.reject(AdmissionModel::NotApplicable, MetricRejection::Auth);
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

#[derive(Serialize)]
struct AttestationDocuments<'a> {
    gateway: &'a GatewayEvidence,
    upstream: &'a serde_json::Value,
}

async fn attestation(
    State(state): State<AppState>,
    Extension(observation): Extension<RequestContext>,
) -> Response {
    let gateway = match verified_gateway_evidence(&state).await {
        Ok(value) => value,
        Err(_) => {
            observation.reject(AdmissionModel::NotApplicable, MetricRejection::Verification);
            return unavailable();
        }
    };
    let upstream = match state.inference.verification_document() {
        Ok(value) => value,
        Err(_) => {
            observation.reject(AdmissionModel::NotApplicable, MetricRejection::Internal);
            return unavailable();
        }
    };
    let bytes = match crate::bounded_json::to_vec(
        &AttestationDocuments {
            gateway: &gateway,
            upstream: &upstream,
        },
        MAX_ATTESTATION_RESPONSE_BYTES,
    ) {
        Ok(value) => value,
        Err(_) => {
            observation.reject(AdmissionModel::NotApplicable, MetricRejection::Internal);
            return unavailable();
        }
    };
    let mut response = Body::from(bytes).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    response
}

// Reserve before either prompt-bearing call. Acceptance transfers one guard to
// detached preflight, then synchronously to compose even after observer loss.
// Session -> submission -> accounting lock order stays inside admit_submission.
// The submitted quote never changes; tokenization only sets context-legal output.
// Delivery (64 KiB/eight outstanding frame owners) cannot cancel/reprice work.
async fn chat(
    State(state): State<AppState>,
    Extension(heavy): Extension<Arc<crate::telemetry::hooks::Lease>>,
    Extension(observation): Extension<RequestContext>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !headers
        .get(header::CONTENT_TYPE)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|h| {
            h.split(';').next().is_some_and(|media| {
                media
                    .trim()
                    .eq_ignore_ascii_case("application/x-www-form-urlencoded")
            })
        })
    {
        observation.reject(AdmissionModel::Unknown, MetricRejection::Input);
        return (
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "form content type required",
        )
            .into_response();
    }
    let form = match decode_continuation(&body) {
        Ok(form) => form,
        Err(_) => {
            observation.reject(AdmissionModel::Unknown, MetricRejection::Input);
            return bad_request();
        }
    };
    #[cfg(test)]
    {
        state
            .preflight_hooks
            .resources
            .input("decoded", &form.history, &form.prompt);
        state.preflight_hooks.resources.at("decoded").await;
    }
    drop(body); // Decoded storage is heavy-owned; the raw ingress owner ends here.
    let Some((session_id, session)) = session_from_headers(&state, &headers) else {
        observation.reject(AdmissionModel::Unknown, MetricRejection::Auth);
        return unauthorized();
    };
    if !Auth::verify_csrf(&session, &form.csrf) {
        observation.reject(AdmissionModel::Unknown, MetricRejection::Auth);
        return bad_request();
    }
    if form.prompt.is_empty() {
        observation.reject(AdmissionModel::Unknown, MetricRejection::Input);
        return bad_request();
    }
    // Only small renderer/admission metadata is copied; history/prompt move once.
    let model = form.model.clone();
    let render_session_id = session_id.clone();
    let csrf = form.csrf.clone();
    let auth = state.auth.clone();
    let inference = state.inference.clone();
    let accounting = state.accounting.clone();
    let render_heavy = heavy.clone();
    #[cfg(test)]
    let resource_hooks = Some(state.preflight_hooks.resources.clone());
    let result = state
        .generation()
        .observed(observation)
        .submit(
            Submission {
                session_id: &session_id,
                csrf: &form.csrf,
                token: &form.token,
            },
            GenerationInput {
                model: form.model,
                history: form.history,
                prompt: form.prompt,
            },
            heavy,
            move |owner, prepared| {
                let reserved_microunits = prepared.reserved_microunits;
                let input = AcceptedChat::new(
                    prepared,
                    render_session_id,
                    csrf,
                    #[cfg(test)]
                    resource_hooks,
                );
                let (body, observer) = streaming_chat::compose_with_credit(
                    owner,
                    render_heavy,
                    input,
                    auth,
                    inference,
                    streaming_chat::ContinuationCredit {
                        accounting,
                        account_id: session.account_id,
                        reserved_microunits,
                    },
                );
                drop(observer);
                body
            },
        )
        .await;
    match result {
        Ok(body) => {
            let mut response = Body::new(body).into_response();
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/html; charset=utf-8"),
            );
            response
        }
        Err(Rejection::Context) => {
            (StatusCode::BAD_REQUEST, "selected model context exceeded").into_response()
        }
        Err(Rejection::InvalidModel | Rejection::Admission(AdmissionError::Auth(_))) => {
            bad_request()
        }
        Err(Rejection::Admission(AdmissionError::Accounting(
            AccountingError::InsufficientCredit,
        ))) => (
            StatusCode::PAYMENT_REQUIRED,
            "insufficient demo credit for selected model maximum reservation",
        )
            .into_response(),
        Err(Rejection::Admission(AdmissionError::Accounting(AccountingError::Concurrency))) => (
            StatusCode::TOO_MANY_REQUESTS,
            "account concurrency limit reached",
        )
            .into_response(),
        Err(Rejection::Duplicate(outcome)) => {
            let notice = match outcome {
                Outcome::InFlight => "This submission is already in progress.",
                Outcome::Settled { .. } => {
                    "This submission already completed and was not regenerated."
                }
                Outcome::Refunded => {
                    "This submission failed and was refunded; start a New chat to retry."
                }
            };
            Html(render::page(&format!(
                "<p role=status>{notice}</p>{}",
                render::chat_controls(&session.csrf, Some(&model))
            )))
            .into_response()
        }
        Err(Rejection::Upstream(failure)) => {
            (StatusCode::SERVICE_UNAVAILABLE, failure.to_string()).into_response()
        }
        Err(Rejection::Unavailable | Rejection::Admission(AdmissionError::Accounting(_))) => {
            unavailable()
        }
    }
}

/// Owned, bounded decoded input for detached production preflight.
/// No Debug implementation: this value contains prompts and authentication data.
pub struct ContinuationForm {
    pub csrf: String,
    pub token: String,
    pub model: String,
    pub prompt: String,
    pub history: Vec<Message>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidContinuation;

/// Strict application/x-www-form-urlencoded boundary. History field names and
/// base64url values are unescaped ASCII in canonical browser submissions. Reject
/// aliases rather than allocating a map or silently accepting duplicate fields.
pub fn decode_continuation(body: &[u8]) -> Result<ContinuationForm, InvalidContinuation> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use render::{HISTORY_BLOCK_BYTES, MAX_HISTORY_FIELDS};
    use serde::de::{self, Deserializer, SeqAccess, Visitor};
    if body.len() > BODY_LIMIT {
        return Err(InvalidContinuation);
    }
    let body = std::str::from_utf8(body).map_err(|_| InvalidContinuation)?;
    let mut manifest = None;
    let mut fields = 0_usize;
    for field in body.split('&') {
        fields = fields.checked_add(1).ok_or(InvalidContinuation)?;
        if fields > MAX_HISTORY_FIELDS + 5 {
            return Err(InvalidContinuation);
        }
        let (name, value) = field.split_once('=').ok_or(InvalidContinuation)?;
        if name == "history_manifest" && manifest.replace(value).is_some() {
            return Err(InvalidContinuation);
        }
    }
    let manifest = manifest.ok_or(InvalidContinuation)?;
    if manifest.len() != 17 {
        return Err(InvalidContinuation);
    }
    let parts: Vec<_> = manifest.split('.').collect(); // At most 17 bytes.
    if parts.len() != 3 || parts[0] != "1" {
        return Err(InvalidContinuation);
    }
    let count: usize = parts[1].parse().map_err(|_| InvalidContinuation)?;
    let decoded_len: usize = parts[2].parse().map_err(|_| InvalidContinuation)?;
    if manifest != format!("1.{count:06}.{decoded_len:08}")
        || count == 0
        || count > MAX_HISTORY_FIELDS
        || decoded_len > render::max_history_decoded_bytes()
        || decoded_len.div_ceil(HISTORY_BLOCK_BYTES) != count
    {
        return Err(InvalidContinuation);
    }
    let mut json = Vec::with_capacity(decoded_len);
    let (mut csrf, mut token, mut model, mut prompt) = (None, None, None, None);
    let mut index = 0_usize;
    for field in body.split('&') {
        let (name, value) = field.split_once('=').ok_or(InvalidContinuation)?;
        let (slot, limit) = match name {
            "csrf" => (&mut csrf, render::TOKEN_FIELD_BYTES),
            "token" => (&mut token, render::TOKEN_FIELD_BYTES),
            "model" => (&mut model, render::MAX_MODEL_FIELD_BYTES),
            "prompt" => (&mut prompt, BODY_LIMIT),
            "history_manifest" => continue,
            _ => {
                if index >= count || name != format!("h{index:06}") {
                    return Err(InvalidContinuation);
                }
                let expected = (decoded_len - json.len()).min(HISTORY_BLOCK_BYTES);
                if value.len() != (expected * 4).div_ceil(3) {
                    return Err(InvalidContinuation);
                }
                let mut block = [0; HISTORY_BLOCK_BYTES];
                let size = URL_SAFE_NO_PAD
                    .decode_slice(value, &mut block)
                    .map_err(|_| InvalidContinuation)?;
                if size != expected || URL_SAFE_NO_PAD.encode(&block[..size]) != value {
                    return Err(InvalidContinuation);
                }
                json.extend_from_slice(&block[..size]);
                index += 1;
                continue;
            }
        };
        if slot.is_some() {
            return Err(InvalidContinuation);
        }
        *slot = Some(decode_form_value(value, limit)?);
    }
    if index != count || json.len() != decoded_len {
        return Err(InvalidContinuation);
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct HistoryMessage {
        role: String,
        content: String,
    }
    struct HistoryVisitor;
    impl<'de> Visitor<'de> for HistoryVisitor {
        type Value = Vec<Message>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("alternating user/assistant history")
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
            // Do not reserve from an untrusted count or collect an intermediate
            // vector. Validate each strict object/sequence entry BEFORE pushing.
            let mut history = Vec::new();
            while let Some(message) = sequence.next_element::<HistoryMessage>()? {
                let valid = if history.len().is_multiple_of(2) {
                    message.role == "user" && !message.content.is_empty()
                } else {
                    message.role == "assistant"
                };
                if !valid {
                    return Err(de::Error::custom("invalid history"));
                }
                history.push(Message {
                    role: message.role,
                    content: message.content,
                });
            }
            if !history.len().is_multiple_of(2) {
                return Err(de::Error::custom("invalid history"));
            }
            Ok(history)
        }
    }
    // Retain serde's depth limit and strict fields, including sequence-form
    // compatibility. Valid prefixes still allocate; admission must budget JSON,
    // strings and vector capacity/growth, not just the final logical length.
    let mut deserializer = serde_json::Deserializer::from_slice(&json);
    let history = deserializer
        .deserialize_seq(HistoryVisitor)
        .map_err(|_| InvalidContinuation)?;
    deserializer.end().map_err(|_| InvalidContinuation)?;
    let form = ContinuationForm {
        csrf: csrf.ok_or(InvalidContinuation)?,
        token: token.ok_or(InvalidContinuation)?,
        model: model.ok_or(InvalidContinuation)?,
        prompt: prompt.ok_or(InvalidContinuation)?,
        history,
    };
    if !render::valid_form_token(&form.csrf)
        || !render::valid_form_token(&form.token)
        || !render::valid_form_model(&form.model)
        || form.prompt.is_empty()
    {
        return Err(InvalidContinuation);
    }
    Ok(form)
}

fn decode_form_value(value: &str, limit: usize) -> Result<String, InvalidContinuation> {
    if value.len() > limit.checked_mul(3).ok_or(InvalidContinuation)? {
        return Err(InvalidContinuation);
    }
    let mut bytes = Vec::with_capacity(value.len().min(limit));
    let mut input = value.bytes();
    while let Some(byte) = input.next() {
        if bytes.len() == limit {
            return Err(InvalidContinuation);
        }
        bytes.push(match byte {
            b'+' => b' ',
            b'%' => {
                let hi = (input.next().ok_or(InvalidContinuation)? as char)
                    .to_digit(16)
                    .ok_or(InvalidContinuation)?;
                let lo = (input.next().ok_or(InvalidContinuation)? as char)
                    .to_digit(16)
                    .ok_or(InvalidContinuation)?;
                (hi * 16 + lo) as u8
            }
            other => other,
        });
    }
    String::from_utf8(bytes).map_err(|_| InvalidContinuation)
}

async fn verified_gateway_evidence(
    state: &AppState,
) -> Result<crate::attestation::GatewayEvidence, crate::attestation::EvidenceError> {
    state.generation().evidence().await
}

async fn current_catalog(
    state: &AppState,
    observation: &RequestContext,
) -> Result<crate::catalog::Catalog, Response> {
    authenticated_catalog(state.inference.as_ref(), now_unix())
        .await
        .map_err(|error| {
            use crate::inference::InferenceFailure;
            observation.reject(
                AdmissionModel::NotApplicable,
                match error.failure() {
                    InferenceFailure::VerificationFailed
                    | InferenceFailure::EndpointBindingFailed => MetricRejection::Verification,
                    InferenceFailure::CatalogFailed => MetricRejection::Catalog,
                    _ => MetricRejection::Internal,
                },
            );
            unavailable()
        })
}

fn login_page_response(
    state: &AppState,
    error: Option<&str>,
    observation: &RequestContext,
) -> Response {
    let challenge = match state.auth.issue_login_challenge() {
        Ok(value) => value,
        Err(error) => {
            observe_auth_rejection(observation, error);
            return unavailable();
        }
    };
    let cookie = match HeaderValue::from_str(&login_challenge_cookie(&challenge)) {
        Ok(value) => value,
        Err(_) => {
            observation.reject(AdmissionModel::NotApplicable, MetricRejection::Internal);
            return internal_error();
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        attestation::{EvidenceError, GatewayEvidence},
        catalog::Model,
        inference::{Inference, InferenceError},
    };
    use async_trait::async_trait;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use sha2::{Digest, Sha256};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::Notify;
    use tower::ServiceExt;

    #[derive(Default)]
    struct PermitProbe {
        prompts: AtomicUsize,
        entered: Notify,
        release: Notify,
    }

    #[async_trait]
    impl Inference for PermitProbe {
        async fn catalog(&self) -> Result<Vec<u8>, InferenceError> {
            Ok(br#"{"object":"list","data":[{"id":"m","type":"chat","context_window":20,"endpoints":["/v1/chat/completions"],"pricing":{"inputTokenPricePer1M":1,"outputTokenPricePer1M":1,"requestPrice":0}}]}"#.to_vec())
        }
        async fn count_tokens(
            &self,
            _: &str,
            _: &[Message],
            _heavy: std::sync::Arc<crate::telemetry::hooks::Lease>,
        ) -> Result<u64, InferenceError> {
            self.prompts.fetch_add(1, Ordering::SeqCst);
            self.entered.notify_one();
            self.release.notified().await;
            Ok(1)
        }

        async fn generate_stream(
            &self,
            _: &Model,
            _: &[Message],
            _heavy: std::sync::Arc<crate::telemetry::hooks::Lease>,
            on_delta: &mut (dyn for<'d> FnMut(&'d str) + Send),
        ) -> Result<crate::inference::stream::StreamUsage, InferenceError> {
            let mut parser = crate::inference::stream::ProtocolParser::default();
            parser.feed(concat!("data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"answer\"},\"finish_reason\":\"stop\"}]}\n\n", "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1,\"total_tokens\":2}}\n\n", "data: [DONE]\n\n").as_bytes(), on_delta).map_err(|_| InferenceError::InvalidResponse)?;
            parser.eof().map_err(|_| InferenceError::InvalidResponse)
        }
        fn verification_document(&self) -> Result<serde_json::Value, InferenceError> {
            Ok(serde_json::json!({"verified":true}))
        }
    }

    #[async_trait]
    impl EvidenceVerifier for PermitProbe {
        async fn verify(&self, _: &str, now: u64) -> Result<GatewayEvidence, EvidenceError> {
            Ok(GatewayEvidence {
                quote: serde_json::json!({"test":true}),
                issued_at_unix: now,
                release_digest: "test".into(),
                endpoint_key_sha256: "test".into(),
                freshness_expires_at_unix: now + 60,
            })
        }
    }

    #[tokio::test]
    async fn chat_extension_shares_admission_without_using_control_or_new_chat_lanes() {
        use http_body_util::BodyExt;

        let hash = URL_SAFE_NO_PAD.encode(Sha256::digest(crate::auth::random_token().as_bytes()));
        let auth = Auth::from_json(&format!(
            r#"[{{"id":"a","credential_sha256":"{hash}","demo_microunits":100}}]"#
        ))
        .unwrap();
        let probe = Arc::new(PermitProbe::default());
        let state = AppState::new(auth, probe.clone(), "unused", probe);
        // Test-only handler observes staged plumbing through raw-body collection;
        // The production handler consumes the same lease when handing off work.
        let app = Router::new()
            .fallback(|request: Request<Body>| async move {
                let lease = request
                    .extensions()
                    .get::<Arc<crate::telemetry::hooks::Lease>>();
                assert_eq!(
                    lease.is_some(),
                    request.method() == Method::POST && request.uri().path() == "/chat"
                );
                let mut response = "frame".into_response();
                if let Some(lease) = lease {
                    response.extensions_mut().insert(lease.clone());
                }
                response
            })
            .layer(middleware::from_fn_with_state(
                state.clone(),
                |state, request, next| total_body_deadline(state, request, next, BODY_DEADLINE),
            ))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                request_admission,
            ));
        let request = |method, uri| {
            Request::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())
                .unwrap()
        };
        let others = state.chat_memory.clone().try_acquire_many_owned(3).unwrap();
        let mut response = app
            .clone()
            .oneshot(request(Method::POST, "/chat?test=1"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK); // Only one free heavy slot needed.
        let lease = response
            .extensions_mut()
            .remove::<Arc<crate::telemetry::hooks::Lease>>()
            .unwrap();
        assert_eq!(state.chat_memory.available_permits(), 0);
        assert_eq!(state.chat_ingress.available_permits(), 4);
        assert_eq!(
            app.clone()
                .oneshot(request(Method::POST, "/chat"))
                .await
                .unwrap()
                .status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        // Heavy saturation leaves both small lanes usable and independent.
        let control = app
            .clone()
            .oneshot(request(Method::GET, "/claims"))
            .await
            .unwrap();
        assert_eq!(control.status(), StatusCode::OK);
        assert_eq!(state.control_memory.available_permits(), 0);
        let new_chat = app
            .clone()
            .oneshot(request(Method::POST, "/chat/new"))
            .await
            .unwrap();
        assert_eq!(new_chat.status(), StatusCode::OK);
        assert_eq!(state.new_chat_memory.available_permits(), 0);
        assert_eq!(
            app.clone()
                .oneshot(request(Method::GET, "/claims"))
                .await
                .unwrap()
                .status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(
            app.clone()
                .oneshot(request(Method::POST, "/chat/new"))
                .await
                .unwrap()
                .status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        drop(control);
        drop(new_chat);
        for (method, uri) in [
            (Method::GET, "/chat"),
            (Method::POST, "/chat/"),
            (Method::POST, "/%63hat"),
        ] {
            let control = app.clone().oneshot(request(method, uri)).await.unwrap();
            assert_eq!(control.status(), StatusCode::OK);
            assert_eq!(state.control_memory.available_permits(), 0);
            assert_eq!(state.new_chat_memory.available_permits(), 1);
            drop(control);
        }
        let mut body = response.into_body();
        let frame = body.frame().await.unwrap().unwrap().into_data().unwrap();
        let clone = frame.clone();
        let slice = clone.slice(1..2);
        drop(frame);
        drop(clone);
        drop(body);
        assert_eq!(state.chat_memory.available_permits(), 0);
        drop(slice);
        assert_eq!(state.chat_memory.available_permits(), 0); // Handler extension still pins it.
        drop(lease);
        assert_eq!(state.chat_memory.available_permits(), 1);
        drop(others);
        assert_eq!(state.chat_memory.available_permits(), 4);
        assert_eq!(state.control_memory.available_permits(), 1);
        assert_eq!(state.new_chat_memory.available_permits(), 1);
    }

    #[tokio::test]
    async fn heavy_memory_admission_gates_reservation_and_is_held_during_tokenization() {
        let credential = crate::auth::random_token();
        let hash = URL_SAFE_NO_PAD.encode(Sha256::digest(credential.as_bytes()));
        let auth = Auth::from_json(&format!(
            r#"[{{"id":"a","credential_sha256":"{hash}","demo_microunits":100}}]"#
        ))
        .unwrap();
        let (id, session) = auth
            .authenticate(&credential, &auth.issue_login_challenge().unwrap())
            .unwrap();
        let token = auth.issue_submission(&id).unwrap();
        let probe = Arc::new(PermitProbe::default());
        let state = AppState::new(auth, probe.clone(), "unused", probe.clone());
        let request = || {
            Request::builder()
                .method("POST")
                .uri("/chat")
                .header(header::COOKIE, session_cookie(&id))
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(format!(
                    "csrf={}&token={token}&model=m&h000000=W10&history_manifest=1.000001.00000002&prompt=hello",
                    session.csrf
                )))
                .unwrap()
        };
        // Failed ingress acquisition never polls/decodes a body or reserves.
        // The heavy lease remains on the small error response until dropped.
        let ingress = state
            .chat_ingress
            .clone()
            .try_acquire_many_owned(4)
            .unwrap();
        let response = router(state.clone()).oneshot(request()).await.unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(state.chat_memory.available_permits(), 3);
        assert_eq!(probe.prompts.load(Ordering::SeqCst), 0);
        assert_eq!(state.accounting.available("a"), Some(100));
        drop(response);
        assert_eq!(state.chat_memory.available_permits(), 4);
        drop(ingress);
        // Cancellation while collecting raw input returns both admissions,
        // without reaching a security gate, tokenizer or reservation.
        let entered = Arc::new(Notify::new());
        let notify = entered.clone();
        let mut pending = request();
        *pending.body_mut() = Body::from_stream(futures_util::stream::once(async move {
            notify.notify_one();
            std::future::pending::<Result<Bytes, std::io::Error>>().await
        }));
        let task = tokio::spawn(router(state.clone()).oneshot(pending));
        entered.notified().await;
        assert_eq!(state.chat_memory.available_permits(), 3);
        assert_eq!(state.chat_ingress.available_permits(), 3);
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert_eq!(state.chat_memory.available_permits(), 4);
        assert_eq!(state.chat_ingress.available_permits(), 4);
        assert_eq!(state.accounting.available("a"), Some(100));
        let all = state.chat_memory.clone().try_acquire_many_owned(4).unwrap();
        let response = router(state.clone()).oneshot(request()).await.unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(probe.prompts.load(Ordering::SeqCst), 0);
        assert_eq!(state.accounting.available("a"), Some(100));
        assert_eq!(state.auth.session(&id).unwrap().selected_model, None);
        drop(response);
        drop(all);
        let task = tokio::spawn(router(state.clone()).oneshot(request()));
        probe.entered.notified().await;
        assert_eq!(state.chat_memory.available_permits(), 3);
        // Raw bytes/form parsing ended before prompt-bearing tokenizer work.
        assert_eq!(state.chat_ingress.available_permits(), 4);
        assert_eq!(state.accounting.available("a"), Some(48));
        probe.release.notify_one();
        let response = task.await.unwrap().unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        drop(response); // Delivery loss does not cancel accepted generation.
        tokio::time::timeout(Duration::from_secs(5), async {
            while state.chat_memory.available_permits() != 4 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(state.chat_memory.available_permits(), 4);
        assert_eq!(state.chat_ingress.available_permits(), 4);
        assert_eq!(state.accounting.available("a"), Some(97));
    }
}
