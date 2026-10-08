//! These tests link the ordinary library, with no cfg(test) runtime substitute.
use possums::telemetry::runtime::{Config, Runtime};

fn config(overrides: &[(&str, Option<&str>)]) -> Config {
    Config::from_lookup(|name| {
        if let Some((_, value)) = overrides.iter().find(|(key, _)| *key == name) {
            return value.map(str::to_owned);
        }
        match name {
            "OTEL_EXPORTER_OTLP_ENDPOINT" => Some("https://api.honeycomb.io".into()),
            "OTEL_EXPORTER_OTLP_HEADERS" => Some("x-honeycomb-team=synthetic-key-canary".into()),
            "OTEL_SERVICE_NAME" => Some("possums-gateway".into()),
            "OTEL_SDK_DISABLED" => None,
            _ => panic!("unexpected configuration read"),
        }
    })
}

#[test]
fn allowlisted_configuration_and_private_diagnostics() {
    assert!(config(&[]).enabled());
    assert!(config(&[("OTEL_SDK_DISABLED", Some("false"))]).enabled());
    assert!(config(&[("OTEL_SERVICE_NAME", None)]).enabled());
    assert!(!Config::from_lookup(|_| None).enabled());
    for (key, value) in [
        ("OTEL_SDK_DISABLED", Some("true")),
        ("OTEL_SDK_DISABLED", Some("invalid")),
        ("OTEL_SDK_DISABLED", Some("\0")),
        ("OTEL_EXPORTER_OTLP_ENDPOINT", None),
        (
            "OTEL_EXPORTER_OTLP_ENDPOINT",
            Some("http://api.honeycomb.io"),
        ),
        (
            "OTEL_EXPORTER_OTLP_ENDPOINT",
            Some("https://api.honeycomb.io.evil.invalid"),
        ),
        (
            "OTEL_EXPORTER_OTLP_ENDPOINT",
            Some("https://secret@api.honeycomb.io"),
        ),
        (
            "OTEL_EXPORTER_OTLP_ENDPOINT",
            Some("https://api.honeycomb.io/v1/metrics"),
        ),
        ("OTEL_SERVICE_NAME", Some("sensitive-name-canary")),
        ("OTEL_EXPORTER_OTLP_HEADERS", None),
        ("OTEL_EXPORTER_OTLP_HEADERS", Some("x-honeycomb-team=")),
        (
            "OTEL_EXPORTER_OTLP_HEADERS",
            Some("authorization=secret-canary"),
        ),
        (
            "OTEL_EXPORTER_OTLP_HEADERS",
            Some("x-honeycomb-team=secret-canary,x-extra=secret"),
        ),
        (
            "OTEL_EXPORTER_OTLP_HEADERS",
            Some("x-honeycomb-team=secret-canary\r\nx-extra: secret"),
        ),
        (
            "OTEL_EXPORTER_OTLP_HEADERS",
            Some("x-honeycomb-team=%73ecret"),
        ),
    ] {
        let config = config(&[(key, value)]);
        assert!(!config.enabled());
        assert_eq!(format!("{config:?}"), "TelemetryConfig { enabled: false }");
    }
    assert_eq!(
        format!("{:?}", config(&[])),
        "TelemetryConfig { enabled: true }"
    );
}

#[tokio::test]
async fn ordinary_component_start_stop_and_explicit_disable() {
    // The configured production source must first complete warmup and a full
    // minute. This immediate start/stop test cannot release a payload or use DNS.
    let runtime = Runtime::start(config(&[]));
    assert!(runtime.enabled());
    let metrics = runtime.metrics().expect("configured attachment");
    drop(metrics.http(possums::telemetry::Endpoint::ChatApi));
    assert!(metrics.request().is_none());
    tokio::task::yield_now().await;
    assert!(runtime.shutdown().await);
    let runtime = Runtime::start(config(&[("OTEL_SDK_DISABLED", Some("true"))]));
    assert!(!runtime.enabled());
    assert!(runtime.metrics().is_none());
    assert!(runtime.shutdown().await);
    let runtime = Runtime::start(Config::from_lookup(|_| None));
    assert!(runtime.metrics().is_none());
    assert!(runtime.shutdown().await);
    drop(Runtime::start(config(&[])));
}

#[test]
fn ambient_otel_defaults_cannot_extend_runtime_configuration() {
    const CHILD: &str = "POSSUMS_RUNTIME_CONFIG_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let config = Config::from_env();
        assert!(config.enabled());
        assert_eq!(format!("{config:?}"), "TelemetryConfig { enabled: true }");
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .env_clear()
        .env(CHILD, "1")
        .env("OTEL_EXPORTER_OTLP_ENDPOINT", "https://api.honeycomb.io")
        .env(
            "OTEL_EXPORTER_OTLP_HEADERS",
            "x-honeycomb-team=synthetic-key-canary",
        )
        .env(
            "OTEL_EXPORTER_OTLP_METRICS_HEADERS",
            "authorization=hostile-secret-canary",
        )
        .env(
            "OTEL_EXPORTER_OTLP_METRICS_ENDPOINT",
            "https://hostile-canary.invalid",
        )
        .env("OTEL_RESOURCE_ATTRIBUTES", "account=hostile-secret-canary")
        .env("OTEL_TRACES_EXPORTER", "otlp")
        .env("OTEL_LOGS_EXPORTER", "otlp")
        .env("HTTPS_PROXY", "https://hostile-secret-canary.invalid")
        .args([
            "--exact",
            "ambient_otel_defaults_cannot_extend_runtime_configuration",
            "--nocapture",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    for bytes in [&output.stdout, &output.stderr] {
        assert!(!String::from_utf8_lossy(bytes).contains("canary"));
    }
}
