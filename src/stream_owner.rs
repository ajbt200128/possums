//! Bounded delivery storage for streamed `/v1/chat/completions`.
//!
//! Payload bytes and outstanding chunk owners (including dequeued frames) are
//! bounded independently. This is not an RSS/allocator or whole-worker budget.
//! Delivery shares a heavy admission lease, never a reservation or inference task.

use crate::telemetry::{DeliveryTerminal, OwnedObservation};
use axum::body::{Bytes, HttpBody};
use std::{
    convert::Infallible,
    pin::Pin,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    task::{Context, Poll},
};
use tokio::sync::{mpsc, watch, OwnedSemaphorePermit, Semaphore};

/// Shared through request preflight and response-body ownership.
#[derive(Clone)]
pub(crate) struct Disconnect(Arc<watch::Sender<bool>>);
impl Default for Disconnect {
    fn default() -> Self {
        Self(Arc::new(watch::channel(false).0))
    }
}
impl Disconnect {
    pub(crate) fn cancel(&self) {
        self.0.send_replace(true);
    }
    pub(crate) async fn cancelled(&self) {
        let mut rx = self.0.subscribe();
        if !*rx.borrow_and_update() {
            let _ = rx.changed().await;
        }
    }
}
pub(crate) struct DisconnectGuard(pub(crate) Option<Disconnect>);
impl Drop for DisconnectGuard {
    fn drop(&mut self) {
        if let Some(signal) = &self.0 {
            signal.cancel();
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Limits {
    pub frames: usize,
    pub payload_bytes: u32,
    pub chunk_bytes: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DeliveryError {
    Closed,
    TooLarge,
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
    producer: AtomicUsize,
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

pub(crate) fn delivery(
    lease: impl Into<Arc<crate::telemetry::hooks::Lease>>,
    limits: Limits,
) -> (DeliveryTx, DeliveryBody) {
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
        producer: AtomicUsize::new(0),
    });
    let final_metrics = Arc::new(Mutex::new(None));
    (
        DeliveryTx {
            sender: Some(sender),
            lease: Some(lease.clone()),
            budget: budget.clone(),
            chunk_bytes: limits.chunk_bytes,
            failure: None,
            final_metrics: final_metrics.clone(),
        },
        DeliveryBody {
            rx,
            _lease: lease,
            observation: None,
            budget,
            disconnect: Disconnect::default(),
            final_metrics,
        },
    )
}

pub(crate) struct DeliveryTx {
    sender: Option<mpsc::Sender<Bytes>>,
    lease: Option<Arc<crate::telemetry::hooks::Lease>>,
    budget: Arc<Budget>,
    chunk_bytes: u32,
    failure: Option<DeliveryError>,
    final_metrics: Arc<Mutex<Option<http::HeaderValue>>>,
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
    _lease: Arc<crate::telemetry::hooks::Lease>,
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
            self.budget.producer.store(2, Ordering::Release);
            // Drop our lease and close the queue, but leave accepted frames for
            // the consumer. A retained detached sender cannot pin a lane forever.
            self.sender.take();
            self.lease.take();
        }
        result
    }

    /// Backpressure owns no copied payload until queue, frame and byte credit
    /// are available. Body closure wakes EVERY wait, even if a transport retains
    /// slices (and their credits) after dropping the body. The caller observes
    /// closure and cancels upstream without cancelling its settlement owner.
    pub(crate) async fn send(&mut self, data: &[u8]) -> Result<(), DeliveryError> {
        let result = async {
            let size = self.size(data)?;
            let sender = self.sender.as_ref().unwrap();
            if sender.is_closed() {
                return Err(DeliveryError::Closed);
            }
            if size == 0 {
                return Ok(());
            }
            tokio::select! {
                biased;
                _ = sender.closed() => Err(DeliveryError::Closed),
                result = async {
                    let slot = sender.reserve().await.map_err(|_| DeliveryError::Closed)?;
                    let frame = self.budget.frames.clone().acquire_owned().await
                        .map_err(|_| DeliveryError::Closed)?;
                    let bytes = self.budget.bytes.clone().acquire_many_owned(size).await
                        .map_err(|_| DeliveryError::Closed)?;
                    slot.send(self.chunk(data, bytes, frame));
                    Ok(())
                } => result,
            }
        }
        .await;
        self.detach_on_error(result)
    }

    /// Explicit normal producer termination, including a normally delivered error.
    pub(crate) fn finish(&mut self) {
        let _ = self
            .budget
            .producer
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire);
        self.sender.take();
    }

    /// One bounded first-party trailer, not queued payload or telemetry. It is
    /// emitted only after all accepted DATA and normal producer termination.
    pub(crate) fn finish_with_metrics(&mut self, metrics: Option<http::HeaderValue>) {
        if self.failure.is_none() && self.sender.is_some() {
            *self.final_metrics.lock().unwrap() = metrics.filter(|v| v.as_bytes().len() <= 4096);
        }
        self.finish();
    }

    pub(crate) fn detach(&mut self) {
        let _ = self.detach_on_error(Err(DeliveryError::Closed));
    }

    pub(crate) fn failure(&self) -> Option<DeliveryError> {
        self.failure
    }

    pub(crate) fn usage(&self) -> Usage {
        self.budget.usage()
    }
}

