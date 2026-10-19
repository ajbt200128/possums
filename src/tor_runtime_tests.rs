// Public synthetic fixtures only. No TorClient, bootstrap or publication.
use super::*;

mod offline_e2e {
    include!("tor_offline_e2e_tests.rs");
}

// Public deterministic seed [7; 32], encoded as a standard v3 address.
const SYNTHETIC_B: &str = "5jfgyy7ctrjavpxvkb5rglwf7gkuo5vox27hxescd3vgsfcg2iwfmxqd.onion";

fn fixture() -> serde_json::Value {
    serde_json::from_str(include_str!("../tests/fixtures/tor_grant_rfc8032.json")).unwrap()
}
fn synthetic_pair(seed: u8) -> HsIdKeypair {
    let pair = tor_llcrypto::pk::ed25519::Keypair::from(&[seed; 32]);
    HsIdKeypair::from(tor_llcrypto::pk::ed25519::ExpandedKeypair::from(&pair))
}
fn memory() -> KeyMgr {
    KeyMgrBuilder::default()
        .primary_store(Box::new(ArtiEphemeralKeystore::new("fixture".into())))
        .build()
        .unwrap()
}
fn scratch() -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt;
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in("/tmp")
        .unwrap()
}
fn fixture_mistrust(path: &Path) -> fs_mistrust::Mistrust {
    // Assume only the synthetic filesystem ancestors, never the key directory or files.
    fs_mistrust::Mistrust::builder()
        .ignore_environment()
        .ignore_prefix(path.parent().expect("fixture parent"))
        .build()
        .unwrap()
}
fn load_fixture_identity(path: &Path, approved: &HsId) -> Result<HsIdKeypair, TorStartupFailure> {
    load_identity_with_mistrust(path, approved, &fixture_mistrust(path))
}
fn source(path: &Path) -> KeyMgr {
    KeyMgrBuilder::default()
        .primary_store(Box::new(
            ArtiNativeKeystore::from_path_and_mistrust(path, &fixture_mistrust(path)).unwrap(),
        ))
        .build()
        .unwrap()
}
fn pem_fixture(private: &str, public: &str) -> Result<TorEndorsementSigner, TorStartupFailure> {
    decode_grant(
        private.as_bytes(),
        public.as_bytes(),
        &HsIdKey::from(&synthetic_pair(7)).id(),
    )
}
#[test]
fn offline_service_builder() {
    assert!(OnionServiceConfig::builder()
        .nickname(nickname())
        .build()
        .is_ok());
}

#[test]
fn offline_production_client_config_keeps_ephemeral_keys_and_strict_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = scratch();
    let config = client_config(scratch.path())
        .unwrap_or_else(|_| panic!("code=offline_client_configuration_invalid"));
    assert_eq!(
        config.keystore().primary_kind(),
        Some(tor_keymgr::config::ArtiKeystoreKind::Ephemeral)
    );
    let leaf = scratch.path().canonicalize().unwrap();
    let check_leaf = |private: bool| {
        // Nix's synthetic /tmp ancestors can themselves be untrusted. Inspect
        // all typed errors so an ancestor refusal cannot mask the leaf policy;
        // neither ignore ancestors nor weaken the actual production verifier.
        let mut leaf_rejected = false;
        if let Err(error) = config
            .fs_mistrust()
            .verifier()
            .require_directory()
            .all_errors()
            .check(&leaf)
        {
            for error in error.errors() {
                match error {
                    fs_mistrust::Error::BadPermission(path, ..) if path == &leaf => {
                        leaf_rejected = true;
                    }
                    fs_mistrust::Error::BadPermission(path, ..)
                    | fs_mistrust::Error::BadOwner(path, _)
                        if path != &leaf && leaf.starts_with(path) => {}
                    _ => panic!("code=offline_permission_check_unexpected"),
                }
            }
        }
        assert_eq!(leaf_rejected, !private);
    };
    check_leaf(true);
    // Even the process's own group must not authorize group access.
    for mode in [0o750, 0o770, 0o777] {
        fs::set_permissions(scratch.path(), fs::Permissions::from_mode(mode)).unwrap();
        check_leaf(false);
    }
    fs::set_permissions(scratch.path(), fs::Permissions::from_mode(0o700)).unwrap();
    check_leaf(true);
    assert!(fs::read_dir(scratch.path()).unwrap().next().is_none());
}

