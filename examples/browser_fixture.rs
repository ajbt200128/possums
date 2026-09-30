use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use possums::{
    attestation::{load_evidence, EvidenceError, EvidenceVerifier, GatewayEvidence},
    auth::Auth,
    catalog::Model,
    inference::{stream, Inference, InferenceError, Message},
    web::{serve, AppState},
};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
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
    scenario: Option<ProgressiveScenario>,
}

// Opt-in, process-local browser-test control; never a production HTTP endpoint.
struct ProgressiveScenario {
    calls: Mutex<usize>,
    release: Arc<tokio::sync::Semaphore>,
}

fn signal(event: &str) {
    println!("{event}");
    std::io::stdout().flush().unwrap();
}

impl ProgressiveScenario {
    fn start() -> Self {
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let reader_release = release.clone();
        std::thread::spawn(move || {
            let mut command = [0];
            if std::io::stdin().read_exact(&mut command).is_ok() && command == *b"R" {
                reader_release.add_permits(1);
            } else {
                reader_release.close();
            }
        });
        Self {
            calls: Mutex::new(0),
            release,
        }
    }

    fn accept(&self, capture: &FixtureCapture) -> Result<usize, InferenceError> {
        let mut calls = self.calls.lock().unwrap();
        let (expected_model, expected): (_, &[(&str, &str)]) = match *calls {
            0 => (FIXTURE_MODELS[0], &[("user", "first turn")]),
            1 => (
                FIXTURE_MODELS[0],
                &[
                    ("user", "first turn"),
                    ("assistant", FIXTURE_TEXT),
                    ("user", "second turn"),
                ],
            ),
            2 => (FIXTURE_MODELS[1], &[("user", "after reset")]),
            _ => return Err(InferenceError::InvalidResponse),
        };
        let (model, messages) = capture.get();
        if model != Some(expected_model)
            || !messages
                .iter()
                .map(|(role, content)| (role.as_str(), content.as_str()))
                .eq(expected.iter().copied())
        {
            return Err(InferenceError::InvalidResponse);
        }
        *calls += 1;
        Ok(*calls)
    }
}

#[async_trait]
impl Inference for FixtureInference {
    async fn catalog(&self) -> Result<Vec<u8>, InferenceError> {
        Ok(serde_json::to_vec(&serde_json::json!({"object":"list", "data": FIXTURE_MODELS.map(|id| {
            serde_json::json!({"id": id,"type":"chat","context_window":20,"endpoints":["/v1/chat/completions"],"pricing":{"inputTokenPricePer1M":1,"outputTokenPricePer1M":1,"requestPrice":0}})
        })})).unwrap())
    }

    async fn count_tokens(
        &self,
        _: &str,
        _: &[Message],
        _heavy: Arc<tokio::sync::OwnedSemaphorePermit>,
    ) -> Result<u64, InferenceError> {
        Ok(1)
    }

