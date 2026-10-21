use async_trait::async_trait;
use axum::{
    body::Body,
    http::{header, Request, StatusCode},
    response::Response,
    Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use http_body_util::BodyExt;
use possums::{
    attestation::{EvidenceError, EvidenceVerifier, GatewayEvidence},
    auth::Auth,
    inference::{Inference, InferenceError, Message},
    server::{router, router_with_body_deadline, AppState},
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{sync::Arc, time::Duration};
use tower::ServiceExt;

struct Fixture;
#[async_trait]
impl EvidenceVerifier for Fixture {
    async fn verify(&self, _: &str, now: u64) -> Result<GatewayEvidence, EvidenceError> {
        Ok(GatewayEvidence {
            quote: json!({}),
            issued_at_unix: now,
            release_digest: String::new(),
            endpoint_key_sha256: String::new(),
            freshness_expires_at_unix: now + 60,
        })
    }
}
#[async_trait]
impl Inference for Fixture {
    async fn catalog(&self) -> Result<Vec<u8>, InferenceError> {
        let data = ["org/model:v1", "other"].map(|id| json!({"id":id,"type":"chat","context_window":20,"endpoints":["/v1/chat/completions"],"pricing":{"inputTokenPricePer1M":1,"outputTokenPricePer1M":1,"requestPrice":0}}));
        Ok(json!({"object":"list", "data": data})
            .to_string()
            .into_bytes())
    }
    async fn count_tokens(
        &self,
        _: &str,
        _: &[Message],
        _: Arc<possums::telemetry::hooks::Lease>,
    ) -> Result<u64, InferenceError> {
        panic!("control cannot tokenize")
    }
    fn verification_document(&self) -> Result<Value, InferenceError> {
        Err(InferenceError::Unavailable)
    }
}
fn fixture(capacity: usize) -> (AppState, String) {
    let credential = URL_SAFE_NO_PAD.encode([7; 32]);
    let config = json!([{"id":"demo", "credential_sha256":URL_SAFE_NO_PAD.encode(Sha256::digest(credential.as_bytes())), "demo_microunits":5_000_000}]);
    let auth = Auth::from_json(&config.to_string())
        .unwrap()
        .with_submission_capacity(capacity);
    (
        AppState::new(auth, Arc::new(Fixture), "unused", Arc::new(Fixture)),
        credential,
    )
}
async fn response(app: &Router, request: Request<Body>, status: StatusCode) -> Value {
    let res = app.clone().oneshot(request).await.unwrap();
    assert_eq!(res.status(), status);
    read(res).await
}
async fn read(res: Response) -> Value {
    assert_eq!(res.headers()[header::CACHE_CONTROL], "no-store");
    assert!(!res.headers().contains_key(header::SET_COOKIE));
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    if status == StatusCode::NO_CONTENT {
        assert!(bytes.is_empty());
        return Value::Null;
    }
    assert!(bytes.len() <= 4096);
    let mut value: Value = serde_json::from_slice(&bytes).unwrap();
    if !status.is_success() {
        assert_eq!(value.as_object().unwrap().len(), 1);
        assert_eq!(value["error"].as_object().unwrap().len(), 2);
        assert!(value["error"]["code"].is_string());
        let message = value["error"]["message"].as_str().unwrap();
        assert!(message.contains("Stage:"));
        assert!(message.contains("constraint:"));
        assert!(message.contains("Not replayed."));
        assert!(bytes.len() < 1024);
        // Check the bounded optional diagnostic above, then retain exact legacy
        // code projections in the assertions below.
        value["error"].as_object_mut().unwrap().remove("message");
    }
    value
}
async fn challenge(app: &Router) -> String {
    response(
        app,
        Request::builder()
            .uri("/v1/auth/challenge")
            .body(Body::empty())
            .unwrap(),
        StatusCode::OK,
    )
    .await["challenge"]
        .as_str()
        .unwrap()
        .to_owned()
}
async fn login(app: &Router, credential: &str) -> String {
    let challenge = challenge(app).await;
    let value = response(
        app,
        Request::post("/v1/sessions")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"challenge":challenge,"credential":credential}).to_string(),
            ))
            .unwrap(),
        StatusCode::OK,
    )
    .await;
    assert_eq!(value["token_type"], "Bearer");
    assert_eq!(value["expires_in"], 43200);
    assert_eq!(value.as_object().unwrap().len(), 3);
    value["token"].as_str().unwrap().to_owned()
}
fn submit(bearer: &str, model: &str, reset: bool) -> Request<Body> {
    Request::post("/v1/submissions")
        .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({"model":model,"new_conversation":reset}).to_string(),
        ))
        .unwrap()
}

