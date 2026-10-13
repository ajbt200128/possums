//! Fixed error-type aggregates for typed interface failures.
//! No clocks, identifiers, strings, metadata, SDK calls or raw exception access.
use crate::failures::InterfaceError;

/// Reviewed closed projection width, reused from the frozen enum rather than
/// duplicated as a magic number.
const ERROR_TYPES: usize = InterfaceError::ALL.len();
/// Supported signed-integer wire limit; values above it are withheld, not clamped.
const MAX_ERRORS: u64 = i64::MAX as u64;

/// Fixed per-error-type counts with whole-family validity.
/// Empty cells are absent; a populated zero is not reachable because a cell is
/// only populated by an increment. Overflow makes the entire family unavailable
/// until reset; no clamp, wrap or mutation of unrelated families/accounting.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct ErrorTotals {
    counts: [u64; ERROR_TYPES],
    /// Whole-family integrity; false withholds every cell for the window.
    valid: bool,
}
impl Default for ErrorTotals {
    fn default() -> Self {
        Self {
            counts: [0; ERROR_TYPES],
            valid: true,
        }
    }
}
impl ErrorTotals {
    /// One terminal observation of an already-classified interface failure.
    /// Increments exactly once per call; the caller owns once-only/terminal
    /// semantics. Per-cell or across-type sum overflow withholds the whole
    /// family rather than publishing a partial sum.
    pub(super) fn observe(&mut self, error: InterfaceError) {
        if !self.valid {
            return;
        }
        let index = error as usize;
        let Some(next) = self.counts[index].checked_add(1) else {
            self.valid = false;
            return;
        };
        if projected_sum(&self.counts, index, next).is_none_or(|sum| sum > MAX_ERRORS) {
            self.valid = false;
            return;
        }
        self.counts[index] = next;
    }
    #[cfg(test)]
    pub(super) fn with_counts(counts: [u64; ERROR_TYPES]) -> Self {
        Self {
            counts,
            valid: true,
        }
    }
    /// Valid, populated count for one error type; otherwise unavailable.
    /// Absent/empty zero cells return `None`, never a healthy zero.
    pub(super) fn get(&self, error: InterfaceError) -> Option<u64> {
        let count = self.counts[error as usize];
        (self.valid && count != 0).then_some(count)
    }
}
/// Across-error-type sum with one cell projected, checked for overflow.
fn projected_sum(cells: &[u64; ERROR_TYPES], index: usize, value: u64) -> Option<u64> {
    cells.iter().enumerate().try_fold(0_u64, |acc, (i, cell)| {
        acc.checked_add(if i == index { value } else { *cell })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn total(t: &ErrorTotals) -> u64 {
        InterfaceError::ALL
            .into_iter()
            .filter_map(|e| t.get(e))
            .sum()
    }

    #[test]
    fn every_fixed_category_round_trips() {
        let mut t = ErrorTotals::default();
        for error in InterfaceError::ALL {
            t.observe(error);
        }
        for error in InterfaceError::ALL {
            assert_eq!(t.get(error), Some(1), "{}", error.label());
        }
        assert_eq!(total(&t), ERROR_TYPES as u64);
    }

    #[test]
    fn counts_reflect_repeated_observations_per_category() {
        let mut t = ErrorTotals::default();
        for _ in 0..3 {
            t.observe(InterfaceError::AuthenticationRefused);
        }
        t.observe(InterfaceError::SettlementFailed);
        assert_eq!(t.get(InterfaceError::AuthenticationRefused), Some(3));
        assert_eq!(t.get(InterfaceError::SettlementFailed), Some(1));
        assert_eq!(total(&t), 4);
    }

    #[test]
    fn absent_cells_are_unavailable_not_healthy_zero() {
        let mut t = ErrorTotals::default();
        for error in InterfaceError::ALL {
            assert_eq!(t.get(error), None, "{}", error.label());
        }
        t.observe(InterfaceError::InputRefused);
        assert_eq!(t.get(InterfaceError::InputRefused), Some(1));
        assert_eq!(t.get(InterfaceError::RouteRefused), None);
    }

    #[test]
    fn per_cell_overflow_withholds_whole_family() {
        let mut t = ErrorTotals::default();
        t.observe(InterfaceError::BodyLimit);
        assert_eq!(t.get(InterfaceError::BodyLimit), Some(1));
        // Synthetic direct table overflow without billions of calls.
        let index = InterfaceError::BodyLimit as usize;
        t.counts[index] = MAX_ERRORS;
        t.observe(InterfaceError::BodyLimit);
        assert!(!t.valid);
        assert_eq!(t.get(InterfaceError::BodyLimit), None);
        assert_eq!(t.get(InterfaceError::InputRefused), None);
        assert_eq!(total(&t), 0);
    }

    #[test]
    fn across_type_sum_overflow_withholds_whole_family() {
        let mut t = ErrorTotals::default();
        t.observe(InterfaceError::BodyLimit);
        t.observe(InterfaceError::HeaderLimit);
        // Two representable cells whose sum exceeds the wire limit.
        t.counts[InterfaceError::BodyLimit as usize] = MAX_ERRORS;
        t.counts[InterfaceError::HeaderLimit as usize] = 1;
        t.observe(InterfaceError::BodyDeadline);
        assert!(!t.valid);
        assert_eq!(t.get(InterfaceError::BodyLimit), None);
        assert_eq!(t.get(InterfaceError::HeaderLimit), None);
        assert_eq!(t.get(InterfaceError::BodyDeadline), None);
    }

    #[test]
    fn invalidity_is_sticky_until_reset() {
        let mut t = ErrorTotals::default();
        t.counts[InterfaceError::InputRefused as usize] = MAX_ERRORS;
        t.observe(InterfaceError::InputRefused);
        assert!(!t.valid);
        // A later valid observation cannot un-invalidate the window.
        t.observe(InterfaceError::RouteRefused);
        assert!(!t.valid);
        assert_eq!(t.get(InterfaceError::RouteRefused), None);
    }

    #[test]
    fn default_is_a_clean_reset() {
        let mut t = ErrorTotals::default();
        t.observe(InterfaceError::InputRefused);
        t.counts[InterfaceError::RouteRefused as usize] = MAX_ERRORS;
        t.observe(InterfaceError::RouteRefused);
        assert!(!t.valid);
        t = ErrorTotals::default();
        assert!(t.valid);
        assert_eq!(t.get(InterfaceError::InputRefused), None);
        assert_eq!(t.get(InterfaceError::RouteRefused), None);
    }
}
