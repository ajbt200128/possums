use super::*;
use crate::telemetry::{
    export::transport::Failure,
    tests::{cohort, new, ready, Vector},
};
use opentelemetry_http::HttpClient;
use opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest;
use prost::Message;
use std::sync::atomic::Ordering::SeqCst;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

fn tls(trust: bool) -> (rustls::ClientConfig, tokio_rustls::TlsAcceptor) {
    let rcgen::CertifiedKey { cert, key_pair } =
        rcgen::generate_simple_self_signed(vec!["api.honeycomb.io".into()]).unwrap();
    let mut roots = rustls::RootCertStore::empty();
    if trust {
        roots.add(cert.der().clone()).unwrap();
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let client = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let key = rustls::pki_types::PrivatePkcs8KeyDer::from(key_pair.serialize_der());
    let server = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![cert.der().clone()], key.into())
        .unwrap();
    (client, tokio_rustls::TlsAcceptor::from(Arc::new(server)))
}

pub(in crate::telemetry) async fn fixture(
    trust: bool,
    name: &'static str,
) -> (TcpListener, Client, tokio_rustls::TlsAcceptor) {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let (tls, acceptor) = tls(trust);
    let client = Client::tls_fixture(listener.local_addr().unwrap(), tls, name);
    (listener, client, acceptor)
}

pub(in crate::telemetry) async fn capture<S: tokio::io::AsyncRead + Unpin>(
    stream: &mut S,
) -> Vec<u8> {
    let mut header = Vec::new();
    while !header.ends_with(b"\r\n\r\n") {
        assert!(header.len() < 4096);
        header.push(stream.read_u8().await.unwrap());
    }
    let header = String::from_utf8(header).unwrap();
    let mut length = None;
    assert!(header.starts_with("POST /v1/metrics HTTP/1.1\r\n"));
    for line in header.split("\r\n").skip(1).filter(|line| !line.is_empty()) {
        let (name, value) = line.split_once(": ").unwrap();
        match name {
            "host" => assert_eq!(value, "api.honeycomb.io"),
            "content-type" => assert_eq!(value, "application/x-protobuf"),
            "connection" => assert_eq!(value, "close"),
            "x-honeycomb-team" => assert_eq!(value, "synthetic-key-canary"),
            "content-length" => length = Some(value.parse::<usize>().unwrap()),
            _ => panic!("unexpected header"),
        }
    }
    assert!(header.contains("x-honeycomb-team: synthetic-key-canary\r\n"));
    let mut body = vec![0; length.unwrap()];
    assert!(body.len() <= crate::telemetry::export::transport::MAX_OUTBOUND);
    stream.read_exact(&mut body).await.unwrap();
    body
}

fn released() -> Arc<crate::telemetry::tests::Metrics> {
    let metrics = Arc::new(new());
    ready(&metrics);
    cohort(&metrics, 10, Vector::Plain);
    metrics.poll();
    metrics
}

