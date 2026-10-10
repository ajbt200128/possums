//! Isolated API route allocation qualification. Requested bytes, not RSS or an
//! SDK/TLS/helper-process bound; no real content or credentials enter this fixture.
use super::*;
use crate::{
    attestation::{EvidenceError, EvidenceVerifier, GatewayEvidence},
    catalog::{Model, MAX_CATALOG_BYTES, MAX_MODELS},
    inference::{
        resource_fixtures, stream,
        tools::{CompletionDelta, ToolInvocation, ToolProfile},
        Inference, InferenceError, Message,
    },
    process_alloc_tests::{Snapshot, ALLOCATOR},
    server::{router, AppState, BODY_LIMIT},
};
use async_trait::async_trait;
use axum::{
    body::Body,
    http::{header, Request, StatusCode},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use sha2::{Digest, Sha256};
use std::{
    fmt::Write,
    sync::atomic::{AtomicUsize, Ordering},
};
use tower::ServiceExt;

const MIB: usize = 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(30);
const CREDIT: u64 = 1_000_000_000;
fn model(n: usize) -> String {
    format!("model-{n:03}-{}", "m".repeat(118))
}
fn catalog() -> Vec<u8> {
    let mut wire = String::from("{\"object\":\"list\",\"data\":[");
    for n in 0..MAX_MODELS {
        if n != 0 {
            wire.push(',');
        }
        write!(wire, "{{\"id\":\"{}\",\"type\":\"chat\",\"context_window\":16000000,\"endpoints\":[\"/v1/chat/completions\"],\"pricing\":{{\"inputTokenPricePer1M\":1,\"outputTokenPricePer1M\":1,\"requestPrice\":0}}}}", model(n)).unwrap();
    }
    wire.push_str("],\"padding\":\"");
    wire.extend(std::iter::repeat_n('p', MAX_CATALOG_BYTES - wire.len() - 2));
    wire.push_str("\"}");
    assert_eq!(wire.len(), MAX_CATALOG_BYTES);
    wire.into_bytes()
}
fn phase(name: &str, baseline: Snapshot) {
    let now = ALLOCATOR.snapshot();
    println!(
        "api resource phase={name} live_delta={} peak_delta={}",
        now.live.saturating_sub(baseline.live),
        now.phase_peak.saturating_sub(baseline.live)
    );
    assert!(
        now.phase_peak.saturating_sub(baseline.live) <= 512 * MIB,
        "requested-allocation envelope at {name}: {now:?} baseline={baseline:?}"
    );
    ALLOCATOR.begin_phase();
}
async fn bounded<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(TIMEOUT, future)
        .await
        .expect("resource rendezvous timed out")
}
struct Probe {
    hooks: Arc<ResourceHooks>,
    calls: AtomicUsize,
    generations: AtomicUsize,
    client: reqwest::Client,
    sink: String,
    document_chars: AtomicUsize,
}
impl Probe {
    async fn drain(&self, body: reqwest::Body) -> u64 {
        assert!(body.as_bytes().is_none());
        self.client
            .post(&self.sink)
            .body(body)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .text()
            .await
            .unwrap()
            .parse()
            .unwrap()
    }
}
#[async_trait]
impl Inference for Probe {
    async fn catalog(&self) -> Result<Vec<u8>, InferenceError> {
        Ok(catalog())
    }
    async fn count_tokens(
        &self,
        model: &str,
        messages: &[Message],
        heavy: Arc<crate::telemetry::hooks::Lease>,
    ) -> Result<u64, InferenceError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let (body, released) = resource_fixtures::tokenizer_body(model, messages, heavy)?;
        self.hooks.at("tokenizer").await;
        let bytes = self.drain(body).await;
        released.await;
        assert!(bytes <= BODY_LIMIT as u64 + MIB as u64);
        Ok(messages.iter().map(|m| m.content.len() as u64 + 1).sum())
    }
    fn tool_profile(&self, _: &str) -> Option<ToolProfile> {
        Some(ToolProfile::OpenAiFunctionsV1)
    }
    async fn count_invocation_tokens(
        &self,
        _: &str,
        invocation: &ToolInvocation,
        _: Arc<crate::telemetry::hooks::Lease>,
    ) -> Result<u64, InferenceError> {
        assert_eq!(invocation.messages().len(), 81);
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.hooks.at("tokenizer").await;
        Ok(2)
    }
    async fn generate_invocation_stream(
        &self,
        _: &Model,
        invocation: &ToolInvocation,
        _: Arc<crate::telemetry::hooks::Lease>,
        on_delta: &mut (dyn for<'d> FnMut(CompletionDelta<'d>) + Send),
    ) -> Result<stream::StreamCompletion, InferenceError> {
        assert_eq!(invocation.messages().len(), 81);
        self.generations.fetch_add(1, Ordering::SeqCst);
        self.hooks.at("generation").await;
        on_delta(CompletionDelta::Text("synthetic"));
        self.hooks.at("completion").await;
        Ok(stream::StreamCompletion {
            usage: stream::StreamUsage {
                input_tokens: 2,
                output_tokens: 3,
                total_tokens: 5,
            },
            finish_reason: stream::FinishReason::Stop,
        })
    }
    async fn generate_completion_stream(
        &self,
        model: &Model,
        messages: &[Message],
        heavy: Arc<crate::telemetry::hooks::Lease>,
        on_delta: &mut (dyn for<'d> FnMut(&'d str) + Send),
    ) -> Result<stream::StreamCompletion, InferenceError> {
        self.generations.fetch_add(1, Ordering::SeqCst);
        let body =
            resource_fixtures::stream_body(&model.id, model.max_output_tokens, messages, heavy)?;
        self.hooks.at("generation").await;
        let bytes = self.drain(body).await;
        assert!(bytes <= BODY_LIMIT as u64 + MIB as u64);
        on_delta("synthetic");
        self.hooks.at("completion").await;
        Ok(stream::StreamCompletion {
            usage: stream::StreamUsage {
                input_tokens: 2,
                output_tokens: 3,
                total_tokens: 5,
            },
            finish_reason: stream::FinishReason::Stop,
        })
    }
    fn verification_document(&self) -> Result<serde_json::Value, InferenceError> {
        Ok(serde_json::Value::String(
            "u".repeat(self.document_chars.load(Ordering::SeqCst)),
        ))
    }
}
#[async_trait]
impl EvidenceVerifier for Probe {
    async fn verify(&self, _: &str, now: u64) -> Result<GatewayEvidence, EvidenceError> {
        let mut evidence = GatewayEvidence {
            quote: serde_json::json!({"padding":""}),
            issued_at_unix: now,
            release_digest: "fixture".into(),
            endpoint_key_sha256: "fixture".into(),
            freshness_expires_at_unix: now + 60,
        };
        let overhead = serde_json::to_vec(&evidence).unwrap().len();
        evidence.quote["padding"] = serde_json::Value::String("e".repeat(MIB - overhead));
        Ok(evidence)
    }
}
struct Fixture {
    state: AppState,
    probe: Arc<Probe>,
    sessions: Vec<String>,
    sink: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.sink.abort();
    }
}
impl Fixture {
    async fn new() -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = axum::Router::new().route(
            "/",
            axum::routing::post(|mut body: Body| async move {
                let mut bytes = 0_u64;
                while let Some(frame) = body.frame().await {
                    if let Ok(data) = frame.unwrap().into_data() {
                        bytes += data.len() as u64;
                        assert!(bytes <= 16 * MIB as u64);
                    }
                }
                bytes.to_string()
            }),
        );
        let sink = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let credentials: Vec<_> = [31, 32].map(|b| URL_SAFE_NO_PAD.encode([b; 32])).into();
        let accounts: Vec<_> = credentials.iter().enumerate().map(|(n,c)| serde_json::json!({"id":format!("account-{n}"),"credential_sha256":URL_SAFE_NO_PAD.encode(Sha256::digest(c.as_bytes())),"demo_microunits":CREDIT})).collect();
        let auth =
            crate::auth::Auth::from_json(&serde_json::to_string(&accounts).unwrap()).unwrap();
        let sessions = credentials
            .iter()
            .map(|c| {
                auth.authenticate_api(c, &auth.issue_api_challenge().unwrap())
                    .unwrap()
                    .0
            })
            .collect();
        let hooks = Arc::new(ResourceHooks::default());
        hooks.capture.store(true, Ordering::SeqCst);
        let probe = Arc::new(Probe {
            hooks: hooks.clone(),
            calls: AtomicUsize::new(0),
            generations: AtomicUsize::new(0),
            client: reqwest::Client::builder().no_proxy().build().unwrap(),
            sink: format!("http://{address}/"),
            document_chars: AtomicUsize::new(MIB - 2),
        });
        let mut state = AppState::new(auth, probe.clone(), "unused", probe.clone());
        state.preflight_hooks = Arc::new(PreflightHooks {
            resources: hooks,
            ..Default::default()
        });
        Self {
            state,
            probe,
            sessions,
            sink,
        }
    }
    fn request(&self, account: usize, shape: usize) -> Request<Body> {
        let token = self
            .state
            .auth
            .issue_api_submission(&self.sessions[account], &model(0), false)
            .unwrap();
        let mut wire = format!(
            "{{\"model\":\"{}\",\"stream\":true,\"submission\":\"{token}\",\"messages\":[",
            model(0)
        );
        match shape {
            0 => {
                // Dense 2,000-message history with long strings, near API body limit.
                for n in 0..2_000 {
                    if n != 0 {
                        wire.push(',');
                    }
                    wire.push_str("{\"role\":\"system\",\"content\":\"");
                    wire.extend(std::iter::repeat_n('x', 3_900));
                    wire.push_str("\"}");
                }
                wire.push(',');
            }
            1 => {
                wire.push_str("{\"role\":\"system\",\"content\":\"");
                wire.extend(std::iter::repeat_n('x', BODY_LIMIT - wire.len() - 256));
                wire.push_str("\"},");
            }
            2 => {
                wire.push_str("{\"role\":\"system\",\"content\":\"");
                for _ in 0..650_000 {
                    wire.push_str("\\u0000");
                }
                wire.push_str("\"},");
            }
            3 => {
                // Historical assistant/tool pairs are admitted as opaque transcript,
                // not the old browser manifest or a new rendered conversation.
                for n in 0..40 {
                    write!(wire, "{{\"role\":\"assistant\",\"content\":null,\"tool_calls\":[{{\"id\":\"prior_{n}\",\"type\":\"function\",\"function\":{{\"name\":\"lookup\",\"arguments\":\"{{}}\"}}}}]}},{{\"role\":\"tool\",\"tool_call_id\":\"prior_{n}\",\"content\":\"").unwrap();
                    wire.extend(std::iter::repeat_n('t', 60_000));
                    wire.push_str("\"},");
                }
            }
            _ => unreachable!(),
        }
        wire.push_str("{\"role\":\"user\",\"content\":\"x\"}]}");
        assert!(wire.len() <= BODY_LIMIT);
        // Fragment the dense JSON case through the actual ingress collector.
        let body = if shape == 0 {
            let fragments = futures_util::stream::unfold(
                axum::body::Bytes::from(wire),
                |mut remaining| async move {
                    if remaining.is_empty() {
                        None
                    } else {
                        let chunk = remaining.split_to(remaining.len().min(1024));
                        Some((Ok::<_, std::io::Error>(chunk), remaining))
                    }
                },
            );
            Body::from_stream(fragments)
        } else {
            Body::from(wire)
        };
        Request::post("/v1/chat/completions")
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", self.sessions[account]),
            )
            .header(header::CONTENT_TYPE, "application/json")
            .body(body)
            .unwrap()
    }
    async fn control(&self, path: &str) -> axum::response::Response {
        let req = Request::builder()
            .uri(path)
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", self.sessions[1]),
            )
            .body(Body::empty())
            .unwrap();
        router(self.state.clone()).oneshot(req).await.unwrap()
    }
}
async fn reached(gates: &mut [Checkpoint]) {
    for gate in gates {
        bounded(&mut gate.reached).await.unwrap();
    }
}
fn open(gates: Vec<Checkpoint>) {
    for gate in gates {
        gate.release.send(()).unwrap();
    }
}

