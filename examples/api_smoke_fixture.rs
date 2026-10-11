//! Synthetic API-only startup probe; no provider network or production credentials.
use async_trait::async_trait;
use possums::{
    attestation::{EvidenceError, EvidenceVerifier, GatewayEvidence},
    auth::Auth,
    inference::{Inference, InferenceError, Message},
    server::{router, AppState},
};
use std::{env, sync::Arc};

struct Synthetic;
#[async_trait]
impl Inference for Synthetic {
    async fn catalog(&self) -> Result<Vec<u8>, InferenceError> {
        Ok(br#"{"object":"list","data":[]}"#.to_vec())
    }
    async fn count_tokens(
        &self,
        _: &str,
        _: &[Message],
        _: Arc<possums::telemetry::hooks::Lease>,
    ) -> Result<u64, InferenceError> {
        Err(InferenceError::Unavailable)
    }
    fn verification_document(&self) -> Result<serde_json::Value, InferenceError> {
        Err(InferenceError::Unavailable)
    }
}
#[async_trait]
impl EvidenceVerifier for Synthetic {
    async fn verify(&self, _: &str, now: u64) -> Result<GatewayEvidence, EvidenceError> {
        Ok(GatewayEvidence {
            quote: serde_json::json!({}),
            issued_at_unix: now,
            release_digest: String::new(),
            endpoint_key_sha256: String::new(),
            freshness_expires_at_unix: now + 60,
        })
    }
}
#[tokio::main]
async fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    let mode = match env::args().skip(1).collect::<Vec<_>>().as_slice() {
        [] => None,
        [mode] if mode == "--health-serving" => Some(false),
        [mode] if mode == "--health-quiescing" => Some(true),
        _ => std::process::exit(1),
    };
    let Ok(accounts) = env::var("POSSUMS_ACCOUNTS_JSON") else {
        return;
    };
    let Ok(auth) = Auth::from_json(&accounts) else {
        return;
    };
    let provider = Arc::new(Synthetic);
    let state = AppState::new(auth, provider.clone(), "synthetic-only", provider);
    if mode == Some(true) {
        state.lifecycle.quiesce();
    }
    let address = if mode.is_some() {
        "127.0.0.1:8080"
    } else {
        "127.0.0.1:0"
    };
    let Ok(listener) = tokio::net::TcpListener::bind(address).await else {
        return;
    };
    let Ok(address) = listener.local_addr() else {
        return;
    };
    println!("http://{address}");
    let _ = axum::serve(listener, router(state)).await;
}
