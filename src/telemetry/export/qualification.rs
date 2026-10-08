use hyper::body::Bytes;
use opentelemetry_http::HttpClient;
use transport::{Client, Failure};

use opentelemetry::{InstrumentationScope, KeyValue};
use opentelemetry_otlp::{MetricExporter, WithExportConfig, WithHttpConfig};
use opentelemetry_proto::tonic::{
    collector::metrics::v1::ExportMetricsServiceRequest,
    metrics::v1::{metric::Data, number_data_point::Value, AggregationTemporality},
};
use opentelemetry_sdk::{
    metrics::{
        data::{
            Histogram, HistogramDataPoint, Metric, ResourceMetrics, ScopeMetrics, Sum, SumDataPoint,
        },
        exporter::PushMetricExporter,
        Temporality,
    },
    Resource,
};
use prost::Message;
use std::time::{Duration, UNIX_EPOCH};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const LIBRARY_BUDGET: usize = 8 * 1024 * 1024;
const RESPONSE_BYTES: usize = 9 * 1024 * 1024;
const START: u64 = 300_000_000_000;
const END: u64 = 600_000_000_000;
const BOUNDS: [f64; 10] = [0.1, 0.5, 1.0, 5.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0];
const BUCKETS: [&str; 11] = [
    "b00", "b01", "b02", "b03", "b04", "b05", "b06", "b07", "b08", "b09", "b10",
];

fn attributes() -> Vec<KeyValue> {
    vec![
        KeyValue::new("possums.endpoint", "chat_api"),
        KeyValue::new("status_class", "2xx"),
        KeyValue::new("http_terminal", "eof"),
    ]
}

// A single distribution qualification fixture, not a released request family or
// a substitute for V10's linked tables. It never enters production aggregation.
fn fixture(native: bool) -> ResourceMetrics {
    let data: Box<dyn opentelemetry_sdk::metrics::data::Aggregation> = if native {
        let mut bucket_counts = vec![0; 11];
        bucket_counts[3] = 10;
        Box::new(Histogram {
            data_points: vec![HistogramDataPoint {
                attributes: attributes(),
                count: 10,
                bounds: BOUNDS.to_vec(),
                bucket_counts,
                min: None,
                max: None,
                // Even this placeholder is serialized as Some(0), not absent.
                sum: 0_u64,
                exemplars: vec![],
            }],
            start_time: UNIX_EPOCH + Duration::from_nanos(START),
            time: UNIX_EPOCH + Duration::from_nanos(END),
            temporality: Temporality::Delta,
        })
    } else {
        Box::new(Sum {
            data_points: BUCKETS
                .iter()
                .enumerate()
                .map(|(index, bucket)| {
                    let mut attributes = attributes();
                    attributes.push(KeyValue::new("bucket", *bucket));
                    SumDataPoint {
                        attributes,
                        value: if index == 3 { 10_u64 } else { 0 },
                        exemplars: vec![],
                    }
                })
                .collect(),
            start_time: UNIX_EPOCH + Duration::from_nanos(START),
            time: UNIX_EPOCH + Duration::from_nanos(END),
            temporality: Temporality::Delta,
            is_monotonic: true,
        })
    };
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
            metrics: vec![Metric {
                name: if native {
                    "possums.http.duration"
                } else {
                    "possums.http.duration.bucket"
                }
                .into(),
                description: "".into(),
                unit: if native { "s" } else { "{observation}" }.into(),
                data,
            }],
        }],
    }
}

async fn capture_header(stream: &mut TcpStream, restricted: bool) -> usize {
    // Test peer only: bounded HTTP envelope capture; supported prost handles OTLP.
    let mut header = Vec::with_capacity(1024);
    while !header.ends_with(b"\r\n\r\n") {
        assert!(header.len() < 8192, "qualification request header bound");
        header.push(stream.read_u8().await.expect("loopback request header"));
    }
    let header = std::str::from_utf8(&header).expect("loopback HTTP header encoding");
    assert!(header.starts_with("POST /v1/metrics HTTP/1.1\r\n"));
    let length: usize = header
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().expect("loopback content length"))
        })
        .expect("loopback bounded request length");
    if restricted {
        let mut names = Vec::new();
        for line in header.lines().skip(1).filter(|line| !line.is_empty()) {
            let (name, value) = line.split_once(':').expect("synthetic header shape");
            let name = name.to_ascii_lowercase();
            assert!(
                matches!(
                    name.as_str(),
                    "host" | "content-type" | "content-length" | "connection"
                ),
                "outbound header allowlist"
            );
            match name.as_str() {
                "content-type" => assert!(value.trim() == "application/x-protobuf"),
                "connection" => assert!(value.trim() == "close"),
                "host" => assert!(value
                    .trim()
                    .parse::<std::net::SocketAddr>()
                    .is_ok_and(|address| address.ip().is_loopback())),
                _ => (),
            }
            names.push(name);
        }
        names.sort();
        assert!(names == ["connection", "content-length", "content-type", "host"]);
    }
    length
}

