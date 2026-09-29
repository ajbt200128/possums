//! Executable lifetime model only. Nothing here is linked into the gateway.
//! These tests do not establish a 512-MiB partition or async worker handoff.
#[path = "support/resource_admission.rs"]
mod resource_admission;

use http_body_util::BodyExt;
use resource_admission::{Admission, Limits};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

// Deliberately tiny test dimensions, not proposed production memory ceilings.
const LIMITS: Limits = Limits {
    frames: 2,
    payload_bytes: 16,
    chunk_bytes: 8,
};

#[test]
fn fail_fast_saturation_leaves_new_chat_and_other_controls_independent() {
    let admission = Admission::new(4);
    let working: Vec<_> = (0..4)
        .map(|_| admission.try_chat().unwrap().into_owners((), LIMITS))
        .collect();
    assert_eq!(admission.available(), (0, 0));
    for _ in 0..100 {
        assert!(admission.try_chat().is_none());
    }
    let control = admission.try_control().unwrap();
    assert!(admission.try_control().is_none());
    let new_chat = admission.try_new_chat().unwrap();
    assert!(admission.try_new_chat().is_none());
    drop(new_chat);
    // Other controls and all four work/delivery owners remain live.
    assert!(admission.try_new_chat().is_some());
    drop(control);
    assert!(admission.try_control().is_some());
    drop(working);
    assert_eq!(admission.available(), (4, 4));
}

#[tokio::test]
async fn control_responses_retain_only_their_own_lane_through_frame_drop() {
    let admission = Admission::new(4);
    let working: Vec<_> = (0..4).map(|_| admission.try_chat().unwrap()).collect();
    let (control_tx, control_body) = admission.try_control().unwrap().into_delivery(LIMITS);
    control_tx.try_send(b"unread").unwrap();
    drop(control_tx);
    assert!(admission.try_control().is_none());
    let (tx, mut body) = admission.try_new_chat().unwrap().into_delivery(LIMITS);
    tx.try_send(b"new chat").unwrap();
    let held = body.frame().await.unwrap().unwrap();
    drop(tx);
    drop(body);
    assert!(admission.try_new_chat().is_none());
    drop(held);
    // Four work slots and an unread ordinary control response remain held.
    assert!(admission.try_new_chat().is_some());
    assert!(admission.try_control().is_none());
    drop(control_body);
    assert!(admission.try_control().is_some());
    drop(working);
    assert_eq!(admission.available(), (4, 4));
}

#[test]
fn completed_workers_with_unread_responses_block_replacement_and_rollback() {
    let admission = Admission::new(4);
    let mut unread = Vec::new();
    for _ in 0..4 {
        let (work, tx, body) = admission.try_chat().unwrap().into_owners((), LIMITS);
        tx.try_send(b"unread").unwrap();
        drop(work);
        drop(tx);
        unread.push(body);
    }
    assert_eq!(admission.available(), (4, 0));
    for _ in 0..100 {
        assert!(admission.try_chat().is_none());
        // Work acquired first, then automatically rolled back on delivery failure.
        assert_eq!(admission.available(), (4, 0));
    }
    assert!(admission.try_new_chat().is_some());
    drop(unread.pop());
    let replacement = admission.try_chat().unwrap();
    assert_eq!(admission.available(), (3, 0));
    drop(replacement);
    drop(unread);
    assert_eq!(admission.available(), (4, 4));
}

#[test]
fn disconnect_does_not_release_work_or_its_reservation() {
    struct Reservation(Arc<AtomicUsize>);
    impl Drop for Reservation {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let admission = Admission::new(1);
    let drops = Arc::new(AtomicUsize::new(0));
    let (work, tx, body) = admission
        .try_chat()
        .unwrap()
        .into_owners(Reservation(drops.clone()), LIMITS);
    tx.try_send(b"queued").unwrap();
    drop(body);
    assert!(tx.try_send(b"gone").is_err());
    assert_eq!(tx.available_bytes(), 16);
    drop(tx);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    assert_eq!(admission.available(), (0, 1));
    assert!(admission.try_chat().is_none());
    assert!(admission.try_new_chat().is_some());
    drop(work);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(admission.available(), (1, 1));
    // This proves only move ownership, not production refund/settlement policy.
}

#[tokio::test]
async fn dequeued_cloned_and_sliced_frames_keep_bytes_and_delivery_charged() {
    let admission = Admission::new(1);
    let (work, tx, mut body) = admission.try_chat().unwrap().into_owners((), LIMITS);
    tx.try_send(b"12345678").unwrap();
    tx.try_send(b"abcdefgh").unwrap();
    let frame = body.frame().await.unwrap().unwrap().into_data().unwrap();
    let clone = frame.clone();
    let slice = clone.slice(1..2);
    drop(frame);
    drop(clone);
    assert_eq!(tx.available_bytes(), 0);
    // Dequeue freed a channel slot, not bytes. One-byte slice retains all eight.
    assert!(tx.try_send(b"x").is_err());
    drop(work);
    drop(tx);
    drop(body); // Discards the second frame, but cannot release the first.
    assert_eq!(admission.available(), (1, 0));
    assert!(admission.try_chat().is_none());
    drop(slice);
    assert_eq!(admission.available(), (1, 1));
}

#[tokio::test]
async fn frame_limit_byte_limit_and_chunk_limit_are_separate() {
    let admission = Admission::new(1);
    let (work, tx, mut body) = admission.try_chat().unwrap().into_owners((), LIMITS);
    assert!(tx.try_send(b"123456789").is_err()); // Chunk limit.
    for _ in 0..100 {
        tx.try_send(b"").unwrap(); // No zero-credit allocation or queue entry.
    }
    assert_eq!(tx.available_bytes(), 16);
    tx.try_send(b"a").unwrap();
    tx.try_send(b"b").unwrap();
    assert!(tx.try_send(b"c").is_err()); // Frame limit, despite free bytes.
    assert_eq!(tx.available_bytes(), 14);
    drop(body.frame().await);
    drop(body.frame().await);
    assert_eq!(tx.available_bytes(), 16);
    tx.try_send(b"12345678").unwrap();
    tx.try_send(b"abcdefgh").unwrap();
    let held = body.frame().await.unwrap().unwrap();
    assert!(tx.try_send(b"x").is_err()); // Byte limit, despite a free slot.
    drop(held);
    assert_eq!(tx.available_bytes(), 8);
    tx.try_send(b"ABCDEFGH").unwrap(); // Failed try_reserve rolled back its slot.
    drop(work);
    drop(tx);
    drop(body);
    assert_eq!(admission.available(), (1, 1));
}

#[tokio::test]
async fn empty_unread_response_and_observed_eof_retain_delivery_until_drop() {
    let admission = Admission::new(1);
    let (work, tx, mut body) = admission.try_chat().unwrap().into_owners((), LIMITS);
    drop(work);
    drop(tx);
    assert_eq!(admission.available(), (1, 0));
    assert!(body.frame().await.is_none());
    assert_eq!(admission.available(), (1, 0));
    drop(body);
    assert_eq!(admission.available(), (1, 1));
}

#[test]
fn early_return_and_unwind_release_admission_without_waiters() {
    let admission = Admission::new(4);
    for _ in 0..100 {
        let admitted = admission.try_chat().unwrap();
        drop(admitted); // Cancellation before owners are split.
        assert_eq!(admission.available(), (4, 4));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let (_work, tx, _body) = admission.try_chat().unwrap().into_owners((), LIMITS);
            tx.try_send(b"queued").unwrap();
            panic!("test unwind");
        }));
        assert!(result.is_err());
        assert_eq!(admission.available(), (4, 4));
    }
}
