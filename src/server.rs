use crate::telemetry::{
    hooks::{HttpObservation, Lease, RequestContext},
    AdmissionModel, Endpoint, HttpTerminal, Lane, Rejection as MetricRejection,
};
use crate::{
    accounting::Accounting,
    attestation::{EvidenceVerifier, GatewayEvidence},
    auth::Auth,
    generation::Generation,
    inference::SharedInference,
};
use axum::{
    body::{Body, Bytes, HttpBody},
    extract::{DefaultBodyLimit, Extension, State},
    http::{header, HeaderValue, Method, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use hyper::server::conn::http1;
use hyper_util::{
    rt::{TokioIo, TokioTimer},
    service::TowerToHyperService,
};
use serde::Serialize;
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
mod admission_tests;
#[cfg(test)]
mod timeout_tests;
#[cfg(test)]
pub(crate) mod resource_streaming_tests;

pub const BODY_LIMIT: usize = 8 * 1024 * 1024;
const BODY_DEADLINE: Duration = Duration::from_secs(30);
const HEADER_DEADLINE: Duration = Duration::from_secs(10);
const MAX_CONNECTIONS: usize = 64;
const MAX_HEADERS: usize = 64;
const MAX_HEADER_BYTES: usize = 32 * 1024;
// Scoped admission targets, not measured RSS or a whole-process memory proof:
// 4 * (104 MiB heavy + 16 MiB ingress) + 16 MiB controls.
// Heavy is acquired BEFORE raw collection/decoding and covers decoded JSON,
// tokenizer, streaming inference and delivery overlap. Ingress covers raw input
// storage; it is NOT a standalone allowance for decoded history. SDK/TLS, shared
// auth/accounting state and allocator overhead are outside these envelopes.
const CHAT_LANES: usize = 4;
const CONTROL_LANES: usize = 1;
// Control requests and unknown routes have bounded bodies.
const CONTROL_BODY_LIMIT: usize = 4 * 1024;
const MAX_ATTESTATION_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

/// Fixed configured resource bounds, not currently available permits.
/// Generation inherits the heavy-memory bound; it has no separate count quota.
pub fn admission_capacities() -> [(Lane, u64); 5] {
    [
        (Lane::Connection, MAX_CONNECTIONS as u64),
        (Lane::Generation, CHAT_LANES as u64),
        (Lane::Heavy, CHAT_LANES as u64),
        (Lane::Ingress, CHAT_LANES as u64),
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
    pub(crate) fn available_lanes(&self) -> [usize; 4] {
        [
            self.generation_headroom(),
            self.chat_memory.available_permits(),
            self.chat_ingress.available_permits(),
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

fn connection_builder(header_deadline: Duration) -> http1::Builder {
    let mut builder = http1::Builder::new();
    builder
        // No absolute connection age may truncate accepted generation.
        // One request per connection avoids unbounded idle keepalive sockets.
        .keep_alive(false)
        .timer(TokioTimer::new())
        .header_read_timeout(header_deadline)
        .max_headers(MAX_HEADERS)
        .max_buf_size(MAX_HEADER_BYTES);
    builder
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
            let builder = connection_builder(header_deadline);
            let connection = builder.serve_connection(TokioIo::new(stream), service);
            let result = connection.await;
            if !entered.load(std::sync::atomic::Ordering::Acquire) {
                let reason = match result {
                    Err(error) if error.is_timeout() => {
                        Some(MetricRejection::ConnectionDeadline)
                    }
                    Err(error) if error.is_parse() => Some(MetricRejection::HeaderProtocol),
                    Err(_) => Some(MetricRejection::TransportUnknown),
                    Ok(()) => None, // clean idle close is not a rejection
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
        (&Method::POST, "/v1/chat/completions") => (&state.chat_memory, Lane::Heavy),
        _ => (&state.control_memory, Lane::Control),
    };
    let api = crate::api::is_api(request.uri().path());
    let Ok(permit) = lane.clone().try_acquire_owned() else {
        context.reject(AdmissionModel::Unknown, MetricRejection::RequestCapacity);
        let response = crate::api::error(StatusCode::SERVICE_UNAVAILABLE);
        return observed_response(response, None, observation);
    };
    let lease = Arc::new(Lease::observed(permit, state.telemetry.as_ref(), label));
    if request.method() == Method::POST && request.uri().path() == "/v1/chat/completions" {
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
    let chat = request.method() == Method::POST && request.uri().path() == "/v1/chat/completions";
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

async fn verified_gateway_evidence(
    state: &AppState,
) -> Result<crate::attestation::GatewayEvidence, crate::attestation::EvidenceError> {
    state.generation().evidence().await
}

fn unavailable() -> Response {
    (StatusCode::SERVICE_UNAVAILABLE, "service unavailable").into_response()
}
