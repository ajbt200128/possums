//! Offline ownership qualification; eligibility priming is not window release.
use super::*;
use crate::telemetry::{AggregateMetrics, Deployment, SystemClock};
use futures_util::stream;
use http_body_util::BodyExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tower::ServiceExt;

fn metrics() -> Arc<AggregateMetrics> {
    let metrics = Arc::new(AggregateMetrics::new(
        Deployment::Production,
        SystemClock::default(),
    ));
    metrics.prime_synthetic_application_window();
    metrics
}
fn state(metrics: &Arc<AggregateMetrics>) -> AppState {
    super::cause_tests::state().with_telemetry(Some(metrics.clone()))
}
async fn send(app: &Router, request: Request<Body>, status: StatusCode) -> Bytes {
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), status);
    response.into_body().collect().await.unwrap().to_bytes()
}
fn request(method: Method, path: &str, body: Body) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .body(body)
        .unwrap()
}
fn only(metrics: &AggregateMetrics, counts: &[(InterfaceError, u64)]) {
    for error in InterfaceError::ALL {
        assert_eq!(
            metrics.interface_error_count(error),
            counts
                .iter()
                .find(|(kind, _)| *kind == error)
                .map(|(_, count)| *count),
            "{error:?}"
        );
    }
}

pub(crate) async fn authentication_refused_fixture(
    metrics: &Arc<AggregateMetrics>,
) -> Arc<crate::accounting::Accounting> {
    let state = state(metrics);
    let accounting = state.accounting.clone();
    let before = accounting.snapshot("a").unwrap();
    let response = send(
        &router(state),
        Request::get("/v1/models")
            .header("x-hostile", "HOSTILE_ROUTER_CANARY")
            .body(Body::empty())
            .unwrap(),
        StatusCode::UNAUTHORIZED,
    )
    .await;
    assert!(!response
        .windows(b"HOSTILE_ROUTER_CANARY".len())
        .any(|bytes| bytes == b"HOSTILE_ROUTER_CANARY"));
    only(metrics, &[(InterfaceError::AuthenticationRefused, 1)]);
    assert_eq!(accounting.snapshot("a").unwrap(), before);
    accounting
}

#[tokio::test]
async fn router_origins_are_counted_once_after_normalization() {
    let metrics = metrics();
    let app = router(state(&metrics));
    send(
        &app,
        request(Method::GET, "/v1/models", Body::empty()),
        StatusCode::UNAUTHORIZED,
    )
    .await;
    send(
        &app,
        request(Method::GET, "/v1/auth/challenge", Body::from("bad")),
        StatusCode::BAD_REQUEST,
    )
    .await;
    send(
        &app,
        request(Method::GET, "/v1/models?bad", Body::empty()),
        StatusCode::BAD_REQUEST,
    )
    .await;
    send(
        &app,
        request(Method::GET, "/missing", Body::empty()),
        StatusCode::NOT_FOUND,
    )
    .await;
    only(
        &metrics,
        &[
            (InterfaceError::AuthenticationRefused, 1),
            (InterfaceError::InputRefused, 2),
            (InterfaceError::RouteRefused, 1),
        ],
    );
}

#[tokio::test]
async fn body_header_and_attestation_sources_keep_their_own_counts_and_diagnostics() {
    let metrics = metrics();
    let app = router(state(&metrics));
    let bytes = send(
        &app,
        request(
            Method::GET,
            "/attestation",
            Body::from(vec![0; CONTROL_BODY_LIMIT + 1]),
        ),
        StatusCode::PAYLOAD_TOO_LARGE,
    )
    .await;
    let message = std::str::from_utf8(&bytes).unwrap();
    assert!(message.contains("[body_limit]"));
    assert!(message.contains("Stage: request_body"));
    assert!(message.contains("constraint:"));
    assert!(message.contains("Check"));
    let mut headers = request(Method::GET, "/v1/models", Body::empty());
    headers.headers_mut().insert(
        "x-large",
        HeaderValue::from_bytes(&vec![b'a'; MAX_HEADER_BYTES + 1]).unwrap(),
    );
    send(&app, headers, StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE).await;
    let evidence = send(
        &app,
        request(Method::GET, "/attestation", Body::empty()),
        StatusCode::SERVICE_UNAVAILABLE,
    )
    .await;
    assert!(std::str::from_utf8(&evidence)
        .unwrap()
        .contains("[evidence_unavailable]"));
    only(
        &metrics,
        &[
            (InterfaceError::BodyLimit, 1),
            (InterfaceError::HeaderLimit, 1),
            (InterfaceError::EvidenceUnavailable, 1),
        ],
    );
}

