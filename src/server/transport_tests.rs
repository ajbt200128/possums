// Offline shared-ingress tests, nested to reuse the original lifecycle fixture.
use super::*;
use futures_util::stream;
use tokio::io::{AsyncRead, AsyncWrite, DuplexStream};
use tokio::sync::mpsc;

fn incoming<IO>() -> (
    mpsc::Sender<std::io::Result<IO>>,
    impl Stream<Item = std::io::Result<IO>>,
) {
    let (tx, rx) = mpsc::channel(1);
    let source = stream::unfold(rx, |mut rx| async {
        rx.recv().await.map(|item| (item, rx))
    });
    (tx, source)
}

async fn exchange(state: AppState, request: &[u8]) -> Vec<u8> {
    let (tx, source) = incoming();
    let (mut client, io) = tokio::io::duplex(64 * 1024);
    let server = tokio::spawn(serve_incoming_until(
        source,
        state.clone(),
        HEADER_DEADLINE,
        std::future::pending(),
    ));
    tx.send(Ok(io)).await.unwrap();
    client.write_all(request).await.unwrap();
    let mut response = Vec::new();
    bounded(client.read_to_end(&mut response)).await.unwrap();
    drop(tx); // Finite source exhaustion is orderly, not an accept failure.
    assert!(bounded(server).await.unwrap().is_ok());
    assert!(!state.lifecycle.is_serving());
    response
}

