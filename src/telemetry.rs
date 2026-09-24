use std::{collections::HashMap, sync::Mutex};

const MIN_EXPORT_COUNT: u64 = 10;
const ALLOWED_METRICS: [&str; 4] = [
    "requests_total",
    "responses_2xx_total",
    "responses_4xx_total",
    "responses_5xx_total",
];

#[derive(Default)]
pub struct AggregateMetrics {
    enabled: bool,
    counters: Mutex<HashMap<&'static str, u64>>,
}

impl AggregateMetrics {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            counters: Mutex::new(HashMap::new()),
        }
    }

    pub fn increment(&self, name: &'static str) {
        if !self.enabled || !ALLOWED_METRICS.contains(&name) {
            return;
        }
        if let Ok(mut counters) = self.counters.lock() {
            let counter = counters.entry(name).or_default();
            *counter = counter.saturating_add(1);
        }
    }

    pub fn exportable_snapshot(&self) -> Vec<(&'static str, u64)> {
        if !self.enabled {
            return Vec::new();
        }
        self.counters
            .lock()
            .map(|counters| {
                counters
                    .iter()
                    .filter_map(|(&name, &count)| {
                        (count >= MIN_EXPORT_COUNT).then_some((name, count))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}
