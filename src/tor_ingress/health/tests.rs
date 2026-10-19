use super::*;
use retry_error::RetryError;
use tor_hsservice::{DescUploadError, FatalError, HsNickname};

#[test]
fn every_sdk_state_preserves_the_existing_health_predicate() {
    let health = TorReadiness::default();
    assert_eq!(health.diagnostic(true).code, "tor_status_not_observed");
    for (state, label) in [
        (State::Shutdown, "shutdown"),
        (State::Bootstrapping, "bootstrapping"),
        (State::DegradedReachable, "degraded_reachable"),
        (State::DegradedUnreachable, "degraded_unreachable"),
        (State::Running, "running"),
        (State::Recovering, "recovering"),
        (State::Broken, "broken"),
    ] {
        for fatal in [false, true] {
            health.observe(EngineStatus::test((state, fatal)));
            let observation = health.diagnostic(true);
            assert_eq!(observation.tor_state, label);
            assert_eq!(observation.ready, state.is_fully_reachable() && !fatal);
            assert_eq!(health.is_ready(), observation.ready);
            assert_eq!(
                observation.tor_problem,
                if fatal { "runtime" } else { "none_observed" }
            );
        }
    }
    health.mark_running();
    let observation = health.diagnostic(false);
    assert_eq!(observation.code, "gateway_not_serving");
    assert!(!observation.ready);
}

fn upload_problem(error: DescUploadRetryError) -> Problem {
    Problem::DescriptorUpload(vec![error])
}

fn hostile_retry() -> RetryError<tor_hsservice::DescUploadError> {
    RetryError::in_attempt_to(
        "HOSTILE_SECRET https://private.invalid/onion account=user prompt=PRIVATE",
    )
}

#[test]
fn supported_sdk_problems_are_projected_without_formatting_or_retaining_payloads() {
    let runtime = Problem::Runtime(FatalError::MissingHsIdKeypair(
        HsNickname::new("hostile_private_identifier".into()).unwrap(),
    ));
    let cases = [
        (None, "none_observed"),
        (Some(runtime), "runtime"),
        (Some(Problem::Ipt(vec![])), "introduction_point"),
        (Some(Problem::DescriptorUpload(vec![])), "descriptor_upload"),
        (
            Some(upload_problem(DescUploadRetryError::FatalError(
                hostile_retry(),
            ))),
            "descriptor_fatal",
        ),
        (
            Some(upload_problem(DescUploadRetryError::MaxRetryCountExceeded(
                hostile_retry(),
            ))),
            "descriptor_retry_exhausted",
        ),
        (
            Some(upload_problem(DescUploadRetryError::Timeout(
                hostile_retry(),
            ))),
            "descriptor_timeout_no_recorded_source",
        ),
    ];
    let health = TorReadiness::default();
    for (problem, expected) in cases {
        health.observe(EngineStatus {
            state: State::DegradedUnreachable,
            problem: ProblemKind::from_sdk(problem.as_ref()),
        });
        drop(problem);
        let observation = health.diagnostic(true);
        assert_eq!(observation.tor_problem, expected);
        let json = serde_json::to_string(&observation).unwrap();
        assert!(json.len() < 1024);
        for sentinel in [
            "HOSTILE",
            "SECRET",
            "https://",
            "private.invalid",
            "account=",
            "prompt=",
            "hostile_private_identifier",
        ] {
            assert!(!json.contains(sentinel));
        }
    }
    let problem = Problem::DescriptorUpload(vec![
        DescUploadRetryError::Timeout(hostile_retry()),
        DescUploadRetryError::FatalError(hostile_retry()),
        DescUploadRetryError::MaxRetryCountExceeded(hostile_retry()),
    ]);
    assert!(matches!(
        ProblemKind::from_sdk(Some(&problem)),
        ProblemKind::DescriptorFatal
    ));
}

#[test]
fn later_stream_failure_preserves_known_problem_and_has_unknown_cause_diagnostics() {
    let health = TorReadiness::default();
    for (interrupted, expected) in [
        (false, "tor_status_stream_ended"),
        (true, "tor_status_stream_interrupted"),
    ] {
        health.observe(EngineStatus {
            state: State::DegradedUnreachable,
            problem: ProblemKind::DescriptorTimeoutUnknown,
        });
        health.ended(interrupted);
        let observation = health.diagnostic(true);
        assert!(!observation.ready);
        assert_eq!(observation.code, expected);
        assert!(observation.constraint.contains("cause_unknown"));
        assert_eq!(observation.tor_problem, "descriptor_timeout_unknown");
    }
    health.0.store(
        UNKNOWN_STATE | ((ProblemKind::Unknown as u16) << PROBLEM_SHIFT),
        Ordering::Release,
    );
    let observation = health.diagnostic(true);
    assert!(!observation.ready);
    assert_eq!(observation.code, "tor_state_unknown");
    assert_eq!(observation.tor_problem, "unknown");
}

