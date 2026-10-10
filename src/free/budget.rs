//! Operator-controlled process-run soft budget. No reservations, markup or reset timer.
use crate::{catalog::Model, inference::stream::StreamUsage};

pub(crate) const THRESHOLD: u64 = 10_000_000;

#[derive(Default)]
pub(crate) struct Budget {
    pub(crate) settled: u64,
    pub(crate) blocked: bool,
}

impl Budget {
    pub(crate) fn admits(&self) -> bool {
        !self.blocked && self.settled < THRESHOLD
    }

    pub(crate) fn settle(&mut self, model: &Model, usage: StreamUsage) -> Result<u64, ()> {
        let result = (|| {
            if self.blocked
                || usage.input_tokens.checked_add(usage.output_tokens) != Some(usage.total_tokens)
                || usage.total_tokens > model.context_tokens
                || usage.output_tokens > model.max_output_tokens
            {
                return Err(());
            }
            let input = u128::from(usage.input_tokens)
                .checked_mul(u128::from(model.input_microunits_per_million_tokens))
                .ok_or(())?;
            let output = u128::from(usage.output_tokens)
                .checked_mul(u128::from(model.output_microunits_per_million_tokens))
                .ok_or(())?;
            let cost = input
                .checked_add(output)
                .and_then(|v| v.checked_add(999_999))
                .ok_or(())?
                / 1_000_000;
            let cost = u64::try_from(cost).map_err(|_| ())?;
            self.settled = self.settled.checked_add(cost).ok_or(())?;
            Ok(cost)
        })();
        if result.is_err() {
            self.blocked = true;
        }
        result
    }
}
