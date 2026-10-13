use crate::telemetry::{
    hooks::{HttpObservation, Lease, RequestContext},
    AdmissionModel, Endpoint, HttpTerminal, Lane, Rejection as MetricRejection,
};
use crate::{
    accounting::Accounting,
    attestation::{EvidenceVerifier, GatewayEvidence},
    auth::Auth,
    failures::InterfaceError,
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
use futures_util::FutureExt;
use hyper::server::conn::http1;
use hyper_util::{
    rt::{TokioIo, TokioTimer},
    service::TowerToHyperService,
};
use serde::Serialize;
use std::{
    future::{poll_fn, Future},
    pin::Pin,
    sync::{atomic::AtomicUsize, Arc},
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    net::TcpListener,
    sync::{watch, Semaphore},
    task::JoinSet,
};
use tower_http::catch_panic::CatchPanicLayer;

#[cfg(test)]
mod admission_tests;
#[cfg(test)]
mod cause_tests;
#[cfg(test)]
mod lifecycle_tests;
#[cfg(test)]
mod observer_tests;
#[cfg(test)]
pub(crate) mod resource_streaming_tests;
#[cfg(test)]
mod timeout_tests;

pub const BODY_LIMIT: usize = 32 * 1024 * 1024;
const BODY_DEADLINE: Duration = Duration::from_secs(600);
const CONTROL_BODY_DEADLINE: Duration = Duration::from_secs(30);
const HEADER_DEADLINE: Duration = Duration::from_secs(10);
const MAX_CONNECTIONS: usize = 64;
const MAX_HEADERS: usize = 64;
const MAX_HEADER_BYTES: usize = 32 * 1024;
// Chat lifetimes are tracked passively, without a shared admission bound.
// Control requests and unknown routes have bounded bodies.
const CONTROL_BODY_LIMIT: usize = 4 * 1024;
const MAX_ATTESTATION_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

/// Fixed configured admission bounds, not occupancy or disconnected work limits.
pub fn admission_capacities() -> [(Lane, u64); 1] {
    [(Lane::Connection, MAX_CONNECTIONS as u64)]
}

#[derive(Clone)]
pub struct AppState {
    pub lifecycle: crate::lifecycle::Lifecycle,
    pub auth: Arc<Auth>,
    pub accounting: Arc<Accounting>,
    pub inference: SharedInference,
    pub gateway_evidence_path: Arc<str>,
    gateway_evidence_verifier: Arc<dyn EvidenceVerifier>,
    heavy_activity: Arc<AtomicUsize>,
    ingress_activity: Arc<AtomicUsize>,
    generation_activity: Arc<AtomicUsize>,
    telemetry: Option<Arc<crate::telemetry::AggregateMetrics>>,
    #[cfg(test)]
    preflight_hooks: Arc<resource_streaming_tests::PreflightHooks>,
    #[cfg(test)]
    server_hooks: Arc<lifecycle_tests::ServerHooks>,
}

impl AppState {
    pub fn with_telemetry(
        mut self,
        metrics: Option<Arc<crate::telemetry::AggregateMetrics>>,
    ) -> Self {
        self.telemetry = metrics;
        self
    }
    // Test-only actual lifetime counts, not headroom or admission capacity.
    #[cfg(test)]
    pub(crate) fn active_lanes(&self) -> [usize; 3] {
        [
            &self.generation_activity,
            &self.heavy_activity,
            &self.ingress_activity,
        ]
        .map(|activity| activity.load(std::sync::atomic::Ordering::Relaxed))
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
            disconnect: Default::default(),
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
            lifecycle: Default::default(),
            auth: Arc::new(auth),
            accounting: Arc::new(accounting),
            inference,
            gateway_evidence_path: evidence_path.into(),
            gateway_evidence_verifier,
            heavy_activity: Arc::default(),
            ingress_activity: Arc::default(),
            generation_activity: Arc::default(),
            telemetry: None,
            #[cfg(test)]
            preflight_hooks: Arc::default(),
            #[cfg(test)]
            server_hooks: Arc::default(),
        }
    }
}

pub fn router(state: AppState) -> Router {
    router_with_body_deadline(state, BODY_DEADLINE)
}

