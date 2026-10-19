//! Bounded typed projection only: never format errors or retain SDK objects.
use serde::{ser::SerializeSeq, Serialize, Serializer};
use tor_error::{ErrorKind, HasKind};
use tor_hsservice::{
    status::{DescUploadRetryError, Problem},
    DescUploadError,
};

const MAX_DEPTH: usize = 8;
const MAX_NODES: usize = 128;

// Each catalog fixes both vocabulary and serialization order. No SDK text is used.
macro_rules! closed_set {
    ($set:ident, $item:ident { $($variant:ident => $label:literal),+ $(,)? }) => {
        #[derive(Clone, Copy)]
        #[repr(u8)]
        enum $item { $($variant),+ }
        #[derive(Clone, Copy, Default, Eq, PartialEq)]
        struct $set(u64);
        impl $set {
            fn insert(&mut self, item: $item) { self.0 |= 1 << item as u8; }
        }
        impl Serialize for $set {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                let mut seq = serializer.serialize_seq(None)?;
                $(if self.0 & (1 << $item::$variant as u8) != 0 { seq.serialize_element($label)?; })+
                seq.end()
            }
        }
        const _: () = { assert!([$($label),+].len() <= 64); };
    };
}

closed_set!(Outcomes, Outcome {
    Fatal => "fatal_error", Exhausted => "max_retry_count_exceeded", Timeout => "timeout",
    Bug => "bug", Unknown => "unknown"
});
closed_set!(Coverage, Gap {
    RetainedOnly => "retained_sources_only", EmptyBatch => "empty_batch",
    NoSources => "no_retained_sources", NoNestedSources => "no_nested_sources",
    Depth => "depth_limit", Nodes => "node_limit", Unknown => "unsupported_variant",
    UnknownKind => "unsupported_kind", UnknownIo => "unsupported_io_kind"
});
closed_set!(Variants, Variant {
    GuardNotUsable => "guard_not_usable", PendingCanceled => "pending_canceled",
    PendingFailed => "pending_failed", LostUsabilityRace => "lost_usability_race",
    CircCanceled => "circuit_canceled", UsageMismatched => "usage_mismatched",
    CircTimeout => "circuit_timeout", RequestTimeout => "request_timeout", NoRelay => "no_relay",
    GuardMgr => "guard_manager", Guard => "guard_selection", VanguardMgrInit => "vanguard_manager_init",
    RequestFailed => "request_failed", Channel => "channel", Protocol => "protocol",
    Spawn => "spawn", State => "persistent_state", Bug => "bug",
    DirTimeout => "directory_timeout", TruncatedHeaders => "truncated_headers",
    ResponseTooLong => "response_size_rejected", HeadersTooLong => "header_size_rejected",
    Utf8 => "utf8_encoding", Io => "io", Proto => "stream_protocol", Tunnel => "tunnel",
    HttpParse => "http_parsing", HttpConstruct => "http_construction", Encoding => "content_encoding",
    ClockSkew => "clock_skew", EmptyRequest => "empty_request", EmptyResponse => "empty_response",
    HttpStatus => "http_status", InvalidStatus => "invalid_http_status", Unknown => "unknown"
});
closed_set!(Kinds, Kind {
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
    TransientFailure => "transient_failure", BadApiUsage => "bad_api_usage",
    CircuitRefused => "circuit_refused", NoPath => "no_path", NoExit => "no_exit",
    TorDirectoryUnusable => "tor_directory_unusable", ClockSkew => "clock_skew",
    TorDocumentRejected => "tor_document_rejected", Internal => "internal", Other => "other", Unknown => "unknown"
});
closed_set!(IoKinds, IoKind {
    NotFound => "not_found", PermissionDenied => "permission_denied", ConnectionRefused => "connection_refused",
    ConnectionReset => "connection_reset", HostUnreachable => "host_unreachable", NetworkUnreachable => "network_unreachable",
    ConnectionAborted => "connection_aborted", NotConnected => "not_connected", AddrInUse => "address_in_use",
    AddrNotAvailable => "address_not_available", NetworkDown => "network_down", BrokenPipe => "broken_pipe",
    AlreadyExists => "already_exists", WouldBlock => "would_block", NotADirectory => "not_a_directory",
    IsADirectory => "is_a_directory", DirectoryNotEmpty => "directory_not_empty", ReadOnlyFilesystem => "read_only_filesystem",
    StaleNetworkFileHandle => "stale_network_file_handle",
    InvalidInput => "invalid_input", InvalidData => "invalid_data", TimedOut => "timed_out", WriteZero => "write_zero",
    StorageFull => "storage_full", NotSeekable => "not_seekable", QuotaExceeded => "quota_exceeded",
    FileTooLarge => "file_too_large", ResourceBusy => "resource_busy", ExecutableFileBusy => "executable_file_busy",
    Deadlock => "deadlock", CrossesDevices => "crosses_devices", TooManyLinks => "too_many_links",
    InvalidFilename => "invalid_filename", ArgumentListTooLong => "argument_list_too_long", Interrupted => "interrupted",
    Unsupported => "unsupported", UnexpectedEof => "unexpected_eof", OutOfMemory => "out_of_memory", Other => "other",
    Unknown => "unknown"
});

