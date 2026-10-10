use possums::{
    attestation::TinfoilEvidenceVerifier,
    free::{serve, AppState, SharedKeys},
    inference::TinfoilInference,
};
use std::{env, process::ExitCode, sync::Arc};

#[tokio::main]
async fn main() -> ExitCode {
    // Never print provider/parser/panic payloads, credentials or request content.
    std::panic::set_hook(Box::new(|_| {}));
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(()) => ExitCode::FAILURE,
    }
}

async fn run() -> Result<(), ()> {
    if !possums::free::build_identity_available() {
        return Err(());
    }
    let keys = SharedKeys::from_json(&env::var("POSSUMS_FREE_API_KEYS_JSON").map_err(|_| ())?)
        .map_err(|_| ())?;
    let host = env::var("POSSUMS_TINFOIL_HOST").map_err(|_| ())?;
    let repository = env::var("POSSUMS_TINFOIL_REPOSITORY").map_err(|_| ())?;
    let api_key = env::var("TINFOIL_API_KEY").map_err(|_| ())?;
    let evidence_path = env::var("POSSUMS_GATEWAY_ATTESTATION_SOCKET").map_err(|_| ())?;
    let gateway_repository = env::var("POSSUMS_GATEWAY_REPOSITORY").map_err(|_| ())?;
    let bind = env::var("POSSUMS_BIND").unwrap_or_else(|_| "127.0.0.1:8080".into());
    let inference = TinfoilInference::connect(&host, &repository, api_key)
        .await
        .map_err(|_| ())?;
    let state = AppState::new(
        keys,
        Arc::new(inference),
        Arc::<str>::from(evidence_path),
        Arc::new(TinfoilEvidenceVerifier::new(
            "/bin/possums-attestation",
            gateway_repository,
        )),
    );
    let listener = tokio::net::TcpListener::bind(bind).await.map_err(|_| ())?;
    // No accounts, paid ledger, telemetry runtime, exporter or balance SDK.
    tokio::select! {
        result = serve(listener, state) => result.map_err(|_| ()),
        _ = shutdown_signal() => Ok(()),
    }
}

async fn shutdown_signal() {
    #[cfg(unix)]
    if let Ok(mut signal) =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
    {
        tokio::select! { _ = signal.recv() => {}, _ = tokio::signal::ctrl_c() => {} }
        return;
    }
    let _ = tokio::signal::ctrl_c().await;
}
