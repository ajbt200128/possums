//! Combined route workload. Fixture transports never collect request/output bodies.
//! Capacity worksheet is in resource_streaming_tests; measurements are requested
//! allocations, not RSS or a universal bound on reqwest/SDK/TLS/helper internals.
use super::*;
use crate::{
    attestation::{EvidenceError, EvidenceVerifier, GatewayEvidence},
    auth::{session_cookie, Auth},
    catalog::{Model, MAX_CATALOG_BYTES, MAX_MODELS},
    inference::{stream, Inference, InferenceError, Message},
    process_alloc_tests::{Snapshot, ALLOCATOR},
    web::{router, AppState, BODY_LIMIT},
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
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    },
};
use tower::ServiceExt;

const MIB: usize = 1024 * 1024;
const CREDIT: u64 = 1_000_000_000;
const CONTEXT: u64 = 16_000_000;
const RESERVATION: u64 = 41_600_000;
const TIMEOUT: Duration = Duration::from_secs(120);

async fn bounded<T>(f: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(TIMEOUT, f)
        .await
        .expect("resource rendezvous timed out")
}

fn phase(name: &str, baseline: Snapshot) {
    let s = ALLOCATOR.snapshot();
    println!(
        "resource phase={name} live={} phase_peak={} lifetime_peak={} live_delta={} peak_delta={}",
        s.live,
        s.phase_peak,
        s.peak,
        s.live.saturating_sub(baseline.live),
        s.phase_peak.saturating_sub(baseline.live)
    );
    // No fixture subtraction: the incremental wire, transports and report state
    // are included in this deliberately stricter observed-delta comparison.
    assert!(
        s.phase_peak.saturating_sub(baseline.live) <= 512 * MIB,
        "fixed envelope exceeded at {name}: {s:?}, baseline={baseline:?}"
    );
    ALLOCATOR.begin_phase();
}

fn model(index: usize) -> String {
    format!("model-{index:03}-{}", "m".repeat(118))
}

fn catalog() -> Vec<u8> {
    let mut wire = String::from("{\"object\":\"list\",\"data\":[");
    for n in 0..MAX_MODELS {
        if n != 0 {
            wire.push(',');
        }
        write!(wire, "{{\"id\":\"{}\",\"type\":\"chat\",\"context_window\":{CONTEXT},\"endpoints\":[\"/v1/chat/completions\"],\"pricing\":{{\"inputTokenPricePer1M\":1,\"outputTokenPricePer1M\":1,\"requestPrice\":0}}}}", model(n)).unwrap();
    }
    wire.push_str("],\"padding\":\"");
    let padding = MAX_CATALOG_BYTES - wire.len() - 2;
    wire.extend(std::iter::repeat_n('p', padding));
    wire.push_str("\"}");
    assert_eq!(wire.len(), MAX_CATALOG_BYTES);
    wire.into_bytes()
}

struct Probe {
    hooks: Arc<ResourceHooks>,
    tokenizer: AtomicUsize,
    generations: AtomicUsize,
    document_chars: AtomicUsize,
    responses: Mutex<std::collections::VecDeque<reqwest::Response>>,
    client: reqwest::Client,
    sink: String,
    // Actual serialization lengths, never prompt-bearing copies.
    lengths: Mutex<Vec<(&'static str, usize, u64)>>,
}

impl Probe {
    async fn drain(&self, body: reqwest::Body) -> u64 {
        assert!(body.as_bytes().is_none()); // Production non-replayable body.
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
        heavy: std::sync::Arc<crate::telemetry::hooks::Lease>,
    ) -> Result<u64, InferenceError> {
        self.tokenizer.fetch_add(1, Ordering::SeqCst);
        let (body, released) = resource_fixtures::tokenizer_body(model, messages, heavy)?;
        self.hooks.at("tokenizer").await;
        let length = self.drain(body).await;
        released.await;
        // Synthetic context estimate, AFTER the real borrowed serializer and
        // bounded transport; not a fabricated terminal usage result.
        let tokens = messages.iter().map(|m| m.content.len() as u64 + 1).sum();
        self.lengths
            .lock()
            .unwrap()
            .push(("tokenizer", messages.len(), length));
        Ok(tokens)
    }

