use async_trait::async_trait;
use axum::{
    body::{Body, Bytes},
    http::{header, Request, StatusCode},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use futures_util::stream;
use possums::{
    attestation::UnavailableEvidenceVerifier,
    auth::Auth,
    catalog::Model,
    inference::{Generation, Inference, InferenceError, Message},
    web::{router_with_body_deadline, serve_with_header_deadline, AppState},
};
use sha2::{Digest, Sha256};
use std::{convert::Infallible, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tower::ServiceExt;

struct UnusedInference;

#[async_trait]
impl Inference for UnusedInference {
    async fn catalog(&self) -> Result<Vec<u8>, InferenceError> {
        Err(InferenceError::Unavailable)
    }

    async fn count_tokens(&self, _: &str, _: &[Message]) -> Result<u64, InferenceError> {
        unreachable!()
    }

    async fn generate(&self, _: &Model, _: &[Message]) -> Result<Generation, InferenceError> {
        unreachable!()
    }

    fn verification_document(&self) -> Result<serde_json::Value, InferenceError> {
        Err(InferenceError::Unavailable)
    }
}

fn state() -> AppState {
    let credential = URL_SAFE_NO_PAD.encode([7_u8; 32]);
    let hash = URL_SAFE_NO_PAD.encode(Sha256::digest(credential.as_bytes()));
    let auth = Auth::from_json(&format!(
        r#"[{{"id":"a","credential_sha256":"{hash}","demo_microunits":100}}]"#
    ))
    .unwrap();
    AppState::new(
        auth,
        Arc::new(UnusedInference),
        Arc::<str>::from("/missing"),
        Arc::new(UnavailableEvidenceVerifier),
    )
}

#[tokio::test(start_paused = true)]
async fn request_body_has_one_total_deadline_despite_trickling() {
    let chunks = stream::unfold((), |_| async {
        tokio::time::sleep(Duration::from_secs(1)).await;
        Some((Ok::<Bytes, Infallible>(Bytes::from_static(b"x")), ()))
    });
    let response = router_with_body_deadline(state(), Duration::from_secs(3))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/login")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from_stream(chunks))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
}

#[tokio::test]
async fn incomplete_headers_are_closed_at_the_total_header_deadline() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(serve_with_header_deadline(
        listener,
        state(),
        Duration::from_millis(50),
    ));
    let mut client = TcpStream::connect(address).await.unwrap();
    client.write_all(b"GET / HTTP/1.1\r\nHost:").await.unwrap();

    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(1), client.read_to_end(&mut response))
        .await
        .expect("header deadline did not close the connection")
        .unwrap();
    server.abort();
}

#[tokio::test]
async fn oversized_headers_are_rejected_before_routing() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(serve_with_header_deadline(
        listener,
        state(),
        Duration::from_secs(1),
    ));
    let mut client = TcpStream::connect(address).await.unwrap();
    let request = format!(
        "GET / HTTP/1.1\r\nHost: local\r\nX-Fill: {}\r\n\r\n",
        "a".repeat(40 * 1024)
    );
    client.write_all(request.as_bytes()).await.unwrap();

    let mut response = [0_u8; 1024];
    let read = tokio::time::timeout(Duration::from_secs(1), client.read(&mut response))
        .await
        .unwrap()
        .unwrap();
    assert!(read == 0 || !response[..read].starts_with(b"HTTP/1.1 200"));
    server.abort();
}
