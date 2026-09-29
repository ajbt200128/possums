//! The real production primitive, compiled here to keep it crate-private.
//! No route, admission policy, settlement, or whole-process memory claims.
#[path = "../src/stream_owner.rs"]
#[allow(dead_code)]
mod stream_owner;

use http_body_util::BodyExt;
use std::{sync::Arc, time::Duration};
use stream_owner::{delivery, DeliveryError, Limits};
use tokio::sync::Semaphore;

const LIMITS: Limits = Limits {
    frames: 2,
    payload_bytes: 16,
    chunk_bytes: 8,
};

fn lane() -> (Arc<Semaphore>, tokio::sync::OwnedSemaphorePermit) {
    let lane = Arc::new(Semaphore::new(1));
    let lease = lane.clone().try_acquire_owned().unwrap();
    (lane, lease)
}

#[tokio::test]
async fn held_frames_clones_and_slices_keep_credit_and_lease() {
    let (lane, lease) = lane();
    let (startup, mut body) = delivery(lease, LIMITS, Duration::from_secs(1));
    let mut tx = startup.into_streaming();
    tx.try_send(b"12345678").unwrap();
    tx.try_send(b"abcdefgh").unwrap();
    let frame = body.frame().await.unwrap().unwrap().into_data().unwrap();
    let clone = frame.clone();
    let slice = clone.slice(1..2);
    drop(frame);
    drop(clone);
    assert_eq!(tx.usage().bytes, 16);
    assert_eq!(tx.usage().frames, 2);
    assert_eq!(tx.try_send(b"x"), Err(DeliveryError::Full));
    assert_eq!(tx.failure(), Some(DeliveryError::Full));
    drop(body);
    assert_eq!(tx.usage().bytes, 8);
    assert_eq!(tx.usage().frames, 1);
    assert_eq!(lane.available_permits(), 0);
    drop(slice);
    assert_eq!(tx.usage().bytes, 0);
    assert_eq!(tx.usage().frames, 0);
    assert_eq!(lane.available_permits(), 1); // Detached tx does not pin lease.
    assert_eq!(tx.try_send(b"x"), Err(DeliveryError::Full));
}

#[tokio::test]
async fn independent_frame_byte_chunk_limits_and_sticky_detachment() {
    for (limits, chunks, expected) in [
        (LIMITS, vec![&b"123456789"[..]], DeliveryError::TooLarge),
        (LIMITS, vec![&b"a"[..], b"b", b"c"], DeliveryError::Full),
        (
            Limits {
                frames: 3,
                ..LIMITS
            },
            vec![&b"12345678"[..], b"abcdefgh", b"x"],
            DeliveryError::Full,
        ),
    ] {
        let (_, lease) = lane();
        let (startup, mut body) = delivery(lease, limits, Duration::from_secs(1));
        let mut tx = startup.into_streaming();
        for _ in 0..100 {
            tx.try_send(b"").unwrap();
        }
        assert_eq!(tx.usage().frames, 0);
        for chunk in &chunks[..chunks.len() - 1] {
            tx.try_send(chunk).unwrap();
        }
        assert_eq!(tx.try_send(chunks.last().unwrap()), Err(expected));
        // Accepted chunks drain to actual EOF while the detached sender lives.
        let mut consumed = 0;
        while let Some(frame) = body.frame().await {
            consumed += frame.unwrap().into_data().unwrap().len();
        }
        assert_eq!(
            consumed,
            chunks[..chunks.len() - 1]
                .iter()
                .map(|c| c.len())
                .sum::<usize>()
        );
        assert_eq!(tx.usage().bytes, 0);
        assert_eq!(tx.usage().frames, 0);
        assert!(tx.usage().high_bytes <= limits.payload_bytes as usize);
        assert!(tx.usage().high_frames <= limits.frames);
        // Model an upstream callback loop: no retry, waiter, or reattachment.
        for _ in 0..1000 {
            assert_eq!(tx.try_send(b"next"), Err(expected));
        }
    }
}

