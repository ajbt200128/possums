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

use possums::{
    inference::{stream::StreamUsage, Message},
    render::{IncrementalRenderer, RenderOutcome, HISTORY_BLOCK_BYTES},
    web::{decode_continuation, BODY_LIMIT},
};
use sha2::{Digest, Sha256};

const TOKEN: &str = "ccccccccccccccccccccccccccccccccccccccccccc";

fn accepted_escape_history() -> Vec<Message> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    let json = format!(
        r#"[{{"role":"user","content":"{}"}},{{"role":"assistant","content":""}}]"#,
        "<".repeat(4 * 1024 * 1024)
    );
    let mut body = format!(
        "csrf={TOKEN}&token={TOKEN}&model=m&prompt=x&history_manifest=1.{:06}.{:08}",
        json.len().div_ceil(HISTORY_BLOCK_BYTES),
        json.len()
    );
    for (index, block) in json.as_bytes().chunks(HISTORY_BLOCK_BYTES).enumerate() {
        body.push_str(&format!("&h{index:06}={}", URL_SAFE_NO_PAD.encode(block)));
    }
    assert_eq!(body.len(), 5_602_549);
    assert!(body.len() < BODY_LIMIT);
    // Fixture JSON/raw form allocations die here, before either rendering pass.
    let accepted = decode_continuation(body.as_bytes()).unwrap();
    assert_eq!(accepted.prompt, "x");
    accepted.history
}

#[tokio::test]
async fn accepted_over_16_mib_startup_really_drains_through_bounded_http_body() {
    let history = accepted_escape_history();
    // Independent compatibility reference: count and hash only, never collect.
    let mut expected_bytes = 0;
    let mut expected_hash = Sha256::new();
    let reference = IncrementalRenderer::open(&history, "x", TOKEN, "m", |part| {
        expected_bytes += part.len();
        expected_hash.update(part.as_bytes());
        Ok(())
    })
    .unwrap();
    assert_eq!(reference.outcome(), RenderOutcome::Ready);
    assert_eq!(expected_bytes, 22_412_733);

    let (lane, lease) = lane();
    // Intentionally much smaller than 16 MiB. Outstanding HTTP frames, not just
    // channel entries, count against both limits; no test capture of the HTML.
    let limits = Limits {
        frames: 8,
        payload_bytes: 64 * 1024,
        chunk_bytes: 8192,
    };
    let (mut startup, mut body) = delivery(lease, limits, Duration::from_secs(30));
    let (filled, fill) = tokio::sync::oneshot::channel();
    let producer = tokio::task::spawn_blocking(move || {
        let mut filled = Some(filled);
        let mut frames = 0;
        let mut bytes = 0;
        let mut hash = Sha256::new();
        let mut sink = |part: &str| {
            startup
                .send_blocking(part.as_bytes())
                .map_err(|_| RenderOutcome::DeliveryFailed)?;
            bytes += part.len();
            hash.update(part.as_bytes());
            frames += 1;
            if frames == limits.frames {
                filled.take().unwrap().send(()).unwrap();
            }
            Ok(())
        };
        let mut renderer =
            IncrementalRenderer::start(&history, "x", TOKEN, "m", &mut sink).unwrap();
        while renderer.emit_next_history(&mut sink).unwrap() {}
        renderer.finish_start(&mut sink).unwrap();
        assert_eq!(renderer.outcome(), RenderOutcome::Ready);
        let tx = startup.into_streaming();
        assert_eq!(tx.failure(), None);
        (bytes, hash.finalize(), tx.usage())
        // Sender closes here. The HTTP consumer must drain accepted frames to EOF.
    });
    // Deliberately withhold ALL body polling until the outstanding-frame limit
    // is reached. The ninth write cannot succeed until this consumer drops data.
    tokio::time::timeout(Duration::from_secs(5), fill)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(body.usage().frames, limits.frames);
    assert_eq!(body.usage().high_frames, limits.frames);
    let mut received = 0;
    let mut received_hash = Sha256::new();
    let mut frames = 0;
    tokio::time::timeout(Duration::from_secs(30), async {
        while let Some(frame) = body.frame().await {
            let bytes = frame.unwrap().into_data().unwrap();
            assert!(bytes.len() <= limits.chunk_bytes as usize);
            received += bytes.len();
            received_hash.update(&bytes);
            frames += 1;
            // This is a real slow async consumer. Keep the frame (and therefore
            // its credit) through a scheduler yield, then actually drop it.
            tokio::task::yield_now().await;
            drop(bytes);
        }
    })
    .await
    .unwrap();
    let (produced, produced_hash, high) = producer.await.unwrap();
    assert_eq!(received, expected_bytes);
    assert_eq!(produced, expected_bytes);
    assert_eq!(received_hash.finalize(), produced_hash);
    assert_eq!(expected_hash.finalize(), produced_hash);
    assert!(frames > limits.frames);
    assert!(received > 16 * 1024 * 1024);
    assert!(high.high_bytes <= limits.payload_bytes as usize);
    assert_eq!(high.high_frames, limits.frames);
    assert_eq!(body.usage().bytes, 0);
    assert_eq!(body.usage().frames, 0);
    assert_eq!(lane.available_permits(), 0); // EOF does not drop the body lease.
    drop(body);
    assert_eq!(lane.available_permits(), 1);
    println!(
        "accepted form=5602549 emitted={received} frames={frames} high_bytes={} high_frames={} sha256={produced_hash:x}",
        high.high_bytes, high.high_frames
    );
}