#[tokio::test]
async fn configured_sender_rewarms_after_invalidation_without_replay() {
    let (_listener, client, _acceptor) = fixture(true, "api.honeycomb.io").await;
    let evidence = client.evidence();
    let metrics = released();
    metrics.invalidate();
    let (stop, stopped) = watch::channel(false);
    let task = tokio::spawn(sender(
        metrics.clone(),
        client,
        stopped,
        crate::web::admission_capacities(),
    ));
    tokio::time::timeout(Duration::from_secs(2), async {
        while metrics.epoch.load(SeqCst) % 2 == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    stop.send_replace(true);
    assert!(task.await.unwrap());
    assert_eq!(evidence.connecting.load(SeqCst), 0);
    let state = metrics.state.lock().unwrap();
    assert_eq!(state.requests.eligible, 900 * crate::telemetry::SECOND);
    assert!(state.requests.attempted >= 600 * crate::telemetry::SECOND);
    assert!(state.requests.pending.is_none());
}

#[tokio::test]
async fn authenticated_tls_owned_sender_payload_stop_and_no_retry() {
    for status in ["200 OK", "307 Temporary Redirect", "503 Unavailable"] {
        let (listener, client, acceptor) = fixture(true, "api.honeycomb.io").await;
        let evidence = client.evidence();
        let metrics = released();
        let (stop, stopped) = watch::channel(false);
        let task = tokio::spawn(sender(
            metrics.clone(),
            client,
            stopped,
            crate::web::admission_capacities(),
        ));
        let (stream, _) = listener.accept().await.unwrap();
        let mut stream = acceptor.accept(stream).await.unwrap();
        let bytes = capture(&mut stream).await;
        assert!(!bytes
            .windows(b"synthetic-key-canary".len())
            .any(|b| b == b"synthetic-key-canary"));
        let decoded = ExportMetricsServiceRequest::decode(bytes.as_slice()).unwrap();
        let resource = decoded.resource_metrics[0].resource.as_ref().unwrap();
        assert_eq!(resource.attributes.len(), 3);
        let env = resource
            .attributes
            .iter()
            .find(|a| a.key == "deployment.environment.name")
            .unwrap();
        assert_eq!(
            env.value.as_ref().unwrap().value,
            Some(
                opentelemetry_proto::tonic::common::v1::any_value::Value::StringValue(
                    "production".into()
                )
            )
        );
        assert!(!decoded.resource_metrics[0].scope_metrics[0]
            .metrics
            .is_empty());
        stream.write_all(format!("HTTP/1.1 {status}\r\nLocation: https://redirect-canary.invalid/\r\nContent-Length: 0\r\n\r\n").as_bytes()).await.unwrap();
        drop(stream);
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(
            tokio::time::timeout(Duration::from_millis(50), listener.accept())
                .await
                .is_err()
        );
        stop.send_replace(true);
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(evidence.connections.load(SeqCst), 1);
        assert_eq!(evidence.live_io.load(SeqCst), 0);
        assert!(metrics.off());
        assert!(metrics.take_window().is_none());
    }
}

#[tokio::test]
async fn tls_certificate_hostname_and_header_boundary() {
    for (trust, name, success) in [
        (true, "api.honeycomb.io", true),
        (false, "api.honeycomb.io", false),
        (true, "wrong.invalid", false),
    ] {
        let (listener, client, acceptor) = fixture(trust, name).await;
        let request = http::Request::post(client.endpoint())
            .header("authorization", "hostile-canary")
            .header("x-extra", "hostile-canary")
            .body(hyper::body::Bytes::from_static(b"fixture"))
            .unwrap();
        let peer = async {
            let (stream, _) = listener.accept().await.unwrap();
            if let Ok(mut stream) = acceptor.accept(stream).await {
                assert!(success);
                assert_eq!(capture(&mut stream).await, b"fixture");
                stream
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                    .await
                    .unwrap();
            } else {
                assert!(!success);
            }
        };
        let (result, ()) = tokio::join!(client.send_bytes(request), peer);
        assert_eq!(result.is_ok(), success);
        if let Err(error) = result {
            assert_eq!(error.to_string(), Failure::Transport.to_string());
            assert!(!format!("{client:?}").contains("canary"));
        }
        assert_eq!(client.evidence().live_io.load(SeqCst), 0);
    }
}

#[tokio::test]
async fn tls_timeout_stop_and_epoch_invalidation_own_actual_io() {
    for mode in ["timeout", "stop", "epoch", "handshake"] {
        let (listener, client, acceptor) = fixture(true, "api.honeycomb.io").await;
        let evidence = client.evidence();
        let metrics = released();
        let (stop, stopped) = watch::channel(false);
        let task = tokio::spawn(sender(
            metrics.clone(),
            client,
            stopped,
            crate::web::admission_capacities(),
        ));
        let (stream, _) = listener.accept().await.unwrap();
        let mut tls_stream = None;
        let mut raw_stream = None;
        if mode == "handshake" {
            raw_stream = Some(stream);
        } else {
            let mut stream = acceptor.accept(stream).await.unwrap();
            capture(&mut stream).await;
            tls_stream = Some(stream);
        }
        if mode == "epoch" {
            metrics.invalidate();
        } else if mode == "stop" {
            stop.send_replace(true);
        } else {
            tokio::time::sleep(Duration::from_millis(1100)).await;
        }
        stop.send_replace(true);
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(evidence.live_io.load(SeqCst), 0);
        let writes = evidence.written.load(SeqCst);
        tokio::time::sleep(Duration::from_millis(120)).await;
        assert_eq!(evidence.written.load(SeqCst), writes);
        assert_eq!(evidence.connections.load(SeqCst), 1);
        assert!(metrics.take_window().is_none());
        drop((tls_stream, raw_stream));
    }
}

#[tokio::test]
async fn tls_bounded_response_and_redirect_rejection() {
    for response in [
        b"HTTP/1.1 200 OK\r\nContent-Length: 18446744073709551615\r\n\r\n".as_slice(),
        b"HTTP/1.1 307 Temporary Redirect\r\nLocation: https://api.honeycomb.io/v1/metrics\r\nContent-Length: 0\r\n\r\n".as_slice(),
    ] {
        let (listener, client, acceptor) = fixture(true, "api.honeycomb.io").await;
        let peer = async {
            let (stream, _) = listener.accept().await.unwrap();
            let mut stream = acceptor.accept(stream).await.unwrap();
            capture(&mut stream).await;
            stream.write_all(response).await.unwrap();
            // More than the cap; no allocation based on hostile Content-Length.
            let _ = stream.write_all(&[b'x'; 65537]).await;
        };
        let request = http::Request::post(client.endpoint()).body(hyper::body::Bytes::from_static(b"fixture")).unwrap();
        let (result, ()) = tokio::join!(client.send_bytes(request), peer);
        assert!(result.is_err());
        assert_eq!(client.evidence().connections.load(SeqCst), 1);
        assert_eq!(client.evidence().live_io.load(SeqCst), 0);
        assert!(client.evidence().received.load(SeqCst) <= 65536);
        assert!(tokio::time::timeout(Duration::from_millis(50), listener.accept()).await.is_err());
    }
}
