use crate::{
    accounting::{Accounting, AccountingError, ReserveResult},
    catalog::Quote,
};
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

/// Opaque identity; no history or content-derived identifier is retained.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConversationId([u8; 32]);

impl ConversationId {
    fn new() -> Self {
        let mut bytes = [0; 32];
        rand::rng().fill_bytes(&mut bytes);
        Self(bytes)
    }
}

#[derive(Clone, Debug)]
pub struct Session {
    pub account_id: String,
    pub csrf: String,
    pub recovery_credential: String,
    pub selected_model: Option<String>,
    pub conversation: ConversationId,
    expires_at: Instant,
}

struct SubmissionToken {
    account_id: String,
    session_id: String,
    conversation: ConversationId,
    model: Option<String>,
    expires_at: Instant,
}

#[derive(Clone, Copy, Debug)]
pub struct BoundSubmission {
    pub id: [u8; 32],
    pub expires_at: Instant,
    pub conversation: ConversationId,
}

#[derive(Debug)]
pub struct Admission {
    pub submission: BoundSubmission,
    pub result: ReserveResult,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AdmissionError {
    #[error("invalid submission")]
    Auth(#[from] AuthError),
    #[error("reservation unavailable")]
    Accounting(#[from] AccountingError),
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
    // Nested lock order: sessions -> submission_tokens -> Accounting::state.
    // Never await while held or call auth from an accounting-held callback.
    sessions: Mutex<HashMap<String, Session>>,
    login_challenges: Mutex<HashMap<String, Instant>>,
    submission_tokens: Mutex<HashMap<String, SubmissionToken>>,
    submission_capacity: usize,
    epoch: [u8; 32],
    #[cfg(test)]
    pause: Mutex<Option<(tests::PausePoint, std::sync::Arc<tests::Pause>)>>,
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
            submission_capacity: MAX_SUBMISSION_TOKENS,
            epoch,
            #[cfg(test)]
            pause: Mutex::new(None),
        })
    }

    #[doc(hidden)]
    pub fn with_submission_capacity(mut self, capacity: usize) -> Self {
        self.submission_capacity = capacity.min(MAX_SUBMISSION_TOKENS);
        self
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
            selected_model: None,
            conversation: ConversationId::new(),
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
        if session.expires_at <= Instant::now() || !Self::verify_csrf(session, csrf) {
            return Err(AuthError::Invalid);
        }
        #[cfg(test)]
        self.pause_at(tests::PausePoint::Invalidation);
        let mut tokens = self
            .submission_tokens
            .lock()
            .map_err(|_| AuthError::Configuration)?;
        tokens.retain(|_, token| token.session_id != id);
        sessions.remove(id);
        Ok(())
    }

    /// Reset auth state only: accepted reservations and account slots belong to
    /// the ledger and survive both reset and logout until a terminal transition.
    pub fn new_chat(&self, id: &str, csrf: &str) -> Result<(), AuthError> {
        let mut sessions = self.sessions.lock().map_err(|_| AuthError::Configuration)?;
        let session = sessions.get_mut(id).ok_or(AuthError::Invalid)?;
        if session.expires_at <= Instant::now() || !Self::verify_csrf(session, csrf) {
            return Err(AuthError::Invalid);
        }
        #[cfg(test)]
        self.pause_at(tests::PausePoint::Invalidation);
        let mut tokens = self
            .submission_tokens
            .lock()
            .map_err(|_| AuthError::Configuration)?;
        tokens.retain(|_, token| token.session_id != id);
        session.conversation = ConversationId::new();
        session.selected_model = None;
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
        let sessions = self.sessions.lock().map_err(|_| AuthError::Configuration)?;
        let session = sessions.get(session_id).ok_or(AuthError::Invalid)?;
        self.insert_submission(session_id, session)
    }

    /// Home/duplicate/completion callers must supply their original snapshot.
    /// Validation and insertion share the session/token locks with reset/logout;
    /// stale completion can neither relock nor mint for the new conversation.
    pub fn issue_submission_for(
        &self,
        session_id: &str,
        conversation: ConversationId,
        model: Option<&str>,
    ) -> Result<String, AuthError> {
        let sessions = self.sessions.lock().map_err(|_| AuthError::Configuration)?;
        let session = sessions.get(session_id).ok_or(AuthError::Invalid)?;
        if session.conversation != conversation || session.selected_model.as_deref() != model {
            return Err(AuthError::Invalid);
        }
        self.insert_submission(session_id, session)
    }