#[tokio::test]
async fn slow_consumer_returns_credit_before_blocking_producer_can_continue() {
    let (_, lease) = lane();
    let limits = Limits {
        frames: 1,
        payload_bytes: 8,
        chunk_bytes: 8,
    };
    let (mut startup, mut body) = delivery(lease, limits, Duration::from_secs(5));
    let (attempted, attempt) = tokio::sync::oneshot::channel();
    let (finished, mut finish) = tokio::sync::oneshot::channel();
    let producer = tokio::task::spawn_blocking(move || {
        startup.send_blocking(b"12345678").unwrap();
        attempted.send(()).unwrap();
        startup.send_blocking(b"abcdefgh").unwrap();
        finished.send(()).unwrap();
        startup.into_streaming().usage()
    });
    attempt.await.unwrap();
    let held = body.frame().await.unwrap().unwrap();
    assert_eq!(body.usage().bytes, 8);
    // Dequeue alone cannot unblock producer: the outstanding frame owns credit.
    assert!(matches!(
        finish.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
    ));
    drop(held);
    let next = body.frame().await.unwrap().unwrap().into_data().unwrap();
    assert_eq!(&next[..], b"abcdefgh");
    drop(next);
    finish.await.unwrap();
    let usage = producer.await.unwrap();
    assert_eq!(usage.high_bytes, 8);
    assert_eq!(usage.high_frames, 1);
    assert!(body.frame().await.is_none());
    assert_eq!(body.usage().bytes, 0);
}

#[tokio::test]
async fn startup_timeout_and_disconnect_detach_without_vetoing_inference() {
    for disconnect in [false, true] {
        let (lane, lease) = lane();
        let limits = Limits {
            frames: 2,
            payload_bytes: 8,
            chunk_bytes: 8,
        };
        let (mut startup, mut body) = delivery(lease, limits, Duration::from_millis(100));
        let (filled, fill) = tokio::sync::oneshot::channel();
        let (resume, resumed) = tokio::sync::oneshot::channel();
        let producer = tokio::task::spawn_blocking(move || {
            startup.send_blocking(b"12345678").unwrap();
            filled.send(()).unwrap();
            resumed.blocking_recv().unwrap();
            let result = startup.send_blocking(b"x");
            (result, startup.into_streaming())
        });
        fill.await.unwrap();
        let held = body.frame().await.unwrap().unwrap();
        // Byte credit is exhausted, despite an empty queue and a free frame slot.
        if disconnect {
            drop(body);
        }
        resume.send(()).unwrap();
        let (result, mut tx) = tokio::time::timeout(Duration::from_secs(3), producer)
            .await
            .unwrap()
            .unwrap();
        let expected = if disconnect {
            DeliveryError::Closed
        } else {
            DeliveryError::TimedOut
        };
        assert_eq!(result, Err(expected));
        assert_eq!(tx.failure(), Some(expected));
        let mut inference_steps = 0;
        for _ in 0..1000 {
            inference_steps += 1;
            assert_eq!(tx.try_send(b"delta"), Err(expected));
        }
        assert_eq!(inference_steps, 1000);
        assert_eq!(lane.available_permits(), 0);
        drop(held);
        assert_eq!(tx.usage().bytes, 0);
    }
}

#[tokio::test]
async fn closed_sink_and_empty_body_have_explicit_lifetimes() {
    let (lane, lease) = lane();
    let (startup, body) = delivery(lease, LIMITS, Duration::from_secs(1));
    let mut tx = startup.into_streaming();
    tx.try_send(b"queued").unwrap();
    drop(body);
    assert_eq!(tx.try_send(b""), Err(DeliveryError::Closed));
    assert_eq!(tx.usage().bytes, 0);
    assert_eq!(lane.available_permits(), 1);

    let lease = lane.clone().try_acquire_owned().unwrap();
    let (startup, mut body) = delivery(lease, LIMITS, Duration::from_secs(1));
    drop(startup);
    assert!(body.frame().await.is_none());
    assert_eq!(lane.available_permits(), 0);
    drop(body);
    assert_eq!(lane.available_permits(), 1);
}
