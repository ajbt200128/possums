//! Fixed, credential-free loopback readiness probe; never uses the inference client.
use hyper::{client::conn::http1, Request, StatusCode};
use hyper_util::rt::TokioIo;
use std::{net::SocketAddr, time::Duration};
use tokio::{net::TcpStream, task::JoinHandle};

const LOCAL_GATEWAY: SocketAddr =
    SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), 8080);
const PROBE_DEADLINE: Duration = Duration::from_secs(2);

/// Closed, locally authored probe stages. Known readiness/protocol constraints
/// stay separate from cause-unknown transport stages. No header value, status
/// text, SDK error or io error is retained here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HealthFailure {
    ConnectUnknown,
    HandshakeUnknown,
    RequestSendUnknown,
    NotReady,
    ProtocolConstraint,
    ResponseBodyUnknown,
    DeadlineExceeded,
}

impl HealthFailure {
    /// Stable, content-free code/stage/constraint/action for the affected user.
    pub fn diagnostic(self) -> &'static str {
        match self {
            Self::ConnectUnknown => {
                "code=connect_unknown stage=connect constraint=loopback_connect action=verify_gateway_listening"
            }
            Self::HandshakeUnknown => {
                "code=handshake_unknown stage=handshake constraint=http1_handshake action=verify_local_gateway_protocol"
            }
            Self::RequestSendUnknown => {
                "code=request_send_unknown stage=request constraint=request_dispatch action=verify_local_gateway"
            }
            Self::NotReady => {
                "code=not_ready stage=status constraint=non_204 action=verify_local_gateway_readiness"
            }
            Self::ProtocolConstraint => {
                "code=protocol_constraint stage=response constraint=body_envelope action=verify_local_gateway_protocol"
            }
            Self::ResponseBodyUnknown => {
                "code=response_body_unknown stage=body constraint=body_read action=verify_local_gateway_protocol"
            }
            Self::DeadlineExceeded => {
                "code=deadline_exceeded stage=probe constraint=deadline action=verify_local_gateway_responsiveness"
            }
        }
    }
}

/// Existing bool wrapper retained for compatibility with current callers.
pub async fn healthcheck() -> bool {
    checked().await.is_ok()
}

/// Fixed-loopback readiness with a closed failure stage.
pub async fn checked() -> Result<(), HealthFailure> {
    check_at(LOCAL_GATEWAY).await
}

// A timed-out probe cannot leave a detached HTTP connection task running.
struct ConnectionTask(JoinHandle<()>);
impl Drop for ConnectionTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn check_at(address: SocketAddr) -> Result<(), HealthFailure> {
    match tokio::time::timeout(PROBE_DEADLINE, probe(address)).await {
        Ok(result) => result,
        Err(_) => Err(HealthFailure::DeadlineExceeded),
    }
}

async fn probe(address: SocketAddr) -> Result<(), HealthFailure> {
    let stream = TcpStream::connect(address)
        .await
        .map_err(|_| HealthFailure::ConnectUnknown)?;
    let (mut sender, connection) = http1::handshake(TokioIo::new(stream))
        .await
        .map_err(|_| HealthFailure::HandshakeUnknown)?;
    let _connection = ConnectionTask(tokio::spawn(async move {
        let _ = connection.await;
    }));
    let request = Request::builder()
        .method("GET")
        .uri("/healthz")
        .header("host", "127.0.0.1")
        .header("connection", "close")
        .body(http_body_util::Empty::<hyper::body::Bytes>::new())
        .map_err(|_| HealthFailure::RequestSendUnknown)?;
    let mut response = sender
        .send_request(request)
        .await
        .map_err(|_| HealthFailure::RequestSendUnknown)?;
    if response.status() != StatusCode::NO_CONTENT {
        return Err(HealthFailure::NotReady);
    }
    if response
        .headers()
        .contains_key(hyper::header::TRANSFER_ENCODING)
        || response
            .headers()
            .get(hyper::header::CONTENT_LENGTH)
            .is_some_and(|length| length.as_bytes() != b"0")
    {
        return Err(HealthFailure::ProtocolConstraint);
    }
    use http_body_util::BodyExt;
    match response.body_mut().frame().await {
        None => Ok(()),
        Some(Ok(_)) => Err(HealthFailure::ProtocolConstraint),
        Some(Err(_)) => Err(HealthFailure::ResponseBodyUnknown),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn mock(reply: &'static [u8]) -> Result<(), HealthFailure> {
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
        assert_eq!(
            mock(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n").await,
            Ok(())
        );
    }

    #[tokio::test]
    async fn non_204_is_not_ready() {
        assert_eq!(
            mock(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n").await,
            Err(HealthFailure::NotReady)
        );
        assert_eq!(
            mock(b"HTTP/1.1 302 Found\r\nLocation: /healthz\r\nContent-Length: 0\r\n\r\n").await,
            Err(HealthFailure::NotReady)
        );
    }

    #[tokio::test]
    async fn declared_body_on_204_is_protocol_constraint() {
        assert_eq!(
            mock(b"HTTP/1.1 204 No Content\r\nContent-Length: 1\r\n\r\nx").await,
            Err(HealthFailure::ProtocolConstraint)
        );
    }

    #[tokio::test]
    async fn malformed_reply_is_request_stage_unknown() {
        assert_eq!(
            mock(b"malformed response\r\n").await,
            Err(HealthFailure::RequestSendUnknown)
        );
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
        assert_eq!(
            tokio::time::timeout(PROBE_DEADLINE + Duration::from_secs(1), checked)
                .await
                .unwrap()
                .unwrap(),
            Err(HealthFailure::DeadlineExceeded)
        );
    }

    #[tokio::test]
    async fn unavailable_listener_is_connect_unknown() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        assert_eq!(check_at(address).await, Err(HealthFailure::ConnectUnknown));
    }

    #[test]
    fn diagnostics_are_closed_and_content_free() {
        let mut diagnostics = std::collections::HashSet::new();
        for failure in [
            HealthFailure::ConnectUnknown,
            HealthFailure::HandshakeUnknown,
            HealthFailure::RequestSendUnknown,
            HealthFailure::NotReady,
            HealthFailure::ProtocolConstraint,
            HealthFailure::ResponseBodyUnknown,
            HealthFailure::DeadlineExceeded,
        ] {
            let diagnostic = failure.diagnostic();
            assert!(diagnostics.insert(diagnostic));
            assert!(diagnostic.split(' ').all(|token| {
                !token.is_empty()
                    && token.bytes().all(|b| {
                        b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'='
                    })
            }));
            for field in ["code=", "stage=", "constraint=", "action="] {
                assert!(diagnostic.contains(field), "{diagnostic}");
            }
            let debug = format!("{failure:?}");
            for sentinel in [
                "synthetic",
                "invalid",
                "malformed response",
                "HTTP/1.1",
                "127.0.0.1:8080",
            ] {
                assert!(!diagnostic.contains(sentinel), "{diagnostic}");
                assert!(!debug.contains(sentinel), "{debug}");
            }
        }
        assert_eq!(diagnostics.len(), 7);
    }
}
