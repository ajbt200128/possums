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

#[derive(Clone)]
pub struct Session {
    pub account_id: String,
    pub admission_binding: String,
    pub selected_model: Option<String>,
    pub conversation: ConversationId,
    expires_at: Instant,
}

// Neither credentials, admission binding nor account/conversation identifiers are diagnostic data.
impl std::fmt::Debug for Session {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Session { [redacted] }")
    }
}

struct LoginChallenge {
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
    login_challenges: Mutex<HashMap<String, LoginChallenge>>,
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

    pub fn issue_api_challenge(&self) -> Result<String, AuthError> {
        let now = Instant::now();
        let mut challenges = self
            .login_challenges
            .lock()
            .map_err(|_| AuthError::Configuration)?;
        challenges.retain(|_, challenge| challenge.expires_at > now);
        if challenges.len() >= MAX_LOGIN_CHALLENGES {
            return Err(AuthError::Capacity);
        }
        let challenge = random_token();
        challenges.insert(
            challenge.clone(),
            LoginChallenge {
                expires_at: now + LOGIN_CHALLENGE_LIFETIME,
            },
        );
        Ok(challenge)
    }

    pub fn authenticate_api(
        &self,
        credential: &str,
        login_challenge: &str,
    ) -> Result<(String, Session), AuthError> {
        let now = Instant::now();
        let challenge = self
            .login_challenges
            .lock()
            .map_err(|_| AuthError::Configuration)?
            .remove(login_challenge)
            .ok_or(AuthError::Invalid)?;
        if challenge.expires_at <= now {
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
            admission_binding: random_token(),
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

    pub fn api_session(&self, id: &str) -> Option<Session> {
        let now = Instant::now();
        let mut sessions = self.sessions.lock().ok()?;
        sessions.retain(|_, session| session.expires_at > now);
        sessions.get(id).cloned()
    }

    pub fn logout_api(&self, id: &str) -> Result<(), AuthError> {
        let mut sessions = self.sessions.lock().map_err(|_| AuthError::Invalid)?;
        let session = sessions.get(id).ok_or(AuthError::Invalid)?;
        if session.expires_at <= Instant::now() {
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

    pub fn verify_admission_binding(session: &Session, binding: &str) -> bool {
        constant_time_equal(session.admission_binding.as_bytes(), binding.as_bytes())
    }

    pub fn account_budgets(&self) -> impl Iterator<Item = (String, u64)> + '_ {
        self.accounts
            .iter()
            .map(|account| (account.id.clone(), account.demo_microunits))
    }

    /// Atomically bind issuance to the selected model, optionally invalidating the
    /// previous conversation. Capacity failure leaves the old conversation intact.
    /// The caller validates the model against the authenticated catalog first.
    pub fn issue_api_submission(
        &self,
        id: &str,
        model: &str,
        new_conversation: bool,
    ) -> Result<String, AuthError> {
        let mut sessions = self.sessions.lock().map_err(|_| AuthError::Configuration)?;
        let session = sessions.get_mut(id).ok_or(AuthError::Invalid)?;
        if session.expires_at <= Instant::now()
            || (!new_conversation
                && session
                    .selected_model
                    .as_deref()
                    .is_some_and(|bound| bound != model))
        {
            return Err(AuthError::Invalid);
        }
        let mut next = session.clone();
        next.selected_model = Some(model.to_owned());
        if new_conversation {
            next.conversation = ConversationId::new();
        }
        #[cfg(test)]
        if new_conversation {
            self.pause_at(tests::PausePoint::Invalidation);
        }
        let mut tokens = self
            .submission_tokens
            .lock()
            .map_err(|_| AuthError::Configuration)?;
        let now = Instant::now();
        tokens.retain(|_, token| token.expires_at > now);
        let retained = tokens
            .values()
            .filter(|token| !new_conversation || token.session_id != id)
            .count();
        if retained >= self.submission_capacity {
            return Err(AuthError::Capacity);
        }
        if new_conversation {
            tokens.retain(|_, token| token.session_id != id);
        }
        let token = Self::insert_token(&mut tokens, id, &next, now);
        #[cfg(test)]
        self.pause_at(tests::PausePoint::Insertion);
        *session = next;
        Ok(token)
    }

    fn insert_token(
        tokens: &mut HashMap<String, SubmissionToken>,
        session_id: &str,
        session: &Session,
        now: Instant,
    ) -> String {
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
        token
    }

    /// Synchronous admission; caller must already hold required resource permits
    /// and have completed evidence/catalog/quote verification, before any prompt
    /// transmission. A new successful reserve is ACCOUNTING ACCEPTANCE, not
    /// successful context preflight, generation startup, or client receipt.
    /// Production route handoff contract:
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
        admission_binding: &str,
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
            || !Self::verify_admission_binding(session, admission_binding)
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
            .authenticate_api(&credential, &auth.issue_api_challenge().unwrap())
            .unwrap();
        let token = auth.issue_api_submission(&id, "m", false).unwrap();
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

    fn invalidate(auth: &Auth, id: &str, logout: bool) {
        if logout {
            auth.logout_api(id).unwrap();
        } else {
            auth.issue_api_submission(id, "m", true).unwrap();
        }
    }

    fn assert_invalidated(auth: &Auth, id: &str, original: ConversationId, logout: bool) {
        if logout {
            assert!(auth.api_session(id).is_none());
        } else {
            assert_ne!(auth.api_session(id).unwrap().conversation, original);
        }
        assert!(auth
            .submission_tokens
            .lock()
            .unwrap()
            .values()
            .all(|token| token.session_id != id || !logout));
    }

    #[test]
    fn reset_or_logout_winning_session_lock_prevents_admission() {
        for logout in [false, true] {
            let (auth, ledger, id, session, token) = fixture();
            let held = pause(&auth, PausePoint::Invalidation);
            let start = Barrier::new(2);
            thread::scope(|scope| {
                let reset = scope.spawn(|| invalidate(&auth, &id, logout));
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
                    auth.admit_submission(&ledger, &id, &session.admission_binding, &token, quote())
                });
                start.wait();
                held.release.wait();
                reset.join().unwrap();
                assert_eq!(
                    admission.join().unwrap().unwrap_err(),
                    AdmissionError::Auth(AuthError::Invalid)
                );
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
                let admission = scope.spawn(|| {
                    auth.admit_submission(&ledger, &id, &session.admission_binding, &token, quote())
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
                let reset = scope.spawn(|| {
                    start.wait();
                    invalidate(&auth, &id, logout);
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
                assert_eq!(
                    auth.issue_api_submission(&id, "m", true),
                    Err(AuthError::Invalid)
                );
                assert_eq!(auth.logout_api(&id), Err(AuthError::Invalid));
            } else {
                auth.submission_tokens
                    .lock()
                    .unwrap()
                    .get_mut(&token)
                    .unwrap()
                    .expires_at = past;
            }
            assert_eq!(
                auth.admit_submission(&ledger, &id, &session.admission_binding, &token, quote())
                    .unwrap_err(),
                AdmissionError::Auth(AuthError::Invalid)
            );
            assert_eq!(
                auth.sessions.lock().unwrap()[&id].selected_model.as_deref(),
                Some("m")
            );
            assert_eq!(
                auth.submission_tokens.lock().unwrap()[&token]
                    .model
                    .as_deref(),
                Some("m")
            );
            assert_eq!(ledger.available("a"), Some(1000));
        }
    }

    fn api_fixture() -> (Auth, Accounting, String, Session, String) {
        let (auth, ledger, previous_id, _, _) = fixture();
        auth.logout_api(&previous_id).unwrap();
        let (id, session) = auth
            .authenticate_api(
                &URL_SAFE_NO_PAD.encode([7; 32]),
                &auth.issue_api_challenge().unwrap(),
            )
            .unwrap();
        let token = auth.issue_api_submission(&id, "m", false).unwrap();
        (auth, ledger, id, session, token)
    }

    #[test]
    fn api_challenge_and_session_capacity_expiry_and_single_use() {
        let (auth, _, id, session, _) = api_fixture();
        let credential = URL_SAFE_NO_PAD.encode([7; 32]);
        let past = Instant::now() - Duration::from_secs(1);
        let challenge = auth.issue_api_challenge().unwrap();
        auth.login_challenges
            .lock()
            .unwrap()
            .get_mut(&challenge)
            .unwrap()
            .expires_at = past;
        assert!(auth.authenticate_api(&credential, &challenge).is_err());
        assert!(!auth
            .login_challenges
            .lock()
            .unwrap()
            .contains_key(&challenge));
        for _ in 0..MAX_LOGIN_CHALLENGES {
            auth.issue_api_challenge().unwrap();
        }
        assert_eq!(auth.issue_api_challenge(), Err(AuthError::Capacity));
        for challenge in auth.login_challenges.lock().unwrap().values_mut() {
            challenge.expires_at = past;
        }
        let challenge = auth.issue_api_challenge().unwrap();
        assert_eq!(auth.login_challenges.lock().unwrap().len(), 1);
        {
            let mut sessions = auth.sessions.lock().unwrap();
            for index in 1..MAX_SESSIONS {
                sessions.insert(index.to_string(), session.clone());
            }
        }
        assert!(matches!(
            auth.authenticate_api(&credential, &challenge),
            Err(AuthError::Capacity)
        ));
        assert!(matches!(
            auth.authenticate_api(&credential, &challenge),
            Err(AuthError::Invalid)
        ));
        for session in auth.sessions.lock().unwrap().values_mut() {
            session.expires_at = past;
        }
        assert!(auth.api_session(&id).is_none());
        assert!(auth.logout_api(&id).is_err());
        assert!(auth.issue_api_submission(&id, "m", true).is_err());
        let challenge = auth.issue_api_challenge().unwrap();
        auth.authenticate_api(&credential, &challenge).unwrap();
        assert_eq!(auth.sessions.lock().unwrap().len(), 1);
    }

    #[test]
    fn api_token_capacity_reset_rollback_and_expiry() {
        let (mut auth, ledger, id, session, token) = api_fixture();
        auth.submission_capacity = 1;
        assert_eq!(
            auth.issue_api_submission(&id, "m", false),
            Err(AuthError::Capacity)
        );
        let replacement = auth.issue_api_submission(&id, "other", true).unwrap();
        let current = auth.api_session(&id).unwrap();
        assert_ne!(session.conversation, current.conversation);
        assert!(auth
            .admit_submission(&ledger, &id, &session.admission_binding, &token, quote())
            .is_err());
        auth.submission_capacity = 0;
        assert_eq!(
            auth.issue_api_submission(&id, "m", true),
            Err(AuthError::Capacity)
        );
        assert_eq!(
            auth.api_session(&id).unwrap().conversation,
            current.conversation
        );
        assert!(auth
            .submission_tokens
            .lock()
            .unwrap()
            .contains_key(&replacement));
        auth.submission_capacity = 1;
        auth.submission_tokens
            .lock()
            .unwrap()
            .get_mut(&replacement)
            .unwrap()
            .expires_at = Instant::now() - Duration::from_secs(1);
        let mut other = quote();
        other.model.id = "other".into();
        assert!(auth
            .admit_submission(
                &ledger,
                &id,
                &session.admission_binding,
                &replacement,
                other
            )
            .is_err());
        auth.issue_api_submission(&id, "other", false).unwrap();
        assert_eq!(ledger.available("a"), Some(1000));
        auth.sessions
            .lock()
            .unwrap()
            .get_mut(&id)
            .unwrap()
            .expires_at = Instant::now() - Duration::from_secs(1);
        assert!(auth.issue_api_submission(&id, "other", true).is_err());
    }

    fn invalidate_api(auth: &Auth, id: &str, logout: bool) {
        if logout {
            auth.logout_api(id).unwrap();
        } else {
            auth.issue_api_submission(id, "other", true).unwrap();
        }
    }

    #[test]
    fn api_reset_logout_race_with_admission_preserves_accepted_accounting() {
        for logout in [false, true] {
            for admission_first in [false, true] {
                let (auth, ledger, id, session, token) = api_fixture();
                let point = if admission_first {
                    PausePoint::Reservation
                } else {
                    PausePoint::Invalidation
                };
                let held = pause(&auth, point);
                thread::scope(|scope| {
                    if admission_first {
                        let admission = scope.spawn(|| {
                            auth.admit_submission(
                                &ledger,
                                &id,
                                &session.admission_binding,
                                &token,
                                quote(),
                            )
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
                        let invalidation = scope.spawn(|| invalidate_api(&auth, &id, logout));
                        held.release.wait();
                        let accepted = admission.join().unwrap().unwrap();
                        invalidation.join().unwrap();
                        assert_eq!(ledger.available("a"), Some(974));
                        assert_eq!(
                            ledger.finish(accepted.submission.id, None).unwrap(),
                            Outcome::Refunded
                        );
                        assert_eq!(
                            ledger.finish(accepted.submission.id, None).unwrap(),
                            Outcome::Refunded
                        );
                    } else {
                        let invalidation = scope.spawn(|| invalidate_api(&auth, &id, logout));
                        held.entered.wait();
                        assert!(matches!(
                            auth.sessions.try_lock(),
                            Err(TryLockError::WouldBlock)
                        ));
                        let admission = scope.spawn(|| {
                            auth.admit_submission(
                                &ledger,
                                &id,
                                &session.admission_binding,
                                &token,
                                quote(),
                            )
                        });
                        held.release.wait();
                        invalidation.join().unwrap();
                        assert!(admission.join().unwrap().is_err());
                    }
                });
                assert_eq!(ledger.available("a"), Some(1000));
            }
        }
    }

    #[test]
    fn api_issuance_reset_logout_are_linearized_by_the_session_lock() {
        for logout in [false, true] {
            for issuance_first in [false, true] {
                let (auth, ledger, id, session, _) = api_fixture();
                let point = if issuance_first {
                    PausePoint::Insertion
                } else {
                    PausePoint::Invalidation
                };
                let held = pause(&auth, point);
                thread::scope(|scope| {
                    if issuance_first {
                        let issue = scope.spawn(|| auth.issue_api_submission(&id, "m", false));
                        held.entered.wait();
                        assert!(matches!(
                            auth.sessions.try_lock(),
                            Err(TryLockError::WouldBlock)
                        ));
                        let invalidation = scope.spawn(|| invalidate_api(&auth, &id, logout));
                        held.release.wait();
                        let token = issue.join().unwrap().unwrap();
                        invalidation.join().unwrap();
                        assert!(auth
                            .admit_submission(
                                &ledger,
                                &id,
                                &session.admission_binding,
                                &token,
                                quote()
                            )
                            .is_err());
                    } else {
                        let invalidation = scope.spawn(|| invalidate_api(&auth, &id, logout));
                        held.entered.wait();
                        let issue = scope.spawn(|| auth.issue_api_submission(&id, "m", false));
                        held.release.wait();
                        invalidation.join().unwrap();
                        assert!(issue.join().unwrap().is_err());
                    }
                });
                assert_eq!(ledger.available("a"), Some(1000));
            }
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
                .admit_submission(&ledger, &id, &session.admission_binding, &token, quote())
                .is_err());
            assert_eq!(
                auth.api_session(&id).unwrap().selected_model.as_deref(),
                Some("m")
            );
            assert_eq!(ledger.available("a"), Some(1000));
        }
    }
}