#[doc(hidden)]
pub fn router_with_body_deadline(state: AppState, body_deadline: Duration) -> Router {
    let health = state.lifecycle.clone();
    let app = Router::new()
        .merge(crate::api::routes())
        .route("/attestation", get(attestation))
        .fallback(not_found)
        .method_not_allowed_fallback(wrong_method)
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            move |state, request, next| total_body_deadline(state, request, next, body_deadline),
        ))
        .layer(CatchPanicLayer::custom(panic_response))
        .layer(middleware::from_fn(header_size_limit))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            request_admission,
        ))
        .layer(middleware::from_fn(security_headers))
        .with_state(state);
    // A credential-free process readiness check, outside application admission,
    // evidence, authentication and request-observation middleware.
    Router::new().merge(app).route(
        "/healthz",
        get(move || {
            let health = health.clone();
            async move {
                if health.is_serving() {
                    StatusCode::NO_CONTENT
                } else {
                    StatusCode::SERVICE_UNAVAILABLE
                }
            }
        }),
    )
}

/// Closed, content-free serving failures behind the public `io::Result` shape.
/// No OS, join, panic or peer detail is retained.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ServerFailure {
    #[error("code=listener_accept_unknown stage=serving constraint=listener_accept cause=unknown action=check_gateway_listener; not replayed; accepted-work outcomes require verified reconciliation")]
    Accept,
    #[error("code=connection_task_unknown stage=serving constraint=task_completion cause=unknown action=check_gateway_service; not replayed; accepted-work outcomes require verified reconciliation")]
    ConnectionTask,
    #[error("code=shutdown_integrity_failed stage=cleanup constraint=accepted_work_drain action=verify_account_reconciliation; not replayed; charge/refund outcomes are unknown without verified evidence")]
    ShutdownIntegrity,
}

impl ServerFailure {
    fn into_io(self) -> std::io::Error {
        std::io::Error::other(self)
    }
}

/// Closed source and legacy rejection projection for the connection owner.
struct ConnectionFailure {
    rejection: MetricRejection,
    cause: InterfaceError,
}

fn classify_connection_error(error: &hyper::Error) -> ConnectionFailure {
    if error.is_timeout() {
        ConnectionFailure {
            rejection: MetricRejection::ConnectionDeadline,
            cause: InterfaceError::ConnectionDeadline,
        }
    } else if error.is_parse() {
        ConnectionFailure {
            rejection: MetricRejection::HeaderProtocol,
            cause: InterfaceError::HeaderProtocol,
        }
    } else {
        ConnectionFailure {
            rejection: MetricRejection::TransportUnknown,
            cause: InterfaceError::ConnectionUnknown,
        }
    }
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
    serve_until(listener, state, std::future::pending()).await
}

pub async fn serve_until(
    listener: TcpListener,
    state: AppState,
    shutdown: impl Future<Output = ()>,
) -> std::io::Result<()> {
    serve_until_with_header_deadline(listener, state, HEADER_DEADLINE, shutdown).await
}

#[doc(hidden)]
pub async fn serve_with_header_deadline(
    listener: TcpListener,
    state: AppState,
    header_deadline: Duration,
) -> std::io::Result<()> {
    serve_until_with_header_deadline(listener, state, header_deadline, std::future::pending()).await
}

