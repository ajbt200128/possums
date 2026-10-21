//! Submission claims are local; only intake can confirm a production monetary hold.
use crate::catalog::{cached_cost, Quote};
use std::{sync::Arc, time::Instant};
use thiserror::Error;

mod remote;
mod synthetic;
#[cfg(test)]
pub(crate) use remote::tests as remote_fixture;
pub(crate) use remote::Attempt;
const MAX_SUBMISSIONS: usize = 100_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// A local attempt is owned, not evidence of an intake hold.
    InFlight,
    Settled {
        charged: u64,
    },
    Refunded,
    Expired,
    Unknown,
}

/// Validated final or last trustworthy cumulative usage, never a sum of snapshots.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FinalUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

/// Synthetic fixture state only. Remote accounting provides no balance facade.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BalanceSnapshot {
    pub available_microunits: u64,
    pub in_flight: u32,
    pub completed_requests: u64,
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum AccountingError {
    #[error("account unavailable")]
    UnknownAccount,
    #[error("insufficient credit")]
    InsufficientCredit,
    #[error("account concurrency limit reached")]
    Concurrency,
    #[error("submission token already used for different input")]
    AlteredDuplicate,
    #[error("submission state capacity reached")]
    Capacity,
    #[error("invalid accounting transition")]
    InvalidTransition,
    #[error("code=ACCOUNTING_STATE_UNAVAILABLE stage=accounting constraint=owned_state_evidence. Local accounting state is unavailable; underlying cause unknown. Do not infer a charge or refund. No replay occurred; ask the operator to check privately. A deliberate new submission may incur another charge.")]
    StateUnavailable,
    #[error("cost cannot be represented")]
    Cost,
    #[error("{0}")]
    Intake(crate::intake::Error),
    #[error("code=INTAKE_HOLD_UNUSABLE stage=admission constraint=active_unexpired_hold. Intake did not confirm an active, unexpired hold at initial admission. No prompt was sent; do not infer a charge or refund. No replay occurred; check intake availability privately before a deliberate new submission.")]
    HoldUnavailable,
}

impl AccountingError {
    pub(crate) fn interface_error(self) -> crate::failures::InterfaceError {
        use crate::failures::InterfaceError as E;
        use crate::intake::ErrorKind as K;
        match self {
            Self::UnknownAccount => E::AccountUnavailable,
            Self::InsufficientCredit => E::CreditRefused,
            Self::Concurrency => E::AccountLimitRefused,
            Self::AlteredDuplicate => E::ConflictRefused,
            Self::Capacity => E::StateCapacityRefused,
            Self::Cost => E::QuoteOverflow,
            Self::Intake(error) => match error.kind {
                K::CreditInsufficient => E::CreditRefused,
                K::AccountUnavailable | K::AdmissionDisabled => E::AccountUnavailable,
                K::Conflict => E::ConflictRefused,
                _ => E::AccountingStateUnknown,
            },
            _ => E::AccountingStateUnknown,
        }
    }

