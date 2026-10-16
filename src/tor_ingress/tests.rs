//! Offline only: no TorClient construction, launch, bootstrap or publication.
use super::*;
use async_trait::async_trait;
use futures_util::{poll, stream::BoxStream};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    },
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tor_keymgr::{ArtiNativeKeystore, KeyMgrBuilder, KeystoreSelector};

fn reporter() -> (ReportOwner, Arc<Mutex<Vec<Failure>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let output = seen.clone();
    (
        ReportOwner::new(Arc::new(move |failure| {
            output.lock().unwrap().push(failure)
        })),
        seen,
    )
}

async fn bounded<T>(work: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(5), work)
        .await
        .unwrap()
}

#[tokio::test]
async fn bad_peer_results_are_transient_closed_and_other_peers_continue() {
    let (report, seen) = reporter();
    for failure in [
        Failure::RendezvousAccept,
        Failure::RendezvousReject,
        Failure::StreamAccept,
        Failure::StreamReject,
        Failure::Circuit,
    ] {
        assert!(peer_result(
            async { Err::<(), _>("HOSTILE_SDK_TEXT/identifier/credential") },
            failure,
            &report
        )
        .await
        .is_none());
        assert_eq!(seen.lock().unwrap().last(), Some(&failure));
        let message = failure.to_string();
        assert!(message.contains("code="));
        assert!(message.contains("stage="));
        assert!(message.contains("cause=unknown"));
        assert!(!message.contains("HOSTILE"));
    }
    assert_eq!(
        peer_result(async { Ok::<_, ()>(42) }, Failure::StreamAccept, &report).await,
        Some(42)
    );
    assert_eq!(seen.lock().unwrap().len(), 5);
    assert!(peer_result::<(), ()>(
        async { panic!("HOSTILE_SYNTHETIC_PANIC") },
        Failure::RendezvousAccept,
        &report,
    )
    .await
    .is_none());
    assert_eq!(
        seen.lock().unwrap().last(),
        Some(&Failure::RendezvousAccept)
    );
    let peers = bounded_peers(
        stream::iter([Ok(false), Ok(true)]).chain(stream::pending()),
        move |good| {
            let report = report.clone();
            stream::once(async move {
                peer_result(
                    async {
                        if good {
                            Ok(42)
                        } else {
                            Err("HOSTILE_SDK_TEXT")
                        }
                    },
                    Failure::RendezvousAccept,
                    &report,
                )
                .await
            })
            .filter_map(std::future::ready)
            .map(Ok)
            .boxed()
        },
        |_| std::future::ready(()),
    );
    tokio::pin!(peers);
    assert_eq!(bounded(peers.next()).await, Some(Ok(42)));
}

#[test]
fn real_sdk_begin_is_accepted_only_on_port_80() {
    use tor_cell::relaycell::msg::{Begin, BeginFlags};
    for port in [0, 1, 79, 81, 443, u16::MAX] {
        let begin = Begin::new("ignored.invalid", port, BeginFlags::empty()).unwrap();
        assert!(!allowed(&IncomingStreamRequest::Begin(begin)));
    }
    let begin = Begin::new("ignored.invalid", 80, BeginFlags::empty()).unwrap();
    assert!(allowed(&IncomingStreamRequest::Begin(begin)));
    assert!(!allowed(&IncomingStreamRequest::BeginDir(
        tor_cell::relaycell::msg::BeginDir::default()
    )));
    assert_eq!(
        End::new_with_reason(EndReason::DONE).reason(),
        EndReason::DONE
    );
}

#[tokio::test]
async fn full_idle_circuits_bound_handshakes_reject_overflow_and_surface_root_eof() {
    let accepted = Arc::new(AtomicUsize::new(0));
    let rejected = Arc::new(AtomicUsize::new(0));
    let active = accepted.clone();
    let dropped = rejected.clone();
    let peers = bounded_peers(
        stream::iter((0..work_bound() + 3).map(Ok)),
        move |_| {
            active.fetch_add(1, Ordering::SeqCst);
            stream::pending::<Result<(), Failure>>()
        },
        move |_| {
            dropped.fetch_add(1, Ordering::SeqCst);
            std::future::ready(())
        },
    );
    tokio::pin!(peers);
    assert_eq!(bounded(peers.next()).await, Some(Err(Failure::SourceEnded)));
    assert_eq!(accepted.load(Ordering::SeqCst), work_bound());
    assert_eq!(rejected.load(Ordering::SeqCst), 3);
    assert_eq!(work_bound(), 64);
}

