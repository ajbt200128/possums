use crate::catalog::{actual_cost, CatalogError, Quote};
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};
use thiserror::Error;

const MAX_SUBMISSIONS: usize = 100_000;
const MAX_ACCOUNT_IN_FLIGHT: u32 = 3;
const CONFIRMATION_LIFETIME: Duration = Duration::from_secs(5 * 60);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    InFlight,
    AwaitingConfirmation,
    Settled { charged: u64 },
    Refunded,
}

#[derive(Clone, Debug)]
struct Submission {
    account_id: String,
    request_digest: [u8; 32],
    quote: Quote,
    outcome: Outcome,
    pending_charge: Option<u64>,
    confirmation_deadline: Option<Instant>,
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
    confirmation_lifetime: Duration,
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
            confirmation_lifetime: CONFIRMATION_LIFETIME,
        }
    }

    #[doc(hidden)]
    pub fn with_confirmation_lifetime(
        accounts: impl IntoIterator<Item = (String, u64)>,
        confirmation_lifetime: Duration,
    ) -> Self {
        let mut accounting = Self::new(accounts);
        accounting.confirmation_lifetime = confirmation_lifetime;
        accounting
    }

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
        refund_expired_confirmations(&mut state)?;
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
                confirmation_deadline: None,
                token_expires_at,
            },
        );
        Ok(ReserveResult::Reserved)
    }

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
        submission.outcome = Outcome::AwaitingConfirmation;
        submission.pending_charge = Some(charged);
        submission.confirmation_deadline = Some(Instant::now() + self.confirmation_lifetime);
        Ok(charged)
    }

    pub fn settle(&self, submission_id: [u8; 32]) -> Result<u64, AccountingError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| AccountingError::InvalidTransition)?;
        refund_expired_confirmations(&mut state)?;
        let submission = state
            .submissions
            .get(&submission_id)
            .ok_or(AccountingError::InvalidTransition)?;
        if let Outcome::Settled { charged } = submission.outcome {
            return Ok(charged);
        }
        if submission.outcome != Outcome::AwaitingConfirmation {
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
        submission.confirmation_deadline = None;
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
            Outcome::InFlight | Outcome::AwaitingConfirmation => {}
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

    pub fn available(&self, account_id: &str) -> Option<u64> {
        let mut state = self.state.lock().ok()?;
        refund_expired_confirmations(&mut state).ok()?;
        state.accounts.get(account_id).map(|a| a.available)
    }
}

fn is_terminal(outcome: &Outcome) -> bool {
    matches!(outcome, Outcome::Settled { .. } | Outcome::Refunded)
}

fn refund_expired_confirmations(state: &mut State) -> Result<(), AccountingError> {
    let now = Instant::now();
    let expired: Vec<_> = state
        .submissions
        .iter()
        .filter_map(|(id, submission)| {
            (submission.outcome == Outcome::AwaitingConfirmation
                && submission
                    .confirmation_deadline
                    .is_some_and(|deadline| deadline <= now))
            .then_some(*id)
        })
        .collect();
    for id in expired {
        let submission = state
            .submissions
            .get(&id)
            .ok_or(AccountingError::InvalidTransition)?;
        let account = state
            .accounts
            .get_mut(&submission.account_id)
            .ok_or(AccountingError::InvalidTransition)?;
        account.available = account
            .available
            .checked_add(submission.quote.reserved_microunits)
            .ok_or(AccountingError::Cost)?;
        account.in_flight = account
            .in_flight
            .checked_sub(1)
            .ok_or(AccountingError::InvalidTransition)?;
        let submission = state
            .submissions
            .get_mut(&id)
            .ok_or(AccountingError::InvalidTransition)?;
        submission.outcome = Outcome::Refunded;
        submission.pending_charge = None;
        submission.confirmation_deadline = None;
    }
    Ok(())
}

fn map_cost(_: CatalogError) -> AccountingError {
    AccountingError::Cost
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Model;

    fn quote() -> Quote {
        Quote {
            model: Model {
                id: "m".into(),
                context_tokens: 2,
                max_output_tokens: 1,
                input_microunits_per_token: 1,
                output_microunits_per_token: 1,
            },
            input_tokens: 1,
            reserved_microunits: 3,
        }
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
                        confirmation_deadline: None,
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
            confirmation_lifetime: CONFIRMATION_LIFETIME,
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
