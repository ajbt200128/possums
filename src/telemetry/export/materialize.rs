//! Test-only SDK table conversion. The borrowed aggregation views are inspection,
//! not send authority; callers must drop them before any network await.
use crate::telemetry::{
    AdmissionModel, DeliveryTerminal, Endpoint, GenerationTerminal, HttpTerminal, Infrastructure,
    Lane, QualifiedModel, Rejection, RequestTables, ResourceMetric, ResourceScope, Status, Window,
};
use opentelemetry::{InstrumentationScope, KeyValue};
use opentelemetry_sdk::{
    metrics::data::{
        Gauge, GaugeDataPoint, Metric, ResourceMetrics, ScopeMetrics, Sum, SumDataPoint,
    },
    metrics::Temporality,
    Resource,
};
use std::time::{Duration, UNIX_EPOCH};

const ENDPOINTS: [(&str, Endpoint); 16] = [
    ("home", Endpoint::Home),
    ("login", Endpoint::Login),
    ("logout", Endpoint::Logout),
    ("chat_web", Endpoint::ChatWeb),
    ("new_chat", Endpoint::NewChat),
    ("recovery", Endpoint::Recovery),
    ("recovery_download", Endpoint::RecoveryDownload),
    ("claims", Endpoint::Claims),
    ("attestation", Endpoint::Attestation),
    ("api_challenge", Endpoint::ApiChallenge),
    ("api_session", Endpoint::ApiSession),
    ("api_submission", Endpoint::ApiSubmission),
    ("api_logout", Endpoint::ApiLogout),
    ("models", Endpoint::Models),
    ("chat_api", Endpoint::ChatApi),
    ("other", Endpoint::Other),
];
const MODELS: [(&str, QualifiedModel); 3] = [
    ("kimi-k3", QualifiedModel(0)),
    ("glm-5-3", QualifiedModel(1)),
    ("other", QualifiedModel(2)),
];
const ADMISSION: [(&str, AdmissionModel); 5] = [
    ("kimi-k3", AdmissionModel::Qualified(QualifiedModel(0))),
    ("glm-5-3", AdmissionModel::Qualified(QualifiedModel(1))),
    ("other", AdmissionModel::Qualified(QualifiedModel(2))),
    ("unknown", AdmissionModel::Unknown),
    ("not_applicable", AdmissionModel::NotApplicable),
];
const STATUS: [(&str, Status); 6] = [
    ("1xx", Status::Informational),
    ("2xx", Status::Success),
    ("3xx", Status::Redirect),
    ("4xx", Status::ClientError),
    ("5xx", Status::ServerError),
    ("unknown", Status::Unknown),
];
const HTTP_TERMINAL: [(&str, HttpTerminal); 3] = [
    ("eof", HttpTerminal::Eof),
    ("error", HttpTerminal::Error),
    ("unknown", HttpTerminal::Unknown),
];
const GENERATION: [(&str, &str, GenerationTerminal); 12] = [
    ("success", "none", GenerationTerminal::Success),
    ("failure", "admission", GenerationTerminal::Admission),
    ("failure", "verification", GenerationTerminal::Verification),
    ("failure", "catalog", GenerationTerminal::Catalog),
    ("failure", "encoding", GenerationTerminal::Encoding),
    ("failure", "transport", GenerationTerminal::Transport),
    ("failure", "decoding", GenerationTerminal::Decoding),
    ("failure", "validation", GenerationTerminal::Validation),
    (
        "failure",
        "terminal_usage",
        GenerationTerminal::TerminalUsage,
    ),
    ("failure", "settlement", GenerationTerminal::Settlement),
    ("failure", "internal", GenerationTerminal::Internal),
    ("unknown", "unknown", GenerationTerminal::Unknown),
];
const DELIVERY: [(&str, DeliveryTerminal); 3] = [
    ("completed", DeliveryTerminal::Completed),
    ("interrupted", DeliveryTerminal::Interrupted),
    ("unknown", DeliveryTerminal::Unknown),
];
const REASONS: [(&str, Rejection); 14] = [
    ("connection_capacity", Rejection::ConnectionCapacity),
    ("header_protocol", Rejection::HeaderProtocol),
    ("connection_deadline", Rejection::ConnectionDeadline),
    ("transport_unknown", Rejection::TransportUnknown),
    ("request_capacity", Rejection::RequestCapacity),
    ("ingress_capacity", Rejection::IngressCapacity),
    ("generation_capacity", Rejection::GenerationCapacity),
    ("account_limit", Rejection::AccountLimit),
    ("credit", Rejection::Credit),
    ("auth", Rejection::Auth),
    ("input", Rejection::Input),
    ("verification", Rejection::Verification),
    ("catalog", Rejection::Catalog),
    ("internal", Rejection::Internal),
];
const LANES: [(&str, Lane); 6] = [
    ("connection", Lane::Connection),
    ("generation", Lane::Generation),
    ("heavy", Lane::Heavy),
    ("ingress", Lane::Ingress),
    ("new_chat", Lane::NewChat),
    ("control", Lane::Control),
];
const SCOPES: [(&str, &str, ResourceScope); 4] = [
    ("tinfoil_native", "enclave", ResourceScope::TinfoilEnclave),
    ("tinfoil_native", "workload", ResourceScope::TinfoilWorkload),
    ("process", "process", ResourceScope::Process),
    ("cgroup", "cgroup", ResourceScope::Cgroup),
];
const RESOURCES: [(&str, &str, ResourceMetric); 4] = [
    (
        "possums.resource.cpu.used",
        "{cpu}",
        ResourceMetric::CpuUsed,
    ),
    (
        "possums.resource.memory.used",
        "By",
        ResourceMetric::MemoryUsed,
    ),
    (
        "possums.resource.cpu.capacity",
        "{cpu}",
        ResourceMetric::CpuCapacity,
    ),
    (
        "possums.resource.memory.capacity",
        "By",
        ResourceMetric::MemoryCapacity,
    ),
];