    // Caller holds sessions throughout insertion (never pass a detached snapshot).
    fn insert_submission(&self, session_id: &str, session: &Session) -> Result<String, AuthError> {
        let mut tokens = self
            .submission_tokens
            .lock()
            .map_err(|_| AuthError::Configuration)?;
        let now = Instant::now();
        if session.expires_at <= now {
            return Err(AuthError::Invalid);
        }
        tokens.retain(|_, token| token.expires_at > now);
        if tokens.len() >= self.submission_capacity {
            return Err(AuthError::Capacity);
        }
        #[cfg(test)]
        self.pause_at(tests::PausePoint::Insertion);
        let token = random_token();
        tokens.insert(
            token.clone(),
            SubmissionToken {
                account_id: session.account_id.clone(),
                session_id: session_id.to_owned(),
                conversation: session.conversation,
                model: session.selected_model.clone(),
                expires_at: (now + SUBMISSION_TOKEN_LIFETIME).min(session.expires_at),
            },
        );
        Ok(token)
    }

    /// Synchronous admission; caller must already hold required resource permits
    /// and have completed evidence/catalog/quote verification, before any prompt
    /// transmission. A new successful reserve is ACCOUNTING ACCEPTANCE, not
    /// successful context preflight, generation startup, or client receipt.
    /// Streaming cutover contract (the buffered route has not migrated yet):
    /// after Reserved, construct ReservedGeneration and synchronously transfer
    /// it, owned decoded input, the middleware's exact heavy Arc and global
    /// generation permit into detached preflight, before ANY await/tokenization.
    /// Duplicate must never construct a second refund owner or launch work.
    /// Lock order remains sessions -> submission_tokens -> accounting; reserve
    /// snapshots this authenticated quote, with model/conversation binding.
    /// Failures commit neither token nor model binding. No content/hashes enter
    /// auth or accounting. Accounting never calls back into auth while locked.
    pub fn admit_submission(
        &self,
        accounting: &Accounting,
        session_id: &str,
        csrf: &str,
        token: &str,
        quote: Quote,
    ) -> Result<Admission, AdmissionError> {
        let mut sessions = self.sessions.lock().map_err(|_| AuthError::Configuration)?;
        let mut tokens = self
            .submission_tokens
            .lock()
            .map_err(|_| AuthError::Configuration)?;
        let now = Instant::now();
        let session = sessions.get_mut(session_id).ok_or(AuthError::Invalid)?;
        let issued = tokens.get_mut(token).ok_or(AuthError::Invalid)?;
        let model = &quote.model.id;
        if session.expires_at <= now
            || !Self::verify_csrf(session, csrf)
            || issued.expires_at <= now
            || issued.session_id != session_id
            || issued.account_id != session.account_id
            || issued.conversation != session.conversation
            || issued.model.as_ref().is_some_and(|bound| bound != model)
            || session
                .selected_model
                .as_ref()
                .is_some_and(|bound| bound != model)
        {
            return Err(AuthError::Invalid.into());
        }
        let mut digest = Sha256::new();
        digest.update(self.epoch);
        digest.update(session.conversation.0);
        for field in [session_id, &session.account_id, token, model] {
            digest.update((field.len() as u64).to_be_bytes());
            digest.update(field.as_bytes());
        }
        let submission = BoundSubmission {
            id: digest.finalize().into(),
            expires_at: issued.expires_at,
            conversation: session.conversation,
        };
        // Allocate bindings before acceptance; commit only on reserve success.
        let selected_model = model.clone();
        let token_model = model.clone();
        let model_binding = Sha256::digest(model.as_bytes()).into();
        #[cfg(test)]
        self.pause_at(tests::PausePoint::Reservation);
        let result = accounting.reserve(
            &session.account_id,
            submission.id,
            model_binding,
            quote,
            submission.expires_at,
        )?;
        session.selected_model = Some(selected_model);
        issued.model = Some(token_model);
        Ok(Admission { submission, result })
    }

