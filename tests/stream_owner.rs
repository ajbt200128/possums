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
async fn queue_frame_and_byte_waits_wake_on_body_drop_even_with_retained_slices() {
    for wait in ["queue", "frame", "bytes"] {
        let (lane, lease) = lane();
        let limits = match wait {
            "frame" => Limits {
                frames: 1,
                ..LIMITS
            },
            "bytes" => Limits {
                frames: 3,
                payload_bytes: 8,
                ..LIMITS
            },
            _ => LIMITS,
        };
        let (mut tx, mut body) = delivery(lease, limits);
        tx.send(b"12345678").await.unwrap();
        let slice = if wait == "queue" {
            tx.send(b"abcdefgh").await.unwrap();
            None
        } else {
            let frame = body.frame().await.unwrap().unwrap().into_data().unwrap();
            let clone = frame.clone();
            let slice = clone.slice(1..2);
            drop(frame);
            drop(clone);
            Some(slice)
        };
        let probe = body.probe();
        let mut send = Box::pin(tx.send(b"x"));
        assert!(futures_util::poll!(&mut send).is_pending());
        let high = probe.usage();
        assert!(high.high_frames <= limits.frames);
        assert!(high.high_bytes <= limits.payload_bytes as usize);
        drop(body);
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(1), send)
                .await
                .unwrap(),
            Err(DeliveryError::Closed)
        );
        assert_eq!(tx.failure(), Some(DeliveryError::Closed));
        assert_eq!(tx.send(b"next").await, Err(DeliveryError::Closed));
        assert_eq!(tx.usage().bytes, if slice.is_some() { 8 } else { 0 });
        if slice.is_some() {
            assert_eq!(lane.available_permits(), 0);
        }
        drop(slice);
        assert_eq!(tx.usage().bytes, 0);
        assert_eq!(tx.usage().frames, 0);
        assert_eq!(lane.available_permits(), 1); // Detached sender cannot pin lease.
    }
}

#[tokio::test]
async fn retained_slice_backpressures_without_detaching_and_release_resumes() {
    let (_, lease) = lane();
    let (mut tx, mut body) = delivery(
        lease,
        Limits {
            frames: 1,
            ..LIMITS
        },
    );
    tx.send(b"12345678").await.unwrap();
    let frame = body.frame().await.unwrap().unwrap().into_data().unwrap();
    let slice = frame.slice(..1);
    drop(frame);
    let mut send = Box::pin(tx.send(b"next"));
    assert!(futures_util::poll!(&mut send).is_pending());
    drop(slice);
    send.await.unwrap();
    assert_eq!(tx.failure(), None);
    tx.finish();
    assert_eq!(
        body.frame().await.unwrap().unwrap().into_data().unwrap(),
        "next"
    );
    assert!(body.frame().await.is_none());
}

#[tokio::test]
async fn lossless_slow_reader_exceeds_eight_frames_with_unchanged_highwater() {
    let (_, lease) = lane();
    let limits = Limits {
        frames: 8,
        payload_bytes: 64 * 1024,
        chunk_bytes: 8 * 1024,
    };
    let (mut tx, mut body) = delivery(lease, limits);
    let probe = body.probe();
    let (ready, waiting) = tokio::sync::oneshot::channel();
    let producer = tokio::spawn(async move {
        for index in 0..8u8 {
            tx.send(&[index; 8192]).await.unwrap();
        }
        let mut ninth = Box::pin(tx.send(&[8; 8192]));
        assert!(futures_util::poll!(&mut ninth).is_pending());
        ready.send(()).unwrap();
        ninth.await.unwrap();
        for index in 9..100u8 {
            tx.send(&[index; 8192]).await.unwrap();
        }
        tx.finish();
        assert_eq!(tx.failure(), None);
    });
    waiting.await.unwrap();
    assert_eq!(
        (probe.usage().high_frames, probe.usage().high_bytes),
        (8, 64 * 1024)
    );
    for index in 0..100u8 {
        tokio::task::yield_now().await;
        let bytes = body.frame().await.unwrap().unwrap().into_data().unwrap();
        assert_eq!(bytes.as_ref(), &[index; 8192]);
    }
    assert!(body.frame().await.is_none());
    producer.await.unwrap();
    assert_eq!(
        (probe.usage().high_frames, probe.usage().high_bytes),
        (8, 64 * 1024)
    );
    assert_eq!((probe.usage().frames, probe.usage().bytes), (0, 0));
}

#[tokio::test]
async fn oversized_chunk_detaches_but_empty_emissions_consume_no_credit() {
    let (_, lease) = lane();
    let (mut tx, mut body) = delivery(lease, LIMITS);
    for _ in 0..100 {
        tx.send(b"").await.unwrap();
    }
    assert_eq!(tx.usage().frames, 0);
    tx.send(b"accepted").await.unwrap();
    assert_eq!(tx.send(b"123456789").await, Err(DeliveryError::TooLarge));
    assert_eq!(
        body.frame().await.unwrap().unwrap().into_data().unwrap(),
        "accepted"
    );
    assert!(body.frame().await.is_none());
    assert_eq!(tx.send(b"next").await, Err(DeliveryError::TooLarge));
    assert_eq!((tx.usage().frames, tx.usage().bytes), (0, 0));
}

#[tokio::test]
async fn closed_sink_and_empty_body_have_explicit_lifetimes() {
    let (lane, lease) = lane();
    let (mut tx, body) = delivery(lease, LIMITS);
    tx.send(b"queued").await.unwrap();
    drop(body);
    assert_eq!(tx.send(b"").await, Err(DeliveryError::Closed));
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
