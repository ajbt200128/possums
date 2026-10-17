// Operator-only offline integration. Public A fixture; B is never signed/exported.
use super::*;
use std::{io::Write, os::unix::fs::PermissionsExt};

impl PreparedTor {
    pub(crate) fn offline_public_fixture() -> Self {
        let public_a = fixture();
        Self::prepare_with_grants(
            TorRuntimeConfig {
                identity: IdentitySource::Native(native_fixture(7)),
                approved: SYNTHETIC_B.parse().unwrap(),
            },
            public_a["private_key_pem"].as_str().unwrap().as_bytes(),
            public_a["public_key_pem"].as_str().unwrap().as_bytes(),
        )
        .expect("public full preparation fixture")
    }
}

fn closed_failure(error: TorStartupFailure) -> &'static str {
    match error {
        TorStartupFailure::Configuration => "PREPARE_CONFIGURATION",
        TorStartupFailure::State => "PREPARE_STATE",
        TorStartupFailure::IdentitySecretInvalid => "IDENTITY_INVALID",
        TorStartupFailure::Identity(_) => "IDENTITY_REJECTED",
        TorStartupFailure::GrantInvalid | TorStartupFailure::GrantMismatch => "GRANT_REJECTED",
        _ => "CAUSE_UNKNOWN",
    }
}

fn bounded_text(path: &Path, limit: u64) -> Result<zeroize::Zeroizing<String>, &'static str> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "INPUT_FILE")?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > limit {
        return Err("INPUT_FILE");
    }
    fs_mistrust::Mistrust::builder()
        .ignore_environment()
        .build()
        .map_err(|_| "INPUT_PERMISSIONS")?
        .verifier()
        .require_file()
        .check(path)
        .map_err(|_| "INPUT_PERMISSIONS")?;
    let mut bytes = zeroize::Zeroizing::new(Vec::new());
    fs::File::open(path)
        .map_err(|_| "INPUT_FILE")?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "INPUT_FILE")?;
    if bytes.len() as u64 > limit {
        return Err("INPUT_FILE");
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| "INPUT_FILE")?;
    Ok(zeroize::Zeroizing::new(text.to_owned()))
}

