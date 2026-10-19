use super::*;
use retry_error::RetryError;
use serde_json::{json, Value};
use std::sync::Arc;
use tor_circmgr::Error as CircuitError;
use tor_dirclient::RequestError;

const HOSTILE: &str = "HOSTILE_SECRET https://private.invalid identity=PRIVATE prompt=PRIVATE";

fn bug(kind: ErrorKind) -> tor_error::Bug {
    tor_error::Bug::new(kind, HOSTILE)
}

fn retries<T>(sources: impl IntoIterator<Item = T>) -> RetryError<T> {
    let mut retries = RetryError::in_attempt_to(HOSTILE);
    for source in sources {
        retries.push(source);
    }
    retries
}

fn upload(sources: impl IntoIterator<Item = DescUploadError>) -> DescriptorEvidence {
    DescriptorEvidence::from_sdk(Some(&Problem::DescriptorUpload(vec![
        DescUploadRetryError::Timeout(retries(sources)),
    ])))
}

fn request_source(error: RequestError) -> DescUploadError {
    DescUploadError::Request(tor_dirclient::RequestFailedError {
        source: None,
        error,
    })
}

fn value(evidence: DescriptorEvidence) -> Value {
    let text = serde_json::to_string(&evidence).unwrap();
    for sentinel in [
        "HOSTILE",
        "SECRET",
        "PRIVATE",
        "private.invalid",
        "https://",
        "identity=",
        "prompt=",
    ] {
        assert!(!text.contains(sentinel));
    }
    serde_json::from_str(&text).unwrap()
}

#[test]
fn selection_and_absent_retained_sources_never_claim_no_failures() {
    assert_eq!(
        value(DescriptorEvidence::default())["selection"],
        "no_snapshot"
    );
    assert_eq!(
        value(DescriptorEvidence::from_sdk(None))["selection"],
        "no_problem_selected"
    );
    assert_eq!(
        value(DescriptorEvidence::from_sdk(Some(&Problem::Ipt(vec![]))))["selection"],
        "other_problem_selected"
    );
    let empty = value(DescriptorEvidence::from_sdk(Some(
        &Problem::DescriptorUpload(vec![]),
    )));
    assert_eq!(empty["selection"], "descriptor_upload_selected");
    assert_eq!(empty["outcomes"], json!([]));
    assert_eq!(
        empty["coverage"],
        json!(["retained_sources_only", "empty_batch"])
    );
    let absent = value(upload([]));
    assert_eq!(absent["outcomes"], json!(["timeout"]));
    assert_eq!(
        absent["coverage"],
        json!(["retained_sources_only", "no_retained_sources"])
    );
}

#[test]
fn outer_outcomes_and_phase_sets_are_independent_unique_and_ordered() {
    let sources = vec![
        DescUploadError::Circuit(CircuitError::CircTimeout(None)),
        DescUploadError::Stream(CircuitError::RequestTimeout),
        request_source(RequestError::HttpStatus(503, HOSTILE.into())),
        DescUploadError::Bug(bug(ErrorKind::Internal)),
    ];
    let rows = vec![
        DescUploadRetryError::Timeout(retries(sources.clone())),
        DescUploadRetryError::FatalError(retries(sources.clone())),
        DescUploadRetryError::MaxRetryCountExceeded(retries([])),
        DescUploadRetryError::Bug(bug(ErrorKind::BadApiUsage)),
    ];
    let baseline = value(DescriptorEvidence::from_sdk(Some(
        &Problem::DescriptorUpload(rows.clone()),
    )));
    assert_eq!(
        baseline["outcomes"],
        json!(["fatal_error", "max_retry_count_exceeded", "timeout", "bug"])
    );
    assert_eq!(baseline["circuit"]["variants"], json!(["circuit_timeout"]));
    assert_eq!(baseline["stream"]["variants"], json!(["request_timeout"]));
    assert_eq!(baseline["request"]["http_statuses"], json!([503]));
    assert_eq!(
        baseline["unphased"]["kinds"],
        json!(["bad_api_usage", "internal"])
    );
    for offset in 0..rows.len() {
        let mut permuted = rows.clone();
        permuted.rotate_left(offset);
        for rows in [permuted.clone(), permuted.into_iter().rev().collect()] {
            assert_eq!(
                value(DescriptorEvidence::from_sdk(Some(
                    &Problem::DescriptorUpload(rows)
                ))),
                baseline
            );
        }
    }
    let baseline = value(upload(sources.clone()));
    for offset in 0..sources.len() {
        let mut permuted = sources.clone();
        permuted.rotate_left(offset);
        let doubled = permuted.clone().into_iter().chain(permuted.clone());
        assert_eq!(value(upload(doubled)), baseline);
        assert_eq!(value(upload(permuted.into_iter().rev())), baseline);
    }
}

