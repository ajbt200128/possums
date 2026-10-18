//! Default-off source integration. Construction/bootstrap are runtime-only;
//! protected mounts, exclusive B publication and measured deployment are host gates.
use std::env;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TorStartupFailure {
    #[error("code=tor_configuration_invalid stage=configuration constraint=explicit_opt_in_and_approved_identity action=check_approved_configuration_offline; no service launched")]
    Configuration,
    #[error("code=tor_feature_unavailable stage=configuration constraint=compiled_tor_feature action=use_an_approved_Tor_build; no service launched")]
    FeatureUnavailable,
    #[error("code=tor_grant_read_unknown stage=grant constraint=mounted_read cause=unknown action=check_approved_grant_mount_offline; no service launched")]
    GrantRead,
    #[error("code=tor_grant_invalid stage=grant constraint=bounded_ed25519_pkcs8_spki action=check_approved_grant_offline; no service launched")]
    GrantInvalid,
    #[error("code=tor_grant_mismatch stage=grant constraint=derived_mounted_A_equality action=check_current_boot_grant_offline; no service launched or endorsement")]
    GrantMismatch,
    #[error("code=tor_identity_file_invalid stage=identity constraint=bounded_regular_native_key_file action=restore_approved_native_key_files_offline; no replacement generated or service launched")]
    IdentityFileInvalid,
    #[error("code=tor_identity_secret_invalid stage=identity constraint=bounded_unencrypted_native_expanded_ed25519 action=restore_approved_native_key_secret_offline; no replacement generated or service launched")]
    IdentitySecretInvalid,
    #[cfg(feature = "tor")]
    #[error("{0}")]
    Identity(crate::tor_ingress::Failure),
    #[error("code=tor_state_unknown stage=state constraint=protected_ephemeral_directory cause=unknown action=check_memory_mount_offline; no service launched")]
    State,
    #[error("code=tor_client_unknown stage=client constraint=sdk_construction cause=unknown action=check_approved_runtime_offline; no service launched")]
    Client,
    #[error("code=tor_bootstrap_unknown stage=bootstrap constraint=sdk_bootstrap cause=unknown action=check_Tor_reachability; no service launched; not replayed")]
    Bootstrap,
}

// Intentionally no Debug/serialization: config may contain a secret and identifying paths/B.
pub struct TorRuntimeConfig {
    #[cfg(feature = "tor")]
    identity: IdentitySource,
    #[cfg(feature = "tor")]
    approved: tor_hscrypto::pk::HsId,
}

#[cfg(feature = "tor")]
enum IdentitySource {
    Directory(std::path::PathBuf),
    // Zeroizes this owned copy only; OS/runtime environment retention is separate.
    Native(zeroize::Zeroizing<String>),
}

#[cfg(feature = "tor")]
const MAX_IDENTITY_SECRET: usize = 16 * 1024;

impl TorRuntimeConfig {
    pub fn from_env() -> Result<Option<Self>, TorStartupFailure> {
        Self::from_lookup(|key| env::var(key))
    }

    fn from_lookup(
        mut read: impl FnMut(&str) -> Result<String, env::VarError>,
    ) -> Result<Option<Self>, TorStartupFailure> {
        match read("POSSUMS_TOR_ENABLED") {
            Err(env::VarError::NotPresent) => return Ok(None),
            Ok(value) if value == "false" => return Ok(None),
            Ok(value) if value == "true" => {}
            _ => return Err(TorStartupFailure::Configuration),
        }
        #[cfg(not(feature = "tor"))]
        return Err(TorStartupFailure::FeatureUnavailable);
        #[cfg(feature = "tor")]
        {
            let directory = read("POSSUMS_TOR_IDENTITY_DIRECTORY");
            let native = read("POSSUMS_TOR_IDENTITY_NATIVE_KEY").map(zeroize::Zeroizing::new);
            let identity = match (directory, native) {
                (Ok(directory), Err(env::VarError::NotPresent)) => {
                    let path = std::path::PathBuf::from(directory);
                    if !path.is_absolute() {
                        return Err(TorStartupFailure::Configuration);
                    }
                    IdentitySource::Directory(path)
                }
                (Err(env::VarError::NotPresent), Ok(native)) => {
                    if native.is_empty() || native.len() > MAX_IDENTITY_SECRET {
                        return Err(TorStartupFailure::IdentitySecretInvalid);
                    }
                    IdentitySource::Native(native)
                }
                _ => return Err(TorStartupFailure::Configuration),
            };
            let address =
                read("POSSUMS_TOR_EXPECTED_ONION").map_err(|_| TorStartupFailure::Configuration)?;
            if address.len() != 62 {
                return Err(TorStartupFailure::Configuration);
            }
            let approved = address
                .parse()
                .map_err(|_| TorStartupFailure::Configuration)?;
            Ok(Some(Self { identity, approved }))
        }
    }
}

