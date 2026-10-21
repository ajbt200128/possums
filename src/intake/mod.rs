//! Gateway-side transport and validated native intake authority for serving.
//!
//! Intake owns authentication and durable accounting. A received HTTP response is
//! not a validated financial receipt; the integration must validate it separately.
pub(crate) mod diagnostics;
mod protocol;
mod transport;
#[cfg(test)]
pub(crate) use protocol::cutoff_fixture;
pub(crate) use protocol::{
    Error, ErrorKind, Intake, Principal, RenewalReply, Reservation, TerminalOutcome,
    TerminalReceipt,
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::Serialize;
use sha2::{Digest, Sha256};

// Deliberately no Debug: both fields are credential material.
#[derive(Serialize)]
struct LoginMaterial {
    identity: String,
    password: String,
}

impl LoginMaterial {
    fn derive(key: &str) -> Result<Self, transport::Failure> {
        let invalid = || transport::Failure::input(transport::Operation::Login);
        if !(43..=128).contains(&key.len())
            || !key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(invalid());
        }
        let decoded = URL_SAFE_NO_PAD.decode(key).map_err(|_| invalid())?;
        if decoded.len() < 32 {
            return Err(invalid());
        }
        let digest = |domain: &[u8]| {
            let mut hash = Sha256::new();
            hash.update(domain);
            hash.update(key.as_bytes());
            hash.finalize()
        };
        Ok(Self {
            identity: format!(
                "v1_{:x}",
                digest(b"possums-intake/customer-key/lookup/v1\0")
            ),
            password: URL_SAFE_NO_PAD.encode(digest(b"possums-intake/customer-key/password/v1\0")),
        })
    }
}

#[cfg(test)]
mod gateway_tests;
#[cfg(test)]
mod tests;