#[test]
fn concurrent_readers_never_combine_different_status_events() {
    let health = std::sync::Arc::new(TorReadiness::default());
    let writer = health.clone();
    let task = std::thread::spawn(move || {
        for _ in 0..10_000 {
            writer.mark_running();
            writer.observe(EngineStatus {
                state: State::Bootstrapping,
                problem: ProblemKind::DescriptorTimeoutUnknown,
            });
        }
    });
    for _ in 0..10_000 {
        let observation = health.diagnostic(true);
        assert!(matches!(
            (
                observation.ready,
                observation.tor_state,
                observation.tor_problem
            ),
            (false, "not_observed", "none_observed")
                | (true, "running", "none_observed")
                | (false, "bootstrapping", "descriptor_timeout_unknown")
        ));
    }
    task.join().unwrap();
}

fn hostile_bug() -> tor_error::Bug {
    tor_error::Bug::new(tor_error::ErrorKind::Internal, "HOSTILE_INTERNAL_SECRET")
}

fn circuit() -> DescUploadError {
    DescUploadError::Circuit(tor_circmgr::Error::RequestTimeout)
}

fn stream() -> DescUploadError {
    DescUploadError::Stream(tor_circmgr::Error::RequestTimeout)
}

fn request() -> DescUploadError {
    DescUploadError::Request(tor_dirclient::RequestFailedError {
        source: None,
        error: tor_dirclient::RequestError::HttpStatus(503, "HOSTILE_REQUEST_SECRET".into()),
    })
}

fn timeout(sources: impl IntoIterator<Item = DescUploadError>) -> DescUploadRetryError {
    let mut retries = hostile_retry();
    for source in sources {
        retries.push(source);
    }
    DescUploadRetryError::Timeout(retries)
}

#[test]
fn timeout_sources_are_closed_order_independent_and_not_root_causes() {
    let cases = [
        (vec![], "descriptor_timeout_no_recorded_source"),
        (vec![circuit()], "descriptor_timeout_circuit"),
        (vec![stream()], "descriptor_timeout_stream"),
        (vec![request()], "descriptor_timeout_request"),
        (
            vec![DescUploadError::Bug(hostile_bug())],
            "descriptor_timeout_internal",
        ),
        (
            vec![request(), DescUploadError::Bug(hostile_bug())],
            "descriptor_timeout_mixed",
        ),
    ];
    for (mut sources, expected) in cases {
        // Both source order and row order are irrelevant; empty rows contribute
        // no class. Repeated observations of the same class remain that class.
        for _ in 0..2 {
            sources.reverse();
            let rows = vec![
                timeout([]),
                timeout(sources.clone()),
                timeout(sources.clone()),
            ];
            for rows in [rows.clone(), rows.into_iter().rev().collect()] {
                let problem = Problem::DescriptorUpload(rows);
                let health = TorReadiness::default();
                health.observe_sdk(State::DegradedUnreachable, Some(&problem));
                drop(problem);
                let observation = health.diagnostic(true);
                assert_eq!(observation.tor_problem, expected);
                assert!(!observation.ready);
                let json = serde_json::to_string(&observation).unwrap();
                assert!(json.len() < 1024);
                assert!(!json.contains("HOSTILE"));
                assert_eq!(
                    serde_json::from_str::<serde_json::Value>(&json)
                        .unwrap()
                        .as_object()
                        .unwrap()
                        .len(),
                    8
                );
            }
        }
    }
    // Distinct classes across separate rows must also fold into mixed, which
    // stays mixed even when a later row repeats an earlier class.
    let rows = [
        timeout([circuit()]),
        timeout([stream()]),
        timeout([circuit()]),
    ];
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let problem = Problem::DescriptorUpload(order.map(|i| rows[i].clone()).into());
        assert!(ProblemKind::from_sdk(Some(&problem)) == ProblemKind::DescriptorTimeoutMixed);
    }
}

#[test]
fn timeout_refinement_never_changes_outer_priority_or_fatality() {
    let timeout = timeout([DescUploadError::Bug(hostile_bug())]);
    let retry = DescUploadRetryError::MaxRetryCountExceeded(hostile_retry());
    let fatal = DescUploadRetryError::FatalError(hostile_retry());
    let internal = DescUploadRetryError::Bug(hostile_bug());
    for (rows, expected) in [
        (
            vec![timeout.clone(), retry.clone()],
            ProblemKind::DescriptorTimeoutInternal,
        ),
        (
            vec![timeout.clone(), retry.clone(), fatal.clone()],
            ProblemKind::DescriptorFatal,
        ),
        (
            vec![timeout, retry, fatal, internal],
            ProblemKind::DescriptorInternal,
        ),
    ] {
        for offset in 0..rows.len() {
            let mut permuted = rows.clone();
            permuted.rotate_left(offset);
            for rows in [permuted.clone(), permuted.into_iter().rev().collect()] {
                let problem = Problem::DescriptorUpload(rows);
                let kind = ProblemKind::from_sdk(Some(&problem));
                assert!(kind == expected);
                assert!(!kind.is_fatal());
            }
        }
    }
    assert!(priority(ProblemKind::DescriptorInternal) > priority(ProblemKind::DescriptorFatal));
    assert!(priority(ProblemKind::DescriptorFatal) > priority(ProblemKind::Unknown));
    assert!(priority(ProblemKind::Unknown) > priority(ProblemKind::DescriptorTimeout));
    assert!(
        priority(ProblemKind::DescriptorTimeout) > priority(ProblemKind::DescriptorRetryExhausted)
    );
    assert!(
        priority(ProblemKind::DescriptorRetryExhausted) > priority(ProblemKind::DescriptorUpload)
    );
}

