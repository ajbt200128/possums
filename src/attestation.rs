use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::Path,
    process::Stdio,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use thiserror::Error;
use tokio::{io::AsyncReadExt, process::Command, time::timeout};

const MAX_EVIDENCE_BYTES: usize = 1024 * 1024;
const MAX_JSON_NODES: usize = 4_096;
const MAX_JSON_DEPTH: usize = 64;
const MAX_EVIDENCE_AGE_SECONDS: u64 = 300;
const HELPER_TIMEOUT: Duration = Duration::from_secs(45);

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GatewayEvidence {
    pub quote: serde_json::Value,
    pub issued_at_unix: u64,
    pub release_digest: String,
    pub endpoint_key_sha256: String,
    pub freshness_expires_at_unix: u64,
}

#[async_trait]
pub trait EvidenceVerifier: Send + Sync {
    // Fixture verifiers can use the supplied time. The production helper must
    // sample its clock after awaiting newly issued evidence instead.
    async fn verify(
        &self,
        evidence_path: &str,
        now_unix: u64,
    ) -> Result<GatewayEvidence, EvidenceError>;

    #[cfg(feature = "tor")]
    async fn verify_tor(
        &self,
        _evidence_path: &str,
        _nonce: &[u8; 32],
    ) -> Result<TorGatewayEvidence, TorEvidenceError> {
        Err(TorEvidenceError::Unsupported)
    }
}

pub struct TinfoilEvidenceVerifier {
    helper_path: String,
    repository: String,
}

impl TinfoilEvidenceVerifier {
    pub fn new(helper_path: impl Into<String>, repository: impl Into<String>) -> Self {
        Self {
            helper_path: helper_path.into(),
            repository: repository.into(),
        }
    }
}

#[async_trait]
impl EvidenceVerifier for TinfoilEvidenceVerifier {
    #[cfg(feature = "tor")]
    async fn verify_tor(
        &self,
        evidence_path: &str,
        nonce: &[u8; 32],
    ) -> Result<TorGatewayEvidence, TorEvidenceError> {
        use tokio::io::AsyncWriteExt;
        let mut command = Command::new(&self.helper_path);
        command
            .arg("--socket")
            .arg(evidence_path)
            .arg("--repo")
            .arg(&self.repository)
            .arg("--tor-nonce-stdin")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        timeout(HELPER_TIMEOUT, async {
            let mut child = command.spawn().map_err(|_| TorEvidenceError::Unavailable)?;
            let mut stdin = child.stdin.take().ok_or(TorEvidenceError::Unavailable)?;
            stdin
                .write_all(nonce)
                .await
                .map_err(|_| TorEvidenceError::Unavailable)?;
            drop(stdin);
            let stdout = child.stdout.take().ok_or(TorEvidenceError::Unavailable)?;
            let mut bytes = Vec::new();
            stdout
                .take((MAX_EVIDENCE_BYTES + 1) as u64)
                .read_to_end(&mut bytes)
                .await
                .map_err(|_| TorEvidenceError::Unavailable)?;
            if bytes.len() > MAX_EVIDENCE_BYTES {
                return Err(TorEvidenceError::Invalid);
            }
            let status = child
                .wait()
                .await
                .map_err(|_| TorEvidenceError::Unavailable)?;
            if !status.success() {
                return Err(TorEvidenceError::Unavailable);
            }
            parse_tor_response(&bytes, tor_now()?)
        })
        .await
        .map_err(|_| TorEvidenceError::Unavailable)?
    }

    async fn verify(
        &self,
        evidence_path: &str,
        _now_unix: u64,
    ) -> Result<GatewayEvidence, EvidenceError> {
        let mut command = Command::new(&self.helper_path);
        command
            .arg("--socket")
            .arg(evidence_path)
            .arg("--repo")
            .arg(&self.repository)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        timeout(HELPER_TIMEOUT, async {
            let mut child = command.spawn().map_err(|_| EvidenceError::Unavailable)?;
            let stdout = child.stdout.take().ok_or(EvidenceError::Unavailable)?;
            let mut bytes = Vec::new();
            stdout
                .take((MAX_EVIDENCE_BYTES + 1) as u64)
                .read_to_end(&mut bytes)
                .await
                .map_err(|_| EvidenceError::Unavailable)?;
            if bytes.len() > MAX_EVIDENCE_BYTES {
                return Err(EvidenceError::Invalid);
            }
            let status = child.wait().await.map_err(|_| EvidenceError::Unavailable)?;
            if !status.success() {
                return Err(EvidenceError::Invalid);
            }
            // The helper issues fresh evidence after this request begins; sampling
            // before awaiting it can reject a valid quote across a second boundary.
            let validated_at = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| EvidenceError::Unavailable)?
                .as_secs();
            parse_and_validate(&bytes, validated_at)
        })
        .await
        .map_err(|_| EvidenceError::Unavailable)?
    }
}

