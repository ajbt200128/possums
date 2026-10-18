//! Finite post-launch observation, not a readiness gate or runtime logger.
use super::*;
use std::future::Future;
use tokio::{sync::watch, time::Instant};
use tor_hsservice::status::{Problem, State as OnionState};

const WAIT: Duration = Duration::from_secs(30);
const END: Duration = Duration::from_secs(180);
const EVENT: &str = "possums.startup_readiness";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum State {
    NotObserved,
    Shutdown,
    Bootstrapping,
    DegradedReachable,
    DegradedUnreachable,
    Running,
    Recovering,
    Broken,
    Unknown,
}
impl From<OnionState> for State {
    fn from(state: OnionState) -> Self {
        match state {
            OnionState::Shutdown => Self::Shutdown,
            OnionState::Bootstrapping => Self::Bootstrapping,
            OnionState::DegradedReachable => Self::DegradedReachable,
            OnionState::DegradedUnreachable => Self::DegradedUnreachable,
            OnionState::Running => Self::Running,
            OnionState::Recovering => Self::Recovering,
            OnionState::Broken => Self::Broken,
            _ => Self::Unknown,
        }
    }
}
impl State {
    fn label(self) -> &'static str {
        match self {
            Self::NotObserved => "not_observed",
            Self::Shutdown => "shutdown",
            Self::Bootstrapping => "bootstrapping",
            Self::DegradedReachable => "degraded_reachable",
            Self::DegradedUnreachable => "degraded_unreachable",
            Self::Running => "running",
            Self::Recovering => "recovering",
            Self::Broken => "broken",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProblemKind {
    None,
    Runtime,
    DescriptorUpload,
    Ipt,
    Unknown,
}
impl ProblemKind {
    pub(crate) fn from_sdk(problem: Option<&Problem>) -> Self {
        match problem {
            None => Self::None,
            Some(Problem::Runtime(_)) => Self::Runtime,
            Some(Problem::DescriptorUpload(_)) => Self::DescriptorUpload,
            Some(Problem::Ipt(_)) => Self::Ipt,
            // Includes feature-gated Pow (not enabled in the pinned SDK) and
            // future variants. Never format/retain the inner SDK error.
            Some(_) => Self::Unknown,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Runtime => "runtime",
            Self::DescriptorUpload => "descriptor_upload",
            Self::Ipt => "ipt",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Terminal {
    FirstLocalReady,
    EngineFailure,
    StatusEnded,
    StatusPanicked,
    Unconfirmed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Record {
    state: State,
    problem: ProblemKind,
    elapsed: &'static str,
    terminal: Option<Terminal>,
}
impl Record {
    fn outcome(self) -> &'static str {
        match self.terminal {
            None => "waiting",
            Some(Terminal::FirstLocalReady) => "first_local_ready",
            Some(Terminal::Unconfirmed) => "unconfirmed",
            Some(_) => "engine_failure",
        }
    }
    fn code(self) -> &'static str {
        match self.terminal {
            None => "tor_startup_readiness_waiting",
            Some(Terminal::FirstLocalReady) => "tor_startup_first_local_ready",
            Some(Terminal::EngineFailure) => "tor_startup_engine_failure",
            Some(Terminal::StatusEnded) => "tor_startup_status_ended",
            Some(Terminal::StatusPanicked) => "tor_startup_status_panicked",
            Some(Terminal::Unconfirmed) => "tor_startup_readiness_unconfirmed",
        }
    }
    fn constraint(self) -> &'static str {
        match self.terminal {
            Some(Terminal::FirstLocalReady) => {
                "observed_sdk_running_once_not_current_health_or_verified_serving"
            }
            Some(Terminal::EngineFailure | Terminal::StatusEnded | Terminal::StatusPanicked) => {
                "observed_engine_failure_cause_unknown"
            }
            _ => "first_sdk_running_not_observed_within_diagnostic_window",
        }
    }
}
impl fmt::Display for Record {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "event={EVENT} outcome={} code={} onion_state={} problem_kind={} elapsed_bucket={} constraint={} action=review_startup_evidence_offline; diagnostic_only; no retry or service_outcome_change", self.outcome(), self.code(), self.state.label(), self.problem.label(), self.elapsed, self.constraint())
    }
}

fn elapsed(start: Instant) -> &'static str {
    match Instant::now().saturating_duration_since(start).as_secs() {
        0..30 => "under_30s",
        30..60 => "30s_to_under_60s",
        60..180 => "60s_to_under_180s",
        _ => "180s_or_more",
    }
}

