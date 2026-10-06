use async_trait::async_trait;
use axum::{
    body::Body,
    http::{header, Request, StatusCode},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use http_body_util::BodyExt;
use possums::{
    attestation::{EvidenceError, EvidenceVerifier, GatewayEvidence},
    auth::Auth,
    catalog::{Catalog, Model, MAX_CATALOG_BYTES, MAX_MODELS},
    inference::{stream, Inference, InferenceError, Message},
    web::{router, AppState},
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex,
};
use tower::ServiceExt;

struct Provider {
    catalog: Mutex<Vec<u8>>,
    unavailable: AtomicBool,
    catalog_calls: AtomicUsize,
    tokenizer_calls: AtomicUsize,
    generation_calls: AtomicUsize,
}
#[async_trait]
impl Inference for Provider {
    async fn catalog(&self) -> Result<Vec<u8>, InferenceError> {
        self.catalog_calls.fetch_add(1, Ordering::SeqCst);
        if self.unavailable.load(Ordering::SeqCst) {
            return Err(InferenceError::Unavailable);
        }
        Ok(self.catalog.lock().unwrap().clone())
    }
    async fn count_tokens(
        &self,
        _: &str,
        _: &[Message],
        _: Arc<tokio::sync::OwnedSemaphorePermit>,
    ) -> Result<u64, InferenceError> {
        self.tokenizer_calls.fetch_add(1, Ordering::SeqCst);
        Err(InferenceError::Unavailable)
    }
    async fn generate_stream(
        &self,
        _: &Model,
        _: &[Message],
        _: Arc<tokio::sync::OwnedSemaphorePermit>,
        _: &mut (dyn for<'d> FnMut(&'d str) + Send),
    ) -> Result<stream::StreamUsage, InferenceError> {
        self.generation_calls.fetch_add(1, Ordering::SeqCst);
        Err(InferenceError::Unavailable)
    }
    fn verification_document(&self) -> Result<Value, InferenceError> {
        Err(InferenceError::Unavailable)
    }
}
struct Evidence {
    valid: AtomicBool,
    calls: AtomicUsize,
}
#[async_trait]
impl EvidenceVerifier for Evidence {
    async fn verify(&self, _: &str, now: u64) -> Result<GatewayEvidence, EvidenceError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if !self.valid.load(Ordering::SeqCst) {
            return Err(EvidenceError::Invalid);
        }
        Ok(GatewayEvidence {
            quote: json!({}),
            issued_at_unix: now,
            release_digest: String::new(),
            endpoint_key_sha256: String::new(),
            freshness_expires_at_unix: now + 60,
        })
    }
}
fn model(id: &str, context: u64, input: Value, output: Value) -> Value {
    json!({"id":id,"type":"chat","context_window":context,"endpoints":["/v1/chat/completions"],"pricing":{"inputTokenPricePer1M":input,"outputTokenPricePer1M":output,"requestPrice":0}})
}
fn bytes(models: Vec<Value>) -> Vec<u8> {
    json!({"object":"list","data":models})
        .to_string()
        .into_bytes()
}
fn fixture(catalog: Vec<u8>) -> (AppState, Arc<Provider>, Arc<Evidence>, String) {
    let credential = URL_SAFE_NO_PAD.encode([7; 32]);
    let auth = Auth::from_json(&json!([{"id":"demo","credential_sha256":URL_SAFE_NO_PAD.encode(Sha256::digest(credential.as_bytes())),"demo_microunits":5_000_000}]).to_string()).unwrap();
    let (bearer, _) = auth
        .authenticate_api(&credential, &auth.issue_api_challenge().unwrap())
        .unwrap();
    let provider = Arc::new(Provider {
        catalog: Mutex::new(catalog),
        unavailable: AtomicBool::new(false),
        catalog_calls: AtomicUsize::new(0),
        tokenizer_calls: AtomicUsize::new(0),
        generation_calls: AtomicUsize::new(0),
    });
    let evidence = Arc::new(Evidence {
        valid: AtomicBool::new(true),
        calls: AtomicUsize::new(0),
    });
    (
        AppState::new(auth, provider.clone(), "unused", evidence.clone()),
        provider,
        evidence,
        bearer,
    )
}
async fn discover(state: &AppState, bearer: Option<&str>, expected: StatusCode) -> Value {
    let mut request = Request::get("/v1/models");
    if let Some(bearer) = bearer {
        request = request.header(header::AUTHORIZATION, format!("Bearer {bearer}"));
    }
    let response = router(state.clone())
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), expected);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    assert!(!response.headers().contains_key(header::SET_COOKIE));
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert!(bytes.len() <= MAX_CATALOG_BYTES);
    let result = serde_json::from_slice(&bytes).unwrap();
    if !expected.is_success() {
        assert!(bytes.len() < 512); // Fixed detail/message/billing, never provider text.
    }
    result
}
fn no_prompt_or_credit_work(state: &AppState, provider: &Provider) {
    assert_eq!(provider.tokenizer_calls.load(Ordering::SeqCst), 0);
    assert_eq!(provider.generation_calls.load(Ordering::SeqCst), 0);
    assert_eq!(state.accounting.available("demo"), Some(5_000_000));
}

