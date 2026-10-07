use super::super::{candidate, capture_header, clean_child};
use super::*;
use crate::telemetry::Histogram;
use opentelemetry_otlp::MetricExporter;
use opentelemetry_proto::tonic::{
    collector::metrics::v1::ExportMetricsServiceRequest,
    metrics::v1::{metric::Data, number_data_point::Value},
};
use opentelemetry_sdk::metrics::exporter::PushMetricExporter;
use prost::Message;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

const W: Window = Window {
    start_ns: 300_000_000_000,
    end_ns: 600_000_000_000,
};
const I: Window = Window {
    start_ns: 300_000_000_000,
    end_ns: 360_000_000_000,
};
fn cohort(n: u64, mixed: bool, missing: u64) -> RequestTables {
    let mut t = RequestTables {
        ticks: 300,
        ..Default::default()
    };
    t.http_starts[14] = n;
    t.http_completed[255] = n;
    t.http_duration[255].0[3] = n;
    t.dispositions[255][0] = n;
    t.generation_starts[3] = n;
    t.delivery[9] = n;
    if mixed {
        for i in [36, 41] {
            t.generation_completed[i] = 10;
            t.generation_duration[i].0[2] = 10;
            t.first_output[i].0[1] = 10;
        }
    } else {
        t.generation_completed[36] = n;
        t.generation_duration[36].0[2] = n;
        t.first_output[36].0[1] = n - missing;
    }
    for lane in [0, 2] {
        t.occupancy[lane] = Histogram([300 - 3 * n, 3 * n, 0, 0, 0, 0, 0, 0, 0, 0]);
    }
    for lane in [1, 3] {
        t.occupancy[lane] = Histogram([300 - n, n, 0, 0, 0, 0, 0, 0, 0, 0]);
    }
    t.contributors = [n, n, n, n, 0, 0];
    t
}
pub(in crate::telemetry) fn decode(bytes: &[u8]) -> ExportMetricsServiceRequest {
    let decoded = ExportMetricsServiceRequest::decode(bytes).unwrap();
    assert_eq!(decoded.encode_to_vec(), bytes);
    let resource = &decoded.resource_metrics[0];
    assert_eq!(decoded.resource_metrics.len(), 1);
    assert!(resource.schema_url.is_empty());
    let attrs = &resource.resource.as_ref().unwrap().attributes;
    assert_eq!(attrs.len(), 3);
    for (k, v) in [
        ("service.name", "possums-gateway"),
        ("deployment.environment.name", "test"),
        ("possums.gateway.slot", "gateway-01"),
    ] {
        assert_eq!(
            attrs
                .iter()
                .find(|a| a.key == k)
                .unwrap()
                .value
                .as_ref()
                .unwrap()
                .value,
            Some(opentelemetry_proto::tonic::common::v1::any_value::Value::StringValue(v.into()))
        );
    }
    assert_eq!(resource.scope_metrics.len(), 1);
    let scope = &resource.scope_metrics[0];
    assert!(scope.schema_url.is_empty());
    assert_eq!(scope.scope.as_ref().unwrap().name, "possums.telemetry");
    assert!(scope.scope.as_ref().unwrap().version.is_empty());
    assert!(scope.scope.as_ref().unwrap().attributes.is_empty());
    for m in &scope.metrics {
        assert!(m.description.is_empty());
    }
    decoded
}
type WirePoints =
    std::collections::BTreeMap<(String, Vec<(String, String)>), (String, i64, u64, u64)>;
