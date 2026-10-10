//! Content-free, per-request runtime metadata; not part of the static index source.
use super::{budget::THRESHOLD, Diagnostic, Stage};
use crate::inference::InferenceFailure;
use serde::Serialize;
use std::sync::{
    atomic::{AtomicU64, AtomicUsize, Ordering},
    Arc,
};

const INFO_BYTES: usize = 2048;
const REVISION: &str = match option_env!("POSSUMS_SOURCE_REVISION") {
    Some(value) => value,
    None => "unavailable",
};
const VERSION: &str = match option_env!("POSSUMS_VERSION") {
    Some(value) => value,
    None => "unavailable",
};

#[derive(Default)]
pub(super) struct RuntimeInfo {
    live: AtomicUsize,
    pub(super) spent: AtomicU64,
}

#[derive(Serialize)]
pub(super) struct Info {
    live_connections: usize,
    source: Option<String>,
    version: &'static str,
    limit_usd: String,
    // Known settled generation costs, not upstream credit or a zero-cost claim
    // for an operation whose charge could not be authenticated.
    spent_usd: String,
    limit_period: &'static str,
}

pub(super) fn source(revision: &str) -> Option<String> {
    (revision.len() == 40
        && revision.bytes().all(|b| b.is_ascii_hexdigit())
        && revision.bytes().any(|b| b != b'0'))
    .then(|| format!("https://github.com/ajbt200128/possums/tree/{revision}"))
}

/// Production startup requires build provenance; unit tests can use unavailable.
pub fn build_identity_available() -> bool {
    source(REVISION).is_some() && version(VERSION) != "unavailable"
}

fn version(value: &str) -> &str {
    if !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-_".contains(&b))
    {
        value
    } else {
        "unavailable"
    }
}

fn usd(micros: u64) -> String {
    format!("{}.{:06}", micros / 1_000_000, micros % 1_000_000)
}

impl RuntimeInfo {
    pub(super) fn connected(self: &Arc<Self>) -> LiveConnection {
        self.live.fetch_add(1, Ordering::SeqCst);
        LiveConnection(self.clone())
    }
    pub(super) fn snapshot(&self) -> Info {
        Info {
            live_connections: self.live.load(Ordering::SeqCst),
            source: source(REVISION),
            version: version(VERSION),
            limit_usd: usd(THRESHOLD),
            spent_usd: usd(self.spent.load(Ordering::SeqCst)),
            limit_period: "run",
        }
    }
}

impl Info {
    pub(super) fn bytes(&self) -> Result<Vec<u8>, Diagnostic> {
        crate::bounded_json::to_vec_pretty(self, INFO_BYTES).map_err(|_| {
            Diagnostic::upstream(
                InferenceFailure::RequestEncodingFailed,
                Stage::Admission,
                false,
            )
        })
    }
    pub(super) fn append_to(&self, index: &str) -> Result<String, Diagnostic> {
        let bytes = self.bytes()?;
        let mut prefix = String::with_capacity(index.len() + 1 + bytes.len());
        prefix.push_str(index);
        prefix.push('\n');
        // The serializer emits UTF-8; conversion cannot echo raw input/errors.
        prefix.push_str(std::str::from_utf8(&bytes).map_err(|_| {
            Diagnostic::upstream(
                InferenceFailure::RequestEncodingFailed,
                Stage::Admission,
                false,
            )
        })?);
        Ok(prefix)
    }
}

// This owner is held ONLY by the downstream HTTP connection, never by a job,
// upload lease or retained frame. Detached upstream work is not a live socket.
pub(super) struct LiveConnection(Arc<RuntimeInfo>);
impl Drop for LiveConnection {
    fn drop(&mut self) {
        self.0.live.fetch_sub(1, Ordering::SeqCst);
    }
}
