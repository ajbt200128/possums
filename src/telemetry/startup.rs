//! Finite startup-only observations. No logger provider, context capture, request
//! hook, resource discovery or generic diagnostic input. See PRIVACY.md's startup
//! exception; HTTP acceptance does not establish ingestion, retention or serving.
use super::{
    export::transport::{self, Client, Signal},
    runtime::Config,
};
use crate::{inference::InferenceFailure, tor_runtime::TorStartupFailure};
use opentelemetry_http::HttpClient;
use opentelemetry_proto::tonic::{
    collector::{
        logs::v1::{ExportLogsServiceRequest, ExportLogsServiceResponse},
        metrics::v1::{ExportMetricsServiceRequest, ExportMetricsServiceResponse},
    },
    common::v1::{any_value::Value, AnyValue, InstrumentationScope, KeyValue},
    logs::v1::{LogRecord, ResourceLogs, ScopeLogs, SeverityNumber},
    metrics::v1::{
        metric::Data, number_data_point, AggregationTemporality, Metric, NumberDataPoint,
        ResourceMetrics, ScopeMetrics, Sum,
    },
    resource::v1::Resource,
};
use prost::Message;
use std::{
    fmt,
    io::Write,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

// The transport already caps each signal attempt at one second. Reserve both
// sequential attempts so stalled logs cannot consume metrics' only opportunity.
const BUDGET: Duration = Duration::from_secs(2);
const MAX_RECORDS: usize = 9; // eight distinct stages and one terminal
const EVENT: &str = "possums.startup";

#[cfg(feature = "tor")]
pub(crate) mod readiness;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Configuration,
    Accounts,
    Signal,
    TorPreparation,
    Connect,
    Bind,
    TorBootstrap,
    TorLaunch,
}
impl Stage {
    fn label(self) -> &'static str {
        match self {
            Self::Configuration => "configuration",
            Self::Accounts => "accounts",
            Self::Signal => "signal",
            Self::TorPreparation => "tor_preparation",
            Self::Connect => "connect",
            Self::Bind => "bind",
            Self::TorBootstrap => "tor_bootstrap",
            Self::TorLaunch => "tor_launch",
        }
    }
}

/// All nested fields were reviewed: InferenceFailure is unit variants;
/// TorStartupFailure contains only closed enums;
/// ServeFailure contains only closed optional causes.
/// Never replace these with SDK/io errors or a string-bearing wrapper.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    ConfigurationMissing,
    ConfigurationEncodingInvalid,
    AccountsInvalid,
    SignalUnknown,
    Connect(InferenceFailure),
    BindUnknown,
    Tor(TorStartupFailure),
    #[cfg(feature = "tor")]
    TorLaunch(crate::tor_ingress::ServeFailure),
}
impl Failure {
    fn diagnostic(self) -> Diagnostic {
        let (code, constraint, action) = match self {
            Self::ConfigurationMissing => (
                "startup_configuration_missing",
                "required_configuration",
                "provide_approved_configuration_offline",
            ),
            Self::ConfigurationEncodingInvalid => (
                "startup_configuration_encoding_invalid",
                "unicode_configuration",
                "check_configuration_encoding_offline",
            ),
            Self::AccountsInvalid => (
                "startup_accounts_invalid",
                "validated_demo_accounts",
                "check_approved_account_configuration_offline",
            ),
            Self::SignalUnknown => (
                "startup_signal_unknown",
                "signal_registration_cause_unknown",
                "check_host_signal_support_offline",
            ),
            Self::Connect(_) => (
                "startup_connect_failed",
                "verified_upstream_connection",
                "check_verified_upstream_and_catalog_offline",
            ),
            Self::BindUnknown => (
                "startup_bind_unknown",
                "tcp_listener_bind_cause_unknown",
                "check_approved_listener_configuration_offline",
            ),
            Self::Tor(_) => (
                "startup_tor_failed",
                "approved_tor_initialization",
                "check_approved_tor_configuration_and_evidence_offline",
            ),
            #[cfg(feature = "tor")]
            Self::TorLaunch(_) => (
                "startup_tor_launch_failed",
                "approved_tor_launch",
                "check_approved_tor_host_state_offline",
            ),
        };
        Diagnostic {
            code,
            constraint,
            action,
        }
    }
}
impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let d = self.diagnostic();
        write!(
            f,
            "code={} constraint={} action={}",
            d.code, d.constraint, d.action
        )?;
        match self {
            // Debug retains distinctions hidden by the shipped serde aliases.
            Self::Connect(cause) => write!(f, " cause={cause:?}; {cause}"),
            Self::Tor(cause) => write!(f, "; {cause}"),
            #[cfg(feature = "tor")]
            Self::TorLaunch(cause) => write!(f, "; {cause}"),
            Self::SignalUnknown | Self::BindUnknown => f.write_str(" cause=unknown"),
            _ => Ok(()),
        }
    }
}
impl std::error::Error for Failure {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    InitializationComplete,
    Interrupted,
    Failed(Failure),
}