    async fn generate_stream(
        &self,
        model: &Model,
        messages: &[Message],
        _heavy: Arc<tokio::sync::OwnedSemaphorePermit>,
        on_delta: &mut (dyn for<'d> FnMut(&'d str) + Send),
    ) -> Result<stream::StreamUsage, InferenceError> {
        let call = {
            let mut capture = self.capture.lock().unwrap();
            capture
                .record(
                    &model.id,
                    messages
                        .iter()
                        .map(|m| (m.role.as_str(), m.content.as_str())),
                )
                .map_err(|_| InferenceError::InvalidResponse)?;
            self.scenario
                .as_ref()
                .map(|scenario| scenario.accept(&capture))
        };
        match call {
            Some(Err(error)) => {
                signal("failed"); // No history, prompts, models or URLs in diagnostics.
                return Err(error);
            }
            Some(Ok(1)) => {
                let mut parser = stream::ProtocolParser::default();
                let partial = FIXTURE_TEXT.strip_suffix(" **safe response**").unwrap();
                parser
                    .feed(
                        &fixture_support::event(fixture_support::choice(Some(partial), None)),
                        &mut *on_delta,
                    )
                    .map_err(|_| InferenceError::InvalidResponse)?;
                signal("held");
                let scenario = self.scenario.as_ref().unwrap();
                match tokio::time::timeout(Duration::from_secs(30), scenario.release.acquire())
                    .await
                {
                    Ok(Ok(permit)) => permit.forget(),
                    _ => {
                        signal("failed");
                        return Err(InferenceError::InvalidResponse);
                    }
                }
                signal("released");
                // Neither finish, synthetic authenticated usage, nor EOF is fed
                // until the browser explicitly releases the first generation.
                for bytes in [
                    fixture_support::event(fixture_support::choice(
                        Some(&FIXTURE_TEXT[partial.len()..]),
                        Some("stop"),
                    )),
                    fixture_support::event(fixture_support::usage(1, 2)),
                    b"data: [DONE]\n\n".to_vec(),
                ] {
                    parser
                        .feed(&bytes, &mut *on_delta)
                        .map_err(|_| InferenceError::InvalidResponse)?;
                }
                return parser.eof().map_err(|_| InferenceError::InvalidResponse);
            }
            Some(Ok(2)) => signal("second-accepted"),
            Some(Ok(3)) => signal("reset-accepted"),
            _ => {}
        }
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
        Arc::new(FixtureInference {
            scenario: (std::env::var("POSSUMS_BROWSER_PROGRESSIVE").as_deref() == Ok("1"))
                .then(ProgressiveScenario::start),
            ..FixtureInference::default()
        }),
        Arc::<str>::from(evidence_path.to_str().unwrap()),
        Arc::new(FixtureVerifier),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    println!("http://{}", listener.local_addr().unwrap());
    std::io::stdout().flush().unwrap();
    serve(listener, state).await.unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progressive_scenario_requires_exact_history_roles_and_locked_models() {
        let scenario = ProgressiveScenario {
            calls: Mutex::new(0),
            release: Arc::new(tokio::sync::Semaphore::new(0)),
        };
        let accept = |model: &str, messages: &[(&str, &str)]| {
            let mut capture = FixtureCapture::default();
            capture.record(model, messages.iter().copied()).unwrap();
            scenario.accept(&capture)
        };
        assert!(accept(FIXTURE_MODELS[1], &[("user", "first turn")]).is_err());
        assert!(accept(FIXTURE_MODELS[0], &[("user", "wrong")]).is_err());
        assert!(accept(FIXTURE_MODELS[0], &[("assistant", "first turn")]).is_err());
        assert_eq!(
            accept(FIXTURE_MODELS[0], &[("user", "first turn")]).ok(),
            Some(1)
        );

        let second = [
            ("user", "first turn"),
            ("assistant", FIXTURE_TEXT),
            ("user", "second turn"),
        ];
        assert!(accept(FIXTURE_MODELS[1], &second).is_err());
        assert!(accept(FIXTURE_MODELS[0], &second[1..]).is_err());
        for index in 0..second.len() {
            let mut wrong = second;
            wrong[index].0 = "system";
            assert!(accept(FIXTURE_MODELS[0], &wrong).is_err());
            wrong = second;
            wrong[index].1 = "wrong";
            assert!(accept(FIXTURE_MODELS[0], &wrong).is_err());
        }
        assert_eq!(accept(FIXTURE_MODELS[0], &second).ok(), Some(2));
        let reset = [("user", "after reset")];
        assert!(accept(FIXTURE_MODELS[0], &reset).is_err());
        let mut old_history = second.to_vec();
        old_history.push(reset[0]);
        assert!(accept(FIXTURE_MODELS[1], &old_history).is_err());
        assert_eq!(accept(FIXTURE_MODELS[1], &reset).ok(), Some(3));
        assert!(accept(FIXTURE_MODELS[1], &reset).is_err());
    }
}