fn usage() -> StreamUsage {
    StreamUsage {
        input_tokens: 1,
        output_tokens: 1,
        total_tokens: 2,
    }
}

#[tokio::test]
async fn small_rendered_responses_are_byte_exact_through_real_delivery() {
    for answer in ["", "hello", "\0\r\n\"'\\🐾é</pre><script>bad()</script>"] {
        let history = vec![
            Message {
                role: "user".into(),
                content: "prior<&🐾".into(),
            },
            Message {
                role: "assistant".into(),
                content: String::new(),
            },
        ];
        let mut expected = Vec::new();
        let mut capture = |part: &str| {
            assert!(expected.len() + part.len() <= 8192); // Small fixture ONLY.
            expected.extend_from_slice(part.as_bytes());
            Ok(())
        };
        let mut renderer =
            IncrementalRenderer::open(&history, "x", TOKEN, "m", &mut capture).unwrap();
        renderer.delta(answer, &mut capture);
        assert_eq!(
            renderer.complete(Ok(usage()), || Some(TOKEN.into()), &mut capture),
            RenderOutcome::Ready
        );
        let (_, lease) = lane();
        let (mut startup, mut body) = delivery(
            lease,
            Limits {
                frames: 64,
                payload_bytes: 8192,
                chunk_bytes: 8192,
            },
            Duration::from_secs(5),
        );
        let producer = tokio::task::spawn_blocking(move || {
            let mut sink = |part: &str| {
                startup
                    .send_blocking(part.as_bytes())
                    .map_err(|_| RenderOutcome::DeliveryFailed)
            };
            let mut renderer =
                IncrementalRenderer::start(&history, "x", TOKEN, "m", &mut sink).unwrap();
            while renderer.emit_next_history(&mut sink).unwrap() {}
            renderer.finish_start(&mut sink).unwrap();
            let mut tx = startup.into_streaming();
            let mut sink = |part: &str| {
                tx.try_send(part.as_bytes())
                    .map_err(|_| RenderOutcome::DeliveryFailed)
            };
            renderer.delta(answer, &mut sink);
            assert_eq!(
                renderer.complete(Ok(usage()), || Some(TOKEN.into()), &mut sink),
                RenderOutcome::Ready
            );
        });
        let mut actual = Vec::new();
        while let Some(frame) = body.frame().await {
            let data = frame.unwrap().into_data().unwrap();
            assert!(actual.len() + data.len() <= 8192);
            actual.extend_from_slice(&data);
        }
        producer.await.unwrap();
        assert_eq!(actual, expected);
        assert!(body.usage().high_bytes <= 8192);
        assert_eq!(body.usage().bytes, 0);
    }
}

#[tokio::test]
async fn failed_startup_renderer_still_accepts_future_upstream_completion() {
    for disconnected in [false, true] {
        let (_, lease) = lane();
        // Zero deadline deterministically expires, independent of scheduler speed.
        let (mut startup, body) = delivery(
            lease,
            Limits {
                frames: 2,
                payload_bytes: 8192,
                chunk_bytes: 8192,
            },
            Duration::ZERO,
        );
        let body = if disconnected {
            drop(body);
            None
        } else {
            Some(body)
        };
        let result = tokio::task::spawn_blocking(move || {
            let mut sink = |part: &str| {
                startup
                    .send_blocking(part.as_bytes())
                    .map_err(|_| RenderOutcome::DeliveryFailed)
            };
            let mut renderer = IncrementalRenderer::start(&[], "x", TOKEN, "m", &mut sink).unwrap();
            assert!(!renderer.emit_next_history(&mut sink).unwrap());
            renderer.finish_start(&mut sink).unwrap();
            assert_eq!(renderer.outcome(), RenderOutcome::DeliveryFailed);
            let tx = startup.into_streaming();
            assert_eq!(
                tx.failure(),
                Some(if disconnected {
                    DeliveryError::Closed
                } else {
                    DeliveryError::TimedOut
                })
            );
            // Delivery failure is not an upstream terminal result. The worker can
            // continue using the renderer and reach authenticated usage later.
            for _ in 0..1000 {
                renderer.delta("future inference", |_| panic!("must remain detached"));
            }
            renderer.complete(
                Ok(usage()),
                || panic!("no continuation"),
                |_| panic!("no delivery"),
            )
        })
        .await
        .unwrap();
        assert_eq!(result, RenderOutcome::DeliveryFailed);
        drop(body);
    }
}
