use std::{
    num::NonZeroUsize,
    sync::atomic::{AtomicUsize, Ordering},
};

use futures::{future::pending, poll, stream, StreamExt};
use possums_tor_arti_spike::{forward_opaque, run_bounded, serve_http, End, Failure};
use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

struct Active<'a>(&'a AtomicUsize);
impl Drop for Active<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn bounded_handshakes_and_active_work_drop_on_stop() {
    let active = AtomicUsize::new(0);
    let polled = AtomicUsize::new(0);
    let requests = stream::repeat(()).inspect(|_| {
        polled.fetch_add(1, Ordering::SeqCst);
    });
    let cancel = CancellationToken::new();
    let run = run_bounded(
        requests,
        NonZeroUsize::new(3).unwrap(),
        cancel.clone(),
        |_| {
            active.fetch_add(1, Ordering::SeqCst);
            let guard = Active(&active);
            async move {
                let _guard = guard;
                pending::<Result<(), Failure>>().await
            }
        },
    );
    tokio::pin!(run);
    assert!(poll!(&mut run).is_pending());
    assert_eq!(active.load(Ordering::SeqCst), 3);
    assert_eq!(polled.load(Ordering::SeqCst), 3);
    cancel.cancel();
    assert_eq!(run.await, Ok(End::Cancelled));
    assert_eq!(active.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn stopped_before_admission_does_not_call_handler() {
    let cancel = CancellationToken::new();
    let called = AtomicUsize::new(0);
    cancel.cancel();
    let result = run_bounded(
        stream::iter([()]),
        NonZeroUsize::new(1).unwrap(),
        cancel,
        |_| {
            called.fetch_add(1, Ordering::SeqCst);
            async { Ok(()) }
        },
    )
    .await;
    assert_eq!(result, Ok(End::Cancelled));
    assert_eq!(called.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn first_failure_is_preserved_and_drops_other_work() {
    let active = AtomicUsize::new(0);
    let cancel = CancellationToken::new();
    let run = run_bounded(
        stream::iter([false, true]),
        NonZeroUsize::new(2).unwrap(),
        cancel,
        |fail| {
            active.fetch_add(1, Ordering::SeqCst);
            let guard = Active(&active);
            async move {
                let _guard = guard;
                if fail {
                    Err(Failure::RendezvousAccept)
                } else {
                    pending().await
                }
            }
        },
    );
    assert_eq!(run.await, Err(Failure::RendezvousAccept));
    assert_eq!(active.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn stop_during_ready_handler_prevents_further_admission() {
    let cancel = CancellationToken::new();
    let called = AtomicUsize::new(0);
    let result = run_bounded(
        stream::repeat(()),
        NonZeroUsize::new(1).unwrap(),
        cancel.clone(),
        |_| {
            called.fetch_add(1, Ordering::SeqCst);
            cancel.cancel();
            async { Ok(()) }
        },
    )
    .await;
    assert_eq!(result, Ok(End::Cancelled));
    assert_eq!(called.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn observed_failure_wins_over_same_handler_stop() {
    let cancel = CancellationToken::new();
    let result = run_bounded(
        stream::iter([()]),
        NonZeroUsize::new(1).unwrap(),
        cancel.clone(),
        |_| {
            cancel.cancel();
            async { Err(Failure::StreamAccept) }
        },
    )
    .await;
    assert_eq!(result, Err(Failure::StreamAccept));
}

#[tokio::test]
async fn http_waiting_for_headers_is_owned_and_cancelled() {
    let (server, mut client) = duplex(64);
    let cancel = CancellationToken::new();
    let run = run_bounded(
        stream::iter([server]),
        NonZeroUsize::new(1).unwrap(),
        cancel.clone(),
        serve_http,
    );
    tokio::pin!(run);
    client.write_all(b"GET / HTTP/1.1\r\n").await.unwrap();
    assert!(poll!(&mut run).is_pending());
    cancel.cancel();
    assert_eq!(run.await, Ok(End::Cancelled));
    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    assert!(response.is_empty());
}

#[tokio::test]
async fn finite_source_exhausts_after_handlers_finish() {
    assert_eq!(
        run_bounded(
            stream::iter([()]),
            NonZeroUsize::new(1).unwrap(),
            CancellationToken::new(),
            |_| async { Ok(()) }
        )
        .await,
        Ok(End::Exhausted)
    );
}

#[tokio::test]
async fn http_adapter_serves_axum_on_local_duplex_without_a_listener() {
    let (server, mut client) = duplex(64);
    let exchange = async {
        client
            .write_all(b"GET / HTTP/1.1\r\nHost: synthetic.invalid\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut response = Vec::new();
        client.read_to_end(&mut response).await.unwrap();
        assert!(response.starts_with(b"HTTP/1.1 200 OK\r\n"));
        assert!(response.ends_with(b"synthetic-only"));
    };
    let (result, ()) = tokio::join!(serve_http(server), exchange);
    assert_eq!(result, Ok(()));
}

#[tokio::test]
async fn opaque_bridge_preserves_non_http_bytes_and_half_close() {
    let (onion, mut client) = duplex(8);
    let (endpoint, mut upstream) = duplex(8);
    let exchange = async {
        client
            .write_all(&[0x16, 0x03, 0x03, 0xff, 0x00])
            .await
            .unwrap();
        client.shutdown().await.unwrap();
        let mut received = Vec::new();
        upstream.read_to_end(&mut received).await.unwrap();
        assert_eq!(received, [0x16, 0x03, 0x03, 0xff, 0x00]);
        // EOF in one direction must not discard the reverse response.
        upstream.write_all(b"opaque-reply").await.unwrap();
        upstream.shutdown().await.unwrap();
        let mut reply = Vec::new();
        client.read_to_end(&mut reply).await.unwrap();
        assert_eq!(reply, b"opaque-reply");
    };
    let (result, ()) = tokio::join!(forward_opaque(onion, endpoint), exchange);
    assert_eq!(result, Ok(()));
}

#[tokio::test]
async fn bridge_backpressure_is_pending_not_disconnect_and_stop_drops_both_sides() {
    let (onion, mut client) = duplex(16);
    let (endpoint, mut upstream) = duplex(16);
    let cancel = CancellationToken::new();
    let run = run_bounded(
        stream::iter([(onion, endpoint)]),
        NonZeroUsize::new(1).unwrap(),
        cancel.clone(),
        |(a, b)| forward_opaque(a, b),
    );
    tokio::pin!(run);
    let bytes = vec![0x55; 64 * 1024];
    {
        let upload = client.write_all(&bytes);
        tokio::pin!(upload);
        // Cooperatively poll both: a stalled reader fills only bounded buffers.
        for _ in 0..16 {
            assert!(poll!(&mut upload).is_pending());
            assert!(poll!(&mut run).is_pending());
        }
    }
    cancel.cancel();
    assert_eq!(run.await, Ok(End::Cancelled));
    let mut drained = Vec::new();
    upstream.read_to_end(&mut drained).await.unwrap();
    assert!(drained.len() <= 16);
    assert!(client.write_all(b"after-stop").await.is_err());
}

#[tokio::test]
async fn abrupt_bridge_peer_loss_has_closed_content_free_failure() {
    let (onion, mut client) = duplex(16);
    let (endpoint, upstream) = duplex(16);
    drop(upstream);
    client.write_all(b"HOSTILE_SECRET").await.unwrap();
    assert_eq!(
        forward_opaque(onion, endpoint).await,
        Err(Failure::OpaqueCopy)
    );
    assert!(!Failure::OpaqueCopy
        .diagnostic()
        .contains("HOSTILE_SECRET_SENTINEL"));
}
