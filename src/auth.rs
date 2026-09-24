use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::RngCore;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;
use thiserror::Error;

const CREDENTIAL_MIN_BYTES: usize = 32;
const CREDENTIAL_MAX_ENCODED_BYTES: usize = 128;
const MAX_ACCOUNTS: usize = 1_000;
const MAX_SESSIONS: usize = 10_000;
const MAX_LOGIN_CHALLENGES: usize = 10_000;
const MAX_SUBMISSION_TOKENS: usize = 100_000;
const SESSION_LIFETIME: Duration = Duration::from_secs(12 * 60 * 60);
const LOGIN_CHALLENGE_LIFETIME: Duration = Duration::from_secs(10 * 60);
const SUBMISSION_TOKEN_LIFETIME: Duration = Duration::from_secs(15 * 60);

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
    expires_at: Instant,
}

struct SubmissionToken {
    account_id: String,
    session_id: String,
    model: Option<String>,
    expires_at: Instant,
}

#[derive(Clone, Copy, Debug)]
pub struct BoundSubmission {
    pub id: [u8; 32],
    pub expires_at: Instant,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AuthError {
    #[error("invalid credentials")]
    Invalid,
    #[error("authentication configuration invalid")]
    Configuration,
    #[error("authentication state capacity reached")]
    Capacity,
}

pub struct Auth {
    accounts: Vec<ProvisionedAccount>,
    sessions: Mutex<HashMap<String, Session>>,
    login_challenges: Mutex<HashMap<String, Instant>>,
    submission_tokens: Mutex<HashMap<String, SubmissionToken>>,
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
            login_challenges: Mutex::new(HashMap::new()),
            submission_tokens: Mutex::new(HashMap::new()),
            epoch,
        })
    }

    pub fn issue_login_challenge(&self) -> Result<String, AuthError> {
        let now = Instant::now();
        let mut challenges = self
            .login_challenges
            .lock()
            .map_err(|_| AuthError::Configuration)?;
        challenges.retain(|_, expires_at| *expires_at > now);
        if challenges.len() >= MAX_LOGIN_CHALLENGES {
            return Err(AuthError::Capacity);
        }
        let challenge = random_token();
        challenges.insert(challenge.clone(), now + LOGIN_CHALLENGE_LIFETIME);
        Ok(challenge)
    }

    pub fn authenticate(
        &self,
        credential: &str,
        login_challenge: &str,
    ) -> Result<(String, Session), AuthError> {
        let now = Instant::now();
        let expires_at = self
            .login_challenges
            .lock()
            .map_err(|_| AuthError::Configuration)?
            .remove(login_challenge)
            .ok_or(AuthError::Invalid)?;
        if expires_at <= now {
            return Err(AuthError::Invalid);
        }
        if credential.len() > CREDENTIAL_MAX_ENCODED_BYTES {
            return Err(AuthError::Invalid);
        }
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
            expires_at: now + SESSION_LIFETIME,
        };
        let mut sessions = self.sessions.lock().map_err(|_| AuthError::Configuration)?;
        sessions.retain(|_, session| session.expires_at > now);
        if sessions.len() >= MAX_SESSIONS {
            return Err(AuthError::Capacity);
        }
        sessions.insert(session_id.clone(), session.clone());
        Ok((session_id, session))
    }

    pub fn session(&self, id: &str) -> Option<Session> {
        let now = Instant::now();
        let mut sessions = self.sessions.lock().ok()?;
        sessions.retain(|_, session| session.expires_at > now);
        sessions.get(id).cloned()
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

    pub fn issue_submission(&self, session_id: &str) -> Result<String, AuthError> {
        let session = self.session(session_id).ok_or(AuthError::Invalid)?;
        let now = Instant::now();
        let mut tokens = self
            .submission_tokens
            .lock()
            .map_err(|_| AuthError::Configuration)?;
        tokens.retain(|_, token| token.expires_at > now);
        if tokens.len() >= MAX_SUBMISSION_TOKENS {
            return Err(AuthError::Capacity);
        }
        let token = random_token();
        tokens.insert(
            token.clone(),
            SubmissionToken {
                account_id: session.account_id,
                session_id: session_id.to_owned(),
                model: None,
                expires_at: now + SUBMISSION_TOKEN_LIFETIME,
            },
        );
        Ok(token)
    }

    pub fn bind_submission(
        &self,
        session_id: &str,
        account_id: &str,
        token: &str,
        model: &str,
    ) -> Result<BoundSubmission, AuthError> {
        let now = Instant::now();
        let mut tokens = self
            .submission_tokens
            .lock()
            .map_err(|_| AuthError::Configuration)?;
        tokens.retain(|_, token| token.expires_at > now);
        let issued = tokens.get_mut(token).ok_or(AuthError::Invalid)?;
        if issued.session_id != session_id || issued.account_id != account_id {
            return Err(AuthError::Invalid);
        }
        match &issued.model {
            Some(bound_model) if bound_model != model => return Err(AuthError::Invalid),
            Some(_) => {}
            None => issued.model = Some(model.to_owned()),
        }
        let mut digest = Sha256::new();
        digest.update(self.epoch);
        digest.update(session_id.as_bytes());
        digest.update(account_id.as_bytes());
        digest.update(token.as_bytes());
        digest.update(model.as_bytes());
        Ok(BoundSubmission {
            id: digest.finalize().into(),
            expires_at: issued.expires_at,
        })
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

pub fn login_challenge_cookie(value: &str) -> String {
    format!(
        "possums_login_csrf={value}; Path=/login; Max-Age=600; Secure; HttpOnly; SameSite=Strict"
    )
}

pub fn clear_session_cookie() -> &'static str {
    "possums_session=; Path=/; Max-Age=0; Secure; HttpOnly; SameSite=Strict"
}