#[derive(Clone, Copy)]
struct Diagnostic {
    code: &'static str,
    constraint: &'static str,
    action: &'static str,
}
#[derive(Clone, Copy)]
struct Observation {
    stage: Stage,
    outcome: Option<Outcome>,
    time: u64,
}
impl Observation {
    fn new(stage: Stage, outcome: Option<Outcome>) -> Self {
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|time| time.as_nanos().try_into().ok())
            .unwrap_or(0);
        Self {
            stage,
            outcome,
            time,
        }
    }
    fn label(self) -> &'static str {
        match self.outcome {
            None => "entered",
            Some(Outcome::InitializationComplete) => "initialization_complete",
            Some(Outcome::Interrupted) => "interrupted",
            Some(Outcome::Failed(_)) => "failed",
        }
    }
    fn diagnostic(self) -> Diagnostic {
        let (code, constraint, action) = match self.outcome {
            None => (
                "startup_stage_entered",
                "initialization_only",
                "wait_for_terminal_initialization_outcome",
            ),
            Some(Outcome::InitializationComplete) => (
                "startup_initialization_complete",
                "local_initialization_only_not_verified_serving",
                "verify_serving_separately_when_authorized",
            ),
            Some(Outcome::Interrupted) => (
                "startup_interrupted",
                "shutdown_before_initialization_complete",
                "review_operator_shutdown_before_deliberate_restart",
            ),
            Some(Outcome::Failed(failure)) => return failure.diagnostic(),
        };
        Diagnostic {
            code,
            constraint,
            action,
        }
    }
    fn emit(self) {
        // Ignore platform presentation failure; never format the io error or
        // replace the original startup result. No panic on a broken stderr.
        let _ = writeln!(std::io::stderr().lock(), "{self}");
    }
}
impl fmt::Display for Observation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "event={EVENT} stage={} outcome={} ",
            self.stage.label(),
            self.label()
        )?;
        if let Some(Outcome::Failed(failure)) = self.outcome {
            return write!(f, "{failure}");
        }
        let d = self.diagnostic();
        write!(
            f,
            "code={} constraint={} action={}",
            d.code, d.constraint, d.action
        )
    }
}