pub(in crate::telemetry) fn points(bytes: &[u8]) -> WirePoints {
    points_in(bytes, W)
}
fn points_in(bytes: &[u8], window: Window) -> WirePoints {
    let decoded = decode(bytes);
    let mut out = std::collections::BTreeMap::new();
    for m in &decoded.resource_metrics[0].scope_metrics[0].metrics {
        let (points, start, end): (Vec<_>, _, _) = match &m.data {
            Some(Data::Sum(s)) => {
                assert_eq!(s.aggregation_temporality, 1);
                assert!(s.is_monotonic);
                (
                    s.data_points.iter().collect(),
                    window.start_ns,
                    window.end_ns,
                )
            }
            Some(Data::Gauge(_)) => continue,
            _ => panic!("unexpected histogram or empty data"),
        };
        for p in points {
            assert!(p.exemplars.is_empty());
            assert_eq!(p.flags, 0);
            let mut attrs: Vec<_> = p
                .attributes
                .iter()
                .map(|a| {
                    let Some(
                        opentelemetry_proto::tonic::common::v1::any_value::Value::StringValue(v),
                    ) = &a.value.as_ref().unwrap().value
                    else {
                        panic!("non-string label")
                    };
                    (a.key.clone(), v.clone())
                })
                .collect();
            attrs.sort();
            assert_eq!((p.start_time_unix_nano, p.time_unix_nano), (start, end));
            let Some(Value::AsInt(value)) = p.value else {
                panic!("noninteger count")
            };
            assert!(out
                .insert((m.name.clone(), attrs), (m.unit.clone(), value, start, end))
                .is_none());
        }
    }
    out
}
fn add_exact(
    out: &mut WirePoints,
    name: &str,
    unit: &str,
    fields: &[(&str, &str)],
    value: i64,
    window: Window,
) {
    let mut attrs: Vec<_> = fields
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    attrs.sort();
    assert!(out
        .insert(
            (name.into(), attrs),
            (unit.into(), value, window.start_ns, window.end_ns)
        )
        .is_none());
}
fn exact_bins(
    out: &mut WirePoints,
    name: &str,
    fields: &[(&str, &str)],
    bin: usize,
    n: i64,
    window: Window,
) {
    for (i, bucket) in DURATION_BUCKETS.iter().enumerate() {
        let mut attrs = fields.to_vec();
        attrs.push(("bucket", bucket));
        add_exact(
            out,
            name,
            "{observation}",
            &attrs,
            if i == bin { n } else { 0 },
            window,
        );
    }
}
pub(in crate::telemetry) fn expected(n: i64, mixed: bool, missing: i64) -> WirePoints {
    let mut out = std::collections::BTreeMap::new();
    let mut add = |name: &str, unit: &str, attrs: Vec<(&str, &str)>, count: i64| {
        let mut attrs: Vec<_> = attrs
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect();
        attrs.sort();
        out.insert(
            (format!("possums.{name}"), attrs),
            (unit.to_owned(), count, W.start_ns, W.end_ns),
        );
    };
    let ep = ("possums.endpoint", "chat_api");
    let model = ("possums.model", "kimi-k3");
    let http = [ep, ("status_class", "2xx"), ("http_terminal", "eof")];
    add("http.requests", "{request}", vec![ep], n);
    add("http.completed", "{request}", http.to_vec(), n);
    macro_rules! buckets {
        ($name:expr, $attrs:expr, $bin:expr, $count:expr, $len:expr) => {{
            let attrs = $attrs;
            for i in 0..$len {
                let mut a = attrs.clone();
                a.push((
                    "bucket",
                    if $len == 11 {
                        DURATION_BUCKETS[i]
                    } else {
                        OCCUPANCY_BUCKETS[i]
                    },
                ));
                add(
                    $name,
                    "{observation}",
                    a,
                    if i == $bin { $count } else { 0 },
                );
            }
        }};
    }
    buckets!("http.duration.bucket", http.to_vec(), 3, n, 11);
    add("generation.started", "{generation}", vec![ep, model], n);
    for (outcome, stage, count, out_count) in if mixed {
        vec![
            ("success", "none", 10, 10),
            ("failure", "transport", 10, 10),
        ]
    } else {
        vec![("success", "none", n, n - missing)]
    } {
        let a = vec![ep, model, ("outcome", outcome), ("failure_stage", stage)];
        add("generation.completed", "{generation}", a.clone(), count);
        buckets!("generation.duration.bucket", a.clone(), 2, count, 11);
        if out_count > 0 {
            buckets!("generation.first_output.bucket", a, 1, out_count, 11);
        }
    }
    add(
        "delivery.completed",
        "{delivery}",
        vec![ep, model, ("outcome", "completed")],
        n,
    );
    for (lane, occupied) in [
        ("connection", 3 * n),
        ("generation", n),
        ("heavy", 3 * n),
        ("ingress", n),
    ] {
        for (i, bucket) in OCCUPANCY_BUCKETS.iter().enumerate() {
            add(
                "admission.occupancy.bucket",
                "{observation}",
                vec![("lane", lane), ("bucket", bucket)],
                if i == 0 {
                    300 - occupied
                } else if i == 1 {
                    occupied
                } else {
                    0
                },
            );
        }
    }
    out
}
async fn wire(mut metrics: opentelemetry_sdk::metrics::data::ResourceMetrics) -> Vec<u8> {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let client = super::super::Client::new(listener.local_addr().unwrap()).unwrap();
    let exporter: MetricExporter = candidate(client.clone()).unwrap();
    let peer = async {
        let (mut stream, _) = listener.accept().await.unwrap();
        let len = capture_header(&mut stream, true).await;
        assert!(len <= super::super::transport::MAX_OUTBOUND);
        let mut bytes = vec![0; len];
        stream.read_exact(&mut bytes).await.unwrap();
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        bytes
    };
    let (result, bytes) = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        tokio::join!(exporter.export(&mut metrics), peer)
    })
    .await
    .unwrap();
    assert!(result.is_ok());
    client.cancel().await;
    assert!(exporter.shutdown().is_ok());
    bytes
}
#[tokio::test]
async fn linked_wire_vectors() {
    if !clean_child("telemetry::export::materialize::tests::linked_wire_vectors") {
        return;
    }
    let empty = RequestTables::default();
    assert!(request(&empty, W).is_none());
    for n in [1, 9] {
        assert!(request(&cohort(n, false, 0), W).is_none());
    }
    for (n, mixed, missing) in [
        (10, false, 0),
        (11, false, 0),
        (20, true, 0),
        (20, false, 10),
        (10, false, 10),
    ] {
        let metrics = request(&cohort(n, mixed, missing), W).unwrap();
        let bytes = wire(metrics).await;
        assert_eq!(points(&bytes), expected(n as i64, mixed, missing as i64));
    }
    let mut t = cohort(11, false, 0);
    t.first_output[36].0[2] = 1;
    t.first_output[36].0[1] -= 1;
    assert!(request(&t, W).is_none());
}

