use possums::{
    render,
    telemetry::{AggregateMetrics, Deployment, Endpoint, SystemClock},
};
use std::process::Command;

#[test]
fn telemetry_is_off_by_default_and_nonisolated_modes_are_silent() {
    for metrics in [
        AggregateMetrics::default(),
        AggregateMetrics::new(Deployment::NonIsolated, SystemClock::default()),
    ] {
        for _ in 0..20 {
            drop(metrics.http(Endpoint::ChatApi));
        }
        assert!(metrics.request().is_none());
        assert!(metrics.infrastructure().is_none());
        assert!(metrics.off());
    }
}

#[test]
fn hostile_routes_cannot_create_labels() {
    for path in [
        "/chat/",
        "/%63hat",
        "/v1",
        "/credential-canary?prompt=secret",
        "https://hostile.invalid/chat",
    ] {
        assert_eq!(Endpoint::route("POST", path), Endpoint::Other);
    }
    assert_eq!(Endpoint::route("GET", "/chat"), Endpoint::Other);
    assert_eq!(Endpoint::route("HEAD", "/v1/models"), Endpoint::Other);
    assert_eq!(Endpoint::route("OPTIONS", "/"), Endpoint::Other);
    assert_eq!(Endpoint::route("POST", "/chat"), Endpoint::ChatWeb);
    assert_eq!(Endpoint::route("HEAD", "/"), Endpoint::Home);
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