pub struct UnavailableEvidenceVerifier;

#[async_trait]
impl EvidenceVerifier for UnavailableEvidenceVerifier {
    async fn verify(
        &self,
        _evidence_path: &str,
        _now_unix: u64,
    ) -> Result<GatewayEvidence, EvidenceError> {
        Err(EvidenceError::Unavailable)
    }
}

#[derive(Debug, Error)]
pub enum EvidenceError {
    #[error("gateway attestation evidence unavailable")]
    Unavailable,
    #[error("gateway attestation evidence is invalid")]
    Invalid,
}

pub fn load_evidence(path: impl AsRef<Path>) -> Result<GatewayEvidence, EvidenceError> {
    let metadata = fs::metadata(path.as_ref()).map_err(|_| EvidenceError::Unavailable)?;
    let length = usize::try_from(metadata.len()).map_err(|_| EvidenceError::Invalid)?;
    if length == 0 || length > MAX_EVIDENCE_BYTES {
        return Err(EvidenceError::Invalid);
    }
    let file = fs::File::open(path).map_err(|_| EvidenceError::Unavailable)?;
    let mut bytes = Vec::new();
    file.take((MAX_EVIDENCE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| EvidenceError::Unavailable)?;
    if bytes.len() > MAX_EVIDENCE_BYTES {
        return Err(EvidenceError::Invalid);
    }
    check_json_structure(&bytes)?;
    serde_json::from_slice(&bytes).map_err(|_| EvidenceError::Invalid)
}

pub fn validate_evidence(
    evidence: GatewayEvidence,
    now_unix: u64,
) -> Result<GatewayEvidence, EvidenceError> {
    let fresh = evidence.issued_at_unix <= now_unix
        && now_unix - evidence.issued_at_unix <= MAX_EVIDENCE_AGE_SECONDS
        && evidence.freshness_expires_at_unix > now_unix;
    if evidence.quote.is_null()
        || !fresh
        || !is_sha256(&evidence.release_digest)
        || !is_sha256(&evidence.endpoint_key_sha256)
    {
        return Err(EvidenceError::Invalid);
    }
    Ok(evidence)
}

fn parse_and_validate(bytes: &[u8], now_unix: u64) -> Result<GatewayEvidence, EvidenceError> {
    if bytes.is_empty() || bytes.len() > MAX_EVIDENCE_BYTES {
        return Err(EvidenceError::Invalid);
    }
    check_json_structure(bytes)?;
    let evidence = serde_json::from_slice(bytes).map_err(|_| EvidenceError::Invalid)?;
    validate_evidence(evidence, now_unix)
}

// Count lexical JSON values (including object keys) before constructing a Value tree.
// serde_json still handles syntax validation; this scan only enforces resource bounds.
fn check_json_structure(bytes: &[u8]) -> Result<(), EvidenceError> {
    let mut nodes = 0usize;
    let mut depth = 0usize;
    let mut index = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'{' | b'[' => {
                nodes += 1;
                depth += 1;
                if depth > MAX_JSON_DEPTH {
                    return Err(EvidenceError::Invalid);
                }
                index += 1;
            }
            b'}' | b']' => {
                depth = depth.saturating_sub(1);
                index += 1;
            }
            b'"' => {
                nodes += 1;
                let start = index;
                index += 1;
                while index < bytes.len() {
                    match bytes[index] {
                        b'\\' => index = (index + 2).min(bytes.len()),
                        b'"' => {
                            index += 1;
                            break;
                        }
                        _ => index += 1,
                    }
                }
                // raw_value can reparse a quoted JSON string into an unguarded
                // Value tree. Check decoded keys, including escaped spellings.
                if bytes[index..]
                    .iter()
                    .find(|byte| !byte.is_ascii_whitespace())
                    == Some(&b':')
                {
                    let key: String = serde_json::from_slice(&bytes[start..index])
                        .map_err(|_| EvidenceError::Invalid)?;
                    if key == "$serde_json::private::RawValue" {
                        return Err(EvidenceError::Invalid);
                    }
                }
            }
            b',' | b':' | b' ' | b'\n' | b'\r' | b'\t' => index += 1,
            _ => {
                nodes += 1;
                index += 1;
                while index < bytes.len()
                    && !matches!(
                        bytes[index],
                        b'{' | b'['
                            | b'}'
                            | b']'
                            | b'"'
                            | b','
                            | b':'
                            | b' '
                            | b'\n'
                            | b'\r'
                            | b'\t'
                    )
                {
                    index += 1;
                }
            }
        }
        if nodes > MAX_JSON_NODES {
            return Err(EvidenceError::Invalid);
        }
    }
    Ok(())
}

