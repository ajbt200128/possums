// Offline physical-transport fixtures only: no Arti, TLS verdict or live dial.
use super::*;
use crate::tor_ingress::{BridgeFixture, BridgeMetadata, Failure, ReportOwner};
use futures_util::{poll, stream};
use tokio::{
    io::{AsyncRead, AsyncWrite, DuplexStream},
    sync::mpsc,
};

type Io = Box<dyn crate::tor_ingress::TokioIo>;

fn reporting() -> (ReportOwner, Arc<Mutex<Vec<Failure>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let output = seen.clone();
    (
        ReportOwner::new(Arc::new(move |failure| {
            output.lock().unwrap().push(failure)
        })),
        seen,
    )
}
fn incoming() -> (
    mpsc::Sender<std::io::Result<Incoming<Io>>>,
    impl Stream<Item = std::io::Result<Incoming<Io>>>,
) {
    let (tx, rx) = mpsc::channel(1);
    (
        tx,
        stream::unfold(rx, |mut rx| async {
            rx.recv().await.map(|item| (item, rx))
        }),
    )
}
fn bridge(
    io: impl crate::tor_ingress::TokioIo + 'static,
    report: ReportOwner,
    fixture: BridgeFixture,
) -> Incoming<Io> {
    Incoming::OpaqueTorBridge(Box::new(io), BridgeMetadata::fixture(report, fixture))
}