#[cfg(feature = "tor")]
mod enabled {
    use super::*;
    use crate::{
        tor_endorsement::TorEndorsementSigner,
        tor_ingress::{require_identity, Failure},
    };
    use arti_client::{config::CfgPath, BootstrapBehavior, TorClient, TorClientConfig};
    #[cfg(feature = "debug-tor-readiness")]
    use futures_util::StreamExt;
    use rustls::pki_types::{pem::PemObject, PrivatePkcs8KeyDer, SubjectPublicKeyInfoDer};
    use spki::{der::Decode, ObjectIdentifier, SubjectPublicKeyInfoRef};
    use std::{fs, io::Read, path::Path, sync::Arc};
    use tor_hscrypto::pk::{HsId, HsIdKey, HsIdKeypair};
    use tor_hsservice::{
        HsIdKeypairSpecifier, HsIdPublicKeySpecifier, HsNickname, OnionServiceConfig,
    };
    use tor_keymgr::{
        ArtiEphemeralKeystore, ArtiNativeKeystore, KeyMgr, KeyMgrBuilder, KeySpecifier, KeyType,
        KeystoreSelector, SshKeyData, ToEncodableKey,
    };
    use tor_rtcompat::PreferredRuntime;

    const PRIVATE_GRANT: &str = "/run/tinfoil/keys/possums-tor-endorsement-v1/private_key.pem";
    const PUBLIC_GRANT: &str = "/run/tinfoil/keys/possums-tor-endorsement-v1/public_key.pem";
    const MAX_KEY_FILE: u64 = 16 * 1024;

    pub struct PreparedTor {
        signer: Arc<TorEndorsementSigner>,
        config: TorClientConfig,
        identity: KeyMgr,
        service: OnionServiceConfig,
        approved: HsId,
        // Kept through bootstrap, publication, shared serving and original drain.
        _scratch: tempfile::TempDir,
    }

    fn nickname() -> HsNickname {
        HsNickname::new("possums".into()).expect("fixed nickname")
    }

    fn read_grant(path: &Path) -> Result<Vec<u8>, TorStartupFailure> {
        let file = fs::File::open(path).map_err(|_| TorStartupFailure::GrantRead)?;
        let metadata = file.metadata().map_err(|_| TorStartupFailure::GrantRead)?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_KEY_FILE {
            return Err(TorStartupFailure::GrantInvalid);
        }
        let mut bytes = Vec::new();
        file.take(MAX_KEY_FILE + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| TorStartupFailure::GrantRead)?;
        if bytes.len() as u64 > MAX_KEY_FILE {
            return Err(TorStartupFailure::GrantInvalid);
        }
        Ok(bytes)
    }

    fn grant_pem<T: PemObject>(bytes: &[u8]) -> Result<T, TorStartupFailure> {
        let mut sections = T::pem_slice_iter(bytes);
        let key = sections
            .next()
            .ok_or(TorStartupFailure::GrantInvalid)?
            .map_err(|_| TorStartupFailure::GrantInvalid)?;
        if !sections.remainder().iter().all(u8::is_ascii_whitespace) {
            return Err(TorStartupFailure::GrantInvalid);
        }
        Ok(key)
    }

    fn decode_grant(
        private: &[u8],
        public: &[u8],
        b: &HsId,
    ) -> Result<TorEndorsementSigner, TorStartupFailure> {
        let private: PrivatePkcs8KeyDer<'_> = grant_pem(private)?;
        let public: SubjectPublicKeyInfoDer<'_> = grant_pem(public)?;
        let spki = SubjectPublicKeyInfoRef::from_der(public.as_ref())
            .map_err(|_| TorStartupFailure::GrantInvalid)?;
        if spki.algorithm.oid != ObjectIdentifier::new_unwrap("1.3.101.112")
            || spki.algorithm.parameters.is_some()
        {
            return Err(TorStartupFailure::GrantInvalid);
        }
        let public: [u8; 32] = spki
            .subject_public_key
            .as_bytes()
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or(TorStartupFailure::GrantInvalid)?;
        let signer = TorEndorsementSigner::from_pkcs8(private.secret_pkcs8_der(), *b.as_ref())
            .map_err(|_| TorStartupFailure::GrantInvalid)?;
        if signer.public_key() != public {
            return Err(TorStartupFailure::GrantMismatch);
        }
        Ok(signer)
    }

