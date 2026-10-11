//! Local synthetic API backend for the actual pinned Tinfoil shim/client spike.
//! Never a production entrypoint. No credentials, content or provider errors print.
use async_trait::async_trait;
use axum::{extract::State, routing::get, Json};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use futures_util::StreamExt;
use possums::{
    attestation::{EvidenceError, EvidenceVerifier, GatewayEvidence},
    auth::Auth,
    catalog::Model,
    inference::{stream::StreamCompletion, Inference, InferenceError, Message},
    server::AppState,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::sync::Notify;

struct Provider {
    tokenizations: AtomicUsize,
    generations: AtomicUsize,
    completed: AtomicUsize,
    cancelled: AtomicUsize,
    release: Arc<Notify>,
}
struct GenerationDrop<'a> {
    cancelled: &'a AtomicUsize,
    complete: bool,
}
impl Drop for GenerationDrop<'_> {
    fn drop(&mut self) {
        if !self.complete {
            self.cancelled.fetch_add(1, Ordering::SeqCst);
        }
    }
}
#[async_trait]
impl Inference for Provider {
    async fn catalog(&self) -> Result<Vec<u8>, InferenceError> {
        Ok(json!({"object":"list","data":[
            {"id":"org/model:v1","type":"chat","context_window":20,"endpoints":["/v1/chat/completions"],"pricing":{"inputTokenPricePer1M":1,"outputTokenPricePer1M":1,"requestPrice":0}},
            {"id":"kimi-k2.5","type":"chat","context_window":262144,"endpoints":["/v1/chat/completions"],"pricing":{"inputTokenPricePer1M":15,"outputTokenPricePer1M":60,"requestPrice":0}}
        ]}).to_string().into_bytes())
    }
    async fn count_tokens(
        &self,
        _: &str,
        _: &[Message],
        _: Arc<possums::telemetry::hooks::Lease>,
    ) -> Result<u64, InferenceError> {
        self.tokenizations.fetch_add(1, Ordering::SeqCst);
        Ok(2)
    }
    async fn generate_forwarded_stream(
        &self,
        model: &Model,
        _: &[Message],
        _: Option<&possums::inference::tools::ToolInvocation>,
        _: Arc<possums::telemetry::hooks::Lease>,
        sink: &mut dyn possums::inference::forward::ForwardSink,
    ) -> Result<StreamCompletion, InferenceError> {
        self.generations.fetch_add(1, Ordering::SeqCst);
        let mut guard = GenerationDrop {
            cancelled: &self.cancelled,
            complete: false,
        };
        let data = |value: Value| {
            http_body::Frame::data(axum::body::Bytes::from(format!("data: {value}\n\n")))
        };
        let first = data(
            json!({"choices":[{"index":0,"delta":{"role":"assistant","content":"synthetic🐾<script>"},"finish_reason":null}],"usage":{"prompt_tokens":2,"completion_tokens":1,"total_tokens":3}}),
        );
        let final_data = data(
            json!({"choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":2,"completion_tokens":3,"total_tokens":5}}),
        );
        let mut trailers = http::HeaderMap::new();
        trailers.insert("x-tinfoil-usage-metrics", format!("prompt=2,completion=3,total=5,model={},cost_usd=999.00,other_cost_usd=1.20,web_search_calls=2,cached_prompt_tokens=0,uncached_prompt_tokens=2,unknown=private-header-marker", model.id).parse().unwrap());
        // Synthetic upstream frames, not hardware authentication evidence. The
        // actual gateway server and downstream shim/EHBP transport are exercised.
        let release = self.release.clone();
        let frames = futures_util::stream::once(async move { Ok::<_, std::io::Error>(first) })
            .chain(futures_util::stream::once(async move {
                release.notified().await;
                Ok(final_data)
            }))
            .chain(futures_util::stream::iter([Ok(
                http_body::Frame::trailers(trailers),
            )]));
        let response = http::Response::builder()
            .header("content-type", "text/event-stream")
            .body(reqwest::Body::wrap(http_body_util::StreamBody::new(frames)))
            .unwrap()
            .into();
        let completion = possums::inference::forward::consume(
            response,
            &model.id,
            std::time::Duration::from_secs(30),
            sink,
        )
        .await?;
        guard.complete = true;
        self.completed.fetch_add(1, Ordering::SeqCst);
        Ok(completion)
    }
    fn verification_document(&self) -> Result<Value, InferenceError> {
        Err(InferenceError::Unavailable)
    }
}
#[async_trait]
impl EvidenceVerifier for Provider {
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
#[derive(Clone)]
struct Fixture {
    provider: Arc<Provider>,
    app: AppState,
}
async fn stats(State(fixture): State<Fixture>) -> Json<Value> {
    Json(
        json!({"tokenizations":fixture.provider.tokenizations.load(Ordering::SeqCst),"generations":fixture.provider.generations.load(Ordering::SeqCst),"completed":fixture.provider.completed.load(Ordering::SeqCst),"cancelled":fixture.provider.cancelled.load(Ordering::SeqCst),"in_flight":fixture.app.accounting.snapshot("fixture").ok().map(|s|s.in_flight),"completed_requests":fixture.app.accounting.snapshot("fixture").ok().map(|s|s.completed_requests),"balance":fixture.app.accounting.available("fixture").map(|v|v.to_string())}),
    )
}
async fn release(State(fixture): State<Fixture>) -> axum::http::StatusCode {
    fixture.provider.release.notify_one();
    axum::http::StatusCode::NO_CONTENT
}
#[tokio::main]
async fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    // Synthetic, fixture-only grant. It can never authenticate a production account.
    let credential = URL_SAFE_NO_PAD.encode([8; 32]);
    let config = json!([{"id":"fixture","credential_sha256":URL_SAFE_NO_PAD.encode(Sha256::digest(credential.as_bytes())),"demo_microunits":5_000_000}]);
    let Ok(auth) = Auth::from_json(&config.to_string()) else {
        return;
    };
    let provider = Arc::new(Provider {
        tokenizations: 0.into(),
        generations: 0.into(),
        completed: 0.into(),
        cancelled: 0.into(),
        release: Arc::new(Notify::new()),
    });
    let app = AppState::new(auth, provider.clone(), "fixture-only", provider.clone());
    let controls = axum::Router::new()
        .route("/fixture/stats", get(stats))
        .route("/fixture/release", get(release))
        .with_state(Fixture {
            provider,
            app: app.clone(),
        });
    let Ok(listener) = tokio::net::TcpListener::bind("127.0.0.1:18444").await else {
        return;
    };
    // Exercise the production disconnect adapter; controls are not gateway routes.
    let Ok(control_listener) = tokio::net::TcpListener::bind("127.0.0.1:18445").await else {
        return;
    };
    tokio::spawn(async move {
        let _ = axum::serve(control_listener, controls).await;
    });
    let _ = possums::server::serve_until(listener, app, std::future::pending()).await;
}
