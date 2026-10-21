use super::*;
use http_body_util::BodyExt as _;
use std::sync::atomic::{AtomicUsize, Ordering};
use tower::ServiceExt as _;

#[tokio::test]
async fn capability_projects_only_typed_errors_and_preserves_default_wire_and_extensions() {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    async fn handler() -> Response {
        CALLS.fetch_add(1, Ordering::SeqCst);
        let mut response = failure(
            StatusCode::SERVICE_UNAVAILABLE,
            InterfaceError::SettlementFailed,
        );
        response.extensions_mut().insert(FailureOwned);
        response
            .extensions_mut()
            .insert(crate::intake::diagnostics::Diagnostic::hold_unusable());
        response
            .headers_mut()
            .insert("x-fixture-preserved", "true".parse().unwrap());
        response
    }
    let app = Router::new()
        .route("/v1/fixture", get(handler))
        .route_layer(middleware::from_fn(intake_diagnostics));
    let mut default = None;
    for (index, values) in [
        vec![],
        vec!["v2"],
        vec!["v1, v1"],
        vec!["v1", "v1"],
        vec!["v1"],
    ]
    .into_iter()
    .enumerate()
    {
        let mut request = Request::builder()
            .uri("/v1/fixture")
            .body(Body::empty())
            .unwrap();
        for value in values {
            request
                .headers_mut()
                .append(crate::intake::diagnostics::HEADER, value.parse().unwrap());
        }
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(CALLS.load(Ordering::SeqCst), index + 1); // No diagnostic work/replay.
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(response.extensions().get::<SafeError>().is_some());
        assert!(response.extensions().get::<FailureOwned>().is_some());
        assert_eq!(
            response.extensions().get::<InterfaceError>(),
            Some(&InterfaceError::SettlementFailed)
        );
        assert_eq!(response.headers()["x-fixture-preserved"], "true");
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body.as_object().unwrap().len(), 1);
        if index < 4 {
            assert_eq!(body["error"].as_object().unwrap().len(), 2);
            if let Some(original) = &default {
                assert_eq!(&bytes, original);
            } else {
                default = Some(bytes);
            }
        } else {
            assert_eq!(body["error"].as_object().unwrap().len(), 3);
            assert_eq!(
                body["error"]["intake"],
                serde_json::json!({
                    "code":"INTAKE_HOLD_UNUSABLE", "operation":"intake_reserve",
                    "stage":"admission", "constraint":"usable_hold", "status":null
                })
            );
        }
    }
    let plain = failure(StatusCode::BAD_REQUEST, InterfaceError::InputRefused);
    let plain = project_intake_diagnostic(plain, true).await;
    let bytes = plain.into_body().collect().await.unwrap().to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["error"].as_object().unwrap().len(), 2);
}
