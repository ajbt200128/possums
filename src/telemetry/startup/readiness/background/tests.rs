use super::*;
use crate::telemetry::startup::{
    tests::{capture, expected_resource, text},
    ExportLogsServiceResponse,
};
use futures_util::FutureExt;
use std::sync::atomic::Ordering::SeqCst;
use tokio::io::AsyncWriteExt;

const HOSTILE: &str = "synthetic-secret-path-account-onion";

fn fixture() -> (Observer, watch::Receiver<Snapshot>) {
    let (snapshot, receiver) = watch::channel(Snapshot::initial());
    (Observer { snapshot }, receiver)
}
fn observed(observer: &Observer, state: State) {
    observer.update(|current| {
        current.source = Source::Observed;
        current.onion = state;
        current.fully_reachable = matches!(state, State::Running | State::DegradedReachable);
    });
}
fn stalled() -> Client {
    super::super::tests::stalled()
}
fn bug() -> tor_keymgr::key_specifier_derive::Bug {
    tor_keymgr::key_specifier_derive::Bug::new(arti_client::ErrorKind::Internal, HOSTILE)
}
fn wire(record: Snapshot) -> LogRecord {
    let bytes = logs(record).encode_to_vec();
    assert!(!String::from_utf8_lossy(&bytes).contains(HOSTILE));
    let message = ExportLogsServiceRequest::decode(bytes.as_slice()).unwrap();
    assert_eq!(message.resource_logs.len(), 1);
    let resource = &message.resource_logs[0];
    assert_eq!(resource.resource, Some(expected_resource()));
    assert!(resource.schema_url.is_empty());
    assert_eq!(resource.scope_logs.len(), 1);
    let scope_logs = &resource.scope_logs[0];
    assert_eq!(scope_logs.scope, Some(scope()));
    assert!(scope_logs.schema_url.is_empty());
    assert_eq!(scope_logs.log_records.len(), 1);
    scope_logs.log_records[0].clone()
}

#[test]
fn actual_ipt_variants_empty_and_mixed_vectors_discard_hostile_messages() {
    for (error, expected) in [
        (IptError::IntroPointNotListed, IPT_NOT_LISTED),
        (IptError::BadEstablished, IPT_BAD_ESTABLISHED),
        (IptError::BadMessage(HOSTILE.into()), IPT_BAD_MESSAGE),
        (IptError::Timeout, IPT_TIMEOUT),
    ] {
        let problem = Problem::Ipt(vec![error]);
        let selected = SelectedProblem::from_sdk(Some(&problem));
        assert_eq!(selected.kind, ProblemKind::Ipt);
        assert_eq!(selected.flags, expected);
        let record = Snapshot {
            problem: selected,
            ..Snapshot::initial()
        };
        assert_eq!(wire(record).severity_number, SeverityNumber::Error as i32);
        assert!(!record.to_string().contains(HOSTILE));
    }
    let mixed = Problem::Ipt(vec![
        IptError::Timeout,
        IptError::BadMessage(HOSTILE.repeat(1000)),
        IptError::IntroPointNotListed,
        IptError::BadEstablished,
        IptError::Timeout,
    ]);
    assert_eq!(
        SelectedProblem::from_sdk(Some(&mixed)).flags,
        IPT_TIMEOUT | IPT_BAD_MESSAGE | IPT_NOT_LISTED | IPT_BAD_ESTABLISHED
    );
    for (problem, expected) in [
        (Problem::Ipt(vec![]), IPT_UNKNOWN),
        (Problem::DescriptorUpload(vec![]), UPLOAD_UNKNOWN),
    ] {
        let selected = SelectedProblem::from_sdk(Some(&problem));
        assert_eq!(selected.flags, expected);
        assert_eq!(
            wire(Snapshot {
                problem: selected,
                ..Snapshot::initial()
            })
            .severity_number,
            SeverityNumber::Error as i32
        );
    }
    assert_eq!(SelectedProblem::from_sdk(None).flags, 0);
}

