//! Fixed numeric token aggregates for validated terminal usage observations.
//! No evidence objects, identifiers, metadata, clocks or dynamic labels enter.
use super::labels::QualifiedModel;

/// Reviewed qualified model partitions (`kimi-k3`, `glm-5-3`, `other`).
const MODELS: usize = 3;
/// Supported signed-integer wire limit; values above it are withheld, not clamped.
const MAX_TOKENS: u64 = i64::MAX as u64;

/// Fixed per-model input/output/cached sums with independent family validity.
/// Empty populations are absent; a populated zero is a measured zero.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct TokenTotals {
    input: [u64; MODELS],
    output: [u64; MODELS],
    cached: [u64; MODELS],
    /// A model has at least one valid input/output observation in this window.
    input_output_populated: [bool; MODELS],
    /// A model has at least one valid known-cache observation in this window.
    cached_populated: [bool; MODELS],
    /// Input/output family integrity; false withholds both for the window.
    input_output_valid: bool,
    /// Whole-window cache availability; false if any usage-bearing generation
    /// lacked trustworthy cache counts or cache arithmetic faulted.
    cache_available: bool,
}
impl Default for TokenTotals {
    fn default() -> Self {
        Self {
            input: [0; MODELS],
            output: [0; MODELS],
            cached: [0; MODELS],
            input_output_populated: [false; MODELS],
            cached_populated: [false; MODELS],
            input_output_valid: true,
            cache_available: true,
        }
    }
}
impl TokenTotals {
    /// One terminal observation of already-authenticated validated usage.
    /// `input` includes cache, `output` includes reasoning, `cached` is a
    /// subset of `input` and is never added to it again. `None` cache withholds
    /// the whole-window cache family while preserving valid input/output.
    pub(super) fn observe(
        &mut self,
        model: QualifiedModel,
        input: u64,
        output: u64,
        total: u64,
        cached: Option<u64>,
    ) {
        let index = model.0 as usize;
        if index >= MODELS
            || input > MAX_TOKENS
            || output > MAX_TOKENS
            || total > MAX_TOKENS
            || cached.is_some_and(|c| c > MAX_TOKENS)
            || input.checked_add(output) != Some(total)
            || cached.is_some_and(|c| c > input)
        {
            self.invalidate_input_output();
            return;
        }
        let (Some(next_input), Some(next_output)) = (
            self.input[index].checked_add(input),
            self.output[index].checked_add(output),
        ) else {
            self.invalidate_input_output();
            return;
        };
        let input_sum = projected_sum(&self.input, index, next_input);
        let output_sum = projected_sum(&self.output, index, next_output);
        if input_sum.is_none_or(|sum| sum > MAX_TOKENS)
            || output_sum.is_none_or(|sum| sum > MAX_TOKENS)
        {
            self.invalidate_input_output();
            return;
        }
        self.input[index] = next_input;
        self.output[index] = next_output;
        self.input_output_populated[index] = true;
        match cached {
            Some(count) => {
                let Some(next_cached) = self.cached[index].checked_add(count) else {
                    self.invalidate_input_output();
                    return;
                };
                if projected_sum(&self.cached, index, next_cached)
                    .is_none_or(|sum| sum > MAX_TOKENS)
                {
                    self.invalidate_input_output();
                    return;
                }
                self.cached[index] = next_cached;
                self.cached_populated[index] = true;
            }
            None => self.cache_available = false,
        }
    }
    fn invalidate_input_output(&mut self) {
        self.input_output_valid = false;
        self.cache_available = false;
    }
    /// Valid, populated input sum for one partition; otherwise unavailable.
    pub(super) fn input(&self, model: QualifiedModel) -> Option<u64> {
        self.partition(&self.input, &self.input_output_populated, model)
            .filter(|_| self.input_output_valid)
    }
    /// Valid, populated output sum for one partition; otherwise unavailable.
    pub(super) fn output(&self, model: QualifiedModel) -> Option<u64> {
        self.partition(&self.output, &self.input_output_populated, model)
            .filter(|_| self.input_output_valid)
    }
    /// Populated known-cache subset; unavailable for the window if any
    /// usage-bearing generation lacked trustworthy cache counts.
    pub(super) fn cached(&self, model: QualifiedModel) -> Option<u64> {
        self.partition(&self.cached, &self.cached_populated, model)
            .filter(|_| self.cache_available)
    }
    fn partition(
        &self,
        cells: &[u64; MODELS],
        populated: &[bool; MODELS],
        model: QualifiedModel,
    ) -> Option<u64> {
        let index = model.0 as usize;
        populated
            .get(index)
            .copied()
            .unwrap_or(false)
            .then(|| cells[index])
    }
}
/// Across-model sum with one cell projected, checked for overflow.
fn projected_sum(cells: &[u64; MODELS], index: usize, value: u64) -> Option<u64> {
    cells.iter().enumerate().try_fold(0_u64, |acc, (i, cell)| {
        acc.checked_add(if i == index { value } else { *cell })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const KIMI: QualifiedModel = QualifiedModel(0);
    const GLM: QualifiedModel = QualifiedModel(1);
    const OTHER: QualifiedModel = QualifiedModel(2);
    const PARTITIONS: [QualifiedModel; MODELS] = [KIMI, GLM, OTHER];

    fn total(t: &TokenTotals, metric: fn(&TokenTotals, QualifiedModel) -> Option<u64>) -> u64 {
        PARTITIONS.iter().filter_map(|m| metric(t, *m)).sum()
    }

    #[test]
    fn three_partitions_and_summed_totals() {
        let mut t = TokenTotals::default();
        t.observe(KIMI, 100, 20, 120, Some(30));
        t.observe(GLM, 50, 10, 60, Some(0));
        t.observe(OTHER, 7, 3, 10, Some(2));
        assert_eq!(t.input(KIMI), Some(100));
        assert_eq!(t.output(KIMI), Some(20));
        assert_eq!(t.cached(KIMI), Some(30));
        assert_eq!(t.input(GLM), Some(50));
        assert_eq!(t.output(GLM), Some(10));
        assert_eq!(t.cached(GLM), Some(0));
        assert_eq!(t.input(OTHER), Some(7));
        assert_eq!(t.output(OTHER), Some(3));
        assert_eq!(t.cached(OTHER), Some(2));
        assert_eq!(total(&t, TokenTotals::input), 157);
        assert_eq!(total(&t, TokenTotals::output), 33);
        assert_eq!(total(&t, TokenTotals::cached), 32);
    }

    #[test]
    fn known_zeros_are_measured_absent_is_unavailable() {
        let mut t = TokenTotals::default();
        t.observe(KIMI, 0, 0, 0, Some(0));
        assert_eq!(t.input(KIMI), Some(0));
        assert_eq!(t.output(KIMI), Some(0));
        assert_eq!(t.cached(KIMI), Some(0));
        assert_eq!(t.input(GLM), None);
        assert_eq!(t.output(GLM), None);
        assert_eq!(t.cached(GLM), None);
    }

    #[test]
    fn unknown_cache_withholds_whole_window_but_preserves_input_output() {
        let mut t = TokenTotals::default();
        t.observe(KIMI, 10, 1, 11, Some(4));
        t.observe(GLM, 20, 2, 22, None);
        assert!(t.input_output_valid);
        assert!(!t.cache_available);
        assert_eq!(t.input(KIMI), Some(10));
        assert_eq!(t.output(KIMI), Some(1));
        assert_eq!(t.input(GLM), Some(20));
        assert_eq!(t.output(GLM), Some(2));
        assert_eq!(t.cached(KIMI), None);
        assert_eq!(t.cached(GLM), None);
    }

    #[test]
    fn cached_is_a_subset_and_reasoning_is_not_added_twice() {
        let mut t = TokenTotals::default();
        // cached == input, output already includes reasoning.
        t.observe(KIMI, 100, 40, 140, Some(100));
        assert_eq!(t.input(KIMI), Some(100));
        assert_eq!(t.cached(KIMI), Some(100));
        assert_eq!(t.output(KIMI), Some(40));
    }

    #[test]
    fn invalid_arithmetic_range_and_overflow_withhold_families() {
        let mut mismatch = TokenTotals::default();
        mismatch.observe(KIMI, 1, 1, 3, Some(0));
        assert!(!mismatch.input_output_valid);
        assert_eq!(mismatch.input(KIMI), None);
        assert_eq!(mismatch.output(KIMI), None);
        assert_eq!(mismatch.cached(KIMI), None);

        let mut oversized_cache = TokenTotals::default();
        oversized_cache.observe(KIMI, 1, 1, 2, Some(2));
        assert!(!oversized_cache.input_output_valid);
        assert_eq!(oversized_cache.input(KIMI), None);

        let too_large = i64::MAX as u64 + 1;
        let mut range = TokenTotals::default();
        range.observe(KIMI, too_large, 0, too_large, Some(0));
        assert!(!range.input_output_valid);
        assert_eq!(range.input(KIMI), None);

        // Across-model sum overflow: each cell is representable, the sum is not.
        let mut sum = TokenTotals::default();
        sum.observe(KIMI, MAX_TOKENS, 0, MAX_TOKENS, Some(0));
        sum.observe(GLM, 1, 0, 1, Some(1));
        assert!(!sum.input_output_valid);
        assert_eq!(sum.input(KIMI), None);
        assert_eq!(sum.input(GLM), None);
        assert_eq!(sum.cached(KIMI), None);

        // Cache cell overflow withholds the input/output family as well.
        let mut cache = TokenTotals::default();
        cache.observe(KIMI, MAX_TOKENS, 0, MAX_TOKENS, Some(MAX_TOKENS));
        cache.observe(GLM, 1, 0, 1, Some(1));
        assert!(!cache.input_output_valid);
        assert_eq!(cache.input(KIMI), None);
        assert_eq!(cache.cached(KIMI), None);
    }

    #[test]
    fn out_of_contract_partition_is_unavailable_without_panicking() {
        let mut t = TokenTotals::default();
        let invalid = QualifiedModel(3);
        assert_eq!(t.input(invalid), None);
        assert_eq!(t.output(invalid), None);
        assert_eq!(t.cached(invalid), None);
        t.observe(invalid, 0, 0, 0, Some(0));
        assert_eq!(t.input(KIMI), None);
        assert!(!t.input_output_valid);
    }

    #[test]
    fn default_is_a_clean_reset() {
        let mut t = TokenTotals::default();
        t.observe(KIMI, 5, 5, 10, Some(5));
        assert_eq!(t.input(KIMI), Some(5));
        t = TokenTotals::default();
        assert_eq!(t.input(KIMI), None);
        assert_eq!(t.output(KIMI), None);
        assert_eq!(t.cached(KIMI), None);
        assert!(t.input_output_valid);
        assert!(t.cache_available);
    }
}