#[derive(Clone, Copy)]
struct Snapshot {
    latest: Record,
    first: Option<Record>,
}
/// Only closed values enter this single bounded cell. Latch before coalescing:
/// Running followed by Broken during a stalled waiting export stays first-ready.
pub(crate) struct Observer {
    start: Instant,
    snapshot: watch::Sender<Snapshot>,
}
impl Observer {
    pub(crate) fn observe(
        &self,
        state: OnionState,
        problem: ProblemKind,
        terminal: Option<Terminal>,
    ) {
        self.update(Some((state.into(), problem)), terminal);
    }
    pub(crate) fn failure(&self, terminal: Terminal) {
        self.update(None, Some(terminal));
    }
    fn update(&self, status: Option<(State, ProblemKind)>, terminal: Option<Terminal>) {
        if Instant::now() >= self.start + END {
            return;
        }
        self.snapshot.send_if_modified(|snapshot| {
            if snapshot.first.is_some() {
                return false;
            }
            if let Some((state, problem)) = status {
                snapshot.latest.state = state;
                snapshot.latest.problem = problem;
            }
            snapshot.latest.elapsed = elapsed(self.start);
            snapshot.latest.terminal = terminal;
            if terminal.is_some() {
                snapshot.first = Some(snapshot.latest);
            }
            true
        });
    }
}

pub(crate) fn new(config: Config) -> (Observer, impl Future<Output = ()>) {
    let start = Instant::now();
    let (snapshot, receiver) = watch::channel(Snapshot {
        latest: Record {
            state: State::NotObserved,
            problem: ProblemKind::None,
            elapsed: "under_30s",
            terminal: None,
        },
        first: None,
    });
    (Observer { start, snapshot }, async move {
        // Disabled config never constructs transport/TLS or performs HTTP.
        let client = config.into_client();
        run(start, receiver, client).await;
    })
}

async fn run(
    start: Instant,
    mut snapshot: watch::Receiver<Snapshot>,
    client: Result<Option<Client>, transport::Failure>,
) {
    let mut waiting_sent = false;
    loop {
        let current = *snapshot.borrow_and_update();
        let record = if let Some(first) = current.first {
            Some(first)
        } else if Instant::now() >= start + END {
            Some(Record {
                terminal: Some(Terminal::Unconfirmed),
                elapsed: elapsed(start),
                ..current.latest
            })
        } else if !waiting_sent && Instant::now() >= start + WAIT {
            waiting_sent = true;
            Some(Record {
                elapsed: elapsed(start),
                ..current.latest
            })
        } else {
            None
        };
        if let Some(record) = record {
            deliver_record(&client, record).await;
            if record.terminal.is_some() {
                break;
            }
        }
        let deadline = start + if waiting_sent { END } else { WAIT };
        tokio::select! {
            _ = tokio::time::sleep_until(deadline) => {},
            changed = snapshot.changed() => {
                if changed.is_err() { break; }
            },
        }
    }
    if let Ok(Some(client)) = client {
        client.cancel().await;
    }
}

fn logs(record: Record) -> ExportLogsServiceRequest {
    ExportLogsServiceRequest {
        resource_logs: vec![ResourceLogs {
            resource: Some(resource()),
            scope_logs: vec![ScopeLogs {
                scope: Some(scope()),
                log_records: vec![LogRecord {
                    event_name: EVENT.into(),
                    severity_number: SeverityNumber::Info as i32,
                    severity_text: "INFO".into(),
                    body: Some(AnyValue {
                        value: Some(Value::StringValue(record.to_string())),
                    }),
                    attributes: vec![
                        attribute("outcome", record.outcome()),
                        attribute("code", record.code()),
                        attribute("onion_state", record.state.label()),
                        attribute("problem_kind", record.problem.label()),
                        attribute("elapsed_bucket", record.elapsed),
                        attribute("constraint", record.constraint()),
                        attribute("action", "review_startup_evidence_offline"),
                    ],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }],
    }
}
async fn deliver_record(client: &Result<Option<Client>, transport::Failure>, record: Record) {
    let _ = writeln!(std::io::stderr().lock(), "{record}");
    let Ok(Some(client)) = client else {
        return;
    };
    let client = client.clone().for_signal(Signal::Logs);
    // The shared sender classifies delivery faults, but this observer emits no
    // extra delivery-warning records: its entire stderr/OTLP budget is two.
    // Missing ingestion is unavailable, never evidence of success or health.
    let _ = send_payload(
        &client,
        Signal::Logs,
        logs(record).encode_to_vec(),
        Instant::now() + Duration::from_secs(1),
    )
    .await;
}

#[cfg(test)]
pub(crate) mod tests;