#[test]
fn actual_runtime_variants_discard_paths_actions_nicknames_and_bug_text() {
    let path: tor_keymgr::ArtiPath = HOSTILE.parse().unwrap();
    for (error, expected) in [
        (
            FatalError::Spawn {
                spawning: HOSTILE,
                cause: std::sync::Arc::new(futures_util::task::SpawnError::shutdown()),
            },
            Runtime::Spawn,
        ),
        (
            FatalError::Keystore(tor_keymgr::Error::KeyAlreadyExists),
            Runtime::Keystore,
        ),
        (
            FatalError::KeystoreRace {
                action: HOSTILE,
                path: path.clone(),
            },
            Runtime::KeystoreRace,
        ),
        (
            FatalError::MissingHsIdKeypair(HOSTILE.parse().unwrap()),
            Runtime::MissingIdentity,
        ),
        (
            FatalError::IptKeysFoundUnexpectedly(path),
            Runtime::UnexpectedIptKeys,
        ),
        (
            FatalError::MissingField(HOSTILE.into()),
            Runtime::MissingField,
        ),
        (FatalError::HsDescTooLong, Runtime::DescriptorTooLong),
        (FatalError::Bug(bug()), Runtime::Bug),
    ] {
        let problem = Problem::Runtime(error);
        let selected = SelectedProblem::from_sdk(Some(&problem));
        assert_eq!(selected.runtime, expected);
        assert_eq!(selected.kind, ProblemKind::Runtime);
        let log = wire(Snapshot {
            problem: selected,
            ..Snapshot::initial()
        });
        assert!(!text(log.body.as_ref().unwrap()).contains(HOSTILE));
    }
}

#[test]
fn actual_descriptor_bug_and_available_inner_variants_use_closed_bits() {
    let problem = Problem::DescriptorUpload(vec![
        DescUploadRetryError::Bug(bug()),
        DescUploadRetryError::Bug(bug()),
    ]);
    assert_eq!(SelectedProblem::from_sdk(Some(&problem)).flags, UPLOAD_BUG);
    for (error, expected) in [
        (DescUploadError::Circuit(bug().into()), DESC_CIRCUIT),
        (DescUploadError::Stream(bug().into()), DESC_STREAM),
        (DescUploadError::Bug(bug()), DESC_BUG),
    ] {
        assert_eq!(descriptor_flag(&error), expected);
    }
    wire(Snapshot {
        problem: SelectedProblem::from_sdk(Some(&problem)),
        ..Snapshot::initial()
    });
    // RequestFailedError lacks a constructor reexport in permitted direct
    // dependencies. NetdirProviderShutdown is a non-exhaustive unit struct with
    // no Default. Future non-exhaustive variants cannot be constructed either;
    // keep those source-reviewed gaps explicit rather than faking SDK values.
}

