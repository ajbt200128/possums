// Operator-only scratch publisher; no clients, TLS shim, restart or identity export.
use super::*;
use axum::http::Request;
use http_body_util::BodyExt;
use std::{
    io::Write,
    os::unix::fs::PermissionsExt,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
use tokio::{
    io::AsyncReadExt,
    net::{TcpListener, UnixStream},
};

const OPT_IN: &str = "ONE_ISOLATED_LOCAL_ONION_RUN";
const IPC_LIMIT: usize = 40 * 1024;
const DIAGNOSTIC_LIMIT: usize = 32 * 1024;
type Closed<T> = Result<T, &'static str>;

#[derive(Default)]
struct DenyInference(AtomicUsize);
impl DenyInference {
    fn deny<T>(&self) -> Result<T, crate::inference::InferenceError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(crate::inference::InferenceError::Unavailable)
    }
}
#[async_trait::async_trait]
impl crate::inference::Inference for DenyInference {
    async fn catalog(&self) -> Result<Vec<u8>, crate::inference::InferenceError> {
        self.deny()
    }
    async fn count_tokens(
        &self,
        _: &str,
        _: &[crate::inference::Message],
        _: Arc<crate::telemetry::hooks::Lease>,
    ) -> Result<u64, crate::inference::InferenceError> {
        self.deny()
    }
    async fn generate_stream(
        &self,
        _: &crate::catalog::Model,
        _: &[crate::inference::Message],
        _: Arc<crate::telemetry::hooks::Lease>,
        _: &mut (dyn for<'d> FnMut(&'d str) + Send),
    ) -> Result<crate::inference::stream::StreamUsage, crate::inference::InferenceError> {
        self.deny()
    }
    async fn generate_completion_stream(
        &self,
        _: &crate::catalog::Model,
        _: &[crate::inference::Message],
        _: Arc<crate::telemetry::hooks::Lease>,
        _: &mut dyn crate::inference::TextDeltaSink,
    ) -> Result<crate::inference::stream::StreamCompletion, crate::inference::InferenceError> {
        self.deny()
    }
    async fn generate_invocation_stream(
        &self,
        _: &crate::catalog::Model,
        _: &crate::inference::tools::ToolInvocation,
        _: Arc<crate::telemetry::hooks::Lease>,
        _: &mut dyn crate::inference::DeltaSink,
    ) -> Result<crate::inference::stream::StreamCompletion, crate::inference::InferenceError> {
        self.deny()
    }
    fn verification_document(&self) -> Result<serde_json::Value, crate::inference::InferenceError> {
        self.deny()
    }
}
fn private_parent(path: &Path) -> Closed<()> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "SCRATCH_INVALID")?;
    if !metadata.is_dir() || metadata.permissions().mode() & 0o777 != 0o700 {
        return Err("SCRATCH_INVALID");
    }
    fs_mistrust::Mistrust::builder()
        .ignore_environment()
        .build()
        .map_err(|_| "SCRATCH_INVALID")?
        .verifier()
        .require_directory()
        .check(path)
        .map_err(|_| "SCRATCH_INVALID")
}
fn pem(label: &str, bytes: &[u8]) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine};
    format!(
        "-----BEGIN {label}-----\n{}\n-----END {label}-----\n",
        STANDARD.encode(bytes)
    )
}
fn fresh_prepared() -> Closed<PreparedTor> {
    use ring::{
        rand::{SecureRandom, SystemRandom},
        signature::{Ed25519KeyPair, KeyPair},
    };
    let rng = SystemRandom::new();
    let mut seed = zeroize::Zeroizing::new([0; 32]);
    rng.fill(seed.as_mut()).map_err(|_| "KEY_GENERATION")?;
    let a = Ed25519KeyPair::generate_pkcs8(&rng).map_err(|_| "KEY_GENERATION")?;
    let a_pair = Ed25519KeyPair::from_pkcs8(a.as_ref()).map_err(|_| "KEY_GENERATION")?;
    let mut spki = vec![
        0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
    ];
    spki.extend_from_slice(a_pair.public_key().as_ref());
    let pair = tor_llcrypto::pk::ed25519::Keypair::from(&*seed);
    let pair = HsIdKeypair::from(tor_llcrypto::pk::ed25519::ExpandedKeypair::from(&pair));
    let approved = HsIdKey::from(&pair).id();
    let key = pair.to_encodable_key();
    let native = opaque_fixture(
        "ed25519-expanded@spec.torproject.org",
        key.to_secret_key_bytes().to_vec(),
        key.public().as_bytes().to_vec(),
    );
    let private = zeroize::Zeroizing::new(pem("PRIVATE KEY", a.as_ref()));
    // Reuse production preparation: its private scratch is in the owned /tmp tmpfs.
    PreparedTor::prepare_with_grants(
        TorRuntimeConfig {
            identity: IdentitySource::Native(native),
            approved,
        },
        private.as_bytes(),
        pem("PUBLIC KEY", &spki).as_bytes(),
    )
    .map_err(|_| "PREPARE_REFUSED")
}
fn send(ipc: &mut std::os::unix::net::UnixStream, message: serde_json::Value) -> Closed<()> {
    let mut bytes = serde_json::to_vec(&message).map_err(|_| "IPC_FORMAT")?;
    if bytes.len() >= IPC_LIMIT {
        return Err("IPC_BOUND");
    }
    bytes.push(b'\n');
    ipc.write_all(&bytes).map_err(|_| "IPC_WRITE")
}
async fn bounded<T>(
    seconds: u64,
    future: impl std::future::Future<Output = Closed<T>>,
) -> Closed<T> {
    tokio::time::timeout(Duration::from_secs(seconds), future)
        .await
        .map_err(|_| "STAGE_DEADLINE")?
}
// Consume before awaiting: malformed/failed delivery cannot permit a second D.
async fn diagnostic_once(app: &mut Option<axum::Router>) -> Closed<serde_json::Value> {
    use tower::ServiceExt;
    let app = app.take().ok_or("IPC_COMMAND")?;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/healthz/diagnostics?detail=descriptor")
                .body(axum::body::Body::empty())
                .map_err(|_| "DIAGNOSTIC_REQUEST")?,
        )
        .await
        .map_err(|_| "DIAGNOSTIC_REQUEST")?;
    if !matches!(response.status().as_u16(), 200 | 503)
        || response
            .headers()
            .get("cache-control")
            .and_then(|v| v.to_str().ok())
            != Some("no-store")
        || response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            != Some("application/json")
    {
        return Err("DIAGNOSTIC_RESPONSE");
    }
    let status = response.status().as_u16();
    let bytes = http_body_util::Limited::new(response.into_body(), DIAGNOSTIC_LIMIT)
        .collect()
        .await
        .map_err(|_| "IPC_BOUND")?
        .to_bytes();
    let body: LocalDiagnostic =
        serde_json::from_slice(&bytes).map_err(|_| "DIAGNOSTIC_RESPONSE")?;
    body.validate(status)?;
    Ok(serde_json::json!({"kind":"diagnostic", "status":status, "body":body}))
}
// Production response types are Serialize-only, with private/static fields.
// This strict decoder keeps that actual router response closed before IPC;
// it does not take a second status snapshot or expose SDK formatter output.
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct LocalDiagnostic {
    code: String,
    stage: String,
    constraint: String,
    action: String,
    lifecycle_serving: bool,
    ready: bool,
    tor_state: String,
    tor_problem: String,
    descriptor_evidence: LocalEvidence,
}
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct LocalEvidence {
    selection: String,
    outcomes: Vec<String>,
    coverage: Vec<String>,
    circuit: LocalPhase,
    stream: LocalPhase,
    request: LocalPhase,
    unphased: LocalPhase,
}
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct LocalPhase {
    variants: Vec<String>,
    kinds: Vec<String>,
    io_kinds: Vec<String>,
    http_statuses: Vec<u16>,
}
const LOCAL_VARIANTS: &str = "guard_not_usable pending_canceled pending_failed lost_usability_race circuit_canceled usage_mismatched circuit_timeout request_timeout no_relay guard_manager guard_selection vanguard_manager_init request_failed channel protocol spawn persistent_state bug directory_timeout truncated_headers response_size_rejected header_size_rejected utf8_encoding io stream_protocol tunnel http_parsing http_construction content_encoding clock_skew empty_request empty_response http_status invalid_http_status unknown";
const LOCAL_KINDS: &str = "tor_access_failed bootstrap_required directory_expired persistent_state_access_failed local_resource_already_in_use filesystem_permissions persistent_state_corrupted cache_corrupted cache_access_failed keystore_corrupted keystore_access_failed reactor_shutting_down arti_shutting_down software_deprecated remote_network_timeout invalid_config invalid_config_transition no_home_directory not_implemented feature_disabled local_protocol_violation tor_protocol_violation local_network_error local_resource_exhausted external_tool_failed relay_identity_mismatch circuit_collapse tor_network_timeout tor_directory_error remote_stream_closed remote_stream_reset remote_stream_error remote_connection_refused exit_policy_rejected exit_timeout remote_network_failed remote_host_not_found remote_host_resolution_failed remote_protocol_violation relay_too_busy invalid_stream_target forbidden_stream_target transient_failure bad_api_usage circuit_refused no_path no_exit tor_directory_unusable clock_skew tor_document_rejected internal other unknown";
const LOCAL_IO_KINDS: &str = "not_found permission_denied connection_refused connection_reset host_unreachable network_unreachable connection_aborted not_connected address_in_use address_not_available network_down broken_pipe already_exists would_block not_a_directory is_a_directory directory_not_empty read_only_filesystem stale_network_file_handle invalid_input invalid_data timed_out write_zero storage_full not_seekable quota_exceeded file_too_large resource_busy executable_file_busy deadlock crosses_devices too_many_links invalid_filename argument_list_too_long interrupted unsupported unexpected_eof out_of_memory other unknown";
fn closed_set(values: &[String], catalog: &str) -> Closed<()> {
    let mut previous = None;
    for value in values {
        let index = catalog
            .split_ascii_whitespace()
            .position(|label| label == value)
            .ok_or("DIAGNOSTIC_RESPONSE")?;
        if previous.is_some_and(|p| p >= index) {
            return Err("DIAGNOSTIC_RESPONSE");
        }
        previous = Some(index);
    }
    Ok(())
}
impl LocalPhase {
    fn validate(&self) -> Closed<()> {
        closed_set(&self.variants, LOCAL_VARIANTS)?;
        closed_set(&self.kinds, LOCAL_KINDS)?;
        closed_set(&self.io_kinds, LOCAL_IO_KINDS)?;
        if self.http_statuses.iter().any(|s| !(100..=599).contains(s))
            || self.http_statuses.windows(2).any(|w| w[0] >= w[1])
        {
            return Err("DIAGNOSTIC_RESPONSE");
        }
        Ok(())
    }
}
impl LocalDiagnostic {
    fn validate(&self, status: u16) -> Closed<()> {
        let messages = [
            ("tor_status_not_observed", "tor_status_stream", "no_observed_sdk_status", "Check gateway startup and Tor initialization; no diagnostic probe or retry was initiated."),
            ("tor_status_stream_ended", "tor_status_stream", "stream_ended_cause_unknown", "Check gateway lifecycle and Tor service; the underlying cause is unknown. Do not replay inference."),
            ("tor_status_stream_interrupted", "tor_status_stream", "stream_poll_unwound_cause_unknown", "Check gateway lifecycle and Tor service; the underlying cause is unknown. Do not replay inference."),
            ("tor_state_unknown", "tor_status_stream", "sdk_state_unknown", "Check the gateway SDK version and Tor service; the underlying cause is unknown. Do not replay inference."),
            ("tor_reachability_unconfirmed", "tor_service", "local_sdk_readiness_not_confirmed", "Inspect the closed SDK state/problem and platform lifecycle; independently check Tor routing. Do not replay inference."),
            ("gateway_not_serving", "gateway_lifecycle", "serving_not_confirmed", "Check gateway startup or shutdown status; do not replay inference."),
            ("gateway_ready", "readiness", "local_readiness_only", "Independently verify endpoint authenticity and reachability before private requests."),
        ];
        if !messages.iter().any(|&(code, stage, constraint, action)| {
            code == self.code && stage == self.stage
                && constraint == self.constraint && action == self.action
        }) || status != if self.ready { 200 } else { 503 }
            || (self.ready && !self.lifecycle_serving)
            || !"not_observed shutdown bootstrapping degraded_reachable degraded_unreachable running recovering broken status_stream_ended status_stream_interrupted unknown"
                .split_ascii_whitespace().any(|s| s == self.tor_state)
            || !"none_observed runtime descriptor_upload introduction_point descriptor_fatal descriptor_retry_exhausted descriptor_timeout descriptor_internal unknown descriptor_timeout_circuit descriptor_timeout_stream descriptor_timeout_request descriptor_timeout_internal descriptor_timeout_mixed descriptor_timeout_no_recorded_source descriptor_timeout_unknown"
                .split_ascii_whitespace().any(|s| s == self.tor_problem)
            || !"no_snapshot no_problem_selected other_problem_selected descriptor_upload_selected"
                .split_ascii_whitespace().any(|s| s == self.descriptor_evidence.selection)
        {
            return Err("DIAGNOSTIC_RESPONSE");
        }
        let evidence = &self.descriptor_evidence;
        closed_set(
            &evidence.outcomes,
            "fatal_error max_retry_count_exceeded timeout bug unknown",
        )?;
        closed_set(&evidence.coverage, "retained_sources_only empty_batch no_retained_sources no_nested_sources depth_limit node_limit unsupported_variant unsupported_kind unsupported_io_kind")?;
        for phase in [
            &evidence.circuit,
            &evidence.stream,
            &evidence.request,
            &evidence.unphased,
        ] {
            phase.validate()?;
        }
        Ok(())
    }
}
async fn exercise(ipc: &mut std::os::unix::net::UnixStream, parent: &Path) -> Closed<()> {
    private_parent(parent)?;
    let prepared = bounded(10, async { fresh_prepared() }).await?;
    send(ipc, serde_json::json!({"kind":"prepared"}))?;
    let client = bounded(180, async {
        prepared.bootstrap().await.map_err(|_| "ARTI_BOOTSTRAP")
    })
    .await?;
    send(ipc, serde_json::json!({"kind":"bootstrapped"}))?;
    let inference = Arc::new(DenyInference::default());
    let bridge_attempts = Arc::new(AtomicUsize::new(0));
    let mut state = prepared.configure(crate::server::AppState::new(
        crate::auth::Auth::without_accounts_for_local_tor(),
        inference.clone(),
        "unused-local-evidence",
        Arc::new(crate::attestation::UnavailableEvidenceVerifier),
    ));
    // Unsolicited onion traffic must never dial the production public shim.
    state.tor_bridge_denied = Some(bridge_attempts.clone());
    let mut diagnostic_app = Some(crate::server::router(state.clone()));
    let gateway = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|_| "GATEWAY_BIND")?;
    let ipc_read = ipc.try_clone().map_err(|_| "IPC_READ")?;
    ipc_read.set_nonblocking(true).map_err(|_| "IPC_READ")?;
    let mut ipc_read = UnixStream::from_std(ipc_read).map_err(|_| "IPC_READ")?;
    let (ready, mut launched) = tokio::sync::oneshot::channel();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = crate::tor_ingress::serve_tcp_and_onion_until_initialized(
        gateway,
        state,
        client,
        prepared.service(),
        prepared.approved(),
        async {
            let _ = stopped.await;
        },
        (Arc::new(|_| {}), || async {
            let _ = ready.send(());
        }),
    );
    tokio::pin!(server);
    let mut server_ended = false;
    let result = async {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(10)) => return Err("LAUNCH_DEADLINE"),
            _ = &mut server => { server_ended = true; return Err("ARTI_LAUNCH"); },
            result = &mut launched => result.map_err(|_| "ARTI_LAUNCH")?,
        }
        send(ipc, serde_json::json!({"kind":"ready"}))?;
        let mut command = [0; 1];
        loop {
            tokio::select! {
                _ = &mut server => { server_ended = true; return Err("GATEWAY_STOPPED"); },
                read = ipc_read.read_exact(&mut command) => {
                    read.map_err(|_| "IPC_READ")?;
                    match command {
                        [b'D'] => send(ipc, bounded(2, diagnostic_once(&mut diagnostic_app)).await?)?,
                        [b'Q'] => break,
                        _ => return Err("IPC_COMMAND"),
                    }
                },
            }
        }
        Ok(())
    };
    let primary = bounded(880, result).await;
    let _ = stop.send(());
    let drained = !server_ended
        && matches!(
            tokio::time::timeout(Duration::from_secs(10), &mut server).await,
            Ok(Ok(()))
        );
    let no_upstream = inference.0.load(Ordering::SeqCst) == 0;
    let bridge_untouched = bridge_attempts.load(Ordering::SeqCst) == 0;
    let cleanup = drained && no_upstream && bridge_untouched;
    let delivered = send(
        ipc,
        serde_json::json!({"kind":"finished", "cleanup":cleanup,
        "zeroUpstream":no_upstream, "bridgeUntouched":bridge_untouched}),
    );
    primary?;
    delivered?;
    if !cleanup {
        return Err("DRAIN_UNCONFIRMED");
    }
    Ok(())
}

#[test]
#[ignore = "operator-only: fresh publisher, one diagnostic, no clients; public Tor protocol traffic"]
fn operator_fresh_descriptor_generation() {
    if env::var("POSSUMS_LOCAL_TOR_E2E").ok().as_deref() != Some(OPT_IN) {
        return;
    }
    std::panic::set_hook(Box::new(|_| {}));
    let Ok(parent) = env::current_dir() else {
        return;
    };
    if private_parent(&parent).is_err() {
        return;
    }
    let Ok(mut ipc) = std::os::unix::net::UnixStream::connect(parent.join("gateway.sock")) else {
        return;
    };
    let _ = ipc.set_write_timeout(Some(Duration::from_secs(1)));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|_| "RUNTIME")?;
        let result = runtime.block_on(exercise(&mut ipc, &parent));
        runtime.shutdown_timeout(Duration::from_secs(1));
        result
    }));
    let code = match result {
        Ok(Ok(())) => "GATEWAY_CLOSED",
        Ok(Err(code)) => code,
        Err(payload) => {
            std::mem::forget(payload);
            "GATEWAY_PANIC"
        }
    };
    let _ = send(
        &mut ipc,
        serde_json::json!({"kind":"terminal", "code":code}),
    );
}