#[cfg(feature = "tor")]
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TorGatewayEvidence {
    pub quote: serde_json::Value,
    pub issued_at_unix: u64,
    pub release_digest: String,
    pub endpoint_key_sha256: String,
    pub freshness_expires_at_unix: u64,
    pub tor_endorsement_key: String,
}

#[cfg(feature = "tor")]
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum TorEvidenceError {
    #[error("code=tor_appraisal_unsupported stage=appraisal constraint=verifier_support action=use_the_approved_Tor_helper; no endorsement or inference")]
    Unsupported,
    #[error("code=tor_helper_unknown stage=helper constraint=bounded_helper_completion cause=unknown action=check_helper_offline; no endorsement or inference")]
    Unavailable,
    #[error("code=tor_helper_invalid stage=helper constraint=bounded_closed_response action=check_helper_offline; no endorsement or inference")]
    Invalid,
    #[error("code=tor_appraisal_expired stage=freshness constraint=current_evidence action=obtain_fresh_evidence; no endorsement or inference")]
    Expired,
    #[error("{0}")]
    Verification(TorVerificationFailure),
    #[error("code=possums_v3_acquisition_http_status stage=acquisition constraint=http_status status={0} action=check_attestation_acquisition_offline; observed acquisition status is not necessarily the gateway response status; no endorsement or inference")]
    AcquisitionStatus(u16),
}

// Closed SDK/helper projection: wire strings are discarded after exact matching.
#[cfg(feature = "tor")]
macro_rules! tor_failures {
    ($($variant:ident => ($stage:literal, $constraint:literal)),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum TorVerificationFailure { $($variant),+ }
        impl TorVerificationFailure {
            fn from_wire(code: &str, stage: &str, constraint: &str) -> Option<Self> {
                match (code, stage, constraint) {
                    $((concat!("possums_v3_", $stage, "_", $constraint), $stage, $constraint) => Some(Self::$variant),)+
                    _ => None,
                }
            }
            pub(crate) fn parts(self) -> (&'static str, &'static str, &'static str) {
                match self { $(Self::$variant => (concat!("possums_v3_", $stage, "_", $constraint), $stage, $constraint)),+ }
            }
        }
        impl std::fmt::Display for TorVerificationFailure {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                let (code, stage, constraint) = self.parts();
                let cause = if constraint.ends_with("unknown") { " cause=unknown" } else { "" };
                write!(f, "code={code} stage={stage} constraint={constraint}{cause} action=check_approved_evidence_and_helper_offline; no endorsement or inference")
            }
        }
    };
}
#[cfg(feature = "tor")]
tor_failures! {
    InputSize => ("input", "size"), InputNonce => ("input", "nonce"),
    InputWorkload => ("input", "workload"), InputUnknown => ("input", "cause_unknown"),
    AcquisitionUnknown => ("acquisition", "cause_unknown"),
    AcquisitionTransport => ("acquisition", "transport_unknown"),
    AcquisitionStatus => ("acquisition", "http_status"),
    AcquisitionRead => ("acquisition", "body_read_unknown"),
    AcquisitionSize => ("acquisition", "size"),
    EnvelopeRejected => ("envelope", "rejected"), EnvelopeUnknown => ("envelope", "cause_unknown"),
    VerificationRejected => ("verification", "rejected"), VerificationUnknown => ("verification", "cause_unknown"),
    VerificationResult => ("verification", "result"),
    PublisherSchema => ("publisher", "schema"), PublisherCertificate => ("publisher", "certificate"),
    PublisherIdentity => ("publisher", "identity"), PublisherArtifact => ("publisher", "artifact"),
    PublisherManifest => ("publisher", "manifest"), PublisherUnknown => ("publisher", "cause_unknown"),
    KeysTls => ("keys", "tls"), KeysHpke => ("keys", "hpke"), KeysUnknown => ("keys", "cause_unknown"),
    TorEntry => ("keys", "tor_endorsement_entry"), TorFormat => ("keys", "tor_endorsement_format"),
    TorSpki => ("keys", "tor_endorsement_spki"), TorAlgorithm => ("keys", "tor_endorsement_algorithm"),
    FreshnessExpired => ("freshness", "expired"),
}

