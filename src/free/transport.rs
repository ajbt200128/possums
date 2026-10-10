use super::{
    queue::{Job, Position},
    AppState, Diagnostic, Failure, Stage, LIFETIME,
};
use crate::{
    stream_owner::{delivery, Limits},
    telemetry::hooks::Lease,
};
use axum::{
    body::{Body, Bytes, HttpBody},
    extract::State,
    http::{header, HeaderValue, Method, Request, StatusCode},
    response::{IntoResponse, Response},
    Router,
};
use hyper_util::{
    rt::{TokioIo, TokioTimer},
    service::TowerToHyperService,
};
use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    net::TcpListener,
    time::{Instant, Sleep},
};

const HEADER_BYTES: usize = 32 * 1024;
const HEADERS: usize = 64;
const UPLOAD_DEADLINE: Duration = Duration::from_secs(30);
const HEARTBEAT: Duration = Duration::from_secs(15);

#[derive(Clone)]
struct Connection {
    lease: Arc<Lease>,
    position: Arc<Position>,
}
struct CancelOnDrop(Arc<Position>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

impl AppState {
    fn connection(&self, accepted: Instant) -> Result<Connection, Diagnostic> {
        let permit = self
            .connections
            .clone()
            .try_acquire_owned()
            .map_err(|_| Diagnostic::local(Failure::Capacity, Stage::Admission))?;
        Ok(Connection {
            lease: Arc::new(Lease::from(permit)),
            position: Arc::new(Position::new(
                accepted + LIFETIME,
                Arc::downgrade(&self.queue),
            )),
        })
    }
}

pub fn router(state: AppState) -> Router {
    Router::new().fallback(handle).with_state(state)
}

pub async fn serve(listener: TcpListener, state: AppState) -> std::io::Result<()> {
    let app = router(state.clone());
    loop {
        let (stream, _) = listener.accept().await?;
        let accepted = Instant::now();
        let live = state.core.info.connected();
        let Ok(context) = state.connection(accepted) else {
            drop(stream);
            continue;
        };
        let service = TowerToHyperService::new(app.clone());
        tokio::spawn(async move {
            let _live = live;
            let _cancel = CancelOnDrop(context.position.clone());
            let deadline = context.position.deadline;
            let service =
                hyper::service::service_fn(move |mut request: Request<hyper::body::Incoming>| {
                    request.extensions_mut().insert(context.clone());
                    let service = service.clone();
                    async move { hyper::service::Service::call(&service, request).await }
                });
            let mut builder = hyper::server::conn::http1::Builder::new();
            builder
                .timer(TokioTimer::new())
                .header_read_timeout(Duration::from_secs(10))
                .max_headers(HEADERS)
                .max_buf_size(HEADER_BYTES)
                .keep_alive(false);
            // One accepted socket, one request, including upload/wait/delivery.
            // Dropping this HTTP future never cancels an already-started worker.
            let _ = tokio::time::timeout_at(
                deadline,
                builder.serve_connection(TokioIo::new(stream), service),
            )
            .await;
        });
    }
}

fn reject(failure: Failure, stage: Stage, status: StatusCode) -> Response {
    Diagnostic::local(failure, stage).response(status)
}

async fn handle(State(state): State<AppState>, mut request: Request<Body>) -> Response {
    // In-process router tests model one downstream request as a connection.
    let live = request
        .extensions()
        .get::<Connection>()
        .is_none()
        .then(|| state.core.info.connected());
    let context = match request.extensions_mut().remove::<Connection>() {
        Some(context) => context,
        None => match state.connection(Instant::now()) {
            Ok(context) => context,
            Err(error) => return error.response(StatusCode::SERVICE_UNAVAILABLE),
        },
    };
    let cancel = CancelOnDrop(context.position.clone());
    let response = route(&state, request, &context).await;
    let heartbeat = response
        .headers()
        .get(header::CONTENT_TYPE)
        .is_some_and(|v| v == "text/event-stream");
    let (mut parts, body) = response.into_parts();
    parts
        .headers
        .insert(header::CONNECTION, HeaderValue::from_static("close"));
    parts
        .headers
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    parts.headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    parts.headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'; base-uri 'none'; frame-ancestors 'none'"),
    );
    parts.headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    Response::from_parts(
        parts,
        Body::new(ResponseBody {
            inner: Some(body),
            lease: context.lease,
            cancel,
            _live: live,
            deadline: Box::pin(tokio::time::sleep_until(context.position.deadline)),
            heartbeat: heartbeat.then(|| Box::pin(tokio::time::sleep(HEARTBEAT))),
        }),
    )
}