async fn serve_until_with_header_deadline(
    listener: TcpListener,
    state: AppState,
    header_deadline: Duration,
    shutdown: impl Future<Output = ()>,
) -> std::io::Result<()> {
    let connections = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    let telemetry = state.telemetry.clone();
    let lifecycle = state.lifecycle.clone();
    #[cfg(test)]
    let hooks = state.server_hooks.clone();
    #[cfg(test)]
    let mut injected_accept_failure = hooks.accept_failure.lock().unwrap().take();
    let app = router(state);
    let (stop, _) = watch::channel(false);
    let mut tasks = JoinSet::new();
    tokio::pin!(shutdown);
    let serving_result = loop {
        tokio::select! {
            biased;
            _ = &mut shutdown => break Ok(()),
            result = tasks.join_next(), if !tasks.is_empty() => {
                if !matches!(result, Some(Ok(()))) {
                    if let Some(metrics) = &telemetry {
                        metrics.interface_error(InterfaceError::LifecycleServeUnknown);
                    }
                    break Err(ServerFailure::ConnectionTask.into_io());
                }
            }
            result = async {
                #[cfg(test)]
                if let Some(failure) = &mut injected_accept_failure {
                    tokio::select! {
                        biased;
                        _ = failure => Err(std::io::Error::other("injected listener failure")),
                        result = listener.accept() => result,
                    }
                } else {
                    listener.accept().await
                }
                #[cfg(not(test))]
                listener.accept().await
            } => {
                let (stream, _) = match result {
                    Ok(accepted) => accepted,
                    Err(_) => {
                        if let Some(metrics) = &telemetry {
                            metrics.interface_error(InterfaceError::LifecycleServeUnknown);
                        }
                        break Err(ServerFailure::Accept.into_io());
                    }
                };
                let Ok(permit) = connections.clone().try_acquire_owned() else {
                    if let Some(metrics) = &telemetry {
                        metrics.reject(None, AdmissionModel::NotApplicable, MetricRejection::ConnectionCapacity);
                        metrics.interface_error(InterfaceError::ConnectionCapacity);
                    }
                    drop(stream);
                    continue;
                };
                let permit = Lease::observed(permit, telemetry.as_ref(), Lane::Connection);
                let entered = Arc::new(std::sync::atomic::AtomicBool::new(false));
                let seen = entered.clone();
                let service = TowerToHyperService::new(app.clone());
                let io_failed = Arc::new(std::sync::atomic::AtomicBool::new(false));
                let request_io_failed = io_failed.clone();
                let disconnect = crate::stream_owner::Disconnect::default();
                let request_disconnect = disconnect.clone();
                let service = hyper::service::service_fn(move |mut request: Request<hyper::body::Incoming>| {
                    request.extensions_mut().insert(request_disconnect.clone());
                    request.extensions_mut().insert(ConnectionIoFailure(request_io_failed.clone()));
                    seen.store(true, std::sync::atomic::Ordering::Release);
                    let service = service.clone();
                    async move { hyper::service::Service::call(&service, request).await }
                });
                let metrics = telemetry.clone();
                let mut closing = stop.subscribe();
                #[cfg(test)]
                let hooks = hooks.clone();
                tasks.spawn(async move {
                    #[cfg(test)]
                    let pause = hooks.connection_start.lock().unwrap().take();
                    #[cfg(test)]
                    if let Some(pause) = pause {
                        pause.wait().await;
                    }
                    let builder = connection_builder(header_deadline);
                    let _disconnect_guard = crate::stream_owner::DisconnectGuard(Some(disconnect.clone()));
                    let connection = builder.serve_connection(TokioIo::new(DisconnectIo { stream, disconnect: disconnect.clone(), io_failed: io_failed.clone() }), service);
                    tokio::pin!(connection);
                    let result = if *closing.borrow_and_update() {
                        connection.as_mut().graceful_shutdown();
                        connection.await
                    } else {
                        tokio::select! {
                            biased;
                            _ = closing.changed() => {
                                connection.as_mut().graceful_shutdown();
                                connection.await
                            }
                            result = &mut connection => result,
                        }
                    };
                    let io_failed = io_failed.load(std::sync::atomic::Ordering::Acquire);
                    if io_failed {
                        if let Some(metrics) = &metrics {
                            metrics.interface_error(InterfaceError::ConnectionUnknown);
                        }
                    }
                    if !entered.load(std::sync::atomic::Ordering::Acquire) {
                        if let Err(error) = &result {
                            // Peer EOF during an unfinished header is ordinary
                            // disconnect, not a fabricated framing fault.
                            let failure = classify_connection_error(error);
                            let peer_ended = failure.cause == InterfaceError::ConnectionUnknown
                                && !io_failed && disconnect.cancelled().now_or_never().is_some();
                            if !peer_ended {
                                if let Some(metrics) = &metrics {
                                    metrics.reject(None, AdmissionModel::NotApplicable, failure.rejection);
                                    if !io_failed || failure.cause != InterfaceError::ConnectionUnknown {
                                        metrics.interface_error(failure.cause);
                                    }
                                }
                            }
                        }
                    }
                    drop(permit);
                    #[cfg(test)]
                    if let Some(done) = hooks.connection_finished.lock().unwrap().take() {
                        let _ = done.send(());
                    }
                });
            }
        }
    };
    lifecycle.quiesce();
    drop(listener);
    stop.send_replace(true);
    #[cfg(test)]
    if let Some(done) = hooks.shutdown_published.lock().unwrap().take() {
        let _ = done.send(());
    }
    // A connection can finish before its detached generation owner. Neither
    // condition substitutes for the other; keep the runtime and tasks alive.
    let (drain, connections) = tokio::join!(lifecycle.wait_drained(), async {
        let mut failed = false;
        while let Some(result) = tasks.join_next().await {
            if result.is_err() {
                failed = true;
                if let Some(metrics) = &telemetry {
                    metrics.interface_error(InterfaceError::LifecycleServeUnknown);
                }
            }
        }
        failed
    });
    serving_result?;
    if drain.is_err() || connections {
        return Err(ServerFailure::ShutdownIntegrity.into_io());
    }
    Ok(())
}

