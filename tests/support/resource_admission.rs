//! Unwired ownership prototype: no worker, route, or approved MiB partition.
//!
//! Slots below model lifetimes, NOT memory usage. Before any production use,
//! separately prove decoder/worker/control envelopes, classify control routes
//! before collecting bodies, and acquire the work lease BEFORE JSON expansion.
//! Ingress cannot be budgeted as just the raw-body limit. Reservation handoff,
//! authenticated upstream draining and exactly-once finalization remain unwired.

use axum::body::{Bytes, HttpBody};
use std::{
    convert::Infallible,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tokio::sync::{mpsc, OwnedSemaphorePermit, Semaphore};

pub struct Admission {
    work: Arc<Semaphore>,
    delivery: Arc<Semaphore>,
    new_chat: Arc<Semaphore>,
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
            new_chat: Arc::new(Semaphore::new(1)),
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

    pub fn try_new_chat(&self) -> Option<ResponseAdmission> {
        self.new_chat.clone().try_acquire_owned().ok().map(ResponseAdmission)
    }

    pub fn try_control(&self) -> Option<ResponseAdmission> {
        self.controls.clone().try_acquire_owned().ok().map(ResponseAdmission)
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
        assert!(limits.frames > 0 && limits.chunk_bytes > 0);
        assert!(limits.chunk_bytes <= limits.payload_bytes);
        let (tx, rx) = mpsc::channel(limits.frames);
        let lease = Arc::new(self.0);
        (
            DeliveryTx {
                tx,
                lease: lease.clone(),
                bytes: Arc::new(Semaphore::new(limits.payload_bytes as usize)),
                chunk_bytes: limits.chunk_bytes,
            },
            DeliveryBody { rx, _lease: lease },
        )
    }
}

#[derive(Clone, Copy)]
pub struct Limits {
    pub frames: usize,
    pub payload_bytes: u32,
    pub chunk_bytes: u32,
}

pub struct DeliveryTx {
    tx: mpsc::Sender<Bytes>,
    lease: Arc<OwnedSemaphorePermit>,
    bytes: Arc<Semaphore>,
    chunk_bytes: u32,
}

struct Chunk {
    data: Box<[u8]>,
    _bytes: OwnedSemaphorePermit,
    _lease: Arc<OwnedSemaphorePermit>,
}

impl AsRef<[u8]> for Chunk {
    fn as_ref(&self) -> &[u8] {
        &self.data
    }
}

impl DeliveryTx {
    /// Bound payload allocations (not allocator/Bytes/channel overhead). Both
    /// permits are acquired before copying. Dequeue does NOT release byte credit:
    /// Bytes clones/slices must retain it, even after body and sender are gone.
    /// Queue saturation/disconnect is a delivery error, never an upstream result.
    pub fn try_send(&self, data: &[u8]) -> Result<(), ()> {
        // Do not create arbitrarily many zero-credit owners outside the queue.
        if data.is_empty() {
            return Ok(());
        }
        let size = u32::try_from(data.len()).map_err(|_| ())?;
        if size > self.chunk_bytes {
            return Err(());
        }
        let slot = self.tx.try_reserve().map_err(|_| ())?;
        let bytes = self
            .bytes
            .clone()
            .try_acquire_many_owned(size)
            .map_err(|_| ())?;
        let chunk = Bytes::from_owner(Chunk {
            data: data.into(),
            _bytes: bytes,
            _lease: self.lease.clone(),
        });
        slot.send(chunk);
        Ok(())
    }

    pub fn available_bytes(&self) -> usize {
        self.bytes.available_permits()
    }
}

pub struct DeliveryBody {
    rx: mpsc::Receiver<Bytes>,
    _lease: Arc<OwnedSemaphorePermit>,
}

impl HttpBody for DeliveryBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Bytes>, Infallible>>> {
        self.rx
            .poll_recv(cx)
            .map(|part| part.map(|bytes| Ok(http_body::Frame::data(bytes))))
    }
}