    // Metadata bounds precede the SDK file decoder. Exclusive, protected source
    // ownership prevents changes between this check and SDK read; not a host lock.
    fn bound_identity_file(
        path: &Path,
        spec: &dyn KeySpecifier,
        ty: KeyType,
        required: bool,
    ) -> Result<(), TorStartupFailure> {
        let relative = spec
            .arti_path()
            .map_err(|_| TorStartupFailure::Identity(Failure::IdentityRead))?;
        let file = path
            .join(relative.as_ref())
            .with_extension(ty.arti_extension());
        let metadata = match fs::symlink_metadata(file) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && !required => {
                return Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(TorStartupFailure::Identity(Failure::IdentityMissing))
            }
            Err(_) => return Err(TorStartupFailure::Identity(Failure::IdentityRead)),
        };
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_KEY_FILE {
            return Err(TorStartupFailure::IdentityFileInvalid);
        }
        Ok(())
    }

    fn load_identity(path: &Path, approved: &HsId) -> Result<HsIdKeypair, TorStartupFailure> {
        let mistrust = fs_mistrust::Mistrust::builder()
            .ignore_environment()
            .build()
            .map_err(|_| TorStartupFailure::State)?;
        load_identity_with_mistrust(path, approved, &mistrust)
    }

    fn load_identity_with_mistrust(
        path: &Path,
        approved: &HsId,
        mistrust: &fs_mistrust::Mistrust,
    ) -> Result<HsIdKeypair, TorStartupFailure> {
        match fs::metadata(path) {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => return Err(TorStartupFailure::IdentityFileInvalid),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(TorStartupFailure::Identity(Failure::IdentityMissing))
            }
            Err(_) => return Err(TorStartupFailure::Identity(Failure::IdentityRead)),
        }
        let name = nickname();
        let spec = HsIdKeypairSpecifier::new(name.clone());
        bound_identity_file(path, &spec, KeyType::Ed25519ExpandedKeypair, true)?;
        bound_identity_file(
            path,
            &HsIdPublicKeySpecifier::new(name.clone()),
            KeyType::Ed25519PublicKey,
            false,
        )?;
        let source = ArtiNativeKeystore::from_path_and_mistrust(path, mistrust)
            .map_err(|_| TorStartupFailure::Identity(Failure::IdentityRead))?;
        let manager = KeyMgrBuilder::default()
            .primary_store(Box::new(source))
            .build()
            .map_err(|_| TorStartupFailure::Identity(Failure::IdentityRead))?;
        require_identity(&manager, &name, approved).map_err(TorStartupFailure::Identity)?;
        manager
            .get::<HsIdKeypair>(&spec)
            .map_err(|_| TorStartupFailure::Identity(Failure::IdentityRead))?
            .ok_or(TorStartupFailure::Identity(Failure::IdentityMissing))
    }

    fn decode_native_identity(
        native: zeroize::Zeroizing<String>,
        approved: &HsId,
    ) -> Result<HsIdKeypair, TorStartupFailure> {
        if native.is_empty() || native.len() > MAX_IDENTITY_SECRET {
            return Err(TorStartupFailure::IdentitySecretInvalid);
        }
        let parsed = ssh_key::PrivateKey::from_openssh(native.as_bytes())
            .map_err(|_| TorStartupFailure::IdentitySecretInvalid)?;
        if parsed.is_encrypted() {
            return Err(TorStartupFailure::IdentitySecretInvalid);
        }
        let key = SshKeyData::try_from_keypair_data(parsed.key_data().clone())
            .map_err(|_| TorStartupFailure::IdentitySecretInvalid)?;
        if key
            .key_type()
            .map_err(|_| TorStartupFailure::IdentitySecretInvalid)?
            != KeyType::Ed25519ExpandedKeypair
        {
            return Err(TorStartupFailure::IdentitySecretInvalid);
        }
        let key = key
            .into_erased()
            .map_err(|_| TorStartupFailure::IdentitySecretInvalid)?
            .downcast::<<HsIdKeypair as ToEncodableKey>::Key>()
            .map_err(|_| TorStartupFailure::IdentitySecretInvalid)?;
        let pair = HsIdKeypair::from_encodable_key(*key);
        if &HsIdKey::from(&pair).id() != approved {
            return Err(TorStartupFailure::Identity(Failure::IdentityMismatch));
        }
        Ok(pair)
    }

    fn hold_identity(
        manager: &KeyMgr,
        pair: HsIdKeypair,
        approved: &HsId,
    ) -> Result<(), TorStartupFailure> {
        manager
            .insert(
                pair,
                &HsIdKeypairSpecifier::new(nickname()),
                KeystoreSelector::Primary,
                false,
            )
            .map_err(|_| TorStartupFailure::Identity(Failure::IdentityRead))?;
        require_identity(manager, &nickname(), approved).map_err(TorStartupFailure::Identity)
    }

    fn client_config(
        scratch: &Path,
    ) -> Result<TorClientConfig, tor_keymgr::config::ConfigBuildError> {
        let mut builder = TorClientConfig::builder();
        builder
            .storage()
            .cache_dir(CfgPath::new_literal(scratch.join("cache")));
        builder
            .storage()
            .state_dir(CfgPath::new_literal(scratch.join("state")));
        builder
            .storage()
            .keystore()
            .primary()
            .kind(tor_keymgr::config::ArtiKeystoreKind::Ephemeral.into());
        builder.storage().permissions().ignore_environment();
        // Minimal images have no passwd/group database. Trust no group rather
        // than resolving the SDK's default same-name group or weakening checks.
        #[cfg(unix)]
        builder.storage().permissions().trust_no_group_id();
        builder.build()
    }

    impl PreparedTor {
        /// Local grant consistency and approved B custody only, not appraisal.
        /// Fresh attestation remains owned by each client-requested endorsement.
        pub async fn prepare(config: TorRuntimeConfig) -> Result<Self, TorStartupFailure> {
            Self::prepare_with_grants(
                config,
                &read_grant(Path::new(PRIVATE_GRANT))?,
                &read_grant(Path::new(PUBLIC_GRANT))?,
            )
        }

        fn prepare_with_grants(
            config: TorRuntimeConfig,
            private: &[u8],
            public: &[u8],
        ) -> Result<Self, TorStartupFailure> {
            let signer = decode_grant(private, public, &config.approved)?;
            let pair = match config.identity {
                IdentitySource::Directory(path) => load_identity(&path, &config.approved)?,
                IdentitySource::Native(native) => decode_native_identity(native, &config.approved)?,
            };
            let service = OnionServiceConfig::builder()
                .nickname(nickname())
                .build()
                .map_err(|_| TorStartupFailure::Configuration)?;
            let mut scratch_builder = tempfile::Builder::new();
            scratch_builder.prefix("possums-tor-");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                scratch_builder.permissions(fs::Permissions::from_mode(0o700));
            }
            let scratch = scratch_builder
                .tempdir_in("/tmp")
                .map_err(|_| TorStartupFailure::State)?;
            let client_config =
                client_config(scratch.path()).map_err(|_| TorStartupFailure::Configuration)?;
            // Exercise the very same supported ephemeral insertion before any client.
            let manager = KeyMgrBuilder::default()
                .primary_store(Box::new(ArtiEphemeralKeystore::new("preflight".into())))
                .build()
                .map_err(|_| TorStartupFailure::State)?;
            hold_identity(&manager, pair, &config.approved)?;
            Ok(Self {
                signer: Arc::new(signer),
                config: client_config,
                identity: manager,
                service,
                approved: config.approved,
                _scratch: scratch,
            })
        }

        /// Invoked only by explicitly opted-in production run, never offline tests.
        /// The caller cancels this future with the already registered signal.
        pub async fn bootstrap(
            &self,
        ) -> Result<Arc<TorClient<PreferredRuntime>>, TorStartupFailure> {
            self.bootstrap_inner(
                #[cfg(feature = "debug-tor-readiness")]
                None,
            )
            .await
        }

        #[cfg(feature = "debug-tor-readiness")]
        pub async fn bootstrap_with_diagnostics(
            &self,
            diagnostics: &crate::tor_ingress::DebugReadiness,
        ) -> Result<Arc<TorClient<PreferredRuntime>>, TorStartupFailure> {
            self.bootstrap_inner(Some(diagnostics)).await
        }

        async fn bootstrap_inner(
            &self,
            #[cfg(feature = "debug-tor-readiness")] diagnostics: Option<
                &crate::tor_ingress::DebugReadiness,
            >,
        ) -> Result<Arc<TorClient<PreferredRuntime>>, TorStartupFailure> {
            let client = TorClient::builder()
                .config(self.config.clone())
                .bootstrap_behavior(BootstrapBehavior::Manual)
                .create_unbootstrapped()
                .map_err(|_| TorStartupFailure::Client)?;
            let pair = self
                .identity
                .get::<HsIdKeypair>(&HsIdKeypairSpecifier::new(nickname()))
                .map_err(|_| TorStartupFailure::Identity(Failure::IdentityRead))?
                .ok_or(TorStartupFailure::Identity(Failure::IdentityMissing))?;
            hold_identity(
                client.keymgr().map_err(|_| TorStartupFailure::Client)?,
                pair,
                &self.approved,
            )?;
            #[cfg(feature = "debug-tor-readiness")]
            let result = if let Some(diagnostics) = diagnostics {
                diagnostics.observe_bootstrap(&client.bootstrap_status());
                observe_bootstrap(
                    client.bootstrap(),
                    client.bootstrap_events(),
                    || client.bootstrap_status(),
                    diagnostics,
                )
                .await
            } else {
                client.bootstrap().await
            };
            #[cfg(not(feature = "debug-tor-readiness"))]
            let result = client.bootstrap().await;
            result.map_err(|_| TorStartupFailure::Bootstrap)?;
            Ok(client)
        }

        pub fn service(&self) -> OnionServiceConfig {
            self.service.clone()
        }
        pub fn approved(&self) -> HsId {
            self.approved
        }
        pub fn configure(&self, state: crate::server::AppState) -> crate::server::AppState {
            state.with_tor_endorsement(self.signer.clone())
        }
    }

    // Poll the supported SDK's existing status stream alongside the ONE
    // bootstrap operation. Stream loss never starts another bootstrap/retry.
    #[cfg(feature = "debug-tor-readiness")]
    async fn observe_bootstrap<T>(
        work: impl std::future::Future<Output = T>,
        updates: impl futures_util::Stream<Item = arti_client::status::BootstrapStatus>,
        final_status: impl FnOnce() -> arti_client::status::BootstrapStatus,
        diagnostics: &crate::tor_ingress::DebugReadiness,
    ) -> T {
        tokio::pin!(work, updates);
        let result = loop {
            tokio::select! {
                biased;
                result = &mut work => break result,
                status = updates.next() => match status {
                    Some(status) => diagnostics.observe_bootstrap(&status),
                    None => break work.await,
                },
            }
        };
        diagnostics.observe_bootstrap(&final_status());
        result
    }

    #[cfg(test)]
    mod tests {
        include!("tor_runtime_tests.rs");
    }
}
#[cfg(feature = "tor")]
pub use enabled::PreparedTor;

