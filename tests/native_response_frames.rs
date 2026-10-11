//! Synthetic qualification of the pinned reqwest native frame conversion.
//! This does not exercise attestation, provider endpoints, or downstream EHBP.
use http_body_util::BodyExt;
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
};

const DEADLINE: Duration = Duration::from_secs(3);

async fn request(stream: &mut TcpStream) {
    let mut bytes = Vec::new();
    loop {
        let mut byte = [0];
        assert_eq!(stream.read(&mut byte).await.unwrap(), 1);
        bytes.push(byte[0]);
        assert!(bytes.len() <= 4096);
        if bytes.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    assert!(bytes.starts_with(b"POST /fixture HTTP/1.1\r\n"));
    let mut body = [0; 2];
    stream.read_exact(&mut body).await.unwrap();
    assert_eq!(&body, b"{}");
}

fn client() -> reqwest::Client {
    let _ = rustls::crypto::ring::default_provider().install_default();
    // Match the pinned SDK's no-idle-pool setting. Loopback plaintext is a
    // fixture only; no production verified transport is modified.
    reqwest::Client::builder()
        .no_proxy()
        .pool_max_idle_per_host(0)
        .build()
        .unwrap()
}

async fn send(origin: String) -> reqwest::Response {
    // Streamed upload is noncloneable, as in the leased production upload path.
    // Its synthetic payload contains no prompt or credentials.
    let body = reqwest::Body::wrap_stream(futures_util::stream::once(async {
        Ok::<_, std::io::Error>(axum::body::Bytes::from_static(b"{}"))
    }));
    client()
        .post(origin)
        .header("Content-Length", "2")
        .header("TE", "trailers")
        .body(body)
        .send()
        .await
        .unwrap()
}

async fn data(stream: &mut TcpStream, bytes: &[u8]) {
    stream
        .write_all(format!("{:x}\r\n", bytes.len()).as_bytes())
        .await
        .unwrap();
    stream.write_all(bytes).await.unwrap();
    stream.write_all(b"\r\n").await.unwrap();
    stream.flush().await.unwrap();
}

async fn no_replay(listener: &TcpListener) {
    // Bounded observation, not a claim about all future operations. Each test
    // invokes exactly one send and never supplies a replayable generation body.
    assert!(
        tokio::time::timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn native_frames_preserve_data_trailers_and_transport_eof() {
    tokio::time::timeout(DEADLINE, async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}/fixture", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            request(&mut stream).await;
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nTrailer: X-Tinfoil-Usage-Metrics\r\nConnection: close\r\n\r\n").await.unwrap();
            data(&mut stream, b"data: [DONE]\n\n").await;
            stream.write_all(b"0\r\nX-Tinfoil-Usage-Metrics: prompt=2,completion=3,total=5,model=fixture\r\n\r\n").await.unwrap();
            stream.shutdown().await.unwrap();
            no_replay(&listener).await;
        });
        let response = send(origin).await;
        let response: http::Response<reqwest::Body> = response.into();
        let mut body = response.into_body();
        let mut bytes = Vec::new();
        let mut trailers = None;
        while let Some(frame) = body.frame().await {
            let frame = frame.unwrap();
            match frame.into_data() {
                Ok(data) => bytes.extend_from_slice(&data),
                Err(frame) => {
                    assert!(trailers.is_none());
                    trailers = Some(frame.into_trailers().unwrap());
                }
            }
        }
        assert_eq!(bytes, b"data: [DONE]\n\n");
        assert_eq!(trailers.unwrap()["x-tinfoil-usage-metrics"], "prompt=2,completion=3,total=5,model=fixture");
        assert!(body.frame().await.is_none());
        server.await.unwrap();
    }).await.unwrap();
}

#[tokio::test]
async fn done_is_not_transport_eof() {
    tokio::time::timeout(DEADLINE, async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}/fixture", listener.local_addr().unwrap());
        let (release, released) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            request(&mut stream).await;
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
                )
                .await
                .unwrap();
            data(&mut stream, b"data: [DONE]\n\n").await;
            released.await.unwrap();
            stream.write_all(b"0\r\n\r\n").await.unwrap();
            stream.shutdown().await.unwrap();
            no_replay(&listener).await;
        });
        let response: http::Response<reqwest::Body> = send(origin).await.into();
        let mut body = response.into_body();
        assert_eq!(
            body.frame().await.unwrap().unwrap().into_data().unwrap(),
            b"data: [DONE]\n\n"[..]
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(50), body.frame())
                .await
                .is_err()
        );
        release.send(()).unwrap();
        assert!(body.frame().await.is_none());
        server.await.unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn dropping_native_body_closes_without_drain_or_replay() {
    tokio::time::timeout(DEADLINE, async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}/fixture", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            request(&mut stream).await;
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
                )
                .await
                .unwrap();
            data(&mut stream, b"data: first\n\n").await;
            // Do not emit DONE, trailers or EOF: only client drop can release
            // this wait. No draining to a terminal trailer is possible.
            let mut byte = [0];
            assert_eq!(stream.read(&mut byte).await.unwrap(), 0);
            no_replay(&listener).await;
        });
        let response: http::Response<reqwest::Body> = send(origin).await.into();
        let mut body = response.into_body();
        assert_eq!(
            body.frame().await.unwrap().unwrap().into_data().unwrap(),
            b"data: first\n\n"[..]
        );
        drop(body);
        server.await.unwrap();
    })
    .await
    .unwrap();
}