#[test]
fn actual_retry_vectors_cover_outer_variants_nested_sources_and_empty_errors() {
    // Parent supplies exact existing retry-error 0.14.0 as a dev dependency;
    // the SDK's nested type has no Default/FromIterator or constructor reexport.
    fn causes() -> retry_error::RetryError<DescUploadError> {
        let mut causes = retry_error::RetryError::in_attempt_to(HOSTILE);
        causes.push(DescUploadError::Circuit(bug().into()));
        causes.push(DescUploadError::Stream(bug().into()));
        causes.push(DescUploadError::Bug(bug()));
        causes.push(DescUploadError::Bug(bug()));
        causes
    }
    for (error, outer) in [
        (DescUploadRetryError::FatalError(causes()), UPLOAD_FATAL),
        (
            DescUploadRetryError::MaxRetryCountExceeded(causes()),
            UPLOAD_MAX_RETRY,
        ),
        (DescUploadRetryError::Timeout(causes()), UPLOAD_TIMEOUT),
    ] {
        let problem = Problem::DescriptorUpload(vec![error]);
        let selected = SelectedProblem::from_sdk(Some(&problem));
        assert_eq!(
            selected.flags,
            outer | DESC_CIRCUIT | DESC_STREAM | DESC_BUG
        );
        wire(Snapshot {
            problem: selected,
            ..Snapshot::initial()
        });
    }
    let problem = Problem::DescriptorUpload(vec![
        DescUploadRetryError::FatalError(causes()),
        DescUploadRetryError::MaxRetryCountExceeded(causes()),
        DescUploadRetryError::Timeout(causes()),
        DescUploadRetryError::Bug(bug()),
        DescUploadRetryError::FatalError(retry_error::RetryError::in_attempt_to(HOSTILE)),
        DescUploadRetryError::MaxRetryCountExceeded(retry_error::RetryError::in_attempt_to(
            HOSTILE,
        )),
        DescUploadRetryError::Timeout(retry_error::RetryError::in_attempt_to(HOSTILE)),
    ]);
    let selected = SelectedProblem::from_sdk(Some(&problem));
    assert_eq!(
        selected.flags,
        UPLOAD_FATAL
            | UPLOAD_MAX_RETRY
            | UPLOAD_TIMEOUT
            | UPLOAD_BUG
            | DESC_CIRCUIT
            | DESC_STREAM
            | DESC_BUG
            | DESC_UNKNOWN
    );
    wire(Snapshot {
        problem: selected,
        ..Snapshot::initial()
    });
    for error in [
        DescUploadRetryError::FatalError(retry_error::RetryError::in_attempt_to(HOSTILE)),
        DescUploadRetryError::MaxRetryCountExceeded(retry_error::RetryError::in_attempt_to(
            HOSTILE,
        )),
        DescUploadRetryError::Timeout(retry_error::RetryError::in_attempt_to(HOSTILE)),
    ] {
        let selected = SelectedProblem::from_sdk(Some(&Problem::DescriptorUpload(vec![error])));
        assert_ne!(selected.flags & DESC_UNKNOWN, 0);
        assert_ne!(
            wire(Snapshot {
                problem: selected,
                ..Snapshot::initial()
            })
            .severity_number,
            SeverityNumber::Info as i32
        );
    }
}

#[test]
fn bootstrap_quarters_typed_blockages_and_default_sdk_status_are_not_health_claims() {
    for (value, expected) in [
        (-0.1, Fraction::Unknown),
        (f32::NAN, Fraction::Unknown),
        (f32::INFINITY, Fraction::Unknown),
        (1.1, Fraction::Unknown),
        (0.0, Fraction::UnderQuarter),
        (0.249, Fraction::UnderQuarter),
        (0.25, Fraction::QuarterToHalf),
        (0.499, Fraction::QuarterToHalf),
        (0.5, Fraction::HalfToThreeQuarters),
        (0.749, Fraction::HalfToThreeQuarters),
        (0.75, Fraction::ThreeQuartersToComplete),
        (0.999, Fraction::ThreeQuartersToComplete),
        (1.0, Fraction::Complete),
    ] {
        assert_eq!(Fraction::from_sdk(value), expected);
    }
    use arti_client::status::BlockageKind as K;
    for (sdk, expected) in [
        (K::Disabled, Blockage::Disabled),
        (K::Offline, Blockage::Offline),
        (K::Filtering, Blockage::Filtering),
        (K::CantReachTor, Blockage::CannotReachTor),
        (K::ClockSkewed, Blockage::ClockSkewed),
        (K::CantBootstrap, Blockage::CannotBootstrap),
    ] {
        assert_eq!(Blockage::from_sdk(sdk), expected);
        assert_eq!(
            wire(Snapshot {
                blockage: expected,
                ..Snapshot::initial()
            })
            .severity_number,
            if matches!(expected, Blockage::Disabled) {
                SeverityNumber::Info as i32
            } else {
                SeverityNumber::Error as i32
            }
        );
    }
    let (observer, receiver) = fixture();
    observer.bootstrap(&arti_client::status::BootstrapStatus::default());
    let current = *receiver.borrow();
    assert!(current.bootstrap_observed);
    assert_eq!(current.bootstrap, Fraction::UnderQuarter);
    assert!(!current.bootstrap_ready);
    assert_eq!(current.source, Source::NotObserved);
    assert!(current
        .to_string()
        .contains("not_current_reachability_proof"));
    assert!(!current.to_string().contains("healthy"));
}

