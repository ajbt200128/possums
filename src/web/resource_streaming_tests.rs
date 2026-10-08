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
//! REJECTED capacity worksheet (independent audit of HEAD 7da10c8).
//! The following is the unchanged admission TARGET, not an established bound.
//! See docs/verification.md, "Packet 3 analytical capacity refutation", for the
//! source-linked phase/ownership audit and the precise decision required.
//! Scoped capacity target (pinned 64-bit Rust/serde, NOT an RSS bound):
//! | Owner / maximum simultaneous lanes | MiB each | MiB total |
//! | heavy (owned input + exclusive transients + bounded work) x4 | 104 | 416 |
//! | raw ingress (collection plus one incoming frame) x4         |  16 |  64 |
//! | complete New chat lane x1                                   |  16 |  16 |
//! | ordinary controls lane x1                                   |  16 |  16 |
//! | scoped admission TOTAL                                     |     | 512 |
//!
//! Proposed heavy split was 32 owned input + 48 exclusive transient + 8 renderer/
//! parser/delivery + 8 catalog/evidence + 8 task/channel/result = 104 MiB.
//! It is NOT accepted by this audit:
//! - serde_json 1.0.151 deserialize_struct accepts sequences, even with
//!   deny_unknown_fields. The old decoder collected eight-byte ["",""] entries
//!   before validation, growing a 48-byte-element vector to 1,048,576 slots
//!   (48 MiB), with an old 24-MiB vector potentially overlapping growth.
//!   web::decode_continuation now validates each entry BEFORE pushing directly
//!   into one Message vector. A valid sequence-form pair needs at least 30 JSON
//!   bytes (including its entry separators); decoded history is <=6 MiB, so
//!   <=419,430 records can survive. On pinned 64-bit Rust a 48-byte Message Vec
//!   grows to at most 524,288 slots (24 MiB), temporarily overlapping its old
//!   262,144-slot allocation (12 MiB). This 36-MiB RECORD ceiling excludes
//!   parsed strings, JSON, the prompt, and other phase allocations; it does
//!   not establish the 104-MiB heavy envelope. No product cap was added.
//! - BoundedWriter checks length BEFORE extending. Rust 1.88 RawVec grows to
//!   max(2*capacity, required, minimum), not to logical length. A 16-MiB writer
//!   therefore needs <32 MiB new plus <16 MiB old, including partial/failing
//!   serialization. Borrowed tokenizer/generation serializers do not copy input.
//! - Decoder scratch dies before tokenization; startup precedes generation
//!   serialization. Four chats may independently occupy any of these phases.
//!   Heavy ownership survives detached/queued work and retained delivery frames;
//!   generation slots do not authorize replacement of a surviving heavy owner.
//! - The decisive blocker is the ordinary /attestation lane: production obtains
//!   an OWNED SDK verification document BEFORE bounded serialization. Normal SDK
//!   provenance constructs three code registers, but does not cap their strings.
//!   For SNP, measurement comparison checks register[0], not TDX register[2].
//!   A 16-MiB+1 register[2] survives the SDK-style clone, then the 1-MiB writer
//!   rejects it. That ONE application-owned string exceeds the 16-MiB lane.
//!   sdk_evidence_clone_bound_counterexample reproduces the pinned schema,
//!   comparison, clone and failing writer; it is not a live attestation test.
//!   Excluding SDK internals/shared state cannot exclude this returned document.
//! - A check after verification_document() or before parsing Value is too late
//!   for that clone. A bounded export/pre-publication SDK contract is required;
//!   no speculative SDK/API redesign or production correction lands here.
//! - Renderer/parser, catalog/gateway evidence, controls and finite task records
//!   are inventoried in the evidence table, NOT certified by blanket slack. The
//!   passing workload remains corroboration of its exercised shapes only.
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
//! Independent audit REFUTED the derivation; passing measurements do not close it.
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