#[tokio::test]
async fn unexpected_body_sdk_error_is_counted_but_physical_io_propagation_is_not() {
    let metrics = metrics();
    let app = router(state(&metrics));
    let body = || {
        Body::from_stream(stream::iter([Err::<Bytes, _>(std::io::Error::other(
            "HOSTILE_BODY_CANARY",
        ))]))
    };
    let bytes = send(
        &app,
        request(Method::POST, "/v1/balance", body()),
        StatusCode::SERVICE_UNAVAILABLE,
    )
    .await;
    assert!(!std::str::from_utf8(&bytes)
        .unwrap()
        .contains("HOSTILE_BODY_CANARY"));
    // The independent connection owner already observed this physical I/O fault.
    metrics.interface_error(InterfaceError::ConnectionUnknown);
    let mut propagated = request(Method::POST, "/v1/balance", body());
    propagated
        .extensions_mut()
        .insert(ConnectionIoFailure(Arc::new(
            std::sync::atomic::AtomicBool::new(true),
        )));
    let bytes = send(&app, propagated, StatusCode::SERVICE_UNAVAILABLE).await;
    assert!(std::str::from_utf8(&bytes)
        .unwrap()
        .contains("[connection_unknown]"));
    only(
        &metrics,
        &[
            (InterfaceError::RequestReadUnknown, 1),
            (InterfaceError::ConnectionUnknown, 1),
        ],
    );
}

#[tokio::test(start_paused = true)]
async fn body_deadline_remains_an_independent_actual_constraint() {
    let metrics = metrics();
    let app = router_with_body_deadline(state(&metrics), Duration::from_millis(1));
    let body = Body::from_stream(stream::pending::<Result<Bytes, std::io::Error>>());
    let bytes = send(
        &app,
        request(Method::POST, "/v1/chat/completions", body),
        StatusCode::REQUEST_TIMEOUT,
    )
    .await;
    assert!(std::str::from_utf8(&bytes)
        .unwrap()
        .contains("[body_deadline]"));
    only(&metrics, &[(InterfaceError::BodyDeadline, 1)]);
}

fn wrapped_app(
    state: AppState,
    response: impl Fn() -> Response + Clone + Send + Sync + 'static,
) -> Router {
    Router::new()
        .route(
            "/v1/test",
            get(move || {
                let response = response.clone();
                async move { response() }
            }),
        )
        .layer(middleware::from_fn_with_state(state, request_admission))
}

#[tokio::test]
async fn terminal_owned_failure_and_ordinary_interruption_are_not_recounted() {
    let metrics = metrics();
    metrics.interface_error(InterfaceError::PreflightDeadline);
    let app = wrapped_app(state(&metrics), || {
        // Force ordinary framework normalization while retaining the source owner.
        let mut response = StatusCode::SERVICE_UNAVAILABLE.into_response();
        response
            .extensions_mut()
            .insert(InterfaceError::PreflightDeadline);
        response.extensions_mut().insert(crate::api::FailureOwned);
        response
    });
    send(
        &app,
        request(Method::GET, "/v1/test", Body::empty()),
        StatusCode::SERVICE_UNAVAILABLE,
    )
    .await;
    let app = wrapped_app(state(&metrics), crate::api::interrupted);
    let bytes = send(
        &app,
        request(Method::GET, "/v1/test", Body::empty()),
        StatusCode::SERVICE_UNAVAILABLE,
    )
    .await;
    assert!(std::str::from_utf8(&bytes)
        .unwrap()
        .contains("[request_interrupted]"));
    only(&metrics, &[(InterfaceError::PreflightDeadline, 1)]);
}

#[tokio::test]
async fn ordinary_upload_eof_is_not_a_fabricated_read_or_connection_fault() {
    let metrics = metrics();
    let state = state(&metrics);
    let (done, finished) = tokio::sync::oneshot::channel();
    *state.server_hooks.connection_finished.lock().unwrap() = Some(done);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(serve_until(listener, state.clone(), async {
        let _ = stopped.await;
    }));
    let mut client = tokio::net::TcpStream::connect(address).await.unwrap();
    client
        .write_all(b"POST /v1/balance HTTP/1.1\r\nHost: fixture\r\nContent-Length: 32\r\n\r\n{}")
        .await
        .unwrap();
    client.shutdown().await.unwrap();
    let mut reply = Vec::new();
    tokio::time::timeout(Duration::from_secs(2), client.read_to_end(&mut reply))
        .await
        .unwrap()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), finished)
        .await
        .unwrap()
        .unwrap();
    // Delivery is not guaranteed after EOF; if delivered, the response remains
    // diagnostic only and cannot invent a charge/refund or completed generation.
    if !reply.is_empty() {
        assert!(std::str::from_utf8(&reply)
            .unwrap()
            .contains("[request_interrupted]"));
    }
    only(&metrics, &[]);
    let account = state.accounting.snapshot("a").unwrap();
    assert_eq!(account.in_flight, 0);
    assert_eq!(account.completed_requests, 0);
    stop.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}