async fn chat_bytes(state: &AppState, session: &str) -> Vec<u8> {
    let body = chat(state, session, false)
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let mut bytes = format!("POST /v1/chat/completions HTTP/1.1\r\nHost: fixture\r\nAuthorization: Bearer {session}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes();
    bytes.extend_from_slice(&body);
    bytes
}

#[tokio::test]
async fn duplex_uses_existing_routes_auth_headers_and_body_pipeline() {
    for (path, status, authenticated) in [
        ("/healthz", "204", false),
        ("/attestation", "200", false),
        ("/v1/auth/challenge", "200", false),
        ("/v1/models", "401", false),
        ("/v1/models", "200", true),
        ("/login", "404", false),
    ] {
        let (state, probe, session) = fixture();
        let authorization = if authenticated {
            format!("Authorization: Bearer {session}\r\n")
        } else {
            String::new()
        };
        let response = exchange(
            state,
            format!("GET {path} HTTP/1.1\r\nHost: fixture\r\n{authorization}\r\n").as_bytes(),
        )
        .await;
        assert!(response.starts_with(format!("HTTP/1.1 {status}").as_bytes()));
        assert_eq!(probe.calls.load(Ordering::SeqCst), 0);
        if authenticated {
            assert_eq!(probe.verify_calls.load(Ordering::SeqCst), 1);
            assert_eq!(probe.catalog_calls.load(Ordering::SeqCst), 1);
        }
    }
    let (state, probe, _) = fixture();
    let body = "x".repeat(CONTROL_BODY_LIMIT + 1);
    let response = exchange(state, format!("GET /v1/auth/challenge HTTP/1.1\r\nHost: fixture\r\nContent-Length: {}\r\n\r\n{body}GET /attestation HTTP/1.1\r\nHost: fixture\r\n\r\n", body.len()).as_bytes()).await;
    assert!(response.starts_with(b"HTTP/1.1 413"));
    assert!(response
        .windows(b"body_limit".len())
        .any(|s| s == b"body_limit"));
    assert_eq!(
        response
            .windows(b"HTTP/1.1".len())
            .filter(|s| *s == b"HTTP/1.1")
            .count(),
        1
    );
    assert_eq!(probe.verify_calls.load(Ordering::SeqCst), 0);
    assert_eq!(probe.document_calls.load(Ordering::SeqCst), 0);

    let (state, probe, _) = fixture();
    let headers = (0..MAX_HEADERS)
        .map(|i| format!("x-{i}: x\r\n"))
        .collect::<String>();
    let response = exchange(
        state,
        format!("GET /attestation HTTP/1.1\r\nHost: fixture\r\n{headers}\r\n").as_bytes(),
    )
    .await;
    assert!(response.starts_with(b"HTTP/1.1 431"));
    assert_eq!(probe.verify_calls.load(Ordering::SeqCst), 0);
    let (state, probe, _) = fixture();
    let response = exchange(
        state,
        format!(
            "GET /attestation HTTP/1.1\r\nHost: fixture\r\nx-large: {}\r\n\r\n",
            "x".repeat(MAX_HEADER_BYTES + 1)
        )
        .as_bytes(),
    )
    .await;
    assert!(response.starts_with(b"HTTP/1.1 431"));
    assert_eq!(probe.verify_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test(start_paused = true)]
async fn generic_ingress_retains_the_initial_header_deadline() {
    let (state, probe, _) = fixture();
    let (tx, source) = incoming();
    let (mut client, io) = tokio::io::duplex(4096);
    let server = tokio::spawn(serve_incoming_until(
        source,
        state,
        Duration::from_millis(20),
        std::future::pending(),
    ));
    tx.send(Ok(io)).await.unwrap();
    client
        .write_all(b"GET /attestation HTTP/1.1\r\n")
        .await
        .unwrap();
    let mut response = Vec::new();
    bounded(client.read_to_end(&mut response)).await.unwrap();
    assert!(response.is_empty());
    drop(tx);
    assert!(bounded(server).await.unwrap().is_ok());
    assert_eq!(probe.verify_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn real_duplex_pending_and_backpressure_are_not_disconnect_but_eof_is() {
    let (io, mut peer) = tokio::io::duplex(1);
    let disconnect = crate::stream_owner::Disconnect::default();
    let io_failed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut io = DisconnectIo {
        stream: io,
        disconnect: disconnect.clone(),
        io_failed: io_failed.clone(),
    };
    let mut bytes = [0; 1];
    {
        let read = io.read(&mut bytes);
        tokio::pin!(read);
        assert!(poll!(&mut read).is_pending());
    }
    io.write_all(b"a").await.unwrap();
    {
        let write = io.write_all(b"b");
        tokio::pin!(write);
        assert!(poll!(&mut write).is_pending()); // The actual duplex buffer is full.
        assert!(disconnect.cancelled().now_or_never().is_none());
        assert_eq!(peer.read_u8().await.unwrap(), b'a');
        bounded(write).await.unwrap();
    }
    assert_eq!(peer.read_u8().await.unwrap(), b'b');
    assert!(disconnect.cancelled().now_or_never().is_none());
    peer.shutdown().await.unwrap();
    assert_eq!(io.read(&mut bytes).await.unwrap(), 0);
    bounded(disconnect.cancelled()).await;
    assert!(!io_failed.load(Ordering::Acquire));
}

#[tokio::test]
async fn duplex_eof_cancels_the_original_reserved_preflight_without_replay() {
    let (state, probe, session) = fixture();
    let (pause, gate) = checkpoint();
    *probe.tokenizer.lock().unwrap() = Some(pause);
    let (tx, source) = incoming();
    let (mut client, io) = tokio::io::duplex(4096);
    let server = tokio::spawn(serve_incoming_until(
        source,
        state.clone(),
        HEADER_DEADLINE,
        std::future::pending(),
    ));
    tx.send(Ok(io)).await.unwrap();
    client
        .write_all(&chat_bytes(&state, &session).await)
        .await
        .unwrap();
    bounded(gate.reached).await.unwrap();
    assert_eq!(state.accounting.snapshot("demo").unwrap().in_flight, 1);
    drop(client);
    drop(tx);
    assert!(bounded(server).await.unwrap().is_ok());
    assert!(gate.release.send(()).is_err());
    assert_eq!(probe.calls.load(Ordering::SeqCst), 0);
    assert_eq!(probe.tokenize_calls.load(Ordering::SeqCst), 1);
    assert_eq!(state.accounting.available("demo"), Some(1000));
    assert_eq!(state.accounting.snapshot("demo").unwrap().in_flight, 0);
}

// Observe actual Hyper writes into a full duplex buffer, without fabricating
// Pending or adding a production observer.
struct WriteProbe {
    io: DuplexStream,
    pending: Option<tokio::sync::oneshot::Sender<()>>,
}
impl AsyncRead for WriteProbe {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.io).poll_read(cx, buf)
    }
}
impl AsyncWrite for WriteProbe {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let result = Pin::new(&mut self.io).poll_write(cx, bytes);
        if result.is_pending() {
            if let Some(pending) = self.pending.take() {
                let _ = pending.send(());
            }
        }
        result
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.io).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.io).poll_shutdown(cx)
    }
}

