//! Key-free, normal-dependency CI binary; never part of the production image.
//! This supplements (not replaces) tests of the actual preparation factory.
use arti_client::{config::CfgPath, BootstrapBehavior, TorClient, TorClientConfig};
use std::sync::atomic::{AtomicU8, Ordering};

static STAGE: AtomicU8 = AtomicU8::new(0);

// Linux libc's POSIX account API. No host command, host NSS or extra Cargo features.
#[cfg(target_os = "linux")]
fn account_records() -> Result<bool, &'static str> {
    use std::ffi::{c_char, c_int, CStr};
    #[repr(C)]
    struct Passwd {
        name: *const c_char,
        password: *const c_char,
        uid: u32,
        gid: u32,
        gecos: *const c_char,
        home: *const c_char,
        shell: *const c_char,
    }
    #[repr(C)]
    struct Group {
        name: *const c_char,
        password: *const c_char,
        gid: u32,
        members: *const *const c_char,
    }
    unsafe extern "C" {
        fn getuid() -> u32;
        fn geteuid() -> u32;
        fn getgid() -> u32;
        fn getegid() -> u32;
        fn getpwuid(uid: u32) -> *const Passwd;
        fn getpwnam(name: *const c_char) -> *const Passwd;
        fn getgrgid(gid: u32) -> *const Group;
        fn getgrnam(name: *const c_char) -> *const Group;
        fn setrlimit(resource: c_int, limit: *const [u64; 2]) -> c_int;
    }
    // Single-threaded, before runtime construction. libc owns these records;
    // consume each before another lookup can invalidate its static storage.
    unsafe {
        // Linux RLIMIT_CORE: never retain a panic/core payload.
        if setrlimit(4, &[0, 0]) != 0 || [getuid(), geteuid(), getgid(), getegid()] != [65532; 4] {
            return Err("identity_or_core_limit_unknown");
        }
        for by_name in [false, true] {
            let entry = if by_name {
                getpwnam(c"possums".as_ptr())
            } else {
                getpwuid(65532)
            };
            if entry.is_null()
                || (*entry).uid != 65532
                || (*entry).gid != 65532
                || CStr::from_ptr((*entry).name).to_bytes() != b"possums"
            {
                return Ok(false);
            }
            let entry = if by_name {
                getgrnam(c"possums".as_ptr())
            } else {
                getgrgid(65532)
            };
            if entry.is_null()
                || (*entry).gid != 65532
                || CStr::from_ptr((*entry).name).to_bytes() != b"possums"
            {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

// Use the existing dependency's typed NSS error, not errno guesses or panic text.
// Failed lookups are not cached by fs-mistrust, so the SDK must still do its own.
fn missing_records_code() -> &'static str {
    match fs_mistrust::Mistrust::builder().build() {
        Err(fs_mistrust::Error::PasswdGroupIoError(error))
            if error.kind() == std::io::ErrorKind::PermissionDenied =>
        {
            "io_permission_denied"
        }
        Ok(_) => "records_unmatched",
        _ => "cause_unknown",
    }
}

fn exercise() -> Result<(), &'static str> {
    STAGE.store(1, Ordering::Relaxed);
    #[cfg(target_os = "linux")]
    let records = account_records()?;
    #[cfg(not(target_os = "linux"))]
    let records = false;
    println!(
        "stage=nss code={}",
        if records {
            "present"
        } else {
            missing_records_code()
        }
    );

    STAGE.store(2, Ordering::Relaxed);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "runtime_unknown")?;
    let result = runtime.block_on(async {
        // Must execute the real SDK's default constructor BEFORE .config().
        STAGE.store(3, Ordering::Relaxed);
        let builder = TorClient::builder();
        println!("stage=sdk_builder code=ok");
        if !records {
            return Err("nss_not_rejected");
        }
        STAGE.store(4, Ordering::Relaxed);
        use std::os::unix::fs::PermissionsExt;
        let scratch = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir_in("/tmp")
            .map_err(|_| "scratch_unknown")?;
        // Supported strict APIs, not a second preparation/identity implementation.
        let mut config = TorClientConfig::builder();
        config
            .storage()
            .cache_dir(CfgPath::new_literal(scratch.path().join("cache")));
        config
            .storage()
            .state_dir(CfgPath::new_literal(scratch.path().join("state")));
        config
            .storage()
            .permissions()
            .ignore_environment()
            .trust_no_group_id();
        config
            .storage()
            .keystore()
            .primary()
            .kind(tor_keymgr::config::ArtiKeystoreKind::Ephemeral.into());
        let config = config.build().map_err(|_| "configuration_unknown")?;
        config
            .fs_mistrust()
            .verifier()
            .secure_dir(scratch.path())
            .map_err(|_| "permissions_unknown")?;
        let builder = builder
            .config(config)
            .bootstrap_behavior(BootstrapBehavior::Manual);
        println!("stage=configure code=ok");
        STAGE.store(5, Ordering::Relaxed);
        let client = builder
            .create_unbootstrapped()
            .map_err(|_| "client_unknown")?;
        println!("stage=create_unbootstrapped code=ok");
        STAGE.store(6, Ordering::Relaxed);
        let manager = client.keymgr().map_err(|_| "keymgr_unknown")?;
        let spec = tor_hsservice::HsIdKeypairSpecifier::new(
            tor_hsservice::HsNickname::new("public-probe".into()).map_err(|_| "keymgr_unknown")?,
        );
        if manager
            .get::<tor_hscrypto::pk::HsIdKeypair>(&spec)
            .map_err(|_| "keymgr_unknown")?
            .is_some()
        {
            return Err("keystore_not_empty");
        }
        println!("stage=ephemeral_keymgr code=ok");
        drop(client);
        drop(scratch);
        Ok(())
    });
    STAGE.store(7, Ordering::Relaxed);
    runtime.shutdown_timeout(std::time::Duration::from_secs(1));
    result
}

fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    let code = match std::panic::catch_unwind(exercise) {
        Ok(Ok(())) => "ok",
        Ok(Err(code)) => code,
        Err(payload) => {
            // Never inspect or format hostile SDK panic payloads.
            std::mem::forget(payload);
            match STAGE.load(Ordering::Relaxed) {
                1 => "nss_panic",
                2 => "runtime_panic",
                3 => "sdk_builder_panic",
                4 => "configuration_panic",
                5 => "client_panic",
                6 => "keymgr_panic",
                7 => "cleanup_panic",
                _ => "cause_unknown",
            }
        }
    };
    println!("stage=terminal code={code}");
    std::process::exit(if code == "ok" { 0 } else { 2 });
}