pub(super) async fn run(startup: Snapshot, runtime: Snapshot) {
    let f = Fixture::new().await;
    let baseline = ALLOCATOR.begin_phase();
    println!(
        "api resource platform={}-{} startup={startup:?} runtime={runtime:?} baseline={baseline:?}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let hooks = &f.probe.hooks;
    let mut ingress = Vec::new();
    let mut tokenizer = Vec::new();
    let mut generation = Vec::new();
    let mut parser = Vec::new();
    let mut waiters = Vec::new();
    for (n, shape) in [0, 1, 2, 3].into_iter().enumerate() {
        ingress.push(hooks.arm("ingress"));
        tokenizer.push(hooks.arm("tokenizer"));
        generation.push(hooks.arm("generation"));
        parser.push(hooks.arm("completion"));
        let req = f.request(usize::from(n == 3), shape);
        phase("wire-built", baseline);
        waiters.push(tokio::spawn(router(f.state.clone()).oneshot(req)));
        bounded(&mut ingress.last_mut().unwrap().reached)
            .await
            .unwrap();
    }
    assert_eq!(f.state.available_lanes()[1..3], [0, 0]);
    let rejected = router(f.state.clone())
        .oneshot(f.request(1, 3))
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(f.probe.calls.load(Ordering::SeqCst), 0);
    phase("four-ingress-reject-fifth", baseline);
    open(ingress);
    reached(&mut tokenizer).await;
    assert_eq!(f.state.available_lanes()[1], 0);
    assert_eq!(f.probe.calls.load(Ordering::SeqCst), 4);
    let raw = std::mem::take(&mut *hooks.raw.lock().unwrap());
    assert_eq!(raw.len(), 4);
    let slices: Vec<_> = raw.iter().map(|b| b.slice(..1)).collect();
    drop(raw);
    assert_eq!(f.state.available_lanes()[2], 0);
    drop(slices);
    assert_eq!(f.state.available_lanes()[2], 4);
    phase("four-tokenizers-with-raw-owners", baseline);
    let control = f.control("/v1/models").await;
    assert_eq!(control.status(), StatusCode::OK);
    assert_eq!(f.state.available_lanes()[3], 0);
    assert_eq!(
        f.control("/v1/models").await.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    let mut body = control.into_body();
    let frame = bounded(body.frame())
        .await
        .unwrap()
        .unwrap()
        .into_data()
        .unwrap();
    let retained = frame.slice(..1);
    drop(frame);
    drop(body);
    assert_eq!(f.state.available_lanes()[3], 0);
    drop(retained);
    assert_eq!(f.state.available_lanes()[3], 1);
    phase("independent-model-control-final-owner", baseline);
    let balance = router(f.state.clone())
        .oneshot(
            Request::post("/v1/balance")
                .header(header::AUTHORIZATION, format!("Bearer {}", f.sessions[0]))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(balance.status(), StatusCode::OK);
    let bytes = bounded(http_body_util::BodyExt::collect(balance.into_body()))
        .await
        .unwrap()
        .to_bytes();
    let snapshot: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    drop(bytes);
    assert_eq!(snapshot["in_flight"], 3);
    assert!(snapshot.as_object().unwrap().keys().all(|key| [
        "available_microunits",
        "in_flight",
        "completed_requests"
    ]
    .contains(&key.as_str())));
    phase("prompt-free-balance-during-saturation", baseline);
    let evidence = f.control("/attestation").await;
    assert_eq!(evidence.status(), StatusCode::SERVICE_UNAVAILABLE);
    drop(evidence);
    f.probe.document_chars.store(
        MIB - 2 - b"{\"gateway\":,\"upstream\":}".len(),
        Ordering::SeqCst,
    );
    let evidence = f.control("/attestation").await;
    assert_eq!(evidence.status(), StatusCode::OK);
    let mut body = evidence.into_body();
    let frame = bounded(body.frame())
        .await
        .unwrap()
        .unwrap()
        .into_data()
        .unwrap();
    assert_eq!(frame.len(), 2 * MIB);
    let retained = frame.slice(..1);
    drop(frame);
    drop(body);
    assert_eq!(f.state.available_lanes()[3], 0);
    phase("maximum-evidence-control", baseline);
    drop(retained);
    open(tokenizer);
    let mut bodies = Vec::new();
    for waiter in waiters {
        let response = bounded(waiter).await.unwrap().unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        bodies.push(response.into_body());
    }
    reached(&mut generation).await;
    phase("four-generation-serializers", baseline);
    open(generation);
    reached(&mut parser).await;
    phase("four-completion-inputs", baseline);
    open(parser);
    let mut retained = Vec::new();
    for mut body in bodies {
        let frame = bounded(body.frame())
            .await
            .unwrap()
            .unwrap()
            .into_data()
            .unwrap();
        retained.push(frame.slice(..1));
        drop(frame);
        drop(body);
    }
    bounded(async {
        while f.state.generation_headroom() != 4 {
            tokio::task::yield_now().await;
        }
    })
    .await;
    assert_eq!(f.probe.generations.load(Ordering::SeqCst), 4);
    assert_eq!(f.state.available_lanes()[1], 0);
    phase("terminal-with-held-sse-slices", baseline);
    drop(retained);
    bounded(async {
        while f.state.available_lanes()[1] != 4 {
            tokio::task::yield_now().await;
        }
    })
    .await;
    assert_eq!(
        f.state.accounting.available("account-0"),
        Some(CREDIT - 3 * 7)
    );
    assert_eq!(f.state.accounting.available("account-1"), Some(CREDIT - 7));
    phase("all-final-owners-released", baseline);
    for capacity in std::mem::take(&mut *hooks.capacities.lock().unwrap()) {
        println!(
            "api input phase={} messages={} vector_bytes={} strings={} prompt={}",
            capacity.phase,
            capacity.messages,
            capacity.vector_bytes,
            capacity.strings,
            capacity.prompt
        );
        assert!(capacity.vector_bytes + capacity.strings + capacity.prompt <= 32 * MIB);
    }
    drop(f);
    phase("fixture-released", baseline);
}