#[tokio::test]
async fn sparse_and_cross_window_wire() {
    if !clean_child("telemetry::export::materialize::tests::sparse_and_cross_window_wire") {
        return;
    }
    for change in 0..9 {
        let mut t = cohort(10, false, 0);
        match change {
            0 => {
                t.generation_completed[41] = 1;
                t.generation_duration[41].0[2] = 1;
                t.first_output[41].0[1] = 1;
            }
            1 => {
                t.generation_starts[4] = 1;
            }
            2 => {
                t.http_starts[3] = 1;
            }
            3 => {
                t.first_output[36].0[1] = 9;
                t.first_output[36].0[2] = 1;
            }
            4 => {
                t.first_output[36].0[1] = 9;
            }
            5 => {
                t.dispositions[255] = [9, 1, 0, 0, 0];
            }
            6 => {
                t.http_duration[255].0[3] = 9;
                t.http_duration[255].0[2] = 1;
            }
            7 => {
                t.rejected[16 * 5 * 14] = 1;
            }
            _ => {
                t.valid = false;
            }
        }
        assert!(request(&t, W).is_none(), "sparse variant {change}");
    }
    assert!(request(
        &cohort(10, false, 0),
        Window {
            start_ns: W.start_ns + 1,
            ..W
        }
    )
    .is_none());
    // Vreject, Vreject-rare: no HTTP or invented model/endpoint.
    for n in [10, 11] {
        let mut t = RequestTables {
            ticks: 300,
            ..Default::default()
        };
        t.rejected[16 * 5 * 14 + 4 * 14] = n;
        let bytes = wire(request(&t, W).unwrap()).await;
        let p = points(&bytes);
        assert_eq!(p.len(), 1);
        assert_eq!(
            p.get(&(
                "possums.admission.rejected".into(),
                vec![
                    ("possums.endpoint".into(), "not_applicable".into()),
                    ("possums.model".into(), "not_applicable".into()),
                    ("reason".into(), "connection_capacity".into())
                ]
            )),
            Some(&("{rejection}".into(), n as i64, W.start_ns, W.end_ns))
        );
    }
    // Vunknown is an aggregation-only fixture with no HTTP/occupancy.
    let mut t = RequestTables {
        ticks: 300,
        ..Default::default()
    };
    t.generation_starts[3] = 10;
    t.generation_completed[47] = 10;
    t.generation_duration[47].0[2] = 10;
    let bytes = wire(request(&t, W).unwrap()).await;
    let mut expected_unknown = WirePoints::new();
    let base = [
        ("possums.endpoint", "chat_api"),
        ("possums.model", "kimi-k3"),
    ];
    let terminal = [
        ("possums.endpoint", "chat_api"),
        ("possums.model", "kimi-k3"),
        ("outcome", "unknown"),
        ("failure_stage", "unknown"),
    ];
    add_exact(
        &mut expected_unknown,
        "possums.generation.started",
        "{generation}",
        &base,
        10,
        W,
    );
    add_exact(
        &mut expected_unknown,
        "possums.generation.completed",
        "{generation}",
        &terminal,
        10,
        W,
    );
    exact_bins(
        &mut expected_unknown,
        "possums.generation.duration.bucket",
        &terminal,
        2,
        10,
        W,
    );
    assert_eq!(points(&bytes), expected_unknown);
    // Vsplit independent starts and terminal cohorts use distinct grid windows.
    let mut early = RequestTables {
        ticks: 300,
        ..Default::default()
    };
    early.http_starts[14] = 10;
    early.generation_starts[3] = 10;
    let mut expected_early = WirePoints::new();
    add_exact(
        &mut expected_early,
        "possums.http.requests",
        "{request}",
        &[("possums.endpoint", "chat_api")],
        10,
        W,
    );
    add_exact(
        &mut expected_early,
        "possums.generation.started",
        "{generation}",
        &base,
        10,
        W,
    );
    assert_eq!(
        points(&wire(request(&early, W).unwrap()).await),
        expected_early
    );
    let next = Window {
        start_ns: W.end_ns,
        end_ns: W.end_ns + 300_000_000_000,
    };
    let mut late = RequestTables {
        ticks: 300,
        ..Default::default()
    };
    late.http_completed[255] = 10;
    late.http_duration[255].0[3] = 10;
    late.dispositions[255][0] = 10;
    late.generation_completed[36] = 10;
    late.generation_duration[36].0[3] = 10;
    late.first_output[36].0[1] = 10;
    late.delivery[9] = 10;
    let bytes = wire(request(&late, next).unwrap()).await;
    let mut expected_late = WirePoints::new();
    let http = [
        ("possums.endpoint", "chat_api"),
        ("status_class", "2xx"),
        ("http_terminal", "eof"),
    ];
    let success = [
        ("possums.endpoint", "chat_api"),
        ("possums.model", "kimi-k3"),
        ("outcome", "success"),
        ("failure_stage", "none"),
    ];
    add_exact(
        &mut expected_late,
        "possums.http.completed",
        "{request}",
        &http,
        10,
        next,
    );
    exact_bins(
        &mut expected_late,
        "possums.http.duration.bucket",
        &http,
        3,
        10,
        next,
    );
    add_exact(
        &mut expected_late,
        "possums.generation.completed",
        "{generation}",
        &success,
        10,
        next,
    );
    exact_bins(
        &mut expected_late,
        "possums.generation.duration.bucket",
        &success,
        3,
        10,
        next,
    );
    exact_bins(
        &mut expected_late,
        "possums.generation.first_output.bucket",
        &success,
        1,
        10,
        next,
    );
    add_exact(
        &mut expected_late,
        "possums.delivery.completed",
        "{delivery}",
        &[
            ("possums.endpoint", "chat_api"),
            ("possums.model", "kimi-k3"),
            ("outcome", "completed"),
        ],
        10,
        next,
    );
    assert_eq!(points_in(&bytes, next), expected_late);
}

