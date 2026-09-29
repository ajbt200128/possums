use crate::catalog::{actual_cost, marked_up_cost, CatalogError, Quote};
use std::{collections::HashMap, sync::Mutex, time::Instant};
use thiserror::Error;

const MAX_SUBMISSIONS: usize = 100_000;
const MAX_ACCOUNT_IN_FLIGHT: u32 = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    InFlight,
    AwaitingDelivery,
    Settled { charged: u64 },
    Refunded,
}

/// Counts from the last authenticated usage event. The caller must establish
/// successful finish, [DONE], EOF, and absence of upstream errors before use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FinalUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Clone, Debug)]
struct Submission {
    account_id: String,
    request_digest: [u8; 32],
    quote: Quote,
    outcome: Outcome,
    pending_charge: Option<u64>,
    token_expires_at: Instant,
}

#[derive(Clone, Debug)]
struct Account {
    available: u64,
    in_flight: u32,
}

#[derive(Default)]
struct State {
    accounts: HashMap<String, Account>,
    submissions: HashMap<[u8; 32], Submission>,
}

pub struct Accounting {
    state: Mutex<State>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AccountingError {
    #[error("account unavailable")]
    UnknownAccount,
    #[error("insufficient demo credit")]
    InsufficientCredit,
    #[error("account concurrency limit reached")]
    Concurrency,
    #[error("submission token already used for different input")]
    AlteredDuplicate,
    #[error("submission state capacity reached")]
    Capacity,
    #[error("invalid accounting transition")]
    InvalidTransition,
    #[error("cost cannot be represented")]
    Cost,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReserveResult {
    Reserved,
    Duplicate(Outcome),
}

impl Accounting {
    pub fn new(accounts: impl IntoIterator<Item = (String, u64)>) -> Self {
        let accounts = accounts
            .into_iter()
            .map(|(id, available)| {
                (
                    id,
                    Account {
                        available,
                        in_flight: 0,
                    },
                )
            })
            .collect();
        Self {
            state: Mutex::new(State {
                accounts,
                submissions: HashMap::new(),
            }),
        }
    }

    /// Auth admission calls this while holding sessions then submission tokens.
    /// This is the innermost lock: no callback into auth or async work is allowed.
    /// `request_digest` is a model binding, never a prompt/history content hash.
    /// Terminal methods release this lock before callers issue continuations.
    pub fn reserve(
        &self,
        account_id: &str,
        submission_id: [u8; 32],
        request_digest: [u8; 32],
        quote: Quote,
        token_expires_at: Instant,
    ) -> Result<ReserveResult, AccountingError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| AccountingError::InvalidTransition)?;
        state.submissions.retain(|_, submission| {
            !is_terminal(&submission.outcome) || submission.token_expires_at > Instant::now()
        });
        if token_expires_at <= Instant::now() {
            return Err(AccountingError::InvalidTransition);
        }
        if let Some(existing) = state.submissions.get(&submission_id) {
            if existing.account_id != account_id || existing.request_digest != request_digest {
                return Err(AccountingError::AlteredDuplicate);
            }
            return Ok(ReserveResult::Duplicate(existing.outcome.clone()));
        }
        if state.submissions.len() >= MAX_SUBMISSIONS {
            return Err(AccountingError::Capacity);
        }
        let account = state
            .accounts
            .get_mut(account_id)
            .ok_or(AccountingError::UnknownAccount)?;
        if account.in_flight >= MAX_ACCOUNT_IN_FLIGHT {
            return Err(AccountingError::Concurrency);
        }
        if account.available < quote.reserved_microunits {
            return Err(AccountingError::InsufficientCredit);
        }
        account.available -= quote.reserved_microunits;
        account.in_flight += 1;
        state.submissions.insert(
            submission_id,
            Submission {
                account_id: account_id.to_owned(),
                request_digest,
                quote,
                outcome: Outcome::InFlight,
                pending_charge: None,
                token_expires_at,
            },
        );
        Ok(ReserveResult::Reserved)
    }

