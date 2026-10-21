//! Bounded, prompt-free duplicate suppression, not a shadow ledger.
use super::*;
use rand::RngCore;
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

pub(super) struct Remote {
    intake: Arc<crate::intake::Intake>,
    claims: Mutex<HashMap<[u8; 32], Claim>>,
    #[cfg(test)]
    service: Option<Arc<tests::Service>>,
}
struct Claim {
    account: String,
    binding: [u8; 32],
    expiry: Instant,
    outcome: Outcome,
    attempt: Option<Attempt>,
}
// No Debug/Clone: only the sole actor can take this immutable dispatch payload.
pub(crate) struct Attempt {
    pub(crate) intake: Arc<crate::intake::Intake>,
    pub(crate) account: String,
    pub(crate) quote: Quote,
    pub(crate) key: String,
    #[cfg(test)]
    service: Option<Arc<tests::Service>>,
}
impl Attempt {
    pub(crate) fn charge(&self, usage: Option<FinalUsage>, cached: u64) -> Option<u64> {
        charge(&self.quote, usage, cached)
    }
    pub(crate) async fn reserve(&self) -> Result<crate::intake::Reservation, AccountingError> {
        #[cfg(test)]
        if let Some(service) = &self.service {
            return service
                .reserve(&self.key, self.quote.reserved_microunits)
                .await;
        }
        self.intake
            .reserve(
                &self.account,
                self.quote.reserved_microunits,
                1800,
                &self.key,
            )
            .await
            .map_err(AccountingError::Intake)
    }
    pub(crate) async fn renew(
        &self,
        reservation: &crate::intake::Reservation,
    ) -> Result<crate::intake::RenewalReply, AccountingError> {
        #[cfg(test)]
        if let Some(service) = &self.service {
            return service.renew(reservation).await;
        }
        self.intake
            .renew(reservation)
            .await
            .map_err(AccountingError::Intake)
    }
    pub(crate) async fn finish(
        &self,
        reservation: &crate::intake::Reservation,
        charge: Option<u64>,
    ) -> Result<crate::intake::TerminalReceipt, AccountingError> {
        #[cfg(test)]
        if let Some(service) = &self.service {
            return service.finish(charge).await;
        }
        self.intake
            .finish(reservation, charge)
            .await
            .map_err(AccountingError::Intake)
    }
}
impl Remote {
    pub(super) fn new(intake: Arc<crate::intake::Intake>) -> Self {
        Self {
            intake,
            claims: Mutex::default(),
            #[cfg(test)]
            service: None,
        }
    }
    pub(super) fn claim(
        &self,
        account: &str,
        id: [u8; 32],
        binding: [u8; 32],
        quote: Quote,
        expiry: Instant,
    ) -> Result<ReserveResult, AccountingError> {
        let mut claims = self
            .claims
            .lock()
            .map_err(|_| AccountingError::StateUnavailable)?;
        let now = Instant::now();
        claims.retain(|_, claim| claim.outcome == Outcome::InFlight || claim.expiry > now);
        if expiry <= now {
            return Err(AccountingError::InvalidTransition);
        }
        if let Some(claim) = claims.get(&id) {
            if claim.account != account || claim.binding != binding {
                return Err(AccountingError::AlteredDuplicate);
            }
            return Ok(ReserveResult::Duplicate(claim.outcome.clone()));
        }
        if claims.len() >= MAX_SUBMISSIONS {
            return Err(AccountingError::Capacity);
        }
        let key = attempt_key()?;
        claims.insert(
            id,
            Claim {
                account: account.to_owned(),
                binding,
                expiry,
                outcome: Outcome::InFlight,
                attempt: Some(Attempt {
                    intake: self.intake.clone(),
                    account: account.to_owned(),
                    quote,
                    key,
                    #[cfg(test)]
                    service: self.service.clone(),
                }),
            },
        );
        Ok(ReserveResult::Claimed)
    }
    pub(super) fn take_attempt(&self, id: [u8; 32]) -> Result<Attempt, AccountingError> {
        self.claims
            .lock()
            .map_err(|_| AccountingError::StateUnavailable)?
            .get_mut(&id)
            .and_then(|claim| claim.attempt.take())
            .ok_or(AccountingError::InvalidTransition)
    }
    pub(super) fn record(&self, id: [u8; 32], outcome: Outcome) -> Result<(), AccountingError> {
        let mut claims = self
            .claims
            .lock()
            .map_err(|_| AccountingError::StateUnavailable)?;
        let claim = claims
            .get_mut(&id)
            .ok_or(AccountingError::InvalidTransition)?;
        if claim.outcome == Outcome::InFlight {
            claim.outcome = outcome;
            claim.attempt = None;
        }
        Ok(())
    }
}
#[cfg(test)]
#[path = "remote_tests.rs"]
pub(crate) mod tests;

fn attempt_key() -> Result<String, AccountingError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| AccountingError::StateUnavailable)?
        .as_millis();
    if !(1_000_000_000_000..=9_999_999_999_999).contains(&millis) {
        return Err(AccountingError::StateUnavailable);
    }
    let mut random = [0u8; 16];
    rand::rng().fill_bytes(&mut random);
    let suffix: String = random
        .into_iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok(format!("v1.{millis}.{suffix}"))
}
