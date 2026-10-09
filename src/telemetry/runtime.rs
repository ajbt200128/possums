//! Startup-owned, best-effort direct export, independent of application owners.
use super::{
    export::{handoff, transport::Client},
    AggregateMetrics, Clock, Deployment, Lane, SystemClock,
};
use http::HeaderValue;
use std::{fmt, sync::Arc, time::Duration};
use tokio::{sync::watch, task::JoinHandle};

const ENDPOINT: &str = "https://api.honeycomb.io";

/// Only these four environment variables are read. No general OTEL discovery.
/// No Debug representation can expose a credential or rejected input.
pub struct Config {
    credential: Option<HeaderValue>,
}
impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TelemetryConfig")
            .field("enabled", &self.enabled())
            .finish()
    }
}
impl Config {
    pub fn from_env() -> Self {
        Self::from_lookup(|name| match std::env::var(name) {
            Ok(value) => Some(value),
            Err(std::env::VarError::NotPresent) => None,
            Err(_) => Some("\0".into()),
        })
    }

    /// Pure configuration parsing; useful to embedding owners and local tests.
    pub fn from_lookup(mut get: impl FnMut(&str) -> Option<String>) -> Self {
        let disabled = Self { credential: None };
        match get("OTEL_SDK_DISABLED").as_deref() {
            None | Some("false") => {}
            // Unknown values fail closed, including non-Unicode from_env below.
            _ => return disabled,
        }
        if !matches!(
            get("OTEL_EXPORTER_OTLP_ENDPOINT").as_deref(),
            Some(ENDPOINT) | Some("https://api.honeycomb.io/")
        ) {
            return disabled;
        }
        if !matches!(
            get("OTEL_SERVICE_NAME").as_deref(),
            None | Some("possums-gateway")
        ) {
            return disabled;
        }
        let Some(headers) = get("OTEL_EXPORTER_OTLP_HEADERS") else {
            return disabled;
        };
        // Exactly one transport-only ingestion header. No arbitrary headers,
        // duplicates, percent decoding, whitespace, commas or control characters.
        let Some(key) = headers.strip_prefix("x-honeycomb-team=") else {
            return disabled;
        };
        if key.is_empty()
            || key.len() > 256
            || !key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return disabled;
        }
        let Ok(mut credential) = HeaderValue::from_str(key) else {
            return disabled;
        };
        credential.set_sensitive(true);
        Self {
            credential: Some(credential),
        }
    }
    pub fn enabled(&self) -> bool {
        self.credential.is_some()
    }
}

/// Owns the sender, not inference or accounting. Explicit shutdown joins actual
/// disposal; Drop latches stop and aborts as a fallback, never claims a join.
pub struct Runtime {
    metrics: Option<Arc<AggregateMetrics>>,
    stop: watch::Sender<bool>,
    task: Option<JoinHandle<bool>>,
}
impl Runtime {
    pub fn start(config: Config, capacities: [(Lane, u64); 6]) -> Self {
        let (stop, stopped) = watch::channel(false);
        let mut shared = None;
        let task = config.credential.and_then(|credential| {
            let client = Client::honeycomb(credential).ok()?;
            // Configuration enables only the reviewed aggregate schema. No
            // request can bypass its sparse-family or field allowlist checks.
            let metrics = Arc::new(AggregateMetrics::new(
                Deployment::Production,
                SystemClock::default(),
            ));
            shared = Some(metrics.clone());
            Some(tokio::spawn(sender(metrics, client, stopped, capacities)))
        });
        Self {
            metrics: shared,
            stop,
            task,
        }
    }
    pub fn metrics(&self) -> Option<Arc<AggregateMetrics>> {
        self.metrics.clone()
    }
    pub fn enabled(&self) -> bool {
        self.task.is_some()
    }

    /// True acknowledges joined disposal and cleared local state, not backend
    /// deletion or recall of bytes already accepted by the transport.
    pub async fn shutdown(mut self) -> bool {
        self.stop.send_replace(true);
        match self.task.take() {
            Some(task) => task.await.unwrap_or(false),
            None => true,
        }
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        self.stop.send_replace(true);
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

async fn sender<C: Clock + 'static>(
    metrics: Arc<AggregateMetrics<C>>,
    template: Client,
    mut stopped: watch::Receiver<bool>,
    capacities: [(Lane, u64); 6],
) -> bool {
    let mut process = super::process::Sampler::new(capacities);
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    'running: loop {
        tokio::select! {
            biased;
            _ = stopped.wait_for(|stop| *stop) => break,
            _ = tick.tick() => {},
        }
        // A lost observation/window must not silently disable configured
        // telemetry forever. Rewarm with retained attempted-window watermarks;
        // never replay an old batch or change an inference lifecycle.
        if metrics.epoch.load(std::sync::atomic::Ordering::SeqCst) % 2 == 0 {
            metrics.enable(Deployment::Production);
        }
        process.sample(&metrics);
        metrics.poll();
        while let Some(permit) = metrics.take_window() {
            // Fresh attempt state; never retry the consumed window. Connection
            // and payload ownership live inside this future, not a driver task.
            let attempt = handoff::send(
                permit,
                template.fresh(),
                #[cfg(test)]
                Default::default(),
            );
            tokio::pin!(attempt);
            // Network progress must not suspend complete-window sampling.
            loop {
                tokio::select! {
                    biased;
                    _ = stopped.wait_for(|stop| *stop) => break 'running,
                    _ = tick.tick() => {
                        process.sample(&metrics);
                        metrics.poll();
                    },
                    _ = &mut attempt => break,
                }
            }
        }
        if *stopped.borrow() {
            break;
        }
    }
    // The attempt has been dropped before this acknowledgement. No flush.
    metrics.stop_export().await
}

#[cfg(test)]
pub(in crate::telemetry) mod tests;
