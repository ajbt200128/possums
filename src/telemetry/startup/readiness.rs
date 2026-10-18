//! Finite post-launch observation, not a readiness gate or runtime logger.
use super::*;
use std::future::Future;
use tokio::{sync::watch, time::Instant};
use tor_hsservice::status::{Problem, State as OnionState};

mod platform;

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
#[derive(Clone)]
pub(crate) struct Observer {
    start: Instant,
    snapshot: watch::Sender<Snapshot>,
    local: std::sync::Arc<Local>,
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
                // Commit the local attempt at observation, before serving can
                // complete in this same poll and drop the owned upload future.
                self.local.emit(format_args!("{}", snapshot.latest));
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
    let observer = Observer {
        start,
        snapshot,
        local: Default::default(),
    };
    let worker = observer.clone();
    (observer, async move {
        // Disabled config never constructs transport/TLS or performs HTTP.
        run(worker, receiver, config.into_client()).await;
    })
}

async fn run(
    observer: Observer,
    mut snapshot: watch::Receiver<Snapshot>,
    client: Result<Option<Client>, transport::Failure>,
) {
    let start = observer.start;
    let mut warned = false;
    if client.is_err() {
        warn(
            &observer.local,
            &mut warned,
            Delivery {
                signal: Signal::Logs,
                constraint: "transport_configuration",
            },
        );
    }
    let mut waiting_sent = false;
    loop {
        snapshot.borrow_and_update();
        let mut record = None;
        // Serialize the waiting/deadline local attempt with observation. Waiting
        // can never follow a committed terminal, even across threads.
        observer.snapshot.send_if_modified(|current| {
            if let Some(first) = current.first {
                record = Some(first); // Local attempt already committed.
            } else if Instant::now() >= start + END {
                let terminal = Record {
                    terminal: Some(Terminal::Unconfirmed),
                    elapsed: elapsed(start),
                    ..current.latest
                };
                current.first = Some(terminal);
                observer.local.emit(format_args!("{terminal}"));
                record = Some(terminal);
                return true;
            } else if !waiting_sent && Instant::now() >= start + WAIT {
                waiting_sent = true;
                let waiting = Record {
                    elapsed: elapsed(start),
                    ..current.latest
                };
                observer.local.emit(format_args!("{waiting}"));
                record = Some(waiting);
            }
            false
        });
        if let Some(record) = record {
            let stop = start
                + END
                + if record.terminal.is_some() {
                    Duration::from_secs(1)
                } else {
                    Duration::ZERO
                };
            let deadline = (Instant::now() + Duration::from_secs(1)).min(stop);
            if let Err(failure) = deliver_record(&client, record, deadline).await {
                warn(&observer.local, &mut warned, failure);
            }
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
fn warn(local: &Local, warned: &mut bool, failure: Delivery) {
    if *warned {
        return;
    }
    *warned = true;
    // Reuse the sender's closed classification, never format its raw cause or
    // recursively export this warning. No HTTP status survives that boundary.
    local.emit(format_args!("code=tor_startup_readiness_delivery_unconfirmed stage=tor_startup_readiness_delivery constraint={} action=check_telemetry_ingestion_configuration_and_connectivity_offline; delivery unknown; not retried; original readiness and service outcome unchanged", failure.constraint));
}

async fn deliver_record(
    client: &Result<Option<Client>, transport::Failure>,
    record: Record,
    deadline: Instant,
) -> Result<(), Delivery> {
    let Ok(Some(client)) = client else {
        return Ok(());
    };
    let client = client.clone().for_signal(Signal::Logs);
    send_payload(
        &client,
        Signal::Logs,
        logs(record).encode_to_vec(),
        deadline,
    )
    .await
}

#[cfg(test)]
mod delivery_tests;
#[cfg(test)]
pub(crate) mod tests;
