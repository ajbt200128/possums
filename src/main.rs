use possums::{
    attestation::TinfoilEvidenceVerifier,
    auth::Auth,
    inference::{InferenceError, InferenceFailure, TinfoilInference},
    server::{serve_until, AppState, ServerFailure},
    telemetry::runtime::{Config as TelemetryConfig, Runtime as TelemetryRuntime, ShutdownFailure},
};
use std::{env, process::ExitCode, sync::Arc};

#[tokio::main]
async fn main() -> ExitCode {
    std::panic::set_hook(Box::new(|_| {}));
    if env::args_os().skip(1).collect::<Vec<_>>() == ["--healthcheck"] {
        return match possums::health::checked().await {
            Ok(()) => ExitCode::SUCCESS,
            Err(failure) => {
                eprintln!("healthcheck_failed: {}", failure.diagnostic());
                ExitCode::FAILURE
            }
        };
    }
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        // Closed stage-specific exit codes remain nonzero and add no automatic
        // startup logging, telemetry or support artifacts.
        Err(failure) => ExitCode::from(failure.exit_code()),
    }
}

/// Closed, locally authored startup stages. The connect stage retains the
/// supported [`InferenceFailure`] source; no raw SDK/io/env/config string is
/// ever stored. Environment failures are classified from supported enum
/// variants, without retaining names or values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StartupFailure {
    ConfigurationMissing,
    ConfigurationEncodingInvalid,
    AccountsInvalid,
    Connect(InferenceFailure),
    BindUnknown,
    SignalUnknown,
    Serve(ServerFailure),
    ServeUnknown,
    Cleanup(ShutdownFailure),
}

#[allow(dead_code)]
impl StartupFailure {
    fn exit_code(self) -> u8 {
        match self {
            Self::ConfigurationMissing => 2,
            Self::AccountsInvalid => 3,
            Self::Connect(_) => 4,
            Self::BindUnknown => 5,
            Self::SignalUnknown => 6,
            Self::Serve(_) | Self::ServeUnknown => 7,
            Self::Cleanup(_) => 9,
            Self::ConfigurationEncodingInvalid => 8,
        }
    }

    fn code(self) -> &'static str {
        match self {
            Self::ConfigurationMissing => "configuration_missing",
            Self::ConfigurationEncodingInvalid => "configuration_encoding_invalid",
            Self::AccountsInvalid => "accounts_invalid",
            Self::Connect(_) => "connect_unknown",
            Self::BindUnknown => "bind_unknown",
            Self::SignalUnknown => "signal_unknown",
            Self::Serve(ServerFailure::Accept) => "listener_accept_unknown",
            Self::Serve(ServerFailure::ConnectionTask) => "connection_task_unknown",
            Self::Serve(ServerFailure::ShutdownIntegrity) => "shutdown_integrity_failed",
            Self::ServeUnknown => "serve_unknown",
            Self::Cleanup(ShutdownFailure::ControllerTaskUnknown) => "telemetry_controller_unknown",
            Self::Cleanup(ShutdownFailure::DisposalUnconfirmed) => "telemetry_disposal_unconfirmed",
        }
    }

    fn stage(self) -> &'static str {
        match self {
            Self::ConfigurationMissing | Self::ConfigurationEncodingInvalid => "configuration",
            Self::AccountsInvalid => "accounts",
            Self::Connect(_) => "connect",
            Self::BindUnknown => "bind",
            Self::SignalUnknown => "signal",
            Self::Serve(_) | Self::ServeUnknown => "serve",
            Self::Cleanup(_) => "cleanup",
        }
    }
}

impl From<env::VarError> for StartupFailure {
    fn from(error: env::VarError) -> Self {
        match error {
            env::VarError::NotPresent => Self::ConfigurationMissing,
            env::VarError::NotUnicode(_) => Self::ConfigurationEncodingInvalid,
        }
    }
}

impl From<std::io::Error> for StartupFailure {
    fn from(error: std::io::Error) -> Self {
        error
            .get_ref()
            .and_then(|source| source.downcast_ref::<ServerFailure>())
            .copied()
            .map(Self::Serve)
            .unwrap_or(Self::ServeUnknown)
    }
}

impl From<InferenceError> for StartupFailure {
    fn from(error: InferenceError) -> Self {
        Self::Connect(error.failure())
    }
}