#[tokio::test]
async fn infrastructure_end_only_and_missing() {
    if !clean_child("telemetry::export::materialize::tests::infrastructure_end_only_and_missing") {
        return;
    }
    let mut t = Infrastructure::default();
    assert!(infrastructure(&t, I).is_none());
    for interval in 0..6 {
        for (kind, value) in [
            (ResourceMetric::CpuUsed, 1.),
            (ResourceMetric::MemoryUsed, 1048576.),
            (ResourceMetric::CpuCapacity, 2.),
            (ResourceMetric::MemoryCapacity, 8388608.),
        ] {
            t.observe(ResourceScope::TinfoilEnclave, kind, interval, value);
        }
        for (_, lane) in LANES {
            t.configuration(lane, interval, crate::telemetry::CAPACITIES[lane as usize]);
        }
    }
    // Another scope without six source intervals is never a zero.
    t.observe(ResourceScope::Process, ResourceMetric::CpuUsed, 0, 9.);
    let bytes = wire(infrastructure(&t, I).unwrap()).await;
    let decoded = decode(&bytes);
    let metrics = &decoded.resource_metrics[0].scope_metrics[0].metrics;
    assert_eq!(metrics.len(), 5);
    let expected = [
        ("possums.resource.cpu.used", "{cpu}", 1.),
        ("possums.resource.memory.used", "By", 1048576.),
        ("possums.resource.cpu.capacity", "{cpu}", 2.),
        ("possums.resource.memory.capacity", "By", 8388608.),
    ];
    for (i, (name, unit, value)) in expected.iter().enumerate() {
        assert_eq!(
            (&metrics[i].name, &metrics[i].unit),
            (&name.to_string(), &unit.to_string())
        );
        let Some(Data::Gauge(g)) = &metrics[i].data else {
            panic!("not gauge")
        };
        assert_eq!(g.data_points.len(), 1);
        let p = &g.data_points[0];
        assert_eq!(p.value, Some(Value::AsDouble(*value)));
        assert_eq!((p.start_time_unix_nano, p.time_unix_nano), (0, I.end_ns));
        assert_eq!(p.attributes.len(), 2);
        assert_eq!(p.attributes[0].key, "source");
        assert_eq!(p.attributes[1].key, "scope");
    }
    let Some(Data::Gauge(g)) = &metrics[4].data else {
        panic!("capacity gauge")
    };
    assert_eq!(g.data_points.len(), 6);
    for (i, p) in g.data_points.iter().enumerate() {
        assert_eq!(
            p.value,
            Some(Value::AsDouble(crate::telemetry::CAPACITIES[i] as f64))
        );
        assert_eq!((p.start_time_unix_nano, p.time_unix_nano), (0, I.end_ns));
        assert_eq!(p.attributes.len(), 3);
    }
    let mut invalid = Infrastructure::default();
    for interval in 0..5 {
        invalid.observe(
            ResourceScope::Cgroup,
            ResourceMetric::MemoryUsed,
            interval,
            1.,
        );
    }
    assert!(infrastructure(&invalid, I).is_none());
    assert!(infrastructure(
        &t,
        Window {
            start_ns: I.start_ns + 1,
            ..I
        }
    )
    .is_none());
}