fn exercise() -> Result<&'static str, &'static str> {
    // Disabled configuration must consult no secret/path, even in the shipping feature build.
    if TorRuntimeConfig::from_lookup(|key| {
        if key != "POSSUMS_TOR_ENABLED" {
            panic!("PUBLIC_DISABLED_LOOKUP_SENTINEL");
        }
        Ok("false".into())
    })
    .map_err(|_| "DISABLED")?
    .is_some()
    {
        return Err("DISABLED");
    }
    let mode = env::var("POSSUMS_OFFLINE_MODE").map_err(|_| "ARGUMENTS")?;
    if mode == "panic" {
        panic!("PUBLIC_HOSTILE_PANIC credential=synthetic-only");
    }
    if mode == "nss_probe" {
        return if fs_mistrust::Mistrust::builder()
            .ignore_environment()
            .build()
            .is_err()
        {
            Ok("NSS_INJECTED")
        } else {
            Err("NSS_NOT_INJECTED")
        };
    }
    if mode == "probe" {
        if std::net::TcpListener::bind("127.0.0.1:0")
            .err()
            .map(|e| e.kind())
            != Some(std::io::ErrorKind::PermissionDenied)
            || std::net::UdpSocket::bind("127.0.0.1:0")
                .err()
                .map(|e| e.kind())
                != Some(std::io::ErrorKind::PermissionDenied)
            || std::net::TcpStream::connect_timeout(
                &"127.0.0.1:9".parse().map_err(|_| "SANDBOX")?,
                std::time::Duration::from_millis(100),
            )
            .err()
            .map(|e| e.kind())
                != Some(std::io::ErrorKind::PermissionDenied)
        {
            return Err("SANDBOX");
        }
        // A real denied write outside the sole ephemeral runtime prefix.
        let denied = env::var_os("POSSUMS_OFFLINE_DENIED_PATH").ok_or("ARGUMENTS")?;
        if fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(denied)
            .err()
            .map(|e| e.kind())
            != Some(std::io::ErrorKind::PermissionDenied)
        {
            return Err("SANDBOX");
        }
        let state = tempfile::Builder::new()
            .prefix("possums-tor-")
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in("/tmp")
            .map_err(|_| "SANDBOX")?;
        fs::write(state.path().join("public-probe"), b"public").map_err(|_| "SANDBOX")?;
        return Ok("SANDBOX_QUALIFIED");
    }
    let (native, approved) = match mode.as_str() {
        "synthetic" => (
            native_fixture(7),
            SYNTHETIC_B.parse().map_err(|_| "ARGUMENTS")?,
        ),
        "private" => {
            if env::var("POSSUMS_OFFLINE_ACK").as_deref() != Ok("PRODUCTION_B_LOCAL_ONLY") {
                return Err("NOT_AUTHORIZED");
            }
            let native_path = env::var_os("POSSUMS_OFFLINE_NATIVE_PATH").ok_or("ARGUMENTS")?;
            let hostname_path = env::var_os("POSSUMS_OFFLINE_HOSTNAME_PATH").ok_or("ARGUMENTS")?;
            let hostname = bounded_text(Path::new(&hostname_path), 64)?;
            let approved = hostname
                .trim_end_matches(['\r', '\n'])
                .parse()
                .map_err(|_| "INPUT_HOSTNAME")?;
            (
                bounded_text(Path::new(&native_path), MAX_IDENTITY_SECRET as u64)?,
                approved,
            )
        }
        _ => return Err("ARGUMENTS"),
    };
    let public_a = fixture();
    let prepared = PreparedTor::prepare_with_grants(
        TorRuntimeConfig {
            identity: IdentitySource::Native(native),
            approved,
        },
        public_a["private_key_pem"]
            .as_str()
            .ok_or("GRANT_REJECTED")?
            .as_bytes(),
        public_a["public_key_pem"]
            .as_str()
            .ok_or("GRANT_REJECTED")?
            .as_bytes(),
    )
    .map_err(closed_failure)?;
    if fs::metadata(prepared._scratch.path())
        .map_err(|_| "PREPARE_STATE")?
        .permissions()
        .mode()
        & 0o777
        != 0o700
    {
        return Err("PREPARE_STATE");
    }
    let scratch_path = prepared._scratch.path().to_owned();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "RUNTIME")?;
    let result = runtime.block_on(async {
        // Exact supported construction and B insertion used before bootstrap in production.
        // Never call bootstrap, launch_onion_service, endorse or any network method.
        let client = TorClient::builder()
            .config(prepared.config.clone())
            .bootstrap_behavior(BootstrapBehavior::Manual)
            .create_unbootstrapped()
            .map_err(|_| "SDK_CLIENT")?;
        let pair = prepared
            .identity
            .get::<HsIdKeypair>(&HsIdKeypairSpecifier::new(nickname()))
            .map_err(|_| "SDK_KEYSTORE")?
            .ok_or("SDK_KEYSTORE")?;
        let manager = client.keymgr().map_err(|_| "SDK_KEYSTORE")?;
        hold_identity(manager, pair, &approved).map_err(|_| "SDK_KEYSTORE")?;
        require_identity(manager, &nickname(), &approved).map_err(|_| "SDK_IDENTITY")?;
        drop(client);
        Ok("PREPARE_AND_SDK_OK")
    });
    runtime.shutdown_timeout(std::time::Duration::from_secs(1));
    drop(prepared);
    if scratch_path.exists() {
        return Err("CLEANUP");
    }
    result
}

#[test]
#[ignore = "operator-only: qualified deny-network/write sandbox wrapper required"]
fn operator_full_path() {
    std::panic::set_hook(Box::new(|_| {}));
    let code = match std::panic::catch_unwind(exercise) {
        Ok(Ok(code) | Err(code)) => code,
        Err(payload) => {
            std::mem::forget(payload);
            "CAUSE_UNKNOWN"
        }
    };
    let _ = writeln!(std::io::stdout().lock(), "POSSUMS_OFFLINE_E2E={code}");
}
