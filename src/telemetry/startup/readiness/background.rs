//! Debug-only, non-request-specific SDK observations; never a readiness gate.
//!
//! Parent integration must feature-gate this module. See PRIVACY.md's debug-only
//! Tor background exception, forbidden-data, processor/retention and required-
//! evidence restrictions. Activation and release qualification belong to parent.
//! One Copy projection coalesces updates, including terminal updates. Transitions
//! can be missed, and the last delivered snapshot is not current reachability
//! evidence. Dropping the owner/future discards pending records and owned I/O;
//! there is no guaranteed fatal upload, shutdown flush, retry or detached work.
use super::super::{
    attribute, resource, scope, send_payload, transport, AnyValue, Client, Config, Delivery,
    Duration, ExportLogsServiceRequest, LogRecord, Message, ResourceLogs, ScopeLogs,
    SeverityNumber, Signal, Value,
};
use super::{platform, ProblemKind, State, Terminal};
use std::{fmt, future::Future};
use tokio::{sync::watch, time::Instant};
use tor_hsservice::{
    status::{DescUploadRetryError, Problem},
    DescUploadError, FatalError, IptError,
};

const EVENT: &str = "possums.tor_background";
const PROFILE: &str = "debug_tor_readiness";
const INTERVAL: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    NotObserved,
    Observed,
    Ended,
    Panicked,
}
impl Source {
    fn label(self) -> &'static str {
        match self {
            Self::NotObserved => "not_observed",
            Self::Observed => "observed",
            Self::Ended => "ended",
            Self::Panicked => "panicked",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fraction {
    Unknown,
    UnderQuarter,
    QuarterToHalf,
    HalfToThreeQuarters,
    ThreeQuartersToComplete,
    Complete,
}
impl Fraction {
    fn from_sdk(value: f32) -> Self {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            Self::Unknown
        } else if value < 0.25 {
            Self::UnderQuarter
        } else if value < 0.5 {
            Self::QuarterToHalf
        } else if value < 0.75 {
            Self::HalfToThreeQuarters
        } else if value < 1.0 {
            Self::ThreeQuartersToComplete
        } else {
            Self::Complete
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::UnderQuarter => "under_quarter",
            Self::QuarterToHalf => "quarter_to_half",
            Self::HalfToThreeQuarters => "half_to_three_quarters",
            Self::ThreeQuartersToComplete => "three_quarters_to_complete",
            Self::Complete => "complete",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Blockage {
    None,
    Disabled,
    Offline,
    Filtering,
    CannotReachTor,
    ClockSkewed,
    CannotBootstrap,
    Unknown,
}
impl Blockage {
    fn from_sdk(kind: arti_client::status::BlockageKind) -> Self {
        use arti_client::status::BlockageKind as K;
        match kind {
            K::Disabled => Self::Disabled,
            K::Offline => Self::Offline,
            K::Filtering => Self::Filtering,
            K::CantReachTor => Self::CannotReachTor,
            K::ClockSkewed => Self::ClockSkewed,
            K::CantBootstrap => Self::CannotBootstrap,
            _ => Self::Unknown,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Disabled => "disabled",
            Self::Offline => "offline",
            Self::Filtering => "filtering",
            Self::CannotReachTor => "cannot_reach_tor",
            Self::ClockSkewed => "clock_skewed",
            Self::CannotBootstrap => "cannot_bootstrap",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Runtime {
    None,
    Spawn,
    Keystore,
    KeystoreRace,
    MissingIdentity,
    UnexpectedIptKeys,
    DirectoryShutdown,
    MissingField,
    DescriptorTooLong,
    Bug,
    Unknown,
}
impl Runtime {
    fn from_sdk(error: &FatalError) -> Self {
        match error {
            FatalError::Spawn { .. } => Self::Spawn,
            FatalError::Keystore(_) => Self::Keystore,
            FatalError::KeystoreRace { .. } => Self::KeystoreRace,
            FatalError::MissingHsIdKeypair(_) => Self::MissingIdentity,
            FatalError::IptKeysFoundUnexpectedly(_) => Self::UnexpectedIptKeys,
            FatalError::NetdirProviderShutdown(_) => Self::DirectoryShutdown,
            FatalError::MissingField(_) => Self::MissingField,
            FatalError::HsDescTooLong => Self::DescriptorTooLong,
            FatalError::Bug(_) => Self::Bug,
            _ => Self::Unknown,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Spawn => "spawn",
            Self::Keystore => "keystore",
            Self::KeystoreRace => "keystore_race",
            Self::MissingIdentity => "missing_identity",
            Self::UnexpectedIptKeys => "unexpected_ipt_keys",
            Self::DirectoryShutdown => "directory_shutdown",
            Self::MissingField => "missing_field",
            Self::DescriptorTooLong => "descriptor_too_long",
            Self::Bug => "bug",
            Self::Unknown => "unknown",
        }
    }
}

// Fixed presence bits, never counts. An absent bit means not observed in the
// selected SDK problem, not a healthy subsystem or a complete error inventory.
const UPLOAD_FATAL: u16 = 1 << 0;
const UPLOAD_MAX_RETRY: u16 = 1 << 1;
const UPLOAD_TIMEOUT: u16 = 1 << 2;
const UPLOAD_BUG: u16 = 1 << 3;
const UPLOAD_UNKNOWN: u16 = 1 << 4;
const DESC_REQUEST: u16 = 1 << 5;
const DESC_CIRCUIT: u16 = 1 << 6;
const DESC_STREAM: u16 = 1 << 7;
const DESC_BUG: u16 = 1 << 8;
const DESC_UNKNOWN: u16 = 1 << 9;
const IPT_NOT_LISTED: u16 = 1 << 10;
const IPT_BAD_ESTABLISHED: u16 = 1 << 11;
const IPT_BAD_MESSAGE: u16 = 1 << 12;
const IPT_TIMEOUT: u16 = 1 << 13;
const IPT_UNKNOWN: u16 = 1 << 14;
const FLAGS: [(&str, u16); 15] = [
    ("upload_fatal", UPLOAD_FATAL),
    ("upload_max_retry", UPLOAD_MAX_RETRY),
    ("upload_timeout", UPLOAD_TIMEOUT),
    ("upload_bug", UPLOAD_BUG),
    ("upload_unknown", UPLOAD_UNKNOWN),
    ("desc_request", DESC_REQUEST),
    ("desc_circuit", DESC_CIRCUIT),
    ("desc_stream", DESC_STREAM),
    ("desc_bug", DESC_BUG),
    ("desc_unknown", DESC_UNKNOWN),
    ("ipt_not_listed", IPT_NOT_LISTED),
    ("ipt_bad_established", IPT_BAD_ESTABLISHED),
    ("ipt_bad_message", IPT_BAD_MESSAGE),
    ("ipt_timeout", IPT_TIMEOUT),
    ("ipt_unknown", IPT_UNKNOWN),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SelectedProblem {
    kind: ProblemKind,
    runtime: Runtime,
    flags: u16,
}
impl SelectedProblem {
    fn from_sdk(problem: Option<&Problem>) -> Self {
        let mut selected = Self {
            kind: ProblemKind::from_sdk(problem),
            runtime: Runtime::None,
            flags: 0,
        };
        match problem {
            Some(Problem::Runtime(error)) => selected.runtime = Runtime::from_sdk(error),
            Some(Problem::DescriptorUpload(errors)) => {
                if errors.is_empty() {
                    selected.flags |= UPLOAD_UNKNOWN;
                }
                for error in errors {
                    let (outer, causes) = match error {
                        DescUploadRetryError::FatalError(causes) => (UPLOAD_FATAL, Some(causes)),
                        DescUploadRetryError::MaxRetryCountExceeded(causes) => {
                            (UPLOAD_MAX_RETRY, Some(causes))
                        }
                        DescUploadRetryError::Timeout(causes) => (UPLOAD_TIMEOUT, Some(causes)),
                        DescUploadRetryError::Bug(_) => (UPLOAD_BUG, None),
                        _ => (UPLOAD_UNKNOWN, None),
                    };
                    selected.flags |= outer;
                    if let Some(causes) = causes {
                        if causes.is_empty() {
                            selected.flags |= DESC_UNKNOWN;
                        }
                        for cause in causes.sources() {
                            selected.flags |= descriptor_flag(cause);
                        }
                    }
                }
            }
            Some(Problem::Ipt(errors)) => {
                if errors.is_empty() {
                    selected.flags |= IPT_UNKNOWN;
                }
                for error in errors {
                    selected.flags |= match error {
                        IptError::IntroPointNotListed => IPT_NOT_LISTED,
                        IptError::BadEstablished => IPT_BAD_ESTABLISHED,
                        IptError::BadMessage(_) => IPT_BAD_MESSAGE,
                        IptError::Timeout => IPT_TIMEOUT,
                        _ => IPT_UNKNOWN,
                    };
                }
            }
            _ => {}
        }
        selected
    }
}

fn descriptor_flag(cause: &DescUploadError) -> u16 {
    match cause {
        DescUploadError::Request(_) => DESC_REQUEST,
        DescUploadError::Circuit(_) => DESC_CIRCUIT,
        DescUploadError::Stream(_) => DESC_STREAM,
        DescUploadError::Bug(_) => DESC_BUG,
        _ => DESC_UNKNOWN,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Snapshot {
    onion: State,
    fully_reachable: bool,
    problem: SelectedProblem,
    source: Source,
    terminal: Option<Terminal>,
    bootstrap: Fraction,
    bootstrap_observed: bool,
    bootstrap_ready: bool,
    blockage: Blockage,
}
impl Snapshot {
    fn initial() -> Self {
        Self {
            onion: State::NotObserved,
            fully_reachable: false,
            problem: SelectedProblem::from_sdk(None),
            source: Source::NotObserved,
            terminal: None,
            bootstrap: Fraction::Unknown,
            bootstrap_observed: false,
            bootstrap_ready: false,
            blockage: Blockage::Unknown,
        }
    }
    fn observed(self) -> bool {
        self.source != Source::NotObserved || self.bootstrap_observed
    }
    fn severity(self) -> (SeverityNumber, &'static str) {
        if self.terminal.is_some()
            || self.problem.kind != ProblemKind::None
            || !matches!(
                self.blockage,
                Blockage::None | Blockage::Unknown | Blockage::Disabled
            )
            || matches!(self.onion, State::Broken | State::Unknown)
            || (self.bootstrap_observed
                && (self.blockage == Blockage::Unknown || self.bootstrap == Fraction::Unknown))
        {
            (SeverityNumber::Error, "ERROR")
        } else if matches!(
            self.onion,
            State::DegradedReachable | State::DegradedUnreachable | State::Recovering
        ) {
            (SeverityNumber::Warn, "WARN")
        } else {
            (SeverityNumber::Info, "INFO")
        }
    }
    fn reachability_label(self) -> &'static str {
        if self.source == Source::Observed {
            boolean(self.fully_reachable)
        } else {
            "unknown"
        }
    }
    fn bootstrap_ready_label(self) -> &'static str {
        if self.bootstrap_observed {
            boolean(self.bootstrap_ready)
        } else {
            "unknown"
        }
    }
    fn problem_label(self) -> &'static str {
        if self.onion == State::NotObserved {
            "not_observed"
        } else {
            self.problem.kind.label()
        }
    }
    fn action(self) -> &'static str {
        if self.terminal.is_some() {
            return "check_tor_engine_lifecycle_offline";
        }
        match self.problem.kind {
            ProblemKind::Runtime => "check_approved_tor_key_configuration_and_runtime_offline",
            ProblemKind::DescriptorUpload => "check_tor_descriptor_upload_connectivity_offline",
            ProblemKind::Ipt => "check_tor_introduction_point_establishment_offline",
            ProblemKind::Unknown => "report_safe_tor_code_and_category",
            ProblemKind::None => "check_tor_bootstrap_configuration_and_connectivity_offline",
        }
    }
    fn code(self) -> &'static str {
        match self.terminal {
            Some(Terminal::StatusEnded) => "tor_background_status_ended",
            Some(Terminal::StatusPanicked) => "tor_background_status_panicked",
            Some(_) => "tor_background_engine_failure",
            None if self.severity().0 == SeverityNumber::Error => "tor_background_problem",
            None if self.severity().0 == SeverityNumber::Warn => "tor_background_degraded",
            None => "tor_background_snapshot",
        }
    }
}
impl fmt::Display for Snapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Compact enough for the existing 1024-byte nonblocking platform sink.
        // Full named presence flags are retained in the sealed OTLP attributes.
        write!(f, "event={EVENT} code={} stage=tor_background diagnostic_profile={PROFILE} onion_state={} sdk_fully_reachable={} problem_kind={} runtime_kind={} source_status={} bootstrap_fraction={} bootstrap_ready={} blockage={} flags=", self.code(), self.onion.label(), self.reachability_label(), self.problem_label(), self.problem.runtime.label(), self.source.label(), self.bootstrap.label(), self.bootstrap_ready_label(), self.blockage.label())?;
        if self.problem.flags == 0 {
            f.write_str("not_observed")?;
        } else {
            for (label, bit) in FLAGS {
                if self.problem.flags & bit != 0 {
                    write!(f, "{label},")?;
                }
            }
        }
        write!(f, " constraint=selected_sdk_snapshot_not_current_reachability_proof action={}; coalesced; diagnostic_only; no retry or service_outcome_change", self.action())
    }
}
fn boolean(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}

/// Observer inputs are immediately projected. No SDK object, string or timestamp
/// enters the single watch/latest cell. Clones do not add delivery workers.
#[derive(Clone)]
pub(super) struct Observer {
    snapshot: watch::Sender<Snapshot>,
}
impl Observer {
    pub(super) fn onion(&self, status: &tor_hsservice::status::OnionServiceStatus) {
        self.onion_parts(status.state(), status.current_problem());
    }
    fn onion_parts(&self, state: tor_hsservice::status::State, problem: Option<&Problem>) {
        let problem = SelectedProblem::from_sdk(problem);
        self.update(|current| {
            // A failed source cannot silently recover from a stray late status.
            if current.terminal.is_none() {
                current.onion = state.into();
                current.fully_reachable = state.is_fully_reachable();
                current.problem = problem;
                current.source = Source::Observed;
            }
        });
    }
    pub(super) fn bootstrap(&self, status: &arti_client::status::BootstrapStatus) {
        let fraction = Fraction::from_sdk(status.as_frac());
        let ready = status.ready_for_traffic();
        // blocked() constructs an SDK-owned diagnostic message internally. Only
        // its supported typed kind is read; the message is never formatted here,
        // retained in the cell, sent to platform output or passed to the encoder.
        let blockage = status
            .blocked()
            .map_or(Blockage::None, |b| Blockage::from_sdk(b.kind()));
        self.update(|current| {
            current.bootstrap = fraction;
            current.bootstrap_observed = true;
            current.bootstrap_ready = ready;
            current.blockage = blockage;
        });
    }
    pub(super) fn failure(&self, terminal: Terminal) {
        let source = match terminal {
            Terminal::EngineFailure | Terminal::StatusEnded => Source::Ended,
            Terminal::StatusPanicked => Source::Panicked,
            Terminal::FirstLocalReady | Terminal::Unconfirmed => return,
        };
        self.update(|current| {
            if current.terminal.is_none() {
                current.source = source;
                current.terminal = Some(terminal);
            }
        });
    }
    fn update(&self, update: impl FnOnce(&mut Snapshot)) {
        self.snapshot.send_if_modified(|current| {
            let previous = *current;
            update(current);
            *current != previous
        });
    }
}

pub(super) fn new(config: Config) -> (Observer, impl Future<Output = ()>) {
    let (snapshot, receiver) = watch::channel(Snapshot::initial());
    (Observer { snapshot }, async move {
        // Off constructs no encoder, TLS/client/network or background output.
        run(receiver, config.into_client()).await;
    })
}

#[derive(Default)]
struct Local {
    #[cfg(test)]
    records: std::sync::Mutex<Vec<String>>,
}
impl Local {
    fn emit(&self, record: fmt::Arguments<'_>) {
        platform::emit(record);
        #[cfg(test)]
        self.records.lock().unwrap().push(record.to_string());
    }
}
fn warn(local: &Local, warned: &mut bool, failure: Delivery) {
    if !*warned {
        *warned = true;
        local.emit(format_args!("code=tor_background_delivery_unconfirmed stage=tor_background_delivery diagnostic_profile={PROFILE} constraint={} action=check_telemetry_ingestion_configuration_and_connectivity_offline; delivery unknown; not retried; original Tor and service outcome unchanged", failure.constraint));
    }
}
async fn run(
    snapshot: watch::Receiver<Snapshot>,
    client: Result<Option<Client>, transport::Failure>,
) {
    run_with_local(snapshot, client, &Local::default()).await;
}
async fn run_with_local(
    mut snapshot: watch::Receiver<Snapshot>,
    client: Result<Option<Client>, transport::Failure>,
    local: &Local,
) {
    let client = match client {
        Ok(Some(client)) => client.for_signal(Signal::Logs),
        Ok(None) => return,
        Err(_) => {
            let mut warned = false;
            warn(
                local,
                &mut warned,
                Delivery {
                    signal: Signal::Logs,
                    constraint: "transport_configuration",
                },
            );
            return;
        }
    };
    let mut warned = false;
    let mut attempted = None;
    let mut next = Instant::now();
    loop {
        // Owner/source closure need not flush pending records or delay drain.
        if snapshot.has_changed().is_err() {
            break;
        }
        let current = *snapshot.borrow_and_update();
        if current.observed() && attempted != Some(current) && Instant::now() >= next {
            next = Instant::now() + INTERVAL;
            attempted = Some(current); // Delivery failures are never retried.
            local.emit(format_args!("{current}"));
            if let Err(failure) = send_payload(
                &client,
                Signal::Logs,
                logs(current).encode_to_vec(),
                Instant::now() + Duration::from_secs(1),
            )
            .await
            {
                warn(local, &mut warned, failure);
            }
            continue;
        }
        if current.observed() && attempted != Some(current) {
            tokio::select! {
                _ = tokio::time::sleep_until(next) => {},
                changed = snapshot.changed() => if changed.is_err() { break; },
            }
        } else if snapshot.changed().await.is_err() {
            break;
        }
    }
    client.cancel().await;
}

// Sealed input: no arbitrary strings, identifiers, SDK errors or times. Fixed
// zero OTLP timestamps avoid exact observation timestamps; no metrics/trace IDs.
fn logs(record: Snapshot) -> ExportLogsServiceRequest {
    let mut attributes = vec![
        attribute("code", record.code()),
        attribute("stage", "tor_background"),
        attribute("diagnostic_profile", PROFILE),
        attribute(
            "constraint",
            "selected_sdk_snapshot_not_current_reachability_proof",
        ),
        attribute("action", record.action()),
        attribute("onion_state", record.onion.label()),
        attribute("sdk_fully_reachable", record.reachability_label()),
        attribute("problem_kind", record.problem_label()),
        attribute("runtime_kind", record.problem.runtime.label()),
        attribute("source_status", record.source.label()),
        attribute("bootstrap_fraction", record.bootstrap.label()),
        attribute("bootstrap_ready", record.bootstrap_ready_label()),
        attribute("blockage", record.blockage.label()),
    ];
    for (key, bit) in FLAGS {
        attributes.push(attribute(
            key,
            if record.problem.flags & bit != 0 {
                "observed"
            } else {
                "not_observed"
            },
        ));
    }
    let (severity, severity_text) = record.severity();
    ExportLogsServiceRequest {
        resource_logs: vec![ResourceLogs {
            resource: Some(resource()),
            scope_logs: vec![ScopeLogs {
                scope: Some(scope()),
                log_records: vec![LogRecord {
                    event_name: EVENT.into(),
                    severity_number: severity as i32,
                    severity_text: severity_text.into(),
                    body: Some(AnyValue {
                        value: Some(Value::StringValue(record.to_string())),
                    }),
                    attributes,
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }],
    }
}

#[cfg(test)]
mod tests;