// All reachable cells populated with all fallback bins. HTTP and generation
// populations are independent cohorts; rejection's documented 1190 product is
// deliberately conservative: pre-router is only 4, non-chat models restricted.
fn maximum() -> Box<RequestTables> {
    let mut t = Box::new(RequestTables {
        ticks: 300,
        ..Default::default()
    });
    fill_maximum(&mut t);
    t
}
pub(in crate::telemetry) fn fill_maximum(t: &mut RequestTables) {
    t.ticks = 300;
    for i in 0..16 {
        t.http_starts[i] = 10;
    }
    for i in 0..288 {
        t.http_completed[i] = 110;
        t.http_duration[i].0 = [10; 11];
        t.dispositions[i] = [70, 10, 10, 10, 10];
    }
    for i in 0..6 {
        t.generation_starts[i] = 10;
    }
    for i in 0..72 {
        t.generation_completed[i] = 110;
        t.generation_duration[i].0 = [10; 11];
        t.first_output[i].0 = [10; 11];
    }
    for i in 0..18 {
        t.delivery[i] = 10;
    }
    for e in 0..17 {
        for m in 0..5 {
            for r in 0..14 {
                let pre = r < 4;
                if (pre && e == 16 && m == 4) || (!pre && e < 16 && (e == 3 || e == 14 || m == 4)) {
                    t.rejected[(e * 5 + m) * 14 + r] = 10;
                }
            }
        }
    }
    for i in 0..6 {
        t.contributors[i] = 10;
        t.occupancy[i].0 = [30; 10];
    }
    assert!(t.releasable());
}
#[tokio::test]
async fn maximum_reachable_cardinality_actual_wire_and_allocations() {
    if !clean_child("telemetry::export::materialize::tests::maximum_reachable_cardinality_actual_wire_and_allocations") {return;}
    const LIMIT: usize = 8 * 1024 * 1024;
    let t = maximum();
    let before = super::super::phase();
    let metrics = request(&t, W).unwrap();
    let after = crate::process_alloc_tests::ALLOCATOR.snapshot();
    if std::env::var_os("POSSUMS_TELEMETRY_ALLOCATION_TEST").is_some() {
        let peak = after.phase_peak.saturating_sub(before.live);
        eprintln!("materialize max requested_peak={peak}");
        assert!(peak <= LIMIT);
    }
    let expected_points = 16 + 288 + 288 * 11 + 6 + 72 + 72 * 11 + 72 * 11 + 18 + 244 + 60;
    assert_eq!(expected_points, 6402 - 1190 + 244);
    assert_eq!(
        metrics.scope_metrics[0]
            .metrics
            .iter()
            .map(|m| {
                let s = m
                    .data
                    .as_any()
                    .downcast_ref::<opentelemetry_sdk::metrics::data::Sum<u64>>()
                    .unwrap();
                s.data_points.len()
            })
            .sum::<usize>(),
        expected_points
    );
    let before = super::super::phase();
    let bytes = wire(metrics).await;
    let after = crate::process_alloc_tests::ALLOCATOR.snapshot();
    if std::env::var_os("POSSUMS_TELEMETRY_ALLOCATION_TEST").is_some() {
        let peak = after.phase_peak.saturating_sub(before.live);
        eprintln!("serialize and bounded peer max requested_peak={peak}");
        assert!(peak <= LIMIT);
    }
    assert!(bytes.len() <= 6_576_128, "actual bounded wire bytes");
    eprintln!(
        "materialize max actual_wire_bytes={} points={expected_points}",
        bytes.len()
    );
    let decoded = decode(&bytes);
    assert_eq!(
        decoded.resource_metrics[0].scope_metrics[0]
            .metrics
            .iter()
            .map(|m| match &m.data {
                Some(Data::Sum(s)) => s.data_points.len(),
                _ => panic!("non-sum"),
            })
            .sum::<usize>(),
        expected_points
    );
}

