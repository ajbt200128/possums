//! Actual process startup failures, before provider/Tor construction. Offline only.
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
        .env("POSSUMS_TOR_ENABLED", "false")
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

fn assert_closed(output: Output, exit: i32, code: &str) {
    assert_eq!(output.status.code(), Some(exit));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains(code), "expected closed diagnostic code");
    assert!(stderr.contains("constraint="));
    assert!(stderr.contains("action="));
    assert!(!stderr.contains(CANARY));
    assert!(!stderr.contains("initialization_complete"));
    assert!(stderr.len() < 16 * 1024);
}

#[tokio::test]
async fn missing_config_remains_exit_two_and_is_visible_with_export_disabled() {
    assert_closed(run(startup()).await, 2, "configuration_missing");
}

#[tokio::test]
async fn hostile_accounts_and_private_configuration_never_enter_platform_diagnostics() {
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
        .env("POSSUMS_TOR_IDENTITY_NATIVE_KEY", CANARY)
        // Invalid telemetry must fail closed without external work. Its error
        // must not replace the primary account failure or echo its value.
        .env("OTEL_SDK_DISABLED", "false")
        .env("OTEL_EXPORTER_OTLP_ENDPOINT", CANARY)
        .env("OTEL_EXPORTER_OTLP_HEADERS", CANARY);
    assert_closed(run(command).await, 3, "accounts_invalid");
}

#[tokio::test]
async fn malformed_tor_opt_in_is_diagnosed_without_reading_identity_values() {
    let mut command = startup();
    command
        .env("POSSUMS_TOR_ENABLED", CANARY)
        .env("POSSUMS_TOR_IDENTITY_NATIVE_KEY", CANARY);
    assert_closed(run(command).await, 10, "tor_configuration_invalid");
}

#[cfg(unix)]
#[tokio::test]
async fn non_unicode_configuration_preserves_exit_eight_without_echoing_bytes() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let mut value = CANARY.as_bytes().to_vec();
    value.push(0xff);
    let mut command = startup();
    command.env("POSSUMS_ACCOUNTS_JSON", OsString::from_vec(value));
    assert_closed(run(command).await, 8, "configuration_encoding_invalid");
}
