use possums::inference::{collect_bounded_response, InferenceError};
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    time::Instant,
};

async fn response_from(raw: &'static [u8], keep_open: bool) -> reqwest::Response {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request).await.unwrap();
        stream.write_all(raw).await.unwrap();
        if keep_open {
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });
    reqwest::get(format!("http://{address}/")).await.unwrap()
}

#[tokio::test]
async fn accepts_chunked_and_absent_length_responses_within_the_limit() {
    let chunked = response_from(
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nhe\r\n3\r\nllo\r\n0\r\n\r\n",
        false,
    )
    .await;
    assert_eq!(
        collect_bounded_response(chunked, 5, Instant::now() + Duration::from_secs(1))
            .await
            .unwrap(),
        b"hello"
    );

    let absent = response_from(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nhello", false).await;
    assert_eq!(
        collect_bounded_response(absent, 5, Instant::now() + Duration::from_secs(1))
            .await
            .unwrap(),
        b"hello"
    );
}

#[tokio::test]
async fn rejects_declared_and_chunked_responses_over_the_limit() {
    let declared = response_from(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n", true).await;
    assert!(matches!(
        collect_bounded_response(declared, 5, Instant::now() + Duration::from_secs(1)).await,
        Err(InferenceError::InvalidResponse)
    ));

    let chunked = response_from(
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n6\r\n123456\r\n0\r\n\r\n",
        false,
    )
    .await;
    assert!(matches!(
        collect_bounded_response(chunked, 5, Instant::now() + Duration::from_secs(1)).await,
        Err(InferenceError::InvalidResponse)
    ));
}

#[tokio::test]
async fn one_deadline_covers_a_stalled_response_body() {
    let stalled = response_from(
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1\r\na\r\n",
        true,
    )
    .await;
    assert!(matches!(
        collect_bounded_response(stalled, 5, Instant::now() + Duration::from_millis(50)).await,
        Err(InferenceError::Unavailable)
    ));
}