#[test]
fn actual_go_pkcs8_v1_pem_spki_and_derived_key_equality() {
    let fixture = fixture();
    let private = fixture["private_key_pem"].as_str().unwrap();
    let public = fixture["public_key_pem"].as_str().unwrap();
    let a = crate::attestation::decode_raw32(fixture["application_key"].as_str().unwrap()).unwrap();
    // The mounted pair is locally consistent, not independently appraised A.
    assert_eq!(pem_fixture(private, public).unwrap().public_key(), a);
    for malformed in [
        "",
        "HOSTILE_SECRET_EXCEPTION",
        "-----BEGIN PUBLIC KEY-----\nAQ==\n-----END PUBLIC KEY-----",
    ] {
        let failure = pem_fixture(private, malformed).err().unwrap();
        assert_eq!(failure, TorStartupFailure::GrantInvalid);
        assert!(!failure.to_string().contains("HOSTILE"));
    }
    for trailing in [public.to_owned(), "HOSTILE_TRAILING_PEM".into()] {
        assert_eq!(
            pem_fixture(&format!("{private}{trailing}"), public).err(),
            Some(TorStartupFailure::GrantInvalid)
        );
        assert_eq!(
            pem_fixture(private, &format!("{public}{trailing}")).err(),
            Some(TorStartupFailure::GrantInvalid)
        );
    }
    // Standard SPKI decoder rejects a non-Ed25519 OID and trailing DER.
    use base64::{engine::general_purpose::STANDARD, Engine};
    let original = SubjectPublicKeyInfoDer::from_pem_slice(public.as_bytes()).unwrap();
    let mut wrong_algorithm = original.as_ref().to_vec();
    wrong_algorithm[8] = 0x6e;
    let mut trailing = original.as_ref().to_vec();
    trailing.push(0);
    let mut different = original.as_ref().to_vec();
    *different.last_mut().unwrap() ^= 1;
    for (der, expected) in [
        (wrong_algorithm, TorStartupFailure::GrantInvalid),
        (trailing, TorStartupFailure::GrantInvalid),
        (different, TorStartupFailure::GrantMismatch),
    ] {
        let pem = format!(
            "-----BEGIN PUBLIC KEY-----\n{}\n-----END PUBLIC KEY-----\n",
            STANDARD.encode(der)
        );
        assert_eq!(pem_fixture(private, &pem).err(), Some(expected));
    }
    assert_eq!(
        pem_fixture(public, public).err(),
        Some(TorStartupFailure::GrantInvalid)
    );
}

#[test]
fn bounded_grants_disappearing_files_and_read_unknown_are_closed() {
    let scratch = tempfile::tempdir().unwrap();
    let path = scratch.path().join("grant");
    assert_eq!(read_grant(&path).err(), Some(TorStartupFailure::GrantRead));
    fs::write(&path, b"synthetic").unwrap();
    assert_eq!(read_grant(&path).unwrap(), b"synthetic");
    fs::remove_file(&path).unwrap();
    assert_eq!(read_grant(&path).err(), Some(TorStartupFailure::GrantRead));
    fs::write(&path, vec![0; MAX_KEY_FILE as usize + 1]).unwrap();
    assert_eq!(
        read_grant(&path).err(),
        Some(TorStartupFailure::GrantInvalid)
    );
    assert_eq!(
        read_grant(scratch.path()).err(),
        Some(TorStartupFailure::GrantInvalid)
    );
}

#[test]
fn native_sdk_existing_pair_survives_source_removal_without_generation() {
    let scratch = scratch();
    let pair = synthetic_pair(7);
    let approved = HsIdKey::from(&pair).id();
    source(scratch.path())
        .insert(
            pair,
            &HsIdKeypairSpecifier::new(nickname()),
            KeystoreSelector::Primary,
            false,
        )
        .unwrap();
    let held = load_fixture_identity(scratch.path(), &approved).unwrap();
    let manager = memory();
    hold_identity(&manager, held, &approved).unwrap();
    fs::remove_dir_all(scratch.path()).unwrap();
    assert_eq!(require_identity(&manager, &nickname(), &approved), Ok(()));
    assert_eq!(
        load_fixture_identity(scratch.path(), &approved).err(),
        Some(TorStartupFailure::Identity(Failure::IdentityMissing))
    );
    assert!(hold_identity(&manager, synthetic_pair(8), &approved).is_err()); // No overwrite fallback.
}