#[test]
fn accepted_sequence_history_record_growth_ceiling() {
    // A valid user/assistant pair cannot be shorter: sequence-form struct
    // entries are denser than object-form entries, and user content is nonempty.
    assert_eq!(r#"[["user","x"],["assistant",""]]"#.len(), 31);
    assert_eq!(std::mem::size_of::<crate::inference::Message>(), 48);
    let decoded = crate::render::max_history_decoded_bytes();
    assert!(decoded <= 6 * 1024 * 1024);
    let max_records = 2 * ((decoded - 1) / 30);
    assert!(max_records <= 524_288);
}

/// Public-schema counterexample: the SDK's legacy unbounded clone still fails.
/// The gateway now uses the pre-clone export tested below, not this legacy path.
/// This is NOT fabricated live SDK authentication.
#[test]
fn sdk_evidence_clone_bound_counterexample() {
    use tinfoil::verifier::{Measurement, PredicateType};
    const CONTROL_BYTES: usize = 16 * 1024 * 1024;
    let enclave = Measurement {
        type_: PredicateType::SevGuestV2,
        registers: vec!["0".repeat(96)],
    };
    // The normal Sigstore extractor constructs exactly these THREE registers.
    // The SNP branch checks only the first; authenticated does not mean bounded.
    let code = Measurement {
        type_: PredicateType::SnpTdxMultiPlatformV1,
        registers: vec![
            enclave.registers[0].clone(),
            "0".repeat(96),
            "x".repeat(CONTROL_BYTES + 1),
        ],
    };
    code.equals(&enclave).unwrap();
    let mut document: tinfoil::VerificationDocument = serde_json::from_value(serde_json::json!({
        "schemaVersion": 1,
        "configRepo": "fixture/repo", "enclaveHost": "fixture.invalid",
        "releaseDigest": "0".repeat(64),
        "codeMeasurement": {
            "type": "https://tinfoil.sh/predicate/snp-tdx-multiplatform/v1",
            "registers": []
        },
        "enclaveMeasurement": {
            "measurement": enclave,
            "tlsPublicKeyFingerprint": "0".repeat(64),
            "hpkePublicKey": "0".repeat(64)
        },
        "tlsPublicKey": "0".repeat(64), "hpkePublicKey": "0".repeat(64),
        "codeFingerprint": "0".repeat(64), "enclaveFingerprint": "0".repeat(64),
        "selectedRouterEndpoint": "fixture.invalid", "securityVerified": true,
        "verifier": {"name": "tinfoil", "version": "fixture"},
        "verifiedAt": "2026-09-30T00:00:00Z",
        "steps": {
            "fetchDigest": {"status": "success"}, "verifyCode": {"status": "success"},
            "verifyEnclave": {"status": "success"},
            "compareMeasurements": {"status": "success"},
            "verifyCertificate": {"status": "success"}
        }
    }))
    .unwrap();
    document.code_measurement = code;
    // SecureClient::ground_truth clones owned strings, then from_ground_truth
    // moves code_measurement into the application-returned VerificationDocument.
    // Cloning this public document reproduces the same load-bearing String clone.
    let owned = document.clone();
    drop(document); // Do not count the shared-state/fixture source copy.
    let retained = owned.code_measurement.registers[2].capacity();
    assert!(crate::bounded_json::to_vec(&owned, 1024 * 1024).is_err());
    assert!(retained > CONTROL_BYTES);
    println!(
        "capacity blocker: cloned_rtmr2_bytes={retained} control_lane_bytes={CONTROL_BYTES} registers=3 serializer_rejected=true"
    );
}

fn synthetic_ground_truth() -> tinfoil::GroundTruth {
    use tinfoil::{GroundTruth, Measurement, PredicateType, SoftwareIdentity};
    let measurement = Measurement {
        type_: PredicateType::SevGuestV2,
        registers: vec!["0".repeat(96)],
    };
    GroundTruth {
        config_repo: "fixture/repo".into(),
        release_tag: Some("v1".into()),
        digest: "0".repeat(64),
        tls_public_key: Some("0".repeat(64)),
        hpke_public_key: Some("1".repeat(64)),
        code_measurement: measurement.clone(),
        enclave_measurement: measurement,
        code_fingerprint: "0".repeat(64),
        enclave_fingerprint: "0".repeat(64),
        verifier: SoftwareIdentity {
            name: "fixture".into(),
            version: "1".into(),
        },
        verified_at: "2026-09-30T00:00:00Z".into(),
    }
}

#[test]
fn sdk_evidence_export_boundary() {
    // Isolate the process-wide counter even when the complete suite runs with
    // parallel tests. Fixture construction precedes each measurement; rejected
    // exports must allocate ZERO bytes, not allocate and immediately free a clone.
    const CHILD: &str = "POSSUMS_EVIDENCE_ALLOCATION_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "web::resource_streaming_tests::sdk_evidence_export_boundary",
                "--test-threads=1",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "isolated evidence allocation test failed"
        );
        print!("{}", String::from_utf8(output.stdout).unwrap());
        return;
    }
    let allocator = &crate::process_alloc_tests::ALLOCATOR;
    let reject = |evidence: &tinfoil::GroundTruth, host: &str| {
        let before = allocator.begin_phase();
        let result = resource_fixtures::verification_value(evidence, host);
        let after = allocator.snapshot();
        assert!(result.is_err());
        assert_eq!(
            after.phase_peak, before.live,
            "rejection allocated before returning"
        );
        assert_eq!(after.live, before.live);
    };
    let mut evidence = synthetic_ground_truth();
    evidence.code_measurement.type_ = tinfoil::PredicateType::SnpTdxMultiPlatformV1;
    evidence
        .code_measurement
        .registers
        .extend(["0".repeat(96), "x".repeat(16_777_217)]);
    evidence
        .code_measurement
        .equals(&evidence.enclave_measurement)
        .unwrap();
    reject(&evidence, "fixture.invalid");
    drop(evidence);

    // Every variable string, including nested identity, optional fields, both
    // measurement vectors, and the configuration-derived host. No field names
    // or contents are emitted by the sanitized production error.
    type Field = fn(&mut tinfoil::GroundTruth) -> &mut String;
    let fields: [Field; 12] = [
        |g| &mut g.config_repo,
        |g| g.release_tag.as_mut().unwrap(),
        |g| &mut g.digest,
        |g| g.tls_public_key.as_mut().unwrap(),
        |g| g.hpke_public_key.as_mut().unwrap(),
        |g| &mut g.code_fingerprint,
        |g| &mut g.enclave_fingerprint,
        |g| &mut g.verifier.name,
        |g| &mut g.verifier.version,
        |g| &mut g.verified_at,
        |g| &mut g.code_measurement.registers[0],
        |g| &mut g.enclave_measurement.registers[0],
    ];
    for field in fields {
        let mut evidence = synthetic_ground_truth();
        *field(&mut evidence) = "x".repeat(4097);
        reject(&evidence, "fixture.invalid");
    }
    reject(&synthetic_ground_truth(), &"x".repeat(4097));
    for code in [true, false] {
        let mut evidence = synthetic_ground_truth();
        let measurement = if code {
            &mut evidence.code_measurement
        } else {
            &mut evidence.enclave_measurement
        };
        measurement.registers = vec![String::new(); 33];
        reject(&evidence, "fixture.invalid");
    }
    let mut aggregate = synthetic_ground_truth();
    aggregate.code_measurement.registers = vec!["x".repeat(4096); 4];
    reject(&aggregate, "fixture.invalid"); // Each field fits; aggregate does not.
    aggregate.code_measurement.registers.clear();
    aggregate.tls_public_key = Some("x".repeat(4096));
    aggregate.hpke_public_key = Some("x".repeat(4096));
    reject(&aggregate, "fixture.invalid"); // Output duplicates keys: >16 KiB.
    for tls in [true, false] {
        let mut missing = synthetic_ground_truth();
        if tls {
            missing.tls_public_key = None;
        } else {
            missing.hpke_public_key = None;
        }
        reject(&missing, "fixture.invalid");
    }

    let mut valid = synthetic_ground_truth();
    valid.code_measurement.registers = vec!["\0".repeat(200); 32];
    valid.enclave_measurement.registers = vec!["\0".repeat(200); 32];
    valid.release_tag = Some("\0".repeat(2048));
    // Compare to the unchanged SDK projection outside the measured export.
    let expected = serde_json::to_value(
        tinfoil::VerificationDocument::from_ground_truth(valid.clone(), "fixture.invalid".into())
            .unwrap(),
    )
    .unwrap();
    let before = allocator.begin_phase();
    let value = resource_fixtures::verification_value(&valid, "fixture.invalid").unwrap();
    let after = allocator.snapshot();
    assert_eq!(value, expected);
    let export_peak = after.phase_peak - before.live;
    assert!(export_peak < 1024 * 1024);
    println!("bounded evidence: unused_register_bytes=16777217 rejected_export_peak_delta=0 other_fields=12 host=1 vector_limits=2 aggregate_cases=2 missing_keys=2 valid_export_peak_delta={export_peak}");
}

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
pub(crate) struct PreflightHooks {
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