#[derive(Clone, Copy, Default, Eq, PartialEq)]
struct HttpStatuses([u64; 8]);
impl HttpStatuses {
    fn insert(&mut self, status: u16) -> bool {
        if !(100..=599).contains(&status) {
            return false;
        }
        let index = usize::from(status - 100);
        self.0[index / 64] |= 1 << (index % 64);
        true
    }
}
impl Serialize for HttpStatuses {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(None)?;
        for status in 100..=599u16 {
            let index = usize::from(status - 100);
            if self.0[index / 64] & (1 << (index % 64)) != 0 {
                seq.serialize_element(&status)?;
            }
        }
        seq.end()
    }
}

#[derive(Clone, Copy, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Selection {
    #[default]
    NoSnapshot,
    NoProblemSelected,
    OtherProblemSelected,
    DescriptorUploadSelected,
}

#[derive(Clone, Copy, Default, Eq, PartialEq, Serialize)]
struct Phase {
    variants: Variants,
    kinds: Kinds,
    io_kinds: IoKinds,
    http_statuses: HttpStatuses,
}

#[derive(Clone, Copy, Default, Eq, PartialEq, Serialize)]
pub(super) struct DescriptorEvidence {
    selection: Selection,
    outcomes: Outcomes,
    coverage: Coverage,
    circuit: Phase,
    stream: Phase,
    request: Phase,
    unphased: Phase,
}

struct Budget {
    visited: usize,
    coverage: Coverage,
}
impl Budget {
    fn visit(&mut self, depth: usize) -> bool {
        if self.visited == MAX_NODES {
            self.coverage.insert(Gap::Nodes);
            return false;
        }
        self.visited += 1;
        if depth > MAX_DEPTH {
            self.coverage.insert(Gap::Depth);
            return false;
        }
        true
    }
    fn exhausted(&self) -> bool {
        self.visited == MAX_NODES
    }
}

impl DescriptorEvidence {
    pub(super) fn from_sdk(problem: Option<&Problem>) -> Self {
        let mut evidence = Self::default();
        let rows = match problem {
            None => {
                evidence.selection = Selection::NoProblemSelected;
                return evidence;
            }
            Some(Problem::DescriptorUpload(rows)) => rows,
            Some(_) => {
                evidence.selection = Selection::OtherProblemSelected;
                return evidence;
            }
        };
        evidence.selection = Selection::DescriptorUploadSelected;
        let mut budget = Budget {
            visited: 0,
            coverage: Coverage::default(),
        };
        // These are retained completed sources, not an exhaustive attempt history
        // or evidence of the final timed-out operation's exact timer/subphase.
        budget.coverage.insert(Gap::RetainedOnly);
        if rows.is_empty() {
            budget.coverage.insert(Gap::EmptyBatch);
        }
        for row in rows {
            if !budget.visit(0) {
                break;
            }
            let sources = match row {
                DescUploadRetryError::FatalError(sources) => {
                    evidence.outcomes.insert(Outcome::Fatal);
                    sources
                }
                DescUploadRetryError::MaxRetryCountExceeded(sources) => {
                    evidence.outcomes.insert(Outcome::Exhausted);
                    sources
                }
                DescUploadRetryError::Timeout(sources) => {
                    evidence.outcomes.insert(Outcome::Timeout);
                    sources
                }
                DescUploadRetryError::Bug(error) => {
                    evidence.outcomes.insert(Outcome::Bug);
                    evidence.unphased.variants.insert(Variant::Bug);
                    kind(error.kind(), &mut evidence.unphased, &mut budget);
                    continue;
                }
                _ => {
                    evidence.outcomes.insert(Outcome::Unknown);
                    unknown(&mut evidence.unphased, &mut budget);
                    continue;
                }
            };
            if sources.sources().next().is_none() {
                budget.coverage.insert(Gap::NoSources);
            }
            for source in sources.sources() {
                if !budget.visit(1) {
                    break;
                }
                match source {
                    DescUploadError::Circuit(error) => {
                        circuit(error, &mut evidence.circuit, &mut budget, 2)
                    }
                    DescUploadError::Stream(error) => {
                        circuit(error, &mut evidence.stream, &mut budget, 2)
                    }
                    DescUploadError::Request(error) => {
                        request(&error.error, &mut evidence.request, &mut budget, 2)
                    }
                    DescUploadError::Bug(error) => {
                        evidence.unphased.variants.insert(Variant::Bug);
                        kind(error.kind(), &mut evidence.unphased, &mut budget);
                    }
                    _ => unknown(&mut evidence.unphased, &mut budget),
                }
            }
        }
        evidence.coverage = budget.coverage;
        evidence
    }
}

