//! Process CPU/RSS and fixed application admission limits, not enclave/cgroup
//! allocation or model GPU usage. Per PRIVACY.md: numeric sources only; errors
//! are discarded, and configured capacity never means currently free permits.
use super::{AggregateMetrics, Clock, Lane, ResourceMetric, ResourceScope, SECOND};
use std::time::{Duration, Instant};

const INTERVAL: u64 = 10 * SECOND;

struct Reading {
    cpu: Option<Duration>,
    at: Instant,
    rss: Option<usize>,
}
impl Reading {
    fn now() -> Self {
        // The Linux dependency feature always_use_statm avoids smaps entirely.
        let rss = memory_stats::memory_stats().map(|stats| stats.physical_mem);
        let cpu = cpu_time::ProcessTime::try_now()
            .ok()
            .map(|t| t.as_duration());
        Self {
            cpu,
            at: Instant::now(),
            rss,
        }
    }
}

pub(super) struct Sampler {
    capacities: [(Lane, u64); 5],
    last: Option<u64>,
    previous: Option<(Instant, Duration)>,
}
impl Sampler {
    pub(super) fn new(capacities: [(Lane, u64); 5]) -> Self {
        Self {
            capacities,
            last: None,
            previous: None,
        }
    }

    pub(super) fn sample<C: Clock>(&mut self, metrics: &AggregateMetrics<C>) {
        self.sample_with(metrics, Reading::now);
    }

    fn sample_with<C: Clock>(
        &mut self,
        metrics: &AggregateMetrics<C>,
        read: impl FnOnce() -> Reading,
    ) {
        let mapped = {
            let Some(mut state) = metrics.lock() else {
                self.previous = None;
                return;
            };
            let Some((_, mapped)) = metrics.time(&mut state) else {
                self.previous = None;
                return;
            };
            mapped
        };
        // All source I/O is outside the aggregation lock, on the sender only.
        let Some((end, cpu, rss)) = self.read(mapped, read) else {
            return;
        };
        if !metrics.infrastructure_sample(end, |table, interval| {
            for &(lane, capacity) in &self.capacities {
                table.configuration(lane, interval, capacity);
            }
            for (metric, value) in [
                (ResourceMetric::CpuUsed, cpu),
                (ResourceMetric::MemoryUsed, rss),
            ] {
                // Mark failure unavailable; never manufacture a zero or allow
                // a later duplicate reading to fill this interval's hole.
                table.observe(
                    ResourceScope::Process,
                    metric,
                    interval,
                    value.unwrap_or(f64::NAN),
                );
            }
        }) {
            // A slow source read or lock failure cannot seed a later rate.
            self.previous = None;
        }
    }

    fn read(
        &mut self,
        mapped: u64,
        read: impl FnOnce() -> Reading,
    ) -> Option<(u64, Option<f64>, Option<f64>)> {
        let end = mapped / INTERVAL * INTERVAL;
        if self.last.is_some_and(|last| end <= last) {
            // Duplicate ticks don't read or revise. Regression loses baseline.
            if self.last.is_some_and(|last| end < last) {
                self.previous = None;
            }
            return None;
        }
        let consecutive = self.last.and_then(|last| last.checked_add(INTERVAL)) == Some(end);
        self.last = Some(end);
        if mapped - end > SECOND {
            self.previous = None;
            return None;
        }
        let reading = read();
        let cpu = self.previous.filter(|_| consecutive).and_then(|(at, cpu)| {
            let elapsed = reading.at.checked_duration_since(at)?.as_secs_f64();
            let used = reading.cpu?.checked_sub(cpu)?.as_secs_f64();
            let cores = used / elapsed;
            (elapsed > 0. && elapsed.is_finite() && cores.is_finite()).then_some(cores)
        });
        self.previous = reading.cpu.map(|cpu| (reading.at, cpu));
        Some((end, cpu, reading.rss.map(|rss| rss as f64)))
    }
}

#[cfg(test)]
mod tests;