#[test]
fn constructible_circuit_structures_keep_constituent_kinds_in_their_original_phase() {
    let cases = vec![
        (
            CircuitError::PendingCanceled,
            "pending_canceled",
            "reactor_shutting_down",
        ),
        (
            CircuitError::CircCanceled,
            "circuit_canceled",
            "transient_failure",
        ),
        (
            CircuitError::CircTimeout(None),
            "circuit_timeout",
            "tor_network_timeout",
        ),
        (
            CircuitError::RequestTimeout,
            "request_timeout",
            "tor_network_timeout",
        ),
        (
            CircuitError::NoRelay {
                path_kind: HOSTILE,
                role: HOSTILE,
                problem: HOSTILE.into(),
            },
            "no_relay",
            "no_path",
        ),
        (
            CircuitError::Guard(bug(ErrorKind::Internal).into()),
            "guard_selection",
            "internal",
        ),
        (
            CircuitError::VanguardMgrInit(bug(ErrorKind::Internal).into()),
            "vanguard_manager_init",
            "internal",
        ),
        (
            CircuitError::Protocol {
                action: HOSTILE,
                peer: None,
                error: tor_proto::Error::CircuitClosed,
                unique_id: None,
            },
            "protocol",
            "circuit_collapse",
        ),
        (
            CircuitError::Spawn {
                spawning: HOSTILE,
                cause: Arc::new(futures_util::task::SpawnError::shutdown()),
            },
            "spawn",
            "reactor_shutting_down",
        ),
        (
            CircuitError::Bug(bug(ErrorKind::BadApiUsage)),
            "bug",
            "bad_api_usage",
        ),
    ];
    for (error, variant, kind) in cases {
        for phase in ["circuit", "stream"] {
            let source = if phase == "circuit" {
                DescUploadError::Circuit(error.clone())
            } else {
                DescUploadError::Stream(error.clone())
            };
            let projected = value(upload([source]));
            assert_eq!(projected[phase]["variants"], json!([variant]));
            assert_eq!(projected[phase]["kinds"], json!([kind]));
            let other = if phase == "circuit" {
                "stream"
            } else {
                "circuit"
            };
            assert_eq!(projected[other]["variants"], json!([]));
        }
    }
    let nested = CircuitError::PendingFailed(Box::new(CircuitError::RequestFailed(retries([
        Box::new(CircuitError::CircTimeout(None)),
        Box::new(CircuitError::NoRelay {
            path_kind: HOSTILE,
            role: HOSTILE,
            problem: HOSTILE.into(),
        }),
    ]))));
    let projected = value(upload([DescUploadError::Stream(nested)]));
    assert_eq!(
        projected["stream"]["variants"],
        json!([
            "pending_failed",
            "circuit_timeout",
            "no_relay",
            "request_failed"
        ])
    );
    assert_eq!(
        projected["stream"]["kinds"],
        json!(["tor_network_timeout", "no_path"])
    );
    let projected = value(upload([DescUploadError::Circuit(
        CircuitError::RequestFailed(retries([])),
    )]));
    assert_eq!(projected["circuit"]["kinds"], json!([]));
    assert_eq!(
        projected["coverage"],
        json!(["retained_sources_only", "no_nested_sources"])
    );
}