#[test]
fn future_timeout_source_fallback_stays_unknown_or_mixed() {
    // The pinned non-exhaustive SDK has no constructible unknown variant.
    // Exercise the local fold destination used by its wildcard, not fake SDK types.
    let unknown = ProblemKind::DescriptorTimeoutUnknown;
    assert!(
        ProblemKind::DescriptorTimeoutNoRecordedSource.merge_timeout_source(unknown) == unknown
    );
    assert!(unknown.merge_timeout_source(unknown) == unknown);
    assert!(
        unknown.merge_timeout_source(ProblemKind::DescriptorTimeoutCircuit)
            == ProblemKind::DescriptorTimeoutMixed
    );
    assert!(
        ProblemKind::DescriptorTimeoutMixed.merge_timeout_source(unknown)
            == ProblemKind::DescriptorTimeoutMixed
    );
}

#[test]
fn all_sixteen_problem_ordinals_preserve_state_readiness_and_fatality() {
    let kinds = [
        ProblemKind::NoneObserved,
        ProblemKind::Runtime,
        ProblemKind::DescriptorUpload,
        ProblemKind::IntroductionPoint,
        ProblemKind::DescriptorFatal,
        ProblemKind::DescriptorRetryExhausted,
        ProblemKind::DescriptorTimeout,
        ProblemKind::DescriptorInternal,
        ProblemKind::Unknown,
        ProblemKind::DescriptorTimeoutCircuit,
        ProblemKind::DescriptorTimeoutStream,
        ProblemKind::DescriptorTimeoutRequest,
        ProblemKind::DescriptorTimeoutInternal,
        ProblemKind::DescriptorTimeoutMixed,
        ProblemKind::DescriptorTimeoutNoRecordedSource,
        ProblemKind::DescriptorTimeoutUnknown,
    ];
    assert_eq!(kinds.len(), 16);
    assert_eq!(PROBLEMS.len(), kinds.len());
    assert_eq!(
        std::mem::size_of::<TorReadiness>(),
        std::mem::size_of::<AtomicU16>()
    );
    for (ordinal, kind) in kinds.into_iter().enumerate() {
        assert_eq!(kind as usize, ordinal);
        assert_eq!(kind.is_fatal(), kind == ProblemKind::Runtime);
        if ordinal >= 9 {
            assert_eq!(priority(kind), 2);
        }
        for (state, label) in [
            (State::Running, "running"),
            (State::Bootstrapping, "bootstrapping"),
        ] {
            let health = TorReadiness::default();
            health.observe(EngineStatus {
                state,
                problem: kind,
            });
            let observation = health.diagnostic(true);
            assert_eq!(observation.tor_problem, PROBLEMS[ordinal]);
            assert_eq!(observation.tor_state, label);
            assert_eq!(
                observation.ready,
                state.is_fully_reachable() && !kind.is_fatal()
            );
            assert_eq!(health.is_ready(), observation.ready);
        }
    }
    // The legacy bare label remains distinct; it says nothing about sources.
    assert_eq!(
        PROBLEMS[ProblemKind::DescriptorTimeout as usize],
        "descriptor_timeout"
    );
}

#[tokio::test]
async fn operational_stream_eof_and_poll_unwind_keep_highest_refined_problem() {
    use futures_util::{stream, StreamExt};
    for (kind, label) in [
        (
            ProblemKind::DescriptorTimeoutNoRecordedSource,
            "descriptor_timeout_no_recorded_source",
        ),
        (
            ProblemKind::DescriptorTimeoutUnknown,
            "descriptor_timeout_unknown",
        ),
    ] {
        for interrupted in [false, true] {
            let health = std::sync::Arc::new(TorReadiness::default());
            let statuses = stream::iter([EngineStatus {
                state: State::Running,
                problem: kind,
            }]);
            let statuses = if interrupted {
                statuses
                    .chain(stream::poll_fn(|_| panic!("HOSTILE_POLL_SECRET")))
                    .boxed()
            } else {
                statuses.boxed()
            };
            let failure = crate::tor_ingress::engine_status(statuses, Some(health.clone())).await;
            assert!(matches!(failure, crate::tor_ingress::Failure::Engine));
            let observation = health.diagnostic(true);
            assert!(!observation.ready);
            assert_eq!(observation.tor_problem, label);
            assert_eq!(
                observation.code,
                if interrupted {
                    "tor_status_stream_interrupted"
                } else {
                    "tor_status_stream_ended"
                }
            );
            assert!(!serde_json::to_string(&observation)
                .unwrap()
                .contains("HOSTILE"));
        }
    }
}
