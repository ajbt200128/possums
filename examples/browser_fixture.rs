use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use possums::{
    attestation::{EvidenceError, EvidenceVerifier, GatewayEvidence},
    auth::Auth,
    catalog::Model,
    inference::{Generation, Inference, InferenceError, Message},
    web::{serve, AppState},
};
use sha2::{Digest, Sha256};
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

const RELEASE: &str = "browser-fixture";
const KEY: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

struct FixtureVerifier;

impl EvidenceVerifier for FixtureVerifier {
    fn verify(&self, evidence: &GatewayEvidence, now: u64) -> Result<(), EvidenceError> {
        if evidence.release_digest == RELEASE
            && evidence.endpoint_key_sha256 == KEY
            && evidence.issued_at_unix <= now
            && now - evidence.issued_at_unix <= 300
        {
            Ok(())
        } else {
            Err(EvidenceError::Invalid)
        }
    }
}

struct FixtureInference;

#[async_trait]
impl Inference for FixtureInference {
    async fn catalog(&self) -> Result<Vec<u8>, InferenceError> {
        Ok(format!(
            r#"{{"issued_at_unix":{},"models":[{{"id":"fixture-model","context_tokens":20,"max_output_tokens":10,"input_microunits_per_token":1,"output_microunits_per_token":1}}]}}"#,
            now()
        )
        .into_bytes())
    }

    async fn count_tokens(&self, _: &str, _: &[Message]) -> Result<u64, InferenceError> {
        Ok(1)
    }

    async fn generate(&self, _: &Model, _: &[Message]) -> Result<Generation, InferenceError> {
        Ok(Generation {
            content: "<script>fetch('https://browser-canary.invalid')</script> ![pixel](https://browser-canary.invalid/pixel) **safe response**".into(),
            input_tokens: 1,
            output_tokens: 2,
        })
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
        }))
        .unwrap(),
    )
    .unwrap();
    let state = AppState::new(
        auth,
        Arc::new(FixtureInference),
        Arc::<str>::from(evidence_path.to_str().unwrap()),
        Arc::new(FixtureVerifier),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    println!("http://{}", listener.local_addr().unwrap());
    serve(listener, state).await.unwrap();
}
