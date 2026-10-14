//! Fixed storage, linked request integrity and independently qualified extensions.
use super::{errors::ErrorTotals, labels::*, tokens::TokenTotals};
use crate::failures::InterfaceError;

// Qualified observed runtime coverage; exclusions and evidence are in docs/verification.md.
const INTERFACE_ERRORS_QUALIFIED: bool = true;
const OBSERVED_TOKENS_QUALIFIED: bool = true;
#[cfg(test)]
pub(super) const COMPILED_EXTENSION_QUALIFICATION: [bool; 2] =
    [INTERFACE_ERRORS_QUALIFIED, OBSERVED_TOKENS_QUALIFIED];
pub const EXTENSION_POINTS: usize = InterfaceError::ALL.len() + 3 * 3;

pub const REQUEST_SERIES: usize = 8 + 144 + 144 + 3 + 36 + 36 + 36 + 9 + 630 + 5;
pub const REQUEST_CELLS: usize = 8 + 144 + 1584 + 3 + 36 + 396 + 396 + 9 + 630 + 50;

/// Typed aggregate bins only. Counts are derived, never separately mutable.
#[derive(Debug, PartialEq, Eq)]
pub struct Histogram<const N: usize>(pub(super) [u64; N]);
impl<const N: usize> Histogram<N> {
    pub fn bins(&self) -> &[u64; N] {
        &self.0
    }
    pub fn count(&self) -> Option<u64> {
        sum(&self.0)
    }
}

