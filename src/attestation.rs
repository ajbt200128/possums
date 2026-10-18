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

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
