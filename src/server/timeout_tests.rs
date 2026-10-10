//! Synthetic HTTP/1 builder tests; no provider, plaintext logs or telemetry export.
use super::*;
use std::{
    convert::Infallible,
    sync::atomic::{AtomicUsize, Ordering},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test(start_paused = true)]
async fn active_body_outlives_connection_age_and_pipelining_is_closed() {
    let (mut client, server) = tokio::io::duplex(4096);
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let (release, ready) = tokio::sync::oneshot::channel();
    let ready = Arc::new(std::sync::Mutex::new(Some(ready)));
    let service = hyper::service::service_fn(move |_| {
        seen.fetch_add(1, Ordering::SeqCst);
        let ready = ready.lock().unwrap().take().unwrap();
        async move {
            let body = Body::from_stream(futures_util::stream::once(async {
                ready.await.unwrap();
                Ok::<_, Infallible>(Bytes::from_static(b"terminal"))
            }));
            Ok::<_, Infallible>(Response::new(body))
        }
    });
    let worker = tokio::spawn(async move {
        connection_builder(HEADER_DEADLINE)
            .serve_connection(TokioIo::new(server), service)
            .await
            .unwrap();
    });
    // An attempted reuse/pipeline must not inherit any connection-age deadline.
    client.write_all(b"GET /first HTTP/1.1\r\nHost: fixture\r\n\r\nGET /second HTTP/1.1\r\nHost: fixture\r\n\r\n").await.unwrap();
    let mut headers = Vec::new();
    while !headers.ends_with(b"\r\n\r\n") {
        headers.push(client.read_u8().await.unwrap());
    }
    assert!(String::from_utf8(headers)
        .unwrap()
        .to_lowercase()
        .contains("connection: close"));
    tokio::time::advance(Duration::from_secs(1200)).await;
    assert!(!worker.is_finished());
    release.send(()).unwrap();
    let mut tail = Vec::new();
    client.read_to_end(&mut tail).await.unwrap();
    worker.await.unwrap();
    assert!(tail.windows(8).any(|bytes| bytes == b"terminal"));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn idle_initial_connection_still_has_header_deadline() {
    let (_client, server) = tokio::io::duplex(4096);
    let service = hyper::service::service_fn(|_| async {
        panic!("headers never completed");
        #[allow(unreachable_code)]
        Ok::<_, Infallible>(Response::new(Body::empty()))
    });
    let result = connection_builder(Duration::from_millis(20))
        .serve_connection(TokioIo::new(server), service)
        .await;
    assert!(result.unwrap_err().is_timeout());
}