#[test]
fn native_sdk_missing_wrong_and_oversized_identity_fail_closed() {
    let scratch = scratch();
    let pair = synthetic_pair(7);
    let approved = HsIdKey::from(&pair).id();
    assert_eq!(
        load_fixture_identity(scratch.path(), &approved).err(),
        Some(TorStartupFailure::Identity(Failure::IdentityMissing))
    );
    let manager = source(scratch.path());
    manager
        .insert(
            pair,
            &HsIdKeypairSpecifier::new(nickname()),
            KeystoreSelector::Primary,
            false,
        )
        .unwrap();
    let wrong = HsIdKey::from(&synthetic_pair(8)).id();
    assert_eq!(
        load_fixture_identity(scratch.path(), &wrong).err(),
        Some(TorStartupFailure::Identity(Failure::IdentityMismatch))
    );
    let file = scratch
        .path()
        .join(
            HsIdKeypairSpecifier::new(nickname())
                .arti_path()
                .unwrap()
                .as_ref(),
        )
        .with_extension(KeyType::Ed25519ExpandedKeypair.arti_extension());
    fs::write(&file, vec![0; MAX_KEY_FILE as usize + 1]).unwrap();
    assert_eq!(
        load_fixture_identity(scratch.path(), &approved).err(),
        Some(TorStartupFailure::IdentityFileInvalid)
    );
    fs::write(&file, "HOSTILE_KEY_TEXT").unwrap();
    let failure = load_fixture_identity(scratch.path(), &approved)
        .err()
        .unwrap();
    assert_eq!(failure, TorStartupFailure::Identity(Failure::IdentityRead));
    assert!(!failure.to_string().contains("HOSTILE"));
}

#[test]
fn fixture_policy_retains_sdk_directory_and_content_permission_checks() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = scratch();
    let pair = synthetic_pair(7);
    let approved = HsIdKey::from(&pair).id();
    let spec = HsIdKeypairSpecifier::new(nickname());
    source(scratch.path())
        .insert(pair, &spec, KeystoreSelector::Primary, false)
        .unwrap();
    assert!(load_fixture_identity(scratch.path(), &approved).is_ok());
    fs::set_permissions(scratch.path(), fs::Permissions::from_mode(0o777)).unwrap();
    assert_eq!(
        load_fixture_identity(scratch.path(), &approved).err(),
        Some(TorStartupFailure::Identity(Failure::IdentityRead))
    );
    fs::set_permissions(scratch.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let file = scratch
        .path()
        .join(spec.arti_path().unwrap().as_ref())
        .with_extension(KeyType::Ed25519ExpandedKeypair.arti_extension());
    // The SDK permits readable content beneath a protected directory; do not
    // claim a per-file unreadability guarantee that its policy does not provide.
    for mode in [0o604, 0o644] {
        fs::set_permissions(&file, fs::Permissions::from_mode(mode)).unwrap();
        assert!(load_fixture_identity(scratch.path(), &approved).is_ok());
    }
    let failures = [0o602, 0o622].map(|mode| {
        fs::set_permissions(&file, fs::Permissions::from_mode(mode)).unwrap();
        load_fixture_identity(scratch.path(), &approved).err()
    });
    fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(load_fixture_identity(scratch.path(), &approved).is_ok());
    assert_eq!(
        failures,
        [Some(TorStartupFailure::Identity(Failure::IdentityRead)); 2]
    );
    for failure in failures.into_iter().flatten() {
        for rendered in [failure.to_string(), format!("{failure:?}")] {
            assert!(!rendered.contains("PRIVATE KEY"));
            assert!(!rendered.contains(file.to_str().unwrap()));
        }
    }
}

