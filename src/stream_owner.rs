//! Bounded delivery storage, deliberately not wired to `/chat` yet.
//!
//! Payload bytes and outstanding chunk owners (including dequeued frames) are
//! bounded independently. This is not an RSS/allocator or whole-worker budget.
//! Delivery shares a heavy admission lease, never a reservation or inference task.
//! This is staged plumbing; the buffered route does not use it yet.

use axum::body::{Bytes, HttpBody};
use std::{
    convert::Infallible,
    pin::Pin,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    task::{Context, Poll},
    time::Duration,
};
use tokio::sync::{mpsc, OwnedSemaphorePermit, Semaphore};

#[derive(Clone, Copy)]
pub(crate) struct Limits {
    pub frames: usize,
    pub payload_bytes: u32,
    pub chunk_bytes: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DeliveryError {
    Full,
    Closed,
    TooLarge,
    TimedOut,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Usage {
    pub bytes: usize,
    pub frames: usize,
    pub high_bytes: usize,
    pub high_frames: usize,
}

struct Budget {
    bytes: Arc<Semaphore>,
    frames: Arc<Semaphore>,
    used_bytes: AtomicUsize,
    used_frames: AtomicUsize,
    high_bytes: AtomicUsize,
    high_frames: AtomicUsize,
}

impl Budget {
    fn usage(&self) -> Usage {
        Usage {
            bytes: self.used_bytes.load(Ordering::SeqCst),
            frames: self.used_frames.load(Ordering::SeqCst),
            high_bytes: self.high_bytes.load(Ordering::SeqCst),
            high_frames: self.high_frames.load(Ordering::SeqCst),
        }
    }
}

/// The only blocking-capable phase. Construct before response handoff, use on
/// `spawn_blocking` while the runtime polls the body, then consume into streaming.
/// The deadline covers ALL startup writes, not a fresh timeout for every chunk.
/// Dropping an async JoinHandle does not cancel spawn_blocking: this finite
/// deadline is therefore essential. It bounds delivery waits, not renderer CPU.
pub(crate) struct StartupTx {
    tx: DeliveryTx,
    deadline: tokio::time::Instant,
}

pub(crate) fn delivery(
    lease: impl Into<Arc<OwnedSemaphorePermit>>,
    limits: Limits,
    startup_timeout: Duration,
) -> (StartupTx, DeliveryBody) {
    assert!(limits.frames > 0 && limits.chunk_bytes > 0);
    assert!(limits.chunk_bytes <= limits.payload_bytes);
    let (sender, rx) = mpsc::channel(limits.frames);
    let lease = lease.into();
    let budget = Arc::new(Budget {
        bytes: Arc::new(Semaphore::new(limits.payload_bytes as usize)),
        frames: Arc::new(Semaphore::new(limits.frames)),
        used_bytes: AtomicUsize::new(0),
        used_frames: AtomicUsize::new(0),
        high_bytes: AtomicUsize::new(0),
        high_frames: AtomicUsize::new(0),
    });
    (
        StartupTx {
            tx: DeliveryTx {
                sender: Some(sender),
                lease: Some(lease.clone()),
                budget: budget.clone(),
                chunk_bytes: limits.chunk_bytes,
                failure: None,
            },
            deadline: tokio::time::Instant::now() + startup_timeout,
        },
        DeliveryBody {
            rx,
            _lease: lease,
            budget,
        },
    )
}

pub(crate) struct DeliveryTx {
    sender: Option<mpsc::Sender<Bytes>>,
    lease: Option<Arc<OwnedSemaphorePermit>>,
    budget: Arc<Budget>,
    chunk_bytes: u32,
    failure: Option<DeliveryError>,
}

// Field order matters: free payload BEFORE returning credit. Bytes::from_owner
// keeps this entire owner alive through clones/slices, even one-byte slices.
struct Chunk {
    data: Box<[u8]>,
    _credit: Credit,
}

struct Credit {
    budget: Arc<Budget>,
    size: usize,
    _bytes: OwnedSemaphorePermit,
    _frame: OwnedSemaphorePermit,
    _lease: Arc<OwnedSemaphorePermit>,
}

impl Drop for Credit {
    fn drop(&mut self) {
        self.budget
            .used_bytes
            .fetch_sub(self.size, Ordering::SeqCst);
        self.budget.used_frames.fetch_sub(1, Ordering::SeqCst);
    }
}

impl AsRef<[u8]> for Chunk {
    fn as_ref(&self) -> &[u8] {
        &self.data
    }
}

impl DeliveryTx {
    fn size(&self, data: &[u8]) -> Result<u32, DeliveryError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        let size = u32::try_from(data.len()).map_err(|_| DeliveryError::TooLarge)?;
        if size > self.chunk_bytes {
            return Err(DeliveryError::TooLarge);
        }
        Ok(size)
    }

    fn chunk(
        &self,
        data: &[u8],
        bytes: OwnedSemaphorePermit,
        frame: OwnedSemaphorePermit,
    ) -> Bytes {
        let used = self
            .budget
            .used_bytes
            .fetch_add(data.len(), Ordering::SeqCst)
            + data.len();
        self.budget.high_bytes.fetch_max(used, Ordering::SeqCst);
        let frames = self.budget.used_frames.fetch_add(1, Ordering::SeqCst) + 1;
        self.budget.high_frames.fetch_max(frames, Ordering::SeqCst);
        Bytes::from_owner(Chunk {
            data: data.into(),
            _credit: Credit {
                budget: self.budget.clone(),
                size: data.len(),
                _bytes: bytes,
                _frame: frame,
                _lease: self.lease.as_ref().unwrap().clone(),
            },
        })
    }

    fn detach_on_error(&mut self, result: Result<(), DeliveryError>) -> Result<(), DeliveryError> {
        if let Err(error) = result {
            self.failure = Some(error);
            // Drop our lease and close the queue, but leave accepted frames for
            // the consumer. A retained detached sender cannot pin a lane forever.
            self.sender.take();
            self.lease.take();
        }
        result
    }

    /// Upstream callbacks NEVER await delivery capacity. Any error permanently
    /// detaches delivery; the caller must keep consuming/settling upstream.
    pub(crate) fn try_send(&mut self, data: &[u8]) -> Result<(), DeliveryError> {
        let result = (|| {
            let size = self.size(data)?;
            let sender = self.sender.as_ref().unwrap();
            if sender.is_closed() {
                return Err(DeliveryError::Closed);
            }
            if size == 0 {
                return Ok(()); // No zero-credit owner or queue entry.
            }
            let slot = sender.try_reserve().map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => DeliveryError::Full,
                mpsc::error::TrySendError::Closed(_) => DeliveryError::Closed,
            })?;
            let frame = self
                .budget
                .frames
                .clone()
                .try_acquire_owned()
                .map_err(|_| DeliveryError::Full)?;
            let bytes = self
                .budget
                .bytes
                .clone()
                .try_acquire_many_owned(size)
                .map_err(|_| DeliveryError::Full)?;
            slot.send(self.chunk(data, bytes, frame));
            Ok(())
        })();
        self.detach_on_error(result)
    }

    pub(crate) fn failure(&self) -> Option<DeliveryError> {
        self.failure
    }

    pub(crate) fn usage(&self) -> Usage {
        self.budget.usage()
    }
}

