//! Deployment-provisioned shared keys; no accounts, sessions or credential fallback.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use http::{header, HeaderMap};
use sha2::{Digest, Sha256};
use subtle::{Choice, ConstantTimeEq};

const MAX_KEYS: usize = 8;
const KEY_BYTES: usize = 32;

pub struct SharedKeys(Vec<[u8; 32]>);

#[derive(Debug, thiserror::Error)]
#[error("shared key configuration is invalid")]
pub struct KeyConfigurationError;

impl SharedKeys {
    /// JSON array of 1..=8 canonical base64url (unpadded) 32-byte random keys.
    /// Rotation overlaps old/new keys; runtime keeps only their SHA-256 digests.
    pub fn from_json(json: &str) -> Result<Self, KeyConfigurationError> {
        if json.len() > 1024 {
            return Err(KeyConfigurationError);
        }
        let keys: Vec<String> = serde_json::from_str(json).map_err(|_| KeyConfigurationError)?;
        if keys.is_empty() || keys.len() > MAX_KEYS {
            return Err(KeyConfigurationError);
        }
        let mut digests = Vec::with_capacity(keys.len());
        for key in keys {
            if !valid_key(&key) {
                return Err(KeyConfigurationError);
            }
            let digest = Sha256::digest(key.as_bytes()).into();
            if digests.contains(&digest) {
                return Err(KeyConfigurationError);
            }
            digests.push(digest);
        }
        Ok(Self(digests))
    }

    pub(crate) fn accepts(&self, headers: &HeaderMap) -> bool {
        let mut values = headers.get_all(header::AUTHORIZATION).iter();
        let key = values
            .next()
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "));
        let Some(key) = key
            .filter(|k| valid_key(k))
            .filter(|_| values.next().is_none())
        else {
            return false;
        };
        let digest: [u8; 32] = Sha256::digest(key.as_bytes()).into();
        self.0
            .iter()
            .fold(Choice::from(0), |found, stored| {
                found | stored.ct_eq(&digest)
            })
            .into()
    }
}

fn valid_key(key: &str) -> bool {
    key.len() == 43
        && URL_SAFE_NO_PAD
            .decode(key)
            .is_ok_and(|bytes| bytes.len() == KEY_BYTES && URL_SAFE_NO_PAD.encode(bytes) == key)
}