#[test]
fn production_identity_wrapper_preserves_real_platform_strict_ancestor_checks() {
    let scratch = scratch();
    let pair = synthetic_pair(7);
    let approved = HsIdKey::from(&pair).id();
    source(scratch.path())
        .insert(
            pair,
            &HsIdKeypairSpecifier::new(nickname()),
            KeystoreSelector::Primary,
            false,
        )
        .unwrap();
    let strict = fs_mistrust::Mistrust::builder()
        .ignore_environment()
        .build()
        .unwrap();
    // Real filesystem ancestors may be untrusted in a Nix sandbox. Compare
    // against the unmodified SDK policy, not the fixture's ignored ancestors.
    let expected = ArtiNativeKeystore::from_path_and_mistrust(scratch.path(), &strict)
        .map(|_| approved)
        .map_err(|_| TorStartupFailure::Identity(Failure::IdentityRead));
    assert_eq!(
        load_identity(scratch.path(), &approved).map(|pair| HsIdKey::from(&pair).id()),
        expected
    );
}

#[tokio::test]
async fn ready_shutdown_never_polls_construction_bootstrap_or_publication() {
    let shutdown = std::future::ready(());
    tokio::pin!(shutdown);
    assert!(
        crate::tor_runtime::until_shutdown(shutdown.as_mut(), async {
            panic!("construction must not be polled");
        })
        .await
        .is_none()
    );
}

struct NoStartupCalls;

#[async_trait::async_trait]
impl crate::attestation::EvidenceVerifier for NoStartupCalls {
    async fn verify(
        &self,
        _: &str,
        _: u64,
    ) -> Result<crate::attestation::GatewayEvidence, crate::attestation::EvidenceError> {
        panic!("startup must not appraise evidence");
    }
    async fn verify_tor(
        &self,
        _: &str,
        _: &[u8; 32],
    ) -> Result<crate::attestation::TorGatewayEvidence, crate::attestation::TorEvidenceError> {
        panic!("startup must not appraise Tor evidence");
    }
}

#[async_trait::async_trait]
impl crate::inference::Inference for NoStartupCalls {
    async fn catalog(&self) -> Result<Vec<u8>, crate::inference::InferenceError> {
        panic!("fixture must not acquire a catalog");
    }
    async fn count_tokens(
        &self,
        _: &str,
        _: &[crate::inference::Message],
        _: Arc<crate::telemetry::hooks::Lease>,
    ) -> Result<u64, crate::inference::InferenceError> {
        panic!("fixture must not tokenize");
    }
    fn verification_document(&self) -> Result<serde_json::Value, crate::inference::InferenceError> {
        panic!("fixture must not acquire inference evidence");
    }
}

#[tokio::test]
async fn local_preparation_and_bootstrap_inputs_do_not_need_an_appraisal() {
    use crate::attestation::{EvidenceVerifier, UnavailableEvidenceVerifier};
    let fixture = fixture();
    let approved = HsIdKey::from(&synthetic_pair(7)).id();
    for verifier in [
        Arc::new(UnavailableEvidenceVerifier) as Arc<dyn EvidenceVerifier>,
        Arc::new(NoStartupCalls),
    ] {
        let state = crate::server::AppState::new(
            crate::auth::Auth::from_json(
                r#"[{"id":"synthetic","credential_sha256":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA","demo_microunits":1}]"#,
            )
            .unwrap(),
            Arc::new(NoStartupCalls),
            Arc::<str>::from("unused"),
            verifier,
        );
        // Same production preparation after fixed-path bounded reads; only
        // public synthetic grant bytes and B replace the mounted inputs.
        let prepared = PreparedTor::prepare_with_grants(
            TorRuntimeConfig {
                identity: IdentitySource::Native(native_fixture(7)),
                approved,
            },
            fixture["private_key_pem"].as_str().unwrap().as_bytes(),
            fixture["public_key_pem"].as_str().unwrap().as_bytes(),
        )
        .unwrap();
        let _state = prepared.configure(state);
        assert_eq!(prepared.approved(), approved);
        assert_eq!(
            require_identity(&prepared.identity, &nickname(), &approved),
            Ok(())
        );
        assert!(prepared._scratch.path().is_dir());
        // No TorClient/network is constructed. Ready cancellation still owns
        // bootstrap and wins without polling any SDK construction/work.
        let shutdown = std::future::ready(());
        tokio::pin!(shutdown);
        assert!(
            crate::tor_runtime::until_shutdown(shutdown.as_mut(), prepared.bootstrap())
                .await
                .is_none()
        );
    }
    assert_eq!(
        PreparedTor::prepare_with_grants(
            TorRuntimeConfig {
                identity: IdentitySource::Native(native_fixture(8)),
                approved,
            },
            fixture["private_key_pem"].as_str().unwrap().as_bytes(),
            fixture["public_key_pem"].as_str().unwrap().as_bytes(),
        )
        .err(),
        Some(TorStartupFailure::Identity(Failure::IdentityMismatch))
    );
    let shutdown = std::future::ready(());
    tokio::pin!(shutdown);
    assert!(crate::tor_runtime::until_shutdown(
        shutdown.as_mut(),
        PreparedTor::prepare(TorRuntimeConfig {
            identity: IdentitySource::Native(native_fixture(7)),
            approved,
        }),
    )
    .await
    .is_none()); // Even the fixed mounted paths must not be read after shutdown.
}

