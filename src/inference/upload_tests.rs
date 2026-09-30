//! Synthetic HTTP/1 only: production serializers/ownership, not provider trust
//! or a universal allocation/RSS bound. Replaces the exploratory upload probe.
use super::*;
use http_body_util::BodyExt;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpSocket,
    sync::{oneshot, Semaphore},
};

const TIMEOUT: Duration = Duration::from_secs(5);

fn admission() -> (Arc<Semaphore>, Arc<OwnedSemaphorePermit>) {
    let lane = Arc::new(Semaphore::new(1));
    let heavy = Arc::new(lane.clone().try_acquire_owned().unwrap());
    (lane, heavy)
}

fn messages() -> [Message; 1] {
    [Message {
        role: "user".into(),
        // Near the production 16-MiB serialized ceiling, beyond socket buffers.
        content: "x".repeat(MAX_TOKENIZER_REQUEST_BYTES - 1024),
    }]
}

async fn early_peer(success: bool) -> (String, oneshot::Sender<()>, tokio::task::JoinHandle<()>) {
    let socket = TcpSocket::new_v4().unwrap();
    socket.set_recv_buffer_size(4096).unwrap();
    socket.bind("127.0.0.1:0".parse().unwrap()).unwrap();
    let listener = socket.listen(1).unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let (release, held) = oneshot::channel();
    let peer = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut headers = Vec::new();
        let mut buffer = [0; 1024];
        while !headers.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = socket.read(&mut buffer).await.unwrap();
            assert!(n > 0 && headers.len() + n <= 16 * 1024);
            headers.extend_from_slice(&buffer[..n]);
        }
        let reply: &[u8] = if success {
            b"HTTP/1.1 200 OK\r\nContent-Length: 18\r\n\r\n{\"input_tokens\":1}"
        } else {
            b"HTTP/1.1 413 Payload Too Large\r\nContent-Length: 0\r\n\r\n"
        };
        socket.write_all(reply).await.unwrap();
        // Do not drain the prompt. Cleanup is explicit, not response/EOF based.
        let _ = tokio::time::timeout(TIMEOUT, held).await;
    });
    (url, release, peer)
}

fn client() -> reqwest::Client {
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder().no_proxy().build().unwrap()
}

async fn released(lane: &Arc<Semaphore>) {
    let permit = tokio::time::timeout(TIMEOUT, lane.acquire())
        .await
        .unwrap()
        .unwrap();
    drop(permit);
    assert_eq!(lane.available_permits(), 1);
}

#[tokio::test]
async fn early_413_keeps_both_production_uploads_charged_after_worker_returns() {
    for streaming in [false, true] {
        let (lane, heavy) = admission();
        let (url, release, peer) = early_peer(false).await;
        let worker = tokio::spawn(async move {
            let messages = messages();
            let (body, receipt) = if streaming {
                (
                    stream::request_body("fixture", 99, &messages, Some(heavy)).unwrap(),
                    None,
                )
            } else {
                let (body, receipt) =
                    tokenizer_request_body("fixture", &messages, Some(heavy)).unwrap();
                (body, Some(receipt))
            };
            drop(messages);
            let client = client();
            let request = client.post(url).body(body).build().unwrap();
            assert!(request.try_clone().is_none());
            let response = client.execute(request).await.unwrap();
            assert_eq!(response.status().as_u16(), 413);
            let deadline = Instant::now() + TIMEOUT;
            if let Some(receipt) = receipt {
                assert!(finish_tokenizer(response, receipt, deadline).await.is_err());
            } else {
                assert!(
                    stream::consume_response(response, deadline, TIMEOUT, |_| panic!())
                        .await
                        .is_err()
                );
            }
        });
        tokio::time::timeout(TIMEOUT, worker)
            .await
            .unwrap()
            .unwrap();
        // No caller, response, message or worker owner remains, just DATA queued
        // in Hyper. A lease on the outer body alone would already have released.
        assert_eq!(lane.available_permits(), 0);
        assert!(
            tokio::time::timeout(Duration::from_millis(30), lane.acquire())
                .await
                .is_err()
        );
        release.send(()).unwrap();
        peer.await.unwrap();
        released(&lane).await;
    }
}

#[tokio::test]
async fn early_success_waits_for_tokenizer_backing_before_generation_serialization() {
    let (lane, heavy) = admission();
    let (url, release, peer) = early_peer(true).await;
    let (responded, response_seen) = oneshot::channel();
    let mut worker = tokio::spawn(async move {
        let messages = messages();
        let (body, receipt) =
            tokenizer_request_body("fixture", &messages, Some(heavy.clone())).unwrap();
        let response = client().post(url).body(body).send().await.unwrap();
        assert_eq!(response.status().as_u16(), 200);
        responded.send(()).unwrap();
        assert_eq!(
            finish_tokenizer(response, receipt, Instant::now() + TIMEOUT)
                .await
                .unwrap(),
            1
        );
        // No tokenizer DATA clone/slice can remain when generation starts.
        assert_eq!(Arc::strong_count(&heavy), 1);
        let body = stream::request_body("fixture", 99, &messages, Some(heavy)).unwrap();
        drop(body.collect().await.unwrap());
    });
    tokio::time::timeout(TIMEOUT, response_seen)
        .await
        .unwrap()
        .unwrap();
    assert!(tokio::time::timeout(Duration::from_millis(30), &mut worker)
        .await
        .is_err());
    assert_eq!(lane.available_permits(), 0);
    release.send(()).unwrap();
    peer.await.unwrap();
    tokio::time::timeout(TIMEOUT, worker)
        .await
        .unwrap()
        .unwrap();
    released(&lane).await;
}

#[tokio::test]
async fn stalled_success_fails_closed_at_deadline_but_upload_keeps_admission() {
    let (lane, heavy) = admission();
    let (url, release, peer) = early_peer(true).await;
    let worker = tokio::spawn(async move {
        let (body, receipt) = tokenizer_request_body("fixture", &messages(), Some(heavy)).unwrap();
        let response = client().post(url).body(body).send().await.unwrap();
        // Short local equivalent of the enclosing route preflight timeout. No
        // generation is allowed on either this timeout or the adapter deadline.
        assert!(tokio::time::timeout(
            Duration::from_millis(30),
            finish_tokenizer(response, receipt, Instant::now() + TIMEOUT),
        )
        .await
        .is_err());
    });
    tokio::time::timeout(TIMEOUT, worker)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(lane.available_permits(), 0);
    release.send(()).unwrap();
    peer.await.unwrap();
    released(&lane).await;
}

#[tokio::test]
async fn production_tokenizer_data_slices_hold_lease_and_release_receipt() {
    let (lane, heavy) = admission();
    let (body, receipt) = tokenizer_request_body("fixture", &[], Some(heavy)).unwrap();
    let data = body.collect().await.unwrap().to_bytes();
    let clone = data.clone();
    let slice = clone.slice(1..2);
    drop(data);
    drop(clone);
    let mut wait = Box::pin(receipt.wait());
    assert!(tokio::time::timeout(Duration::from_millis(10), &mut wait)
        .await
        .is_err());
    assert_eq!(lane.available_permits(), 0);
    drop(slice);
    tokio::time::timeout(TIMEOUT, wait).await.unwrap();
    assert_eq!(lane.available_permits(), 1);
}