#[test]
fn every_request_structure_discards_payloads_and_preserves_kind() {
    let http_error = http::Request::builder()
        .header("bad\nheader", HOSTILE)
        .body(())
        .unwrap_err();
    let cases = vec![
        (
            RequestError::DirTimeout,
            "directory_timeout",
            "tor_network_timeout",
        ),
        (
            RequestError::TruncatedHeaders,
            "truncated_headers",
            "tor_protocol_violation",
        ),
        (
            RequestError::ResponseTooLong(usize::MAX),
            "response_size_rejected",
            "tor_protocol_violation",
        ),
        (
            RequestError::HeadersTooLong(usize::MAX),
            "header_size_rejected",
            "tor_protocol_violation",
        ),
        (
            RequestError::Utf8Encoding(String::from_utf8(vec![255]).unwrap_err()),
            "utf8_encoding",
            "tor_protocol_violation",
        ),
        (
            RequestError::IoError(Arc::new(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                HOSTILE,
            ))),
            "io",
            "tor_directory_error",
        ),
        (
            RequestError::Proto(tor_proto::Error::StreamProto(HOSTILE.into())),
            "stream_protocol",
            "tor_protocol_violation",
        ),
        (
            RequestError::Tunnel(CircuitError::RequestTimeout),
            "tunnel",
            "tor_network_timeout",
        ),
        (
            RequestError::HttparseError(httparse::Error::HeaderName),
            "http_parsing",
            "tor_protocol_violation",
        ),
        (
            RequestError::HttpError(Arc::new(http_error)),
            "http_construction",
            "internal",
        ),
        (
            RequestError::ContentEncoding(HOSTILE.into()),
            "content_encoding",
            "tor_protocol_violation",
        ),
        (
            RequestError::TooMuchClockSkew,
            "clock_skew",
            "tor_directory_error",
        ),
        (RequestError::EmptyRequest, "empty_request", "internal"),
        (
            RequestError::EmptyResponse,
            "empty_response",
            "tor_protocol_violation",
        ),
        (
            RequestError::HttpStatus(503, HOSTILE.into()),
            "http_status",
            "tor_directory_error",
        ),
    ];
    for (error, variant, kind) in cases {
        let projected = value(upload([request_source(error)]));
        assert!(projected["request"]["variants"]
            .as_array()
            .unwrap()
            .contains(&json!(variant)));
        assert_eq!(projected["request"]["kinds"], json!([kind]));
        assert_eq!(projected["circuit"]["variants"], json!([]));
        assert_eq!(projected["stream"]["variants"], json!([]));
    }
}

#[test]
fn http_statuses_are_a_finite_valid_set_never_arbitrary_u16_or_reason_text() {
    let statuses = [0, 99, 100, 200, 503, 599, 600, u16::MAX, 503];
    let projected = value(upload(statuses.map(|status| {
        request_source(RequestError::HttpStatus(status, HOSTILE.into()))
    })));
    assert_eq!(
        projected["request"]["http_statuses"],
        json!([100, 200, 503, 599])
    );
    assert_eq!(
        projected["request"]["variants"],
        json!(["http_status", "invalid_http_status"])
    );
    let mut bits = HttpStatuses::default();
    for status in 0..=u16::MAX {
        assert_eq!(bits.insert(status), (100..=599).contains(&status));
    }
    assert_eq!(
        serde_json::to_value(bits).unwrap(),
        json!((100..=599).collect::<Vec<u16>>())
    );
}

