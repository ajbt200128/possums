//! Isolated synthetic composition tests: no network, credentials or live inference.
//! PRIVACY.md#mandatory-typed-interface-failures: accepted work continues without
//! early billing; terminal evidence never invents completion or replay authority.
use super::*;
use crate::{
    accounting::{
        remote_fixture::{receipt, reservation, script},
        AccountingError, ReserveResult,
    },
    catalog::{Quote, ReasoningOptions},
    inference::{stream::StreamCompletion, stream::StreamUsage, Inference},
    intake::{cutoff_fixture, TerminalOutcome},
    lifecycle::{DrainFailed, Lifecycle},
    telemetry::hooks::Lease,
};
use futures_util::poll;
use http_body_util::BodyExt as _;
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};
use tokio::sync::oneshot;

const FORMER_WORK_BUDGET: Duration = Duration::from_secs(29 * 60);
const INITIAL_HOLD: Duration = Duration::from_secs(30 * 60);

fn quote() -> Quote {
    Quote {
        model: Model {
            id: "m".into(),
            public_name: None,
            reasoning: None,
            reasoning_params: None,
            context_tokens: 100,
            max_output_tokens: 10,
            input_microunits_per_million_tokens: 2_000_000,
            output_microunits_per_million_tokens: 3_000_000,
            cached_input_microunits_per_million_tokens: None,
        },
        input_tokens: 5,
        reserved_microunits: 52,
    }
}

fn lease(lane: crate::telemetry::Lane) -> Lease {
    Lease::tracking(Arc::new(AtomicUsize::new(0)), None, lane)
}

async fn bounded<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(5), future)
        .await
        .expect("synthetic continued-work barrier exceeded its budget")
}

struct ProviderLifetime {
    cancellations: Arc<AtomicUsize>,
    exited: Option<oneshot::Sender<()>>,
    returned: bool,
}
impl Drop for ProviderLifetime {
    fn drop(&mut self) {
        if !self.returned {
            self.cancellations.fetch_add(1, Ordering::SeqCst);
        }
        let _ = self.exited.take().unwrap().send(());
    }
}

#[derive(Clone, Copy)]
enum Ending {
    Complete,
    DecodeFailure,
}
impl Ending {
    fn failure(self) -> InferenceFailure {
        match self {
            Self::Complete => InferenceFailure::SettlementFailed,
            Self::DecodeFailure => InferenceFailure::SdkStreamDecodeFailed,
        }
    }
}

struct SyntheticInference {
    calls: AtomicUsize,
    cancellations: Arc<AtomicUsize>,
    stalled: Mutex<Option<oneshot::Sender<()>>>,
    exited: Mutex<Option<oneshot::Sender<()>>>,
    release: Mutex<Option<oneshot::Receiver<()>>>,
    backpressure: bool,
    ending: Ending,
}

#[async_trait]
impl Inference for SyntheticInference {
    async fn catalog(&self) -> Result<Vec<u8>, InferenceError> {
        panic!("continued-work fixture must not discover models")
    }

    async fn count_tokens(
        &self,
        _: &str,
        _: &[Message],
        _: Arc<Lease>,
    ) -> Result<u64, InferenceError> {
        panic!("continued-work fixture must not tokenize or replay")
    }

    fn verification_document(&self) -> Result<Value, InferenceError> {
        panic!("continued-work fixture must not acquire external evidence")
    }