    pub(crate) fn reserve_uncertain(self) -> bool {
        match self {
            Self::Intake(error) => {
                error.dispatch_attempted
                    && !matches!(
                        error.kind,
                        crate::intake::ErrorKind::CreditInsufficient
                            | crate::intake::ErrorKind::AccountUnavailable
                            | crate::intake::ErrorKind::AdmissionDisabled
                            | crate::intake::ErrorKind::Conflict
                    )
            }
            Self::StateUnavailable | Self::InvalidTransition => true,
            _ => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReserveResult {
    /// Confirmed by the synchronous synthetic fixture backend only.
    Reserved,
    /// Local submission slot only; not a confirmed monetary hold.
    Claimed,
    Duplicate(Outcome),
}

enum Backend {
    Synthetic(synthetic::Accounting),
    Remote(remote::Remote),
}
pub struct Accounting(Backend);
impl Accounting {
    /// Explicit synthetic fixture constructor; never a production fallback.
    pub fn new(accounts: impl IntoIterator<Item = (String, u64)>) -> Self {
        Self(Backend::Synthetic(synthetic::Accounting::new(accounts)))
    }
    pub(crate) fn from_intake(intake: Arc<crate::intake::Intake>) -> Self {
        Self(Backend::Remote(remote::Remote::new(intake)))
    }
    pub(crate) fn is_remote(&self) -> bool {
        matches!(self.0, Backend::Remote(_))
    }

    /// Innermost admission lock only. No network work or callback into auth.
    pub fn reserve(
        &self,
        account: &str,
        id: [u8; 32],
        binding: [u8; 32],
        quote: Quote,
        expiry: Instant,
    ) -> Result<ReserveResult, AccountingError> {
        match &self.0 {
            Backend::Synthetic(local) => local.reserve(account, id, binding, quote, expiry),
            Backend::Remote(remote) => remote.claim(account, id, binding, quote, expiry),
        }
    }
    pub(crate) fn take_attempt(&self, id: [u8; 32]) -> Result<Attempt, AccountingError> {
        match &self.0 {
            Backend::Remote(remote) => remote.take_attempt(id),
            _ => Err(AccountingError::InvalidTransition),
        }
    }
    pub(crate) fn record_remote(
        &self,
        id: [u8; 32],
        outcome: Outcome,
    ) -> Result<(), AccountingError> {
        match &self.0 {
            Backend::Remote(remote) => remote.record(id, outcome),
            _ => Err(AccountingError::InvalidTransition),
        }
    }
    /// Synchronous terminal operations are exclusively for the synthetic backend.
    pub fn finish(
        &self,
        id: [u8; 32],
        usage: Option<FinalUsage>,
    ) -> Result<Outcome, AccountingError> {
        match &self.0 {
            Backend::Synthetic(local) => local.finish(id, usage),
            _ => Err(AccountingError::InvalidTransition),
        }
    }
    pub(crate) fn finish_cached(
        &self,
        id: [u8; 32],
        usage: Option<FinalUsage>,
        cached: u64,
    ) -> Result<Outcome, AccountingError> {
        match &self.0 {
            Backend::Synthetic(local) => local.finish_cached(id, usage, cached),
            _ => Err(AccountingError::InvalidTransition),
        }
    }
    pub fn snapshot(&self, account: &str) -> Result<BalanceSnapshot, AccountingError> {
        match &self.0 {
            Backend::Synthetic(local) => local.snapshot(account),
            _ => Err(AccountingError::InvalidTransition),
        }
    }
    pub fn available(&self, account: &str) -> Option<u64> {
        match &self.0 {
            Backend::Synthetic(local) => local.available(account),
            _ => None,
        }
    }
    #[cfg(test)]
    pub(crate) fn hold_test_lock(&self) -> impl Drop + '_ {
        match &self.0 {
            Backend::Synthetic(local) => local.hold_test_lock(),
            _ => panic!("synthetic accounting fixture required"),
        }
    }
}

fn charge(quote: &Quote, usage: Option<FinalUsage>, cached: u64) -> Option<u64> {
    let usage = usage?;
    if usage.input_tokens.checked_add(usage.output_tokens) != Some(usage.total_tokens) {
        return None;
    }
    cached_cost(
        usage.input_tokens,
        usage.output_tokens,
        cached,
        quote.model.input_microunits_per_million_tokens,
        quote.model.output_microunits_per_million_tokens,
        quote
            .model
            .cached_input_microunits_per_million_tokens
            .unwrap_or(quote.model.input_microunits_per_million_tokens),
    )
    .ok()
    .map(|amount| amount.min(quote.reserved_microunits))
}

impl From<crate::intake::TerminalOutcome> for Outcome {
    fn from(outcome: crate::intake::TerminalOutcome) -> Self {
        match outcome {
            crate::intake::TerminalOutcome::Settled { charged } => Self::Settled { charged },
            crate::intake::TerminalOutcome::Refunded => Self::Refunded,
            crate::intake::TerminalOutcome::Expired => Self::Expired,
        }
    }
}