#[test]
fn io_projection_uses_only_closed_kind_not_os_code_or_message() {
    macro_rules! check { ($($variant:ident => $label:literal),+ $(,)?) => { $(
        let projected = value(upload([request_source(RequestError::IoError(Arc::new(std::io::Error::new(std::io::ErrorKind::$variant, HOSTILE))))]));
        assert_eq!(projected["request"]["io_kinds"], json!([$label]));
    )+ }; }
    check!(
        NotFound => "not_found", PermissionDenied => "permission_denied", ConnectionRefused => "connection_refused",
        ConnectionReset => "connection_reset", HostUnreachable => "host_unreachable", NetworkUnreachable => "network_unreachable",
        ConnectionAborted => "connection_aborted", NotConnected => "not_connected", AddrInUse => "address_in_use",
        AddrNotAvailable => "address_not_available", NetworkDown => "network_down", BrokenPipe => "broken_pipe",
        AlreadyExists => "already_exists", WouldBlock => "would_block", NotADirectory => "not_a_directory",
        IsADirectory => "is_a_directory", DirectoryNotEmpty => "directory_not_empty", ReadOnlyFilesystem => "read_only_filesystem",
        StaleNetworkFileHandle => "stale_network_file_handle", InvalidInput => "invalid_input", InvalidData => "invalid_data",
        TimedOut => "timed_out", WriteZero => "write_zero", StorageFull => "storage_full", NotSeekable => "not_seekable",
        QuotaExceeded => "quota_exceeded", FileTooLarge => "file_too_large", ResourceBusy => "resource_busy",
        ExecutableFileBusy => "executable_file_busy", Deadlock => "deadlock", CrossesDevices => "crosses_devices",
        TooManyLinks => "too_many_links", InvalidFilename => "invalid_filename", ArgumentListTooLong => "argument_list_too_long",
        Interrupted => "interrupted", Unsupported => "unsupported", UnexpectedEof => "unexpected_eof", OutOfMemory => "out_of_memory", Other => "other"
    );
    // Pinned Rust cannot construct future ErrorKind variants. The local fallback
    // is qualified separately below, rather than inventing an SDK fixture.
}

#[test]
fn has_kind_projection_uses_an_explicit_catalog_including_unknown_fallback() {
    macro_rules! check { ($($variant:ident => $label:literal),+ $(,)?) => { $(
        // Bug is the pinned public typed fixture permitting each HasKind value;
        // its hostile message must not be consulted even for unknown kinds.
        let projected = value(upload([DescUploadError::Circuit(CircuitError::Bug(bug(ErrorKind::$variant)))]));
        assert_eq!(projected["circuit"]["kinds"], json!([$label]));
    )+ }; }
    check!(
        TorAccessFailed => "tor_access_failed", BootstrapRequired => "bootstrap_required",
        DirectoryExpired => "directory_expired", PersistentStateAccessFailed => "persistent_state_access_failed",
        LocalResourceAlreadyInUse => "local_resource_already_in_use", FsPermissions => "filesystem_permissions",
        PersistentStateCorrupted => "persistent_state_corrupted", CacheCorrupted => "cache_corrupted",
        CacheAccessFailed => "cache_access_failed", KeystoreCorrupted => "keystore_corrupted",
        KeystoreAccessFailed => "keystore_access_failed", ReactorShuttingDown => "reactor_shutting_down",
        ArtiShuttingDown => "arti_shutting_down", SoftwareDeprecated => "software_deprecated",
        RemoteNetworkTimeout => "remote_network_timeout", InvalidConfig => "invalid_config",
        InvalidConfigTransition => "invalid_config_transition", NoHomeDirectory => "no_home_directory",
        NotImplemented => "not_implemented", FeatureDisabled => "feature_disabled",
        LocalProtocolViolation => "local_protocol_violation", TorProtocolViolation => "tor_protocol_violation",
        LocalNetworkError => "local_network_error", LocalResourceExhausted => "local_resource_exhausted",
        ExternalToolFailed => "external_tool_failed", RelayIdMismatch => "relay_identity_mismatch",
        CircuitCollapse => "circuit_collapse", TorNetworkTimeout => "tor_network_timeout",
        TorDirectoryError => "tor_directory_error", RemoteStreamClosed => "remote_stream_closed",
        RemoteStreamReset => "remote_stream_reset", RemoteStreamError => "remote_stream_error",
        RemoteConnectionRefused => "remote_connection_refused", ExitPolicyRejected => "exit_policy_rejected",
        ExitTimeout => "exit_timeout", RemoteNetworkFailed => "remote_network_failed",
        RemoteHostNotFound => "remote_host_not_found", RemoteHostResolutionFailed => "remote_host_resolution_failed",
        RemoteProtocolViolation => "remote_protocol_violation", RelayTooBusy => "relay_too_busy",
        InvalidStreamTarget => "invalid_stream_target", ForbiddenStreamTarget => "forbidden_stream_target",
        TransientFailure => "transient_failure", BadApiUsage => "bad_api_usage", CircuitRefused => "circuit_refused",
        NoPath => "no_path", NoExit => "no_exit", TorDirectoryUnusable => "tor_directory_unusable",
        ClockSkew => "clock_skew", TorDocumentRejected => "tor_document_rejected", Internal => "internal", Other => "other"
    );
    // This kind is deliberately outside the relevant descriptor catalog. Future
    // non-exhaustive SDK variants are not constructible; they use this same arm.
    let projected = value(upload([DescUploadError::Circuit(CircuitError::Bug(bug(
        ErrorKind::OnionServiceNotFound,
    )))]));
    assert_eq!(projected["circuit"]["kinds"], json!(["unknown"]));
    assert_eq!(
        projected["coverage"],
        json!(["retained_sources_only", "unsupported_kind"])
    );
}