/// Non-authorizing propagation marker for the connection's sole I/O owner.
#[derive(Clone)]
struct ConnectionIoFailure(Arc<std::sync::atomic::AtomicBool>);

/// Notify request ownership on actual socket EOF/error as well as body drop.
struct DisconnectIo {
    stream: tokio::net::TcpStream,
    disconnect: crate::stream_owner::Disconnect,
    io_failed: Arc<std::sync::atomic::AtomicBool>,
}
impl DisconnectIo {
    fn failed(&self, error: &std::io::Error) {
        // Supported connection-end kinds do not establish a separate fault.
        // Other I/O failures are observed once by the connection owner; raw
        // OS errors and peer metadata never leave this polling boundary.
        if !matches!(
            error.kind(),
            std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::ConnectionAborted
                | std::io::ErrorKind::BrokenPipe
                | std::io::ErrorKind::NotConnected
                | std::io::ErrorKind::UnexpectedEof
        ) {
            self.io_failed
                .store(true, std::sync::atomic::Ordering::Release);
        }
    }
}
impl tokio::io::AsyncRead for DisconnectIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let result = Pin::new(&mut self.stream).poll_read(cx, buf);
        if let Poll::Ready(Err(error)) = &result {
            self.failed(error);
        }
        if matches!(result, Poll::Ready(Err(_)))
            || matches!(result, Poll::Ready(Ok(())))
                && buf.remaining() > 0
                && buf.filled().len() == before
        {
            self.disconnect.cancel();
        }
        result
    }
}
impl tokio::io::AsyncWrite for DisconnectIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let result = Pin::new(&mut self.stream).poll_write(cx, bytes);
        if let Poll::Ready(Err(error)) = &result {
            self.failed(error);
        }
        if matches!(result, Poll::Ready(Err(_)) | Poll::Ready(Ok(0))) && !bytes.is_empty() {
            self.disconnect.cancel();
        }
        result
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let result = Pin::new(&mut self.stream).poll_flush(cx);
        if let Poll::Ready(Err(error)) = &result {
            self.failed(error);
            self.disconnect.cancel();
        }
        result
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}

async fn wrong_method(Extension(observation): Extension<RequestContext>) -> Response {
    observation.reject(AdmissionModel::NotApplicable, MetricRejection::Input);
    route_refused(StatusCode::METHOD_NOT_ALLOWED)
}

// Explicit missing-route fallback: attach the typed source here rather than
// inferring a cause from the 404 status at normalization.
async fn not_found() -> Response {
    route_refused(StatusCode::NOT_FOUND)
}

fn route_refused(status: StatusCode) -> Response {
    let mut response = (status, InterfaceError::RouteRefused.message()).into_response();
    response
        .extensions_mut()
        .insert(InterfaceError::RouteRefused);
    response
}

// Locally authored panic response. The payload is never inspected, retained or
// logged; process-level panic suppression remains in effect.
fn panic_response(_: Box<dyn std::any::Any + Send + 'static>) -> Response {
    let mut response = (
        StatusCode::INTERNAL_SERVER_ERROR,
        InterfaceError::RequestDispatchUnknown.message(),
    )
        .into_response();
    response
        .extensions_mut()
        .insert(InterfaceError::RequestDispatchUnknown);
    response
}

async fn request_admission(
    State(state): State<AppState>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    let Some(ticket) = state.lifecycle.try_admit() else {
        return crate::api::quiescing();
    };
    request.extensions_mut().insert(ticket.clone());
    // The independent middleware envelope outlives all handler captures, even
    // when an extractor consumes/drops the extension or the future is cancelled.
    let signal = request
        .extensions()
        .get::<crate::stream_owner::Disconnect>()
        .cloned()
        .unwrap_or_default();
    request.extensions_mut().insert(signal.clone());
    let mut guard = crate::stream_owner::DisconnectGuard(Some(signal));
    let response = ticket.track(admitted_request(state, request, next)).await;
    guard.0.take(); // API response body now owns cancellation.
    response
}