#[tokio::test]
async fn cookie_free_login_single_use_model_binding_reset_and_logout() {
    let (state, credential) = fixture(1);
    let app = router(state.clone());
    let challenge = challenge(&app).await;
    let body = json!({"challenge":challenge,"credential":credential}).to_string();
    let first = response(
        &app,
        Request::post("/v1/sessions")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.clone()))
            .unwrap(),
        StatusCode::OK,
    )
    .await;
    response(
        &app,
        Request::post("/v1/sessions")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .unwrap(),
        StatusCode::UNAUTHORIZED,
    )
    .await;
    let bearer = first["token"].as_str().unwrap();
    let original = state.auth.api_session(bearer).unwrap();
    let first = response(&app, submit(bearer, "org/model:v1", false), StatusCode::OK).await;
    assert_eq!(first.as_object().unwrap().len(), 1);
    assert_eq!(first["submission"].as_str().unwrap().len(), 43);
    response(
        &app,
        submit(bearer, "org/model:v1", false),
        StatusCode::SERVICE_UNAVAILABLE,
    )
    .await;
    response(&app, submit(bearer, "other", false), StatusCode::CONFLICT).await;
    response(
        &app,
        submit(bearer, "unknown", true),
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert_eq!(
        state.auth.api_session(bearer).unwrap().conversation,
        original.conversation
    );
    response(&app, submit(bearer, "other", true), StatusCode::OK).await;
    assert_ne!(
        state.auth.api_session(bearer).unwrap().conversation,
        original.conversation
    );
    response(
        &app,
        Request::delete("/v1/sessions/current")
            .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
            .body(Body::empty())
            .unwrap(),
        StatusCode::NO_CONTENT,
    )
    .await;
    response(
        &app,
        submit(bearer, "other", false),
        StatusCode::UNAUTHORIZED,
    )
    .await;
    let next = login(&app, &credential).await;
    assert!(next != bearer);
    assert_eq!(state.accounting.available("demo"), Some(5_000_000));
}

#[tokio::test]
async fn rejects_missing_duplicate_malformed_and_cookie_authentication() {
    let (state, credential) = fixture(10);
    let app = router(state.clone());
    let api_id = login(&app, &credential).await;
    let valid = format!("Bearer {api_id}");
    for value in [
        None,
        Some(""),
        Some("Bearer"),
        Some("bearer bad"),
        Some("Basic bad"),
        Some("Bearer bad"),
        Some("Bearer  bad"),
        Some(" Bearer bad"),
        Some("Bearer bad,bad"),
        Some("Bearer bad\t"),
        Some(credential.as_str()),
    ] {
        let mut builder = Request::get("/v1/models");
        if let Some(value) = value {
            builder = builder.header(header::AUTHORIZATION, value);
        }
        response(
            &app,
            builder.body(Body::empty()).unwrap(),
            StatusCode::UNAUTHORIZED,
        )
        .await;
    }
    for id in [&credential] {
        response(
            &app,
            Request::get("/v1/models")
                .header(header::AUTHORIZATION, format!("Bearer {id}"))
                .body(Body::empty())
                .unwrap(),
            StatusCode::UNAUTHORIZED,
        )
        .await;
    }
    response(
        &app,
        Request::get("/v1/models")
            .header(header::AUTHORIZATION, &valid)
            .header(header::AUTHORIZATION, &valid)
            .body(Body::empty())
            .unwrap(),
        StatusCode::UNAUTHORIZED,
    )
    .await;
    for with_bearer in [false, true] {
        let mut builder =
            Request::get("/v1/models").header(header::COOKIE, format!("possums_session={api_id}"));
        if with_bearer {
            builder = builder.header(header::AUTHORIZATION, &valid);
        }
        response(
            &app,
            builder.body(Body::empty()).unwrap(),
            StatusCode::BAD_REQUEST,
        )
        .await;
    }
}

