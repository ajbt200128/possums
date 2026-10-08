//! Application-only typed hooks. No content, identifiers, or raw errors enter here.
use super::*;
use std::sync::Arc;
use tokio::sync::OwnedSemaphorePermit;

/// A marker on the SAME shared permit owner used by uploads and retained frames.
/// No additional acquisition, and no dependence of permit release on telemetry.
pub struct Lease {
    // Retire the marker before returning the slot to a possible new owner.
    _observation: Option<OwnedObservation>,
    generation: std::sync::OnceLock<(Arc<AggregateMetrics>, u64)>,
    _permit: OwnedSemaphorePermit,
}
impl Lease {
    // Non-owning signal to the ORIGINAL terminal observation. It cannot finish,
    // release, or extend that record; late calls see an already-retired key.
    pub(crate) fn observe_generation(&self, observation: &OwnedObservation) {
        let _ = self
            .generation
            .set((observation.owner.clone(), observation.key));
    }
    /// Inference adapters call immediately before generation send, AFTER encoding
    /// and trust acquisition. Never for tokenizer calls or renderer startup.
    pub fn dispatch_generation(&self) {
        if let Some((metrics, key)) = self.generation.get() {
            metrics.update(*key, Update::Dispatch);
        }
    }

    pub(crate) fn observed(
        permit: OwnedSemaphorePermit,
        metrics: Option<&Arc<AggregateMetrics>>,
        lane: Lane,
    ) -> Self {
        Self {
            _permit: permit,
            _observation: metrics.map(|m| m.lease(lane).into_owned(m)),
            generation: Default::default(),
        }
    }
}
impl From<OwnedSemaphorePermit> for Lease {
    fn from(permit: OwnedSemaphorePermit) -> Self {
        Self {
            _permit: permit,
            _observation: None,
            generation: Default::default(),
        }
    }
}

/// Non-owning capability: cloning this cannot retain or finish the HTTP lifetime.
/// It contains only an aggregator and pool key; never a request or account.
#[derive(Clone, Default)]
pub(crate) struct RequestContext {
    parent: Option<(Arc<AggregateMetrics>, u64)>,
}
impl RequestContext {
    pub(crate) fn disposition(&self, disposition: Disposition) {
        if let Some((owner, key)) = &self.parent {
            owner.update(*key, Update::Disposition(disposition));
        }
    }
    pub(crate) fn generation(&self, model: QualifiedModel) -> Option<OwnedObservation> {
        let (owner, key) = self.parent.as_ref()?;
        Some(
            owner
                .create(
                    Kind::Generation,
                    Endpoint::Other,
                    model,
                    Lane::Control,
                    Some(*key),
                )
                .into_owned(owner),
        )
    }
    pub(crate) fn reject(&self, model: AdmissionModel, reason: Rejection) {
        let Some((owner, key)) = &self.parent else {
            return;
        };
        // Reject only a live, eligible HTTP attempt, once, and never after reserve.
        let endpoint = {
            let Some(mut state) = owner.lock() else {
                return;
            };
            let Some(record) = state.record(*key).copied() else {
                return;
            };
            if record.epoch != state.epoch
                || record.epoch != owner.epoch.load(Ordering::SeqCst)
                || record.terminal
                || matches!(
                    record.disposition,
                    Disposition::NewGeneration
                        | Disposition::Duplicate
                        | Disposition::PreReservationRejected
                )
            {
                return;
            }
            state.records[*key as usize & 255].disposition = Disposition::PreReservationRejected;
            record.endpoint
        };
        owner.reject(
            Some(endpoint),
            if endpoint.chat().is_some() {
                model
            } else {
                AdmissionModel::NotApplicable
            },
            reason,
        );
    }
}

pub(crate) struct HttpObservation {
    observation: Option<OwnedObservation>,
    status: Status,
}
impl HttpObservation {
    pub(crate) fn new(
        metrics: Option<&Arc<AggregateMetrics>>,
        endpoint: Endpoint,
    ) -> (Self, RequestContext) {
        let observation = metrics.map(|m| m.http(endpoint).into_owned(m));
        let context = RequestContext {
            parent: observation.as_ref().map(|o| (o.owner.clone(), o.key)),
        };
        if endpoint.chat().is_none() {
            context.disposition(Disposition::ControlOrOther);
        }
        (
            Self {
                observation,
                status: Status::Unknown,
            },
            context,
        )
    }
    pub(crate) fn status(&mut self, status: u16) {
        self.status = match status / 100 {
            1 => Status::Informational,
            2 => Status::Success,
            3 => Status::Redirect,
            4 => Status::ClientError,
            5 => Status::ServerError,
            _ => Status::Unknown,
        };
    }
    pub(crate) fn finish(&mut self, terminal: HttpTerminal) {
        if let Some(mut observation) = self.observation.take() {
            observation.update(Update::HttpSelected(self.status, terminal));
        }
    }
}
impl Drop for HttpObservation {
    fn drop(&mut self) {
        self.finish(HttpTerminal::Unknown);
    }
}