async fn admitted_request(state: AppState, mut request: Request<Body>, next: Next) -> Response {
    // Match the router's exact method/path semantics; query strings do not
    // affect routing. Wrong methods, trailing slashes and encoded aliases stay
    // outside heavy tracking and cannot reach a heavy handler.
    let endpoint = Endpoint::route(request.method().as_str(), request.uri().path());
    let (observation, context) = HttpObservation::new(state.telemetry.as_ref(), endpoint);
    request.extensions_mut().insert(context.clone());
    let api = crate::api::is_api(request.uri().path());
    let lease =
        if request.method() == Method::POST && request.uri().path() == "/v1/chat/completions" {
            let lease = Arc::new(Lease::tracking(
                state.heavy_activity.clone(),
                state.telemetry.as_ref(),
                Lane::Heavy,
            ));
            // Detached preflight, generation and delivery share THIS heavy marker.
            request.extensions_mut().insert(lease.clone());
            Some(lease)
        } else {
            None
        };
    let response = if api && (request.uri().query().is_some() || request.method() == Method::HEAD) {
        context.reject(AdmissionModel::Unknown, MetricRejection::Input);
        crate::api::failure(StatusCode::BAD_REQUEST, InterfaceError::InputRefused)
    } else {
        next.run(request).await
    };
    // Normalize even framework, panic, header and body-limit failures, while
    // retaining the SAME lifetime marker through the replacement response. A
    // typed source attached at the originating handler survives; an unclassified
    // framework response is dispatch-stage cause-unknown, never a guessed cause.
    let response = if api
        && !response.status().is_success()
        && response
            .extensions()
            .get::<crate::api::SafeError>()
            .is_none()
    {
        // Preserve any typed source attached at the originating handler and the
        // terminal-owner marker, so HTTP propagation never recounts a fault the
        // original operation already owns.
        let cause = response
            .extensions()
            .get::<InterfaceError>()
            .copied()
            .unwrap_or(InterfaceError::RequestDispatchUnknown);
        let owned = response
            .extensions()
            .get::<crate::api::FailureOwned>()
            .is_some();
        let mut normalized = crate::api::failure(response.status(), cause);
        if owned {
            normalized.extensions_mut().insert(crate::api::FailureOwned);
        }
        normalized
    } else {
        response
    };
    if response
        .extensions()
        .get::<crate::api::FailureOwned>()
        .is_none()
    {
        if let Some(cause) = response.extensions().get::<InterfaceError>().copied() {
            context.interface_error(cause);
        }
    }
    observed_response(response, lease, observation)
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
    let ingress = if chat {
        Some(Arc::new(Lease::tracking(
            state.ingress_activity.clone(),
            state.telemetry.as_ref(),
            Lane::Ingress,
        )))
    } else {
        None
    };
    let limit = if chat { BODY_LIMIT } else { CONTROL_BODY_LIMIT };
    let deadline = if chat {
        deadline
    } else {
        deadline.min(CONTROL_BODY_DEADLINE)
    };
    let (parts, body) = request.into_parts();
    let bytes = match tokio::time::timeout(deadline, collect_request(body, limit)).await {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(failure)) => {
            let io_failed = parts
                .extensions
                .get::<ConnectionIoFailure>()
                .is_some_and(|fault| fault.0.load(std::sync::atomic::Ordering::Acquire));
            if failure == RequestBodyFailure::Disconnected && !io_failed {
                return if crate::api::is_api(parts.uri.path()) {
                    crate::api::interrupted()
                } else {
                    let mut response =
                        (StatusCode::SERVICE_UNAVAILABLE, REQUEST_INTERRUPTED_MESSAGE)
                            .into_response();
                    response.extensions_mut().insert(crate::api::FailureOwned);
                    response
                };
            }
            observation.reject(AdmissionModel::Unknown, MetricRejection::Input);
            let (status, cause) = match failure {
                RequestBodyFailure::Limit => {
                    (StatusCode::PAYLOAD_TOO_LARGE, InterfaceError::BodyLimit)
                }
                RequestBodyFailure::ReadUnknown | RequestBodyFailure::Disconnected => (
                    StatusCode::SERVICE_UNAVAILABLE,
                    InterfaceError::RequestReadUnknown,
                ),
            };
            let io_owned = matches!(
                failure,
                RequestBodyFailure::ReadUnknown | RequestBodyFailure::Disconnected
            ) && io_failed;
            let mut response = body_failure_response(
                status,
                if io_owned {
                    InterfaceError::ConnectionUnknown
                } else {
                    cause
                },
            );
            if io_owned {
                response.extensions_mut().insert(crate::api::FailureOwned);
            }
            return response;
        }
        Err(_) => {
            observation.reject(AdmissionModel::Unknown, MetricRejection::Input);
            return body_failure_response(
                StatusCode::REQUEST_TIMEOUT,
                InterfaceError::BodyDeadline,
            );
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

/// Closed request-body acquisition outcome. The frame, underlying error and
/// collected bytes are never retained.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RequestBodyFailure {
    Limit,
    ReadUnknown,
    Disconnected,
}

pub(crate) const REQUEST_INTERRUPTED_MESSAGE: &str = "[request_interrupted] Stage: request_body; constraint: incomplete_upload. Observed disconnect/cancellation ended upload; no generation started by this incomplete submission. Not replayed. Prior accepted-work outcomes require authenticated receipts; a deliberate new submission may incur another charge. Check connection/service before a deliberate new submission.";

fn body_failure_response(status: StatusCode, cause: InterfaceError) -> Response {
    let mut response = (status, cause.message()).into_response();
    response.extensions_mut().insert(cause);
    response
}

async fn collect_request(mut body: Body, limit: usize) -> Result<Bytes, RequestBodyFailure> {
    // Do not collect a list of frames: a 32-MiB one-byte-fragmented request
    // otherwise retains millions of frame headers before coalescing. One fixed
    // allocation, one borrowed incoming frame, no growth/copy-overlap or queue.
    let mut bytes = Vec::with_capacity(limit);
    while let Some(frame) = poll_fn(|cx| Pin::new(&mut body).poll_frame(cx)).await {
        let frame = frame.map_err(|error| {
            let inner = error.into_inner();
            if inner.downcast_ref::<hyper::Error>().is_some_and(|error| {
                error.is_incomplete_message()
                    || error.is_canceled()
                    || std::error::Error::source(error)
                        .and_then(|source| source.downcast_ref::<std::io::Error>())
                        .is_some_and(|source| {
                            matches!(
                                source.kind(),
                                std::io::ErrorKind::UnexpectedEof
                                    | std::io::ErrorKind::ConnectionReset
                                    | std::io::ErrorKind::ConnectionAborted
                                    | std::io::ErrorKind::BrokenPipe
                                    | std::io::ErrorKind::NotConnected
                            )
                        })
            }) {
                RequestBodyFailure::Disconnected
            } else {
                RequestBodyFailure::ReadUnknown
            }
        })?;
        if let Ok(data) = frame.into_data() {
            if data.len() > limit - bytes.len() {
                return Err(RequestBodyFailure::Limit);
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
        let mut response = (
            StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE,
            "request headers too large",
        )
            .into_response();
        response
            .extensions_mut()
            .insert(InterfaceError::HeaderLimit);
        return response;
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
        Err(error) => {
            observation.reject(AdmissionModel::NotApplicable, MetricRejection::Verification);
            return unavailable(evidence_cause(error));
        }
    };
    let upstream = match state.inference.verification_document() {
        Ok(value) => value,
        Err(error) => {
            observation.reject(AdmissionModel::NotApplicable, MetricRejection::Internal);
            return unavailable(match error.failure() {
                crate::inference::InferenceFailure::RequestEncodingFailed => {
                    InterfaceError::ResponseEncodingUnknown
                }
                crate::inference::InferenceFailure::InferenceUnavailable => {
                    InterfaceError::EvidenceUnavailable
                }
                failure => failure.into(),
            });
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
        Err(error) => {
            observation.reject(AdmissionModel::NotApplicable, MetricRejection::Internal);
            return unavailable(encoding_cause(error));
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

fn evidence_cause(error: crate::attestation::EvidenceError) -> InterfaceError {
    match error {
        crate::attestation::EvidenceError::Unavailable => InterfaceError::EvidenceUnavailable,
        crate::attestation::EvidenceError::Invalid => InterfaceError::EvidenceInvalid,
    }
}

fn encoding_cause(error: crate::bounded_json::EncodingFailure) -> InterfaceError {
    match error {
        crate::bounded_json::EncodingFailure::LimitExceeded => {
            InterfaceError::ResponseEncodingLimit
        }
        crate::bounded_json::EncodingFailure::CauseUnknown => {
            InterfaceError::ResponseEncodingUnknown
        }
    }
}

fn unavailable(cause: InterfaceError) -> Response {
    let mut response = (StatusCode::SERVICE_UNAVAILABLE, cause.message()).into_response();
    response.extensions_mut().insert(cause);
    response
}