#[tokio::test]
async fn co_closing_separate_resources_and_boundaries() {
    if !clean_child(
        "telemetry::export::materialize::tests::co_closing_separate_resources_and_boundaries",
    ) {
        return;
    }
    let mut infra = Infrastructure::default();
    for interval in 0..6 {
        infra.configuration(Lane::Connection, interval, 64);
    }
    let request_bytes = wire(request(&cohort(10, false, 0), W).unwrap()).await;
    let infra_bytes = wire(
        infrastructure(
            &infra,
            Window {
                start_ns: 540_000_000_000,
                end_ns: W.end_ns,
            },
        )
        .unwrap(),
    )
    .await;
    for (bytes, start) in [
        (&request_bytes, W.start_ns),
        (&infra_bytes, 540_000_000_000),
    ] {
        let r = decode(bytes);
        assert_eq!(r.resource_metrics.len(), 1);
        assert_eq!(r.resource_metrics[0].scope_metrics.len(), 1);
        for m in &r.resource_metrics[0].scope_metrics[0].metrics {
            match &m.data {
                Some(Data::Sum(s)) => {
                    for p in &s.data_points {
                        assert_eq!(
                            (p.start_time_unix_nano, p.time_unix_nano),
                            (start, W.end_ns)
                        );
                    }
                }
                Some(Data::Gauge(g)) => {
                    for p in &g.data_points {
                        assert_eq!((p.start_time_unix_nano, p.time_unix_nano), (0, W.end_ns));
                    }
                }
                _ => panic!("unexpected payload"),
            }
        }
    }
}

