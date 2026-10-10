//! Fixed, credential-free loopback readiness probe; never uses the inference client.
use hyper::{client::conn::http1, Request, StatusCode};
use hyper_util::rt::TokioIo;
use std::{net::SocketAddr, time::Duration};
use tokio::{net::TcpStream, task::JoinHandle};

const LOCAL_GATEWAY: SocketAddr =
    SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), 8080);
const PROBE_DEADLINE: Duration = Duration::from_secs(2);

pub async fn healthcheck() -> bool {
    check_at(LOCAL_GATEWAY).await
}

// A timed-out probe cannot leave a detached HTTP connection task running.
struct ConnectionTask(JoinHandle<()>);
impl Drop for ConnectionTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn check_at(address: SocketAddr) -> bool {
    tokio::time::timeout(PROBE_DEADLINE, async {
        let stream = TcpStream::connect(address).await.map_err(|_| ())?;
        let (mut sender, connection) = http1::handshake(TokioIo::new(stream))
            .await
            .map_err(|_| ())?;
        let _connection = ConnectionTask(tokio::spawn(async move {
            let _ = connection.await;
        }));
        let request = Request::builder()
            .method("GET")
            .uri("/healthz")
            .header("host", "127.0.0.1")
            .header("connection", "close")
            .body(http_body_util::Empty::<hyper::body::Bytes>::new())
            .map_err(|_| ())?;
        let mut response = sender.send_request(request).await.map_err(|_| ())?;
        if response.status() != StatusCode::NO_CONTENT
            || response
                .headers()
                .contains_key(hyper::header::TRANSFER_ENCODING)
            || response
                .headers()
                .get(hyper::header::CONTENT_LENGTH)
                .is_some_and(|length| length.as_bytes() != b"0")
        {
            return Err(());
        }
        use http_body_util::BodyExt;
        if response.body_mut().frame().await.is_some() {
            return Err(());
        }
        Ok(())
    })
    .await
        == Ok(Ok(()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn mock(reply: &'static [u8]) -> bool {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0; 512];
            let count = stream.read(&mut request).await.unwrap();
            assert!(request[..count].starts_with(b"GET /healthz HTTP/1.1\r\n"));
            stream.write_all(reply).await.unwrap();
        });
        let checked = check_at(address).await;
        server.await.unwrap();
        checked
    }

    #[tokio::test]
    async fn only_empty_204_is_ready() {
        assert!(mock(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n").await);
        assert!(!mock(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n").await);
        assert!(
            !mock(b"HTTP/1.1 302 Found\r\nLocation: /healthz\r\nContent-Length: 0\r\n\r\n").await
        );
        assert!(!mock(b"malformed response\r\n").await);
        assert!(!mock(b"HTTP/1.1 204 No Content\r\nContent-Length: 1\r\n\r\nx").await);
    }

    #[tokio::test]
    async fn slow_probe_has_total_deadline() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let checked = tokio::spawn(check_at(address));
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0; 3];
        tokio::time::timeout(Duration::from_secs(1), stream.read_exact(&mut request))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&request, b"GET");
        assert!(
            !tokio::time::timeout(PROBE_DEADLINE + Duration::from_secs(1), checked)
                .await
                .unwrap()
                .unwrap()
        );
    }

    #[tokio::test]
    async fn unavailable_listener_is_not_ready() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        assert!(!check_at(address).await);
    }
}