async fn route(state: &AppState, request: Request<Body>, context: &Connection) -> Response {
    let headers = request.headers();
    let header_bytes = headers.iter().fold(0_usize, |size, (k, v)| {
        size.saturating_add(k.as_str().len())
            .saturating_add(v.as_bytes().len())
    });
    if header_bytes > HEADER_BYTES || headers.len() > HEADERS {
        return reject(
            Failure::HeadersTooLarge,
            Stage::Admission,
            StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE,
        );
    }
    if request.uri().query().is_some()
        || headers.contains_key(header::COOKIE)
        || headers.contains_key(header::CONTENT_ENCODING)
    {
        return reject(
            Failure::InvalidRequest,
            Stage::Admission,
            StatusCode::BAD_REQUEST,
        );
    }
    let chat = request.method() == Method::POST && request.uri().path() == "/v1/chat/completions";
    let models = request.method() == Method::GET && request.uri().path() == "/v1/models";
    let info = request.method() == Method::GET && request.uri().path() == "/v1/info";
    let index =
        request.method() == Method::GET && matches!(request.uri().path(), "/" | "/index.html");
    let attestation = request.method() == Method::GET && request.uri().path() == "/attestation";
    if !chat && !models && !info && !index && !attestation {
        return reject(Failure::NotFound, Stage::Admission, StatusCode::NOT_FOUND);
    }
    if (chat || models || info) && !state.keys.accepts(headers) {
        return reject(
            Failure::Unauthorized,
            Stage::Authentication,
            StatusCode::UNAUTHORIZED,
        );
    }
    if chat {
        let mut types = headers.get_all(header::CONTENT_TYPE).iter();
        if types.next().is_none_or(|v| v != "application/json") || types.next().is_some() {
            return reject(
                Failure::ContentType,
                Stage::Admission,
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
            );
        }
    } else if headers.contains_key(header::CONTENT_TYPE) {
        return reject(
            Failure::InvalidRequest,
            Stage::Admission,
            StatusCode::BAD_REQUEST,
        );
    }
    let deadline = context
        .position
        .deadline
        .min(Instant::now() + UPLOAD_DEADLINE);
    let limit = if chat {
        crate::server::BODY_LIMIT
    } else {
        4096
    };
    let raw = match tokio::time::timeout_at(
        deadline,
        crate::server::collect_request(request.into_body(), limit),
    )
    .await
    {
        Ok(Ok(raw)) => raw,
        Ok(Err(())) => {
            return reject(
                Failure::BodyTooLarge,
                Stage::Admission,
                StatusCode::PAYLOAD_TOO_LARGE,
            )
        }
        Err(_) => {
            return reject(
                Failure::UploadTimeout,
                Stage::Admission,
                StatusCode::REQUEST_TIMEOUT,
            )
        }
    };
    if !chat && !raw.is_empty() {
        return reject(
            Failure::InvalidRequest,
            Stage::Admission,
            StatusCode::BAD_REQUEST,
        );
    }
    if index {
        return match state.core.info.snapshot().append_to(&state.core.index) {
            Ok(text) => {
                ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], text).into_response()
            }
            Err(error) => error.response(StatusCode::SERVICE_UNAVAILABLE),
        };
    }
    if info {
        return match state.core.info.snapshot().bytes() {
            Ok(bytes) => ([(header::CONTENT_TYPE, "application/json")], bytes).into_response(),
            Err(error) => error.response(StatusCode::SERVICE_UNAVAILABLE),
        };
    }
    if chat {
        let (mut tx, body) = delivery(
            context.lease.clone(),
            Limits {
                frames: 8,
                payload_bytes: 64 * 1024,
                chunk_bytes: 8 * 1024,
            },
        );
        let _ = tx.try_send(b": queued\n\n");
        let job = Job {
            raw,
            position: context.position.clone(),
            lease: context.lease.clone(),
            tx,
        };
        if state.queue.push(job).is_err() {
            return reject(
                Failure::Capacity,
                Stage::Admission,
                StatusCode::SERVICE_UNAVAILABLE,
            );
        }
        return (
            [
                (header::CONTENT_TYPE, "text/event-stream"),
                (header::HeaderName::from_static("x-accel-buffering"), "no"),
            ],
            Body::new(body),
        )
            .into_response();
    }
    if models {
        let catalog = match state.core.catalog().await {
            Ok(c) => c,
            Err(e) => return e.response(StatusCode::SERVICE_UNAVAILABLE),
        };
        let data: Vec<_> = catalog.models.iter().map(|model| serde_json::json!({
            "id":model.id, "object":"model", "context_tokens":model.context_tokens.to_string(),
            "max_output_tokens":model.max_output_tokens.to_string(),
            "input_microunits_per_million_tokens":model.input_microunits_per_million_tokens.to_string(),
            "output_microunits_per_million_tokens":model.output_microunits_per_million_tokens.to_string(),
            "tool_protocol":state.core.inference.tool_profile(&model.id).map(|p| p.protocol())
        })).collect();
        return bounded_json(
            &serde_json::json!({"object":"list", "data":data}),
            crate::catalog::MAX_CATALOG_BYTES,
        );
    }
    let gateway = match state.core.evidence().await {
        Ok(v) => v,
        Err(e) => return e.response(StatusCode::SERVICE_UNAVAILABLE),
    };
    let upstream = match state.core.inference.verification_document() {
        Ok(v) => v,
        Err(e) => {
            return Diagnostic::upstream(e.failure(), Stage::Verification, false)
                .response(StatusCode::SERVICE_UNAVAILABLE)
        }
    };
    bounded_json(
        &serde_json::json!({"gateway":gateway, "upstream":upstream}),
        2 * 1024 * 1024,
    )
}

