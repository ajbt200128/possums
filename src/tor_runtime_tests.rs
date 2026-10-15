// Public synthetic fixtures only. No TorClient, bootstrap or publication.
use super::*;

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
fn source(path: &Path) -> KeyMgr {
    KeyMgrBuilder::default()
        .primary_store(Box::new(
            ArtiNativeKeystore::from_path_and_mistrust(
                path,
                &fs_mistrust::Mistrust::builder()
                    .ignore_environment()
                    .build()
                    .unwrap(),
            )
            .unwrap(),
        ))
        .build()
        .unwrap()
}
fn pem_fixture(
    private: &str,
    public: &str,
    a: [u8; 32],
) -> Result<TorEndorsementSigner, TorStartupFailure> {
    decode_grant(
        private.as_bytes(),
        public.as_bytes(),
        a,
        &HsIdKey::from(&synthetic_pair(7)).id(),
    )
}
#[test]
fn actual_go_pkcs8_v1_pem_spki_and_derived_key_equality() {
    let fixture = fixture();
    let private = fixture["private_key_pem"].as_str().unwrap();
    let public = fixture["public_key_pem"].as_str().unwrap();
    let a = crate::attestation::decode_raw32(fixture["application_key"].as_str().unwrap()).unwrap();
    assert_eq!(pem_fixture(private, public, a).unwrap().public_key(), a);
    assert_eq!(
        pem_fixture(private, public, [8; 32]).err(),
        Some(TorStartupFailure::GrantMismatch)
    );
    for malformed in [
        "",
        "HOSTILE_SECRET_EXCEPTION",
        "-----BEGIN PUBLIC KEY-----\nAQ==\n-----END PUBLIC KEY-----",
    ] {
        let failure = pem_fixture(private, malformed, a).err().unwrap();
        assert_eq!(failure, TorStartupFailure::GrantInvalid);
        assert!(!failure.to_string().contains("HOSTILE"));
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
        assert_eq!(pem_fixture(private, &pem, a).err(), Some(expected));
    }
    assert_eq!(
        pem_fixture(public, public, a).err(),
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
    let held = load_identity(scratch.path(), &approved).unwrap();
    let manager = memory();
    hold_identity(&manager, held, &approved).unwrap();
    fs::remove_dir_all(scratch.path()).unwrap();
    assert_eq!(require_identity(&manager, &nickname(), &approved), Ok(()));
    assert_eq!(
        load_identity(scratch.path(), &approved).err(),
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
        load_identity(scratch.path(), &approved).err(),
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
        load_identity(scratch.path(), &wrong).err(),
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
        load_identity(scratch.path(), &approved).err(),
        Some(TorStartupFailure::IdentityFileInvalid)
    );
    fs::write(&file, "HOSTILE_KEY_TEXT").unwrap();
    let failure = load_identity(scratch.path(), &approved).err().unwrap();
    assert_eq!(failure, TorStartupFailure::Identity(Failure::IdentityRead));
    assert!(!failure.to_string().contains("HOSTILE"));
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

#[tokio::test]
async fn preflight_appraisal_failure_precedes_all_mount_state_client_work() {
    for identity in [
        IdentitySource::Directory("/nonexistent/synthetic".into()),
        IdentitySource::Native(zeroize::Zeroizing::new("HOSTILE_KEY_TEXT".into())),
    ] {
        let config = TorRuntimeConfig {
            identity,
            approved: HsIdKey::from(&synthetic_pair(7)).id(),
        };
        let failure = PreparedTor::prepare(
            config,
            &crate::attestation::UnavailableEvidenceVerifier,
            "unused",
        )
        .await
        .err()
        .unwrap();
        assert_eq!(
            failure,
            TorStartupFailure::Appraisal(crate::attestation::TorEvidenceError::Unsupported)
        );
    }
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