    async fn generate_stream(
        &self,
        model: &Model,
        messages: &[Message],
        _heavy: std::sync::Arc<crate::telemetry::hooks::Lease>,
        on_delta: &mut (dyn for<'d> FnMut(&'d str) + Send),
    ) -> Result<stream::StreamUsage, InferenceError> {
        self.generations.fetch_add(1, Ordering::SeqCst);
        let tokens: u64 = messages.iter().map(|m| m.content.len() as u64 + 1).sum();
        assert_eq!(model.max_output_tokens, CONTEXT - tokens);
        if model.id == self::model(1) {
            assert_eq!(messages.len(), 1);
            assert_eq!(messages[0].content, "x");
        }
        let body =
            resource_fixtures::stream_body(&model.id, model.max_output_tokens, messages, _heavy)?;
        self.hooks.at("generation").await;
        let length = self.drain(body).await;
        self.lengths
            .lock()
            .unwrap()
            .push(("generation", messages.len(), length));
        let response = self.responses.lock().unwrap().pop_front().unwrap();
        self.hooks.at("parser").await;
        resource_fixtures::consume_response(
            response,
            tokio::time::Instant::now() + TIMEOUT,
            TIMEOUT,
            on_delta,
        )
        .await
    }
    fn verification_document(&self) -> Result<serde_json::Value, InferenceError> {
        // Exactly the production 1-MiB upstream serialized-document ceiling.
        Ok(serde_json::Value::String(
            "u".repeat(self.document_chars.load(Ordering::SeqCst)),
        ))
    }
}
#[async_trait]
impl EvidenceVerifier for Probe {
    async fn verify(&self, _: &str, now: u64) -> Result<GatewayEvidence, EvidenceError> {
        // Size the complete evidence to the 1-MiB input ceiling, not just quote.
        let mut evidence = GatewayEvidence {
            quote: serde_json::json!({"padding":"", "nodes":vec![serde_json::Value::Null; 4_080]}),
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

struct Login {
    session: String,
    csrf: String,
}
struct Fixture {
    state: AppState,
    probe: Arc<Probe>,
    logins: Vec<Login>,
    sink_task: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.sink_task.abort();
    }
}
impl Fixture {
    async fn new() -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        // Bounded Hyper transport with no source copies, unbounded queues or
        // accumulated request body: one frame is borrowed and dropped at a time.
        let app = axum::Router::new().route(
            "/",
            axum::routing::post(|mut body: Body| async move {
                let mut bytes = 0_u64;
                while let Some(frame) = body.frame().await {
                    let frame = frame.unwrap();
                    if let Ok(data) = frame.into_data() {
                        bytes += data.len() as u64;
                        assert!(bytes <= 16 * MIB as u64);
                    }
                }
                bytes.to_string()
            }),
        );
        let sink_task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let credentials: Vec<_> = [31, 32]
            .into_iter()
            .map(|b| URL_SAFE_NO_PAD.encode([b; 32]))
            .collect();
        let accounts: Vec<_> = credentials.iter().enumerate().map(|(n, c)| serde_json::json!({"id":format!("account-{n}"),"credential_sha256":URL_SAFE_NO_PAD.encode(Sha256::digest(c.as_bytes())),"demo_microunits":CREDIT})).collect();
        let auth = Auth::from_json(&serde_json::to_string(&accounts).unwrap()).unwrap();
        let logins = credentials
            .iter()
            .map(|c| {
                let (session, data) = auth
                    .authenticate(c, &auth.issue_login_challenge().unwrap())
                    .unwrap();
                Login {
                    session,
                    csrf: data.csrf,
                }
            })
            .collect();
        let hooks = Arc::new(ResourceHooks::default());
        hooks.capture.store(true, Ordering::SeqCst);
        let probe = Arc::new(Probe {
            hooks: hooks.clone(),
            tokenizer: AtomicUsize::new(0),
            generations: AtomicUsize::new(0),
            document_chars: AtomicUsize::new(MIB - 2),
            responses: Mutex::default(),
            client: reqwest::Client::builder().no_proxy().build().unwrap(),
            sink: format!("http://{address}/"),
            lengths: Mutex::default(),
        });
        let mut state = AppState::new(auth, probe.clone(), "unused", probe.clone());
        state.preflight_hooks = Arc::new(PreflightHooks {
            resources: hooks,
            ..Default::default()
        });
        Self {
            state,
            probe,
            logins,
            sink_task,
        }
    }
    fn request(&self, account: usize, path: &str, wire: String) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(path)
            .header(
                header::COOKIE,
                session_cookie(&self.logins[account].session),
            )
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(wire))
            .unwrap()
    }
    fn wire(&self, account: usize, shape: usize, selected: usize) -> String {
        let login = &self.logins[account];
        let token = self.state.auth.issue_submission(&login.session).unwrap();
        let mut wire = String::with_capacity(if shape < 3 { BODY_LIMIT } else { 512 });
        write!(
            wire,
            "csrf={}&token={token}&model={}&prompt=x",
            login.csrf,
            model(selected)
        )
        .unwrap();
        let json = match shape {
            0 => {
                let mut json = String::with_capacity(6_275_009);
                json.push('[');
                for n in 0..98_047 {
                    if n != 0 {
                        json.push(',');
                    }
                    json.push_str(
                        r#"{"role":"user","content":"x"},{"role":"assistant","content":""}"#,
                    );
                }
                json.push(']');
                assert_eq!(json.len(), 6_275_009);
                json
            }
            1 => "[]".into(),
            2 => {
                let mut json = String::with_capacity(6_275_009);
                json.push_str(r#"[{"role":"user","content":""#);
                let pattern = r#"\u0000\"<&'"#;
                for _ in 0..(6_274_940 / pattern.len()) {
                    json.push_str(pattern);
                }
                json.extend(std::iter::repeat_n('\'', 6_274_940 % pattern.len()));
                json.push_str(r#""},{"role":"assistant","content":""}]"#);
                json
            }
            _ => "[]".into(),
        };
        write!(
            wire,
            "&history_manifest=1.{:06}.{:08}",
            json.len().div_ceil(crate::render::HISTORY_BLOCK_BYTES),
            json.len()
        )
        .unwrap();
        for (n, block) in json
            .as_bytes()
            .chunks(crate::render::HISTORY_BLOCK_BYTES)
            .enumerate()
        {
            write!(wire, "&h{n:06}={}", URL_SAFE_NO_PAD.encode(block)).unwrap();
        }
        drop(json); // Release construction source BEFORE route ingress.
        if shape == 1 {
            // Put padding in prompt, not an unknown field. No huge second copy.
            let suffix = wire.split_off(wire.find("&history_manifest").unwrap());
            wire.extend(std::iter::repeat_n(
                'x',
                BODY_LIMIT - wire.len() - suffix.len(),
            ));
            wire.push_str(&suffix);
        }
        if shape == 0 {
            assert_eq!(wire.len(), 8_381_765);
        }
        assert!(wire.len() <= BODY_LIMIT);
        if shape < 3 {
            assert!(wire.len() > BODY_LIMIT - 8 * 1024);
        }
        println!(
            "resource input shape={shape} bytes={} capacity={}",
            wire.len(),
            wire.capacity()
        );
        wire
    }
    async fn control(&self, account: usize, new: bool) -> axum::response::Response {
        let mut request = self.request(
            account,
            if new { "/chat/new" } else { "/" },
            format!("csrf={}", self.logins[account].csrf),
        );
        if !new {
            *request.method_mut() = axum::http::Method::GET;
        }
        router(self.state.clone()).oneshot(request).await.unwrap()
    }
    async fn slots(&self, n: usize) {
        bounded(async {
            while self.state.generation_slots.available_permits() != n {
                tokio::task::yield_now().await;
            }
        })
        .await;
    }
}

async fn text(mut body: Body) -> String {
    let mut text = String::new();
    while let Some(frame) = bounded(body.frame()).await {
        let data = frame.unwrap().into_data().unwrap();
        assert!(text.len() + data.len() <= 2 * MIB);
        text.push_str(std::str::from_utf8(&data).unwrap());
    }
    text
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
    assert_eq!(std::mem::size_of::<Message>(), 48);
    assert!(std::mem::size_of::<serde_json::Value>() <= 32);
    assert!(crate::render::max_history_decoded_bytes() <= 6 * MIB);
    assert_eq!(model(0).len(), 128);
    let f = Fixture::new().await;
    let baseline = ALLOCATOR.begin_phase();
    println!(
        "resource platform={}-{} startup={startup:?} runtime={runtime:?} shared={baseline:?}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    account_limit(&f, baseline).await;
    for mixed in [false, true] {
        round(&f, baseline, mixed).await;
    }
    phase("fixture-release-before", baseline);
    drop(f);
    // Runtime teardown and its surviving baseline are reported by the sync test.
    phase("fixture-release-after", baseline);
}

async fn account_limit(f: &Fixture, baseline: Snapshot) {
    let hooks = &f.probe.hooks;
    hooks.capture.store(false, Ordering::SeqCst);
    let mut waiters = Vec::new();
    let mut gates = Vec::new();
    let mut peers = Vec::new();
    for _ in 0..3 {
        let (response, peer) = support::raw_response().await;
        f.probe.responses.lock().unwrap().push_back(response);
        peers.push(peer);
        let mut gate = hooks.arm("tokenizer");
        waiters.push(tokio::spawn(router(f.state.clone()).oneshot(f.request(
            0,
            "/chat",
            f.wire(0, 3, 0),
        ))));
        bounded(&mut gate.reached).await.unwrap();
        gates.push(gate);
    }
    assert_eq!(f.state.generation_slots.available_permits(), 1);
    assert_eq!(f.state.chat_memory.available_permits(), 1);
    let calls = f.probe.tokenizer.load(Ordering::SeqCst);
    let response = router(f.state.clone())
        .oneshot(f.request(0, "/chat", f.wire(0, 3, 0)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    drop(response);
    assert_eq!(f.probe.tokenizer.load(Ordering::SeqCst), calls);
    assert_eq!(
        f.state.accounting.available("account-0"),
        Some(CREDIT - 3 * RESERVATION)
    );
    phase("three-account-reject-fourth-with-global-free", baseline);
    open(gates);
    for waiter in waiters {
        drop(bounded(waiter).await.unwrap().unwrap());
    }
    // Accepted work survives lost HTTP observers and failed startup delivery.
    for peer in &peers {
        peer.send(&support::event(support::choice(None, Some("stop"))), 1024)
            .await;
        peer.send(&support::event(support::usage(2, 3)), 1024).await;
        peer.send(b"data: [DONE]\n\n", 1024).await;
        peer.eof().await;
    }
    f.slots(4).await;
    bounded(async {
        while f.state.chat_memory.available_permits() != 4 {
            tokio::task::yield_now().await;
        }
    })
    .await;
    assert_eq!(f.state.accounting.available("account-0"), Some(CREDIT - 21));
    assert_eq!(f.probe.generations.load(Ordering::SeqCst), 3);
    for peer in peers {
        peer.finished().await;
    }
    f.probe.lengths.lock().unwrap().clear();
    hooks.capture.store(true, Ordering::SeqCst);
    phase("per-account-observer-loss-cleanup", baseline);
}

async fn replacement_cleanup(f: &Fixture, baseline: Snapshot) {
    let hooks = &f.probe.hooks;
    // A retained New chat selector already reset account 1 during saturation.
    // Submit its other model with EMPTY history once one heavy lane is free.
    let (response, peer) = support::raw_response().await;
    f.probe.responses.lock().unwrap().push_back(response);
    let mut input = hooks.arm("startup-input");
    let mut queued = hooks.arm("startup-queued");
    let mut parser = hooks.arm("parser");
    let calls = f.probe.generations.load(Ordering::SeqCst);
    let response = router(f.state.clone())
        .oneshot(f.request(1, "/chat", f.wire(1, 3, 1)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    bounded(&mut input.reached).await.unwrap();
    drop(response); // Failed sends cannot cancel this blocking input/accepted work.
    assert_eq!(f.state.chat_memory.available_permits(), 0);
    assert_eq!(f.state.generation_slots.available_permits(), 3);
    phase("replacement-observer-gone-blocking-input", baseline);
    input.release.send(()).unwrap();
    bounded(&mut queued.reached).await.unwrap(); // JoinHandle IS finished, unclaimed result lives.
    assert_eq!(f.state.chat_memory.available_permits(), 0);
    assert_eq!(f.probe.generations.load(Ordering::SeqCst), calls);
    phase("replacement-queued-blocking-result", baseline);
    queued.release.send(()).unwrap();
    bounded(&mut parser.reached).await.unwrap();
    parser.release.send(()).unwrap();
    peer.send(&support::event(support::choice(None, Some("stop"))), 1024)
        .await;
    peer.send(&support::event(support::usage(2, 3)), 1024).await;
    peer.send(b"data: [DONE]\n\n", 1024).await;
    assert_eq!(f.state.generation_slots.available_permits(), 3);
    peer.eof().await;
    f.slots(4).await;
    bounded(async {
        while f.state.chat_memory.available_permits() != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await;
    assert_eq!(f.probe.generations.load(Ordering::SeqCst), calls + 1);
    assert_eq!(f.state.chat_ingress.available_permits(), 3);
    hooks.raw.lock().unwrap().clear();
    assert_eq!(f.state.chat_ingress.available_permits(), 4);
    peer.finished().await;
    phase("replacement-other-model-terminal-no-replay", baseline);
}

async fn round(f: &Fixture, baseline: Snapshot, mixed: bool) {
    println!(
        "resource round={}",
        if mixed { "mixed" } else { "synchronized" }
    );
    let hooks = &f.probe.hooks;
    let balances = [
        f.state.accounting.available("account-0").unwrap(),
        f.state.accounting.available("account-1").unwrap(),
    ];
    let calls = f.probe.tokenizer.load(Ordering::SeqCst);
    let generations = f.probe.generations.load(Ordering::SeqCst);
    // Every round starts with fresh conversations, not automatic replay.
    for a in 0..2 {
        assert_eq!(f.control(a, true).await.status(), StatusCode::OK);
    }
    hooks.deliveries.lock().unwrap().clear();
    let mut peers = Vec::new();
    for _ in 0..4 {
        let (response, peer) = support::raw_response().await;
        f.probe.responses.lock().unwrap().push_back(response);
        peers.push(peer);
    }
    phase("fixture-transports-ready", baseline);
    let mut ingress = Vec::new();
    let mut decoded = Vec::new();
    let mut tokenizer = Vec::new();
    let mut precompose = Vec::new();
    let mut startup = Vec::new();
    let mut result = Vec::new();
    let mut queued = Vec::new();
    let mut generation = Vec::new();
    let mut parser = Vec::new();
    let mut waiters = Vec::new();
    for (n, shape) in [0, 0, 1, 2].into_iter().enumerate() {
        ingress.push(hooks.arm("ingress"));
        decoded.push(hooks.arm("decoded"));
        tokenizer.push(hooks.arm("tokenizer"));
        precompose.push(hooks.arm("pre-compose"));
        startup.push(hooks.arm("startup-input"));
        result.push(hooks.arm("startup-result"));
        queued.push(hooks.arm("startup-queued"));
        generation.push(hooks.arm("generation"));
        parser.push(hooks.arm("parser"));
        let request = f.request(
            usize::from(n == 3),
            "/chat",
            f.wire(usize::from(n == 3), shape, 0),
        );
        phase("fixture-wire-built-json-released", baseline);
        waiters.push(tokio::spawn(router(f.state.clone()).oneshot(request)));
        bounded(&mut ingress.last_mut().unwrap().reached)
            .await
            .unwrap();
        phase("ingress-source-released", baseline);
    }
    assert_eq!(f.state.chat_memory.available_permits(), 0);
    assert_eq!(f.state.chat_ingress.available_permits(), 0);
    let rejected = router(f.state.clone())
        .oneshot(f.request(1, "/chat", f.wire(1, 3, 0)))
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::SERVICE_UNAVAILABLE);
    drop(rejected);
    assert_eq!(f.probe.tokenizer.load(Ordering::SeqCst), calls);
    open(ingress);
    reached(&mut decoded).await;
    phase("four-decoded-with-raw-owners", baseline);
    open(decoded);
    reached(&mut tokenizer).await;
    assert_eq!(f.state.generation_slots.available_permits(), 0);
    assert_eq!(f.probe.tokenizer.load(Ordering::SeqCst), calls + 4);
    assert_eq!(
        f.state.accounting.available("account-0"),
        Some(balances[0] - 3 * RESERVATION)
    );
    assert_eq!(
        f.state.accounting.available("account-1"),
        Some(balances[1] - RESERVATION)
    );
    phase("four-tokenizer-bodies", baseline);
    // All actual raw Bytes owners remain pinned independently of heavy.
    let raw = std::mem::take(&mut *hooks.raw.lock().unwrap());
    assert_eq!(raw.len(), 4);
    let raw_slices: Vec<_> = raw.iter().map(|b| b.slice(..1)).collect();
    drop(raw);
    assert_eq!(f.state.chat_ingress.available_permits(), 0);
    drop(raw_slices);
    assert_eq!(f.state.chat_ingress.available_permits(), 4);

    // Complete selectors, not redirects; retained ordinary/control frames have
    // independent final-owner leases even when their response body is exhausted.
    let control = f.control(1, false).await;
    assert_eq!(control.status(), StatusCode::OK);
    let mut control_body = control.into_body();
    let control_frame = control_body
        .frame()
        .await
        .unwrap()
        .unwrap()
        .into_data()
        .unwrap();
    assert_eq!(f.state.control_memory.available_permits(), 0);
    let new = f.control(1, true).await;
    assert_eq!(new.status(), StatusCode::OK);
    let mut new_body = new.into_body();
    let new_frame = new_body
        .frame()
        .await
        .unwrap()
        .unwrap()
        .into_data()
        .unwrap();
    let selector = std::str::from_utf8(&new_frame).unwrap();
    assert!(
        selector.contains("<select name=model>")
            && selector.contains("name=token")
            && selector.contains("name=h000000 value=\"W10\"")
    );
    for n in 0..MAX_MODELS {
        assert!(selector.contains(&format!("value=\"{}\"", model(n))));
    }
    assert!(new_body.frame().await.is_none());
    let new_slice = new_frame.clone().slice(..1);
    drop(new_frame);
    drop(new_body);
    assert_eq!(
        f.control(1, true).await.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    drop(control_body);
    let control_slice = control_frame.clone().slice(..1);
    drop(control_frame);
    assert_eq!(
        f.control(1, false).await.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    drop(control_slice);
    let available = f.control(1, false).await;
    assert_eq!(available.status(), StatusCode::OK);
    let ordinary = text(available.into_body()).await;
    for n in 0..MAX_MODELS {
        assert!(ordinary.contains(&model(n)));
    }
    drop(ordinary);
    drop(new_slice);
    assert_eq!(f.state.new_chat_memory.available_permits(), 1);
    phase("saturated-independent-selectors", baseline);

    // Maximum bounded evidence/control response is also exercised while all
    // four tokenizer bodies are live. Combined exact 1-MiB documents exceed the
    // response ceiling by framing: fail closed, without collecting the output.
    let request = Request::builder()
        .uri("/attestation")
        .body(Body::empty())
        .unwrap();
    let evidence = router(f.state.clone()).oneshot(request).await.unwrap();
    assert_eq!(evidence.status(), StatusCode::SERVICE_UNAVAILABLE);
    drop(evidence);
    let framing = b"{\"gateway\":,\"upstream\":}".len();
    f.probe
        .document_chars
        .store(MIB - 2 - framing, Ordering::SeqCst);
    let request = Request::builder()
        .uri("/attestation")
        .body(Body::empty())
        .unwrap();
    let evidence = router(f.state.clone()).oneshot(request).await.unwrap();
    assert_eq!(evidence.status(), StatusCode::OK);
    let mut evidence_body = evidence.into_body();
    let evidence_frame = evidence_body
        .frame()
        .await
        .unwrap()
        .unwrap()
        .into_data()
        .unwrap();
    assert_eq!(evidence_frame.len(), 2 * MIB);
    assert!(evidence_body.frame().await.is_none());
    let evidence_slice = evidence_frame.slice(..1);
    drop(evidence_frame);
    drop(evidence_body);
    assert_eq!(f.state.control_memory.available_permits(), 0);
    phase("maximum-evidence-control-retained-slice", baseline);
    drop(evidence_slice);
    assert_eq!(f.state.control_memory.available_permits(), 1);
    f.probe.document_chars.store(MIB - 2, Ordering::SeqCst);

    let mut bodies = Vec::new();
    if mixed {
        // One serializer survives while three advance to pre-compose. Both
        // phases carry the same admitted input, never a copied fixture model.
        let last = tokenizer.pop().unwrap();
        open(tokenizer);
        reached(&mut precompose[..3]).await;
        precompose.remove(0).release.send(()).unwrap();
        bounded(&mut startup[0].reached).await.unwrap();
        startup.remove(0).release.send(()).unwrap();
        phase("mixed-tokenizer-precompose-blocking-renderer", baseline);
        last.release.send(()).unwrap();
        bounded(&mut precompose[2].reached).await.unwrap();
    } else {
        open(tokenizer);
        reached(&mut precompose).await;
    }
    phase("four-precompose", baseline);
    open(precompose);
    reached(&mut startup).await;
    for waiter in waiters {
        let response = bounded(waiter).await.unwrap().unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        bodies.push(response.into_body());
    }
    phase("four-blocking-startup-inputs", baseline);
    open(startup);
    bounded(async {
        loop {
            if hooks
                .deliveries
                .lock()
                .unwrap()
                .iter()
                .all(|p| p.usage().high_frames == 8)
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    phase("startup-blocked-on-eight-frames", baseline);
    // Drain startup without collecting transcript. Retain first actual frame,
    // a clone AND a one-byte slice until after worker/terminal completion.
    let mut readers = Vec::new();
    let mut stop_readers = Vec::new();
    let mut reader_gates = Vec::new();
    for mut body in bodies {
        let first = body.frame().await.unwrap().unwrap().into_data().unwrap();
        let clone = first.clone();
        let slice = first.slice(..1);
        let (stop, mut stopped) = oneshot::channel();
        let (pause, gate) = checkpoint();
        stop_readers.push(stop);
        reader_gates.push(gate);
        readers.push(tokio::spawn(async move {
            let mut bytes = first.len();
            let mut pause = Some(pause);
            loop {
                tokio::select! {
                    biased;
                    _ = &mut stopped, if pause.is_some() => {
                        pause.take().unwrap().wait().await;
                    }
                    frame = body.frame() => {
                        let Some(frame) = frame else { break; };
                        let data = frame.unwrap().into_data().unwrap();
                        bytes += data.len();
                    }
                }
            }
            (body, first, clone, slice, bytes)
        }));
    }
    reached(&mut result).await;
    phase("four-blocking-startup-results", baseline);
    open(result);
    reached(&mut queued).await;
    phase("four-unclaimed-blocking-results", baseline);
    open(queued);
    reached(&mut generation).await;
    assert_eq!(f.probe.generations.load(Ordering::SeqCst), generations + 4);
    phase("four-generation-bodies", baseline);
    for stop in stop_readers {
        stop.send(()).unwrap();
    }
    reached(&mut reader_gates).await;
    if mixed {
        let last = generation.pop().unwrap();
        open(generation);
        reached(&mut parser[..3]).await;
        for gate in parser.drain(..3) {
            gate.release.send(()).unwrap();
        }
        phase("mixed-parser-and-generation-body", baseline);
        last.release.send(()).unwrap();
    } else {
        open(generation);
    }
    reached(&mut parser).await;
    phase("four-parser-inputs", baseline);
    open(parser);
    for peer in &peers {
        peer.send(
            &support::event(support::choice(Some(&"'".repeat(60 * 1024)), None)),
            1024,
        )
        .await;
        peer.send(&support::event(support::usage(9, 9)), 1024).await;
    }
    phase("parser-delivery-finish-held", baseline);
    assert_eq!(f.state.generation_slots.available_permits(), 0);
    for peer in &peers {
        peer.send(&support::event(support::choice(None, Some("stop"))), 1024)
            .await;
        peer.send(&support::event(support::usage(2, 3)), 1024).await;
    }
    assert_eq!(f.state.generation_slots.available_permits(), 0);
    phase("terminal-done-held", baseline);
    for peer in &peers {
        peer.send(b"data: [DONE]\n\n", 1024).await;
    }
    assert_eq!(f.state.generation_slots.available_permits(), 0);
    phase("terminal-eof-held", baseline);
    for peer in &peers {
        peer.eof().await;
    }
    f.slots(4).await;
    assert_eq!(f.state.chat_memory.available_permits(), 0);
    for probe in hooks.deliveries.lock().unwrap().iter() {
        assert_eq!(probe.usage().frames, 8);
        assert!(probe.usage().high_bytes <= 64 * 1024);
    }
    phase("terminal-with-bodies-and-full-delivery-queues", baseline);
    open(reader_gates);
    let mut retained = Vec::new();
    for reader in readers {
        retained.push(bounded(reader).await.unwrap());
    }
    assert_eq!(f.state.chat_memory.available_permits(), 0);
    assert_eq!(
        f.state.accounting.available("account-0"),
        Some(balances[0] - 21)
    );
    assert_eq!(
        f.state.accounting.available("account-1"),
        Some(balances[1] - 7)
    );
    phase("terminal-workers-done-bodies-clones-slices-held", baseline);
    let rejected = router(f.state.clone())
        .oneshot(f.request(1, "/chat", f.wire(1, 3, 1)))
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::SERVICE_UNAVAILABLE);
    drop(rejected);
    for probe in hooks.deliveries.lock().unwrap().iter() {
        let usage = probe.usage();
        println!("resource delivery={usage:?}");
        assert_eq!(usage.high_frames, 8);
        assert!(usage.high_bytes <= 64 * 1024);
        assert_eq!(usage.frames, 1);
        assert!(probe.heavy_alive());
    }
    let mut slices = Vec::new();
    for (body, first, clone, slice, bytes) in retained {
        assert!(bytes > 8 * MIB);
        drop(body);
        drop(first);
        drop(clone);
        slices.push(slice);
    }
    assert_eq!(f.state.chat_memory.available_permits(), 0);
    drop(slices.pop());
    assert_eq!(f.state.chat_memory.available_permits(), 1);
    replacement_cleanup(f, baseline).await;
    assert_eq!(
        f.state.accounting.available("account-1"),
        Some(balances[1] - 14)
    );
    drop(slices);
    assert_eq!(f.state.chat_memory.available_permits(), 4);
    for probe in hooks.deliveries.lock().unwrap().iter() {
        assert_eq!(probe.usage().frames, 0);
        assert!(!probe.heavy_alive());
    }
    for peer in peers {
        peer.finished().await;
    }
    phase("round-final-owners-released", baseline);
    for capacity in std::mem::take(&mut *hooks.capacities.lock().unwrap()) {
        println!(
            "resource input phase={} messages={} vector_bytes={} strings={} prompt={}",
            capacity.phase,
            capacity.messages,
            capacity.vector_bytes,
            capacity.strings,
            capacity.prompt
        );
        assert!(capacity.vector_bytes + capacity.strings + capacity.prompt <= 32 * MIB);
    }
    for (kind, messages, bytes) in std::mem::take(&mut *f.probe.lengths.lock().unwrap()) {
        println!("resource serialization={kind} messages={messages} bytes={bytes}");
    }
}
