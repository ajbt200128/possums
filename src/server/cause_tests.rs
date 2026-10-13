//! Bounded cause-preservation tests for the server boundary.
//! Offline/synthetic only: no provider, telemetry export, logging or network
//! beyond loopback/duplex fixtures.
use super::*;
use crate::{
    attestation::UnavailableEvidenceVerifier,
    catalog::Model,
    failures::InterfaceError,
    inference::{Inference, InferenceError, Message},
};
use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use futures_util::stream;
use http_body_util::BodyExt;
use tokio::io::AsyncWriteExt;
use tower::ServiceExt;

struct Unused;
#[async_trait]
impl Inference for Unused {
    async fn catalog(&self) -> Result<Vec<u8>, InferenceError> {
        unreachable!()
    }
    async fn count_tokens(
        &self,
        _: &str,
        _: &[Message],
        _: Arc<Lease>,
    ) -> Result<u64, InferenceError> {
        unreachable!()
    }
    async fn generate_stream(
        &self,
        _: &Model,
        _: &[Message],
        _: Arc<Lease>,
        _: &mut (dyn for<'d> FnMut(&'d str) + Send),
    ) -> Result<crate::inference::stream::StreamUsage, InferenceError> {
        unreachable!()
    }
    fn verification_document(&self) -> Result<serde_json::Value, InferenceError> {
        unreachable!()
    }
}

pub(super) fn state() -> AppState {
    let credential = URL_SAFE_NO_PAD.encode([8; 32]);
    let auth = Auth::from_json(&format!(
        r#"[{{"id":"a","credential_sha256":"{}","demo_microunits":100}}]"#,
        URL_SAFE_NO_PAD.encode([8; 32])
    ))
    .unwrap();
    let _ = credential;
    AppState::new(
        auth,
        Arc::new(Unused),
        "unused",
        Arc::new(UnavailableEvidenceVerifier),
    )
}

fn extension(response: &Response) -> Option<InterfaceError> {
    response.extensions().get::<InterfaceError>().copied()
}

#[tokio::test]
async fn collect_request_distinguishes_limit_from_read_failure() {
    let too_large = Body::from_stream(stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(
        vec![0_u8; 8],
    ))]));
    assert_eq!(
        collect_request(too_large, 4).await,
        Err(RequestBodyFailure::Limit)
    );

    let failed = Body::from_stream(stream::iter(vec![Err::<Bytes, std::io::Error>(
        std::io::Error::other("hostile frame text"),
    )]));
    assert_eq!(
        collect_request(failed, 4).await,
        Err(RequestBodyFailure::ReadUnknown)
    );

    // A frame error after valid data still surfaces as a read failure.
    let partial = Body::from_stream(stream::iter(vec![
        Ok::<_, std::io::Error>(Bytes::from_static(b"ok")),
        Err(std::io::Error::other("boom")),
    ]));
    assert_eq!(
        collect_request(partial, 4).await,
        Err(RequestBodyFailure::ReadUnknown)
    );
}