    async fn generate_forwarded_request(
        &self,
        _: &Model,
        _: (&[Message], Option<&ToolInvocation>, &ReasoningOptions),
        _: &crate::auth::CacheScope,
        _: Arc<Lease>,
        sink: &mut dyn ForwardSink,
    ) -> Result<StreamCompletion, InferenceError> {
        assert_eq!(self.calls.fetch_add(1, Ordering::SeqCst), 0);
        let mut lifetime = ProviderLifetime {
            cancellations: self.cancellations.clone(),
            exited: self.exited.lock().unwrap().take(),
            returned: false,
        };
        let release = self.release.lock().unwrap().take().unwrap();
        // Trustworthy synthetic cumulative snapshots, not increments. Only the
        // last snapshot may reach the single terminal charge attempt.
        sink.evidence(Evidence::sse(StreamUsage {
            input_tokens: 3,
            output_tokens: 2,
            total_tokens: 5,
        }));
        let usage = StreamUsage {
            input_tokens: 5,
            output_tokens: 4,
            total_tokens: 9,
        };
        sink.evidence(Evidence::sse(usage));
        let queued = if self.backpressure { 8 } else { 1 };
        for _ in 0..queued {
            sink.event(json!({"choices":[{"index":0,"delta":{"content":"synthetic"}}]}))
                .await?;
        }
        if self.backpressure {
            let blocked =
                sink.event(json!({"choices":[{"index":0,"delta":{"content":"blocked"}}]}));
            tokio::pin!(blocked);
            // Publish only after the ninth frame really blocks on the composed
            // sink, with the connected reader still alive.
            assert!(poll!(&mut blocked).is_pending());
            self.stalled
                .lock()
                .unwrap()
                .take()
                .unwrap()
                .send(())
                .unwrap();
            blocked.await?;
        } else {
            self.stalled
                .lock()
                .unwrap()
                .take()
                .unwrap()
                .send(())
                .unwrap();
        }
        release.await.unwrap();
        lifetime.returned = true;
        match self.ending {
            Ending::Complete => Ok(StreamCompletion {
                usage,
                finish_reason: crate::inference::stream::FinishReason::Stop,
            }),
            Ending::DecodeFailure => Err(InferenceFailure::SdkStreamDecodeFailed.into()),
        }
    }
}

#[derive(Clone, Copy)]
enum Terminal {
    CommitUnknown,
    Expired,
    Settled,
}
impl Terminal {
    fn diagnostic(self) -> Option<Value> {
        match self {
            Self::CommitUnknown => Some(json!({
                "code":"INTAKE_REMOTE_UNAVAILABLE", "operation":"intake_settle",
                "stage":"transaction_completion", "constraint":"commit_evidence", "status":503
            })),
            Self::Expired => Some(json!({
                "code":"INTAKE_HOLD_EXPIRED", "operation":"intake_settle",
                "stage":"settlement", "constraint":"immutable_hold_expiry", "status":409
            })),
            Self::Settled => None,
        }
    }
}

fn parse_events(text: &str) -> (Vec<Value>, bool) {
    let mut events = Vec::new();
    let mut done = false;
    for line in text.lines().filter(|line| !line.is_empty()) {
        assert!(!done, "no event may follow DONE");
        let data = line.strip_prefix("data: ").expect("native SSE data frame");
        if data == "[DONE]" {
            done = true;
        } else {
            events.push(serde_json::from_str(data).expect("synthetic SSE JSON"));
        }
    }
    (events, done)
}

