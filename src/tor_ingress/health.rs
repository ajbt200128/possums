//! Request-only, closed projection of the existing operational status stream.
//! No SDK error, identifier, timestamp, history or diagnostic work is retained.
use std::sync::atomic::{AtomicU16, Ordering};

use serde::Serialize;
use tor_hsservice::status::{DescUploadRetryError, Problem, State};

const READY: u16 = 1 << 8;
const PROBLEM_SHIFT: u32 = 4;
const FIELD_MASK: u16 = 0xf;
const STREAM_ENDED: u16 = 8;
const STREAM_INTERRUPTED: u16 = 9;
const UNKNOWN_STATE: u16 = 10;

const STATES: &[&str] = &[
    "not_observed",
    "shutdown",
    "bootstrapping",
    "degraded_reachable",
    "degraded_unreachable",
    "running",
    "recovering",
    "broken",
    "status_stream_ended",
    "status_stream_interrupted",
    "unknown",
];

#[derive(Clone, Copy, Default)]
#[repr(u16)]
pub(super) enum ProblemKind {
    #[default]
    NoneObserved,
    Runtime,
    DescriptorUpload,
    IntroductionPoint,
    DescriptorFatal,
    DescriptorRetryExhausted,
    DescriptorTimeout,
    DescriptorInternal,
    Unknown,
}

impl ProblemKind {
    pub(super) fn from_sdk(problem: Option<&Problem>) -> Self {
        match problem {
            None => Self::NoneObserved,
            Some(Problem::Runtime(_)) => Self::Runtime,
            Some(Problem::DescriptorUpload(errors)) => {
                // The SDK may supply multiple failures. Select one closed category,
                // with internal/fatal failures before timeouts/retry exhaustion.
                let mut kind = Self::DescriptorUpload;
                for error in errors {
                    let next = match error {
                        DescUploadRetryError::Bug(_) => Self::DescriptorInternal,
                        DescUploadRetryError::FatalError(_) => Self::DescriptorFatal,
                        DescUploadRetryError::Timeout(_) => Self::DescriptorTimeout,
                        DescUploadRetryError::MaxRetryCountExceeded(_) => {
                            Self::DescriptorRetryExhausted
                        }
                        _ => Self::Unknown,
                    };
                    if priority(next) > priority(kind) {
                        kind = next;
                    }
                }
                kind
            }
            Some(Problem::Ipt(_)) => Self::IntroductionPoint,
            Some(_) => Self::Unknown,
        }
    }

    pub(super) fn is_fatal(self) -> bool {
        matches!(self, Self::Runtime)
    }
}

fn priority(problem: ProblemKind) -> u8 {
    match problem {
        ProblemKind::DescriptorInternal => 5,
        ProblemKind::DescriptorFatal => 4,
        ProblemKind::Unknown => 3,
        ProblemKind::DescriptorTimeout => 2,
        ProblemKind::DescriptorRetryExhausted => 1,
        _ => 0,
    }
}

const PROBLEMS: &[&str] = &[
    "none_observed",
    "runtime",
    "descriptor_upload",
    "introduction_point",
    "descriptor_fatal",
    "descriptor_retry_exhausted",
    "descriptor_timeout",
    "descriptor_internal",
    "unknown",
];

#[derive(Clone, Copy)]
pub(super) struct EngineStatus {
    pub(super) state: State,
    pub(super) problem: ProblemKind,
}

impl EngineStatus {
    #[cfg(test)]
    pub(super) fn test((state, fatal): (State, bool)) -> Self {
        Self {
            state,
            problem: if fatal {
                ProblemKind::Runtime
            } else {
                ProblemKind::NoneObserved
            },
        }
    }
}

/// One coherent word keeps readiness and its explanation from different events
/// from being combined. Only the existing engine owner writes it.
#[derive(Default)]
pub(crate) struct TorReadiness(AtomicU16);