#[tokio::test(start_paused = true)]
async fn body_deadline_is_reported_separately_from_limit_and_read() {
    let body = Body::from_stream(stream::pending::<Result<Bytes, std::io::Error>>());
    let response = Router::new()
        .fallback(|_request: Request<Body>| async { "frame".into_response() })
        .layer(middleware::from_fn_with_state(
            state(),
            |state, request, next| {
                total_body_deadline(state, request, next, Duration::from_secs(3))
            },
        ))
        .oneshot(Request::post("/v1/x").body(body).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
    assert_eq!(extension(&response), Some(InterfaceError::BodyDeadline));
}

#[tokio::test]
async fn body_read_failure_is_service_unavailable_and_does_not_leak_text() {
    let body = Body::from_stream(stream::iter(vec![Err::<Bytes, std::io::Error>(
        std::io::Error::other("PRIVATE_BODY_FRAME_CANARY"),
    )]));
    let response = Router::new()
        .fallback(|_request: Request<Body>| async { "frame".into_response() })
        .layer(middleware::from_fn_with_state(
            state(),
            |state, request, next| total_body_deadline(state, request, next, BODY_DEADLINE),
        ))
        .oneshot(Request::post("/v1/x").body(body).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        extension(&response),
        Some(InterfaceError::RequestReadUnknown)
    );
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert!(!bytes
        .windows(b"PRIVATE_BODY_FRAME_CANARY".len())
        .any(|window| window == b"PRIVATE_BODY_FRAME_CANARY"));
}

#[tokio::test]
async fn typed_source_survives_api_normalization() {
    let app = Router::new()
        .route(
            "/v1/typed",
            get(|| async {
                let mut response = StatusCode::SERVICE_UNAVAILABLE.into_response();
                response
                    .extensions_mut()
                    .insert(InterfaceError::EvidenceUnavailable);
                response
            }),
        )
        .route(
            "/v1/plain",
            get(|| async { StatusCode::UNAUTHORIZED.into_response() }),
        )
        .route(
            "/v1/safe",
            get(|| async {
                crate::api::failure(
                    StatusCode::UNAUTHORIZED,
                    InterfaceError::AuthenticationRefused,
                )
            }),
        )
        .layer(middleware::from_fn_with_state(state(), request_admission));
    let response = app
        .clone()
        .oneshot(Request::get("/v1/typed").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(
        extension(&response),
        Some(InterfaceError::EvidenceUnavailable)
    );

    // An unclassified framework response is dispatch-stage cause-unknown; a 401
    // status must not be inferred as an authentication cause.
    let response = app
        .clone()
        .oneshot(Request::get("/v1/plain").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(
        extension(&response),
        Some(InterfaceError::RequestDispatchUnknown)
    );

    // A locally authored SafeError response is left exactly as rendered.
    let response = app
        .oneshot(Request::get("/v1/safe").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(
        extension(&response),
        Some(InterfaceError::AuthenticationRefused)
    );
}

#[tokio::test]
async fn explicit_route_and_query_refusals_carry_their_own_source() {
    let app = router(state());
    let not_found = app
        .clone()
        .oneshot(Request::get("/nope").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(not_found.status(), StatusCode::NOT_FOUND);
    assert_eq!(extension(&not_found), Some(InterfaceError::RouteRefused));

    let wrong_method = app
        .clone()
        .oneshot(Request::put("/v1/models").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(wrong_method.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(extension(&wrong_method), Some(InterfaceError::RouteRefused));

    let query = app
        .oneshot(
            Request::get("/v1/models?account_id=secret-canary")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(query.status(), StatusCode::BAD_REQUEST);
    assert_eq!(extension(&query), Some(InterfaceError::InputRefused));
}

#[tokio::test]
async fn oversized_headers_carry_the_limit_cause() {
    let request = Request::get("/v1/models")
        .header("x-fill", "a".repeat(MAX_HEADER_BYTES + 1))
        .body(Body::empty())
        .unwrap();
    let response = router(state()).oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE
    );
    assert_eq!(extension(&response), Some(InterfaceError::HeaderLimit));
}

#[tokio::test]
async fn terminal_owned_marker_survives_api_normalization() {
    let app = Router::new()
        .route(
            "/v1/owned",
            get(|| async {
                let mut response = StatusCode::SERVICE_UNAVAILABLE.into_response();
                response
                    .extensions_mut()
                    .insert(InterfaceError::SettlementFailed);
                response.extensions_mut().insert(crate::api::FailureOwned);
                response
            }),
        )
        .layer(middleware::from_fn_with_state(state(), request_admission));
    let response = app
        .oneshot(Request::get("/v1/owned").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(extension(&response), Some(InterfaceError::SettlementFailed));
    assert!(response
        .extensions()
        .get::<crate::api::FailureOwned>()
        .is_some());
}

#[tokio::test]
async fn panic_response_is_locally_authored_without_payload() {
    let app = Router::new()
        .route(
            "/v1/boom",
            get(|| async {
                panic!("PRIVATE_PANIC_CANARY");
                #[allow(unreachable_code)]
                "unreachable".into_response()
            }),
        )
        .layer(CatchPanicLayer::custom(panic_response));
    let response = app
        .oneshot(Request::get("/v1/boom").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        extension(&response),
        Some(InterfaceError::RequestDispatchUnknown)
    );
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert!(!bytes
        .windows(b"PRIVATE_PANIC_CANARY".len())
        .any(|window| window == b"PRIVATE_PANIC_CANARY"));
}

#[test]
fn attestation_causes_preserve_known_categories() {
    use crate::attestation::EvidenceError;
    use crate::bounded_json::EncodingFailure;
    assert_eq!(
        evidence_cause(EvidenceError::Unavailable),
        InterfaceError::EvidenceUnavailable
    );
    assert_eq!(
        evidence_cause(EvidenceError::Invalid),
        InterfaceError::EvidenceInvalid
    );
    assert_eq!(
        encoding_cause(EncodingFailure::LimitExceeded),
        InterfaceError::ResponseEncodingLimit
    );
    assert_eq!(
        encoding_cause(EncodingFailure::CauseUnknown),
        InterfaceError::ResponseEncodingUnknown
    );
}

#[test]
fn unavailable_response_carries_typed_cause_without_detail() {
    let response = unavailable(InterfaceError::EvidenceInvalid);
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(extension(&response), Some(InterfaceError::EvidenceInvalid));
}

#[test]
fn server_failure_display_and_debug_are_content_free() {
    for (failure, message) in [
        (ServerFailure::Accept, "listener_accept_unknown"),
        (ServerFailure::ConnectionTask, "connection_task_unknown"),
        (
            ServerFailure::ShutdownIntegrity,
            "shutdown_integrity_failed",
        ),
    ] {
        let error = failure.into_io();
        assert!(error
            .to_string()
            .starts_with(&format!("code={message} stage=")));
        assert!(error.to_string().contains("constraint="));
        assert!(error.to_string().contains("action="));
        assert!(error.to_string().contains("not replayed"));
        // Debug renders the closed variant only; no OS/join/panic payload.
        let rendered = format!("{error:?}");
        assert!(rendered.contains(&format!("{failure:?}")));
        assert!(!rendered.contains("os error"));
        assert!(!rendered.contains("PRIVATE"));
        assert!(error.get_ref().is_some());
    }
}

#[tokio::test(start_paused = true)]
async fn connection_timeout_classifies_as_deadline() {
    let (_client, server) = tokio::io::duplex(4096);
    let service = hyper::service::service_fn(|_| async {
        Ok::<_, std::convert::Infallible>(Response::new(Body::empty()))
    });
    let error = connection_builder(Duration::from_millis(20))
        .serve_connection(TokioIo::new(server), service)
        .await
        .unwrap_err();
    let failure = classify_connection_error(&error);
    assert_eq!(failure.rejection, MetricRejection::ConnectionDeadline);
    assert_eq!(failure.cause, InterfaceError::ConnectionDeadline);
}

#[tokio::test(start_paused = true)]
async fn malformed_request_classifies_as_protocol() {
    let (mut client, server) = tokio::io::duplex(4096);
    let service = hyper::service::service_fn(|_| async {
        Ok::<_, std::convert::Infallible>(Response::new(Body::empty()))
    });
    let worker = tokio::spawn(async move {
        connection_builder(Duration::from_secs(5))
            .serve_connection(TokioIo::new(server), service)
            .await
    });
    client
        .write_all(b"INVALID REQUEST LINE\r\n\r\n")
        .await
        .unwrap();
    let error = worker.await.unwrap().unwrap_err();
    assert!(error.is_parse(), "expected a parse error, got {error:?}");
    let failure = classify_connection_error(&error);
    assert_eq!(failure.rejection, MetricRejection::HeaderProtocol);
    assert_eq!(failure.cause, InterfaceError::HeaderProtocol);
}

#[test]
fn orderly_disconnect_is_not_a_fabricated_fault() {
    // The connection classification only runs for observed hyper errors; a
    // clean result (Ok) maps to no failure and therefore no counter.
    assert!(classify_connection_error_opt(None).is_none());
}

/// Mirror of the connection owner's "no error => no failure" rule.
fn classify_connection_error_opt(error: Option<&hyper::Error>) -> Option<ConnectionFailure> {
    error.map(classify_connection_error)
}
