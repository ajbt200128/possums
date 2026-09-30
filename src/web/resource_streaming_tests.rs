//! Packet-3 combined production-route resource gate (local synthetic evidence).
//!
//! Exact identity: web::resource_streaming_tests::combined_resource_gate
//! Command (fresh test process; assert harness reports exactly ONE selected test):
//! nix develop -c cargo test --lib web::resource_streaming_tests::combined_resource_gate -- --exact --test-threads=1 --nocapture
//! The exact command must select ONE test. A zero-selected-test exit is NOT
//! evidence. Independent instrumentation/ownership review remains required.
//!
//! Workload contract:
//! - One multi-thread Tokio runtime (four workers plus blocking workers), real
//!   production router/decoder/preflight/composer/renderer/ledger. Four accepted
//!   near-8-MiB legal forms, accounts distributed 3+1 (never >3/account). Reject a
//!   fifth/global and fourth/account without tokenizer or generation calls.
//! - Include 196,094 tiny messages / 8,381,638-byte counterexample (adjust only
//!   credential/model framing length), large scalar, and escape-heavy histories.
//!   Run synchronized four-way and mixed-phase rounds, not four isolated peaks.
//! - resource_fixtures must serialize BORROWED route messages. Hold actual
//!   tokenizer/stream request Bodies, then drain them via bounded loopback HTTP
//!   transport. No retained second serialization, constant-token-only shortcut,
//!   or collecting transcripts/output. Synthetic counts can follow serialization
//!   and drain but must be consistent with context fixture admission.
//! - Hold tokenizer serialization on all four workers; separately hold all four
//!   generation serializers. Mix tokenizer/startup/queued blocking result/parser
//!   phases. The latter must execute resource_fixtures::consume_response with
//!   separately held finish, DONE and EOF. Never fabricate terminal StreamUsage.
//! - One-shot Pause/Checkpoint below at tokenization, before compose and just
//!   after compose. Optional per-AppState cfg(test) hook queues are carried
//!   into detached preflight, never installed process-globally. At pre-
//!   compose pause the original owner/input/permit live; at post-compose pause
//!   only the queued body owns delivery while generation runs independently.
//!   Observer abort/drop must not abort those tasks. Release/dropped test hooks
//!   cannot veto generation; production has NO corresponding await gap.
//! - Child module reads AppState semaphores directly. Ledger assertions use real
//!   accounting snapshots/outcomes and fixture call counts, not a copied ledger.
//!   No new ledger events are needed: balances, call counts and admissions prove
//!   the exercised terminal outcomes. No identifying/content diagnostics export.
//! - Probe actual delivery budgets before wrapping bodies. Keep bodies, dequeued
//!   Bytes clones and one-byte slices after generation ends. Generation/account
//!   slots release earlier; heavy stays unavailable until its LAST owner drops.
//!   Also hold raw ingress clones: only their own final drop returns ingress.
//!   Assert cleanup for queued startup inputs/results, failed sends and observer
//!   loss. DeliveryProbe owns no heavy permit and observes actual credit drops.
//! - Hold ordinary control body/frame under chat saturation; POST /chat/new must
//!   return the COMPLETE usable empty selector directly, with every model visible.
//!   Consume it, release a chat lane and select the other model with empty history.
//!   Separately retain New chat frames and assert its own lane is unavailable;
//!   ordinary controls work whenever THEIR lane is free. Redirect success is not
//!   selector availability. Include maximum bounded catalog/evidence shapes.
//!
//! Allocation measurements (all bytes are requested allocation, not RSS):
//! - ALLOCATOR is installed for the entire lib-test binary, including harness,
//!   Tokio workers and blocking workers; never reset live or lifetime peak.
//! - Record process-start snapshot, initialized runtime/shared-state baseline,
//!   fixture preparation peak/live, then begin_phase/snapshot at ingress/decode,
//!   tokenizer-body hold, pre-compose, startup input/queued result, stream-body
//!   hold, mixed phases, parser/delivery, terminal with held frames, final cleanup.
//!   begin_phase seeds phase peak from surviving live bytes atomically. Report
//!   absolute lifetime peak, every absolute phase peak and saturating baseline
//!   deltas. Printing/snapshot storage also count; print after the measured phase.
//! - Build/send fixtures incrementally, release source copies explicitly, record
//!   live/peak before/after each fixture boundary. Bound client/loopback buffers,
//!   fixture queues and evidence/catalog data separately. Never subtract an
//!   unexplained residual; report absolute totals alongside scoped attribution.
//!
//! Scoped capacity derivation (pinned 64-bit Rust/serde, NOT an RSS bound):
//! | Owner / maximum simultaneous lanes | MiB each | MiB total |
//! | heavy (owned input + exclusive transients + bounded work) x4 | 104 | 416 |
//! | raw ingress (collection plus one incoming frame) x4         |  16 |  64 |
//! | complete New chat lane x1                                   |  16 |  16 |
//! | ordinary controls lane x1                                   |  16 |  16 |
//! | scoped admission TOTAL                                     |     | 512 |
//!
//! Heavy = 32 owned input + 48 exclusive transient + 8 renderer/parser/delivery
//! + 8 catalog/evidence + 8 task/channel/result allowance = 104 MiB:
//! - History JSON <=6 MiB. Even before valid_history, the two required String
//!   fields require >=25 wire bytes/element including separator. <=251659
//!   elements, hence a doubling Vec capacity <=262144 * 48 = 12 MiB. Legal
//!   histories are smaller still. Strings own at most twice their JSON spans
//!   (12 MiB conservatively); prompt capacity <=8 MiB. Total <=32 MiB. Empty
//!   Strings allocate nothing. Appending the prompt can grow the message vector;
//!   any old/new overlap fits the transient allowance, not a second input copy.
//! - Decode: JSON <=6 MiB, serde escaped-string scratch <=12 MiB, old message
//!   vector during growth <=6 MiB, and even a non-reusing conversion <=12 MiB:
//!   <=36 MiB transient. All die BEFORE borrowed serialization. Serializer
//!   logical limit 16 MiB gives Vec capacity <32 MiB; conservatively charge
//!   an additional old <16-MiB allocation during growth: <=48 MiB. Tokenizer
//!   body dies before pre-compose; generation body is constructed only AFTER
//!   startup. These are mutually exclusive, witnessed by the phase barriers.
//! - Renderer owns a 4096-byte block, borrows input, and emits <=6144-byte visible
//!   / <5600-byte hidden chunks. Escape replacement old/new temporaries stay
//!   below 32 KiB. Delivery is <=65536 payload bytes and eight owner records;
//!   dequeued clones/slices retain those SAME owners, not additional payloads.
//!   Parser buffers are 64+256 KiB, transport fragment <=256 KiB, at most 8192
//!   JSON nodes/depth 16 and 16-KiB optional strings: conservative <8 MiB with
//!   JSON map/vector growth. There is never a complete collected answer.
//! - Catalog input <=256 KiB. The densest stored unvalidated field is an array
//!   of empty endpoint strings (>=3 bytes/24-byte String): doubling storage
//!   <=3 MiB plus <=1.5 MiB old capacity. Other model fields, input and scratch
//!   fit the remaining 3.5 MiB. Validated output is <=256 models/128-byte IDs.
//!   Gateway evidence is <=1 MiB and <=4096 nodes; its temporary parse/growth
//!   fits 8 MiB and ends before catalog parsing. SDK/helper internals excluded.
//! - Eight MiB task allowance is deliberately loose: bounded one-shot/channel
//!   records, Arc owners, model/token/session strings and the two blocking
//!   input/result envelopes are <1 MiB per lane; the history moves, never copies.
//!   Queued input AND unclaimed result own the original heavy lease. Generation
//!   slots release at accounting, heavy/ingress only at their final owners.
//! - Ingress uses one fixed 8-MiB Vec plus at most one 8-MiB incoming frame;
//!   no vector of fragments. New chat/ordinary selectors use <=8-MiB catalog
//!   work, then <=256 safe 128-byte IDs (<80 KiB options, conservative <2 MiB
//!   rendered intermediates); 4-KiB input and bounded chrome fit 16 MiB.
//!   Attestation controls borrow two bounded documents and serialize <=2 MiB
//!   (capacity <4 MiB plus old <2 MiB); gateway <=4096 nodes and the pinned
//!   SDK document schema are assumptions, NOT arbitrary unbounded Value inputs.
//!
//! Baseline runtime/auth/ledger/shared provider state is separate from admission.
//! Fixture construction retains ONE 8-MiB wire plus <=6-MiB source JSON, releasing
//! JSON before ingress; subsequent source wire dies in collection. Retained raw
//! clones/slices share the actual charged allocation. Four loopback peers have
//! two queued <=1024-byte fragments each here; one <=64-KiB event is constructed
//! at a time. Serializer HTTP sinks discard frames immediately and return only
//! a decimal byte count. reqwest/Hyper runtime buffers and test bookkeeping are
//! INCLUDED in absolute/live/delta observations, never silently subtracted.
//! The 512-MiB assertion includes fixture deltas and is stricter than subtracting
//! them. It cannot establish SDK/TLS/helper or allocator/RSS universal bounds.
//! Independent review must validate this derivation as well as measurements.
//! SDK/TLS/helper universal bounds, allocator overhead and whole-process RSS remain
//! unproven even if local requested allocation fits. Synthetic transport proves
//! neither live Tinfoil authentication nor invoice/billable-cost bounds.