    pub(crate) fn input(
        &self,
        phase: &'static str,
        history: &Vec<crate::inference::Message>,
        prompt: &String,
    ) {
        if self.capture.load(std::sync::atomic::Ordering::SeqCst) {
            self.capacities.lock().unwrap().push(InputCapacity {
                phase,
                messages: history.len(),
                vector_bytes: history.capacity() * std::mem::size_of::<crate::inference::Message>(),
                strings: history
                    .iter()
                    .map(|m| m.role.capacity() + m.content.capacity())
                    .sum(),
                prompt: prompt.capacity(),
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
    pub(crate) async fn before_compose(&self) {
        self.resources.at("pre-compose").await;
        let pause = self.before.lock().unwrap().take();
        if let Some(pause) = pause {
            pause.wait().await;
        }
    }
    pub(crate) async fn after_compose(&self) {
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
        inference::{stream, Inference, InferenceError, Message},
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
        evidence: Mutex<tinfoil::GroundTruth>,
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
            heavy: std::sync::Arc<crate::telemetry::hooks::Lease>,
        ) -> Result<u64, InferenceError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let (body, released) = resource_fixtures::tokenizer_body(model, messages, heavy)?;
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
            released.await;
            Ok(self.tokens)
        }

        async fn generate_stream(
            &self,
            model: &Model,
            messages: &[Message],
            _heavy: std::sync::Arc<crate::telemetry::hooks::Lease>,
            on_delta: &mut (dyn for<'d> FnMut(&'d str) + Send),
        ) -> Result<stream::StreamUsage, InferenceError> {
            self.generations.fetch_add(1, Ordering::SeqCst);
            assert_eq!(model.id, "m");
            assert_eq!(model.max_output_tokens, 19); // Full context-legal allowance.
            let body = resource_fixtures::stream_body(
                &model.id,
                model.max_output_tokens,
                messages,
                _heavy,
            )?;
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
            resource_fixtures::verification_value(&self.evidence.lock().unwrap(), "fixture.invalid")
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
                evidence: Mutex::new(synthetic_ground_truth()),
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
    #[tokio::test]
    async fn attestation_uses_bounded_adapter_export() {
        // Real router and production export helper, with explicitly SYNTHETIC
        // ground truth. SDK snapshot selection/refresh has separate SDK tests.
        let (fixture, _peer) = Fixture::new(1).await;
        let request = || {
            Request::builder()
                .uri("/attestation")
                .body(Body::empty())
                .unwrap()
        };
        let response = router(fixture.state.clone())
            .oneshot(request())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let document: serde_json::Value =
            serde_json::from_str(&drain(response.into_body()).await).unwrap();
        let expected =
            resource_fixtures::verification_value(&synthetic_ground_truth(), "fixture.invalid")
                .unwrap();
        assert_eq!(document["upstream"], expected);
        fixture
            .probe
            .evidence
            .lock()
            .unwrap()
            .code_measurement
            .registers
            .push("x".repeat(16_777_217));
        let response = router(fixture.state.clone())
            .oneshot(request())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = drain(response.into_body()).await;
        assert!(!body.contains("fixture.invalid"));
        assert!(!body.contains("16777217"));
        assert_eq!(fixture.probe.calls.load(Ordering::SeqCst), 0);
        assert_eq!(fixture.probe.generations.load(Ordering::SeqCst), 0);
    }

    async fn complete(peer: &support::RawPeer, input: u64, output: u64) {
        peer.send(&support::event(support::choice(None, Some("stop"))), 1024)
            .await;
        peer.send(&support::event(support::usage(input, output)), 1024)
            .await;
        peer.send(b"data: [DONE]\n\n", 1024).await;
        peer.eof().await;
    }

    #[test]
    fn shared_preflight_pre_poll_drop_refunds_after_destroying_handoff_inputs() {
        use crate::generation::{GenerationInput, Submission};
        use std::task::Poll;

        struct DropProbe {
            heavy: Arc<Semaphore>,
            slots: Arc<Semaphore>,
            dropped_while_charged: Arc<AtomicBool>,
        }
        impl Drop for DropProbe {
            fn drop(&mut self) {
                self.dropped_while_charged.store(
                    self.heavy.available_permits() == 3 && self.slots.available_permits() == 3,
                    Ordering::SeqCst,
                );
            }
        }

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let (fixture, peer) = runtime.block_on(Fixture::new(1));
        let dropped = Arc::new(AtomicBool::new(false));
        let probe = DropProbe {
            heavy: fixture.state.chat_memory.clone(),
            slots: fixture.state.generation_slots.clone(),
            dropped_while_charged: dropped.clone(),
        };
        runtime.block_on(async {
            let submission = fixture.state.generation().submit(
                Submission {
                    session_id: &fixture.session,
                    csrf: &fixture.csrf,
                    token: &fixture.token,
                },
                GenerationInput {
                    model: "m".into(),
                    history: Vec::new(),
                    prompt: "hello".into(),
                },
                Arc::new(
                    fixture
                        .state
                        .chat_memory
                        .clone()
                        .try_acquire_owned()
                        .unwrap()
                        .into(),
                ),
                move |owner, _| {
                    drop(probe);
                    drop(owner);
                    panic!("pre-poll handoff must not run");
                },
            );
            tokio::pin!(submission);
            // Immediate fixture evidence/catalog allow admission in this poll.
            // Do not yield to the detached task before shutting down the runtime.
            assert!(matches!(
                futures_util::poll!(&mut submission),
                Poll::Pending
            ));
            assert_eq!(fixture.state.accounting.available("a"), Some(48));
            assert_eq!(fixture.probe.calls.load(Ordering::SeqCst), 0);
        });
        assert!(!dropped.load(Ordering::SeqCst)); // Dropping observer did not cancel.
        drop(runtime);
        assert!(dropped.load(Ordering::SeqCst));
        assert_eq!(fixture.state.accounting.available("a"), Some(100));
        assert_eq!(fixture.state.generation_slots.available_permits(), 4);
        assert_eq!(fixture.state.chat_memory.available_permits(), 4);
        assert_eq!(fixture.probe.calls.load(Ordering::SeqCst), 0);
        assert_eq!(fixture.probe.generations.load(Ordering::SeqCst), 0);
        drop(peer);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shared_preflight_moves_input_to_non_html_handoff_at_original_price() {
        use crate::generation::{GenerationInput, Rejection, Submission};

        let (fixture, peer) = Fixture::new(1).await;
        let (pause, gate) = checkpoint();
        *fixture.probe.tokenizer.lock().unwrap() = Some(pause);
        let state = fixture.state.clone();
        let id = fixture.session.clone();
        let csrf = fixture.csrf.clone();
        let token = fixture.token.clone();
        // Spare capacity permits the tokenizer's temporary prompt push without
        // reallocating: assert the core transfers these allocations, not copies.
        let mut history = Vec::with_capacity(4);
        history.push(Message {
            role: "user".into(),
            content: "prior".into(),
        });
        history.push(Message {
            role: "assistant".into(),
            content: "reply".into(),
        });
        let prompt = "hello".to_owned();
        let history_ptr = history.as_ptr() as usize;
        let prompt_ptr = prompt.as_ptr() as usize;
        let waiter = tokio::spawn(async move {
            let inference = state.inference.clone();
            let heavy: Arc<crate::telemetry::hooks::Lease> = Arc::new(
                state
                    .chat_memory
                    .clone()
                    .try_acquire_owned()
                    .unwrap()
                    .into(),
            );
            let work_heavy = heavy.clone();
            state
                .generation()
                .submit(
                    Submission {
                        session_id: &id,
                        csrf: &csrf,
                        token: &token,
                    },
                    GenerationInput {
                        model: "m".into(),
                        history,
                        prompt,
                    },
                    heavy,
                    move |owner, mut prepared| {
                        assert_eq!(prepared.history.as_ptr() as usize, history_ptr);
                        assert_eq!(prepared.prompt.as_ptr() as usize, prompt_ptr);
                        assert_eq!(prepared.history.len(), 2);
                        assert_eq!(prepared.reserved_microunits, 52);
                        assert_eq!(prepared.model.max_output_tokens, 19);
                        assert_eq!(
                            prepared.model.input_microunits_per_million_tokens,
                            1_000_000
                        );
                        assert_eq!(
                            prepared.model.output_microunits_per_million_tokens,
                            1_000_000
                        );
                        // No HTML composer, body, renderer or answer buffer. The same
                        // owner remains terminal authority for this synthetic sink.
                        drop(owner.spawn_settling((), move |(), settlement| async move {
                            prepared.history.push(Message {
                                role: "user".into(),
                                content: prepared.prompt,
                            });
                            let result = inference
                                .generate_stream(
                                    &prepared.model,
                                    &prepared.history,
                                    work_heavy,
                                    &mut |_| {},
                                )
                                .await;
                            settlement.finish(&result)
                        }));
                    },
                )
                .await
        });
        gate.reached.await.unwrap();
        fixture.probe.repriced.store(true, Ordering::SeqCst);
        // Duplicate admission must neither construct a guard nor invoke handoff.
        let duplicate = fixture
            .state
            .generation()
            .submit(
                Submission {
                    session_id: &fixture.session,
                    csrf: &fixture.csrf,
                    token: &fixture.token,
                },
                GenerationInput {
                    model: "m".into(),
                    history: Vec::new(),
                    prompt: "changed".into(),
                },
                Arc::new(
                    fixture
                        .state
                        .chat_memory
                        .clone()
                        .try_acquire_owned()
                        .unwrap()
                        .into(),
                ),
                |_, _| panic!("duplicate handoff must not run"),
            )
            .await;
        assert_eq!(
            duplicate,
            Err(Rejection::Duplicate(crate::accounting::Outcome::InFlight))
        );
        assert_eq!(fixture.state.accounting.available("a"), Some(48));
        assert_eq!(fixture.probe.calls.load(Ordering::SeqCst), 1);
        waiter.abort();
        assert!(waiter.await.unwrap_err().is_cancelled());
        gate.release.send(()).unwrap();
        fixture.probe.entered.notified().await;
        complete(&peer, 2, 3).await;
        fixture.terminal(93).await;
        assert_eq!(fixture.probe.generations.load(Ordering::SeqCst), 1);
        // Generation-slot availability alone is not a memory-lease rendezvous.
        let memory = tokio::time::timeout(
            Duration::from_secs(5),
            fixture.state.chat_memory.clone().acquire_many_owned(4),
        )
        .await
        .unwrap()
        .unwrap();
        drop(memory);
        assert_eq!(fixture.state.chat_memory.available_permits(), 4);
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
            let (startup_tx, startup_rx) = oneshot::channel();
            let has_token = Arc::new(AtomicBool::new(false));
            let seen = has_token.clone();
            let reader = tokio::spawn(async move {
                let mut startup_tx = Some(startup_tx);
                let mut body = response.into_body();
                let frame = body.frame().await.unwrap().unwrap().into_data().unwrap();
                // Keep a one-byte slice after worker completion, not the full body.
                frame_tx.send(frame.slice(..1)).unwrap();
                drop(frame);
                let mut html = String::new();
                while let Some(frame) = body.frame().await {
                    if let Ok(data) = frame.unwrap().into_data() {
                        html.push_str(std::str::from_utf8(&data).unwrap());
                        if html.contains("<pre aria-label=\"Assistant\">") {
                            if let Some(tx) = startup_tx.take() {
                                tx.send(()).unwrap();
                            }
                        }
                        seen.store(html.contains("name=token"), Ordering::SeqCst);
                    }
                }
                html
            });
            let retained: Bytes = frame_rx.await.unwrap();
            fixture.probe.entered.notified().await;
            startup_rx.await.unwrap();
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
            peer.allow_disconnect();
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
            // Settlement and the generation slot can finish before the last
            // admitted upload/delivery owner releases its heavy lease.
            tokio::time::timeout(Duration::from_secs(5), async {
                while fixture.state.chat_memory.available_permits() != 4 {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap_or_else(|_| panic!("heavy lease retained after fault {fault}"));
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
    let lane = Arc::new(Semaphore::new(1));
    let heavy: Arc<crate::telemetry::hooks::Lease> =
        Arc::new(lane.clone().try_acquire_owned().unwrap().into());
    for (body, streaming) in [
        (
            resource_fixtures::tokenizer_body("fixture", &messages, heavy.clone())
                .unwrap()
                .0,
            false,
        ),
        (
            resource_fixtures::stream_body("fixture", 99, &messages, heavy.clone()).unwrap(),
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
        crate::telemetry::hooks::Lease::from(lease),
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