#[test]
fn sealed_actual_protobuf_has_exact_allowlist_and_only_default_non_log_metadata() {
    // Encoder API itself has no string-bearing input or optional enrichment.
    let _: fn(Snapshot) -> ExportLogsServiceRequest = logs;
    fn copy_only<T: Copy>() {}
    copy_only::<Snapshot>();
    assert!(std::mem::size_of::<Snapshot>() <= 32);
    let mut record = Snapshot::initial();
    record.problem.flags = FLAGS.iter().fold(0, |all, (_, bit)| all | bit);
    record.problem.kind = ProblemKind::Unknown;
    record.problem.runtime = Runtime::Unknown;
    record.onion = State::Unknown;
    record.source = Source::Observed;
    let log = wire(record);
    let attrs: Vec<_> = log
        .attributes
        .iter()
        .map(|a| (a.key.as_str(), text(a.value.as_ref().unwrap())))
        .collect();
    let mut expected = vec![
        ("code", "tor_background_problem"),
        ("stage", "tor_background"),
        ("diagnostic_profile", "debug_tor_readiness"),
        (
            "constraint",
            "selected_sdk_snapshot_not_current_reachability_proof",
        ),
        ("action", "report_safe_tor_code_and_category"),
        ("onion_state", "unknown"),
        ("sdk_fully_reachable", "false"),
        ("problem_kind", "unknown"),
        ("runtime_kind", "unknown"),
        ("source_status", "observed"),
        ("bootstrap_fraction", "unknown"),
        ("bootstrap_ready", "unknown"),
        ("blockage", "unknown"),
    ];
    expected.extend(FLAGS.iter().map(|(key, _)| (*key, "observed")));
    assert_eq!(attrs, expected);
    assert_eq!(
        log,
        LogRecord {
            event_name: EVENT.into(),
            severity_number: SeverityNumber::Error as i32,
            severity_text: "ERROR".into(),
            body: log.body.clone(),
            attributes: log.attributes.clone(),
            ..Default::default()
        }
    );
    assert_eq!(text(log.body.as_ref().unwrap()), record.to_string());
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
        record.onion = state;
        record.problem.kind = ProblemKind::DescriptorUpload;
        record.problem.runtime = Runtime::UnexpectedIptKeys;
        record.bootstrap = Fraction::ThreeQuartersToComplete;
        record.blockage = Blockage::CannotBootstrap;
        record.terminal = Some(Terminal::StatusPanicked);
        assert!(record.to_string().len() < 1024);
    }
}

#[test]
fn severity_distinguishes_initial_recovery_degradation_and_failure() {
    let (observer, receiver) = fixture();
    assert!(!receiver.borrow().observed());
    observed(&observer, State::Shutdown);
    assert_eq!(receiver.borrow().severity().0, SeverityNumber::Info);
    observed(&observer, State::Broken);
    assert_eq!(receiver.borrow().severity().0, SeverityNumber::Error);
    observed(&observer, State::Bootstrapping);
    assert_eq!(receiver.borrow().severity().0, SeverityNumber::Info);
    observed(&observer, State::DegradedReachable);
    assert_eq!(receiver.borrow().severity().0, SeverityNumber::Warn);
    observed(&observer, State::Running);
    assert_eq!(receiver.borrow().severity().0, SeverityNumber::Info);
    observer.update(|current| current.problem.kind = ProblemKind::Unknown);
    assert_eq!(receiver.borrow().severity().0, SeverityNumber::Error);
    observer.failure(Terminal::StatusPanicked);
    assert_eq!(receiver.borrow().source, Source::Panicked);
    observer.failure(Terminal::StatusEnded);
    assert_eq!(receiver.borrow().terminal, Some(Terminal::StatusPanicked));
    assert_eq!(receiver.borrow().code(), "tor_background_status_panicked");
}

