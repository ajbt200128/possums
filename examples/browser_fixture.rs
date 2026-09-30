use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use possums::{
    attestation::{load_evidence, EvidenceError, EvidenceVerifier, GatewayEvidence},
    auth::Auth,
    catalog::Model,
    inference::{stream, Generation, Inference, InferenceError, Message},
    web::{serve, AppState},
};
use sha2::{Digest, Sha256};
use std::{
    io::Write,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

#[path = "../tests/support/stream.rs"]
mod fixture_support;
use fixture_support::{FixtureCapture, FIXTURE_MODELS, FIXTURE_TEXT};

const RELEASE: &str = "browser-fixture";
const KEY: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

struct FixtureVerifier;

#[async_trait]
impl EvidenceVerifier for FixtureVerifier {
    async fn verify(
        &self,
        evidence_path: &str,
        now: u64,
    ) -> Result<GatewayEvidence, EvidenceError> {
        let evidence = load_evidence(evidence_path)?;
        if evidence.release_digest == RELEASE
            && evidence.endpoint_key_sha256 == KEY
            && evidence.issued_at_unix <= now
            && now - evidence.issued_at_unix <= 300
            && evidence.freshness_expires_at_unix > now
        {
            Ok(evidence)
        } else {
            Err(EvidenceError::Invalid)
        }
    }
}

#[derive(Default)]
struct FixtureInference {
    capture: Mutex<FixtureCapture>,
}

#[async_trait]
impl Inference for FixtureInference {
    async fn catalog(&self) -> Result<Vec<u8>, InferenceError> {
        Ok(serde_json::to_vec(&serde_json::json!({"object":"list", "data": FIXTURE_MODELS.map(|id| {
            serde_json::json!({"id": id,"type":"chat","context_window":20,"endpoints":["/v1/chat/completions"],"pricing":{"inputTokenPricePer1M":1,"outputTokenPricePer1M":1,"requestPrice":0}})
        })})).unwrap())
    }

    async fn count_tokens(&self, _: &str, _: &[Message]) -> Result<u64, InferenceError> {
        Ok(1)
    }

    async fn generate(&self, _: &Model, _: &[Message]) -> Result<Generation, InferenceError> {
        unreachable!("buffered route forbidden")
    }

    async fn generate_stream(
        &self,
        model: &Model,
        messages: &[Message],
        on_delta: &mut (dyn for<'d> FnMut(&'d str) + Send),
    ) -> Result<stream::StreamUsage, InferenceError> {
        self.capture
            .lock()
            .unwrap()
            .record(
                &model.id,
                messages
                    .iter()
                    .map(|m| (m.role.as_str(), m.content.as_str())),
            )
            .map_err(|_| InferenceError::InvalidResponse)?;
        fixture_support::terminal(FIXTURE_TEXT, 1, 2, on_delta)
            .map_err(|_| InferenceError::InvalidResponse)
    }

    fn verification_document(&self) -> Result<serde_json::Value, InferenceError> {
        Ok(serde_json::json!({"fixture": true}))
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[tokio::main]
async fn main() {
    let credential = URL_SAFE_NO_PAD.encode([7_u8; 32]);
    let hash = URL_SAFE_NO_PAD.encode(Sha256::digest(credential.as_bytes()));
    let default_accounts =
        format!(r#"[{{"id":"browser","credential_sha256":"{hash}","demo_microunits":1000}}]"#);
    let accounts = std::env::var("POSSUMS_ACCOUNTS_JSON").unwrap_or(default_accounts);
    let auth = Auth::from_json(&accounts).unwrap();
    let evidence_path =
        std::env::temp_dir().join(format!("possums-browser-evidence-{}", std::process::id()));
    std::fs::write(
        &evidence_path,
        serde_json::to_vec(&serde_json::json!({
            "quote": {"fixture": true},
            "issued_at_unix": now(),
            "release_digest": RELEASE,
            "endpoint_key_sha256": KEY,
            "freshness_expires_at_unix": now() + 300,
        }))
        .unwrap(),
    )
    .unwrap();
    let state = AppState::new(
        auth,
        Arc::new(FixtureInference::default()),
        Arc::<str>::from(evidence_path.to_str().unwrap()),
        Arc::new(FixtureVerifier),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    println!("http://{}", listener.local_addr().unwrap());
    std::io::stdout().flush().unwrap();
    serve(listener, state).await.unwrap();
}