impl StartupTx {
    /// PRE-INFERENCE only; call exclusively from `spawn_blocking`, never from an
    /// async runtime worker. Only bounded per-chunk storage is copied. Disconnect
    /// also interrupts a byte-credit wait when the consumer retains old frames.
    pub(crate) fn send_blocking(&mut self, data: &[u8]) -> Result<(), DeliveryError> {
        let result = (|| {
            let size = self.tx.size(data)?;
            let sender = self.tx.sender.as_ref().unwrap();
            if sender.is_closed() {
                return Err(DeliveryError::Closed);
            }
            if tokio::time::Instant::now() >= self.deadline {
                return Err(DeliveryError::TimedOut);
            }
            if size == 0 {
                return Ok(());
            }
            tokio::runtime::Handle::current().block_on(async {
                tokio::time::timeout_at(self.deadline, async {
                    tokio::select! {
                        biased;
                        _ = sender.closed() => Err(DeliveryError::Closed),
                        result = async {
                            let slot = sender.reserve().await.map_err(|_| DeliveryError::Closed)?;
                            let frame = self.tx.budget.frames.clone().acquire_owned().await.map_err(|_| DeliveryError::Closed)?;
                            let bytes = self.tx.budget.bytes.clone().acquire_many_owned(size).await.map_err(|_| DeliveryError::Closed)?;
                            slot.send(self.tx.chunk(data, bytes, frame));
                            Ok(())
                        } => result,
                    }
                }).await.unwrap_or(Err(DeliveryError::TimedOut))
            })
        })();
        self.tx.detach_on_error(result)
    }

    /// This transition is one-way: inference has no blocking send API. A failed
    /// startup still yields a detached sender; it does not veto future inference.
    pub(crate) fn into_streaming(self) -> DeliveryTx {
        self.tx
    }
}

pub(crate) struct DeliveryBody {
    rx: mpsc::Receiver<Bytes>,
    _lease: Arc<OwnedSemaphorePermit>,
    budget: Arc<Budget>,
}

impl DeliveryBody {
    pub(crate) fn usage(&self) -> Usage {
        self.budget.usage()
    }

    #[cfg(test)]
    pub(crate) fn probe(&self) -> DeliveryProbe {
        DeliveryProbe {
            budget: self.budget.clone(),
            heavy: Arc::downgrade(&self._lease),
        }
    }
}

// Observe the ACTUAL budget even after body drop, without extending admission.
// Weak heavy observation is asserted only at deterministic quiescent barriers.
#[cfg(test)]
pub(crate) struct DeliveryProbe {
    budget: Arc<Budget>,
    heavy: std::sync::Weak<OwnedSemaphorePermit>,
}

#[cfg(test)]
impl DeliveryProbe {
    pub(crate) fn usage(&self) -> Usage {
        self.budget.usage()
    }

    pub(crate) fn heavy_alive(&self) -> bool {
        self.heavy.strong_count() != 0
    }
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