#[test]
fn sdk_state_predicate_and_missing_or_lost_observations_are_distinguished() {
    use tor_hsservice::status::State as SdkState;
    let (observer, receiver) = fixture();
    let initial = *receiver.borrow();
    assert_eq!(initial.reachability_label(), "unknown");
    assert_eq!(initial.bootstrap_ready_label(), "unknown");
    assert_eq!(initial.problem_label(), "not_observed");
    for (state, reachable) in [
        (SdkState::Shutdown, false),
        (SdkState::Bootstrapping, false),
        (SdkState::DegradedUnreachable, false),
        (SdkState::Recovering, false),
        (SdkState::Broken, false),
        (SdkState::DegradedReachable, true),
        (SdkState::Running, true),
    ] {
        observer.onion_parts(state, None);
        let record = *receiver.borrow();
        assert_eq!(record.source, Source::Observed);
        assert_eq!(record.fully_reachable, reachable);
        assert_eq!(record.reachability_label(), boolean(reachable));
        wire(record);
    }
    observer.failure(Terminal::StatusEnded);
    let ended = *receiver.borrow();
    assert_eq!(ended.onion, State::Running); // retained as the last observation
    assert_eq!(ended.reachability_label(), "unknown");
    assert_eq!(ended.severity().0, SeverityNumber::Error);
    observer.onion_parts(SdkState::Running, None);
    assert_eq!(*receiver.borrow(), ended); // late status never clears source loss
}

#[tokio::test(start_paused = true)]
async fn one_global_minute_budget_deduplicates_and_coalesces_bounded_latest_cell() {
    let (observer, receiver) = fixture();
    let client = stalled();
    let evidence = client.evidence();
    let local = Local::default();
    let mut worker = Box::pin(run_with_local(receiver.clone(), Ok(Some(client)), &local));
    assert!(worker.as_mut().now_or_never().is_none());
    assert_eq!(evidence.connecting.load(SeqCst), 0); // no fabricated initial healthy record
    observed(&observer, State::Bootstrapping);
    assert!(worker.as_mut().now_or_never().is_none());
    assert_eq!(evidence.connecting.load(SeqCst), 1);
    for index in 0..10000 {
        observed(
            &observer,
            if index % 2 == 0 {
                State::Running
            } else {
                State::Recovering
            },
        );
    }
    assert_eq!(receiver.borrow().onion, State::Recovering);
    observer.update(|current| current.problem.kind = ProblemKind::Ipt);
    tokio::time::advance(Duration::from_secs(1)).await;
    assert!(worker.as_mut().now_or_never().is_none());
    assert_eq!(evidence.connecting.load(SeqCst), 1);
    tokio::time::advance(Duration::from_secs(58)).await;
    assert!(worker.as_mut().now_or_never().is_none());
    assert_eq!(evidence.connecting.load(SeqCst), 1);
    tokio::time::advance(Duration::from_secs(1)).await;
    assert!(worker.as_mut().now_or_never().is_none());
    assert_eq!(evidence.connecting.load(SeqCst), 2);
    tokio::time::advance(Duration::from_secs(1)).await;
    assert!(worker.as_mut().now_or_never().is_none());
    for _ in 0..1000 {
        observed(&observer, State::Recovering);
    }
    tokio::time::advance(INTERVAL * 10).await;
    assert!(worker.as_mut().now_or_never().is_none());
    assert_eq!(evidence.connecting.load(SeqCst), 2); // unchanged never repeats, even on outage
    observer.failure(Terminal::EngineFailure);
    assert!(worker.as_mut().now_or_never().is_none());
    assert_eq!(evidence.connecting.load(SeqCst), 3);
    tokio::time::advance(Duration::from_secs(1)).await;
    assert!(worker.as_mut().now_or_never().is_none());
    drop(observer);
    worker.await;
    let records = local.records.lock().unwrap();
    assert_eq!(records.len(), 4); // three snapshots, one local-only warning
    assert_eq!(
        records
            .iter()
            .filter(|r| r.contains("delivery_unconfirmed"))
            .count(),
        1
    );
    assert!(records[2].contains("problem_kind=ipt"));
    assert!(records[3].contains("code=tor_background_engine_failure"));
}