fn attributes(fields: &[(&'static str, &'static str)]) -> Vec<KeyValue> {
    fields
        .iter()
        .map(|(key, value)| KeyValue::new(*key, *value))
        .collect()
}
fn envelope(metrics: Vec<Metric>) -> ResourceMetrics {
    ResourceMetrics {
        resource: Resource::builder_empty()
            .with_attributes([
                KeyValue::new("service.name", "possums-gateway"),
                KeyValue::new("deployment.environment.name", "test"),
                KeyValue::new("possums.gateway.slot", "gateway-01"),
            ])
            .build(),
        scope_metrics: vec![ScopeMetrics {
            scope: InstrumentationScope::builder("possums.telemetry").build(),
            metrics,
        }],
    }
}
fn sum_metric(
    name: &'static str,
    unit: &'static str,
    points: Vec<SumDataPoint<u64>>,
    window: Window,
) -> Option<Metric> {
    (!points.is_empty()).then(|| Metric {
        name: name.into(),
        description: "".into(),
        unit: unit.into(),
        data: Box::new(Sum {
            data_points: points,
            start_time: UNIX_EPOCH + Duration::from_nanos(window.start_ns),
            time: UNIX_EPOCH + Duration::from_nanos(window.end_ns),
            temporality: Temporality::Delta,
            is_monotonic: true,
        }),
    })
}
fn point(
    points: &mut Vec<SumDataPoint<u64>>,
    fields: &[(&'static str, &'static str)],
    value: Option<u64>,
) {
    if let Some(value) = value {
        points.push(SumDataPoint {
            attributes: attributes(fields),
            value,
            exemplars: vec![],
        });
    }
}
fn bins<const N: usize>(
    points: &mut Vec<SumDataPoint<u64>>,
    fields: &[(&'static str, &'static str)],
    values: Option<&crate::telemetry::Histogram<N>>,
) {
    if let Some(values) = values {
        for (i, &value) in values.bins().iter().enumerate() {
            let bucket = if N == 11 {
                DURATION_BUCKETS[i]
            } else {
                OCCUPANCY_BUCKETS[i]
            };
            let mut fields = fields.to_vec();
            fields.push(("bucket", bucket));
            point(points, &fields, Some(value));
        }
    }
}
const DURATION_BUCKETS: [&str; 11] = [
    "b00", "b01", "b02", "b03", "b04", "b05", "b06", "b07", "b08", "b09", "b10",
];
const OCCUPANCY_BUCKETS: [&str; 10] = [
    "b00", "b01", "b02", "b03", "b04", "b05", "b06", "b07", "b08", "b09",
];
fn append(
    metrics: &mut Vec<Metric>,
    name: &'static str,
    unit: &'static str,
    points: Vec<SumDataPoint<u64>>,
    window: Window,
) {
    if let Some(metric) = sum_metric(name, unit, points, window) {
        metrics.push(metric);
    }
}
fn request_window(window: Window) -> bool {
    window.start_ns % 300_000_000_000 == 0
        && window.end_ns.checked_sub(window.start_ns) == Some(300_000_000_000)
}
// The supported prost transform maps an out-of-range u64 to zero. Reject the
// entire request family rather than silently changing a released population.
fn wire_representable(t: &RequestTables) -> bool {
    [
        t.http_starts.as_slice(),
        &t.http_completed,
        &t.generation_starts,
        &t.generation_completed,
        &t.delivery,
        &t.rejected,
    ]
    .into_iter()
    .flatten()
    .chain(t.http_duration.iter().flat_map(|h| h.bins()))
    .chain(t.generation_duration.iter().flat_map(|h| h.bins()))
    .chain(t.first_output.iter().flat_map(|h| h.bins()))
    .chain(t.occupancy.iter().flat_map(|h| h.bins()))
    .all(|n| *n <= i64::MAX as u64)
}
/// Returns no partial family on invalid/sparse data. No send permit is conferred.
pub(super) fn request(t: &RequestTables, window: Window) -> Option<ResourceMetrics> {
    if !request_window(window) || !t.releasable() || !wire_representable(t) {
        return None;
    }
    // The table predicate validates arithmetic and thresholds; this additional
    // check rejects impossible typed rejection combinations before any allocation.
    for e in ENDPOINTS
        .iter()
        .map(|(_, e)| Some(*e))
        .chain(std::iter::once(None))
    {
        for (mi, (_, model)) in ADMISSION.iter().enumerate() {
            for (ri, (_, reason)) in REASONS.iter().enumerate() {
                let pre = ri < 4;
                if (pre != e.is_none()
                    || (pre && mi != 4)
                    || (e.is_some_and(|e| e.chat().is_none()) && mi != 4))
                    && t.rejected(e, *model, *reason).is_some()
                {
                    return None;
                }
            }
        }
    }
    let mut metrics = Vec::with_capacity(10);
    let mut starts = Vec::new();
    let mut completed = Vec::new();
    let mut duration = Vec::new();
    for (endpoint, e) in ENDPOINTS {
        let base = [("possums.endpoint", endpoint)];
        point(&mut starts, &base, t.http_started(e));
        for (status, s) in STATUS {
            for (terminal, h) in HTTP_TERMINAL {
                let fields = [
                    ("possums.endpoint", endpoint),
                    ("status_class", status),
                    ("http_terminal", terminal),
                ];
                point(&mut completed, &fields, t.http_terminal(e, s, h));
                bins(&mut duration, &fields, t.http_duration(e, s, h));
            }
        }
    }
    append(
        &mut metrics,
        "possums.http.requests",
        "{request}",
        starts,
        window,
    );
    append(
        &mut metrics,
        "possums.http.completed",
        "{request}",
        completed,
        window,
    );
    append(
        &mut metrics,
        "possums.http.duration.bucket",
        "{observation}",
        duration,
        window,
    );
    let mut started = Vec::new();
    let mut terminal = Vec::new();
    let mut gen_duration = Vec::new();
    let mut output = Vec::new();
    let mut delivery = Vec::new();
    for (endpoint, e) in [ENDPOINTS[3], ENDPOINTS[14]] {
        for (model, m) in MODELS {
            let base = [("possums.endpoint", endpoint), ("possums.model", model)];
            point(&mut started, &base, t.generation_started(e, m));
            for (outcome, stage, g) in GENERATION {
                let fields = [
                    ("possums.endpoint", endpoint),
                    ("possums.model", model),
                    ("outcome", outcome),
                    ("failure_stage", stage),
                ];
                point(&mut terminal, &fields, t.generation_terminal(e, m, g));
                bins(&mut gen_duration, &fields, t.generation_duration(e, m, g));
                bins(&mut output, &fields, t.first_output(e, m, g));
            }
            for (outcome, d) in DELIVERY {
                let fields = [
                    ("possums.endpoint", endpoint),
                    ("possums.model", model),
                    ("outcome", outcome),
                ];
                point(&mut delivery, &fields, t.delivery(e, m, d));
            }
        }
    }
    append(
        &mut metrics,
        "possums.generation.started",
        "{generation}",
        started,
        window,
    );
    append(
        &mut metrics,
        "possums.generation.completed",
        "{generation}",
        terminal,
        window,
    );
    append(
        &mut metrics,
        "possums.generation.duration.bucket",
        "{observation}",
        gen_duration,
        window,
    );
    append(
        &mut metrics,
        "possums.generation.first_output.bucket",
        "{observation}",
        output,
        window,
    );
    append(
        &mut metrics,
        "possums.delivery.completed",
        "{delivery}",
        delivery,
        window,
    );
    let mut rejected = Vec::new();
    for e in ENDPOINTS
        .iter()
        .map(|(name, e)| (*name, Some(*e)))
        .chain(std::iter::once(("not_applicable", None)))
    {
        for (model, m) in ADMISSION {
            for (reason, r) in REASONS {
                let fields = [
                    ("possums.endpoint", e.0),
                    ("possums.model", model),
                    ("reason", reason),
                ];
                point(&mut rejected, &fields, t.rejected(e.1, m, r));
            }
        }
    }
    append(
        &mut metrics,
        "possums.admission.rejected",
        "{rejection}",
        rejected,
        window,
    );
    let mut occupancy = Vec::new();
    for (lane, l) in LANES {
        bins(&mut occupancy, &[("lane", lane)], t.occupancy(l));
    }
    append(
        &mut metrics,
        "possums.admission.occupancy.bucket",
        "{observation}",
        occupancy,
        window,
    );
    Some(envelope(metrics))
}