#[tokio::test]
async fn finite_source_drains_backpressured_generation_and_settles_once() {
    let (state, probe, session) = fixture();
    let (pause, gate) = checkpoint();
    *probe.generation.lock().unwrap() = Some(pause);
    let (published, shutdown_published) = tokio::sync::oneshot::channel();
    *state.server_hooks.shutdown_published.lock().unwrap() = Some(published);
    let (tx, source) = incoming();
    let (mut client, io) = tokio::io::duplex(64);
    let (pending, write_pending) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(serve_incoming_until(
        source,
        state.clone(),
        HEADER_DEADLINE,
        std::future::pending(),
    ));
    tx.send(Ok(WriteProbe {
        io,
        pending: Some(pending),
    }))
    .await
    .unwrap();
    probe.emit_progress.store(true, Ordering::SeqCst);
    bounded(client.write_all(&chat_bytes(&state, &session).await))
        .await
        .unwrap();
    bounded(gate.reached).await.unwrap();
    bounded(write_pending).await.unwrap();
    drop(tx);
    bounded(shutdown_published).await.unwrap();
    assert!(state.lifecycle.try_admit().is_none());
    assert!(!server.is_finished());
    assert_eq!(state.accounting.snapshot("demo").unwrap().in_flight, 1);
    gate.release.send(()).unwrap();
    let mut response = Vec::new();
    bounded(client.read_to_end(&mut response)).await.unwrap();
    assert!(response.starts_with(b"HTTP/1.1 200"));
    assert!(response.windows(b"[DONE]".len()).any(|s| s == b"[DONE]"));
    assert!(bounded(server).await.unwrap().is_ok());
    let balance = state.accounting.snapshot("demo").unwrap();
    assert_eq!((balance.in_flight, balance.completed_requests), (0, 1));
    assert!(balance.available_microunits < 1000);
    assert_eq!(probe.calls.load(Ordering::SeqCst), 1);
    assert_eq!(state.active_lanes(), [0, 0, 0]);
}

