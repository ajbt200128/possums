use possums::{
    attestation::TinfoilEvidenceVerifier,
    auth::Auth,
    inference::TinfoilInference,
    server::{serve_until, AppState},
    telemetry::runtime::{Config as TelemetryConfig, Runtime as TelemetryRuntime},
};
use std::{env, process::ExitCode, sync::Arc};

#[tokio::main]
async fn main() -> ExitCode {
    std::panic::set_hook(Box::new(|_| {}));
    if env::args_os().skip(1).collect::<Vec<_>>() == ["--healthcheck"] {
        if possums::health::healthcheck().await {
            return ExitCode::SUCCESS;
        }
        eprintln!("healthcheck_failed: local readiness unavailable");
        return ExitCode::FAILURE;
    }
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(()) => ExitCode::FAILURE,
    }
}

async fn run() -> Result<(), ()> {
    let accounts = env::var("POSSUMS_ACCOUNTS_JSON").map_err(|_| ())?;
    let auth = Auth::from_json(&accounts).map_err(|_| ())?;
    let host = env::var("POSSUMS_TINFOIL_HOST").map_err(|_| ())?;
    let repository = env::var("POSSUMS_TINFOIL_REPOSITORY").map_err(|_| ())?;
    let api_key = env::var("TINFOIL_API_KEY").map_err(|_| ())?;
    let attestation_socket = env::var("POSSUMS_GATEWAY_ATTESTATION_SOCKET").map_err(|_| ())?;
    let gateway_repository = env::var("POSSUMS_GATEWAY_REPOSITORY").map_err(|_| ())?;
    let bind = env::var("POSSUMS_BIND").unwrap_or_else(|_| "127.0.0.1:8080".into());

    let inference = TinfoilInference::connect(&host, &repository, api_key)
        .await
        .map_err(|_| ())?;
    let state = AppState::new(
        auth,
        Arc::new(inference),
        Arc::<str>::from(attestation_socket),
        Arc::new(TinfoilEvidenceVerifier::new(
            "/bin/possums-attestation",
            gateway_repository,
        )),
    );
    let listener = tokio::net::TcpListener::bind(bind).await.map_err(|_| ())?;
    // Register both handlers before the first accept. Never serve without a
    // working termination signal subscription.
    #[cfg(unix)]
    let terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|_| ())?;
    #[cfg(unix)]
    let interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
        .map_err(|_| ())?;
    let telemetry = TelemetryRuntime::start(
        TelemetryConfig::from_env(),
        possums::server::admission_capacities(),
    );
    let state = state.with_telemetry(telemetry.metrics());
    #[cfg(unix)]
    let result = serve_until(listener, state, shutdown_signal(terminate, interrupt))
        .await
        .map_err(|_| ());
    #[cfg(not(unix))]
    let result = serve_until(listener, state, tokio::signal::ctrl_c())
        .await
        .map_err(|_| ());
    // Telemetry disposal failure must not replace the serving outcome.
    let _ = telemetry.shutdown().await;
    result
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