async fn capture(stream: &mut TcpStream) -> Vec<u8> {
    let length = capture_header(stream, false).await;
    assert!(length <= 64 * 1024, "qualification request body bound");
    let mut body = vec![0; length];
    stream
        .read_exact(&mut body)
        .await
        .expect("loopback request body");
    body
}

async fn exchange(native: bool, response_bytes: usize, success: bool) -> Vec<u8> {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("loopback bind");
    let address = listener.local_addr().expect("loopback address");
    let exporter = MetricExporter::builder()
        .with_http()
        .with_endpoint(format!("http://{address}/v1/metrics"))
        .with_timeout(ATTEMPT_TIMEOUT)
        .with_temporality(Temporality::Delta)
        .with_http_client(
            opentelemetry_http::hyper::HyperClient::with_default_connector(ATTEMPT_TIMEOUT, None),
        )
        .build()
        .expect("synthetic exporter construction");
    let mut metrics = fixture(native);
    let peer = async {
        let (mut stream, _) = listener.accept().await.expect("loopback accept");
        let body = capture(&mut stream).await;
        let status = if success { "200 OK" } else { "500 Synthetic" };
        let header = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {response_bytes}\r\nConnection: close\r\n\r\n"
        );
        stream
            .write_all(header.as_bytes())
            .await
            .expect("loopback response header");
        // No response-sized peer allocation to falsely attribute to the client.
        let chunk = [0_u8; 8192];
        let mut remaining = response_bytes;
        while remaining > 0 {
            let length = remaining.min(chunk.len());
            stream
                .write_all(&chunk[..length])
                .await
                .expect("loopback response chunk");
            remaining -= length;
        }
        stream.shutdown().await.expect("loopback peer shutdown");
        body
    };
    let isolated = std::env::var_os("POSSUMS_TELEMETRY_ALLOCATION_TEST").is_some();
    let before = if isolated {
        crate::process_alloc_tests::ALLOCATOR.begin_phase()
    } else {
        crate::process_alloc_tests::ALLOCATOR.snapshot()
    };
    // Neither future is spawned/detached; a failed bounded attempt drops both.
    let (result, bytes) = tokio::time::timeout(ATTEMPT_TIMEOUT, async {
        tokio::join!(exporter.export(&mut metrics), peer)
    })
    .await
    .expect("synthetic attempt exceeded one second");
    let after = crate::process_alloc_tests::ALLOCATOR.snapshot();
    assert_eq!(result.is_ok(), success, "synthetic export status category");
    if isolated && response_bytes == RESPONSE_BYTES {
        let peak = after.phase_peak.saturating_sub(before.live);
        assert!(
            peak > LIBRARY_BUDGET,
            "stock transport blocker did not reproduce"
        );
        eprintln!("stock transport success={success} response_bytes={response_bytes} incremental_requested_peak={peak} budget={LIBRARY_BUDGET}");
    }
    assert!(exporter.shutdown().is_ok());
    if response_bytes == 0 {
        eprintln!(
            "qualification payload native={native} serialized_bytes={}",
            bytes.len()
        );
    }
    bytes
}

fn decode(bytes: &[u8]) -> ExportMetricsServiceRequest {
    let decoded = ExportMetricsServiceRequest::decode(bytes).expect("synthetic OTLP decoding");
    // Inspect the actual wire capture; roundtrip must preserve every serialized field.
    assert_eq!(decoded.encode_to_vec(), bytes);
    let resource = &decoded.resource_metrics[0];
    assert_eq!(decoded.resource_metrics.len(), 1);
    assert!(resource.schema_url.is_empty());
    let attributes = &resource.resource.as_ref().unwrap().attributes;
    assert_eq!(attributes.len(), 3);
    for (key, value) in [
        ("service.name", "possums-gateway"),
        ("deployment.environment.name", "test"),
        ("possums.gateway.slot", "gateway-01"),
    ] {
        assert!(attributes.iter().any(|attribute| attribute.key == key
            && attribute.value.as_ref().unwrap().value
                == Some(
                    opentelemetry_proto::tonic::common::v1::any_value::Value::StringValue(
                        value.into()
                    )
                )));
    }
    assert_eq!(
        resource.resource.as_ref().unwrap().dropped_attributes_count,
        0
    );
    assert_eq!(resource.scope_metrics.len(), 1);
    let scope_metrics = &resource.scope_metrics[0];
    assert!(scope_metrics.schema_url.is_empty());
    let scope = scope_metrics.scope.as_ref().unwrap();
    assert_eq!(scope.name, "possums.telemetry");
    assert!(scope.version.is_empty());
    assert!(scope.attributes.is_empty());
    assert_eq!(scope.dropped_attributes_count, 0);
    assert_eq!(scope_metrics.metrics.len(), 1);
    assert!(scope_metrics.metrics[0].description.is_empty());
    decoded
}

