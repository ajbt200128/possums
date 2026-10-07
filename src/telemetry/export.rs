//! Test-only packet 2C exporter qualification, NOT a runtime sender.
//! The stock 0.29 HTTP transport exceeds the frozen response allocation budget.
//! Keep aggregation views and serving startup unchanged until reviewed resolution.
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

const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(1);
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

async fn capture(stream: &mut TcpStream) -> Vec<u8> {
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
    const CHILD: &str = "POSSUMS_EXPORT_QUALIFICATION_CHILD";
    if std::env::var_os(CHILD).is_some() {
        return true;
    }
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .env_clear()
        .env(CHILD, "1")
        .args(["--exact", test, "--test-threads=1", "--nocapture"]);
    if std::env::var_os("POSSUMS_TELEMETRY_ALLOCATION_TEST").is_some() {
        command.env("POSSUMS_TELEMETRY_ALLOCATION_TEST", "1");
    }
    let output = command
        .output()
        .expect("synthetic qualification subprocess");
    assert!(
        output.status.success(),
        "synthetic qualification child failed"
    );
    // Only locally authored numeric allocation evidence is forwarded; do not
    // print raw SDK errors, panic payloads, HTTP headers or captured wire data.
    for line in String::from_utf8_lossy(&output.stderr).lines() {
        if line.starts_with("stock transport success=")
            || line.starts_with("qualification payload native=")
        {
            eprintln!("{line}");
        }
    }
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
    let decoded = decode(&bytes);
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