use crate::{
    inference::{resource_fixtures, stream_support as support},
    stream_owner,
};
use http_body_util::BodyExt;
use std::{sync::Arc, time::Duration};
use tokio::sync::{oneshot, Semaphore};

// One-use test-only rendezvous. No Notify wakeup race, blocking runtime thread,
// global registry, or production instrumentation API. Dropping the controller
// releases the pause; it must never become a new cancellation authority.
pub(super) struct Pause {
    reached: oneshot::Sender<()>,
    release: oneshot::Receiver<()>,
}

pub(super) struct Checkpoint {
    pub reached: oneshot::Receiver<()>,
    pub release: oneshot::Sender<()>,
}

pub(super) fn checkpoint() -> (Pause, Checkpoint) {
    let (reached_tx, reached) = oneshot::channel();
    let (release, release_rx) = oneshot::channel();
    (
        Pause {
            reached: reached_tx,
            release: release_rx,
        },
        Checkpoint { reached, release },
    )
}

impl Pause {
    pub(super) async fn wait(self) {
        let _ = self.reached.send(());
        let _ = self.release.await;
    }
}

#[derive(Default)]
pub(super) struct PreflightHooks {
    before: std::sync::Mutex<Option<Pause>>,
    after: std::sync::Mutex<Option<Pause>>,
    pub(crate) resources: Arc<ResourceHooks>,
}

