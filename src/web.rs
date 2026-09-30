use crate::{
    accounting::{Accounting, Outcome, ReserveResult},
    attestation::{EvidenceVerifier, GatewayEvidence},
    auth::{
        clear_session_cookie, login_challenge_cookie, session_cookie, AdmissionError, Auth, Session,
    },
    catalog::actual_cost,
    inference::{authenticated_catalog, Message, SharedInference},
    render,
};
use axum::{
    body::{Body, Bytes, HttpBody},
    extract::{DefaultBodyLimit, Form, State},
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
    sync::Arc,
    task::{Context, Poll},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    net::TcpListener,
    sync::{OwnedSemaphorePermit, Semaphore},
};
use tower_http::catch_panic::CatchPanicLayer;

#[cfg(test)]
mod resource_streaming_tests;

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
// tokenizer, buffered inference and rendering overlap. Ingress covers raw input
// storage; it is NOT a standalone allowance for decoded history. SDK/TLS, shared
// auth/accounting state and allocator overhead are outside these envelopes.
const CHAT_LANES: usize = 4;
// Native control forms contain only CSRF/credentials (43/128 bytes), never
// conversation history. Apply this ceiling to fallbacks/wrong methods as well.
const CONTROL_BODY_LIMIT: usize = 4 * 1024;
const MAX_CONTROL_RENDERED_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const MAX_RENDERED_RESPONSE_BYTES: usize = 32 * 1024 * 1024;
const MAX_ATTESTATION_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

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
            chat_memory: Arc::new(Semaphore::new(CHAT_LANES)),
            chat_ingress: Arc::new(Semaphore::new(CHAT_LANES)),
            new_chat_memory: Arc::new(Semaphore::new(1)),
            control_memory: Arc::new(Semaphore::new(1)),
            generation_slots: Arc::new(Semaphore::new(CHAT_LANES)),
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
        .route("/chat/new", post(new_chat))
        .route("/recovery", get(recovery))
        .route("/recovery/download", get(recovery_download))
        .route("/claims", get(claims))
        .route("/attestation", get(attestation))
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
    mut request: Request<Body>,
    next: Next,
) -> Response {
    // Match the router's exact method/path semantics; query strings do not
    // affect routing. Wrong methods, trailing slashes and encoded aliases stay
    // in the bounded control lane and cannot reach a heavy handler.
    let lane = match (request.method(), request.uri().path()) {
        (&Method::POST, "/chat") => &state.chat_memory,
        (&Method::POST, "/chat/new") => &state.new_chat_memory,
        _ => &state.control_memory,
    };
    let Ok(permit) = lane.clone().try_acquire_owned() else {
        return (StatusCode::SERVICE_UNAVAILABLE, "service busy").into_response();
    };
    let lease = Arc::new(permit);
    if request.method() == Method::POST && request.uri().path() == "/chat" {
        // Staged plumbing: a future detached worker and delivery body must share
        // THIS heavy admission, not acquire another. The buffered handler does
        // not use this extension yet; this is not a streaming safety proof.
        request.extensions_mut().insert(lease.clone());
    }
    let response = next.run(request).await;
    let (parts, body) = response.into_parts();
    Response::from_parts(parts, Body::new(AdmissionBody { inner: body, lease }))
}

struct AdmissionBody {
    inner: Body,
    lease: Arc<OwnedSemaphorePermit>,
}

