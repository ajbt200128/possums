use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::RngCore;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{collections::HashMap, sync::Mutex};
use subtle::ConstantTimeEq;
use thiserror::Error;

const CREDENTIAL_MIN_BYTES: usize = 32;
const MAX_ACCOUNTS: usize = 1_000;

#[derive(Clone, Debug, Deserialize)]
pub struct ProvisionedAccount {
    pub id: String,
    pub credential_sha256: String,
    pub demo_microunits: u64,
}

#[derive(Clone, Debug)]
pub struct Session {
    pub account_id: String,
    pub csrf: String,
    pub recovery_credential: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AuthError {
    #[error("invalid credentials")]
    Invalid,
    #[error("authentication configuration invalid")]
    Configuration,
}

pub struct Auth {
    accounts: Vec<ProvisionedAccount>,
    sessions: Mutex<HashMap<String, Session>>,
    epoch: [u8; 32],
}

impl Auth {
    pub fn from_json(json: &str) -> Result<Self, AuthError> {
        let accounts: Vec<ProvisionedAccount> =
            serde_json::from_str(json).map_err(|_| AuthError::Configuration)?;
        if accounts.is_empty() || accounts.len() > MAX_ACCOUNTS {
            return Err(AuthError::Configuration);
        }
        for account in &accounts {
            let hash = decode_hash(&account.credential_sha256)?;
            if account.id.is_empty() || account.id.len() > 128 || account.demo_microunits == 0 {
                return Err(AuthError::Configuration);
            }
            let _ = hash;
        }
        let mut epoch = [0; 32];
        rand::rng().fill_bytes(&mut epoch);
        Ok(Self {
            accounts,
            sessions: Mutex::new(HashMap::new()),
            epoch,
        })
    }

    pub fn authenticate(&self, credential: &str) -> Result<(String, Session), AuthError> {
        let decoded = URL_SAFE_NO_PAD
            .decode(credential)
            .map_err(|_| AuthError::Invalid)?;
        if decoded.len() < CREDENTIAL_MIN_BYTES {
            return Err(AuthError::Invalid);
        }
        let candidate: [u8; 32] = Sha256::digest(credential.as_bytes()).into();
        let mut matched = None;
        for account in &self.accounts {
            let expected = decode_hash(&account.credential_sha256)?;
            if bool::from(expected.ct_eq(&candidate)) {
                matched = Some(account);
            }
        }
        let account = matched.ok_or(AuthError::Invalid)?;
        let session_id = random_token();
        let session = Session {
            account_id: account.id.clone(),
            csrf: random_token(),
            recovery_credential: credential.to_owned(),
        };
        self.sessions
            .lock()
            .map_err(|_| AuthError::Configuration)?
            .insert(session_id.clone(), session.clone());
        Ok((session_id, session))
    }

    pub fn session(&self, id: &str) -> Option<Session> {
        self.sessions.lock().ok()?.get(id).cloned()
    }

    pub fn logout(&self, id: &str, csrf: &str) -> Result<(), AuthError> {
        let mut sessions = self.sessions.lock().map_err(|_| AuthError::Invalid)?;
        let session = sessions.get(id).ok_or(AuthError::Invalid)?;
        if !constant_time_equal(session.csrf.as_bytes(), csrf.as_bytes()) {
            return Err(AuthError::Invalid);
        }
        sessions.remove(id);
        Ok(())
    }

    pub fn verify_csrf(session: &Session, csrf: &str) -> bool {
        constant_time_equal(session.csrf.as_bytes(), csrf.as_bytes())
    }

    pub fn account_budgets(&self) -> impl Iterator<Item = (String, u64)> + '_ {
        self.accounts
            .iter()
            .map(|account| (account.id.clone(), account.demo_microunits))
    }

    pub fn bind_submission(&self, account_id: &str, token: &str) -> [u8; 32] {
        let mut digest = Sha256::new();
        digest.update(self.epoch);
        digest.update(account_id.as_bytes());
        digest.update(token.as_bytes());
        digest.finalize().into()
    }
}

fn decode_hash(encoded: &str) -> Result<[u8; 32], AuthError> {
    let decoded = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| AuthError::Configuration)?;
    decoded.try_into().map_err(|_| AuthError::Configuration)
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len() && bool::from(left.ct_eq(right))
}

pub fn random_token() -> String {
    let mut bytes = [0; 32];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn session_cookie(value: &str) -> String {
    format!("possums_session={value}; Path=/; Secure; HttpOnly; SameSite=Strict")
}

pub fn clear_session_cookie() -> &'static str {
    "possums_session=; Path=/; Max-Age=0; Secure; HttpOnly; SameSite=Strict"
}