// The official SSH SDK encodes deterministic public test material, entirely in memory.
fn native_fixture(seed: u8) -> zeroize::Zeroizing<String> {
    let pair = synthetic_pair(seed).to_encodable_key();
    opaque_fixture(
        "ed25519-expanded@spec.torproject.org",
        pair.to_secret_key_bytes().to_vec(),
        pair.public().as_bytes().to_vec(),
    )
}

fn opaque_fixture(
    algorithm: &str,
    private: Vec<u8>,
    public: Vec<u8>,
) -> zeroize::Zeroizing<String> {
    let public =
        ssh_key::public::OpaquePublicKey::new(public, ssh_key::Algorithm::new(algorithm).unwrap());
    let pair = ssh_key::private::OpaqueKeypair::new(private, public);
    ssh_key::PrivateKey::new(
        ssh_key::private::KeypairData::Other(pair),
        "public synthetic fixture",
    )
    .unwrap()
    .to_openssh(ssh_key::LineEnding::LF)
    .unwrap()
}

#[test]
fn native_secret_sdk_roundtrip_preserves_exact_pair_and_approved_b() {
    let approved = HsIdKey::from(&synthetic_pair(7)).id();
    let held = decode_native_identity(native_fixture(7), &approved).unwrap();
    assert_eq!(HsIdKey::from(&held).id(), approved);
    assert_eq!(
        held.to_encodable_key().to_secret_key_bytes(),
        synthetic_pair(7).to_encodable_key().to_secret_key_bytes()
    );
    let held = decode_native_identity(native_fixture(7), &approved).unwrap();
    let manager = memory();
    hold_identity(&manager, held, &approved).unwrap();
    assert_eq!(require_identity(&manager, &nickname(), &approved), Ok(()));
    assert_eq!(
        decode_native_identity(native_fixture(8), &approved).err(),
        Some(TorStartupFailure::Identity(Failure::IdentityMismatch))
    );
}

#[test]
fn native_secret_rejects_standard_ed25519_without_expanding_it() {
    let pair = ssh_key::private::Ed25519Keypair {
        private: ssh_key::private::Ed25519PrivateKey::from_bytes(&[7; 32]),
        public: ssh_key::public::Ed25519PublicKey(*HsIdKey::from(&synthetic_pair(7)).id().as_ref()),
    };
    let native = ssh_key::PrivateKey::new(pair.into(), "public synthetic fixture")
        .unwrap()
        .to_openssh(ssh_key::LineEnding::LF)
        .unwrap();
    // Even the matching expanded B does not authorize conversion of a standard key.
    let approved = HsIdKey::from(&synthetic_pair(7)).id();
    assert_eq!(
        decode_native_identity(native, &approved).err(),
        Some(TorStartupFailure::IdentitySecretInvalid)
    );
}

#[test]
fn native_secret_rejects_wrong_algorithm_lengths_and_invalid_payload() {
    let pair = synthetic_pair(7).to_encodable_key();
    let private = pair.to_secret_key_bytes().to_vec();
    let public = pair.public().as_bytes().to_vec();
    let approved = HsIdKey::from(&synthetic_pair(7)).id();
    for native in [
        opaque_fixture("x25519@spec.torproject.org", vec![7; 32], public.clone()),
        opaque_fixture("hostile@fixture.invalid", private.clone(), public.clone()),
        opaque_fixture(
            "ed25519-expanded@spec.torproject.org",
            vec![7; 63],
            public.clone(),
        ),
        opaque_fixture(
            "ed25519-expanded@spec.torproject.org",
            vec![7; 65],
            public.clone(),
        ),
        opaque_fixture(
            "ed25519-expanded@spec.torproject.org",
            private.clone(),
            vec![7; 31],
        ),
        opaque_fixture(
            "ed25519-expanded@spec.torproject.org",
            private.clone(),
            vec![7; 33],
        ),
        opaque_fixture(
            "ed25519-expanded@spec.torproject.org",
            vec![0xff; 64],
            public,
        ),
        opaque_fixture(
            "ed25519-expanded@spec.torproject.org",
            private,
            synthetic_pair(8)
                .to_encodable_key()
                .public()
                .as_bytes()
                .to_vec(),
        ),
    ] {
        assert_eq!(
            decode_native_identity(native, &approved).err(),
            Some(TorStartupFailure::IdentitySecretInvalid)
        );
    }
}