// Data drops BEFORE its lease. from_owner preserves the lease across frame
// dequeue, body/EOF drop, Bytes clones and even one-byte slices, without copying
// payload. The same owner is used for raw request bytes through Form decoding.
struct AdmittedBytes {
    data: Bytes,
    _lease: Arc<OwnedSemaphorePermit>,
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
        Pin::new(&mut self.inner).poll_frame(context).map(|frame| {
            frame.map(|result| {
                result.map(|frame| {
                    frame.map_data(|data| {
                        Bytes::from_owner(AdmittedBytes {
                            data,
                            _lease: self.lease.clone(),
                        })
                    })
                })
            })
        })
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
    let chat = request.method() == Method::POST && request.uri().path() == "/chat";
    // The outer heavy permit is already owned. Failure here rolls it back
    // through the bounded error response, with no waiter or body polling.
    let ingress = if chat {
        match state.chat_ingress.clone().try_acquire_owned() {
            Ok(permit) => Some(Arc::new(permit)),
            Err(_) => return (StatusCode::SERVICE_UNAVAILABLE, "service busy").into_response(),
        }
    } else {
        None
    };
    let limit = if chat { BODY_LIMIT } else { CONTROL_BODY_LIMIT };
    let (parts, body) = request.into_parts();
    let bytes = match tokio::time::timeout(deadline, collect_request(body, limit)).await {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(())) => {
            return (StatusCode::PAYLOAD_TOO_LARGE, "request body too large").into_response()
        }
        Err(_) => return (StatusCode::REQUEST_TIMEOUT, "request body timed out").into_response(),
    };
    let bytes = match ingress {
        Some(lease) => Bytes::from_owner(AdmittedBytes {
            data: bytes,
            _lease: lease,
        }),
        None => bytes,
    };
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
        Ok(catalog) => match state.auth.issue_submission_for(
            &session_id,
            session.conversation,
            session.selected_model.as_deref(),
        ) {
            Ok(token) => render::chat_page(
                &catalog.models,
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

// Cutover contract: after CSRF validation/reset, render the COMPLETE bounded
// empty continuation + authenticated model selector directly in this lane,
// preserving every supported model and catalog fail-closed behavior. Redirecting
// through GET / below is buffered compatibility, NOT independent availability.
// No availability promise applies while the New chat lane itself is retained.
async fn new_chat(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<CsrfForm>,
) -> Response {
    let Some(id) = cookie_value(&headers, "possums_session") else {
        return unauthorized();
    };
    if state.auth.new_chat(&id, &form.csrf).is_err() {
        return unauthorized();
    }
    // History lives only in the submitted form; the home route starts empty.
    Redirect::to("/").into_response()
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

#[derive(Serialize)]
struct AttestationDocuments<'a> {
    gateway: &'a GatewayEvidence,
    upstream: &'a serde_json::Value,
}

async fn attestation(State(state): State<AppState>) -> Response {
    let gateway = match verified_gateway_evidence(&state).await {
        Ok(value) => value,
        Err(_) => return unavailable(),
    };
    let upstream = match state.inference.verification_document() {
        Ok(value) => value,
        Err(_) => return unavailable(),
    };
    let bytes = match crate::bounded_json::to_vec(
        &AttestationDocuments {
            gateway: &gateway,
            upstream: &upstream,
        },
        MAX_ATTESTATION_RESPONSE_BYTES,
    ) {
        Ok(value) => value,
        Err(_) => return unavailable(),
    };
    let mut response = Body::from(bytes).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    response
}

#[derive(Deserialize)]
struct ChatForm {
    csrf: String,
    token: String,
    model: String,
    history: String,
    prompt: String,
}

// PACKET-2 HANDOFF CONTRACT (not implemented by this buffered handler):
// 1. Bounded raw decode, trust/catalog validation, CSRF/model/conversation checks,
//    global-four permit and session->submission->accounting reserve (three/account).
//    Insufficient credit/concurrency/duplicate paths send no prompt, even to the
//    tokenizer. Keep the submitted authenticated quote in the ledger unchanged.
// 2. New Reserved is accounting acceptance. With NO intervening await or prompt
//    operation: construct ReservedGeneration and synchronously spawn detached
//    preflight with owned decoded input + the SAME middleware heavy Arc + permit.
//    Never create ReservationGuard as well, disarm/rearm, or automatically replay.
// 3. Preflight owns tokenization and context checks under a finite overall
//    deadline (30s, including tokenization). Move prompt into the message vector
//    for borrowed serialization, then move it back for compose; do not clone it.
//    Use the submitted catalog snapshot to compute the full context-legal output
//    allowance; this must NOT replace/reprice the ledger's original reservation.
//    On failure/deadline drop the sole owner to refund once before reporting a
//    fixed rejection. On success synchronously call streaming_chat::compose with
//    that SAME owner regardless of whether the result receiver is still open.
// 4. Result is oneshot::Receiver<Result<DeliveryBody, PreflightRejection>>:
//    one bounded body handle OR a content-free Context/Unavailable rejection.
//    Attached clients await this BEFORE streaming headers. The detached task
//    drops compose's settlement observer; it conveys no cancellation authority.
//    A failed send drops delivery ONLY; it never vetoes compose or settlement.
//    Receiver disappearance after acceptance must never abort the preflight task.
// 5. Use explicit field order in the detached envelope: pinned work/input first,
//    ReservedGeneration second, heavy Arc last. Work can take the owner only for
//    the synchronous compose call. All prompt storage dies before the last heavy
//    lease, including on pre-poll cancellation/unwind. A queued success is the
//    DeliveryBody (already owns heavy); rejection holds no prompt. spawn_blocking
//    inputs AND unclaimed results carry heavy last, as Startup/Started already do.
//    Generation/account slots release at worker/terminal boundaries, independently
//    of retained delivery. Body, queued chunks and Bytes clones/slices keep heavy
//    until their last owner drops. Raw ingress keeps its SEPARATE lease through
//    AdmittedBytes until its final owner drops; decoding does not release it early.
// 6. Test-only one-shot reached/release barriers: inside tokenizer after actual
//    serialization (body retained), after tokenization/context before compose,
//    and immediately after synchronous compose before sending its body. Await
//    barriers only under cfg(test); production has no await in either handoff.
//    Tests cancel the HTTP waiter at each point, then release accepted work and
//    assert one generation/settlement or zero generations/one preflight refund.
//    Additional blocking-startup/parser gates belong to fixtures, not production.
// The delivery contract remains 64 KiB / eight outstanding frame owners, including
// dequeued clones/slices. This contract is not an aggregate resource proof.
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
    drop(form.history); // The decoded Vec now owns history; do not retain both.
    history.push(Message {
        role: "user".into(),
        content: form.prompt,
    });

    // Gateway provenance and serving-key evidence is a mandatory pre-prompt gate.
    if verified_gateway_evidence(&state).await.is_err() {
        return unavailable();
    }
    let catalog = match current_catalog(&state).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let reservation_quote = match catalog.reservation_quote(&form.model) {
        Ok(value) => value,
        Err(_) => return bad_request(),
    };
    // The heavy permit is already owned by middleware. Obtain the
    // existing global generation permit nonblockingly before admission and any
    // prompt-bearing tokenizer call. Failed/duplicate admission releases it.
    let permit = match state.generation_slots.clone().try_acquire_owned() {
        Ok(value) => value,
        Err(_) => return (StatusCode::SERVICE_UNAVAILABLE, "service busy").into_response(),
    };
    let admission = match state.auth.admit_submission(
        &state.accounting,
        &session_id,
        &form.csrf,
        &form.token,
        reservation_quote,
    ) {
        Ok(value) => value,
        Err(AdmissionError::Auth(_)) => return bad_request(),
        Err(AdmissionError::Accounting(_)) => {
            return (StatusCode::PAYMENT_REQUIRED, "request unavailable").into_response()
        }
    };
    let submission = admission.submission;
    let submission_id = submission.id;
    match admission.result {
        ReserveResult::Reserved => {}
        ReserveResult::Duplicate(outcome) => {
            drop(permit);
            let notice = match outcome {
                Outcome::InFlight | Outcome::AwaitingDelivery => {
                    "This submission is already in progress."
                }
                Outcome::Settled { .. } => {
                    "This submission already completed and was not regenerated."
                }
                Outcome::Refunded => {
                    "This submission failed and was refunded; use the new form to retry."
                }
            };
            let token = match state.auth.issue_submission_for(
                &session_id,
                submission.conversation,
                Some(&form.model),
            ) {
                Ok(value) => value,
                Err(_) => return unavailable(),
            };
            return render::chat_page(
                &catalog.models,
                &session.csrf,
                &token,
                &history,
                Some(&form.model),
                Some(notice),
                MAX_RENDERED_RESPONSE_BYTES,
            )
            .map_or_else(unavailable, |html| Html(html).into_response());
        }
    }

    let reservation = ReservationGuard::new(state.accounting.clone(), submission_id);
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
    // prepare_settlement has released accounting before acquiring auth locks.
    // Compatibility remains delivery-owned: stale continuation still refunds
    // through ReservationGuard; detached usage-owned settlement is packet 6.
    let token = match state.auth.issue_submission_for(
        &session_id,
        submission.conversation,
        Some(&form.model),
    ) {
        Ok(value) => value,
        Err(_) => return unavailable(),
    };
    let Some(html) = render::chat_page(
        &catalog.models,
        &session.csrf,
        &token,
        &history,
        Some(&form.model),
        None,
        MAX_RENDERED_RESPONSE_BYTES,
    ) else {
        return unavailable();
    };
    let (parts, body) = Html(html).into_response().into_parts();
    Response::from_parts(parts, Body::new(ReservationBody::new(body, reservation)))
}

/// Additive decoder, deliberately not wired to production ChatForm yet.
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
    // Serde's depth limit also bounds hostile nested JSON; schema denies unknown
    // and duplicate fields. The decoded input and message allocation are bounded
    // separately from the renderer; later admission must budget both.
    let parsed: Vec<HistoryMessage> =
        serde_json::from_slice(&json).map_err(|_| InvalidContinuation)?;
    let history: Vec<Message> = parsed
        .into_iter()
        .map(|m| Message {
            role: m.role,
            content: m.content,
        })
        .collect();
    if !render::valid_history(&history) {
        return Err(InvalidContinuation);
    }
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

    fn settle(&mut self) {
        if self.accounting.settle(self.submission_id).is_ok() {
            self.armed = false;
        }
    }
}