async fn run() -> Result<(), StartupFailure> {
    let accounts = env::var("POSSUMS_ACCOUNTS_JSON")?;
    let auth = Auth::from_json(&accounts).map_err(|_| StartupFailure::AccountsInvalid)?;
    let host = env::var("POSSUMS_TINFOIL_HOST")?;
    let repository = env::var("POSSUMS_TINFOIL_REPOSITORY")?;
    let api_key = env::var("TINFOIL_API_KEY")?;
    let attestation_socket = env::var("POSSUMS_GATEWAY_ATTESTATION_SOCKET")?;
    let gateway_repository = env::var("POSSUMS_GATEWAY_REPOSITORY")?;
    let bind = match env::var("POSSUMS_BIND") {
        Ok(bind) => bind,
        Err(env::VarError::NotPresent) => "127.0.0.1:8080".into(),
        Err(error) => return Err(error.into()),
    };

    let inference = TinfoilInference::connect(&host, &repository, api_key)
        .await
        .map_err(StartupFailure::from)?;
    let state = AppState::new(
        auth,
        Arc::new(inference),
        Arc::<str>::from(attestation_socket),
        Arc::new(TinfoilEvidenceVerifier::new(
            "/bin/possums-attestation",
            gateway_repository,
        )),
    );
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .map_err(|_| StartupFailure::BindUnknown)?;
    // Register both handlers before the first accept. Never serve without a
    // working termination signal subscription.
    #[cfg(unix)]
    let terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|_| StartupFailure::SignalUnknown)?;
    #[cfg(unix)]
    let interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
        .map_err(|_| StartupFailure::SignalUnknown)?;
    let telemetry = TelemetryRuntime::start(
        TelemetryConfig::from_env(),
        possums::server::admission_capacities(),
    );
    let state = state.with_telemetry(telemetry.metrics());
    #[cfg(unix)]
    let result = serve_until(listener, state, shutdown_signal(terminate, interrupt))
        .await
        .map_err(StartupFailure::from);
    #[cfg(not(unix))]
    let result = serve_until(listener, state, tokio::signal::ctrl_c())
        .await
        .map_err(StartupFailure::from);
    // Classify independent disposal failure, without replacing the serving
    // primary or recursively feeding telemetry's own metric.
    let cleanup = telemetry
        .checked_shutdown()
        .await
        .map_err(StartupFailure::Cleanup);
    result.and(cleanup)
}

#[cfg(unix)]
async fn shutdown_signal(
    mut terminate: tokio::signal::unix::Signal,
    mut interrupt: tokio::signal::unix::Signal,
) {
    tokio::select! {
        _ = terminate.recv() => {},
        _ = interrupt.recv() => {},
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_codes_and_stages_are_closed_and_unique() {
        let cases = [
            (
                StartupFailure::ConfigurationMissing,
                "configuration_missing",
                "configuration",
            ),
            (
                StartupFailure::ConfigurationEncodingInvalid,
                "configuration_encoding_invalid",
                "configuration",
            ),
            (
                StartupFailure::AccountsInvalid,
                "accounts_invalid",
                "accounts",
            ),
            (
                StartupFailure::Connect(InferenceFailure::VerificationFailed),
                "connect_unknown",
                "connect",
            ),
            (StartupFailure::BindUnknown, "bind_unknown", "bind"),
            (StartupFailure::SignalUnknown, "signal_unknown", "signal"),
            (StartupFailure::ServeUnknown, "serve_unknown", "serve"),
            (
                StartupFailure::Cleanup(ShutdownFailure::DisposalUnconfirmed),
                "telemetry_disposal_unconfirmed",
                "cleanup",
            ),
        ];
        let mut codes = std::collections::HashSet::new();
        let mut stages = std::collections::HashSet::new();
        let mut exits = std::collections::HashSet::new();
        for (failure, code, stage) in cases {
            assert_eq!(failure.code(), code);
            assert_eq!(failure.stage(), stage);
            assert!(codes.insert(failure.code()));
            stages.insert(failure.stage());
            assert!(exits.insert(failure.exit_code()));
            assert_ne!(failure.exit_code(), 0);
        }
        assert_eq!(codes.len(), cases.len());
        assert_eq!(exits.len(), cases.len());
        assert_eq!(stages.len(), 7);
    }

    #[test]
    fn serving_source_is_preserved_and_unknown_io_payload_is_discarded() {
        assert_eq!(
            StartupFailure::from(std::io::Error::other(ServerFailure::Accept)),
            StartupFailure::Serve(ServerFailure::Accept)
        );
        let unknown = StartupFailure::from(std::io::Error::other("HOSTILE_IO_CANARY"));
        assert_eq!(unknown, StartupFailure::ServeUnknown);
        assert!(!format!("{unknown:?}").contains("HOSTILE_IO_CANARY"));
    }

    #[test]
    fn environment_failure_classification_discards_values() {
        assert_eq!(
            StartupFailure::from(env::VarError::NotPresent),
            StartupFailure::ConfigurationMissing
        );
        let failure =
            StartupFailure::from(env::VarError::NotUnicode("HOSTILE_CONFIG_CANARY".into()));
        assert_eq!(failure, StartupFailure::ConfigurationEncodingInvalid);
        assert!(!format!("{failure:?}").contains("HOSTILE_CONFIG_CANARY"));
    }

    #[test]
    fn connect_stage_preserves_supported_inference_failure() {
        let projected = StartupFailure::from(InferenceError::Detailed(
            InferenceFailure::VerificationFailed,
        ));
        assert_eq!(
            projected,
            StartupFailure::Connect(InferenceFailure::VerificationFailed)
        );
        assert_eq!(projected.stage(), "connect");
        assert_eq!(
            StartupFailure::from(InferenceError::Unavailable),
            StartupFailure::Connect(InferenceFailure::InferenceUnavailable)
        );
        // Closed public diagnostics; never an SDK/io/env/config string.
        let rendered = format!("{projected:?} {}", projected.code());
        for sentinel in [
            "synthetic-host",
            "sk-secret",
            "POSSUMS_TINFOIL_HOST",
            "io error",
        ] {
            assert!(!rendered.contains(sentinel));
        }
    }
}
