//! Numeric aggregation fixtures only, NOT a resource reader or unit normalizer.
use super::labels::{Lane, CAPACITIES};
pub const INFRASTRUCTURE_SERIES: usize = 19;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ResourceScope {
    TinfoilEnclave,
    TinfoilWorkload,
    Process,
    Cgroup,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceMetric {
    CpuUsed,
    MemoryUsed,
    CpuCapacity,
    MemoryCapacity,
}
fn index(scope: ResourceScope, metric: ResourceMetric) -> Option<usize> {
    let s = scope as usize;
    match metric {
        ResourceMetric::CpuUsed => Some(s),
        ResourceMetric::MemoryUsed => Some(4 + s),
        ResourceMetric::CpuCapacity if s != 2 => Some(8 + if s == 3 { 2 } else { s }),
        ResourceMetric::MemoryCapacity if s != 2 => Some(11 + if s == 3 { 2 } else { s }),
        _ => None,
    }
}
#[derive(Debug, PartialEq)]
pub struct Infrastructure {
    samples: [[f64; 6]; 19],
    seen: [u8; 19],
    invalid: [bool; 19],
}
impl Default for Infrastructure {
    fn default() -> Self {
        Self {
            samples: [[0.; 6]; 19],
            seen: [0; 19],
            invalid: [false; 19],
        }
    }
}
impl Infrastructure {
    pub(super) fn observe(
        &mut self,
        scope: ResourceScope,
        metric: ResourceMetric,
        interval: usize,
        value: f64,
    ) {
        if let Some(i) = index(scope, metric) {
            self.insert(i, interval, value);
        }
    }
    pub(super) fn configuration(&mut self, lane: Lane, interval: usize, capacity: u64) {
        let i = lane as usize;
        if capacity != CAPACITIES[i] {
            self.invalid[14 + i] = true;
        }
        self.insert(14 + i, interval, capacity as f64);
    }
    fn insert(&mut self, i: usize, interval: usize, value: f64) {
        if !value.is_finite() || value < 0. || interval >= 6 {
            self.invalid[i] = true;
            return;
        }
        let bit = 1 << interval;
        if self.seen[i] & bit != 0 && self.samples[i][interval] != value {
            self.invalid[i] = true;
        }
        self.samples[i][interval] = value;
        self.seen[i] |= bit;
    }
    fn value(&self, i: usize) -> Option<f64> {
        if self.invalid[i] || self.seen[i] != 63 {
            return None;
        }
        if i >= 8 && self.samples[i].iter().any(|v| *v != self.samples[i][0]) {
            return None;
        }
        // Arithmetic overflow makes the point unavailable, never a healthy zero.
        let mean = self.samples[i].iter().sum::<f64>() / 6.;
        mean.is_finite().then_some(mean)
    }
    pub fn resource(&self, scope: ResourceScope, metric: ResourceMetric) -> Option<f64> {
        self.value(index(scope, metric)?)
    }
    pub fn capacity(&self, lane: Lane) -> Option<f64> {
        self.value(14 + lane as usize)
    }
    pub fn series_count(&self) -> usize {
        (0..19).filter(|i| self.value(*i).is_some()).count()
    }
}