impl Drop for ReservationGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.accounting.refund(self.submission_id);
        }
    }
}

struct ReservationBody {
    inner: Body,
    reservation: Option<ReservationGuard>,
}

impl ReservationBody {
    fn new(inner: Body, reservation: ReservationGuard) -> Self {
        Self {
            inner,
            reservation: Some(reservation),
        }
    }
}

impl HttpBody for ReservationBody {
    type Data = Bytes;
    type Error = axum::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        let result = Pin::new(&mut self.inner).poll_frame(context);
        if matches!(result, Poll::Ready(None)) {
            if let Some(mut reservation) = self.reservation.take() {
                reservation.settle();
            }
        }
        result
    }
}

async fn verified_gateway_evidence(
    state: &AppState,
) -> Result<crate::attestation::GatewayEvidence, crate::attestation::EvidenceError> {
    state
        .gateway_evidence_verifier
        .verify(state.gateway_evidence_path.as_ref(), now_unix())
        .await
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        attestation::{EvidenceError, GatewayEvidence},
        catalog::Model,
        inference::{Generation, Inference, InferenceError},
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
        async fn count_tokens(&self, _: &str, _: &[Message]) -> Result<u64, InferenceError> {
            self.prompts.fetch_add(1, Ordering::SeqCst);
            self.entered.notify_one();
            self.release.notified().await;
            Ok(1)
        }
        async fn generate(&self, _: &Model, _: &[Message]) -> Result<Generation, InferenceError> {
            Ok(Generation {
                content: "answer".into(),
                input_tokens: 1,
                output_tokens: 1,
            })
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
        // production /chat remains buffered and does not consume the extension.
        let app = Router::new()
            .fallback(|request: Request<Body>| async move {
                let lease = request.extensions().get::<Arc<OwnedSemaphorePermit>>();
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
            .remove::<Arc<OwnedSemaphorePermit>>()
            .unwrap();
        assert_eq!(state.chat_memory.available_permits(), 0);
        assert_eq!(state.chat_ingress.available_permits(), 4);
        assert_eq!(state.generation_slots.available_permits(), 4);
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
    async fn four_global_permits_gate_reservation_and_are_held_during_tokenization() {
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
                    "csrf={}&token={token}&model=m&history=%5B%5D&prompt=hello",
                    session.csrf
                )))
                .unwrap()
        };
        assert_eq!(state.generation_slots.available_permits(), 4);
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
        let all = state
            .generation_slots
            .clone()
            .try_acquire_many_owned(4)
            .unwrap();
        let response = router(state.clone()).oneshot(request()).await.unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(probe.prompts.load(Ordering::SeqCst), 0);
        assert_eq!(state.accounting.available("a"), Some(100));
        assert_eq!(state.auth.session(&id).unwrap().selected_model, None);
        drop(response);
        drop(all);
        let task = tokio::spawn(router(state.clone()).oneshot(request()));
        probe.entered.notified().await;
        assert_eq!(state.generation_slots.available_permits(), 3);
        assert_eq!(state.chat_memory.available_permits(), 3);
        // Raw bytes/form parsing ended before prompt-bearing tokenizer work.
        assert_eq!(state.chat_ingress.available_permits(), 4);
        assert_eq!(state.accounting.available("a"), Some(48));
        probe.release.notify_one();
        let response = task.await.unwrap().unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        drop(response); // Buffered compatibility refund; not a streaming worker.
        assert_eq!(state.generation_slots.available_permits(), 4);
        assert_eq!(state.chat_memory.available_permits(), 4);
        assert_eq!(state.chat_ingress.available_permits(), 4);
        assert_eq!(state.accounting.available("a"), Some(100));
    }
}