#[derive(Default)]
pub(crate) struct ResourceHooks {
    pauses: std::sync::Mutex<
        std::collections::BTreeMap<&'static str, std::collections::VecDeque<Pause>>,
    >,
    capture: std::sync::atomic::AtomicBool,
    raw: std::sync::Mutex<Vec<axum::body::Bytes>>,
    deliveries: std::sync::Mutex<Vec<stream_owner::DeliveryProbe>>,
    capacities: std::sync::Mutex<Vec<InputCapacity>>,
}

#[derive(Debug)]
struct InputCapacity {
    phase: &'static str,
    messages: usize,
    vector_bytes: usize,
    strings: usize,
    prompt: usize,
}

impl ResourceHooks {
    fn arm(&self, phase: &'static str) -> Checkpoint {
        let (pause, checkpoint) = checkpoint();
        self.pauses
            .lock()
            .unwrap()
            .entry(phase)
            .or_default()
            .push_back(pause);
        checkpoint
    }

    pub(crate) async fn at(&self, phase: &'static str) {
        let pause = self
            .pauses
            .lock()
            .unwrap()
            .get_mut(phase)
            .and_then(|q| q.pop_front());
        if let Some(pause) = pause {
            pause.wait().await;
        }
    }

    pub(crate) async fn queued<T>(&self, job: &tokio::task::JoinHandle<T>) {
        let pause = self
            .pauses
            .lock()
            .unwrap()
            .get_mut("startup-queued")
            .and_then(|q| q.pop_front());
        if let Some(pause) = pause {
            while !job.is_finished() {
                tokio::task::yield_now().await;
            }
            pause.wait().await;
        }
    }

    pub(crate) fn input(&self, phase: &'static str, form: &super::ContinuationForm) {
        if self.capture.load(std::sync::atomic::Ordering::SeqCst) {
            self.capacities.lock().unwrap().push(InputCapacity {
                phase,
                messages: form.history.len(),
                vector_bytes: form.history.capacity()
                    * std::mem::size_of::<crate::inference::Message>(),
                strings: form
                    .history
                    .iter()
                    .map(|m| m.role.capacity() + m.content.capacity())
                    .sum(),
                prompt: form.prompt.capacity(),
            });
        }
    }

    pub(crate) fn raw(&self, bytes: &axum::body::Bytes) {
        if self.capture.load(std::sync::atomic::Ordering::SeqCst) {
            self.raw.lock().unwrap().push(bytes.clone());
        }
    }

    pub(crate) fn delivery(&self, body: &stream_owner::DeliveryBody) {
        if self.capture.load(std::sync::atomic::Ordering::SeqCst) {
            self.deliveries.lock().unwrap().push(body.probe());
        }
    }
}

impl PreflightHooks {
    pub(super) async fn before_compose(&self) {
        self.resources.at("pre-compose").await;
        let pause = self.before.lock().unwrap().take();
        if let Some(pause) = pause {
            pause.wait().await;
        }
    }
    pub(super) async fn after_compose(&self) {
        let pause = self.after.lock().unwrap().take();
        if let Some(pause) = pause {
            pause.wait().await;
        }
    }
}

#[path = "resource_gate.rs"]
mod combined;

#[test]
fn combined_resource_gate() {
    let startup = crate::process_alloc_tests::ALLOCATOR.begin_phase();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .max_blocking_threads(8)
        .enable_all()
        .build()
        .unwrap();
    let initialized = crate::process_alloc_tests::ALLOCATOR.begin_phase();
    runtime.block_on(combined::run(startup, initialized));
    drop(runtime);
    println!(
        "resource runtime-released={:?}",
        crate::process_alloc_tests::ALLOCATOR.snapshot()
    );
}

