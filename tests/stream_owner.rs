//! The real production primitive, compiled here to keep it crate-private.
//! No route, admission policy, settlement, or whole-process memory claims.
#[path = "../src/stream_owner.rs"]
#[allow(dead_code)]
mod stream_owner;

use http_body_util::BodyExt;
use possums::telemetry;
use std::sync::Arc;
use stream_owner::{delivery, DeliveryError, Limits};
use tokio::sync::Semaphore;

const LIMITS: Limits = Limits {
    frames: 2,
    payload_bytes: 16,
    chunk_bytes: 8,
};

fn lane() -> (Arc<Semaphore>, possums::telemetry::hooks::Lease) {
    let lane = Arc::new(Semaphore::new(1));
    let lease = possums::telemetry::hooks::Lease::from(lane.clone().try_acquire_owned().unwrap());
    (lane, lease)
}

#[tokio::test]
async fn held_frames_clones_and_slices_keep_credit_and_lease() {
    let (lane, lease) = lane();
    let (mut tx, mut body) = delivery(lease, LIMITS);
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
        let (mut tx, mut body) = delivery(lease, limits);
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
async fn dequeued_frame_keeps_credit_until_final_slice_drops() {
    let (_, lease) = lane();
    let limits = Limits {
        frames: 1,
        payload_bytes: 8,
        chunk_bytes: 8,
    };
    let (mut tx, mut body) = delivery(lease, limits);
    tx.try_send(b"12345678").unwrap();
    let frame = body.frame().await.unwrap().unwrap().into_data().unwrap();
    let slice = frame.slice(0..1);
    drop(frame);
    assert_eq!(tx.usage().bytes, 8);
    assert_eq!(tx.try_send(b"x"), Err(DeliveryError::Full));
    drop(slice);
    assert_eq!(tx.usage().bytes, 0);
    assert_eq!(tx.failure(), Some(DeliveryError::Full));
    assert!(body.frame().await.is_none());
}

#[tokio::test]
async fn disconnected_consumer_detaches_without_pinning_delivery_lane() {
    let (lane, lease) = lane();
    let (mut tx, mut body) = delivery(lease, LIMITS);
    tx.try_send(b"queued").unwrap();
    let held = body.frame().await.unwrap().unwrap().into_data().unwrap();
    drop(body);
    assert_eq!(tx.try_send(b"next"), Err(DeliveryError::Closed));
    assert_eq!(tx.failure(), Some(DeliveryError::Closed));
    assert_eq!(lane.available_permits(), 0);
    drop(held);
    assert_eq!(lane.available_permits(), 1);
}

#[tokio::test]
async fn closed_sink_and_empty_body_have_explicit_lifetimes() {
    let (lane, lease) = lane();
    let (mut tx, body) = delivery(lease, LIMITS);
    tx.try_send(b"queued").unwrap();
    drop(body);
    assert_eq!(tx.try_send(b""), Err(DeliveryError::Closed));
    assert_eq!(tx.usage().bytes, 0);
    assert_eq!(lane.available_permits(), 1);

    let lease = possums::telemetry::hooks::Lease::from(lane.clone().try_acquire_owned().unwrap());
    let (tx, mut body) = delivery(lease, LIMITS);
    drop(tx);
    assert!(body.frame().await.is_none());
    assert_eq!(lane.available_permits(), 0);
    drop(body);
    assert_eq!(lane.available_permits(), 1);
}