impl TorReadiness {
    pub(super) fn observe(&self, status: EngineStatus) {
        let state = match status.state {
            State::Shutdown => 1,
            State::Bootstrapping => 2,
            State::DegradedReachable => 3,
            State::DegradedUnreachable => 4,
            State::Running => 5,
            State::Recovering => 6,
            State::Broken => 7,
            _ => UNKNOWN_STATE,
        };
        let ready = status.state.is_fully_reachable() && !status.problem.is_fatal();
        self.0.store(
            state | ((status.problem as u16) << PROBLEM_SHIFT) | if ready { READY } else { 0 },
            Ordering::Release,
        );
    }

    pub(super) fn ended(&self, interrupted: bool) {
        let problem = self.0.load(Ordering::Relaxed) & (FIELD_MASK << PROBLEM_SHIFT);
        let state = if interrupted {
            STREAM_INTERRUPTED
        } else {
            STREAM_ENDED
        };
        // Preserve the last observed problem across a later stream failure.
        self.0.store(state | problem, Ordering::Release);
    }

    #[cfg(test)]
    pub(crate) fn mark_running(&self) {
        self.observe(EngineStatus::test((State::Running, false)));
    }

    pub(crate) fn is_ready(&self) -> bool {
        self.0.load(Ordering::Acquire) & READY != 0
    }

    pub(crate) fn diagnostic(&self, lifecycle_serving: bool) -> ReadinessDiagnostic {
        let snapshot = self.0.load(Ordering::Acquire);
        let ready = snapshot & READY != 0;
        let state = snapshot & FIELD_MASK;
        let (code, stage, constraint, action) = if !ready {
            match state {
                0 => ("tor_status_not_observed", "tor_status_stream", "no_observed_sdk_status", "Check gateway startup and Tor initialization; no diagnostic probe or retry was initiated."),
                STREAM_ENDED => ("tor_status_stream_ended", "tor_status_stream", "stream_ended_cause_unknown", "Check gateway lifecycle and Tor service; the underlying cause is unknown. Do not replay inference."),
                STREAM_INTERRUPTED => ("tor_status_stream_interrupted", "tor_status_stream", "stream_poll_unwound_cause_unknown", "Check gateway lifecycle and Tor service; the underlying cause is unknown. Do not replay inference."),
                UNKNOWN_STATE => ("tor_state_unknown", "tor_status_stream", "sdk_state_unknown", "Check the gateway SDK version and Tor service; the underlying cause is unknown. Do not replay inference."),
                _ => ("tor_reachability_unconfirmed", "tor_service", "local_sdk_readiness_not_confirmed", "Inspect the closed SDK state/problem and platform lifecycle; independently check Tor routing. Do not replay inference."),
            }
        } else if !lifecycle_serving {
            (
                "gateway_not_serving",
                "gateway_lifecycle",
                "serving_not_confirmed",
                "Check gateway startup or shutdown status; do not replay inference.",
            )
        } else {
            ("gateway_ready", "readiness", "local_readiness_only", "Independently verify endpoint authenticity and reachability before private requests.")
        };
        ReadinessDiagnostic {
            code,
            stage,
            constraint,
            action,
            lifecycle_serving,
            ready: ready && lifecycle_serving,
            tor_state: STATES.get(state as usize).copied().unwrap_or("unknown"),
            tor_problem: PROBLEMS
                .get(((snapshot >> PROBLEM_SHIFT) & FIELD_MASK) as usize)
                .copied()
                .unwrap_or("unknown"),
        }
    }
}

#[derive(Serialize)]
pub(crate) struct ReadinessDiagnostic {
    pub(crate) code: &'static str,
    stage: &'static str,
    constraint: &'static str,
    action: &'static str,
    lifecycle_serving: bool,
    pub(crate) ready: bool,
    tor_state: &'static str,
    tor_problem: &'static str,
}

#[cfg(test)]
mod tests;