#[tokio::test(start_paused = true)]
async fn returning_to_attempted_projection_before_deadline_sends_nothing() {
    let (observer, receiver) = fixture();
    let local = Local::default();
    let client = stalled();
    let evidence = client.evidence();
    observed(&observer, State::Running);
    let mut worker = Box::pin(run_with_local(receiver, Ok(Some(client)), &local));
    assert!(worker.as_mut().now_or_never().is_none());
    tokio::time::advance(Duration::from_secs(1)).await;
    assert!(worker.as_mut().now_or_never().is_none());
    observed(&observer, State::Recovering);
    assert!(worker.as_mut().now_or_never().is_none());
    observed(&observer, State::Running);
    assert!(worker.as_mut().now_or_never().is_none());
    tokio::time::advance(INTERVAL * 2).await;
    assert!(worker.as_mut().now_or_never().is_none());
    assert_eq!(evidence.connecting.load(SeqCst), 1);
    drop(observer);
    worker.await;
}

#[tokio::test]
async fn disabled_configuration_failure_and_closed_owner_never_deliver_snapshots() {
    let config = Config::from_lookup(|key| {
        assert_eq!(key, "OTEL_SDK_DISABLED");
        Some("true".into())
    });
    let (observer, worker) = new(config);
    observer.failure(Terminal::StatusEnded);
    assert!(worker.now_or_never().is_some());
    for failed in [false, true] {
        let (observer, receiver) = fixture();
        observer.failure(Terminal::StatusEnded);
        let local = Local::default();
        let client = if failed {
            Err(transport::Failure::Configuration)
        } else {
            Ok(None)
        };
        run_with_local(receiver, client, &local).await;
        let records = local.records.lock().unwrap();
        assert_eq!(records.len(), usize::from(failed));
        if failed {
            assert!(records[0].contains("constraint=transport_configuration"));
            assert!(!records[0].contains("event="));
        }
    }
    let (observer, receiver) = fixture();
    observer.failure(Terminal::StatusPanicked);
    drop(observer);
    let client = stalled();
    let evidence = client.evidence();
    let local = Local::default();
    run_with_local(receiver, Ok(Some(client)), &local).await;
    assert_eq!(evidence.connecting.load(SeqCst), 0);
    assert!(local.records.lock().unwrap().is_empty()); // no guaranteed fatal upload
}

#[tokio::test(start_paused = true)]
async fn terminal_cannot_bypass_minute_budget_and_drop_does_not_flush_or_retry() {
    let (observer, receiver) = fixture();
    observed(&observer, State::Running);
    let local = Local::default();
    let client = stalled();
    let evidence = client.evidence();
    let mut worker = Box::pin(run_with_local(receiver, Ok(Some(client)), &local));
    assert!(worker.as_mut().now_or_never().is_none());
    tokio::time::advance(Duration::from_secs(1)).await;
    assert!(worker.as_mut().now_or_never().is_none());
    observer.failure(Terminal::StatusEnded);
    assert!(worker.as_mut().now_or_never().is_none());
    tokio::time::advance(Duration::from_secs(58)).await;
    assert!(worker.as_mut().now_or_never().is_none());
    assert_eq!(evidence.connecting.load(SeqCst), 1);
    drop(worker);
    tokio::time::advance(INTERVAL * 10).await;
    assert_eq!(evidence.connecting.load(SeqCst), 1);
    assert_eq!(
        observer.snapshot.borrow().terminal,
        Some(Terminal::StatusEnded)
    );
}