#[test]
fn depth_and_total_node_limits_preserve_already_classified_detail() {
    let mut nested = CircuitError::CircTimeout(None);
    for _ in 0..MAX_DEPTH + 4 {
        nested = CircuitError::PendingFailed(Box::new(nested));
    }
    let projected = value(upload([
        request_source(RequestError::DirTimeout),
        DescUploadError::Circuit(nested),
    ]));
    assert_eq!(
        projected["request"]["variants"],
        json!(["directory_timeout"])
    );
    assert_eq!(projected["circuit"]["variants"], json!(["pending_failed"]));
    assert_eq!(projected["circuit"]["kinds"], json!([]));
    assert!(projected["coverage"]
        .as_array()
        .unwrap()
        .contains(&json!("depth_limit")));
    let many = (0..MAX_NODES * 2).map(|_| Box::new(CircuitError::RequestTimeout));
    let projected = value(upload([
        DescUploadError::Circuit(CircuitError::RequestFailed(retries(many))),
        request_source(RequestError::DirTimeout),
    ]));
    assert!(projected["coverage"]
        .as_array()
        .unwrap()
        .contains(&json!("node_limit")));
    assert_eq!(
        projected["circuit"]["kinds"],
        json!(["tor_network_timeout"])
    );
    assert_eq!(projected["request"]["variants"], json!([]));
    // Rejected depth visits also consume the total budget, so a wide deep batch
    // cannot turn depth-limited traversal into unbounded sibling processing.
    let mut budget = Budget {
        visited: 0,
        coverage: Coverage::default(),
    };
    for _ in 0..MAX_NODES {
        assert!(!budget.visit(MAX_DEPTH + 1));
    }
    assert!(!budget.visit(0));
    assert_eq!(budget.visited, MAX_NODES);
    let mut phase = Phase::default();
    unknown(&mut phase, &mut budget);
    assert_eq!(
        serde_json::to_value(phase.variants).unwrap(),
        json!(["unknown"])
    );
    assert_eq!(
        serde_json::to_value(phase.kinds).unwrap(),
        json!(["unknown"])
    );
}

#[test]
fn worst_case_serialization_and_snapshot_storage_are_bounded() {
    let mut statuses = HttpStatuses::default();
    for status in 100..=599 {
        assert!(statuses.insert(status));
    }
    let phase = Phase {
        variants: Variants(u64::MAX),
        kinds: Kinds(u64::MAX),
        io_kinds: IoKinds(u64::MAX),
        http_statuses: statuses,
    };
    let evidence = DescriptorEvidence {
        selection: Selection::DescriptorUploadSelected,
        outcomes: Outcomes(u64::MAX),
        coverage: Coverage(u64::MAX),
        circuit: phase,
        stream: phase,
        request: phase,
        unphased: phase,
    };
    let encoded = serde_json::to_vec(&evidence).unwrap();
    // Deliberately over-approximates reachable states: every catalog flag and
    // every valid status in all four buckets at once, with no source count.
    assert!(encoded.len() < 32 * 1024);
    assert!(std::mem::size_of::<DescriptorEvidence>() <= 512);
    let value = value(evidence);
    for phase in ["circuit", "stream", "request", "unphased"] {
        for field in ["variants", "kinds", "io_kinds", "http_statuses"] {
            let labels = value[phase][field].as_array().unwrap();
            for (index, label) in labels.iter().enumerate() {
                assert!(!labels[..index].contains(label));
            }
        }
    }
}