// These are production-route lifecycle proofs, NOT the aggregate resource gate.
mod route {
    use super::*;
    use crate::{
        attestation::{EvidenceError, EvidenceVerifier, GatewayEvidence},
        auth::{session_cookie, Auth},
        catalog::Model,
        inference::{stream, Generation, Inference, InferenceError, Message},
        web::{router, AppState},
    };
    use async_trait::async_trait;
    use axum::{
        body::{Body, Bytes},
        http::{header, Request, StatusCode},
    };
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use sha2::{Digest, Sha256};
    use std::sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Mutex,
    };
    use tokio::sync::Notify;
    use tower::ServiceExt;

    struct Probe {
        tokenizer: Mutex<Option<Pause>>,
        tokens: u64,
        calls: AtomicUsize,
        generations: AtomicUsize,
        repriced: AtomicBool,
        overall_timeout: AtomicBool,
        response: Mutex<Option<reqwest::Response>>,
        entered: Notify,
        delta: Notify,
    }

    #[async_trait]
    impl Inference for Probe {
        async fn catalog(&self) -> Result<Vec<u8>, InferenceError> {
            let price = if self.repriced.load(Ordering::SeqCst) {
                9
            } else {
                1
            };
            Ok(serde_json::to_vec(&serde_json::json!({"object":"list", "data":[{"id":"m","type":"chat","context_window":20,"endpoints":["/v1/chat/completions"],"pricing":{"inputTokenPricePer1M":price,"outputTokenPricePer1M":price,"requestPrice":0}}]})).unwrap())
        }
        async fn count_tokens(
            &self,
            model: &str,
            messages: &[Message],
        ) -> Result<u64, InferenceError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let body = resource_fixtures::tokenizer_body(model, messages)?;
            let pause = self.tokenizer.lock().unwrap().take();
            if let Some(pause) = pause {
                pause.wait().await;
            }
            // Actual borrowed production serialization, retained across the gate.
            drop(
                body.collect()
                    .await
                    .map_err(|_| InferenceError::Unavailable)?,
            );
            Ok(self.tokens)
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
            self.generations.fetch_add(1, Ordering::SeqCst);
            assert_eq!(model.id, "m");
            assert_eq!(model.max_output_tokens, 19); // Full context-legal allowance.
            let body =
                resource_fixtures::stream_body(&model.id, model.max_output_tokens, messages)?;
            drop(
                body.collect()
                    .await
                    .map_err(|_| InferenceError::Unavailable)?,
            );
            let response = self.response.lock().unwrap().take().unwrap();
            self.entered.notify_one();
            resource_fixtures::consume_response(
                response,
                tokio::time::Instant::now() + Duration::from_secs(3),
                if self.overall_timeout.load(Ordering::SeqCst) {
                    Duration::from_secs(10)
                } else {
                    Duration::from_secs(1)
                },
                |delta| {
                    on_delta(delta);
                    self.delta.notify_one();
                },
            )
            .await
        }
        fn verification_document(&self) -> Result<serde_json::Value, InferenceError> {
            Ok(serde_json::json!({"fixture":true}))
        }
    }
    #[async_trait]
    impl EvidenceVerifier for Probe {
        async fn verify(&self, _: &str, now: u64) -> Result<GatewayEvidence, EvidenceError> {
            Ok(GatewayEvidence {
                quote: serde_json::json!({"fixture":true}),
                issued_at_unix: now,
                release_digest: "fixture".into(),
                endpoint_key_sha256: "fixture".into(),
                freshness_expires_at_unix: now + 60,
            })
        }
    }
    struct Fixture {
        state: AppState,
        probe: Arc<Probe>,
        session: String,
        csrf: String,
        token: String,
    }
    impl Fixture {
        async fn new(tokens: u64) -> (Self, support::RawPeer) {
            let credential = URL_SAFE_NO_PAD.encode([21; 32]);
            let hash = URL_SAFE_NO_PAD.encode(Sha256::digest(credential.as_bytes()));
            let auth = Auth::from_json(&format!(
                r#"[{{"id":"a","credential_sha256":"{hash}","demo_microunits":100}}]"#
            ))
            .unwrap();
            let (id, session) = auth
                .authenticate(&credential, &auth.issue_login_challenge().unwrap())
                .unwrap();
            let token = auth.issue_submission(&id).unwrap();
            let (response, peer) = support::raw_response().await;
            let probe = Arc::new(Probe {
                tokenizer: Mutex::new(None),
                tokens,
                calls: AtomicUsize::new(0),
                generations: AtomicUsize::new(0),
                repriced: AtomicBool::new(false),
                overall_timeout: AtomicBool::new(false),
                response: Mutex::new(Some(response)),
                entered: Notify::new(),
                delta: Notify::new(),
            });
            let state = AppState::new(auth, probe.clone(), "unused", probe.clone());
            (
                Self {
                    state,
                    probe,
                    session: id,
                    csrf: session.csrf,
                    token,
                },
                peer,
            )
        }
        fn request(&self) -> Request<Body> {
            Request::builder()
                .method("POST")
                .uri("/chat")
                .header(header::COOKIE, session_cookie(&self.session))
                .header(
                    header::CONTENT_TYPE,
                    "application/x-www-form-urlencoded; charset=UTF-8",
                )
                .body(Body::from(self.wire()))
                .unwrap()
        }
        fn wire(&self) -> String {
            format!("csrf={}&token={}&model=m&h000000=W10&history_manifest=1.000001.00000002&prompt=hello", self.csrf, self.token)
        }
        async fn terminal(&self, expected: u64) {
            tokio::time::timeout(Duration::from_secs(5), async {
                while self.state.generation_slots.available_permits() != 4 {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            assert_eq!(self.state.accounting.available("a"), Some(expected));
        }
        async fn duplicate(&self, expected: &str) {
            let response = router(self.state.clone())
                .oneshot(self.request())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let html = drain(response.into_body()).await;
            assert!(html.contains(expected));
            assert!(!html.contains("name=token"));
            assert!(!html.contains("hello"));
            assert_eq!(self.probe.calls.load(Ordering::SeqCst), 1);
        }
    }
    async fn drain(mut body: Body) -> String {
        let mut bytes = Vec::new();
        while let Some(frame) = body.frame().await {
            if let Ok(data) = frame.unwrap().into_data() {
                bytes.extend_from_slice(&data);
            }
        }
        String::from_utf8(bytes).unwrap()
    }
    async fn complete(peer: &support::RawPeer, input: u64, output: u64) {
        peer.send(&support::event(support::choice(None, Some("stop"))), 1024)
            .await;
        peer.send(&support::event(support::usage(input, output)), 1024)
            .await;
        peer.send(b"data: [DONE]\n\n", 1024).await;
        peer.eof().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn observer_abort_at_each_handoff_keeps_one_generation_and_settlement() {
        for phase in 0..3 {
            let (fixture, peer) = Fixture::new(1).await;
            let (pause, gate) = checkpoint();
            match phase {
                0 => *fixture.probe.tokenizer.lock().unwrap() = Some(pause),
                1 => *fixture.state.preflight_hooks.before.lock().unwrap() = Some(pause),
                _ => *fixture.state.preflight_hooks.after.lock().unwrap() = Some(pause),
            }
            let waiter = tokio::spawn(router(fixture.state.clone()).oneshot(fixture.request()));
            tokio::time::timeout(Duration::from_secs(5), gate.reached)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(fixture.state.accounting.available("a"), Some(48));
            assert_eq!(fixture.state.generation_slots.available_permits(), 3);
            assert_eq!(fixture.state.chat_memory.available_permits(), 3);
            waiter.abort();
            assert!(waiter.await.unwrap_err().is_cancelled());
            assert_eq!(fixture.state.accounting.available("a"), Some(48));
            gate.release.send(()).unwrap();
            tokio::time::timeout(Duration::from_secs(5), fixture.probe.entered.notified())
                .await
                .unwrap();
            complete(&peer, 2, 3).await;
            fixture.terminal(93).await;
            assert_eq!(fixture.probe.generations.load(Ordering::SeqCst), 1);
            fixture.duplicate("already completed").await;
            assert_eq!(fixture.state.chat_memory.available_permits(), 4);
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn failed_detached_context_preflight_refunds_without_generation_or_replay() {
        for disconnect in [false, true] {
            let (fixture, _peer) = Fixture::new(21).await;
            let (pause, gate) = checkpoint();
            *fixture.probe.tokenizer.lock().unwrap() = Some(pause);
            let waiter = tokio::spawn(router(fixture.state.clone()).oneshot(fixture.request()));
            gate.reached.await.unwrap();
            assert_eq!(fixture.state.accounting.available("a"), Some(48));
            if disconnect {
                waiter.abort();
            }
            gate.release.send(()).unwrap();
            if disconnect {
                assert!(waiter.await.unwrap_err().is_cancelled());
            } else {
                assert_eq!(
                    waiter.await.unwrap().unwrap().status(),
                    StatusCode::BAD_REQUEST
                );
            }
            fixture.terminal(100).await;
            assert_eq!(fixture.probe.generations.load(Ordering::SeqCst), 0);
            fixture.duplicate("refunded").await;
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn last_usage_only_after_finish_done_eof_and_retained_frames_pin_only_heavy() {
        for (last_input, last_output, balance) in [(2, 3, 93), (200, 300, 48)] {
            let (fixture, peer) = Fixture::new(1).await;
            let response = router(fixture.state.clone())
                .oneshot(fixture.request())
                .await
                .unwrap();
            let (frame_tx, frame_rx) = oneshot::channel();
            let has_token = Arc::new(AtomicBool::new(false));
            let seen = has_token.clone();
            let reader = tokio::spawn(async move {
                let mut body = response.into_body();
                let frame = body.frame().await.unwrap().unwrap().into_data().unwrap();
                // Keep a one-byte slice after worker completion, not the full body.
                frame_tx.send(frame.slice(..1)).unwrap();
                drop(frame);
                let mut html = String::new();
                while let Some(frame) = body.frame().await {
                    if let Ok(data) = frame.unwrap().into_data() {
                        html.push_str(std::str::from_utf8(&data).unwrap());
                        seen.store(html.contains("name=token"), Ordering::SeqCst);
                    }
                }
                html
            });
            let retained: Bytes = frame_rx.await.unwrap();
            fixture.probe.entered.notified().await;
            peer.send(&support::event(support::choice(Some("visible"), None)), 3)
                .await;
            fixture.probe.delta.notified().await;
            peer.send(&support::event(support::usage(8, 9)), 1024).await;
            peer.send(&support::event(support::choice(None, Some("stop"))), 1024)
                .await;
            peer.send(
                &support::event(support::usage(last_input, last_output)),
                1024,
            )
            .await;
            assert_eq!(fixture.state.accounting.available("a"), Some(48));
            assert!(!reader.is_finished());
            peer.send(b"data: [DONE]\n\n", 1024).await;
            fixture.probe.repriced.store(true, Ordering::SeqCst);
            assert_eq!(fixture.state.accounting.available("a"), Some(48));
            assert!(!reader.is_finished());
            assert!(!has_token.load(Ordering::SeqCst));
            peer.eof().await;
            let html = reader.await.unwrap();
            assert!(html.contains("visible"));
            assert!(html.contains("name=token"));
            fixture.terminal(balance).await;
            assert_eq!(fixture.state.chat_memory.available_permits(), 3);
            assert_eq!(fixture.state.chat_ingress.available_permits(), 4);
            assert_eq!(fixture.probe.generations.load(Ordering::SeqCst), 1);
            drop(retained);
            assert_eq!(fixture.state.chat_memory.available_permits(), 4);
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn real_socket_close_before_output_after_delta_and_while_eof_held_keeps_settlement() {
        use tokio::{
            io::{AsyncReadExt, AsyncWriteExt},
            net::{TcpListener, TcpStream},
        };
        for phase in 0..3 {
            let (fixture, peer) = Fixture::new(1).await;
            let (pause, gate) = checkpoint();
            *fixture.probe.tokenizer.lock().unwrap() = Some(pause);
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(crate::web::serve(listener, fixture.state.clone()));
            let mut socket = TcpStream::connect(address).await.unwrap();
            let wire = fixture.wire();
            socket.write_all(format!("POST /chat HTTP/1.1\r\nHost: local\r\nCookie: {}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n{}", session_cookie(&fixture.session).split(';').next().unwrap(), wire.len(), wire).as_bytes()).await.unwrap();
            gate.reached.await.unwrap();
            let mut socket = Some(socket);
            if phase == 0 {
                drop(socket.take());
            }
            assert_eq!(fixture.state.accounting.available("a"), Some(48));
            gate.release.send(()).unwrap();
            fixture.probe.entered.notified().await;
            if phase != 0 {
                peer.send(
                    &support::event(support::choice(Some("visible"), None)),
                    1024,
                )
                .await;
                fixture.probe.delta.notified().await;
                let mut visible = Vec::new();
                tokio::time::timeout(Duration::from_secs(5), async {
                    while !visible.windows(7).any(|s| s == b"visible") {
                        let mut bytes = [0; 1024];
                        let size = socket.as_mut().unwrap().read(&mut bytes).await.unwrap();
                        assert!(size > 0 && visible.len() + size < 16 * 1024);
                        visible.extend_from_slice(&bytes[..size]);
                    }
                })
                .await
                .unwrap();
                assert!(!String::from_utf8_lossy(&visible).contains("name=token"));
                assert!(String::from_utf8_lossy(&visible).contains("action=/logout"));
                if phase == 1 {
                    drop(socket.take());
                }
            }
            peer.send(&support::event(support::choice(None, Some("stop"))), 1024)
                .await;
            peer.send(&support::event(support::usage(2, 3)), 1024).await;
            peer.send(b"data: [DONE]\n\n", 1024).await;
            assert_eq!(fixture.state.accounting.available("a"), Some(48));
            drop(socket);
            peer.eof().await;
            fixture.terminal(93).await;
            fixture.duplicate("already completed").await;
            assert_eq!(fixture.probe.generations.load(Ordering::SeqCst), 1);
            server.abort();
            let _ = server.await;
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn racing_duplicate_has_one_reservation_and_restart_never_replays() {
        let (fixture, peer) = Fixture::new(1).await;
        let (pause, gate) = checkpoint();
        *fixture.probe.tokenizer.lock().unwrap() = Some(pause);
        let mut first = tokio::spawn(router(fixture.state.clone()).oneshot(fixture.request()));
        let mut second = tokio::spawn(router(fixture.state.clone()).oneshot(fixture.request()));
        gate.reached.await.unwrap();
        let (duplicate, accepted) = tokio::select! {
            response = &mut first => (response.unwrap().unwrap(), second),
            response = &mut second => (response.unwrap().unwrap(), first),
        };
        let html = drain(duplicate.into_body()).await;
        assert!(html.contains("already in progress"));
        assert!(!html.contains("name=token"));
        assert_eq!(fixture.state.accounting.available("a"), Some(48));
        gate.release.send(()).unwrap();
        drop(accepted.await.unwrap().unwrap());
        fixture.probe.entered.notified().await;
        complete(&peer, 1, 1).await;
        fixture.terminal(97).await;
        assert_eq!(fixture.probe.generations.load(Ordering::SeqCst), 1);
        let (restarted, _peer) = Fixture::new(1).await;
        let response = router(restarted.state.clone())
            .oneshot(fixture.request())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(restarted.probe.calls.load(Ordering::SeqCst), 0);
        assert_eq!(restarted.probe.generations.load(Ordering::SeqCst), 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn racing_reservations_reject_insufficient_credit_before_second_tokenization() {
        let (fixture, peer) = Fixture::new(1).await;
        let (pause, gate) = checkpoint();
        *fixture.probe.tokenizer.lock().unwrap() = Some(pause);
        let other_token = fixture
            .state
            .auth
            .issue_submission(&fixture.session)
            .unwrap();
        let mut other_request = fixture.request();
        *other_request.body_mut() =
            Body::from(fixture.wire().replace(&fixture.token, &other_token));
        let mut first = tokio::spawn(router(fixture.state.clone()).oneshot(fixture.request()));
        let mut second = tokio::spawn(router(fixture.state.clone()).oneshot(other_request));
        gate.reached.await.unwrap();
        let (rejected, accepted) = tokio::select! {
            response = &mut first => (response.unwrap().unwrap(), second),
            response = &mut second => (response.unwrap().unwrap(), first),
        };
        assert_eq!(rejected.status(), StatusCode::PAYMENT_REQUIRED);
        assert!(drain(rejected.into_body())
            .await
            .contains("insufficient demo credit"));
        assert_eq!(fixture.state.accounting.available("a"), Some(48));
        assert_eq!(fixture.probe.calls.load(Ordering::SeqCst), 1);
        assert_eq!(fixture.probe.generations.load(Ordering::SeqCst), 0);
        gate.release.send(()).unwrap();
        drop(accepted.await.unwrap().unwrap());
        fixture.probe.entered.notified().await;
        complete(&peer, 1, 1).await;
        fixture.terminal(97).await;
        assert_eq!(fixture.probe.generations.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn content_type_and_noncanonical_continuations_fail_before_reservation() {
        let (fixture, _peer) = Fixture::new(1).await;
        for content_type in [
            None,
            Some("application/json"),
            Some("application/x-www-form-urlencoded-extra"),
        ] {
            let mut request = fixture.request();
            request.headers_mut().remove(header::CONTENT_TYPE);
            if let Some(value) = content_type {
                request
                    .headers_mut()
                    .insert(header::CONTENT_TYPE, value.parse().unwrap());
            }
            let response = router(fixture.state.clone())
                .oneshot(request)
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
        }
        for replacement in [
            "history=%5B%5D",
            "h000000=W10",
            "h000000=W10&history_manifest=1.000001.00000002&h000000=W10",
        ] {
            let mut request = fixture.request();
            *request.body_mut() = Body::from(fixture.wire().replace(
                "h000000=W10&history_manifest=1.000001.00000002",
                replacement,
            ));
            let response = router(fixture.state.clone())
                .oneshot(request)
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        }
        assert_eq!(fixture.state.accounting.available("a"), Some(100));
        assert_eq!(fixture.probe.calls.load(Ordering::SeqCst), 0);
        assert_eq!(fixture.probe.generations.load(Ordering::SeqCst), 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn terminal_faults_refund_once_recover_capacity_and_never_replay() {
        for fault in 0..10 {
            let (fixture, peer) = Fixture::new(1).await;
            fixture
                .probe
                .overall_timeout
                .store(fault == 9, Ordering::SeqCst);
            let response = router(fixture.state.clone())
                .oneshot(fixture.request())
                .await
                .unwrap();
            drop(response);
            fixture.probe.entered.notified().await;
            peer.send(
                &support::event(support::choice(
                    Some("partial"),
                    if fault == 8 { None } else { Some("stop") },
                )),
                1024,
            )
            .await;
            if fault != 0 {
                let usage = if fault == 1 {
                    serde_json::json!({"choices":[],"usage":{"prompt_tokens":1,"completion_tokens":2,"total_tokens":99}})
                } else {
                    support::usage(1, 2)
                };
                peer.send(&support::event(usage), 1024).await;
            }
            match fault {
                2 => {
                    peer.send(b"data: {\"error\":{\"message\":\"hostile\"}}\n\n", 1024)
                        .await
                }
                3 => peer.send(b"data: [DONE]\n\ndata: {}\n\n", 1024).await,
                4 => peer.send(b"data: [DONE]\n\ntrailing", 1024).await,
                5 => {} // EOF without DONE
                _ => peer.send(b"data: [DONE]\n\n", 1024).await,
            }
            if fault == 6 {
                peer.fault().await;
            } else if fault != 7 && fault != 9 {
                peer.eof().await;
            } // DONE with stalled transport EOF: idle (7) or overall (9) deadline.
            fixture.terminal(100).await;
            fixture.duplicate("refunded").await;
            assert_eq!(fixture.state.chat_memory.available_permits(), 4);
            assert_eq!(fixture.probe.generations.load(Ordering::SeqCst), 1);
        }
    }
}

#[tokio::test]
async fn proof_seams_use_real_serializers_and_consumer() {
    // Small seam smoke only; packet 3 must use borrowed *route* messages and
    // bounded real transport, not this small in-memory response/collection.
    let messages = [crate::inference::Message {
        role: "user".into(),
        content: "borrowed \"<&>🐾\r\n".into(),
    }];
    for (body, streaming) in [
        (
            resource_fixtures::tokenizer_body("fixture", &messages).unwrap(),
            false,
        ),
        (
            resource_fixtures::stream_body("fixture", 99, &messages).unwrap(),
            true,
        ),
    ] {
        assert!(body.as_bytes().is_none()); // Actual non-replayable wrapper.
        let bytes = body.collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["messages"], serde_json::to_value(&messages).unwrap());
        assert_eq!(json["model"], "fixture");
        if streaming {
            assert_eq!(json["max_tokens"], 99);
            assert_eq!(json["stream_options"]["include_usage"], true);
        }
    }
    let response = http::Response::builder()
        .header("content-type", "text/event-stream")
        .body(concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"x\"},\"finish_reason\":\"stop\"}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":3,\"total_tokens\":5}}\n\n",
            "data: [DONE]\n\n"
        ))
        .unwrap();
    let mut deltas = 0;
    let usage = resource_fixtures::consume_response(
        response.into(),
        tokio::time::Instant::now() + Duration::from_secs(5),
        Duration::from_secs(1),
        |delta| {
            assert_eq!(delta, "x");
            deltas += 1;
        },
    )
    .await
    .unwrap();
    assert_eq!(deltas, 1);
    assert_eq!(usage.total_tokens, 5);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn proof_probe_does_not_pin_heavy_and_observes_sliced_frames() {
    let lane = Arc::new(Semaphore::new(1));
    let lease = lane.clone().try_acquire_owned().unwrap();
    let (tx, mut body) = stream_owner::delivery(
        lease,
        stream_owner::Limits {
            frames: 8,
            payload_bytes: 64 * 1024,
            chunk_bytes: 8 * 1024,
        },
        Duration::from_secs(1),
    );
    let probe = body.probe();
    let mut tx = tx.into_streaming();
    tx.try_send(b"retained").unwrap();
    let frame = body.frame().await.unwrap().unwrap().into_data().unwrap();
    let slice = frame.clone().slice(..1);
    drop(frame);
    drop(body);
    drop(tx);
    assert_eq!(probe.usage().bytes, 8);
    assert_eq!(probe.usage().frames, 1);
    assert!(probe.heavy_alive());
    assert_eq!(lane.available_permits(), 0);
    let (pause, checkpoint) = checkpoint();
    let worker = tokio::spawn(async move {
        pause.wait().await;
        drop(slice);
    });
    tokio::time::timeout(Duration::from_secs(5), checkpoint.reached)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(probe.usage().frames, 1);
    checkpoint.release.send(()).unwrap();
    worker.await.unwrap();
    assert_eq!(probe.usage().bytes, 0);
    assert_eq!(probe.usage().frames, 0);
    assert_eq!(probe.usage().high_bytes, 8);
    assert_eq!(probe.usage().high_frames, 1);
    assert!(!probe.heavy_alive());
    assert_eq!(lane.available_permits(), 1);
}