/// Independent minute, end timestamp only; absent/invalid source points stay absent.
pub(super) fn infrastructure(t: &Infrastructure, window: Window) -> Option<ResourceMetrics> {
    if window.start_ns % 60_000_000_000 != 0
        || window.end_ns.checked_sub(window.start_ns) != Some(60_000_000_000)
    {
        return None;
    }
    let mut metrics = Vec::new();
    for (name, unit, kind) in RESOURCES {
        let mut points = Vec::new();
        for (source, scope, s) in SCOPES {
            if let Some(value) = t.resource(s, kind).filter(|n| n.is_finite() && *n >= 0.) {
                points.push(GaugeDataPoint {
                    attributes: attributes(&[("source", source), ("scope", scope)]),
                    value,
                    exemplars: vec![],
                });
            }
        }
        if !points.is_empty() {
            metrics.push(Metric {
                name: name.into(),
                description: "".into(),
                unit: unit.into(),
                data: Box::new(Gauge {
                    data_points: points,
                    start_time: None,
                    time: UNIX_EPOCH + Duration::from_nanos(window.end_ns),
                }),
            });
        }
    }
    let mut points = Vec::new();
    for (lane, l) in LANES {
        if let Some(value) = t.capacity(l).filter(|n| n.is_finite() && *n >= 0.) {
            points.push(GaugeDataPoint {
                attributes: attributes(&[
                    ("source", "configuration"),
                    ("scope", "gateway"),
                    ("lane", lane),
                ]),
                value,
                exemplars: vec![],
            });
        }
    }
    if !points.is_empty() {
        metrics.push(Metric {
            name: "possums.admission.capacity".into(),
            description: "".into(),
            unit: "{permit}".into(),
            data: Box::new(Gauge {
                data_points: points,
                start_time: None,
                time: UNIX_EPOCH + Duration::from_nanos(window.end_ns),
            }),
        });
    }
    (!metrics.is_empty()).then(|| envelope(metrics))
}

#[cfg(test)]
mod tests;
