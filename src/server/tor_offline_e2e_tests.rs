// Public fixtures only. Arti rendezvous/accept, attestation, upstream generation
// and API-session creation are substitutions, never live trust/accounting proof.
use super::*;
use crate::tor_ingress::{
    serve_connection_sources_until, BridgeFixture, BridgeMetadata, ReportOwner,
};
use futures_util::stream;
use rustls::pki_types::{PrivatePkcs8KeyDer, ServerName};
use tokio_rustls::{TlsAcceptor, TlsConnector};
use tokio_util::compat::{FuturesAsyncReadCompatExt, TokioAsyncReadCompatExt};

type Io = Box<dyn crate::tor_ingress::TokioIo>;

async fn at<T>(stage: &'static str, future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(5), future)
        .await
        .unwrap_or_else(|_| panic!("public_e2e_timeout stage={stage}"))
}

#[tokio::test]
async fn full_preparation_shared_ingress_tls_bridge_stream_and_owned_drain() {
    let prepared = crate::tor_runtime::PreparedTor::offline_public_fixture();
    let (state, probe, session) = fixture();
    let state = prepared.configure(state);
    probe.emit_progress.store(true, Ordering::SeqCst);
    let (pause, generation) = checkpoint();
    *probe.generation.lock().unwrap() = Some(pause);

    // Real local gateway TCP acceptor and separate public-shim substitution.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let gateway_address = listener.local_addr().unwrap();
    let shim_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let shim_address = shim_listener.local_addr().unwrap();
    let certificate = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let cert = certificate.cert.der().clone();
    let tls_server = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![cert.clone()],
            PrivatePkcs8KeyDer::from(certificate.key_pair.serialize_der()).into(),
        )
        .unwrap();
    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert).unwrap();
    let tls_client = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let shim = tokio::spawn(async move {
        let (socket, _) = shim_listener.accept().await.unwrap();
        let mut tls = TlsAcceptor::from(Arc::new(tls_server))
            .accept(socket)
            .await
            .unwrap();
        let mut gateway = tokio::net::TcpStream::connect(gateway_address)
            .await
            .unwrap();
        tokio::io::copy_bidirectional(&mut tls, &mut gateway)
            .await
            .unwrap();
    });

    let tcp = stream::unfold(listener, |listener| async move {
        let (socket, _) = listener.accept().await.unwrap();
        Some((Ok(Incoming::Http(Box::new(socket) as Io)), listener))
    });
    let (client, onion_io) = tokio::io::duplex(64);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let reports = seen.clone();
    let report = ReportOwner::new(Arc::new(move |failure| {
        reports.lock().unwrap().push(failure)
    }));
    let dials = Arc::new(AtomicUsize::new(0));
    // Exact production Futures-to-Tokio compat boundary; no Arti request claimed.
    let onion_io = Box::new(onion_io.compat().compat()) as Io;
    let onion = stream::once(std::future::ready(Ok(Incoming::OpaqueTorBridge(
        onion_io,
        BridgeMetadata::fixture(
            report.clone(),
            BridgeFixture::Tcp(shim_address, dials.clone()),
        ),
    ))))
    .chain(stream::pending());
    let (stop, shutdown) = tokio::sync::oneshot::channel();
    let (published, quiesced) = tokio::sync::oneshot::channel();
    *state.server_hooks.shutdown_published.lock().unwrap() = Some(published);
    let server = tokio::spawn(serve_connection_sources_until(
        tcp,
        onion,
        state.clone(),
        HEADER_DEADLINE,
        async {
            let _ = shutdown.await;
        },
        report,
    ));
    let mut client = at(
        "tls",
        TlsConnector::from(Arc::new(tls_client))
            .connect(ServerName::try_from("localhost").unwrap(), client),
    )
    .await
    .unwrap();
    let body = chat(&state, &session, false)
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let mut request = format!("POST /v1/chat/completions HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {session}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes();
    request.extend_from_slice(&body);
    at("request", client.write_all(&request)).await.unwrap();
    at("request_flush", client.flush()).await.unwrap();
    let mut received = Vec::new();
    at("progress", async {
        while !received
            .windows(b"synthetic progress".len())
            .any(|w| w == b"synthetic progress")
        {
            let mut chunk = [0; 256];
            let count = client.read(&mut chunk).await.unwrap();
            assert!(count > 0);
            received.extend_from_slice(&chunk[..count]);
            assert!(received.len() < 65536);
        }
    })
    .await;
    at("generation", generation.reached).await.unwrap();
    assert_eq!(state.accounting.snapshot("demo").unwrap().in_flight, 1);
    assert!(state.accounting.available("demo").unwrap() < 1000);
    // Accepted work must remain owned through quiesce and real bridged delivery.
    stop.send(()).unwrap();
    at("quiesce", quiesced).await.unwrap();
    assert!(!server.is_finished());
    assert!(!state.lifecycle.is_serving());
    generation.release.send(()).unwrap();
    at("terminal_delivery", client.read_to_end(&mut received))
        .await
        .unwrap();
    at("client_close", client.shutdown()).await.unwrap();
    drop(client);
    at("shim_join", shim).await.unwrap();
    assert!(at("server_drain", server).await.unwrap().is_ok());
    assert!(received.starts_with(b"HTTP/1.1 200"));
    assert!(received.windows(b"[DONE]".len()).any(|w| w == b"[DONE]"));
    for marker in [
        b"\"outcome\":\"settled\"".as_slice(),
        b"\"status\":\"complete\"",
    ] {
        assert!(received.windows(marker.len()).any(|w| w == marker));
    }
    assert_eq!(probe.calls.load(Ordering::SeqCst), 1);
    assert_eq!(probe.tokenize_calls.load(Ordering::SeqCst), 1);
    assert_eq!(state.accounting.snapshot("demo").unwrap().in_flight, 0);
    assert_eq!(
        state
            .accounting
            .snapshot("demo")
            .unwrap()
            .completed_requests,
        1
    );
    assert_eq!(dials.load(Ordering::SeqCst), 1);
    assert!(seen.lock().unwrap().is_empty());
    // Both owned listeners are gone, not detached tasks holding open ports.
    assert!(tokio::net::TcpStream::connect(gateway_address)
        .await
        .is_err());
    assert!(tokio::net::TcpStream::connect(shim_address).await.is_err());
    drop(prepared);
}
