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
async fn heavy_saturation_keeps_control_independent_and_frames_pin_exact_lease() {
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
    let other_heavy = state.chat_memory.clone().try_acquire_many_owned(3).unwrap();
    let response = app
        .clone()
        .oneshot(request(Method::POST, "/v1/chat/completions"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(state.chat_memory.available_permits(), 0);
    assert_eq!(state.chat_ingress.available_permits(), CHAT_LANES);
    assert_eq!(
        app.clone()
            .oneshot(request(Method::POST, "/v1/chat/completions"))
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    let control = app
        .clone()
        .oneshot(request(Method::GET, "/v1/models"))
        .await
        .unwrap();
    assert_eq!(control.status(), StatusCode::OK);
    assert_eq!(state.control_memory.available_permits(), 0);
    assert_eq!(
        app.clone()
            .oneshot(request(Method::GET, "/v1/models"))
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    drop(control);
    let mut body = response.into_body();
    let frame = body.frame().await.unwrap().unwrap().into_data().unwrap();
    let slice = frame.slice(1..2);
    drop(frame);
    drop(body);
    assert_eq!(state.chat_memory.available_permits(), 0);
    drop(slice);
    assert_eq!(state.chat_memory.available_permits(), 1);
    drop(other_heavy);
    assert_eq!(state.chat_memory.available_permits(), CHAT_LANES);
    assert_eq!(state.control_memory.available_permits(), CONTROL_LANES);
}