#[tokio::test]
async fn complete_models_exact_prices_and_maximum_quotes_ignore_affordability() {
    let raw = bytes(vec![
        model("org/cheap:v1", 20, json!(0.1234567), json!(1.2345678)),
        model("kimi-k2.5", 262144, json!(15), json!(60)),
        model("precise", 1, json!("unused"), json!(1)),
    ]);
    // An exact decimal JSON number produces a microunit value above JS's safe integer.
    let mut source: Value = serde_json::from_slice(&raw).unwrap();
    source["data"][2]["pricing"]["inputTokenPricePer1M"] =
        serde_json::from_str("9007199254.740993").unwrap();
    let raw = source.to_string().into_bytes();
    let catalog = Catalog::parse_authenticated(&raw, 0).unwrap();
    let (state, provider, _, bearer) = fixture(raw);
    let list = discover(&state, Some(&bearer), StatusCode::OK).await;
    assert_eq!(list.as_object().unwrap().len(), 2);
    assert_eq!(list["object"], "list");
    assert_eq!(list["data"].as_array().unwrap().len(), 3);
    for (entry, model) in list["data"].as_array().unwrap().iter().zip(&catalog.models) {
        assert_eq!(entry.as_object().unwrap().len(), 7);
        assert_eq!(entry["object"], "model");
        assert_eq!(entry["id"], model.id);
        assert_eq!(entry["context_tokens"], model.context_tokens.to_string());
        assert_eq!(
            entry["max_output_tokens"],
            model.max_output_tokens.to_string()
        );
        assert_eq!(
            entry["input_microunits_per_million_tokens"],
            model.input_microunits_per_million_tokens.to_string()
        );
        assert_eq!(
            entry["output_microunits_per_million_tokens"],
            model.output_microunits_per_million_tokens.to_string()
        );
        assert_eq!(
            entry["maximum_reservation_microunits"],
            catalog
                .reservation_quote(&model.id)
                .unwrap()
                .reserved_microunits
                .to_string()
        );
    }
    assert_eq!(
        list["data"][0]["input_microunits_per_million_tokens"],
        "123457"
    );
    assert_eq!(
        list["data"][1]["maximum_reservation_microunits"],
        "25559040"
    );
    assert_eq!(
        list["data"][2]["input_microunits_per_million_tokens"],
        "9007199254740993"
    );
    // Discovery does not reserve; even expensive-model submission issuance is credit-free.
    let res = router(state.clone())
        .oneshot(
            Request::post("/v1/submissions")
                .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"model":"kimi-k2.5","new_conversation":false}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    drop(res);
    no_prompt_or_credit_work(&state, &provider);
}

#[tokio::test]
async fn authentication_gateway_and_catalog_fail_closed_without_stale_fallback() {
    let raw = bytes(vec![model("m", 20, json!(1), json!(1))]);
    let (state, provider, evidence, bearer) = fixture(raw.clone());
    discover(&state, None, StatusCode::UNAUTHORIZED).await;
    assert_eq!(evidence.calls.load(Ordering::SeqCst), 0);
    assert_eq!(provider.catalog_calls.load(Ordering::SeqCst), 0);
    evidence.valid.store(false, Ordering::SeqCst);
    discover(&state, Some(&bearer), StatusCode::SERVICE_UNAVAILABLE).await;
    assert_eq!(provider.catalog_calls.load(Ordering::SeqCst), 0);
    evidence.valid.store(true, Ordering::SeqCst);
    discover(&state, Some(&bearer), StatusCode::OK).await;
    provider.unavailable.store(true, Ordering::SeqCst);
    assert_eq!(
        discover(&state, Some(&bearer), StatusCode::SERVICE_UNAVAILABLE).await,
        json!({"error":{"code":"unavailable","detail":"inference_unavailable","message":"Verified inference is unavailable.","billing":"unknown"}})
    );
    provider.unavailable.store(false, Ordering::SeqCst);
    for invalid in [
        b"not-json provider-canary".to_vec(),
        bytes(vec![]),
        bytes(vec![model("duplicate", 20, json!(1), json!(1)); 2]),
        bytes(vec![model("bad", 20, json!(0), json!(1))]),
        vec![b' '; MAX_CATALOG_BYTES + 1],
        bytes(vec![model("overflow", u64::MAX, json!(1), json!(1))]),
    ] {
        *provider.catalog.lock().unwrap() = invalid;
        assert_eq!(
            discover(&state, Some(&bearer), StatusCode::SERVICE_UNAVAILABLE).await,
            json!({"error":{"code":"unavailable","detail":"catalog_failed","message":"The authenticated model catalog is unavailable or invalid.","billing":"unknown"}})
        );
    }
    *provider.catalog.lock().unwrap() = raw;
    discover(&state, Some(&bearer), StatusCode::OK).await;
    assert_eq!(provider.catalog_calls.load(Ordering::SeqCst), 9);
    no_prompt_or_credit_work(&state, &provider);
}

#[tokio::test]
async fn maximum_catalog_remains_bounded_and_never_truncated_or_filtered() {
    let models: Vec<_> = (0..MAX_MODELS)
        .map(|i| {
            model(
                &format!("{}:{i:03}", "x".repeat(124)),
                u64::MAX / 100,
                json!(0.000001),
                json!(0.000001),
            )
        })
        .collect();
    let (state, provider, _, bearer) = fixture(bytes(models.clone()));
    let list = discover(&state, Some(&bearer), StatusCode::OK).await;
    assert_eq!(list["data"].as_array().unwrap().len(), MAX_MODELS);
    assert!(
        list["data"][0]["context_tokens"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            > 9_007_199_254_740_991
    );
    let mut too_many = models;
    too_many.push(model("one-too-many", 20, json!(1), json!(1)));
    *provider.catalog.lock().unwrap() = bytes(too_many);
    discover(&state, Some(&bearer), StatusCode::SERVICE_UNAVAILABLE).await;
    no_prompt_or_credit_work(&state, &provider);
}