#[test]
fn all_or_none_sparse_variants_and_invalid_pairs() {
    for variant in 0..6 {
        let mut t = cohort(11, false, 0);
        match variant {
            0 => {
                t.generation_starts[4] = 1;
            } // rare authenticated model
            1 => {
                t.http_starts[3] = 1;
            } // rare endpoint
            2 => {
                t.generation_completed[41] = 1;
                t.generation_duration[41].0[2] = 1;
                t.first_output[41].0[1] = 1;
            }
            3 => {
                t.first_output[36].0[1] = 10;
            } // missing complement=1
            4 => {
                t.dispositions[255] = [10, 1, 0, 0, 0];
            } // duplicate complement
            _ => {
                t.rejected[(16 * 5 + 4) * 14 + 4] = 10;
            } // invalid pre-router/post-router pair
        }
        assert!(request(&t, W).is_none(), "variant {variant}");
    }
    // A valid request with no first output omits the metric, not zero bins.
    let materialized = request(&cohort(10, false, 10), W).unwrap();
    assert!(!materialized.scope_metrics[0]
        .metrics
        .iter()
        .any(|m| m.name == "possums.generation.first_output.bucket"));
}

#[test]
fn sdk_integer_overflow_cannot_become_zero() {
    let mut t = cohort(10, false, 0);
    t.http_starts[14] = i64::MAX as u64 + 1;
    assert!(t.releasable());
    assert!(request(&t, W).is_none());
}

#[tokio::test]
async fn maximum_permitted_infrastructure_pairs_wire() {
    if !clean_child(
        "telemetry::export::materialize::tests::maximum_permitted_infrastructure_pairs_wire",
    ) {
        return;
    }
    let mut infra = Infrastructure::default();
    for i in 0..6 {
        for (_, _, scope) in SCOPES {
            infra.observe(scope, ResourceMetric::CpuUsed, i, 1.);
            infra.observe(scope, ResourceMetric::MemoryUsed, i, 1048576.);
            if scope != ResourceScope::Process {
                infra.observe(scope, ResourceMetric::CpuCapacity, i, 2.);
                infra.observe(scope, ResourceMetric::MemoryCapacity, i, 8388608.);
            }
        }
        for (_, lane) in LANES {
            infra.configuration(lane, i, crate::telemetry::CAPACITIES[lane as usize]);
        }
    }
    assert_eq!(infra.series_count(), 20);
    let bytes = wire(infrastructure(&infra, I).unwrap()).await;
    let decoded = decode(&bytes);
    let metrics = &decoded.resource_metrics[0].scope_metrics[0].metrics;
    assert_eq!(metrics.len(), 5);
    assert_eq!(
        metrics
            .iter()
            .map(|m| match &m.data {
                Some(Data::Gauge(g)) => g.data_points.len(),
                _ => panic!("not gauge"),
            })
            .sum::<usize>(),
        20
    );
    for (j, m) in metrics.iter().enumerate() {
        let Some(Data::Gauge(g)) = &m.data else {
            panic!("not gauge")
        };
        assert_eq!(
            m.name,
            if j == 4 {
                "possums.admission.capacity"
            } else {
                RESOURCES[j].0
            }
        );
        assert_eq!(m.unit, if j == 4 { "{permit}" } else { RESOURCES[j].1 });
        for p in &g.data_points {
            assert_eq!((p.start_time_unix_nano, p.time_unix_nano), (0, I.end_ns));
            assert_eq!(p.flags, 0);
            assert!(p.exemplars.is_empty());
            assert_eq!(p.attributes.len(), if j == 4 { 3 } else { 2 });
        }
    }
    assert!(bytes.len() <= 6_576_128);
    eprintln!(
        "materialize max infrastructure actual_wire_bytes={} points=20",
        bytes.len()
    );
}