struct State {
    config: Config,
    seen: u8,
    current: Stage,
    records: Vec<Observation>,
}
/// The host must finish before accepting requests. active() describes lifecycle
/// ownership, not telemetry enablement. finish takes state before its first await;
/// repeated finish/enter calls are inert, including after a cancelled finish.
/// The terminal outcome is committed before delivery: a later shutdown does not
/// retroactively undo observed local initialization or imply verified serving.
pub struct Startup {
    state: Option<State>,
    #[cfg(test)]
    client: Option<Client>,
}
impl Startup {
    pub fn new(config: Config) -> Self {
        Self {
            state: Some(State {
                config,
                seen: 0,
                current: Stage::Configuration,
                records: Vec::with_capacity(MAX_RECORDS),
            }),
            #[cfg(test)]
            client: None,
        }
    }
    pub fn active(&self) -> bool {
        self.state.is_some()
    }
    pub fn enter(&mut self, stage: Stage) {
        let Some(state) = &mut self.state else {
            return;
        };
        let bit = 1 << stage as u8;
        if state.seen & bit != 0 {
            return;
        }
        state.seen |= bit;
        state.current = stage;
        let record = Observation::new(stage, None);
        record.emit();
        state.records.push(record);
    }
    pub async fn finish(&mut self, outcome: Outcome) {
        let Some(mut state) = self.state.take() else {
            return;
        };
        let terminal = Observation::new(state.current, Some(outcome));
        terminal.emit();
        state.records.push(terminal);
        let client = state.config.into_client();
        #[cfg(test)]
        let client = self.client.take().map_or(client, |client| Ok(Some(client)));
        let result = match client {
            Ok(Some(client)) => deliver(client, &state.records).await,
            Ok(None) => return, // No SDK, encoder, TLS construction or network work.
            Err(_) => Err(Delivery {
                signal: Signal::Logs,
                constraint: "transport_configuration",
            }),
        };
        if let Err(failure) = result {
            let _ = writeln!(std::io::stderr().lock(), "{failure}");
        }
        // State, payloads and connections are owned here and dropped, not spooled.
    }
}

