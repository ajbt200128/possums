use super::*;
use crate::telemetry::startup::tests::{capture, expected_resource, text};
use futures_util::FutureExt;
use std::sync::atomic::Ordering::SeqCst;
use tokio::io::AsyncWriteExt;

fn off() -> Config {
    Config::from_lookup(|_| None)
}
fn fixture() -> (Observer, watch::Receiver<Snapshot>) {
    let (observer, unused) = new(off());
    drop(unused);
    let receiver = observer.snapshot.subscribe();
    (observer, receiver)
}
fn stalled() -> Client {
    let mut client = Client::new("127.0.0.1:1".parse().unwrap()).unwrap();
    client.stall_connect = true;
    client
}

// Integration fixture uses the real bounded sender's pending-connect path.
// No network or production configuration, and no alternate production dialer.
pub(crate) fn stalled_observer() -> (Observer, impl Future<Output = ()>) {
    let (observer, receiver) = fixture();
    let worker = run(observer.start, receiver, Ok(Some(stalled())));
    (observer, worker)
}

#[tokio::test(start_paused = true)]
async fn ready_before_waiting_is_terminal_once_and_duplicates_are_inert() {
    let (observer, receiver) = fixture();
    observer.observe(
        OnionState::Running,
        ProblemKind::None,
        Some(Terminal::FirstLocalReady),
    );
    let first = receiver.borrow().first.unwrap();
    assert_eq!(first.elapsed, "under_30s");
    for _ in 0..100 {
        observer.observe(
            OnionState::Broken,
            ProblemKind::Runtime,
            Some(Terminal::EngineFailure),
        );
        observer.failure(Terminal::StatusEnded);
    }
    assert_eq!(receiver.borrow().first, Some(first));
    let client = stalled();
    let evidence = client.evidence();
    let worker = run(observer.start, receiver, Ok(Some(client)));
    tokio::pin!(worker);
    assert!(worker.as_mut().now_or_never().is_none());
    assert_eq!(evidence.connecting.load(SeqCst), 1);
    tokio::time::advance(Duration::from_secs(1)).await;
    worker.await;
    assert_eq!(evidence.connecting.load(SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn waiting_export_stall_cannot_erase_first_ready_or_failure() {
    for terminal in [
        Terminal::FirstLocalReady,
        Terminal::EngineFailure,
        Terminal::StatusEnded,
        Terminal::StatusPanicked,
    ] {
        let (observer, receiver) = fixture();
        let client = stalled();
        let evidence = client.evidence();
        let worker = run(observer.start, receiver.clone(), Ok(Some(client)));
        tokio::pin!(worker);
        assert!(worker.as_mut().now_or_never().is_none());
        tokio::time::advance(WAIT).await;
        assert!(worker.as_mut().now_or_never().is_none());
        assert_eq!(evidence.connecting.load(SeqCst), 1);
        observer.observe(OnionState::Running, ProblemKind::None, Some(terminal));
        let first = receiver.borrow().first;
        observer.observe(
            OnionState::Broken,
            ProblemKind::Runtime,
            Some(Terminal::EngineFailure),
        );
        observer.failure(Terminal::StatusPanicked);
        assert_eq!(receiver.borrow().first, first);
        assert_eq!(receiver.borrow().first.unwrap().terminal, Some(terminal));
        assert_eq!(receiver.borrow().first.unwrap().elapsed, "30s_to_under_60s");
        tokio::time::advance(Duration::from_secs(1)).await;
        assert!(worker.as_mut().now_or_never().is_none());
        assert_eq!(evidence.connecting.load(SeqCst), 2);
        tokio::time::advance(Duration::from_secs(1)).await;
        worker.await;
        assert_eq!(evidence.connecting.load(SeqCst), 2);
    }
}

#[tokio::test(start_paused = true)]
async fn never_ready_deadline_only_ends_observer_and_future_status_is_inert() {
    let (observer, receiver) = fixture();
    let client = stalled();
    let evidence = client.evidence();
    let worker = run(observer.start, receiver.clone(), Ok(Some(client)));
    tokio::pin!(worker);
    assert!(worker.as_mut().now_or_never().is_none());
    observer.observe(OnionState::Bootstrapping, ProblemKind::Ipt, None);
    tokio::time::advance(WAIT).await;
    assert!(worker.as_mut().now_or_never().is_none());
    assert_eq!(evidence.connecting.load(SeqCst), 1);
    tokio::time::advance(Duration::from_secs(1)).await;
    assert!(worker.as_mut().now_or_never().is_none());
    tokio::time::advance(Duration::from_secs(149)).await;
    assert!(worker.as_mut().now_or_never().is_none());
    assert_eq!(evidence.connecting.load(SeqCst), 2);
    tokio::time::advance(Duration::from_secs(1)).await;
    worker.await;
    // Deadline has no readiness reference and never fabricates an engine fault.
    assert!(receiver.borrow().first.is_none());
    observer.observe(
        OnionState::Running,
        ProblemKind::None,
        Some(Terminal::FirstLocalReady),
    );
    assert!(receiver.borrow().first.is_none());
    assert_eq!(receiver.borrow().latest.state, State::Bootstrapping);
    tokio::time::advance(Duration::from_secs(1000)).await;
    assert_eq!(evidence.connecting.load(SeqCst), 2);
}

#[tokio::test(start_paused = true)]
async fn monotonic_bucket_boundaries_and_off_path() {
    let start = Instant::now();
    for (advance, expected) in [
        (0, "under_30s"),
        (29, "under_30s"),
        (1, "30s_to_under_60s"),
        (29, "30s_to_under_60s"),
        (1, "60s_to_under_180s"),
        (119, "60s_to_under_180s"),
        (1, "180s_or_more"),
    ] {
        tokio::time::advance(Duration::from_secs(advance)).await;
        assert_eq!(elapsed(start), expected);
    }
    let config = Config::from_lookup(|key| {
        assert_eq!(key, "OTEL_SDK_DISABLED", "off must not read secrets/config");
        Some("true".into())
    });
    assert!(config.into_client().unwrap().is_none());
    let (observer, worker) = new(off());
    observer.observe(
        OnionState::Running,
        ProblemKind::None,
        Some(Terminal::FirstLocalReady),
    );
    // Off returns without encoding, TLS construction or dialing.
    assert!(worker.now_or_never().is_some());
}

#[test]
fn typed_projection_never_formats_sdk_inner_errors_and_unknown_is_closed() {
    const HOSTILE: &str = "hostile-secret-url-account-onion-panic";
    let problems = [
        (
            Problem::Runtime(tor_hsservice::FatalError::MissingHsIdKeypair(
                HOSTILE.parse().unwrap(),
            )),
            ProblemKind::Runtime,
        ),
        (
            Problem::DescriptorUpload(vec![]),
            ProblemKind::DescriptorUpload,
        ),
        (Problem::Ipt(vec![]), ProblemKind::Ipt),
    ];
    for (problem, expected) in &problems {
        let projected = ProblemKind::from_sdk(Some(problem));
        assert_eq!(projected, *expected);
        let record = Record {
            state: State::Broken,
            problem: projected,
            elapsed: "under_30s",
            terminal: Some(Terminal::EngineFailure),
        };
        assert!(!String::from_utf8_lossy(&logs(record).encode_to_vec()).contains(HOSTILE));
    }
    assert_eq!(ProblemKind::from_sdk(None), ProblemKind::None);
    // Rust cannot construct a future non_exhaustive SDK variant; exercise its
    // closed fallback output explicitly. Pow is absent from the pinned features.
    for problem in [
        ProblemKind::None,
        ProblemKind::Runtime,
        ProblemKind::DescriptorUpload,
        ProblemKind::Ipt,
        ProblemKind::Unknown,
    ] {
        for state in [
            State::NotObserved,
            State::Shutdown,
            State::Bootstrapping,
            State::DegradedReachable,
            State::DegradedUnreachable,
            State::Running,
            State::Recovering,
            State::Broken,
            State::Unknown,
        ] {
            let record = Record {
                state,
                problem,
                elapsed: "under_30s",
                terminal: Some(Terminal::EngineFailure),
            };
            let bytes = logs(record).encode_to_vec();
            assert!(!String::from_utf8_lossy(&bytes).contains(HOSTILE));
            let message = ExportLogsServiceRequest::decode(bytes.as_slice()).unwrap();
            assert_eq!(message.resource_logs[0].resource, Some(expected_resource()));
            let scope_logs = &message.resource_logs[0].scope_logs[0];
            assert_eq!(scope_logs.scope, Some(scope()));
            assert_eq!(scope_logs.schema_url, "");
            assert_eq!(scope_logs.log_records.len(), 1);
            let log = &scope_logs.log_records[0];
            assert_eq!(text(log.body.as_ref().unwrap()), record.to_string());
            let attrs: Vec<_> = log
                .attributes
                .iter()
                .map(|a| (a.key.as_str(), text(a.value.as_ref().unwrap())))
                .collect();
            assert_eq!(
                attrs,
                vec![
                    ("outcome", "engine_failure"),
                    ("code", "tor_startup_engine_failure"),
                    ("onion_state", state.label()),
                    ("problem_kind", problem.label()),
                    ("elapsed_bucket", "under_30s"),
                    ("constraint", "observed_engine_failure_cause_unknown"),
                    ("action", "review_startup_evidence_offline"),
                ]
            );
            assert_eq!(
                *log,
                LogRecord {
                    event_name: EVENT.into(),
                    severity_number: SeverityNumber::Info as i32,
                    severity_text: "INFO".into(),
                    body: log.body.clone(),
                    attributes: log.attributes.clone(),
                    ..Default::default()
                }
            );
            assert_eq!(message.resource_logs[0].schema_url, "");
            assert_eq!(bytes, message.encode_to_vec());
        }
    }
}

#[tokio::test]
async fn actual_tls_otlp_wire_waiting_then_latched_ready_has_only_sealed_logs() {
    let (listener, client, acceptor) =
        crate::telemetry::runtime::tests::fixture(true, "api.honeycomb.io").await;
    let evidence = client.evidence();
    let (mut observer, receiver) = fixture();
    observer.start -= WAIT; // synthetic clock position, no real 30-second wait
    observer.observe(
        OnionState::Bootstrapping,
        ProblemKind::DescriptorUpload,
        None,
    );
    let peer = async {
        let mut bodies = Vec::new();
        for index in 0..2 {
            let (io, _) = listener.accept().await.unwrap();
            let mut io = acceptor.accept(io).await.unwrap();
            bodies.push(capture(&mut io, "/v1/logs").await);
            if index == 0 {
                observer.observe(
                    OnionState::Running,
                    ProblemKind::None,
                    Some(Terminal::FirstLocalReady),
                );
                observer.observe(
                    OnionState::Broken,
                    ProblemKind::Runtime,
                    Some(Terminal::EngineFailure),
                );
            }
            io.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
        }
        bodies
    };
    let (_, bodies) = tokio::join!(run(observer.start, receiver, Ok(Some(client))), peer);
    assert_eq!(evidence.connections.load(SeqCst), 2);
    assert_eq!(evidence.live_io.load(SeqCst), 0);
    for (bytes, outcome) in bodies.iter().zip(["waiting", "first_local_ready"]) {
        let message = ExportLogsServiceRequest::decode(bytes.as_slice()).unwrap();
        let log = &message.resource_logs[0].scope_logs[0].log_records[0];
        assert!(text(log.body.as_ref().unwrap()).contains(&format!("outcome={outcome}")));
        assert_eq!(log.attributes.len(), 7);
        assert!(log.trace_id.is_empty() && log.span_id.is_empty());
        assert!(!String::from_utf8_lossy(bytes).contains("synthetic-key-canary"));
    }
}

#[tokio::test]
async fn dropping_worker_closes_actual_io_without_late_upload_or_retry() {
    let (listener, client, acceptor) =
        crate::telemetry::runtime::tests::fixture(true, "api.honeycomb.io").await;
    let evidence = client.evidence();
    let (observer, receiver) = fixture();
    observer.observe(
        OnionState::Running,
        ProblemKind::None,
        Some(Terminal::FirstLocalReady),
    );
    let mut worker = Box::pin(run(observer.start, receiver, Ok(Some(client))));
    let peer = async {
        let (io, _) = listener.accept().await.unwrap();
        let mut io = acceptor.accept(io).await.unwrap();
        capture(&mut io, "/v1/logs").await;
        io
    };
    let io = tokio::select! {
        _ = &mut worker => panic!("peer withholds response"),
        io = peer => io,
    };
    assert_eq!(evidence.live_io.load(SeqCst), 1);
    drop(worker);
    assert_eq!(evidence.live_io.load(SeqCst), 0);
    let writes = evidence.write_polls.load(SeqCst);
    tokio::task::yield_now().await;
    assert_eq!(evidence.write_polls.load(SeqCst), writes);
    assert_eq!(evidence.connections.load(SeqCst), 1);
    drop(io);
}
