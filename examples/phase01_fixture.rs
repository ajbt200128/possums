//! Local synthetic API backend for the actual pinned Tinfoil shim/client spike.
//! Never a production entrypoint. No credentials, content or provider errors print.
use async_trait::async_trait;
use axum::{extract::State, routing::get, Json};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use possums::{
    attestation::{EvidenceError, EvidenceVerifier, GatewayEvidence},
    auth::Auth,
    catalog::Model,
    inference::{
        stream::{FinishReason, StreamCompletion, StreamUsage},
        Inference, InferenceError, Message,
    },
    web::{router, AppState},
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
    release: Notify,
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
    async fn generate_completion_stream(
        &self,
        _: &Model,
        _: &[Message],
        _: Arc<possums::telemetry::hooks::Lease>,
        on_delta: &mut (dyn for<'d> FnMut(&'d str) + Send),
    ) -> Result<StreamCompletion, InferenceError> {
        self.generations.fetch_add(1, Ordering::SeqCst);
        on_delta("synthetic🐾<script>");
        if tokio::time::timeout(std::time::Duration::from_secs(30), self.release.notified())
            .await
            .is_err()
        {
            return Err(InferenceError::Unavailable);
        }
        self.completed.fetch_add(1, Ordering::SeqCst);
        Ok(StreamCompletion {
            finish_reason: FinishReason::Stop,
            usage: StreamUsage {
                input_tokens: 2,
                output_tokens: 3,
                total_tokens: 5,
            },
        })
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
        json!({"tokenizations":fixture.provider.tokenizations.load(Ordering::SeqCst),"generations":fixture.provider.generations.load(Ordering::SeqCst),"completed":fixture.provider.completed.load(Ordering::SeqCst),"balance":fixture.app.accounting.available("fixture").map(|v|v.to_string())}),
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
        release: Notify::new(),
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
    let _ = axum::serve(listener, router(app).merge(controls)).await;
}