pub(crate) struct DeliveryBody {
    observation: Option<OwnedObservation>,
    rx: mpsc::Receiver<Bytes>,
    _lease: Arc<crate::telemetry::hooks::Lease>,
    budget: Arc<Budget>,
    disconnect: Disconnect,
    final_metrics: Arc<Mutex<Option<http::HeaderValue>>>,
}

impl DeliveryBody {
    pub(crate) fn disconnected(mut self, signal: Disconnect) -> Self {
        self.disconnect = signal;
        self
    }
    pub(crate) fn observed(mut self, observation: Option<OwnedObservation>) -> Self {
        self.observation = observation;
        self
    }
    fn finish(&mut self, terminal: DeliveryTerminal) {
        if let Some(mut observation) = self.observation.take() {
            observation.finish_delivery(terminal);
        }
    }
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
    heavy: std::sync::Weak<crate::telemetry::hooks::Lease>,
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
        let part = std::task::ready!(self.rx.poll_recv(cx));
        if part.is_none() {
            let terminal = match self.budget.producer.load(Ordering::Acquire) {
                1 => DeliveryTerminal::Completed,
                2 => DeliveryTerminal::Interrupted,
                _ => DeliveryTerminal::Unknown,
            };
            self.finish(terminal);
            let metrics = self.final_metrics.lock().unwrap().take();
            if self.budget.producer.load(Ordering::Acquire) == 1 {
                if let Some(metrics) = metrics {
                    let mut trailers = http::HeaderMap::new();
                    trailers.insert("x-tinfoil-usage-metrics", metrics);
                    return Poll::Ready(Some(Ok(http_body::Frame::trailers(trailers))));
                }
            }
        }
        Poll::Ready(part.map(|bytes| Ok(http_body::Frame::data(bytes))))
    }
}

impl Drop for DeliveryBody {
    fn drop(&mut self) {
        self.disconnect.cancel();
        self.finish(DeliveryTerminal::Interrupted);
    }
}

#[cfg(test)]
mod wait_tests {
    use super::*;
    use http_body_util::BodyExt;
    #[tokio::test]
    async fn metrics_follow_data_once_only_on_normal_bounded_finish() {
        for normal in [false, true] {
            for oversized in [false, true] {
                let heavy: crate::telemetry::hooks::Lease = Arc::new(Semaphore::new(1))
                    .try_acquire_owned()
                    .unwrap()
                    .into();
                let (mut tx, mut body) = delivery(
                    heavy,
                    Limits {
                        frames: 1,
                        payload_bytes: 4,
                        chunk_bytes: 4,
                    },
                );
                tx.send(b"data").await.unwrap();
                if !normal {
                    tx.detach();
                }
                let metrics = if oversized {
                    "x".repeat(4097)
                } else {
                    "prompt=2,completion=1,total=3,model=m".into()
                };
                tx.finish_with_metrics(Some(metrics.parse().unwrap()));
                assert_eq!(
                    body.frame().await.unwrap().unwrap().into_data().unwrap(),
                    b"data"[..]
                );
                if normal && !oversized {
                    let trailers = body
                        .frame()
                        .await
                        .unwrap()
                        .unwrap()
                        .into_trailers()
                        .unwrap();
                    assert_eq!(trailers.len(), 1);
                    assert_eq!(trailers["x-tinfoil-usage-metrics"], metrics);
                }
                assert!(body.frame().await.is_none());
                assert!(body.frame().await.is_none());
                assert_eq!(body.usage().bytes, 0);
            }
        }
    }
    #[tokio::test]
    async fn full_capacity_waits_for_retained_credit_then_resumes() {
        let heavy: crate::telemetry::hooks::Lease = Arc::new(Semaphore::new(1))
            .try_acquire_owned()
            .unwrap()
            .into();
        let (mut tx, mut body) = delivery(
            heavy,
            Limits {
                frames: 1,
                payload_bytes: 4,
                chunk_bytes: 4,
            },
        );
        tx.send(b"1234").await.unwrap();
        let mut sending = Box::pin(tx.send(b"5678"));
        assert!(futures_util::poll!(&mut sending).is_pending());
        let frame = body.frame().await.unwrap().unwrap().into_data().unwrap();
        let slice = frame.slice(..1);
        drop(frame);
        assert!(futures_util::poll!(&mut sending).is_pending());
        drop(slice);
        tokio::time::timeout(std::time::Duration::from_secs(1), &mut sending)
            .await
            .unwrap()
            .unwrap();
        drop(sending);
        assert_eq!(body.usage().high_frames, 1);
        assert_eq!(body.usage().high_bytes, 4);
        assert_eq!(
            body.frame().await.unwrap().unwrap().into_data().unwrap(),
            b"5678"[..]
        );
        assert!(tx.failure().is_none());
    }
    #[tokio::test]
    async fn actual_body_drop_wakes_capacity_wait_without_drain() {
        let heavy: crate::telemetry::hooks::Lease = Arc::new(Semaphore::new(1))
            .try_acquire_owned()
            .unwrap()
            .into();
        let (mut tx, body) = delivery(
            heavy,
            Limits {
                frames: 1,
                payload_bytes: 4,
                chunk_bytes: 4,
            },
        );
        tx.send(b"1234").await.unwrap();
        let mut sending = Box::pin(tx.send(b"5678"));
        assert!(futures_util::poll!(&mut sending).is_pending());
        drop(body);
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(1), sending)
                .await
                .unwrap(),
            Err(DeliveryError::Closed)
        );
    }
}
