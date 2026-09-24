use serde::{Deserialize, Serialize};
use std::{fs, path::Path};
use thiserror::Error;

const MAX_EVIDENCE_BYTES: u64 = 256 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GatewayEvidence {
    pub quote: serde_json::Value,
    pub release_digest: String,
    pub endpoint_key_sha256: String,
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
    if metadata.len() == 0 || metadata.len() > MAX_EVIDENCE_BYTES {
        return Err(EvidenceError::Invalid);
    }
    let bytes = fs::read(path).map_err(|_| EvidenceError::Unavailable)?;
    let evidence: GatewayEvidence =
        serde_json::from_slice(&bytes).map_err(|_| EvidenceError::Invalid)?;
    if evidence.release_digest.is_empty() || evidence.endpoint_key_sha256.len() != 64 {
        return Err(EvidenceError::Invalid);
    }
    Ok(evidence)
}
