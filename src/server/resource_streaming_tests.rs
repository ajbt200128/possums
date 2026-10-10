//! API generation/resource ownership hooks and bounded SDK evidence fixtures.

use crate::{inference::resource_fixtures, stream_owner};
use http_body_util::BodyExt;
use std::{sync::Arc, time::Duration};
use tokio::sync::{oneshot, Semaphore};

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
                "server::resource_streaming_tests::sdk_evidence_export_boundary",
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
    pub(super) after: std::sync::Mutex<Option<Pause>>,
    pub(crate) resources: Arc<ResourceHooks>,
}

#[derive(Default)]
pub(crate) struct ResourceHooks {
    pauses: std::sync::Mutex<
        std::collections::BTreeMap<&'static str, std::collections::VecDeque<Pause>>,
    >,
    capture: std::sync::atomic::AtomicBool,
    raw: std::sync::Mutex<Vec<axum::body::Bytes>>,
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
    const CHILD: &str = "POSSUMS_API_RESOURCE_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "server::resource_streaming_tests::combined_resource_gate",
                "--test-threads=1",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "isolated API resource test failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        print!("{}", String::from_utf8_lossy(&output.stdout));
        return;
    }
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
        "api resource runtime-released={:?}",
        crate::process_alloc_tests::ALLOCATOR.snapshot()
    );
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
    let usage =
        resource_fixtures::consume_response(response.into(), Duration::from_secs(1), |delta| {
            assert_eq!(delta, "x");
            deltas += 1;
        })
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
    );
    let probe = body.probe();
    let mut tx = tx;
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
