use possums::{render, telemetry::AggregateMetrics};
use std::process::Command;

#[test]
fn telemetry_is_off_by_default_and_suppresses_sparse_buckets() {
    let off = AggregateMetrics::default();
    for _ in 0..20 {
        off.increment("requests_total");
    }
    assert!(off.exportable_snapshot().is_empty());

    let enabled = AggregateMetrics::new(true);
    enabled.increment("requests_total");
    enabled.increment("credential-canary");
    assert!(enabled.exportable_snapshot().is_empty());
    for _ in 1..10 {
        enabled.increment("requests_total");
    }
    assert_eq!(enabled.exportable_snapshot(), vec![("requests_total", 10)]);
}

#[test]
fn startup_errors_emit_no_sensitive_output_or_writable_artifacts() {
    let canary = "seeded-sensitive-credential-canary";
    let directory = std::env::temp_dir().join(format!(
        "possums-privacy-{}-{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    std::fs::create_dir(&directory).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_possums"))
        .current_dir(&directory)
        .env_clear()
        .env("POSSUMS_ACCOUNTS_JSON", canary)
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains(canary));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(canary));
    assert!(std::fs::read_dir(&directory).unwrap().next().is_none());
    std::fs::remove_dir(directory).unwrap();
}

#[test]
fn hostile_content_is_neither_active_nor_remotely_loaded() {
    let canary = "sensitive-canary.invalid";
    let rendered = render::markdown(&format!(
        "<script>fetch('https://{canary}')</script> ![x](https://{canary}/pixel)"
    ));
    assert!(!rendered.contains("<script"));
    assert!(!rendered.contains("<img"));
    assert!(!rendered.contains("src="));
}
