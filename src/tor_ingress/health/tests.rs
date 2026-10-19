use super::*;
use retry_error::RetryError;
use tor_hsservice::{FatalError, HsNickname};

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
            "descriptor_timeout",
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
            problem: ProblemKind::DescriptorTimeout,
        });
        health.ended(interrupted);
        let observation = health.diagnostic(true);
        assert!(!observation.ready);
        assert_eq!(observation.code, expected);
        assert!(observation.constraint.contains("cause_unknown"));
        assert_eq!(observation.tor_problem, "descriptor_timeout");
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
                problem: ProblemKind::DescriptorTimeout,
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
                | (false, "bootstrapping", "descriptor_timeout")
        ));
    }
    task.join().unwrap();
}
