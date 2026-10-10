use crate::catalog::{marked_up_cost, Quote};
use std::{collections::HashMap, sync::Mutex, time::Instant};
use thiserror::Error;

const MAX_SUBMISSIONS: usize = 100_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    InFlight,
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
    token_expires_at: Instant,
}

#[derive(Clone, Debug)]
struct Account {
    available: u64,
    in_flight: u32,
    completed_requests: u64,
}

/// One atomic view of transient first-party accounting state, not telemetry.
/// Completions include both settlements (even zero charges) and refunds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BalanceSnapshot {
    pub available_microunits: u64,
    pub in_flight: u32,
    pub completed_requests: u64,
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
                        completed_requests: 0,
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
    /// `finish` releases this lock before callers issue continuations.
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
                token_expires_at,
            },
        );
        Ok(ReserveResult::Reserved)
    }

    /// The sole terminal operation: finish a streaming reservation independently of receipt.
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
        let completed_requests = account
            .completed_requests
            .checked_add(1)
            .ok_or(AccountingError::InvalidTransition)?;
        // Commit all fields and the absorbing outcome only after every check.
        account.available = available;
        account.in_flight = in_flight;
        account.completed_requests = completed_requests;
        submission.outcome = outcome.clone();
        Ok(outcome)
    }

    #[cfg(test)]
    pub(crate) fn hold_test_lock(&self) -> impl Drop + '_ {
        self.state.lock().unwrap()
    }

    /// All fields come from the same mutex acquisition; never assemble this
    /// view from separate balance/concurrency reads during reconciliation.
    pub fn snapshot(&self, account_id: &str) -> Result<BalanceSnapshot, AccountingError> {
        let state = self
            .state
            .lock()
            .map_err(|_| AccountingError::InvalidTransition)?;
        let account = state
            .accounts
            .get(account_id)
            .ok_or(AccountingError::UnknownAccount)?;
        Ok(BalanceSnapshot {
            available_microunits: account.available,
            in_flight: account.in_flight,
            completed_requests: account.completed_requests,
        })
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
                token_expires_at,
            } = &state.submissions[&[1; 32]];
            assert_eq!(account_id, "a");
            assert_eq!(*request_digest, binding);
            assert_eq!(*quote, original);
            assert_eq!(*outcome, Outcome::Settled { charged: 3 });
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
    fn completion_counter_overflow_does_not_partially_commit_a_terminal_transition() {
        for usage in [
            None,
            Some(FinalUsage {
                input_tokens: 1,
                output_tokens: 0,
                total_tokens: 1,
            }),
            Some(FinalUsage {
                input_tokens: 0,
                output_tokens: 0,
                total_tokens: 0,
            }),
        ] {
            let ledger = Accounting::new([("a".into(), 100)]);
            let expiry = Instant::now() + Duration::from_secs(60);
            ledger
                .reserve("a", [1; 32], [2; 32], quote(), expiry)
                .unwrap();
            ledger
                .state
                .lock()
                .unwrap()
                .accounts
                .get_mut("a")
                .unwrap()
                .completed_requests = u64::MAX;
            let before = ledger.snapshot("a").unwrap();
            for _ in 0..2 {
                assert_eq!(
                    ledger.finish([1; 32], usage),
                    Err(AccountingError::InvalidTransition)
                );
                assert_eq!(ledger.snapshot("a").unwrap(), before);
                assert_eq!(
                    ledger.state.lock().unwrap().submissions[&[1; 32]].outcome,
                    Outcome::InFlight
                );
            }
            // The last representable completion succeeds; later conflicting
            // terminal calls are still absorbing even at the counter maximum.
            ledger
                .state
                .lock()
                .unwrap()
                .accounts
                .get_mut("a")
                .unwrap()
                .completed_requests -= 1;
            let outcome = ledger.finish([1; 32], usage).unwrap();
            assert_eq!(ledger.snapshot("a").unwrap().completed_requests, u64::MAX);
            assert_eq!(ledger.snapshot("a").unwrap().in_flight, 0);
            assert_eq!(ledger.finish([1; 32], None).unwrap(), outcome);
        }
    }

    #[test]
    fn snapshot_unknown_account_and_poison_fail_closed() {
        let ledger = Accounting::new([("a".into(), 100)]);
        assert_eq!(
            ledger.snapshot("missing"),
            Err(AccountingError::UnknownAccount)
        );
        assert_eq!(ledger.available("missing"), None);
        let _ = std::panic::catch_unwind(|| {
            let _guard = ledger.state.lock().unwrap();
            panic!("synthetic lock poison");
        });
        assert_eq!(
            ledger.snapshot("a"),
            Err(AccountingError::InvalidTransition)
        );
        assert_eq!(ledger.available("a"), None);
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
                        completed_requests: 0,
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