    /// Atomically finish a future streaming reservation, independently of receipt.
    /// Pass None for any protocol/upstream failure or missing/invalid usage.
    /// Invalid totals or unrepresentable charges also refund. Terminal outcomes
    /// are absorbing: repeated or conflicting calls return the original outcome.
    pub fn finish(
        &self,
        submission_id: [u8; 32],
        usage: Option<FinalUsage>,
    ) -> Result<Outcome, AccountingError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| AccountingError::InvalidTransition)?;
        let State {
            accounts,
            submissions,
        } = &mut *state;
        let submission = submissions
            .get_mut(&submission_id)
            .ok_or(AccountingError::InvalidTransition)?;
        if is_terminal(&submission.outcome) {
            return Ok(submission.outcome.clone());
        }
        let reserved = submission.quote.reserved_microunits;
        let charge = usage.and_then(|usage| {
            if usage.input_tokens.checked_add(usage.output_tokens) != Some(usage.total_tokens) {
                return None;
            }
            marked_up_cost(
                usage.input_tokens,
                usage.output_tokens,
                submission.quote.model.input_microunits_per_million_tokens,
                submission.quote.model.output_microunits_per_million_tokens,
            )
            .ok()
        });
        // Operational token bounds are not billable-token validation. The
        // reservation alone caps user liability; the operator absorbs excess.
        let outcome = charge.map_or(Outcome::Refunded, |charge| Outcome::Settled {
            charged: charge.min(reserved),
        });
        let charged = match outcome {
            Outcome::Settled { charged } => charged,
            _ => 0,
        };
        let account = accounts
            .get_mut(&submission.account_id)
            .ok_or(AccountingError::InvalidTransition)?;
        let available = account
            .available
            .checked_add(reserved - charged)
            .ok_or(AccountingError::Cost)?;
        let in_flight = account
            .in_flight
            .checked_sub(1)
            .ok_or(AccountingError::InvalidTransition)?;
        account.available = available;
        account.in_flight = in_flight;
        submission.outcome = outcome.clone();
        submission.pending_charge = None;
        Ok(outcome)
    }

    // Temporary buffered-route compatibility. Streaming uses finish instead.
    pub fn prepare_settlement(
        &self,
        submission_id: [u8; 32],
        input_tokens: u64,
        output_tokens: u64,
    ) -> Result<u64, AccountingError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| AccountingError::InvalidTransition)?;
        let submission = state
            .submissions
            .get(&submission_id)
            .ok_or(AccountingError::InvalidTransition)?;
        if submission.outcome != Outcome::InFlight || input_tokens > submission.quote.input_tokens {
            return Err(AccountingError::InvalidTransition);
        }
        let mut actual_quote = submission.quote.clone();
        actual_quote.input_tokens = input_tokens;
        let charged = actual_cost(&actual_quote, output_tokens).map_err(map_cost)?;
        if charged > submission.quote.reserved_microunits {
            return Err(AccountingError::InvalidTransition);
        }
        let submission = state
            .submissions
            .get_mut(&submission_id)
            .ok_or(AccountingError::InvalidTransition)?;
        submission.outcome = Outcome::AwaitingDelivery;
        submission.pending_charge = Some(charged);
        Ok(charged)
    }

    pub fn settle(&self, submission_id: [u8; 32]) -> Result<u64, AccountingError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| AccountingError::InvalidTransition)?;
        let submission = state
            .submissions
            .get(&submission_id)
            .ok_or(AccountingError::InvalidTransition)?;
        if let Outcome::Settled { charged } = submission.outcome {
            return Ok(charged);
        }
        if submission.outcome != Outcome::AwaitingDelivery {
            return Err(AccountingError::InvalidTransition);
        }
        let charged = submission
            .pending_charge
            .ok_or(AccountingError::InvalidTransition)?;
        let account_id = submission.account_id.clone();
        let refund = submission.quote.reserved_microunits - charged;
        let account = state
            .accounts
            .get_mut(&account_id)
            .ok_or(AccountingError::InvalidTransition)?;
        account.available = account
            .available
            .checked_add(refund)
            .ok_or(AccountingError::Cost)?;
        account.in_flight = account
            .in_flight
            .checked_sub(1)
            .ok_or(AccountingError::InvalidTransition)?;
        let submission = state
            .submissions
            .get_mut(&submission_id)
            .ok_or(AccountingError::InvalidTransition)?;
        submission.outcome = Outcome::Settled { charged };
        submission.pending_charge = None;
        Ok(charged)
    }

    pub fn refund(&self, submission_id: [u8; 32]) -> Result<(), AccountingError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| AccountingError::InvalidTransition)?;
        let submission = state
            .submissions
            .get(&submission_id)
            .ok_or(AccountingError::InvalidTransition)?;
        match submission.outcome {
            Outcome::Refunded => return Ok(()),
            Outcome::Settled { .. } => return Err(AccountingError::InvalidTransition),
            Outcome::InFlight | Outcome::AwaitingDelivery => {}
        }
        let account_id = submission.account_id.clone();
        let reserved = submission.quote.reserved_microunits;
        let account = state
            .accounts
            .get_mut(&account_id)
            .ok_or(AccountingError::InvalidTransition)?;
        account.available = account
            .available
            .checked_add(reserved)
            .ok_or(AccountingError::Cost)?;
        account.in_flight = account
            .in_flight
            .checked_sub(1)
            .ok_or(AccountingError::InvalidTransition)?;
        state
            .submissions
            .get_mut(&submission_id)
            .ok_or(AccountingError::InvalidTransition)?
            .outcome = Outcome::Refunded;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn hold_test_lock(&self) -> impl Drop + '_ {
        self.state.lock().unwrap()
    }

    pub fn available(&self, account_id: &str) -> Option<u64> {
        self.state
            .lock()
            .ok()?
            .accounts
            .get(account_id)
            .map(|account| account.available)
    }
}

fn is_terminal(outcome: &Outcome) -> bool {
    matches!(outcome, Outcome::Settled { .. } | Outcome::Refunded)
}

