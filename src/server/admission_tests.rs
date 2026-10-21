use super::*;
use crate::{
    attestation::UnavailableEvidenceVerifier,
    catalog::Model,
    inference::{Inference, InferenceError, Message},
};
use async_trait::async_trait;
use http_body_util::BodyExt;
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

#[tokio::test]
async fn chat_markers_do_not_reject_and_frames_pin_exact_lease() {
    let auth = Auth::from_json(&format!(
        r#"[{{"id":"a","credential_sha256":"{}","demo_microunits":100}}]"#,
        base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, [0; 32])
    ))
    .unwrap();
    let state = AppState::new(
        auth,
        Arc::new(Unused),
        "unused",
        Arc::new(UnavailableEvidenceVerifier),
    );
    let app = Router::new()
        .fallback(|request: Request<Body>| async move {
            let lease = request.extensions().get::<Arc<Lease>>();
            assert_eq!(
                lease.is_some(),
                request.method() == Method::POST && request.uri().path() == "/v1/chat/completions"
            );
            "frame".into_response()
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
    let mut responses = Vec::new();
    for _ in 0..9 {
        let response = app
            .clone()
            .oneshot(request(Method::POST, "/v1/chat/completions"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        responses.push(response);
    }
    assert_eq!(state.active_lanes(), [0, 9, 0]);
    let control = app
        .clone()
        .oneshot(request(Method::GET, "/v1/models"))
        .await
        .unwrap();
    assert_eq!(control.status(), StatusCode::OK);
    let mut controls = vec![control];
    for (method, uri) in [
        (Method::GET, "/v1/models"),
        (Method::POST, "/v1/submissions"),
        (Method::GET, "/attestation"),
        (Method::POST, "/v1/chat/completions/"),
    ] {
        let response = app.clone().oneshot(request(method, uri)).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        controls.push(response); // Leave every response unread while issuing the next.
    }
    assert_eq!(state.active_lanes(), [0, 9, 0]);
    drop(controls);
    let response = responses.pop().unwrap();
    let mut body = response.into_body();
    let frame = body.frame().await.unwrap().unwrap().into_data().unwrap();
    let slice = frame.slice(1..2);
    drop(frame);
    drop(body);
    assert_eq!(state.active_lanes(), [0, 9, 0]);
    drop(responses);
    assert_eq!(state.active_lanes(), [0, 1, 0]);
    drop(slice);
    assert_eq!(state.active_lanes(), [0, 0, 0]);
}