#[tokio::test]
async fn actual_owned_tls_otlp_logs_use_existing_sender_and_sealed_wire() {
    let (listener, client, acceptor) =
        crate::telemetry::runtime::tests::fixture(true, "api.honeycomb.io").await;
    let evidence = client.evidence();
    let (observer, receiver) = fixture();
    observer.failure(Terminal::StatusEnded);
    let local = Local::default();
    let peer = async move {
        let (io, _) = listener.accept().await.unwrap();
        let mut io = acceptor.accept(io).await.unwrap();
        let bytes = capture(&mut io, "/v1/logs").await;
        let request = ExportLogsServiceRequest::decode(bytes.as_slice()).unwrap();
        assert_eq!(request.resource_logs[0].resource, Some(expected_resource()));
        let log = &request.resource_logs[0].scope_logs[0].log_records[0];
        assert_eq!(log.event_name, EVENT);
        assert_eq!(log.attributes.len(), 28);
        assert_eq!(log.severity_text, "ERROR");
        assert_eq!(log.time_unix_nano, 0);
        assert_eq!(log.observed_time_unix_nano, 0);
        assert!(log.trace_id.is_empty() && log.span_id.is_empty());
        assert!(!String::from_utf8_lossy(&bytes).contains("synthetic-key-canary"));
        io.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        drop(observer);
    };
    tokio::join!(run_with_local(receiver, Ok(Some(client)), &local), peer);
    assert_eq!(evidence.connections.load(SeqCst), 1);
    assert_eq!(evidence.live_io.load(SeqCst), 0);
    assert_eq!(local.records.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn hostile_http_otlp_failures_are_local_only_closed_delivery_constraints() {
    use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsPartialSuccess;
    let partial = ExportLogsServiceResponse {
        partial_success: Some(ExportLogsPartialSuccess {
            rejected_log_records: 0,
            error_message: HOSTILE.into(),
        }),
    }
    .encode_to_vec();
    for (status, body, constraint) in [
        (503, HOSTILE.as_bytes().to_vec(), "http_response_rejected"),
        (200, HOSTILE.as_bytes().to_vec(), "otlp_response_invalid"),
        (200, partial, "otlp_partial_success_or_warning"),
    ] {
        let (listener, client, acceptor) =
            crate::telemetry::runtime::tests::fixture(true, "api.honeycomb.io").await;
        let (observer, receiver) = fixture();
        observer.failure(Terminal::StatusPanicked);
        let local = Local::default();
        let peer = async move {
            let (io, _) = listener.accept().await.unwrap();
            let mut io = acceptor.accept(io).await.unwrap();
            let bytes = capture(&mut io, "/v1/logs").await;
            assert!(!String::from_utf8_lossy(&bytes).contains(HOSTILE));
            let header = format!(
                "HTTP/1.1 {status} Synthetic\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            io.write_all(header.as_bytes()).await.unwrap();
            let _ = io.write_all(&body).await;
            drop(observer);
        };
        tokio::join!(run_with_local(receiver, Ok(Some(client)), &local), peer);
        let records = local.records.lock().unwrap();
        assert_eq!(records.len(), 2);
        assert!(records[0].contains("code=tor_background_status_panicked"));
        assert!(records[1].contains(&format!("constraint={constraint} ")));
        assert!(records.iter().all(|r| !r.contains(HOSTILE)));
    }
}

#[tokio::test]
async fn dropping_worker_cancels_real_owned_io_without_late_writes() {
    let (listener, client, acceptor) =
        crate::telemetry::runtime::tests::fixture(true, "api.honeycomb.io").await;
    let evidence = client.evidence();
    let (observer, receiver) = fixture();
    observer.failure(Terminal::EngineFailure);
    let local = Local::default();
    let mut worker = Box::pin(run_with_local(receiver, Ok(Some(client)), &local));
    let peer = async {
        let (io, _) = listener.accept().await.unwrap();
        let mut io = acceptor.accept(io).await.unwrap();
        capture(&mut io, "/v1/logs").await;
        io
    };
    let io = tokio::select! {
        _ = &mut worker => panic!("synthetic peer withholds response"),
        io = peer => io,
    };
    assert_eq!(evidence.live_io.load(SeqCst), 1);
    drop(worker);
    assert_eq!(evidence.live_io.load(SeqCst), 0);
    let writes = evidence.write_polls.load(SeqCst);
    tokio::task::yield_now().await;
    assert_eq!(evidence.write_polls.load(SeqCst), writes);
    assert_eq!(evidence.connections.load(SeqCst), 1);
    assert_eq!(local.records.lock().unwrap().len(), 1);
    drop(io);
}
