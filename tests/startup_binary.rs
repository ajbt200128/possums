//! Actual process startup failures, before provider construction. Offline only.
use std::{
    process::{Output, Stdio},
    time::Duration,
};
use tokio::process::Command;

const CANARY: &str = "HOSTILE_STARTUP_SECRET_CANARY";

fn startup() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_possums"));
    command
        .env_clear()
        .env("OTEL_SDK_DISABLED", "true")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    command
}

async fn run(mut command: Command) -> Output {
    tokio::time::timeout(Duration::from_secs(10), command.output())
        .await
        .expect("offline startup fixture exceeded its bound")
        .expect("start fixture binary")
}

fn assert_silent(output: Output, exit: i32) {
    assert_eq!(output.status.code(), Some(exit));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[tokio::test]
async fn missing_config_remains_exit_two_without_startup_diagnostics() {
    assert_silent(run(startup()).await, 2);
}

#[tokio::test]
async fn export_off_with_valid_ingestion_configuration_keeps_startup_silent() {
    let mut command = startup();
    command
        .env("OTEL_EXPORTER_OTLP_ENDPOINT", "https://api.honeycomb.io")
        .env(
            "OTEL_EXPORTER_OTLP_HEADERS",
            format!("x-honeycomb-team={CANARY}"),
        );
    assert_silent(run(command).await, 2);
}

#[tokio::test]
async fn hostile_accounts_and_private_configuration_remain_silent() {
    let mut command = startup();
    command
        .env(
            "POSSUMS_ACCOUNTS_JSON",
            format!("{{\"{CANARY}\":\"{CANARY}\"}}"),
        )
        .env("POSSUMS_TINFOIL_HOST", CANARY)
        .env("POSSUMS_TINFOIL_REPOSITORY", CANARY)
        .env("TINFOIL_API_KEY", CANARY)
        .env("POSSUMS_GATEWAY_ATTESTATION_SOCKET", CANARY)
        .env("POSSUMS_GATEWAY_REPOSITORY", CANARY)
        // Invalid telemetry must fail closed without external work. Its error
        // must not replace the primary account failure or echo its value.
        .env("OTEL_SDK_DISABLED", "false")
        .env("OTEL_EXPORTER_OTLP_ENDPOINT", CANARY)
        .env("OTEL_EXPORTER_OTLP_HEADERS", CANARY);
    assert_silent(run(command).await, 3);
}

#[cfg(unix)]
#[tokio::test]
async fn non_unicode_configuration_preserves_exit_eight_without_echoing_bytes() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let mut value = CANARY.as_bytes().to_vec();
    value.push(0xff);
    let mut command = startup();
    command.env("POSSUMS_ACCOUNTS_JSON", OsString::from_vec(value));
    assert_silent(run(command).await, 8);
}
