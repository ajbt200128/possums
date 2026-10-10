//! Unwired ownership prototype: no worker, route, or approved MiB partition.
//!
//! Slots below model lifetimes, NOT memory usage. Before any production use,
//! separately prove decoder/worker/control envelopes, classify control routes
//! before collecting bodies, and acquire the work lease BEFORE JSON expansion.
//! Ingress cannot be budgeted as just the raw-body limit. Reservation handoff,
//! authenticated upstream draining and exactly-once finalization remain unwired.

use crate::stream_owner::{delivery, DeliveryBody, DeliveryTx, Limits};
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

pub struct Admission {
    work: Arc<Semaphore>,
    delivery: Arc<Semaphore>,
    controls: Arc<Semaphore>,
}

pub struct Admitted {
    work: OwnedSemaphorePermit,
    delivery: OwnedSemaphorePermit,
}

/// A control handler owns this until transferring it to response storage.
/// Its sender, body and surviving frames then share the same lane permit.
pub struct ResponseAdmission(OwnedSemaphorePermit);

/// Move-only storage for a future worker's reservation and work lease. This is
/// not an async handoff/acknowledgement protocol and does not spawn a worker.
/// The reservation is dropped before the work lease; the body never owns it.
pub struct Work<R> {
    _reservation: R,
    _work: OwnedSemaphorePermit,
}

impl Admission {
    pub fn new(generations: usize) -> Self {
        Self {
            work: Arc::new(Semaphore::new(generations)),
            delivery: Arc::new(Semaphore::new(generations)),
            controls: Arc::new(Semaphore::new(1)),
        }
    }

    /// No waiter/future is created. A failed second acquisition rolls back the
    /// first. Unread responses therefore cannot accumulate over generations.
    pub fn try_chat(&self) -> Option<Admitted> {
        let work = self.work.clone().try_acquire_owned().ok()?;
        let delivery = self.delivery.clone().try_acquire_owned().ok()?;
        Some(Admitted { work, delivery })
    }

    pub fn try_control(&self) -> Option<ResponseAdmission> {
        self.controls
            .clone()
            .try_acquire_owned()
            .ok()
            .map(ResponseAdmission)
    }

    pub fn available(&self) -> (usize, usize) {
        (
            self.work.available_permits(),
            self.delivery.available_permits(),
        )
    }
}

impl Admitted {
    pub fn into_owners<R>(
        self,
        reservation: R,
        limits: Limits,
    ) -> (Work<R>, DeliveryTx, DeliveryBody) {
        let (tx, body) = ResponseAdmission(self.delivery).into_delivery(limits);
        (
            Work {
                _reservation: reservation,
                _work: self.work,
            },
            tx,
            body,
        )
    }
}

impl ResponseAdmission {
    pub fn into_delivery(self, limits: Limits) -> (DeliveryTx, DeliveryBody) {
        delivery(possums::telemetry::hooks::Lease::from(self.0), limits)
    }
}