// Sealed encoders accept only this module's finite observations. They do not
// widen the independent request-metric sanitizer or release partial windows.
fn attribute(key: &'static str, value: &'static str) -> KeyValue {
    KeyValue {
        key: key.into(),
        value: Some(AnyValue {
            value: Some(Value::StringValue(value.into())),
        }),
    }
}
fn resource() -> Resource {
    Resource {
        attributes: vec![
            attribute("service.name", "possums-gateway"),
            attribute("deployment.environment.name", "production"),
            attribute("possums.gateway.slot", "gateway-01"),
        ],
        ..Default::default()
    }
}
fn scope() -> InstrumentationScope {
    InstrumentationScope {
        name: "possums.telemetry".into(),
        ..Default::default()
    }
}
fn logs(records: &[Observation]) -> ExportLogsServiceRequest {
    ExportLogsServiceRequest {
        resource_logs: vec![ResourceLogs {
            resource: Some(resource()),
            scope_logs: vec![ScopeLogs {
                scope: Some(scope()),
                log_records: records
                    .iter()
                    .map(|record| {
                        let d = record.diagnostic();
                        let (severity, text) = match record.outcome {
                            Some(Outcome::Failed(_)) => (SeverityNumber::Error, "ERROR"),
                            _ => (SeverityNumber::Info, "INFO"),
                        };
                        LogRecord {
                            time_unix_nano: record.time,
                            observed_time_unix_nano: record.time,
                            severity_number: severity as i32,
                            severity_text: text.into(),
                            event_name: EVENT.into(),
                            body: Some(AnyValue {
                                value: Some(Value::StringValue(record.to_string())),
                            }),
                            attributes: vec![
                                attribute("stage", record.stage.label()),
                                attribute("outcome", record.label()),
                                attribute("code", d.code),
                                attribute("constraint", d.constraint),
                                attribute("action", d.action),
                            ],
                            ..Default::default()
                        }
                    })
                    .collect(),
                ..Default::default()
            }],
            ..Default::default()
        }],
    }
}
fn metrics(records: &[Observation]) -> ExportMetricsServiceRequest {
    // Each observed (stage,outcome) is a disjoint transition, not an overlapping
    // total. No code/cause dimensions, zero points, exemplars or request metadata.
    let end = records.last().map_or(0, |record| record.time);
    let start = records.first().map_or(0, |record| record.time);
    ExportMetricsServiceRequest {
        resource_metrics: vec![ResourceMetrics {
            resource: Some(resource()),
            scope_metrics: vec![ScopeMetrics {
                scope: Some(scope()),
                metrics: vec![Metric {
                    name: "possums.startup.transitions".into(),
                    unit: "{transition}".into(),
                    data: Some(Data::Sum(Sum {
                        aggregation_temporality: AggregationTemporality::Delta as i32,
                        is_monotonic: true,
                        data_points: records
                            .iter()
                            .map(|record| NumberDataPoint {
                                attributes: vec![
                                    attribute("stage", record.stage.label()),
                                    attribute("outcome", record.label()),
                                ],
                                start_time_unix_nano: start,
                                time_unix_nano: end,
                                value: Some(number_data_point::Value::AsInt(1)),
                                ..Default::default()
                            })
                            .collect(),
                    })),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }],
    }
}

struct Delivery {
    signal: Signal,
    constraint: &'static str,
}
impl fmt::Display for Delivery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let stage = match self.signal {
            Signal::Logs => "startup_logs",
            Signal::Metrics => "startup_metrics",
        };
        write!(f, "code=startup_delivery_unconfirmed stage={stage} constraint={} action=check_telemetry_ingestion_configuration_and_connectivity_offline; delivery unknown; not retried; original startup outcome unchanged", self.constraint)
    }
}
async fn send(
    client: &Client,
    signal: Signal,
    records: &[Observation],
    deadline: tokio::time::Instant,
) -> Result<(), Delivery> {
    let payload = match signal {
        Signal::Logs => logs(records).encode_to_vec(),
        Signal::Metrics => metrics(records).encode_to_vec(),
    };
    send_payload(client, signal, payload, deadline).await
}

async fn send_payload(
    client: &Client,
    signal: Signal,
    payload: Vec<u8>,
    deadline: tokio::time::Instant,
) -> Result<(), Delivery> {
    let failure = |constraint| Delivery { signal, constraint };
    // The transport's own deadline can resolve at the same instant as ours.
    // Never begin the next connection after the shared budget is spent.
    if tokio::time::Instant::now() >= deadline {
        return Err(failure("shared_delivery_deadline"));
    }
    let request = http::Request::post(client.endpoint())
        .body(hyper::body::Bytes::from(payload))
        .map_err(|_| failure("request_encoding"))?;
    let response = tokio::time::timeout_at(deadline, client.send_bytes(request))
        .await
        .map_err(|_| failure("shared_delivery_deadline"))?
        .map_err(|error| {
            let constraint = match error.downcast_ref::<transport::Failure>() {
                Some(transport::Failure::Response) => "http_response_rejected",
                Some(transport::Failure::Expired) => "signal_delivery_deadline",
                Some(transport::Failure::Configuration) => "transport_configuration",
                _ => "transport_cause_unknown",
            };
            failure(constraint)
        })?;
    if response.status() != http::StatusCode::OK {
        return Err(failure("http_response_rejected"));
    }
    let body = response.into_body();
    let accepted = match signal {
        Signal::Logs => ExportLogsServiceResponse::decode(body).map(|r| {
            r.partial_success
                .is_none_or(|p| p.rejected_log_records == 0 && p.error_message.is_empty())
        }),
        Signal::Metrics => ExportMetricsServiceResponse::decode(body).map(|r| {
            r.partial_success
                .is_none_or(|p| p.rejected_data_points == 0 && p.error_message.is_empty())
        }),
    }
    .map_err(|_| failure("otlp_response_invalid"))?;
    if accepted {
        Ok(())
    } else {
        Err(failure("otlp_partial_success_or_warning"))
    }
}
async fn deliver(client: Client, records: &[Observation]) -> Result<(), Delivery> {
    let deadline = tokio::time::Instant::now() + BUDGET;
    let logs_client = client.clone().for_signal(Signal::Logs);
    let metrics_client = client.for_signal(Signal::Metrics);
    let log_result = send(&logs_client, Signal::Logs, records, deadline).await;
    let metric_result = send(&metrics_client, Signal::Metrics, records, deadline).await;
    // Each owned attempt/timeout has been dropped, including HTTP driver/I/O.
    // Both handles share transport state; no upload task exists to join.
    logs_client.cancel().await;
    // Preserve the first known delivery failure across a later deadline/fault.
    log_result.and(metric_result)
}

#[cfg(test)]
mod tests;
