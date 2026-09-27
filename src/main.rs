use possums::{
    attestation::TinfoilEvidenceVerifier,
    auth::Auth,
    inference::TinfoilInference,
    web::{serve, AppState},
};
use std::{env, process::ExitCode, sync::Arc};

#[tokio::main]
async fn main() -> ExitCode {
    std::panic::set_hook(Box::new(|_| {}));
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
    serve(listener, state).await.map_err(|_| ())
}