fn map_cost(_: CatalogError) -> AccountingError {
    AccountingError::Cost
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Model;
    use std::time::Duration;

    fn quote() -> Quote {
        Quote {
            model: Model {
                id: "m".into(),
                context_tokens: 2,
                max_output_tokens: 1,
                input_microunits_per_million_tokens: 1_000_000,
                output_microunits_per_million_tokens: 1_000_000,
            },
            input_tokens: 1,
            reserved_microunits: 3,
        }
    }

    #[test]
    fn usage_terminal_retains_only_prompt_free_tombstone_through_expiry() {
        let ledger = Accounting::new([("a".into(), 100)]);
        let expiry = Instant::now() + Duration::from_secs(60);
        let binding = [2; 32]; // Model binding only; never a prompt/history hash.
        let original = quote();
        ledger
            .reserve("a", [1; 32], binding, original.clone(), expiry)
            .unwrap();
        ledger
            .finish(
                [1; 32],
                Some(FinalUsage {
                    input_tokens: 100,
                    output_tokens: 101,
                    total_tokens: 201,
                }),
            )
            .unwrap();
        {
            let state = ledger.state.lock().unwrap();
            // Exhaustive retained-state schema: no content, content hashes, or
            // final usage event is added to the terminal tombstone.
            let Submission {
                account_id,
                request_digest,
                quote,
                outcome,
                pending_charge,
                token_expires_at,
            } = &state.submissions[&[1; 32]];
            assert_eq!(account_id, "a");
            assert_eq!(*request_digest, binding);
            assert_eq!(*quote, original);
            assert_eq!(*outcome, Outcome::Settled { charged: 3 });
            assert_eq!(*pending_charge, None);
            assert_eq!(*token_expires_at, expiry);
            assert_eq!(state.accounts["a"].in_flight, 0);
        }
        assert_eq!(
            ledger
                .reserve("a", [1; 32], binding, original.clone(), expiry)
                .unwrap(),
            ReserveResult::Duplicate(Outcome::Settled { charged: 3 })
        );
        let expired = Instant::now() - Duration::from_secs(1);
        ledger
            .state
            .lock()
            .unwrap()
            .submissions
            .get_mut(&[1; 32])
            .unwrap()
            .token_expires_at = expired;
        assert_eq!(
            ledger.reserve("a", [1; 32], binding, original, expired),
            Err(AccountingError::InvalidTransition)
        );
        assert!(ledger.state.lock().unwrap().submissions.is_empty());
        assert_eq!(ledger.available("a"), Some(97));
    }

    #[test]
    fn expired_in_flight_reservation_is_retained_until_terminal_completion() {
        let ledger = Accounting::new([("a".into(), 100)]);
        let expiry = Instant::now() + Duration::from_secs(60);
        ledger
            .reserve("a", [1; 32], [2; 32], quote(), expiry)
            .unwrap();
        ledger
            .state
            .lock()
            .unwrap()
            .submissions
            .get_mut(&[1; 32])
            .unwrap()
            .token_expires_at = Instant::now() - Duration::from_secs(1);
        ledger
            .reserve("a", [3; 32], [2; 32], quote(), expiry)
            .unwrap();
        assert_eq!(ledger.state.lock().unwrap().submissions.len(), 2);
        assert_eq!(ledger.finish([1; 32], None).unwrap(), Outcome::Refunded);
        assert_eq!(ledger.available("a"), Some(97));
        ledger
            .reserve("a", [4; 32], [2; 32], quote(), expiry)
            .unwrap();
        let state = ledger.state.lock().unwrap();
        assert!(!state.submissions.contains_key(&[1; 32]));
        assert_eq!(state.accounts["a"].in_flight, 2);
    }

    #[test]
    fn full_terminal_capacity_reclaims_only_expired_tokens() {
        let future = Instant::now() + Duration::from_secs(60);
        let quote = quote();
        let submissions = (0..MAX_SUBMISSIONS)
            .map(|number| {
                let mut id = [0_u8; 32];
                id[..8].copy_from_slice(&(number as u64).to_le_bytes());
                (
                    id,
                    Submission {
                        account_id: "a".into(),
                        request_digest: [1; 32],
                        quote: quote.clone(),
                        outcome: Outcome::Refunded,
                        pending_charge: None,
                        token_expires_at: future,
                    },
                )
            })
            .collect();
        let ledger = Accounting {
            state: Mutex::new(State {
                accounts: [(
                    "a".into(),
                    Account {
                        available: 100,
                        in_flight: 0,
                    },
                )]
                .into_iter()
                .collect(),
                submissions,
            }),
        };
        assert_eq!(
            ledger
                .reserve("a", [255; 32], [2; 32], quote.clone(), future)
                .unwrap_err(),
            AccountingError::Capacity
        );
        for submission in ledger.state.lock().unwrap().submissions.values_mut() {
            submission.token_expires_at = Instant::now() - Duration::from_secs(1);
        }
        assert_eq!(
            ledger
                .reserve("a", [255; 32], [2; 32], quote, future)
                .unwrap(),
            ReserveResult::Reserved
        );
        assert_eq!(ledger.state.lock().unwrap().submissions.len(), 1);
    }
}