fn circuit(error: &tor_circmgr::Error, phase: &mut Phase, budget: &mut Budget, depth: usize) {
    use tor_circmgr::Error as E;
    if !budget.visit(depth) {
        return;
    }
    let variant = match error {
        E::PendingFailed(inner) => {
            phase.variants.insert(Variant::PendingFailed);
            circuit(inner, phase, budget, depth + 1);
            return;
        }
        E::RequestFailed(sources) => {
            phase.variants.insert(Variant::RequestFailed);
            if sources.sources().next().is_none() {
                budget.coverage.insert(Gap::NoNestedSources);
            }
            for inner in sources.sources() {
                if budget.exhausted() {
                    budget.coverage.insert(Gap::Nodes);
                    break;
                }
                circuit(inner, phase, budget, depth + 1);
            }
            return;
        }
        E::GuardNotUsable(_) => Variant::GuardNotUsable,
        E::PendingCanceled => Variant::PendingCanceled,
        E::LostUsabilityRace(_) => Variant::LostUsabilityRace,
        E::CircCanceled => Variant::CircCanceled,
        E::UsageMismatched(_) => Variant::UsageMismatched,
        E::CircTimeout(_) => Variant::CircTimeout,
        E::RequestTimeout => Variant::RequestTimeout,
        E::NoRelay { .. } => Variant::NoRelay,
        E::GuardMgr(_) => Variant::GuardMgr,
        E::Guard(_) => Variant::Guard,
        E::VanguardMgrInit(_) => Variant::VanguardMgrInit,
        E::Channel { .. } => Variant::Channel,
        E::Protocol { .. } => Variant::Protocol,
        E::Spawn { .. } => Variant::Spawn,
        E::State(_) => Variant::State,
        E::Bug(_) => Variant::Bug,
        _ => {
            unknown(phase, budget);
            return;
        }
    };
    phase.variants.insert(variant);
    // Never call recursive HasKind on PendingFailed/RequestFailed: those are
    // traversed above under the same budget, collecting constituent kinds.
    kind(error.kind(), phase, budget);
}

fn request(
    error: &tor_dirclient::RequestError,
    phase: &mut Phase,
    budget: &mut Budget,
    depth: usize,
) {
    use tor_dirclient::RequestError as E;
    if !budget.visit(depth) {
        return;
    }
    let variant = match error {
        E::Tunnel(inner) => {
            phase.variants.insert(Variant::Tunnel);
            circuit(inner, phase, budget, depth + 1);
            return;
        }
        E::DirTimeout => Variant::DirTimeout,
        E::TruncatedHeaders => Variant::TruncatedHeaders,
        E::ResponseTooLong(_) => Variant::ResponseTooLong,
        E::HeadersTooLong(_) => Variant::HeadersTooLong,
        E::Utf8Encoding(_) => Variant::Utf8,
        E::IoError(error) => {
            io_kind(error.kind(), phase, budget);
            Variant::Io
        }
        E::Proto(_) => Variant::Proto,
        E::HttparseError(_) => Variant::HttpParse,
        E::HttpError(_) => Variant::HttpConstruct,
        E::ContentEncoding(_) => Variant::Encoding,
        E::TooMuchClockSkew => Variant::ClockSkew,
        E::EmptyRequest => Variant::EmptyRequest,
        E::EmptyResponse => Variant::EmptyResponse,
        E::HttpStatus(status, _) => {
            if !phase.http_statuses.insert(*status) {
                phase.variants.insert(Variant::InvalidStatus);
            }
            Variant::HttpStatus
        }
        _ => {
            unknown(phase, budget);
            return;
        }
    };
    phase.variants.insert(variant);
    kind(error.kind(), phase, budget);
}

