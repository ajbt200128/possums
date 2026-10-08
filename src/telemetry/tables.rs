//! Fixed storage and the single linked-family release predicate.
use super::labels::*;

pub const REQUEST_SERIES: usize = 16 + 288 + 288 + 6 + 72 + 72 + 72 + 18 + 1190 + 6;
pub const REQUEST_CELLS: usize = 16 + 288 + 3168 + 6 + 72 + 792 + 792 + 18 + 1190 + 60;

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
    pub(super) http_starts: [u64; 16],
    pub(super) http_completed: [u64; 288],
    pub(super) http_duration: [Histogram<11>; 288],
    pub(super) generation_starts: [u64; 6],
    pub(super) generation_completed: [u64; 72],
    pub(super) generation_duration: [Histogram<11>; 72],
    pub(super) first_output: [Histogram<11>; 72],
    pub(super) delivery: [u64; 18],
    pub(super) rejected: [u64; 1190],
    pub(super) occupancy: [Histogram<10>; 6],
    pub(super) dispositions: [[u64; 5]; 288],
    pub(super) contributors: [u64; 6],
    pub(super) valid: bool,
    pub(super) ticks: u16,
}
impl Default for RequestTables {
    fn default() -> Self {
        Self {
            http_starts: [0; 16],
            http_completed: [0; 288],
            http_duration: std::array::from_fn(|_| Histogram([0; 11])),
            generation_starts: [0; 6],
            generation_completed: [0; 72],
            generation_duration: std::array::from_fn(|_| Histogram([0; 11])),
            first_output: std::array::from_fn(|_| Histogram([0; 11])),
            delivery: [0; 18],
            rejected: [0; 1190],
            occupancy: std::array::from_fn(|_| Histogram([0; 10])),
            dispositions: [[0; 5]; 288],
            contributors: [0; 6],
            valid: true,
            ticks: 0,
        }
    }
}
fn sparse(values: &[u64]) -> bool {
    values.iter().any(|n| (1..10).contains(n))
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
    pub(super) fn releasable(&self) -> bool {
        if !self.valid || self.ticks != 300 {
            return false;
        }
        for cells in [
            self.http_starts.as_slice(),
            &self.http_completed,
            &self.generation_starts,
            &self.generation_completed,
            &self.delivery,
            &self.rejected,
            &self.contributors,
        ] {
            if sparse(cells) {
                return false;
            }
        }
        for i in 0..288 {
            if sparse(&self.http_duration[i].0)
                || sparse(&self.dispositions[i])
                || self.http_duration[i].count() != Some(self.http_completed[i])
                || sum(&self.dispositions[i]) != Some(self.http_completed[i])
            {
                return false;
            }
        }
        for i in 0..72 {
            let Some(output) = self.first_output[i].count() else {
                return false;
            };
            let Some(missing) = self.generation_completed[i].checked_sub(output) else {
                return false;
            };
            if sparse(&self.generation_duration[i].0)
                || sparse(&self.first_output[i].0)
                || sparse(&[missing])
                || self.generation_duration[i].count() != Some(self.generation_completed[i])
            {
                return false;
            }
        }
        for i in 0..6 {
            if self.contributors[i] > 0
                && (sparse(&self.occupancy[i].0) || self.occupancy[i].count() != Some(300))
            {
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
        present(self.rejected[(e.map_or(16, |e| e as usize) * 5 + m.index()) * 14 + r as usize])
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