/// Biased cancellation prevents even polling work after a ready signal. It does
/// not claim SDK background shutdown or erase previously accepted work.
#[cfg(feature = "tor")]
pub async fn until_shutdown<T>(
    shutdown: std::pin::Pin<&mut impl std::future::Future<Output = ()>>,
    work: impl std::future::Future<Output = T>,
) -> Option<T> {
    tokio::select! { biased; _ = shutdown => None, result = work => Some(result) }
}

#[cfg(test)]
mod config_tests {
    use super::*;
    #[test]
    fn disabled_reads_only_switch_and_opt_in_is_exact() {
        for value in [None, Some("false")] {
            assert!(TorRuntimeConfig::from_lookup(|key| {
                assert_eq!(
                    key, "POSSUMS_TOR_ENABLED",
                    "disabled must not read identity configuration"
                );
                value.map(String::from).ok_or(env::VarError::NotPresent)
            })
            .unwrap()
            .is_none());
        }
        for value in ["TRUE", "1", "", " false", "true ", "HOSTILE_CONFIG"] {
            assert_eq!(
                TorRuntimeConfig::from_lookup(|_| Ok(value.into())).err(),
                Some(TorStartupFailure::Configuration)
            );
        }
        #[cfg(unix)]
        assert_eq!(
            TorRuntimeConfig::from_lookup(|key| {
                use std::os::unix::ffi::OsStringExt;
                assert_eq!(key, "POSSUMS_TOR_ENABLED");
                Err(env::VarError::NotUnicode(std::ffi::OsString::from_vec(
                    vec![0xff],
                )))
            })
            .err(),
            Some(TorStartupFailure::Configuration)
        );
        #[cfg(not(feature = "tor"))]
        assert_eq!(
            TorRuntimeConfig::from_lookup(|key| {
                assert_eq!(key, "POSSUMS_TOR_ENABLED");
                Ok("true".into())
            })
            .err(),
            Some(TorStartupFailure::FeatureUnavailable)
        );
    }
}