#[test]
fn native_secret_rejects_encrypted_public_sdk_fixture_without_decryption() {
    // Public fixture from ssh-key-fork-arti 0.6.7 tests/examples/id_ed25519.aes256-ctr.enc.
    let native: zeroize::Zeroizing<String> = zeroize::Zeroizing::new(
        concat!(
            "-----BEGIN OPENSSH PRIVATE KEY-----\n",
            "b3BlbnNzaC1rZXktdjEAAAAACmFlczI1Ni1jdHIAAAAGYmNyeXB0AAAAGAAAABBKH96ujW\n",
            "umB6/WnTNPjTeaAAAAEAAAAAEAAAAzAAAAC3NzaC1lZDI1NTE5AAAAILM+rvN+ot98qgEN\n",
            "796jTiQfZfG1KaT0PtFDJ/XFSqtiAAAAoFzvbvyFMhAiwBOXF0mhUUacPUCMZXivG2up2c\n",
            "hEnAw1b6BLRPyWbY5cC2n9ggD4ivJ1zSts6sBgjyiXQAReyrP35myYvT/OIB/NpwZM/xIJ\n",
            "N7MHSUzlkX4adBrga3f7GS4uv4ChOoxC4XsE5HsxtGsq1X8jzqLlZTmOcxkcEneYQexrUc\n",
            "bQP0o+gL5aKK8cQgiIlXeDbRjqhc4+h4EF6lY=\n",
            "-----END OPENSSH PRIVATE KEY-----\n",
        )
        .into(),
    );
    assert!(ssh_key::PrivateKey::from_openssh(native.as_bytes())
        .unwrap()
        .is_encrypted());
    assert_eq!(
        decode_native_identity(native, &HsIdKey::from(&synthetic_pair(7)).id()).err(),
        Some(TorStartupFailure::IdentitySecretInvalid)
    );
}

#[test]
fn native_secret_malformed_hostile_and_oversize_errors_are_content_free() {
    let approved = HsIdKey::from(&synthetic_pair(7)).id();
    for native in [
        String::new(),
        "HOSTILE_KEY_TEXT\ncredential=PUBLIC_SENTINEL".into(),
        "-----BEGIN OPENSSH PRIVATE KEY-----\nAQ==\n-----END OPENSSH PRIVATE KEY-----".into(),
        "x".repeat(MAX_IDENTITY_SECRET + 1),
        "é".repeat(MAX_IDENTITY_SECRET / 2 + 1),
    ] {
        let failure = decode_native_identity(zeroize::Zeroizing::new(native), &approved)
            .err()
            .unwrap();
        assert_eq!(failure, TorStartupFailure::IdentitySecretInvalid);
        for rendered in [failure.to_string(), format!("{failure:?}")] {
            for sentinel in ["HOSTILE", "PUBLIC_SENTINEL", "credential=", "AQ=="] {
                assert!(!rendered.contains(sentinel));
            }
        }
    }
    let mut padded = native_fixture(7);
    let padding = MAX_IDENTITY_SECRET + 1 - padded.len();
    padded.push_str(&" ".repeat(padding));
    assert_eq!(
        decode_native_identity(padded, &approved).err(),
        Some(TorStartupFailure::IdentitySecretInvalid)
    );
}