#[tokio::test]
async fn stalled_handshake_and_idle_circuit_do_not_starve_ready_circuit() {
    let source = stream::iter([Ok(0), Ok(1), Ok(2)]).chain(stream::pending());
    let peers = bounded_peers(
        source,
        |id| -> BoxStream<'static, Result<usize, Failure>> {
            match id {
                0 => stream::once(std::future::pending()).boxed(),
                1 => stream::pending().boxed(),
                _ => stream::iter([Ok(id)]).chain(stream::pending()).boxed(),
            }
        },
        |_| std::future::ready(()),
    );
    tokio::pin!(peers);
    assert_eq!(bounded(peers.next()).await, Some(Ok(2)));
}

#[tokio::test]
async fn fair_mux_alternates_ready_sources_and_never_spins_on_eof() {
    let combined = stream::select(stream::iter([0, 0, 0]), stream::iter([1, 1, 1]));
    assert_eq!(combined.collect::<Vec<_>>().await, vec![0, 1, 0, 1, 0, 1]);
    let (report, seen) = reporter();
    let empty_state = state();
    assert!(bounded(serve_sources_until(
        stream::empty(),
        stream::empty(),
        empty_state.clone(),
        Duration::from_secs(10),
        std::future::pending(),
        report
    ))
    .await
    .is_ok());
    assert!(!empty_state.lifecycle.is_serving());
    assert!(seen.lock().unwrap().is_empty());
    assert_eq!(engine_failure(stream::empty()).await, Failure::Engine);
    let (report, seen) = reporter();
    let onion = onion_streams(
        stream::poll_fn(|_| panic!("HOSTILE_SYNTHETIC_SOURCE_PANIC")),
        report.clone(),
    );
    let result = serve_sources_until(
        stream::pending(),
        onion,
        state(),
        Duration::from_secs(10),
        std::future::pending(),
        report,
    )
    .await;
    assert_eq!(
        result,
        Err(ServeFailure {
            source: Some(Failure::SourceUnknown),
            shared: None,
            reporting: None,
        })
    );
    assert_eq!(*seen.lock().unwrap(), vec![Failure::SourceUnknown]);
    assert!(!result.unwrap_err().to_string().contains("HOSTILE"));
}

#[tokio::test]
async fn engine_terminal_state_is_visible_but_initial_shutdown_is_not_fabricated_death() {
    for status in [(State::Broken, false), (State::Shutdown, true)] {
        assert_eq!(
            engine_failure(stream::once(async move { status }).chain(stream::pending())).await,
            Failure::Engine
        );
    }
    assert_eq!(
        engine_failure(
            stream::iter([
                (State::Shutdown, false),
                (State::Bootstrapping, false),
                (State::Shutdown, false),
            ])
            .chain(stream::pending())
        )
        .await,
        Failure::Engine
    );
    let running = engine_failure(
        stream::iter([
            (State::Shutdown, false),
            (State::Recovering, false),
            (State::Running, false),
        ])
        .chain(stream::pending()),
    );
    tokio::pin!(running);
    assert!(poll!(&mut running).is_pending());
}