// The SDK builder reads OTEL_* headers even with explicit configuration. Never
// let a qualification test read inherited credentials or export configuration.
fn clean_child(test: &str) -> bool {
    clean_child_env(test, &[])
}

fn clean_child_env(test: &str, environment: &[(&str, &str)]) -> bool {
    const CHILD: &str = "POSSUMS_EXPORT_QUALIFICATION_CHILD";
    if std::env::var_os(CHILD).is_some() {
        return true;
    }
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .env_clear()
        .env(CHILD, "1")
        .envs(environment.iter().copied())
        .args(["--exact", test, "--test-threads=1", "--nocapture"]);
    if std::env::var_os("POSSUMS_TELEMETRY_ALLOCATION_TEST").is_some() {
        command.env("POSSUMS_TELEMETRY_ALLOCATION_TEST", "1");
    }
    let output = command
        .output()
        .expect("synthetic qualification subprocess");
    // Only locally authored numeric allocation evidence is forwarded; do not
    // print raw SDK errors, panic payloads, HTTP headers or captured wire data.
    for line in String::from_utf8_lossy(&output.stderr).lines() {
        if line.starts_with("stock transport success=")
            || line.starts_with("qualification payload native=")
            || line.starts_with("candidate case=")
            || line.starts_with("materialize max ")
            || line.starts_with("serialize and bounded peer max ")
            || line.starts_with("handoff allocation ")
        {
            eprintln!("{line}");
        }
    }
    assert!(
        output.status.success(),
        "synthetic qualification child failed"
    );
    false
}