#[tokio::test]
async fn strict_json_media_types_and_hostile_controls_are_bounded_and_sanitized() {
    let (state, credential) = fixture(10);
    let app = router(state);
    let bearer = login(&app, &credential).await;
    let valid = json!({"model":"org/model:v1","new_conversation":false}).to_string();
    for body in [
        "{}".to_owned(),
        "[]".into(),
        r#"["org/model:v1",false]"#.into(),
        "null".into(),
        r#"{"model":"org/model:v1","new_conversation":false,"extra":1}"#.into(),
        r#"{"model":"other","model":"org/model:v1","new_conversation":false}"#.into(),
        r#"{"model":"org/model:v1","new_conversation":false,"new_conversation":true}"#.into(),
        r#"{"model":"org/model:v1","new_conversation":"false"}"#.into(),
        format!("{valid}{{}}"),
        format!(
            "{{\"model\":{},\"new_conversation\":false}}",
            "[".repeat(150)
        ),
        format!("{{\"{}\":0}}", "x".repeat(3000)),
    ] {
        response(
            &app,
            Request::post("/v1/submissions")
                .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .unwrap(),
            StatusCode::BAD_REQUEST,
        )
        .await;
    }
    for body in [
        r#"{"challenge":"x","challenge":"y","credential":"secret-canary"}"#,
        r#"{"challenge":"x","credential":"secret-canary","credential":"z"}"#,
        r#"{"challenge":"x","credential":"secret-canary","extra":1}"#,
        r#"["x","secret-canary"]"#,
    ] {
        let value = response(
            &app,
            Request::post("/v1/sessions")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .unwrap(),
            StatusCode::BAD_REQUEST,
        )
        .await;
        assert!(!value.to_string().contains("secret-canary"));
    }
    for content_type in [
        None,
        Some("text/plain"),
        Some("application/x-www-form-urlencoded"),
        Some("application/json; charset=utf-8"),
    ] {
        let mut req = Request::post("/v1/submissions")
            .header(header::AUTHORIZATION, format!("Bearer {bearer}"));
        if let Some(value) = content_type {
            req = req.header(header::CONTENT_TYPE, value);
        }
        response(
            &app,
            req.body(Body::from(valid.clone())).unwrap(),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        )
        .await;
    }
    let mut duplicate = submit(&bearer, "other", false);
    duplicate
        .headers_mut()
        .append(header::CONTENT_TYPE, "application/json".parse().unwrap());
    response(&app, duplicate, StatusCode::UNSUPPORTED_MEDIA_TYPE).await;
    for length in [4096, 4097] {
        response(
            &app,
            Request::post("/v1/sessions")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(" ".repeat(length)))
                .unwrap(),
            if length == 4096 {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::PAYLOAD_TOO_LARGE
            },
        )
        .await;
    }
    for (method, path) in [
        ("PUT", "/v1/sessions"),
        ("POST", "/v1/models"),
        ("GET", "/v1/unknown"),
        ("GET", "/v1/models/"),
        ("POST", "/v1/unknown"),
    ] {
        response(
            &app,
            Request::builder()
                .method(method)
                .uri(path)
                .body(Body::empty())
                .unwrap(),
            if path == "/v1/sessions" || path == "/v1/models" {
                StatusCode::METHOD_NOT_ALLOWED
            } else {
                StatusCode::NOT_FOUND
            },
        )
        .await;
    }
    response(
        &app,
        Request::get("/v1/auth/challenge?credential=canary")
            .body(Body::empty())
            .unwrap(),
        StatusCode::BAD_REQUEST,
    )
    .await;
    response(
        &app,
        Request::get("/v1/auth/challenge")
            .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
            .body(Body::empty())
            .unwrap(),
        StatusCode::BAD_REQUEST,
    )
    .await;
    response(
        &app,
        Request::get("/v1/models")
            .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
            .header(header::CONTENT_ENCODING, "gzip")
            .body(Body::empty())
            .unwrap(),
        StatusCode::BAD_REQUEST,
    )
    .await;
}