#[tokio::test]
async fn readiness_tracks_only_supported_running_and_clears_on_recovery_and_terminal_eof() {
    let ready = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (send, mut recv) = tokio::sync::mpsc::channel(1);
    let statuses = stream::poll_fn(move |cx| recv.poll_recv(cx));
    let monitor = engine_status(statuses, Some(ready.clone()));
    tokio::pin!(monitor);
    for (state, expected) in [
        (State::Shutdown, false),
        (State::Bootstrapping, false),
        (State::Running, true),
        (State::Recovering, false),
        (State::Running, true),
    ] {
        send.send((state, false)).await.unwrap();
        bounded(async {
            loop {
                assert!(poll!(&mut monitor).is_pending());
                if ready.load(Ordering::Acquire) == expected {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await;
    }
    drop(send);
    assert_eq!(bounded(monitor).await, Failure::Engine);
    assert!(!ready.load(Ordering::Acquire));
}

#[tokio::test]
async fn mixed_sources_use_one_existing_64_connection_pool() {
    let mut tcp_peers = Vec::new();
    let mut onion_peers = Vec::new();
    let mut tcp_ios = Vec::new();
    let mut onion_ios = Vec::new();
    for (peers, ios) in [
        (&mut tcp_peers, &mut tcp_ios),
        (&mut onion_peers, &mut onion_ios),
    ] {
        for _ in 0..33 {
            let (peer, io) = tokio::io::duplex(64);
            peers.push(peer);
            ios.push(Ok(Box::new(io) as BoxIo));
        }
    }
    let offered = Arc::new(AtomicUsize::new(0));
    let source = |ios| {
        let offered = offered.clone();
        stream::iter(ios)
            .inspect(move |_| {
                offered.fetch_add(1, Ordering::SeqCst);
            })
            .chain(stream::pending())
    };
    let (report, seen) = reporter();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let serve = serve_sources_until(
        source(tcp_ios),
        source(onion_ios),
        state(),
        Duration::from_secs(10),
        async {
            stopped.await.unwrap();
        },
        report,
    );
    tokio::pin!(serve);
    bounded(async {
        loop {
            assert!(poll!(&mut serve).is_pending());
            if offered.load(Ordering::SeqCst) == 66 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    for peers in [&mut tcp_peers, &mut onion_peers] {
        for peer in &mut peers[..32] {
            let mut byte = [0];
            let read = peer.read(&mut byte);
            tokio::pin!(read);
            assert!(poll!(&mut read).is_pending());
        }
        let mut byte = [0];
        assert_eq!(bounded(peers[32].read(&mut byte)).await.unwrap(), 0);
    }
    stop.send(()).unwrap();
    assert!(bounded(serve).await.is_ok());
    assert!(seen.lock().unwrap().is_empty());
}

struct NeverInference;
#[async_trait]
impl crate::inference::Inference for NeverInference {
    async fn catalog(&self) -> Result<Vec<u8>, crate::inference::InferenceError> {
        unreachable!()
    }
    async fn count_tokens(
        &self,
        _: &str,
        _: &[crate::inference::Message],
        _: Arc<crate::telemetry::hooks::Lease>,
    ) -> Result<u64, crate::inference::InferenceError> {
        unreachable!()
    }
    async fn generate_stream(
        &self,
        _: &crate::catalog::Model,
        _: &[crate::inference::Message],
        _: Arc<crate::telemetry::hooks::Lease>,
        _: &mut (dyn for<'d> FnMut(&'d str) + Send),
    ) -> Result<crate::inference::stream::StreamUsage, crate::inference::InferenceError> {
        unreachable!()
    }
    fn verification_document(&self) -> Result<serde_json::Value, crate::inference::InferenceError> {
        unreachable!()
    }
}

fn state() -> AppState {
    AppState::new(
        crate::auth::Auth::from_json(
            r#"[{"id":"synthetic","credential_sha256":"CAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAg","demo_microunits":100}]"#,
        ).unwrap(),
        Arc::new(NeverInference),
        "unused-synthetic",
        Arc::new(crate::attestation::UnavailableEvidenceVerifier),
    )
}

#[tokio::test]
async fn duplex_and_compat_sources_share_actual_router_and_drain() {
    let (mut tcp_peer, tcp_io) = tokio::io::duplex(4096);
    let (mut onion_peer, onion_io) = tokio::io::duplex(4096);
    // Exercise the exact Futures-to-Tokio compat path without Arti/network.
    use tokio_util::compat::TokioAsyncReadCompatExt;
    let tcp = stream::once(async move { Ok(Box::new(tcp_io) as BoxIo) }).chain(stream::pending());
    let onion = stream::once(async move { Ok(Box::new(onion_io.compat().compat()) as BoxIo) })
        .chain(stream::pending());
    let state = state();
    let (report, seen) = reporter();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(serve_sources_until(
        tcp,
        onion,
        state.clone(),
        Duration::from_secs(10),
        async {
            stopped.await.unwrap();
        },
        report,
    ));
    for peer in [&mut tcp_peer, &mut onion_peer] {
        peer.write_all(b"GET /healthz HTTP/1.1\r\nHost: synthetic\r\n\r\n")
            .await
            .unwrap();
        let mut response = Vec::new();
        bounded(peer.read_to_end(&mut response)).await.unwrap();
        assert!(response.starts_with(b"HTTP/1.1 204"));
    }
    stop.send(()).unwrap();
    assert!(bounded(server).await.unwrap().is_ok());
    assert!(!state.lifecycle.is_serving());
    assert!(seen.lock().unwrap().is_empty());
}

struct Retained(Arc<AtomicUsize>);
impl Drop for Retained {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn source_primary_and_drain_failure_survive_without_aborting_original_owner() {
    let state = state();
    let root = state.lifecycle.try_admit().unwrap();
    state.lifecycle.accounting_failed();
    let retained = Arc::new(AtomicUsize::new(0));
    let guard = Retained(retained.clone());
    let onion = stream::unfold(Some(guard), |guard| async move {
        Some((Err::<BoxIo, _>(Failure::Engine), guard))
    });
    let (report, seen) = reporter();
    let serve = serve_sources_until(
        stream::pending(),
        onion,
        state.clone(),
        Duration::from_secs(10),
        std::future::pending(),
        report,
    );
    tokio::pin!(serve);
    assert!(poll!(&mut serve).is_pending());
    assert!(!state.lifecycle.is_serving());
    assert_eq!(retained.load(Ordering::SeqCst), 0);
    assert_eq!(*seen.lock().unwrap(), vec![Failure::Engine]);
    assert!(poll!(&mut serve).is_pending());
    drop(root);
    assert_eq!(
        bounded(serve).await,
        Err(ServeFailure {
            source: Some(Failure::Engine),
            shared: Some(ServerFailure::ShutdownIntegrity),
            reporting: None,
        })
    );
    assert_eq!(retained.load(Ordering::SeqCst), 1);
    assert_eq!(seen.lock().unwrap().len(), 1);
}

fn panicking_reporter() -> ReportOwner {
    // Fixed content-free payload: the host's process hook precedes containment.
    ReportOwner::new(Arc::new(|_| panic!("synthetic reporter fault")))
}

#[test]
fn reporting_fault_retains_first_category_without_recursive_fallback() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let report = ReportOwner::new(Arc::new(move |_| {
        observed.fetch_add(1, Ordering::SeqCst);
        panic!("synthetic reporter fault");
    }));
    report.report(Failure::PortRefused);
    report.report(Failure::StreamReject);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let failure = report.failure().unwrap();
    assert_eq!(failure.original, Failure::PortRefused);
    let text = failure.to_string();
    for required in [
        "code=tor_port_refused",
        "code=tor_reporter_unknown",
        "stage=reporter",
        "constraint=host_callback",
        "cause=unknown",
        "action=",
        "billing unknown",
        "not replayed",
        "may incur another charge",
    ] {
        assert!(text.contains(required));
    }
    assert!(!text.contains("synthetic reporter fault"));
}

// Reuse the real reservation/terminal owner and current module's lifecycle
// fixture: no replacement accounting, inference call or authorization path.
fn held_original_generation(
    state: &AppState,
) -> (
    tokio::sync::oneshot::Sender<()>,
    tokio::sync::oneshot::Receiver<
        Result<crate::accounting::Outcome, crate::generation_owner::OwnerError>,
    >,
) {
    use crate::{
        accounting::ReserveResult,
        catalog::{Model, Quote},
        telemetry::hooks::Lease,
    };
    let quote = Quote {
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
    };
    assert_eq!(
        state
            .accounting
            .reserve(
                "synthetic",
                [1; 32],
                [99; 32],
                quote,
                std::time::Instant::now() + Duration::from_secs(60),
            )
            .unwrap(),
        ReserveResult::Reserved
    );
    let lease = || {
        Arc::new(tokio::sync::Semaphore::new(1))
            .try_acquire_owned()
            .unwrap()
    };
    let owner = crate::generation_owner::ReservedGeneration::new(
        state.accounting.clone(),
        [1; 32],
        lease(),
        Lease::from(lease()),
    )
    .tracked(state.lifecycle.try_admit().unwrap());
    let (release, held) = tokio::sync::oneshot::channel();
    let completion = owner.spawn_settling((), |(), settlement| async move {
        held.await.unwrap();
        settlement.finish(&Ok(crate::inference::stream::StreamUsage {
            input_tokens: 5,
            output_tokens: 4,
            total_tokens: 9,
        }))
    });
    (release, completion)
}

async fn reporter_fault_drains_original_owner(source: Option<Failure>) {
    let state = state();
    let (release, completion) = held_original_generation(&state);
    state.lifecycle.accounting_failed(); // Independent shared drain failure.
    let report = panicking_reporter();
    let original = source.unwrap_or(Failure::RendezvousAccept);
    let peer_report = report.clone();
    let retained = Arc::new(AtomicUsize::new(0));
    let guard = Retained(retained.clone());
    let (fail, triggered) = tokio::sync::oneshot::channel();
    let onion = stream::once(async move {
        triggered.await.unwrap();
        if let Some(source) = source {
            Some(Err::<BoxIo, _>(source))
        } else {
            assert!(peer_result(
                async { Err::<(), ()>(()) },
                Failure::RendezvousAccept,
                &peer_report,
            )
            .await
            .is_none());
            None
        }
    })
    .filter_map(std::future::ready)
    .chain(stream::pending())
    .inspect(move |_| {
        let _ = &guard;
    });
    // Reuse the actual duplex/router path; one-byte delivery backpressure holds
    // an original Hyper connection independently of the settlement owner.
    let (mut peer, io) = tokio::io::duplex(1);
    let tcp = stream::once(async move { Ok(Box::new(io) as BoxIo) }).chain(stream::pending());
    let serve = tokio::spawn(serve_sources_until(
        tcp,
        onion,
        state.clone(),
        Duration::from_secs(10),
        std::future::pending(),
        report,
    ));
    bounded(peer.write_all(b"GET /healthz HTTP/1.1\r\nHost: synthetic\r\n\r\n"))
        .await
        .unwrap();
    let mut first = [0];
    bounded(peer.read_exact(&mut first)).await.unwrap();
    assert_eq!(&first, b"H");
    fail.send(()).unwrap();
    bounded(async {
        while state.lifecycle.is_serving() {
            assert!(!serve.is_finished());
            tokio::task::yield_now().await;
        }
    })
    .await;
    assert!(state.lifecycle.try_admit().is_none());
    assert!(!serve.is_finished());
    assert_eq!(retained.load(Ordering::SeqCst), 0);
    assert_eq!(state.accounting.snapshot("synthetic").unwrap().in_flight, 1);
    release.send(()).unwrap();
    assert_eq!(
        bounded(completion).await.unwrap(),
        Ok(crate::accounting::Outcome::Settled { charged: 29 })
    );
    assert!(!serve.is_finished()); // Original Hyper delivery still backpressured.
    let mut response = first.to_vec();
    bounded(peer.read_to_end(&mut response)).await.unwrap();
    assert!(response.starts_with(b"HTTP/1.1 204"));
    let failure = bounded(serve).await.unwrap().unwrap_err();
    assert_eq!(
        failure,
        ServeFailure {
            source,
            shared: Some(ServerFailure::ShutdownIntegrity),
            reporting: Some(ReportingFailure { original }),
        }
    );
    assert!(!failure.to_string().contains("synthetic reporter fault"));
    let balance = state.accounting.snapshot("synthetic").unwrap();
    assert_eq!(
        (
            balance.in_flight,
            balance.completed_requests,
            balance.available_microunits
        ),
        (0, 1, 71)
    );
    assert_eq!(retained.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn source_reporter_panic_preserves_primary_and_drains_original_settlement_once() {
    reporter_fault_drains_original_owner(Some(Failure::SourceUnknown)).await;
}

#[tokio::test]
async fn peer_reporter_panic_stops_admission_without_fabricating_root_failure() {
    reporter_fault_drains_original_owner(None).await;
}

#[tokio::test]
async fn identity_reporter_panic_never_launches_and_launch_fault_stays_separate() {
    let scratch = scratch();
    let manager = open(scratch.path());
    let approved = provision(&manager, &"synthetic-other".parse().unwrap(), [8; 32]);
    let identity = require_identity(&manager, &"synthetic-missing".parse().unwrap(), &approved);
    assert_eq!(identity, Err(Failure::IdentityMissing));
    let report = panicking_reporter();
    let mut launched = false;
    let shutdown = std::future::pending();
    tokio::pin!(shutdown);
    let result = launch_checked(
        || identity,
        || {
            launched = true;
            Ok(())
        },
        shutdown.as_mut(),
        &report,
        || async { panic!("startup handoff must not run after identity failure") },
    )
    .await;
    assert!(!launched);
    assert_eq!(
        result,
        Err(ServeFailure {
            source: Some(Failure::IdentityMissing),
            shared: None,
            reporting: Some(ReportingFailure {
                original: Failure::IdentityMissing
            }),
        })
    );
    assert_eq!(
        launch_checked::<(), _>(
            || Ok(()),
            || Err(Failure::Launch),
            shutdown.as_mut(),
            &panicking_reporter(),
            || async { panic!("startup handoff must not run after launch failure") },
        )
        .await,
        Err(ServeFailure {
            source: Some(Failure::Launch),
            shared: None,
            reporting: Some(ReportingFailure {
                original: Failure::Launch
            }),
        })
    );
}

#[tokio::test]
async fn shutdown_ready_during_identity_read_never_runs_launch() {
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let shutdown = async { stopped.await.unwrap() };
    tokio::pin!(shutdown);
    let (report, seen) = reporter();
    let mut launched = false;
    assert_eq!(
        launch_checked(
            || {
                stop.send(()).unwrap();
                Ok(())
            },
            || {
                launched = true;
                Ok(())
            },
            shutdown.as_mut(),
            &report,
            || async { panic!("startup handoff must not run after interruption") },
        )
        .await,
        Ok(None)
    );
    assert!(!launched);
    assert!(seen.lock().unwrap().is_empty());

    // An already observed identity/reporting failure is not erased by shutdown.
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let shutdown = async { stopped.await.unwrap() };
    tokio::pin!(shutdown);
    assert_eq!(
        launch_checked::<(), _>(
            || {
                stop.send(()).unwrap();
                Err(Failure::IdentityMismatch)
            },
            || panic!("launch must not run"),
            shutdown.as_mut(),
            &panicking_reporter(),
            || async { panic!("startup handoff must not erase identity failure") },
        )
        .await,
        Err(ServeFailure {
            source: Some(Failure::IdentityMismatch),
            shared: None,
            reporting: Some(ReportingFailure {
                original: Failure::IdentityMismatch,
            }),
        })
    );
    let shutdown = std::future::ready(());
    tokio::pin!(shutdown);
    assert_eq!(
        launch_checked::<(), _>(
            || panic!("identity must not run after ready shutdown"),
            || panic!("launch must not run"),
            shutdown.as_mut(),
            &report,
            || async { panic!("startup handoff must not run after ready shutdown") },
        )
        .await,
        Ok(None)
    );
}

#[tokio::test]
async fn initialization_handoff_runs_once_after_successful_local_launch() {
    let events = std::sync::Mutex::new(Vec::new());
    let shutdown = std::future::pending();
    tokio::pin!(shutdown);
    let (report, seen) = reporter();
    let result = launch_checked(
        || {
            events.lock().unwrap().push("identity");
            Ok(())
        },
        || {
            events.lock().unwrap().push("launch");
            Ok(42)
        },
        shutdown.as_mut(),
        &report,
        || async {
            tokio::task::yield_now().await;
            events.lock().unwrap().push("initialization_complete");
        },
    )
    .await;
    assert_eq!(result, Ok(Some(42)));
    assert_eq!(
        *events.lock().unwrap(),
        ["identity", "launch", "initialization_complete"]
    );
    assert!(seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn ready_shutdown_does_not_poll_either_source_or_report_death() {
    let polled = Arc::new(AtomicUsize::new(0));
    let make = || {
        let polled = polled.clone();
        stream::poll_fn(move |_| {
            polled.fetch_add(1, Ordering::SeqCst);
            std::task::Poll::Ready(Some(Err::<BoxIo, _>(Failure::Engine)))
        })
    };
    let (report, seen) = reporter();
    assert!(serve_sources_until(
        make(),
        make(),
        state(),
        Duration::from_secs(10),
        std::future::ready(()),
        report
    )
    .await
    .is_ok());
    assert_eq!(polled.load(Ordering::SeqCst), 0);
    assert!(seen.lock().unwrap().is_empty());
    let report = panicking_reporter();
    report.report(Failure::PortRefused);
    assert_eq!(
        serve_sources_until(
            make(),
            make(),
            state(),
            Duration::from_secs(10),
            std::future::ready(()),
            report,
        )
        .await,
        Err(ServeFailure {
            source: None,
            shared: None,
            reporting: Some(ReportingFailure {
                original: Failure::PortRefused
            }),
        }),
    );
    assert_eq!(polled.load(Ordering::SeqCst), 0);
}

fn scratch() -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt;
    tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap()
}

fn open(path: &Path) -> KeyMgr {
    KeyMgrBuilder::default()
        .primary_store(Box::new(
            ArtiNativeKeystore::from_path_and_mistrust(
                path,
                &fs_mistrust::Mistrust::builder()
                    .ignore_environment()
                    // Assume only synthetic ancestors; the key directory/files stay checked.
                    .ignore_prefix(path.parent().expect("fixture parent"))
                    .build()
                    .unwrap(),
            )
            .unwrap(),
        ))
        .build()
        .unwrap()
}

// Intentionally public deterministic synthetic seeds, never runtime key material.
fn provision(manager: &KeyMgr, name: &HsNickname, seed: [u8; 32]) -> HsId {
    let pair = tor_llcrypto::pk::ed25519::Keypair::from(&seed);
    let pair = HsIdKeypair::from(tor_llcrypto::pk::ed25519::ExpandedKeypair::from(&pair));
    let approved = HsIdKey::from(&pair).id();
    manager
        .insert(
            pair,
            &HsIdKeypairSpecifier::new(name.clone()),
            KeystoreSelector::Primary,
            false,
        )
        .unwrap();
    approved
}

#[test]
fn native_identity_reopens_without_generation_or_launch() {
    let scratch = scratch();
    let name = "synthetic-adapter".parse().unwrap();
    let approved = {
        let manager = open(scratch.path());
        let approved = provision(&manager, &name, [7; 32]);
        assert_eq!(require_identity(&manager, &name, &approved), Ok(()));
        approved
    };
    assert_eq!(
        require_identity(&open(scratch.path()), &name, &approved),
        Ok(())
    );
}

#[test]
fn missing_pair_is_not_generated_and_wrong_approval_fails_closed() {
    let scratch = scratch();
    let manager = open(scratch.path());
    let name: HsNickname = "synthetic-adapter".parse().unwrap();
    let other: HsNickname = "synthetic-other".parse().unwrap();
    let approved = provision(&manager, &name, [7; 32]);
    let wrong = provision(&manager, &other, [8; 32]);
    assert_eq!(
        require_identity(&manager, &name, &wrong),
        Err(Failure::IdentityMismatch)
    );
    let public = manager
        .get::<HsIdKey>(&HsIdPublicKeySpecifier::new(name.clone()))
        .unwrap()
        .unwrap();
    manager
        .remove::<HsIdKeypair>(
            &HsIdKeypairSpecifier::new(name.clone()),
            KeystoreSelector::Primary,
        )
        .unwrap();
    manager
        .insert(
            public,
            &HsIdPublicKeySpecifier::new(name.clone()),
            KeystoreSelector::Primary,
            false,
        )
        .unwrap();
    assert_eq!(
        require_identity(&manager, &name, &approved),
        Err(Failure::IdentityMissing)
    );
    assert!(manager
        .get::<HsIdKeypair>(&HsIdKeypairSpecifier::new(name))
        .unwrap()
        .is_none());
}

#[test]
fn stored_public_private_disagreement_fails_closed() {
    let scratch = scratch();
    let manager = open(scratch.path());
    let name: HsNickname = "synthetic-adapter".parse().unwrap();
    let other: HsNickname = "synthetic-other".parse().unwrap();
    let approved = provision(&manager, &name, [7; 32]);
    provision(&manager, &other, [8; 32]);
    let public = manager
        .get::<HsIdKey>(&HsIdPublicKeySpecifier::new(other))
        .unwrap()
        .unwrap();
    manager
        .insert(
            public,
            &HsIdPublicKeySpecifier::new(name.clone()),
            KeystoreSelector::Primary,
            true,
        )
        .unwrap();
    assert_eq!(
        require_identity(&manager, &name, &approved),
        Err(Failure::IdentityMismatch)
    );
}