fn bounded_json(value: &impl serde::Serialize, cap: usize) -> Response {
    match crate::bounded_json::to_vec(value, cap) {
        Ok(bytes) => ([(header::CONTENT_TYPE, "application/json")], bytes).into_response(),
        Err(_) => Diagnostic::upstream(
            crate::inference::InferenceFailure::RequestEncodingFailed,
            Stage::Admission,
            false,
        )
        .response(StatusCode::SERVICE_UNAVAILABLE),
    }
}

struct ResponseBody {
    inner: Option<Body>,
    lease: Arc<Lease>,
    cancel: CancelOnDrop,
    _live: Option<super::info::LiveConnection>,
    deadline: Pin<Box<Sleep>>,
    heartbeat: Option<Pin<Box<Sleep>>>,
}
struct OwnedBytes {
    bytes: Bytes,
    _lease: Arc<Lease>,
}
impl AsRef<[u8]> for OwnedBytes {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

impl HttpBody for ResponseBody {
    type Data = Bytes;
    type Error = axum::Error;
    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Bytes>, Self::Error>>> {
        if self.deadline.as_mut().poll(cx).is_ready() {
            self.cancel.0.cancel();
            self.inner.take();
            return Poll::Ready(None);
        }
        let Some(body) = &mut self.inner else {
            return Poll::Ready(None);
        };
        match Pin::new(body).poll_frame(cx) {
            Poll::Ready(None) => {
                self.inner.take();
                Poll::Ready(None)
            }
            Poll::Ready(Some(result)) => Poll::Ready(Some(result.map(|frame| {
                frame.map_data(|bytes| {
                    Bytes::from_owner(OwnedBytes {
                        bytes,
                        _lease: self.lease.clone(),
                    })
                })
            }))),
            Poll::Pending => {
                if let Some(timer) = &mut self.heartbeat {
                    if timer.as_mut().poll(cx).is_ready() {
                        timer.as_mut().reset(Instant::now() + HEARTBEAT);
                        return Poll::Ready(Some(Ok(http_body::Frame::data(Bytes::from_static(
                            b": waiting\n\n",
                        )))));
                    }
                }
                Poll::Pending
            }
        }
    }
}