// The probe observes genuine maintained-duplex backpressure and pre-admission
// polling; hostile I/O faults are synthetic and never exported or diagnosed raw.
struct ProbeIo {
    io: DuplexStream,
    reads: Arc<AtomicUsize>,
    pending: Option<tokio::sync::oneshot::Sender<()>>,
    fail_read: bool,
    fail_write: bool,
}
impl ProbeIo {
    fn new(io: DuplexStream) -> Self {
        Self {
            io,
            reads: Arc::default(),
            pending: None,
            fail_read: false,
            fail_write: false,
        }
    }
}
impl AsyncRead for ProbeIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let count = self.reads.fetch_add(1, Ordering::SeqCst);
        if self.fail_read && count > 0 {
            return Poll::Ready(Err(std::io::Error::other(
                "HOSTILE_IO/credential/identifier",
            )));
        }
        Pin::new(&mut self.io).poll_read(cx, buf)
    }
}
impl AsyncWrite for ProbeIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        if self.fail_write {
            return Poll::Ready(Err(std::io::Error::other(
                "HOSTILE_IO/credential/identifier",
            )));
        }
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
async fn ready_shutdown_neither_polls_nor_dials_bridge() {
    let (state, probe, _) = fixture();
    let (report, seen) = reporting();
    let (io, _client) = tokio::io::duplex(64);
    let io = ProbeIo::new(io);
    let reads = io.reads.clone();
    let dials = Arc::new(AtomicUsize::new(0));
    let source = stream::once(std::future::ready(Ok(bridge(
        io,
        report,
        BridgeFixture::Fail(dials.clone()),
    ))));
    assert!(
        serve_connections_until(source, state, HEADER_DEADLINE, std::future::ready(()))
            .await
            .is_ok()
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(dials.load(Ordering::SeqCst), 0);
    assert!(seen.lock().unwrap().is_empty());
    assert_eq!(probe.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn exactly_64_mixed_physical_connections_reject_without_poll_or_dial() {
    let (state, _, _) = fixture();
    let (report, seen) = reporting();
    let mut accepted = Vec::new();
    let mut clients = Vec::new();
    let mut upstreams = Vec::new();
    let dials = Arc::new(AtomicUsize::new(0));
    for index in 0..MAX_CONNECTIONS {
        let (mut client, io) = tokio::io::duplex(64);
        if index % 2 == 0 {
            let (upstream, shim) = tokio::io::duplex(64);
            client.write_all(&[0x16]).await.unwrap();
            upstreams.push(shim);
            accepted.push(Ok(bridge(
                io,
                report.clone(),
                BridgeFixture::Duplex(upstream, dials.clone()),
            )));
        } else {
            accepted.push(Ok(Incoming::Http(Box::new(io) as Io)));
        }
        clients.push(client);
    }
    let (mut rejected, io) = tokio::io::duplex(64);
    let io = ProbeIo::new(io);
    let reads = io.reads.clone();
    let rejected_dials = Arc::new(AtomicUsize::new(0));
    accepted.push(Ok(bridge(
        io,
        report,
        BridgeFixture::Fail(rejected_dials.clone()),
    )));
    let (stop, shutdown) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(serve_connections_until(
        stream::iter(accepted).chain(stream::pending()),
        state,
        Duration::from_secs(600),
        async {
            let _ = shutdown.await;
        },
    ));
    assert_eq!(
        bounded(rejected.read_u8()).await.unwrap_err().kind(),
        std::io::ErrorKind::UnexpectedEof
    );
    for shim in &mut upstreams {
        assert_eq!(bounded(shim.read_u8()).await.unwrap(), 0x16);
    }
    assert_eq!(dials.load(Ordering::SeqCst), MAX_CONNECTIONS / 2);
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(rejected_dials.load(Ordering::SeqCst), 0);
    assert!(seen.lock().unwrap().is_empty());
    stop.send(()).unwrap();
    drop(clients);
    drop(upstreams);
    assert!(bounded(server).await.unwrap().is_ok());
}

#[tokio::test(start_paused = true)]
async fn first_byte_and_connect_share_ten_second_deadline() {
    for first_after in [None, Some(0), Some(9)] {
        let (state, _, _) = fixture();
        let (report, seen) = reporting();
        let (mut client, io) = tokio::io::duplex(64);
        let dials = Arc::new(AtomicUsize::new(0));
        if first_after == Some(0) {
            client.write_all(&[0x16]).await.unwrap();
        }
        let server = serve_connections_until(
            stream::iter([Ok(bridge(
                io,
                report,
                BridgeFixture::Pending(dials.clone()),
            ))]),
            state,
            HEADER_DEADLINE,
            std::future::pending(),
        );
        tokio::pin!(server);
        assert!(poll!(&mut server).is_pending());
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(9)).await;
        if first_after == Some(9) {
            client.write_all(&[0x16]).await.unwrap();
            tokio::task::yield_now().await;
        }
        assert!(poll!(&mut server).is_pending());
        tokio::time::advance(Duration::from_secs(1)).await;
        assert!(server.await.is_ok());
        assert_eq!(
            dials.load(Ordering::SeqCst),
            usize::from(first_after.is_some())
        );
        assert_eq!(*seen.lock().unwrap(), vec![Failure::BridgeDeadline]);
    }
}

#[tokio::test]
async fn opaque_bytes_are_forwarded_both_ways_without_router_or_tls_parsing() {
    let (state, probe, _) = fixture();
    let (report, seen) = reporting();
    let (mut client, io) = tokio::io::duplex(64);
    let (upstream, mut shim) = tokio::io::duplex(64);
    let dials = Arc::new(AtomicUsize::new(0));
    let server = tokio::spawn(serve_connections_until(
        stream::iter([Ok(bridge(
            io,
            report,
            BridgeFixture::Duplex(upstream, dials.clone()),
        ))]),
        state,
        HEADER_DEADLINE,
        std::future::pending(),
    ));
    let bytes = b"\x16\x03\x01opaque\x00\xffGET /not-http";
    client.write_all(bytes).await.unwrap();
    let mut received = vec![0; bytes.len()];
    bounded(shim.read_exact(&mut received)).await.unwrap();
    assert_eq!(received, bytes);
    shim.write_all(b"\x16\x03opaque-response").await.unwrap();
    let mut received = vec![0; b"\x16\x03opaque-response".len()];
    bounded(client.read_exact(&mut received)).await.unwrap();
    assert_eq!(received, b"\x16\x03opaque-response");
    drop(client);
    assert!(bounded(server).await.unwrap().is_ok());
    assert_eq!(dials.load(Ordering::SeqCst), 1);
    assert!(seen.lock().unwrap().is_empty());
    assert_eq!(probe.calls.load(Ordering::SeqCst), 0);
    assert_eq!(probe.verify_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test(start_paused = true)]
async fn bidirectional_backpressure_retains_lease_and_drain_without_age_timeout() {
    let (state, _, _) = fixture();
    let (report, seen) = reporting();
    let (mut client, io) = tokio::io::duplex(1);
    let (upstream, mut shim) = tokio::io::duplex(1);
    let (pending, blocked) = tokio::sync::oneshot::channel();
    let mut io = ProbeIo::new(io);
    io.pending = Some(pending);
    let (tx, source) = incoming();
    let (stop, shutdown) = tokio::sync::oneshot::channel();
    let (published, quiesced) = tokio::sync::oneshot::channel();
    *state.server_hooks.shutdown_published.lock().unwrap() = Some(published);
    let server = tokio::spawn(serve_connections_until(
        source,
        state.clone(),
        Duration::from_secs(600),
        async {
            let _ = shutdown.await;
        },
    ));
    tx.send(Ok(bridge(
        io,
        report.clone(),
        BridgeFixture::Duplex(upstream, Arc::default()),
    )))
    .await
    .unwrap();
    client.write_all(b"a").await.unwrap();
    assert_eq!(shim.read_u8().await.unwrap(), b'a');
    shim.write_all(b"xy").await.unwrap();
    blocked.await.unwrap();
    // Reverse traffic still flows while downstream delivery is backpressured.
    client.write_all(b"b").await.unwrap();
    assert_eq!(shim.read_u8().await.unwrap(), b'b');
    // Fill the remaining physical slots. A blocked copy must still hold its
    // connection lease: the next bridge is rejected without even polling I/O.
    let mut idle = Vec::new();
    for _ in 1..MAX_CONNECTIONS {
        let (client, io) = tokio::io::duplex(1);
        idle.push(client);
        tx.send(Ok(Incoming::Http(Box::new(io)))).await.unwrap();
    }
    let (mut rejected, io) = tokio::io::duplex(1);
    let io = ProbeIo::new(io);
    let reads = io.reads.clone();
    let dials = Arc::new(AtomicUsize::new(0));
    tx.send(Ok(bridge(io, report, BridgeFixture::Fail(dials.clone()))))
        .await
        .unwrap();
    assert_eq!(
        rejected.read_u8().await.unwrap_err().kind(),
        std::io::ErrorKind::UnexpectedEof
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(dials.load(Ordering::SeqCst), 0);
    stop.send(()).unwrap();
    drop(idle);
    quiesced.await.unwrap();
    assert!(!state.lifecycle.is_serving());
    tokio::time::advance(Duration::from_secs(3600)).await;
    assert!(!server.is_finished());
    assert_eq!(client.read_u8().await.unwrap(), b'x');
    assert_eq!(client.read_u8().await.unwrap(), b'y');
    drop(client);
    assert!(server.await.unwrap().is_ok());
    assert!(seen.lock().unwrap().is_empty());
}

#[tokio::test(start_paused = true)]
async fn both_copy_directions_can_pause_without_truncating_accepted_bridge() {
    let (state, _, _) = fixture();
    let (report, seen) = reporting();
    let (mut client, io) = tokio::io::duplex(1);
    let (upstream, mut shim) = tokio::io::duplex(1);
    let (pending, blocked) = tokio::sync::oneshot::channel();
    let mut io = ProbeIo::new(io);
    io.pending = Some(pending);
    let (tx, source) = incoming();
    let (stop, shutdown) = tokio::sync::oneshot::channel();
    let (published, quiesced) = tokio::sync::oneshot::channel();
    *state.server_hooks.shutdown_published.lock().unwrap() = Some(published);
    let server = tokio::spawn(serve_connections_until(
        source,
        state,
        HEADER_DEADLINE,
        async {
            let _ = shutdown.await;
        },
    ));
    tx.send(Ok(bridge(
        io,
        report,
        BridgeFixture::Duplex(upstream, Arc::default()),
    )))
    .await
    .unwrap();
    client.write_all(b"a").await.unwrap();
    assert_eq!(shim.read_u8().await.unwrap(), b'a');
    shim.write_all(b"xy").await.unwrap();
    blocked.await.unwrap();
    {
        // More than Tokio's bounded copy buffers in each direction, without
        // either peer reading. Poll the two local writers, not invented Pending.
        let bytes = vec![0x16; 64 * 1024];
        let writing = async { tokio::join!(client.write_all(&bytes), shim.write_all(&bytes)) };
        tokio::pin!(writing);
        assert!(poll!(&mut writing).is_pending());
        tokio::task::yield_now().await;
        assert!(poll!(&mut writing).is_pending());
        stop.send(()).unwrap();
        quiesced.await.unwrap();
        tokio::time::advance(Duration::from_secs(3600)).await;
        assert!(poll!(&mut writing).is_pending());
        assert!(!server.is_finished());
    }
    drop(client); // Actual downstream write failure wakes blocked delivery.
    assert!(server.await.unwrap().is_ok());
    assert!(seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn eof_before_first_byte_does_not_dial_or_fabricate_failure() {
    let (state, _, _) = fixture();
    let (report, seen) = reporting();
    let (client, io) = tokio::io::duplex(1);
    let dials = Arc::new(AtomicUsize::new(0));
    drop(client);
    assert!(serve_connections_until(
        stream::iter([Ok(bridge(io, report, BridgeFixture::Fail(dials.clone())))]),
        state,
        HEADER_DEADLINE,
        std::future::pending()
    )
    .await
    .is_ok());
    assert_eq!(dials.load(Ordering::SeqCst), 0);
    assert!(seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn maintained_tcp_connect_refusal_reports_only_closed_failure() {
    let (state, _, _) = fixture();
    let (report, seen) = reporting();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let (mut client, io) = tokio::io::duplex(1);
    let dials = Arc::new(AtomicUsize::new(0));
    let server = tokio::spawn(serve_connections_until(
        stream::iter([Ok(bridge(
            io,
            report,
            BridgeFixture::Tcp(address, dials.clone()),
        ))]),
        state,
        HEADER_DEADLINE,
        std::future::pending(),
    ));
    client.write_all(b"a").await.unwrap();
    assert!(bounded(server).await.unwrap().is_ok());
    assert_eq!(dials.load(Ordering::SeqCst), 1);
    assert_eq!(*seen.lock().unwrap(), vec![Failure::BridgeConnect]);
}

#[tokio::test]
async fn observed_downstream_eof_drops_full_tcp_socket_while_upstream_reader_is_open() {
    let (state, _, _) = fixture();
    let (report, seen) = reporting();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (mut client, io) = tokio::io::duplex(64);
    let server = tokio::spawn(serve_connections_until(
        stream::iter([Ok(bridge(
            io,
            report,
            BridgeFixture::Tcp(listener.local_addr().unwrap(), Arc::default()),
        ))]),
        state,
        HEADER_DEADLINE,
        std::future::pending(),
    ));
    client.write_all(b"a").await.unwrap();
    let (mut shim, _) = bounded(listener.accept()).await.unwrap();
    assert_eq!(bounded(shim.read_u8()).await.unwrap(), b'a');
    // Keep the downstream reader alive: a write-side EOF alone must close the
    // entire upstream owner, not wait indefinitely for a shim response/EOF.
    client.shutdown().await.unwrap();
    assert!(bounded(server).await.unwrap().is_ok());
    assert_eq!(
        bounded(shim.read_u8()).await.unwrap_err().kind(),
        std::io::ErrorKind::UnexpectedEof
    );
    assert!(seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn bridge_connect_and_io_faults_keep_closed_categories_without_replay() {
    for mode in 0..4 {
        let (state, probe, _) = fixture();
        let (report, seen) = reporting();
        let (mut client, io) = tokio::io::duplex(64);
        let mut io = ProbeIo::new(io);
        io.fail_read = mode == 1;
        io.fail_write = mode == 2;
        let dials = Arc::new(AtomicUsize::new(0));
        let (upstream, mut shim) = tokio::io::duplex(64);
        let target = if mode == 0 {
            BridgeFixture::Fail(dials.clone())
        } else {
            BridgeFixture::Duplex(upstream, dials.clone())
        };
        let server = tokio::spawn(serve_connections_until(
            stream::iter([Ok(bridge(io, report, target))]),
            state,
            HEADER_DEADLINE,
            std::future::pending(),
        ));
        if mode == 3 {
            drop(shim); // Initial upstream write fails; no raw error survives.
        } else if mode == 2 {
            client.write_all(b"a").await.unwrap();
            assert_eq!(bounded(shim.read_u8()).await.unwrap(), b'a');
            shim.write_all(b"x").await.unwrap();
        }
        if mode != 2 {
            client.write_all(b"a").await.unwrap();
        }
        assert!(bounded(server).await.unwrap().is_ok());
        let expected = if mode == 0 {
            Failure::BridgeConnect
        } else {
            Failure::BridgeIo
        };
        assert_eq!(*seen.lock().unwrap(), vec![expected]);
        let text = expected.to_string();
        for required in [
            "code=",
            "stage=",
            "constraint=",
            "action=",
            "billing unknown",
            "not replayed",
            "may incur another charge",
        ] {
            assert!(text.contains(required));
        }
        assert!(!text.contains("HOSTILE"));
        assert_eq!(dials.load(Ordering::SeqCst), 1);
        assert_eq!(probe.calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn bridge_reporter_fault_stops_admission_preserves_first_failure_and_drains() {
    let (state, _, _) = fixture();
    let callbacks = Arc::new(AtomicUsize::new(0));
    let called = callbacks.clone();
    let report = ReportOwner::new(Arc::new(move |_| {
        called.fetch_add(1, Ordering::SeqCst);
        panic!("synthetic callback presentation fault");
    }));
    let (mut client, io) = tokio::io::duplex(64);
    let (mut accepted, accepted_io) = tokio::io::duplex(64);
    let mut accepted_io = ProbeIo::new(accepted_io);
    accepted_io.fail_write = true;
    let (upstream, mut shim) = tokio::io::duplex(64);
    let (published, quiesced) = tokio::sync::oneshot::channel();
    *state.server_hooks.shutdown_published.lock().unwrap() = Some(published);
    let onion = stream::iter([
        Ok(bridge(
            accepted_io,
            report.clone(),
            BridgeFixture::Duplex(upstream, Arc::default()),
        )),
        Ok(bridge(
            io,
            report.clone(),
            BridgeFixture::Fail(Arc::default()),
        )),
    ])
    .chain(stream::pending());
    let server = tokio::spawn(crate::tor_ingress::serve_connection_sources_until(
        stream::pending(),
        onion,
        state.clone(),
        HEADER_DEADLINE,
        std::future::pending(),
        report,
    ));
    accepted.write_all(b"a").await.unwrap();
    assert_eq!(bounded(shim.read_u8()).await.unwrap(), b'a');
    client.write_all(b"a").await.unwrap();
    bounded(quiesced).await.unwrap();
    assert!(!server.is_finished());
    // A later I/O fault during drain cannot replace the original callback fault.
    shim.write_all(b"x").await.unwrap();
    let result = bounded(server).await.unwrap().unwrap_err();
    assert_eq!(callbacks.load(Ordering::SeqCst), 1);
    assert_eq!(result.source, None);
    assert_eq!(result.shared, None);
    assert_eq!(result.reporting.unwrap().original, Failure::BridgeConnect);
    assert!(!state.lifecycle.is_serving());
    let text = result.to_string();
    assert!(text.contains("tor_bridge_connect_unknown"));
    assert!(text.contains("tor_reporter_unknown"));
    assert!(!text.contains("synthetic callback"));
}