fn unknown(phase: &mut Phase, budget: &mut Budget) {
    budget.coverage.insert(Gap::Unknown);
    phase.variants.insert(Variant::Unknown);
    phase.kinds.insert(Kind::Unknown);
}

fn kind(value: ErrorKind, phase: &mut Phase, budget: &mut Budget) {
    use ErrorKind as E;
    macro_rules! project { ($($variant:ident),+ $(,)?) => { match value {
        $(E::$variant => Kind::$variant,)+
        _ => { budget.coverage.insert(Gap::UnknownKind); Kind::Unknown }
    } }; }
    phase.kinds.insert(project!(
        TorAccessFailed,
        BootstrapRequired,
        DirectoryExpired,
        PersistentStateAccessFailed,
        LocalResourceAlreadyInUse,
        FsPermissions,
        PersistentStateCorrupted,
        CacheCorrupted,
        CacheAccessFailed,
        KeystoreCorrupted,
        KeystoreAccessFailed,
        ReactorShuttingDown,
        ArtiShuttingDown,
        SoftwareDeprecated,
        RemoteNetworkTimeout,
        InvalidConfig,
        InvalidConfigTransition,
        NoHomeDirectory,
        NotImplemented,
        FeatureDisabled,
        LocalProtocolViolation,
        TorProtocolViolation,
        LocalNetworkError,
        LocalResourceExhausted,
        ExternalToolFailed,
        RelayIdMismatch,
        CircuitCollapse,
        TorNetworkTimeout,
        TorDirectoryError,
        RemoteStreamClosed,
        RemoteStreamReset,
        RemoteStreamError,
        RemoteConnectionRefused,
        ExitPolicyRejected,
        ExitTimeout,
        RemoteNetworkFailed,
        RemoteHostNotFound,
        RemoteHostResolutionFailed,
        RemoteProtocolViolation,
        RelayTooBusy,
        InvalidStreamTarget,
        ForbiddenStreamTarget,
        TransientFailure,
        BadApiUsage,
        CircuitRefused,
        NoPath,
        NoExit,
        TorDirectoryUnusable,
        ClockSkew,
        TorDocumentRejected,
        Internal,
        Other
    ));
}

fn io_kind(value: std::io::ErrorKind, phase: &mut Phase, budget: &mut Budget) {
    use std::io::ErrorKind as E;
    macro_rules! project { ($($variant:ident),+ $(,)?) => { match value {
        $(E::$variant => IoKind::$variant,)+
        _ => { budget.coverage.insert(Gap::UnknownIo); IoKind::Unknown }
    } }; }
    phase.io_kinds.insert(project!(
        NotFound,
        PermissionDenied,
        ConnectionRefused,
        ConnectionReset,
        HostUnreachable,
        NetworkUnreachable,
        ConnectionAborted,
        NotConnected,
        AddrInUse,
        AddrNotAvailable,
        NetworkDown,
        BrokenPipe,
        AlreadyExists,
        WouldBlock,
        NotADirectory,
        IsADirectory,
        DirectoryNotEmpty,
        ReadOnlyFilesystem,
        StaleNetworkFileHandle,
        InvalidInput,
        InvalidData,
        TimedOut,
        WriteZero,
        StorageFull,
        NotSeekable,
        QuotaExceeded,
        FileTooLarge,
        ResourceBusy,
        ExecutableFileBusy,
        Deadlock,
        CrossesDevices,
        TooManyLinks,
        InvalidFilename,
        ArgumentListTooLong,
        Interrupted,
        Unsupported,
        UnexpectedEof,
        OutOfMemory,
        Other
    ));
}

#[cfg(test)]
mod tests;