/// Index products are frozen in telemetry-schema.md. No cumulative snapshots.
/// Only active scalars and nonempty distributions are output; zero bins within
/// those distributions are genuine zeros. The arrays themselves stay local.
#[derive(Debug, PartialEq, Eq)]
pub struct RequestTables {
    pub(super) http_starts: [u64; 8],
    pub(super) http_completed: [u64; 144],
    pub(super) http_duration: [Histogram<11>; 144],
    pub(super) generation_starts: [u64; 3],
    pub(super) generation_completed: [u64; 36],
    pub(super) generation_duration: [Histogram<11>; 36],
    pub(super) first_output: [Histogram<11>; 36],
    pub(super) delivery: [u64; 9],
    pub(super) rejected: [u64; 630],
    pub(super) occupancy: [Histogram<10>; 5],
    pub(super) dispositions: [[u64; 5]; 144],
    pub(super) contributors: [u64; 5],
    pub(super) valid: bool,
    pub(super) window_valid: bool,
    pub(super) ticks: u16,
    pub(super) errors: ErrorTotals,
    pub(super) tokens: TokenTotals,
    #[cfg(test)]
    pub(super) extension_qualification: Option<[bool; 2]>,
}
impl Default for RequestTables {
    fn default() -> Self {
        Self {
            http_starts: [0; 8],
            http_completed: [0; 144],
            http_duration: std::array::from_fn(|_| Histogram([0; 11])),
            generation_starts: [0; 3],
            generation_completed: [0; 36],
            generation_duration: std::array::from_fn(|_| Histogram([0; 11])),
            first_output: std::array::from_fn(|_| Histogram([0; 11])),
            delivery: [0; 9],
            rejected: [0; 630],
            occupancy: std::array::from_fn(|_| Histogram([0; 10])),
            dispositions: [[0; 5]; 144],
            contributors: [0; 5],
            valid: true,
            window_valid: true,
            ticks: 0,
            errors: ErrorTotals::default(),
            tokens: TokenTotals::default(),
            #[cfg(test)]
            extension_qualification: Some([false; 2]),
        }
    }
}
fn sum(values: &[u64]) -> Option<u64> {
    values.iter().try_fold(0_u64, |a, b| a.checked_add(*b))
}
pub(super) fn increment(cell: &mut u64, valid: &mut bool) {
    if let Some(next) = cell.checked_add(1) {
        *cell = next;
    } else {
        *valid = false;
    }
}
pub(super) fn http_index(e: Endpoint, s: Status, t: HttpTerminal) -> usize {
    (e as usize * 6 + s as usize) * 3 + t as usize
}
pub(super) fn generation_index(e: Endpoint, m: QualifiedModel) -> Option<usize> {
    Some(e.chat()? * 3 + m.0 as usize)
}
impl RequestTables {
    pub(super) fn errors_qualified(&self) -> bool {
        #[cfg(test)]
        if let Some(gates) = self.extension_qualification {
            return gates[0];
        }
        INTERFACE_ERRORS_QUALIFIED
    }
    pub(super) fn tokens_qualified(&self) -> bool {
        #[cfg(test)]
        if let Some(gates) = self.extension_qualification {
            return gates[1];
        }
        OBSERVED_TOKENS_QUALIFIED
    }
    pub(super) fn complete(&self) -> bool {
        self.window_valid && self.ticks == 300
    }
    pub(super) fn releasable(&self) -> bool {
        self.complete()
            && (self.request_families_releasable()
                || (self.errors_qualified()
                    && InterfaceError::ALL
                        .iter()
                        .any(|e| self.errors.get(*e).is_some()))
                || (self.tokens_qualified()
                    && (0..3).any(|m| self.tokens.input(QualifiedModel(m)).is_some())))
    }
    pub(super) fn request_families_releasable(&self) -> bool {
        if !self.valid || !self.complete() {
            return false;
        }
        // Complete aggregates have no minimum population. The linked family
        // still fails closed on arithmetic or lifecycle inconsistency.
        for i in 0..144 {
            if self.http_duration[i].count() != Some(self.http_completed[i])
                || sum(&self.dispositions[i]) != Some(self.http_completed[i])
            {
                return false;
            }
        }
        for i in 0..36 {
            let Some(output) = self.first_output[i].count() else {
                return false;
            };
            if output > self.generation_completed[i]
                || self.generation_duration[i].count() != Some(self.generation_completed[i])
            {
                return false;
            }
        }
        for i in 0..5 {
            if self.contributors[i] > 0 && self.occupancy[i].count() != Some(300) {
                return false;
            }
        }
        self.http_starts
            .iter()
            .chain(&self.http_completed)
            .chain(&self.generation_starts)
            .chain(&self.generation_completed)
            .chain(&self.delivery)
            .chain(&self.rejected)
            .chain(&self.contributors)
            .any(|n| *n != 0)
    }
    pub fn http_started(&self, e: Endpoint) -> Option<u64> {
        present(self.http_starts[e as usize])
    }
    pub fn http_terminal(&self, e: Endpoint, s: Status, t: HttpTerminal) -> Option<u64> {
        present(self.http_completed[http_index(e, s, t)])
    }
    pub fn http_duration(&self, e: Endpoint, s: Status, t: HttpTerminal) -> Option<&Histogram<11>> {
        histogram(&self.http_duration[http_index(e, s, t)])
    }
    pub fn generation_started(&self, e: Endpoint, m: QualifiedModel) -> Option<u64> {
        present(self.generation_starts[generation_index(e, m)?])
    }
    pub fn generation_terminal(
        &self,
        e: Endpoint,
        m: QualifiedModel,
        t: GenerationTerminal,
    ) -> Option<u64> {
        present(self.generation_completed[generation_index(e, m)? * 12 + t as usize])
    }
    pub fn generation_duration(
        &self,
        e: Endpoint,
        m: QualifiedModel,
        t: GenerationTerminal,
    ) -> Option<&Histogram<11>> {
        histogram(&self.generation_duration[generation_index(e, m)? * 12 + t as usize])
    }
    pub fn first_output(
        &self,
        e: Endpoint,
        m: QualifiedModel,
        t: GenerationTerminal,
    ) -> Option<&Histogram<11>> {
        histogram(&self.first_output[generation_index(e, m)? * 12 + t as usize])
    }
    pub fn delivery(&self, e: Endpoint, m: QualifiedModel, t: DeliveryTerminal) -> Option<u64> {
        present(self.delivery[generation_index(e, m)? * 3 + t as usize])
    }
    pub fn rejected(&self, e: Option<Endpoint>, m: AdmissionModel, r: Rejection) -> Option<u64> {
        present(self.rejected[(e.map_or(8, |e| e as usize) * 5 + m.index()) * 14 + r as usize])
    }
    pub fn occupancy(&self, lane: Lane) -> Option<&Histogram<10>> {
        (self.contributors[lane as usize] != 0).then_some(&self.occupancy[lane as usize])
    }
    pub fn series_count(&self) -> usize {
        self.http_starts
            .iter()
            .chain(&self.http_completed)
            .chain(&self.generation_starts)
            .chain(&self.generation_completed)
            .chain(&self.delivery)
            .chain(&self.rejected)
            .filter(|n| **n > 0)
            .count()
            + self
                .http_duration
                .iter()
                .chain(&self.generation_duration)
                .chain(&self.first_output)
                .filter(|h| h.0.iter().any(|n| *n > 0))
                .count()
            + self.contributors.iter().filter(|n| **n > 0).count()
    }
}
fn present(n: u64) -> Option<u64> {
    (n != 0).then_some(n)
}
fn histogram<const N: usize>(h: &Histogram<N>) -> Option<&Histogram<N>> {
    h.0.iter().any(|n| *n > 0).then_some(h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_fixture_gates_override_compiled_qualification_exactly() {
        let mut table = RequestTables::default();
        assert_eq!(table.extension_qualification, Some([false; 2]));
        for gates in [[true, false], [false, true], [false; 2], [true; 2]] {
            table.extension_qualification = Some(gates);
            assert_eq!(table.errors_qualified(), gates[0]);
            assert_eq!(table.tokens_qualified(), gates[1]);
            table.ticks = 300;
            assert!(!table.releasable()); // No fabricated inactive populations.
        }
        table.extension_qualification = None;
        assert_eq!(table.errors_qualified(), INTERFACE_ERRORS_QUALIFIED);
        assert_eq!(table.tokens_qualified(), OBSERVED_TOKENS_QUALIFIED);
        assert!(!table.releasable());
    }
}