    #[cfg(test)]
    fn pause_at(&self, point: tests::PausePoint) {
        let pause = {
            let mut slot = self.pause.lock().unwrap();
            if slot
                .as_ref()
                .is_some_and(|(expected, _)| *expected == point)
            {
                slot.take().map(|(_, pause)| pause)
            } else {
                None
            }
        };
        if let Some(pause) = pause {
            pause.entered.wait();
            pause.release.wait();
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{accounting::Outcome, catalog::Model};
    use std::{
        sync::{Arc, Barrier, TryLockError},
        thread,
    };

    #[derive(Clone, Copy, PartialEq, Eq)]
    pub(super) enum PausePoint {
        Invalidation,
        Reservation,
        Insertion,
    }

    pub(super) struct Pause {
        pub entered: Barrier,
        pub release: Barrier,
    }

    fn pause(auth: &Auth, point: PausePoint) -> Arc<Pause> {
        let pause = Arc::new(Pause {
            entered: Barrier::new(2),
            release: Barrier::new(2),
        });
        *auth.pause.lock().unwrap() = Some((point, pause.clone()));
        pause
    }

    fn fixture() -> (Auth, Accounting, String, Session, String) {
        let credential = URL_SAFE_NO_PAD.encode([7; 32]);
        let hash = URL_SAFE_NO_PAD.encode(Sha256::digest(credential.as_bytes()));
        let auth = Auth::from_json(&format!(
            r#"[{{"id":"a","credential_sha256":"{hash}","demo_microunits":1000}}]"#
        ))
        .unwrap();
        let (id, session) = auth
            .authenticate(&credential, &auth.issue_login_challenge().unwrap())
            .unwrap();
        let token = auth.issue_submission(&id).unwrap();
        let ledger = Accounting::new(auth.account_budgets());
        (auth, ledger, id, session, token)
    }

    fn quote() -> Quote {
        Quote {
            model: Model {
                id: "m".into(),
                context_tokens: 20,
                max_output_tokens: 10,
                input_microunits_per_million_tokens: 1_000_000,
                output_microunits_per_million_tokens: 1_000_000,
            },
            input_tokens: 10,
            reserved_microunits: 26,
        }
    }

    fn invalidate(auth: &Auth, id: &str, csrf: &str, logout: bool) {
        if logout {
            auth.logout(id, csrf).unwrap();
        } else {
            auth.new_chat(id, csrf).unwrap();
        }
    }

    fn assert_invalidated(auth: &Auth, id: &str, original: ConversationId, logout: bool) {
        if logout {
            assert!(auth.session(id).is_none());
        } else {
            let current = auth.session(id).unwrap();
            assert_ne!(current.conversation, original);
            assert_eq!(current.selected_model, None);
        }
        assert!(auth.submission_tokens.lock().unwrap().is_empty());
    }

    #[test]
    fn reset_or_logout_winning_session_lock_prevents_admission_and_continuation() {
        for logout in [false, true] {
            let (auth, ledger, id, session, token) = fixture();
            let held = pause(&auth, PausePoint::Invalidation);
            let start = Barrier::new(3);
            thread::scope(|scope| {
                let reset = scope.spawn(|| invalidate(&auth, &id, &session.csrf, logout));
                held.entered.wait();
                assert!(matches!(
                    auth.sessions.try_lock(),
                    Err(TryLockError::WouldBlock)
                ));
                // Reset has sessions but has not acquired tokens or touched accounting.
                assert!(auth.submission_tokens.try_lock().is_ok());
                assert_eq!(ledger.available("a"), Some(1000));
                let admission = scope.spawn(|| {
                    start.wait();
                    auth.admit_submission(&ledger, &id, &session.csrf, &token, quote())
                });
                let continuation = scope.spawn(|| {
                    start.wait();
                    auth.issue_submission_for(&id, session.conversation, None)
                });
                start.wait();
                held.release.wait();
                reset.join().unwrap();
                assert_eq!(
                    admission.join().unwrap().unwrap_err(),
                    AdmissionError::Auth(AuthError::Invalid)
                );
                assert_eq!(continuation.join().unwrap(), Err(AuthError::Invalid));
            });
            assert_eq!(ledger.available("a"), Some(1000));
            assert_invalidated(&auth, &id, session.conversation, logout);
        }
    }

    #[test]
    fn validation_through_reserve_holds_session_then_tokens_even_while_accounting_is_busy() {
        for logout in [false, true] {
            let (auth, ledger, id, session, token) = fixture();
            let held = pause(&auth, PausePoint::Reservation);
            let start = Barrier::new(2);
            thread::scope(|scope| {
                let accounting_lock = ledger.hold_test_lock();
                let admission = scope
                    .spawn(|| auth.admit_submission(&ledger, &id, &session.csrf, &token, quote()));
                held.entered.wait();
                assert!(matches!(
                    auth.sessions.try_lock(),
                    Err(TryLockError::WouldBlock)
                ));
                assert!(matches!(
                    auth.submission_tokens.try_lock(),
                    Err(TryLockError::WouldBlock)
                ));
                let reset = scope.spawn(|| {
                    start.wait();
                    invalidate(&auth, &id, &session.csrf, logout);
                });
                start.wait();
                held.release.wait();
                // No reverse acquisition: accounting's owner can release without
                // any auth access, allowing reserve then invalidation to finish.
                drop(accounting_lock);
                let accepted = admission.join().unwrap().unwrap();
                assert_eq!(accepted.result, ReserveResult::Reserved);
                reset.join().unwrap();
                assert_eq!(ledger.available("a"), Some(974));
                assert_invalidated(&auth, &id, session.conversation, logout);
                assert_eq!(
                    ledger.finish(accepted.submission.id, None).unwrap(),
                    Outcome::Refunded
                );
                assert_eq!(
                    ledger.finish(accepted.submission.id, None).unwrap(),
                    Outcome::Refunded
                );
                assert_eq!(ledger.available("a"), Some(1000));
            });
        }
    }

    #[test]
    fn insertion_winning_session_lock_is_invalidated_by_later_reset_or_logout() {
        for logout in [false, true] {
            let (auth, ledger, id, session, token) = fixture();
            let accepted = auth
                .admit_submission(&ledger, &id, &session.csrf, &token, quote())
                .unwrap();
            let held = pause(&auth, PausePoint::Insertion);
            let start = Barrier::new(2);
            thread::scope(|scope| {
                let issue = scope.spawn(|| {
                    auth.issue_submission_for(&id, accepted.submission.conversation, Some("m"))
                });
                held.entered.wait();
                assert!(matches!(
                    auth.sessions.try_lock(),
                    Err(TryLockError::WouldBlock)
                ));
                assert!(matches!(
                    auth.submission_tokens.try_lock(),
                    Err(TryLockError::WouldBlock)
                ));
                // Terminal operations need no auth locks, even during issuance.
                assert_eq!(
                    ledger.finish(accepted.submission.id, None).unwrap(),
                    Outcome::Refunded
                );
                let reset = scope.spawn(|| {
                    start.wait();
                    invalidate(&auth, &id, &session.csrf, logout);
                });
                start.wait();
                held.release.wait();
                let continuation = issue.join().unwrap().unwrap();
                reset.join().unwrap();
                assert_invalidated(&auth, &id, session.conversation, logout);
                assert!(auth
                    .admit_submission(&ledger, &id, &session.csrf, &continuation, quote())
                    .is_err());
            });
        }
    }

    #[test]
    fn expired_sessions_and_tokens_are_revalidated_without_committing_bindings() {
        for expire_session in [false, true] {
            let (auth, ledger, id, session, token) = fixture();
            let past = Instant::now() - Duration::from_secs(1);
            if expire_session {
                auth.sessions
                    .lock()
                    .unwrap()
                    .get_mut(&id)
                    .unwrap()
                    .expires_at = past;
                assert_eq!(auth.new_chat(&id, &session.csrf), Err(AuthError::Invalid));
                assert_eq!(auth.logout(&id, &session.csrf), Err(AuthError::Invalid));
                assert_eq!(
                    auth.issue_submission_for(&id, session.conversation, None),
                    Err(AuthError::Invalid)
                );
            } else {
                auth.submission_tokens
                    .lock()
                    .unwrap()
                    .get_mut(&token)
                    .unwrap()
                    .expires_at = past;
            }
            assert_eq!(
                auth.admit_submission(&ledger, &id, &session.csrf, &token, quote())
                    .unwrap_err(),
                AdmissionError::Auth(AuthError::Invalid)
            );
            assert_eq!(auth.sessions.lock().unwrap()[&id].selected_model, None);
            assert_eq!(auth.submission_tokens.lock().unwrap()[&token].model, None);
            assert_eq!(ledger.available("a"), Some(1000));
        }
    }

    #[test]
    fn mismatched_account_or_conversation_token_is_rejected_before_reservation() {
        for wrong_account in [false, true] {
            let (auth, ledger, id, session, token) = fixture();
            {
                let mut tokens = auth.submission_tokens.lock().unwrap();
                let issued = tokens.get_mut(&token).unwrap();
                if wrong_account {
                    issued.account_id = "other".into();
                } else {
                    issued.conversation = ConversationId::new();
                }
            }
            assert!(auth
                .admit_submission(&ledger, &id, &session.csrf, &token, quote())
                .is_err());
            assert_eq!(auth.session(&id).unwrap().selected_model, None);
            assert_eq!(ledger.available("a"), Some(1000));
        }
    }
}