async fn continued_work(terminal: Terminal, backpressure: bool, ending: Ending, disconnect: bool) {
    let script = script();
    let lifecycle = Lifecycle::default();
    let quoted = quote();
    assert_eq!(
        script.accounting.reserve(
            "a",
            [1; 32],
            [2; 32],
            quoted.clone(),
            Instant::now() + Duration::from_secs(60),
        ),
        Ok(ReserveResult::Claimed)
    );
    let heavy = Arc::new(lease(crate::telemetry::Lane::Heavy));
    let mut owner = ReservedGeneration::new(
        script.accounting.clone(),
        [1; 32],
        lease(crate::telemetry::Lane::Generation),
        heavy.clone(),
    )
    .tracked(lifecycle.try_admit().unwrap());
    let ready = owner.start_remote().expect("owned remote terminal actor");
    let (key, maximum) = bounded(script.reserve_started).await.unwrap();
    assert_eq!(maximum, 52);
    let active = reservation(&key, INITIAL_HOLD.as_secs(), "active");
    let expires_at = active.expires_at();
    let reply = match terminal {
        Terminal::CommitUnknown => Err(AccountingError::Intake(cutoff_fixture::commit_unknown())),
        Terminal::Expired => Ok(cutoff_fixture::expired(&active)),
        // Only the before-expiry scenarios script a successful charge. This is
        // not evidence that production can collect from an expired reservation.
        Terminal::Settled => receipt(TerminalOutcome::Settled { charged: 29 }, true),
    };
    assert!(script.reserve.send(Ok(active)).is_ok());
    assert_eq!(bounded(ready).await.unwrap(), Ok(expires_at));

    let (stalled, provider_stalled) = oneshot::channel();
    let (exited, mut provider_exited) = oneshot::channel();
    let (release, released) = oneshot::channel();
    let inference = Arc::new(SyntheticInference {
        calls: AtomicUsize::new(0),
        cancellations: Arc::new(AtomicUsize::new(0)),
        stalled: Mutex::new(Some(stalled)),
        exited: Mutex::new(Some(exited)),
        release: Mutex::new(Some(released)),
        backpressure,
        ending,
    });
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(crate::intake::diagnostics::HEADER, "v1".parse().unwrap());
    headers.insert("x-tinfoil-request-usage-metrics", "true".parse().unwrap());
    let body = compose_core(
        owner,
        heavy,
        quoted.model,
        quoted.reserved_microunits,
        (
            crate::auth::CacheScope::default(),
            vec![Message {
                role: "user".into(),
                content: "synthetic".into(),
            }],
            None,
            ReasoningOptions::default(),
        ),
        inference.clone(),
        Options::parse(&headers),
    );
    bounded(provider_stalled).await.unwrap();
    let queued = if backpressure { 8 } else { 1 };
    assert_eq!(body.usage().frames, queued);
    assert_eq!(body.usage().high_frames, queued);
    lifecycle.quiesce();
    let drain = lifecycle.wait_drained();
    tokio::pin!(drain);
    assert!(poll!(&mut drain).is_pending());

    let mut finish_started = script.finish_started;
    if !matches!(terminal, Terminal::Settled) {
        // Paused Tokio time exercises both former cancellation horizons. It
        // does not advance SystemTime; the expired scenario separately uses
        // authoritative, bound 409 evidence through check_terminal.
        for elapsed in [
            FORMER_WORK_BUDGET,
            INITIAL_HOLD - FORMER_WORK_BUDGET + Duration::from_secs(1),
        ] {
            tokio::time::advance(elapsed).await;
            tokio::task::yield_now().await;
            assert_eq!(body.usage().frames, queued);
            assert_eq!(inference.calls.load(Ordering::SeqCst), 1);
            assert_eq!(inference.cancellations.load(Ordering::SeqCst), 0);
            assert_eq!(script.service.finishes.load(Ordering::SeqCst), 0);
            assert!(matches!(
                finish_started.try_recv(),
                Err(oneshot::error::TryRecvError::Empty)
            ));
            assert!(matches!(
                provider_exited.try_recv(),
                Err(oneshot::error::TryRecvError::Empty)
            ));
            assert!(poll!(&mut drain).is_pending());
        }
    }

    let collected = if disconnect {
        drop(body); // A genuine body loss still cancels the original operation.
        None
    } else {
        // Free the eight credits and let the same ninth send resume naturally.
        let collected = tokio::spawn(async move {
            let mut body = body;
            let mut bytes = Vec::new();
            let mut trailers = false;
            while let Some(frame) = body.frame().await {
                let frame = frame.unwrap();
                if frame.is_trailers() {
                    trailers = true;
                } else {
                    // Copy and release each credit: retaining eight DATA frames
                    // in BodyExt::collect would itself keep the ninth blocked.
                    let data = frame.into_data().unwrap();
                    bytes.extend_from_slice(&data);
                    assert!(bytes.len() <= 64 * 1024);
                }
            }
            (bytes, trailers)
        });
        release.send(()).unwrap();
        Some(collected)
    };
    assert_eq!(bounded(finish_started).await.unwrap(), Some(29));
    bounded(provider_exited).await.unwrap();
    assert_eq!(
        inference.cancellations.load(Ordering::SeqCst),
        usize::from(disconnect)
    );
    assert_eq!(script.service.finishes.load(Ordering::SeqCst), 1);
    assert!(poll!(&mut drain).is_pending());
    assert!(script.finish.send(reply).is_ok());

    if let Some(collected) = collected {
        let (bytes, trailers) = bounded(collected).await.unwrap();
        assert!(!trailers, "SSE evidence has no metrics trailer");
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(!text.contains("INTAKE_HOLD_WORK_DEADLINE"));
        let (events, done) = parse_events(text);
        let complete = matches!(terminal, Terminal::Settled) && matches!(ending, Ending::Complete);
        let errors: Vec<_> = events
            .iter()
            .filter_map(|event| event.get("error"))
            .collect();
        assert_eq!(errors.len(), usize::from(!complete));
        if !complete {
            let error = errors[0];
            let diagnostic = terminal.diagnostic();
            assert_eq!(
                error.as_object().unwrap().len(),
                if diagnostic.is_some() { 5 } else { 4 }
            );
            assert_eq!(error["code"], "generation_failed");
            assert_eq!(
                error["detail"],
                serde_json::to_value(ending.failure()).unwrap()
            );
            assert_eq!(error["billing"], "unknown");
            assert_eq!(error.get("intake"), diagnostic.as_ref());
            let message = error["message"].as_str().unwrap();
            if matches!(ending, Ending::DecodeFailure) {
                assert!(message.contains(&ending.failure().to_string()));
            }
            if terminal.diagnostic().is_some() {
                let message = message.to_ascii_lowercase();
                assert!(
                    message.contains("no replay occurred")
                        || message.contains("no automatic replay occurred")
                );
            }
            for forbidden in [
                "charged_microunits",
                "refunded_microunits",
                "reserved_microunits",
            ] {
                assert!(error.get(forbidden).is_none());
            }
        }
        assert!(events
            .iter()
            .all(|event| event.pointer("/choices/0/delta/tool_calls").is_none()));
        let receipts: Vec<_> = events
            .iter()
            .filter_map(|event| event.get("possums"))
            .collect();
        if matches!(terminal, Terminal::Settled) {
            assert_eq!(receipts.len(), 1);
            let receipt = receipts[0];
            assert_eq!(receipt["outcome"], "settled");
            assert_eq!(
                receipt["status"],
                if complete { "complete" } else { "incomplete" }
            );
            assert_eq!(receipt["charged_microunits"], "29");
            assert_eq!(receipt["refunded_microunits"], "23");
            assert_eq!(
                receipt["usage"],
                json!({"prompt_tokens":5,"completion_tokens":4,"total_tokens":9})
            );
            assert!(done);
        } else {
            assert!(receipts.is_empty(), "no invented charge/refund receipt");
            assert!(!done);
            assert!(events
                .iter()
                .all(|event| event.pointer("/possums/status") != Some(&json!("complete"))));
        }
        assert_eq!(
            events.len(),
            queued + usize::from(backpressure) + usize::from(!complete) + receipts.len()
        );
    }
    assert_eq!(
        bounded(drain).await,
        if matches!(terminal, Terminal::Settled) {
            Ok(())
        } else {
            Err(DrainFailed)
        }
    );
    assert_eq!(
        inference.calls.load(Ordering::SeqCst),
        1,
        "same provider future, no replay"
    );
    assert_eq!(
        inference.cancellations.load(Ordering::SeqCst),
        usize::from(disconnect)
    );
    assert_eq!(script.service.reserves.load(Ordering::SeqCst), 1);
    assert_eq!(script.service.finishes.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn connected_accepted_work_survives_both_horizons_until_natural_terminal_attempt() {
    for terminal in [Terminal::CommitUnknown, Terminal::Expired] {
        for backpressure in [false, true] {
            for ending in [Ending::Complete, Ending::DecodeFailure] {
                continued_work(terminal, backpressure, ending, false).await;
            }
        }
    }
}

#[tokio::test(start_paused = true)]
async fn genuine_disconnect_after_hold_horizon_cancels_and_attempts_billing_once() {
    for backpressure in [false, true] {
        continued_work(Terminal::Expired, backpressure, Ending::Complete, true).await;
    }
}

#[tokio::test(start_paused = true)]
async fn before_expiry_success_keeps_complete_and_partial_receipts_distinct() {
    for backpressure in [false, true] {
        for ending in [Ending::Complete, Ending::DecodeFailure] {
            continued_work(Terminal::Settled, backpressure, ending, false).await;
        }
    }
}