#[tokio::test]
async fn finite_source_waits_for_detached_original_owner_after_connection_retires() {
    let (state, probe, session) = fixture();
    let (pause, preflight) = checkpoint();
    *state.preflight_hooks.after.lock().unwrap() = Some(pause);
    let (pause, generation) = checkpoint();
    *probe.generation.lock().unwrap() = Some(pause);
    let (finished, connection_finished) = tokio::sync::oneshot::channel();
    *state.server_hooks.connection_finished.lock().unwrap() = Some(finished);
    let (published, shutdown_published) = tokio::sync::oneshot::channel();
    *state.server_hooks.shutdown_published.lock().unwrap() = Some(published);
    let (tx, source) = incoming();
    let (mut client, io) = tokio::io::duplex(4096);
    let server = tokio::spawn(serve_incoming_until(
        source,
        state.clone(),
        HEADER_DEADLINE,
        std::future::pending(),
    ));
    tx.send(Ok(io)).await.unwrap();
    client
        .write_all(&chat_bytes(&state, &session).await)
        .await
        .unwrap();
    bounded(generation.reached).await.unwrap();
    bounded(preflight.reached).await.unwrap();
    drop(client);
    bounded(connection_finished).await.unwrap();
    bounded(async {
        while state.accounting.snapshot("demo").unwrap().in_flight != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await;
    drop(tx);
    bounded(shutdown_published).await.unwrap();
    assert!(!server.is_finished()); // Connection and reservation retired, root did not.
    assert!(generation.release.send(()).is_err());
    preflight.release.send(()).unwrap();
    assert!(bounded(server).await.unwrap().is_ok());
    let balance = state.accounting.snapshot("demo").unwrap();
    assert_eq!(
        (
            balance.in_flight,
            balance.completed_requests,
            balance.available_microunits
        ),
        (0, 1, 1000)
    );
    assert_eq!(probe.calls.load(Ordering::SeqCst), 1);
    assert_eq!(state.active_lanes(), [0, 0, 0]);
}

// Only this mixed-source fixture needs type erasure; production remains generic.
trait TestIo: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> TestIo for T {}
type MixedIo = Box<dyn TestIo>;

#[tokio::test]
async fn tcp_and_duplex_share_one_connection_limit_and_shutdown_drain() {
    let (state, probe, _) = fixture();
    let metrics = Arc::new(crate::telemetry::AggregateMetrics::new(
        crate::telemetry::Deployment::IsolatedSynthetic,
        crate::telemetry::SystemClock::default(),
    ));
    metrics.prime_synthetic_application_window();
    let state = state.with_telemetry(Some(metrics.clone()));
    let (pause, gate) = checkpoint();
    *state.server_hooks.connection_start.lock().unwrap() = Some(pause);
    let (published, shutdown_published) = tokio::sync::oneshot::channel();
    *state.server_hooks.shutdown_published.lock().unwrap() = Some(published);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let tcp = stream::unfold(listener, |listener| async {
        let accepted = listener
            .accept()
            .await
            .map(|(io, _)| Box::new(io) as MixedIo);
        Some((accepted, listener))
    });
    let (tx, synthetic) = incoming::<MixedIo>();
    let seen = Arc::new(AtomicUsize::new(0));
    let count = seen.clone();
    let source = stream::select(Box::pin(tcp), Box::pin(synthetic)).inspect(move |_| {
        count.fetch_add(1, Ordering::SeqCst);
    });
    let (stop, stopping) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(serve_incoming_until(
        source,
        state.clone(),
        Duration::from_secs(600),
        async {
            let _ = stopping.await;
        },
    ));
    let mut duplex_clients = Vec::new();
    let mut tcp_clients = Vec::new();
    for _ in 0..MAX_CONNECTIONS / 2 {
        let (client, io) = tokio::io::duplex(4096);
        tx.send(Ok(Box::new(io))).await.unwrap();
        duplex_clients.push(client);
        tcp_clients.push(tokio::net::TcpStream::connect(address).await.unwrap());
    }
    bounded(gate.reached).await.unwrap();
    bounded(async {
        while seen.load(Ordering::SeqCst) != MAX_CONNECTIONS {
            tokio::task::yield_now().await;
        }
    })
    .await;
    let (mut rejected, io) = tokio::io::duplex(4096);
    tx.send(Ok(Box::new(io))).await.unwrap();
    assert_eq!(bounded(rejected.read(&mut [0; 1])).await.unwrap(), 0);
    let mut rejected = tokio::net::TcpStream::connect(address).await.unwrap();
    assert_eq!(bounded(rejected.read(&mut [0; 1])).await.unwrap(), 0);
    assert_eq!(
        metrics.interface_error_count(InterfaceError::ConnectionCapacity),
        Some(2)
    );
    stop.send(()).unwrap();
    bounded(shutdown_published).await.unwrap();
    assert!(!server.is_finished()); // The originally accepted paused owner remains.
    assert!(state.lifecycle.try_admit().is_none());
    gate.release.send(()).unwrap();
    assert!(bounded(server).await.unwrap().is_ok());
    for mut client in duplex_clients {
        assert_eq!(bounded(client.read(&mut [0; 1])).await.unwrap(), 0);
    }
    for mut client in tcp_clients {
        assert_eq!(bounded(client.read(&mut [0; 1])).await.unwrap(), 0);
    }
    assert_eq!(probe.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn source_exhaustion_error_and_shutdown_bias_keep_existing_semantics() {
    let (state, _, _) = fixture();
    assert!(bounded(serve_incoming_until(
        stream::empty::<std::io::Result<DuplexStream>>(),
        state.clone(),
        HEADER_DEADLINE,
        std::future::pending()
    ))
    .await
    .is_ok());
    assert!(state.lifecycle.try_admit().is_none());
    let (state, _, _) = fixture();
    let result = bounded(serve_incoming_until(
        stream::iter([Err::<DuplexStream, _>(std::io::Error::other(
            "HOSTILE_ACCEPT_CANARY",
        ))]),
        state.clone(),
        HEADER_DEADLINE,
        std::future::pending(),
    ))
    .await
    .unwrap_err();
    assert_eq!(
        result
            .get_ref()
            .and_then(|source| source.downcast_ref::<ServerFailure>()),
        Some(&ServerFailure::Accept)
    );
    assert!(!result.to_string().contains("HOSTILE_ACCEPT_CANARY"));
    assert!(state.lifecycle.try_admit().is_none());
    let (state, _, _) = fixture();
    let source = stream::poll_fn(|_| -> Poll<Option<std::io::Result<DuplexStream>>> {
        panic!("ready shutdown must win before source polling")
    });
    assert!(bounded(serve_incoming_until(
        source,
        state,
        HEADER_DEADLINE,
        std::future::ready(())
    ))
    .await
    .is_ok());
}