#[tokio::test]
async fn controls_keep_body_deadline_header_limits_without_response_frame_admission() {
    let (state, _) = fixture(10);
    let app = router_with_body_deadline(state, Duration::from_millis(20));
    let stalled = Body::from_stream(futures_util::stream::pending::<
        Result<axum::body::Bytes, std::io::Error>,
    >());
    response(
        &app,
        Request::post("/v1/sessions")
            .header(header::CONTENT_TYPE, "application/json")
            .body(stalled)
            .unwrap(),
        StatusCode::REQUEST_TIMEOUT,
    )
    .await;
    response(
        &app,
        Request::get("/v1/models")
            .header("x-wide", "x".repeat(33 * 1024))
            .body(Body::empty())
            .unwrap(),
        StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE,
    )
    .await;
    let mut builder = Request::get("/v1/models");
    for _ in 0..65 {
        builder = builder.header("x-many", "x");
    }
    response(
        &app,
        builder.body(Body::empty()).unwrap(),
        StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE,
    )
    .await;
    let res = app
        .clone()
        .oneshot(
            Request::get("/v1/auth/challenge")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let slice = bytes.slice(..1);
    drop(bytes);
    response(
        &app,
        Request::get("/v1/auth/challenge")
            .body(Body::empty())
            .unwrap(),
        StatusCode::OK,
    )
    .await;
    drop(slice);
    challenge(&app).await;
    // Retained normalized error bytes do not block another control request.
    let res = app
        .clone()
        .oneshot(Request::get("/v1/missing").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    response(
        &app,
        Request::get("/v1/auth/challenge")
            .body(Body::empty())
            .unwrap(),
        StatusCode::OK,
    )
    .await;
    drop(bytes);
    challenge(&app).await;
}

// A retired route must never reach any upstream service.
struct NoInference;
#[async_trait]
impl EvidenceVerifier for NoInference {
    async fn verify(&self, _: &str, _: u64) -> Result<GatewayEvidence, EvidenceError> {
        panic!("retired route cannot verify inference evidence")
    }
}
#[async_trait]
impl Inference for NoInference {
    async fn catalog(&self) -> Result<Vec<u8>, InferenceError> {
        panic!("retired route cannot fetch catalog")
    }
    async fn count_tokens(
        &self,
        _: &str,
        _: &[Message],
        _: Arc<possums::telemetry::hooks::Lease>,
    ) -> Result<u64, InferenceError> {
        panic!("retired route cannot tokenize")
    }
    async fn generate_completion_stream(
        &self,
        _: &possums::catalog::Model,
        _: &[Message],
        _: Arc<possums::telemetry::hooks::Lease>,
        _: &mut dyn possums::inference::TextDeltaSink,
    ) -> Result<possums::inference::stream::StreamCompletion, InferenceError> {
        panic!("retired route cannot generate")
    }
    fn verification_document(&self) -> Result<Value, InferenceError> {
        panic!("retired route cannot fetch verification document")
    }
}
#[tokio::test]
async fn retired_balance_is_closed_not_found_without_auth_or_upstream_work() {
    let credential = URL_SAFE_NO_PAD.encode([7; 32]);
    let config = json!([{"id":"demo", "credential_sha256":URL_SAFE_NO_PAD.encode(Sha256::digest(credential.as_bytes())), "demo_microunits":5_000_000}]);
    // Local synthetic auth only: no intake adapter or remote transport is installed.
    let state = AppState::new(
        Auth::from_json(&config.to_string()).unwrap(),
        Arc::new(NoInference),
        "unused",
        Arc::new(NoInference),
    );
    let app = router(state.clone());
    let bearer = login(&app, &credential).await;
    let before = state.accounting.snapshot("demo").unwrap();
    let missing = app
        .clone()
        .oneshot(Request::post("/v1/missing").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    let expected_bytes = missing.into_body().collect().await.unwrap().to_bytes();

    // The fake shipped-client request retains the old POST + bearer + {} shape.
    // Missing and invalid bearers must reach the same fallback, not authenticate.
    for authorization in [
        None,
        Some("Bearer RETIRED_AUTH_CANARY".to_owned()),
        Some(format!("Bearer {bearer}")),
    ] {
        let mut request = Request::post("/v1/balance")
            .header(header::CONTENT_TYPE, "application/json")
            .header("x-account-id", "RETIRED_ACCOUNT_CANARY")
            .body(Body::from("{}"))
            .unwrap();
        if let Some(authorization) = authorization {
            request
                .headers_mut()
                .insert(header::AUTHORIZATION, authorization.parse().unwrap());
        }
        let res = app.clone().oneshot(request).await.unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        assert_eq!(res.headers()[header::CONTENT_TYPE], "application/json");
        assert_eq!(res.headers()[header::CACHE_CONTROL], "no-store");
        assert!(!res.headers().contains_key(header::SET_COOKIE));
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(bytes, expected_bytes);
        let text = std::str::from_utf8(&bytes).unwrap();
        for secret in [
            "RETIRED_AUTH_CANARY",
            "RETIRED_ACCOUNT_CANARY",
            &bearer,
            &credential,
        ] {
            assert!(!text.contains(secret));
        }
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 1);
        assert_eq!(value["error"].as_object().unwrap().len(), 2);
        assert_eq!(value["error"]["code"], "not_found");
        assert!(value["error"]["message"]
            .as_str()
            .unwrap()
            .contains("[route_refused]"));
    }

    assert_eq!(state.accounting.snapshot("demo").unwrap(), before);
    assert!(state.auth.api_session(&bearer).is_some());
    assert_eq!(
        possums::telemetry::Endpoint::route("POST", "/v1/balance"),
        possums::telemetry::Endpoint::Other
    );
}