#[tokio::test]
async fn native_histogram_serializes_prohibited_sum_even_when_zero() {
    if !clean_child("telemetry::export::native_histogram_serializes_prohibited_sum_even_when_zero")
    {
        return;
    }
    let bytes = exchange(true, 0, true).await;
    let decoded = decode(&bytes);
    let metric = &decoded.resource_metrics[0].scope_metrics[0].metrics[0];
    let Some(Data::Histogram(histogram)) = &metric.data else {
        panic!("expected native histogram qualification fixture");
    };
    assert_eq!(
        histogram.aggregation_temporality,
        AggregationTemporality::Delta as i32
    );
    let point = &histogram.data_points[0];
    assert_eq!(point.sum, Some(0.0)); // PRESENT on the actual wire: native rejected.
    assert!(point.min.is_none() && point.max.is_none() && point.exemplars.is_empty());
    assert_eq!(point.count, 10);
    assert_eq!(point.bucket_counts, [0, 0, 0, 10, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(point.explicit_bounds, BOUNDS);
    assert_eq!(
        (point.start_time_unix_nano, point.time_unix_nano),
        (START, END)
    );
}

#[tokio::test]
async fn fallback_serializes_delta_bins_including_true_zeros() {
    if !clean_child("telemetry::export::fallback_serializes_delta_bins_including_true_zeros") {
        return;
    }
    let bytes = exchange(false, 0, true).await;
    assert_fallback(&bytes);
}

fn assert_fallback(bytes: &[u8]) {
    let decoded = decode(bytes);
    let metric = &decoded.resource_metrics[0].scope_metrics[0].metrics[0];
    assert_eq!(metric.name, "possums.http.duration.bucket");
    assert_eq!(metric.unit, "{observation}");
    let Some(Data::Sum(sum)) = &metric.data else {
        panic!("expected fallback sum qualification fixture");
    };
    assert_eq!(
        sum.aggregation_temporality,
        AggregationTemporality::Delta as i32
    );
    assert!(sum.is_monotonic);
    assert_eq!(sum.data_points.len(), 11);
    for (index, point) in sum.data_points.iter().enumerate() {
        assert_eq!(
            point.value,
            Some(Value::AsInt(if index == 3 { 10 } else { 0 }))
        );
        assert_eq!(
            (point.start_time_unix_nano, point.time_unix_nano),
            (START, END)
        );
        assert_eq!(point.flags, 0);
        assert!(point.exemplars.is_empty());
        let expected = attributes()
            .into_iter()
            .chain([KeyValue::new("bucket", BUCKETS[index])])
            .map(|attribute| {
                (
                    attribute.key.as_str().to_owned(),
                    attribute.value.to_string(),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let actual = point
            .attributes
            .iter()
            .map(|attribute| {
                let Some(opentelemetry_proto::tonic::common::v1::any_value::Value::StringValue(
                    value,
                )) = &attribute.value.as_ref().unwrap().value
                else {
                    panic!("expected closed string attribute");
                };
                (attribute.key.clone(), value.clone())
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(point.attributes.len(), 4);
        assert_eq!(actual, expected);
    }
}

#[tokio::test]
async fn stock_transport_exceeds_frozen_allocation_budget() {
    if !clean_child("telemetry::export::stock_transport_exceeds_frozen_allocation_budget") {
        return;
    }
    // Passing means the candidate is REFUTED, not that the pipeline is safe.
    // The global peak assertion runs only in the exact isolated command.
    for success in [true, false] {
        let bytes = exchange(false, RESPONSE_BYTES, success).await;
        decode(&bytes);
    }
}

fn candidate(client: Client) -> Result<MetricExporter, Failure> {
    // Do not read credential values or mutate process environment. Reject even
    // empty/non-Unicode inherited header settings BEFORE the SDK builder runs.
    if [
        "OTEL_EXPORTER_OTLP_HEADERS",
        "OTEL_EXPORTER_OTLP_METRICS_HEADERS",
    ]
    .iter()
    .any(|key| std::env::var_os(key).is_some())
    {
        return Err(Failure::Configuration);
    }
    MetricExporter::builder()
        .with_http()
        .with_endpoint(client.endpoint())
        .with_protocol(opentelemetry_otlp::Protocol::HttpBinary)
        .with_timeout(ATTEMPT_TIMEOUT)
        .with_temporality(Temporality::Delta)
        .with_http_client(client)
        .build()
        .map_err(|_| Failure::Configuration)
}

fn request(client: &Client, bytes: Bytes) -> http::Request<Bytes> {
    http::Request::builder()
        .method("POST")
        .uri(client.endpoint())
        // Independently hostile to the outbound-header boundary, not just env.
        .header("authorization", "synthetic-credential-canary")
        .header("user-agent", "synthetic-agent-canary")
        .header("x-synthetic", "synthetic-content-canary")
        .header("content-type", "synthetic/wrong")
        .body(bytes)
        .expect("synthetic request")
}

#[derive(Clone, Copy)]
enum Wire {
    Length,
    Absent,
    Chunked,
    TinyChunked,
    Truncated,
    Conflicting,
    HugeLength,
    Headers,
    LongHeader,
    Trailers,
    OneTrailer,
    LongTrailer,
}

async fn reply(stream: &mut TcpStream, wire: Wire, n: usize, status: u16) -> std::io::Result<()> {
    stream
        .write_all(format!("HTTP/1.1 {status} Synthetic\r\nConnection: close\r\n").as_bytes())
        .await?;
    if (300..400).contains(&status) {
        stream
            .write_all(b"Location: http://127.0.0.1:1/synthetic-no-follow\r\n")
            .await?;
    }
    match wire {
        Wire::Length | Wire::Truncated | Wire::Conflicting => {
            let declared = n + usize::from(matches!(wire, Wire::Truncated));
            stream
                .write_all(format!("Content-Length: {declared}\r\n").as_bytes())
                .await?;
            if matches!(wire, Wire::Conflicting) {
                stream.write_all(b"Content-Length: 999\r\n").await?;
            }
        }
        Wire::HugeLength => {
            stream
                .write_all(b"Content-Length: 18446744073709551615\r\n")
                .await?;
        }
        Wire::Chunked
        | Wire::TinyChunked
        | Wire::Trailers
        | Wire::OneTrailer
        | Wire::LongTrailer => {
            stream.write_all(b"Transfer-Encoding: chunked\r\n").await?;
        }
        Wire::Headers => {
            for _ in 0..65 {
                stream.write_all(b"X-Synthetic: 0\r\n").await?;
            }
        }
        Wire::LongHeader => {
            stream.write_all(b"X-Synthetic: ").await?;
            for _ in 0..5 {
                stream.write_all(&[b'x'; 8192]).await?;
            }
            stream.write_all(b"\r\n").await?;
        }
        Wire::Absent => (),
    }
    stream.write_all(b"\r\n").await?;
    if matches!(
        wire,
        Wire::Chunked | Wire::TinyChunked | Wire::Trailers | Wire::OneTrailer | Wire::LongTrailer
    ) {
        // Many tiny HTTP frames, no response-sized peer allocation.
        let mut tiny_frames = [0_u8; 6 * 1024];
        for frame in tiny_frames.chunks_exact_mut(6) {
            frame.copy_from_slice(b"1\r\nx\r\n");
        }
        let mut remaining = n;
        while remaining > 0 {
            if matches!(wire, Wire::TinyChunked) {
                let frames = remaining.min(1024);
                stream.write_all(&tiny_frames[..frames * 6]).await?;
                remaining -= frames;
            } else {
                let bytes = remaining.min(8192);
                stream
                    .write_all(format!("{bytes:x}\r\n").as_bytes())
                    .await?;
                stream.write_all(&[b'x'; 8192][..bytes]).await?;
                stream.write_all(b"\r\n").await?;
                remaining -= bytes;
            }
        }
        stream.write_all(b"0\r\n").await?;
        if matches!(wire, Wire::OneTrailer) {
            stream.write_all(b"X-Synthetic: 0\r\n").await?;
        }
        if matches!(wire, Wire::Trailers) {
            for _ in 0..65 {
                stream.write_all(b"X-Synthetic: 0\r\n").await?;
            }
        }
        if matches!(wire, Wire::LongTrailer) {
            stream.write_all(b"X-Synthetic: ").await?;
            for _ in 0..5 {
                stream.write_all(&[b'x'; 8192]).await?;
            }
            stream.write_all(b"\r\n").await?;
        }
        stream.write_all(b"\r\n").await?;
    } else {
        let chunk = [b'x'; 8192];
        let mut remaining = n;
        while remaining > 0 {
            let length = remaining.min(chunk.len());
            stream.write_all(&chunk[..length]).await?;
            remaining -= length;
        }
    }
    stream.shutdown().await
}

fn phase() -> crate::process_alloc_tests::Snapshot {
    if std::env::var_os("POSSUMS_TELEMETRY_ALLOCATION_TEST").is_some() {
        crate::process_alloc_tests::ALLOCATOR.begin_phase()
    } else {
        crate::process_alloc_tests::ALLOCATOR.snapshot()
    }
}

fn peak(case: usize, before: crate::process_alloc_tests::Snapshot) {
    if std::env::var_os("POSSUMS_TELEMETRY_ALLOCATION_TEST").is_some() {
        let peak = crate::process_alloc_tests::ALLOCATOR
            .snapshot()
            .phase_peak
            .saturating_sub(before.live);
        eprintln!(
            "candidate case={case} incremental_requested_peak={peak} budget={LIBRARY_BUDGET}"
        );
        assert!(
            peak <= LIBRARY_BUDGET,
            "candidate transport allocation ceiling"
        );
    }
}

#[tokio::test]
async fn candidate_wire_and_response_matrix() {
    if !clean_child("telemetry::export::candidate_wire_and_response_matrix") {
        return;
    }
    use transport::RESPONSE_LIMIT as CAP;
    let cases = [
        (Wire::Length, 0, 200, true),
        (Wire::Length, 17, 200, true),
        (Wire::Length, CAP, 200, true),
        (Wire::Length, CAP + 1, 200, false),
        (Wire::Length, RESPONSE_BYTES, 200, false),
        (Wire::Length, RESPONSE_BYTES, 500, false),
        (Wire::Absent, 17, 200, true),
        (Wire::Absent, CAP + 1, 200, false),
        (Wire::Chunked, CAP, 200, true),
        (Wire::Chunked, CAP + 1, 200, false),
        (Wire::Truncated, 17, 200, false),
        (Wire::Conflicting, 17, 200, false),
        (Wire::Headers, 0, 200, false),
        (Wire::LongHeader, 0, 200, false),
        (Wire::Trailers, 1, 200, false),
        (Wire::LongTrailer, 1, 200, false),
        (Wire::Length, 0, 301, false),
        (Wire::Length, 0, 302, false),
        (Wire::Length, 0, 307, false),
        (Wire::Length, 0, 308, false),
        (Wire::Length, 0, 400, false),
        (Wire::Length, 0, 429, false),
        (Wire::Length, 0, 500, false),
        (Wire::Length, 0, 503, false),
        (Wire::Length, 0, 101, false),
        (Wire::Length, 0, 204, true),
        (Wire::TinyChunked, 1024, 200, true),
        (Wire::HugeLength, 0, 200, false),
        (Wire::OneTrailer, 1, 200, false),
    ];
    for (case, (wire, n, status, expected)) in cases.into_iter().enumerate() {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let client = Client::new(listener.local_addr().unwrap()).unwrap();
        let evidence = client.evidence();
        let exporter = candidate(client.clone()).expect("candidate construction");
        let mut metrics = fixture(false);
        let peer = async {
            let (mut stream, _) = listener.accept().await.unwrap();
            let length = capture_header(&mut stream, true).await;
            assert!(length <= 64 * 1024);
            let mut bytes = vec![0; length];
            stream
                .read_exact(&mut bytes)
                .await
                .expect("candidate payload capture");
            // Rejection is allowed to close before the hostile peer finishes.
            let _ = reply(&mut stream, wire, n, status).await;
            bytes
        };
        let before = phase();
        let (result, bytes) = tokio::time::timeout(Duration::from_secs(2), async {
            tokio::join!(exporter.export(&mut metrics), peer)
        })
        .await
        .expect("candidate matrix supervisor deadline");
        peak(case, before);
        assert_eq!(result.is_ok(), expected, "candidate matrix case {case}");
        assert_fallback(&bytes);
        assert_eq!(bytes.len(), 1571);
        use std::sync::atomic::Ordering::SeqCst;
        assert_eq!(evidence.connections.load(SeqCst), 1);
        assert_eq!(evidence.live_io.load(SeqCst), 0);
        assert!(
            tokio::time::timeout(Duration::from_millis(10), listener.accept())
                .await
                .is_err()
        );
        client.cancel().await;
        assert!(exporter.shutdown().is_ok());
    }
}

const HOSTILE_ENV: &[(&str, &str)] = &[
    (
        "OTEL_EXPORTER_OTLP_ENDPOINT",
        "http://synthetic-only.invalid:9",
    ),
    (
        "OTEL_EXPORTER_OTLP_METRICS_ENDPOINT",
        "http://synthetic-only.invalid:9",
    ),
    ("OTEL_EXPORTER_OTLP_PROTOCOL", "grpc"),
    ("OTEL_EXPORTER_OTLP_METRICS_PROTOCOL", "http/json"),
    ("OTEL_EXPORTER_OTLP_TIMEOUT", "999999"),
    ("OTEL_EXPORTER_OTLP_METRICS_TIMEOUT", "999999"),
    ("OTEL_EXPORTER_OTLP_COMPRESSION", "gzip"),
    ("OTEL_EXPORTER_OTLP_METRICS_COMPRESSION", "gzip"),
    (
        "OTEL_RESOURCE_ATTRIBUTES",
        "synthetic.secret=synthetic-resource-canary",
    ),
    ("OTEL_SERVICE_NAME", "synthetic-service-canary"),
    ("HTTP_PROXY", "http://synthetic-only.invalid:9"),
    ("HTTPS_PROXY", "http://synthetic-only.invalid:9"),
    ("ALL_PROXY", "http://synthetic-only.invalid:9"),
    ("http_proxy", "http://synthetic-only.invalid:9"),
    ("https_proxy", "http://synthetic-only.invalid:9"),
    ("all_proxy", "http://synthetic-only.invalid:9"),
    ("NO_PROXY", ""),
];

#[tokio::test]
async fn candidate_environment_and_header_boundary() {
    let name = "telemetry::export::candidate_environment_and_header_boundary";
    if !clean_child_env(name, HOSTILE_ENV) {
        for key in [
            "OTEL_EXPORTER_OTLP_HEADERS",
            "OTEL_EXPORTER_OTLP_METRICS_HEADERS",
        ] {
            for value in ["authorization=synthetic-credential-canary", ""] {
                let mut environment = HOSTILE_ENV.to_vec();
                environment.push((key, value));
                assert!(!clean_child_env(name, &environment));
            }
        }
        return;
    }
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let client = Client::new(listener.local_addr().unwrap()).unwrap();
    let exporter = candidate(client.clone());
    if [
        "OTEL_EXPORTER_OTLP_HEADERS",
        "OTEL_EXPORTER_OTLP_METRICS_HEADERS",
    ]
    .iter()
    .any(|key| std::env::var_os(key).is_some())
    {
        assert!(matches!(exporter, Err(Failure::Configuration)));
        assert_eq!(
            client
                .evidence()
                .connections
                .load(std::sync::atomic::Ordering::SeqCst),
            0
        );
        return;
    }
    let exporter = exporter.expect("explicit configuration candidate");
    let mut metrics = fixture(false);
    let peer = async {
        let (mut stream, _) = listener.accept().await.unwrap();
        let length = capture_header(&mut stream, true).await;
        assert!(length <= 64 * 1024);
        let mut bytes = vec![0; length];
        stream.read_exact(&mut bytes).await.unwrap();
        reply(&mut stream, Wire::Length, 0, 200).await.unwrap();
        bytes
    };
    let (result, bytes) = tokio::join!(exporter.export(&mut metrics), peer);
    assert!(result.is_ok());
    assert_fallback(&bytes);
    // Direct HttpClient callers cannot smuggle headers either.
    let outbound = request(&client, Bytes::from_static(b"synthetic"));
    let peer = async {
        let (mut stream, _) = listener.accept().await.unwrap();
        let length = capture_header(&mut stream, true).await;
        assert_eq!(length, 9);
        let mut bytes = [0; 9];
        stream.read_exact(&mut bytes).await.unwrap();
        reply(&mut stream, Wire::Length, 0, 200).await.unwrap();
    };
    let (result, ()) = tokio::join!(client.send_bytes(outbound), peer);
    assert!(result.is_ok());
    assert_eq!(format!("{client:?}"), "TelemetryClient");
    for failure in [
        Failure::Configuration,
        Failure::Unavailable,
        Failure::Transport,
        Failure::Response,
        Failure::Expired,
    ] {
        assert!(failure.to_string().starts_with("telemetry "));
        assert!(!format!("{failure:?}").contains("synthetic"));
    }
    client.cancel().await;
    assert!(exporter.shutdown().is_ok());
    // Hostile inherited timeout must not change the actual whole-attempt limit.
    stop_case(400, StopAt::Headers, false).await;
}

#[derive(Clone, Copy)]
enum StopAt {
    Connect,
    Upload,
    Headers,
    Body,
    Complete,
    SlowBody,
}

async fn drain_request(stream: &mut TcpStream) -> usize {
    let n = capture_header(stream, true).await;
    assert!(n <= transport::MAX_OUTBOUND);
    let mut remaining = n;
    let mut buf = [0; 8192];
    while remaining > 0 {
        let length = remaining.min(buf.len());
        stream
            .read_exact(&mut buf[..length])
            .await
            .expect("bounded synthetic drain");
        remaining -= length;
    }
    n
}

async fn stop_case(case: usize, at: StopAt, cancel: bool) {
    use std::sync::atomic::Ordering::SeqCst;
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let mut client = Client::new(listener.local_addr().unwrap()).unwrap();
    client.stall_connect = matches!(at, StopAt::Connect);
    let stale = client.clone();
    let evidence = client.evidence();
    let size = if matches!(at, StopAt::Upload) {
        transport::MAX_OUTBOUND
    } else {
        9
    };
    // Already-owned serialized buffer is a DIFFERENT budget. Baseline includes it;
    // the transport must not clone its contents or allocate another size-N body.
    let owned_bytes = Bytes::from(vec![0; size]);
    let outbound = request(&client, owned_bytes.clone());
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    let peer = async {
        if matches!(at, StopAt::Connect) {
            while evidence.connecting.load(SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        } else {
            let (mut stream, _) = listener.accept().await.unwrap();
            if matches!(at, StopAt::Upload) {
                while evidence.pending_writes.load(SeqCst) == 0
                    || evidence.written.load(SeqCst) == 0
                {
                    tokio::task::yield_now().await;
                }
                assert!(evidence.written.load(SeqCst) < transport::MAX_OUTBOUND);
            } else {
                drain_request(&mut stream).await;
                if matches!(at, StopAt::SlowBody) {
                    tokio::time::sleep(Duration::from_millis(650)).await;
                }
                if matches!(at, StopAt::Body | StopAt::SlowBody) {
                    stream
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nx")
                        .await
                        .unwrap();
                } else if matches!(at, StopAt::Complete) {
                    reply(&mut stream, Wire::Length, 0, 200).await.unwrap();
                }
            }
            ready_tx.send(()).unwrap();
            if matches!(at, StopAt::SlowBody) {
                tokio::time::sleep(Duration::from_millis(650)).await;
                let _ = stream.write_all(b"x").await;
            }
            let _ = done_rx.await;
            return;
        }
        ready_tx.send(()).unwrap();
        let _ = done_rx.await;
    };
    let controller = async {
        ready_rx.await.unwrap();
        if cancel {
            // Let response/body progress before cancellation; Complete deliberately
            // races completion. No assertion mistakes buffered peer bytes for writes.
            tokio::task::yield_now().await;
            if matches!(at, StopAt::Body) {
                while evidence.received.load(SeqCst) == 0 {
                    tokio::task::yield_now().await;
                }
            }
            let start = tokio::time::Instant::now();
            tokio::time::timeout(ATTEMPT_TIMEOUT, client.cancel())
                .await
                .expect("bounded cancel acknowledgement");
            assert!(start.elapsed() <= ATTEMPT_TIMEOUT);
            assert_eq!(evidence.live_io.load(SeqCst), 0);
            let polls = evidence.write_polls.load(SeqCst);
            let written = evidence.written.load(SeqCst);
            let connects = evidence.connecting.load(SeqCst);
            for _ in 0..8 {
                let result = stale.send_bytes(request(&stale, Bytes::new())).await;
                assert!(result.is_err_and(
                    |error| error.downcast_ref::<Failure>() == Some(&Failure::Unavailable)
                ));
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
            assert_eq!(evidence.write_polls.load(SeqCst), polls);
            assert_eq!(evidence.written.load(SeqCst), written);
            assert_eq!(evidence.connecting.load(SeqCst), connects);
        }
    };
    let sender = async {
        let start = tokio::time::Instant::now();
        let result = client.send_bytes(outbound).await;
        if matches!(at, StopAt::Complete) {
            assert!(
                result.is_ok()
                    || result.as_ref().is_err_and(
                        |error| error.downcast_ref::<Failure>() == Some(&Failure::Unavailable)
                    ),
                "completion race closed outcomes"
            );
        } else {
            let expected = if cancel {
                Failure::Unavailable
            } else {
                Failure::Expired
            };
            assert!(
                result.is_err_and(|error| error.downcast_ref::<Failure>() == Some(&expected)),
                "closed stop category"
            );
        }
        if !cancel {
            assert!(start.elapsed() >= ATTEMPT_TIMEOUT);
            // Wall scheduling tolerance is NOT an extension to the 1s deadline.
            assert!(start.elapsed() < Duration::from_millis(1200));
        }
        assert_eq!(evidence.live_io.load(SeqCst), 0);
        let _ = done_tx.send(());
    };
    let before = phase();
    tokio::time::timeout(Duration::from_secs(3), async {
        tokio::join!(sender, peer, controller)
    })
    .await
    .expect("stop supervisor deadline");
    peak(case, before);
    drop(owned_bytes); // keep baseline storage live throughout the measured phase
    client.cancel().await;
    assert_eq!(evidence.live_io.load(SeqCst), 0);
}

#[tokio::test]
async fn candidate_deadlines_cancellation_and_pressure() {
    if !clean_child("telemetry::export::candidate_deadlines_cancellation_and_pressure") {
        return;
    }
    for (case, at) in [
        StopAt::Connect,
        StopAt::Upload,
        StopAt::Headers,
        StopAt::Body,
        StopAt::SlowBody,
    ]
    .into_iter()
    .enumerate()
    {
        stop_case(100 + case, at, false).await;
    }
    for round in 0..8 {
        for (case, at) in [
            StopAt::Connect,
            StopAt::Upload,
            StopAt::Headers,
            StopAt::Body,
            StopAt::Complete,
        ]
        .into_iter()
        .enumerate()
        {
            stop_case(200 + round * 5 + case, at, true).await;
        }
    }
}

#[tokio::test]
async fn candidate_owned_maximum_outbound_and_refusal() {
    if !clean_child("telemetry::export::candidate_owned_maximum_outbound_and_refusal") {
        return;
    }
    use std::sync::atomic::Ordering::SeqCst;
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let client = Client::new(listener.local_addr().unwrap()).unwrap();
    for case in 300..304 {
        let owned_bytes = Bytes::from(vec![0; transport::MAX_OUTBOUND]);
        // Shallow Bytes clone keeps the already-charged allocation alive, so
        // freeing it during upload cannot hide later response allocations.
        let outbound = request(&client, owned_bytes.clone());
        let peer = async {
            let (mut stream, _) = listener.accept().await.unwrap();
            assert_eq!(drain_request(&mut stream).await, transport::MAX_OUTBOUND);
            let _ = reply(&mut stream, Wire::Chunked, transport::RESPONSE_LIMIT, 200).await;
        };
        let before = phase();
        let (result, ()) = tokio::time::timeout(Duration::from_secs(2), async {
            tokio::join!(client.send_bytes(outbound), peer)
        })
        .await
        .expect("maximum outbound supervisor");
        peak(case, before);
        drop(owned_bytes);
        assert!(result.is_ok());
        assert_eq!(client.evidence().live_io.load(SeqCst), 0);
    }
    drop(listener);
    let result = client.send_bytes(request(&client, Bytes::new())).await;
    assert!(result.is_err_and(|error| error.downcast_ref::<Failure>() == Some(&Failure::Transport)));
    let connects = client.evidence().connecting.load(SeqCst);
    let mut wrong_uri = request(&client, Bytes::new());
    *wrong_uri.uri_mut() = "http://synthetic-only.invalid/v1/metrics".parse().unwrap();
    assert!(client.send_bytes(wrong_uri).await.is_err());
    let oversized = request(&client, Bytes::from(vec![0; transport::MAX_OUTBOUND + 1]));
    assert!(client.send_bytes(oversized).await.is_err());
    assert_eq!(client.evidence().connecting.load(SeqCst), connects);
    assert!(Client::new("192.0.2.1:1".parse().unwrap()).is_err());
    client.cancel().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn candidate_exporter_shutdown_and_joined_cancel() {
    if !clean_child("telemetry::export::candidate_exporter_shutdown_and_joined_cancel") {
        return;
    }
    use std::sync::{atomic::Ordering::SeqCst, Arc};
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let client = Client::new(listener.local_addr().unwrap()).unwrap();
    let exporter = Arc::new(candidate(client.clone()).unwrap());
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    // The supervisor owns and joins this task; transport never spawns one.
    let owned_exporter = exporter.clone();
    let mut tasks = tokio::task::JoinSet::new();
    let task = tasks.spawn(async move { owned_exporter.export(&mut fixture(false)).await });
    let peer = async {
        let (mut stream, _) = listener.accept().await.unwrap();
        drain_request(&mut stream).await;
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nx")
            .await
            .unwrap();
        ready_tx.send(()).unwrap();
        let mut byte = [0; 1];
        // Socket close, not a timeout/reference-drop, is observed by the peer.
        let result = stream.read(&mut byte).await;
        assert!(matches!(result, Ok(0) | Err(_)));
    };
    let supervisor = async {
        ready_rx.await.unwrap();
        assert!(exporter.shutdown().is_ok());
        assert!(
            !task.is_finished(),
            "shutdown alone must not certify cancellation"
        );
        assert!(client
            .send_bytes(request(&client, Bytes::new()))
            .await
            .is_err());
        let start = tokio::time::Instant::now();
        client.cancel().await;
        assert!(start.elapsed() <= ATTEMPT_TIMEOUT);
        let joined = tasks
            .join_next()
            .await
            .expect("owned task present")
            .expect("owned task joined");
        assert!(joined.is_err());
        assert_eq!(client.evidence().live_io.load(SeqCst), 0);
        assert!(client
            .send_bytes(request(&client, Bytes::new()))
            .await
            .is_err());
    };
    // Both futures live in this scope; no task handle is dropped on success.
    tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(supervisor, peer)
    })
    .await
    .expect("joined cancellation supervisor deadline");
    assert!(tasks.is_empty());
}
