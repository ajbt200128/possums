use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{fs, path::Path, process::Stdio, time::Duration};
use thiserror::Error;
use tokio::{process::Command, time::timeout};

const MAX_EVIDENCE_BYTES: usize = 17 * 1024 * 1024;
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
    async fn verify(
        &self,
        evidence_path: &str,
        now_unix: u64,
    ) -> Result<GatewayEvidence, EvidenceError>;
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
    async fn verify(
        &self,
        evidence_path: &str,
        now_unix: u64,
    ) -> Result<GatewayEvidence, EvidenceError> {
        let mut command = Command::new(&self.helper_path);
        command
            .arg("--socket")
            .arg(evidence_path)
            .arg("--repo")
            .arg(&self.repository)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let output = timeout(HELPER_TIMEOUT, command.output())
            .await
            .map_err(|_| EvidenceError::Unavailable)?
            .map_err(|_| EvidenceError::Unavailable)?;
        if !output.status.success() {
            return Err(EvidenceError::Invalid);
        }
        parse_and_validate(&output.stdout, now_unix)
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
    let bytes = fs::read(path).map_err(|_| EvidenceError::Unavailable)?;
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
    let evidence = serde_json::from_slice(bytes).map_err(|_| EvidenceError::Invalid)?;
    validate_evidence(evidence, now_unix)
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
