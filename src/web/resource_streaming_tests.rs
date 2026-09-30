//! Packet-3 proof target, NOT an implemented/passing aggregate gate.
//!
//! Exact identity: web::resource_streaming_tests::combined_resource_gate
//! Command (fresh test process; assert harness reports exactly ONE selected test):
//! nix develop -c cargo test --lib web::resource_streaming_tests::combined_resource_gate -- --exact --test-threads=1 --nocapture
//! No test with that name exists until the route workload is implemented. A
//! zero-selected-test exit is NOT evidence. The seam tests below are not the gate.
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
//!   after compose. Packet 2 adds optional per-request cfg(test) hooks, carried
//!   from AppState into detached preflight, never process-global hooks. At pre-
//!   compose pause the original owner/input/permit live; at post-compose pause
//!   only the queued body owns delivery while generation runs independently.
//!   Observer abort/drop must not abort those tasks. Release/dropped test hooks
//!   cannot veto generation; production has NO corresponding await gap.
//! - Child module reads AppState semaphores directly. Ledger assertions use real
//!   accounting snapshots/outcomes and fixture call counts, not a copied ledger.
//!   Add cfg(test) ledger events only if public prompt-free snapshots cannot prove
//!   exactly-once finish. Never put identifiers/content in exported diagnostics.
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
//! Conservative capacity ALLOCATIONS TO JUSTIFY at the gate, not proven limits:
//! | Owner / maximum simultaneous lanes | MiB each | MiB total |
//! | heavy (decoded/vector/preflight/startup/parser/delivery) x4 | 104 | 416 |
//! | raw ingress (including collection capacity) x4            |  16 |  64 |
//! | complete New chat lane x1                                 |  16 |  16 |
//! | ordinary controls lane x1                                 |  16 |  16 |
//! | scoped admission TOTAL                                   |     | 512 |
//! Heavy worksheet per lane: 40 MiB decoded strings/message vector capacities
//! (including conversion/growth overlap); 8 MiB decode JSON/staging; 32 MiB
//! serializer capacity/transients (16-MiB logical limit is NOT capacity proof);
//! 8 MiB renderer/parser/delivery incl. 64-KiB/eight-frame owners; 8 MiB bounded
//! catalog/evidence/model metadata; 8 MiB task/channel/blocking-result slack.
//! Derive actual capacities from accepted form shapes and allocator/growth code,
//! not just observed peaks. Mutually exclusive phases may share allowances only
//! when destruction/barrier evidence proves non-overlap. Startup inputs and
//! unclaimed results retain heavy, not an uncharged fifth lane. Account for
//! payload capacity versus length, and retained slices charging full allocation.
//! Shared runtime/auth/ledger/catalog state and fixture overhead need explicit
//! separate attribution; they are not magically inside the 512-MiB lane sum.
//! If justified overlap exceeds a lane or 512 MiB, record command/input/phase/bytes
//! and STOP for a decision: no target increase or allowance/concurrency reduction.
//! SDK/TLS/helper universal bounds, allocator overhead and whole-process RSS remain
//! unproven even if local requested allocation fits. Synthetic transport proves
//! neither live Tinfoil authentication nor invoice/billable-cost bounds.

use crate::{inference::resource_fixtures, stream_owner};
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