fn lookup_config(
    directory: Result<String, env::VarError>,
    native: Result<String, env::VarError>,
    address: Result<String, env::VarError>,
) -> Result<Option<TorRuntimeConfig>, TorStartupFailure> {
    TorRuntimeConfig::from_lookup(|key| match key {
        "POSSUMS_TOR_ENABLED" => Ok("true".into()),
        "POSSUMS_TOR_IDENTITY_DIRECTORY" => directory.clone(),
        "POSSUMS_TOR_IDENTITY_NATIVE_KEY" => native.clone(),
        "POSSUMS_TOR_EXPECTED_ONION" => address.clone(),
        _ => panic!("unexpected configuration read"),
    })
}

#[test]
fn enabled_configuration_requires_exactly_one_source_and_expected_b() {
    let approved = HsIdKey::from(&synthetic_pair(7)).id();
    assert_eq!(SYNTHETIC_B.parse::<HsId>().unwrap(), approved);
    let address = Ok(SYNTHETIC_B.to_string());
    let native = native_fixture(7);
    let config = lookup_config(
        Err(env::VarError::NotPresent),
        Ok(native.to_string()),
        address.clone(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(config.approved, approved);
    match config.identity {
        IdentitySource::Native(secret) => {
            let pair = decode_native_identity(secret, &approved).unwrap();
            assert_eq!(HsIdKey::from(&pair).id(), approved);
        }
        IdentitySource::Directory(_) => panic!("native source must be preserved"),
    }
    let config = lookup_config(
        Ok("/public/synthetic".into()),
        Err(env::VarError::NotPresent),
        address.clone(),
    )
    .unwrap()
    .unwrap();
    assert!(
        matches!(config.identity, IdentitySource::Directory(path) if path == Path::new("/public/synthetic"))
    );
    for (directory, native) in [
        (
            Err(env::VarError::NotPresent),
            Err(env::VarError::NotPresent),
        ),
        (Ok("/public/synthetic".into()), Ok(native.to_string())),
        (Ok("/public/synthetic".into()), Ok(String::new())),
        (Ok("relative".into()), Err(env::VarError::NotPresent)),
        (Ok(String::new()), Err(env::VarError::NotPresent)),
    ] {
        assert_eq!(
            lookup_config(directory, native, address.clone()).err(),
            Some(TorStartupFailure::Configuration)
        );
    }
    for bad_address in [
        Err(env::VarError::NotPresent),
        Ok(String::new()),
        Ok("HOSTILE_ADDRESS".into()),
        Ok("x".repeat(62)),
    ] {
        assert_eq!(
            lookup_config(
                Err(env::VarError::NotPresent),
                Ok(native.to_string()),
                bad_address
            )
            .err(),
            Some(TorStartupFailure::Configuration)
        );
    }
    for secret in [
        String::new(),
        "x".repeat(MAX_IDENTITY_SECRET + 1),
        "é".repeat(MAX_IDENTITY_SECRET / 2 + 1),
    ] {
        assert_eq!(
            lookup_config(Err(env::VarError::NotPresent), Ok(secret), address.clone()).err(),
            Some(TorStartupFailure::IdentitySecretInvalid)
        );
    }
}

#[cfg(unix)]
#[test]
fn nonunicode_configuration_never_selects_an_alternative_source() {
    use std::os::unix::ffi::OsStringExt;
    let nonunicode = || {
        Err(env::VarError::NotUnicode(std::ffi::OsString::from_vec(
            vec![0xff],
        )))
    };
    let address = Ok(SYNTHETIC_B.to_string());
    for (directory, native, address) in [
        (
            nonunicode(),
            Ok(native_fixture(7).to_string()),
            address.clone(),
        ),
        (
            Ok("/public/synthetic".into()),
            nonunicode(),
            address.clone(),
        ),
        (
            Err(env::VarError::NotPresent),
            nonunicode(),
            address.clone(),
        ),
        (
            nonunicode(),
            Err(env::VarError::NotPresent),
            address.clone(),
        ),
        (
            Err(env::VarError::NotPresent),
            Ok(native_fixture(7).to_string()),
            nonunicode(),
        ),
    ] {
        assert_eq!(
            lookup_config(directory, native, address).err(),
            Some(TorStartupFailure::Configuration)
        );
    }
    assert_eq!(
        TorRuntimeConfig::from_lookup(|key| {
            assert_eq!(key, "POSSUMS_TOR_ENABLED");
            nonunicode()
        })
        .err(),
        Some(TorStartupFailure::Configuration)
    );
}