#[cfg(feature = "tor")]
pub(crate) fn tor_now() -> Result<u64, TorEvidenceError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_secs())
        .map_err(|_| TorEvidenceError::Unavailable)
}

#[cfg(feature = "tor")]
impl TorGatewayEvidence {
    pub(crate) fn current_key(&self, now: u64) -> Result<[u8; 32], TorEvidenceError> {
        if self.freshness_expires_at_unix <= now {
            return Err(TorEvidenceError::Expired);
        }
        if self.quote.is_null()
            || self.issued_at_unix > now
            || now - self.issued_at_unix > MAX_EVIDENCE_AGE_SECONDS
            || !is_sha256(&self.release_digest)
            || !is_sha256(&self.endpoint_key_sha256)
        {
            return Err(TorEvidenceError::Invalid);
        }
        decode_raw32(&self.tor_endorsement_key).ok_or(TorEvidenceError::Invalid)
    }
}

#[cfg(feature = "tor")]
pub(crate) fn decode_raw32(text: &str) -> Option<[u8; 32]> {
    if !is_sha256(text) {
        return None;
    }
    let mut result = [0; 32];
    for (out, pair) in result.iter_mut().zip(text.as_bytes().chunks_exact(2)) {
        *out = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(result)
}

#[cfg(feature = "tor")]
fn parse_tor_response(bytes: &[u8], now: u64) -> Result<TorGatewayEvidence, TorEvidenceError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct WireFailure {
        code: String,
        stage: String,
        constraint: String,
        #[serde(default, deserialize_with = "tor_status")]
        status: Option<u16>,
    }
    // Absent is supported; present null/string/float/out-of-range is not status.
    fn tor_status<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<u16>, D::Error> {
        let status = u16::deserialize(deserializer)?;
        if !(100..=999).contains(&status) {
            return Err(serde::de::Error::custom("invalid acquisition status"));
        }
        Ok(Some(status))
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Wire {
        evidence: Option<TorGatewayEvidence>,
        failure: Option<WireFailure>,
    }
    if bytes.is_empty() || bytes.len() > MAX_EVIDENCE_BYTES {
        return Err(TorEvidenceError::Invalid);
    }
    check_json_structure(bytes).map_err(|_| TorEvidenceError::Invalid)?;
    let wire: Wire = serde_json::from_slice(bytes).map_err(|_| TorEvidenceError::Invalid)?;
    match (wire.evidence, wire.failure) {
        (Some(evidence), None) => {
            evidence.current_key(now)?;
            Ok(evidence)
        }
        (None, Some(failure)) => {
            let category = TorVerificationFailure::from_wire(
                &failure.code,
                &failure.stage,
                &failure.constraint,
            );
            Err(match (category, failure.status) {
                (Some(TorVerificationFailure::AcquisitionStatus), Some(status)) => {
                    TorEvidenceError::AcquisitionStatus(status)
                }
                (Some(category), None) => TorEvidenceError::Verification(category),
                _ => TorEvidenceError::Invalid,
            })
        }
        _ => Err(TorEvidenceError::Invalid),
    }
}

#[cfg(all(test, feature = "tor"))]
mod tor_tests {
    include!("attestation/tor_tests.rs");
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
