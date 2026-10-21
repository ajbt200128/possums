use super::{transport::*, LoginMaterial};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

const KEY: &str = "synthetic-ledger-key-0000000000000000";
const ID: &str = "0123456789abcdef0123456789abcdef";
const IDEMPOTENCY: &str = "v1.1800000000000.0123456789abcdef0123456789abcdef";
const EXPECTED_EXPIRY: &str = "2023-11-14T22:13:50.000Z";

fn failure<T>(result: Result<T, Failure>) -> Failure {
    match result {
        Ok(_) => panic!("expected closed failure"),
        Err(error) => error,
    }
}

#[test]
fn native_key_derivation_is_strict_and_domain_separated() {
    let material = LoginMaterial::derive(&"A".repeat(43)).unwrap();
    let value = serde_json::to_value(&material).unwrap();
    assert_eq!(
        value,
        json!({
            "identity": "v1_fdef11380ae70e3c25831445bd21f0fcbdbca05f006cb9fc8085778e8091274a",
            "password": "_i33WB8MUy6ZbO-brsR_vczsW21zwU87YlzDwjCPaQM"
        })
    );
    for key in [
        "A".repeat(42),
        "A".repeat(129),
        format!("{}B", "A".repeat(42)),
        format!("{}=", "A".repeat(43)),
        format!(" {}", "A".repeat(43)),
        "é".repeat(43),
    ] {
        let error = failure(LoginMaterial::derive(&key));
        assert_eq!(
            error,
            Failure {
                operation: Operation::Login,
                constraint: Constraint::Input,
                status: None,
                dispatch_attempted: false
            }
        );
    }
    assert!(LoginMaterial::derive(&"_".repeat(128)).is_ok());
}

#[test]
fn origin_and_key_validation_never_dials_or_exposes_values() {
    for origin in [
        "http://localhost",
        "https://user:secret@example.invalid",
        "https://example.invalid/path",
        "https://example.invalid?secret",
        "https://example.invalid#secret",
        " https://example.invalid",
        "https://example.invalid/../",
        "https://example.invalid\\",
        "not a URL",
    ] {
        let error = failure(IntakeClient::new(origin, KEY));
        assert_eq!(error.constraint, Constraint::Configuration);
        assert!(!error.dispatch_attempted);
        assert!(!error.to_string().contains(origin));
    }
    for key in [
        "short",
        "synthetic-secret-012345678901234567890\r\n",
        "synthetic-secret-012345678901234567890\0",
    ] {
        let error = failure(IntakeClient::new("https://example.invalid", key));
        assert_eq!(error.constraint, Constraint::Configuration);
        assert!(!format!("{error:?} {error}").contains(key));
    }
    // Configuration itself is not a connectivity/health check.
    assert!(IntakeClient::new("https://example.invalid:444/", KEY).is_ok());
}

async fn fixture(
    trusted: bool,
    hostname: &str,
    timeout: Duration,
) -> (TcpListener, tokio_rustls::TlsAcceptor, IntakeClient) {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let rcgen::CertifiedKey { cert, key_pair } =
        rcgen::generate_simple_self_signed(vec![hostname.into()]).unwrap();
    let mut roots = rustls::RootCertStore::empty();
    if trusted {
        roots.add(cert.der().clone()).unwrap();
    }
    let tls = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(
        vec![cert.der().clone()],
        rustls::pki_types::PrivatePkcs8KeyDer::from(key_pair.serialize_der()).into(),
    )
    .unwrap();
    let origin = format!("https://{}", listener.local_addr().unwrap());
    (
        listener,
        tokio_rustls::TlsAcceptor::from(Arc::new(tls)),
        IntakeClient::fixture(&origin, KEY, roots, timeout),
    )
}

async fn capture<S: tokio::io::AsyncRead + Unpin>(stream: &mut S) -> (String, Value) {
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        assert!(head.len() < 8192);
        head.push(stream.read_u8().await.unwrap());
    }
    let head = String::from_utf8(head).unwrap();
    let length = head
        .lines()
        .find_map(|line| line.strip_prefix("content-length: "))
        .map(|s| s.parse::<usize>().unwrap())
        .unwrap_or(0);
    assert!(length <= 4096);
    let mut bytes = vec![0; length];
    stream.read_exact(&mut bytes).await.unwrap();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (head, body)
}

#[tokio::test]
async fn request_routes_separate_credentials_and_encode_only_contract_fields() {
    let login = LoginMaterial::derive(&"A".repeat(43)).unwrap();
    let cases = [
        (
            Request::Login(&login),
            "/api/collections/customer_keys/auth-with-password",
            serde_json::to_value(&login).unwrap(),
            None,
        ),
        (
            Request::Refresh("synthetic-native-token"),
            "/api/collections/customer_keys/auth-refresh",
            json!({}),
            Some("synthetic-native-token".to_owned()),
        ),
        (
            Request::AuthMethods,
            "/api/collections/customer_keys/auth-methods",
            Value::Null,
            None,
        ),
        (
            Request::Reserve {
                account: "opaque\u{0}🐾",
                maximum: u64::MAX,
                timeout_seconds: 1800,
                key: IDEMPOTENCY,
            },
            "/reservations",
            json!({"account_id":"opaque\u{0}🐾", "maximum": u64::MAX.to_string(), "timeout_seconds":1800}),
            Some(format!("Bearer {KEY}")),
        ),
        (
            Request::Renew {
                reservation: ID,
                expected_expires_at: EXPECTED_EXPIRY,
            },
            "/reservations/renew",
            json!({"reservation_id":ID,"expected_expires_at":EXPECTED_EXPIRY}),
            Some(format!("Bearer {KEY}")),
        ),
        (
            Request::Settle {
                reservation: ID,
                charged: 0,
            },
            "/reservations/settle",
            json!({"reservation_id":ID,"charged_amount":"0"}),
            Some(format!("Bearer {KEY}")),
        ),
        (
            Request::Refund(ID),
            "/reservations/refund",
            json!({"reservation_id":ID}),
            Some(format!("Bearer {KEY}")),
        ),
        (
            Request::Lookup(ID),
            "/reservations/lookup",
            json!({"reservation_id":ID}),
            Some(format!("Bearer {KEY}")),
        ),
        (
            Request::Balance("opaque\u{0}🐾"),
            "/balance",
            json!({"account_id":"opaque\u{0}🐾"}),
            Some(format!("Bearer {KEY}")),
        ),
    ];
    for (request, path, expected, authorization) in cases {
        let (listener, acceptor, client) = fixture(true, "127.0.0.1", Duration::from_secs(2)).await;
        let task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut stream = acceptor.accept(stream).await.unwrap();
            let (head, body) = capture(&mut stream).await;
            let method = if expected.is_null() { "GET" } else { "POST" };
            assert!(head.starts_with(&format!("{method} {path} HTTP/1.1\r\n")));
            assert_eq!(body, expected);
            let auth: Vec<_> = head
                .lines()
                .filter(|l| l.starts_with("authorization:"))
                .collect();
            assert_eq!(
                auth,
                authorization
                    .as_ref()
                    .map(|a| format!("authorization: {a}"))
                    .into_iter()
                    .collect::<Vec<_>>()
            );
            assert_eq!(head.contains("idempotency-key:"), path == "/reservations");
            if path == "/reservations" {
                assert!(head.contains(IDEMPOTENCY));
            }
            assert!(head.contains("accept-encoding: identity\r\n"));
            assert!(!head.contains("cookie:"));
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}")
                .await
                .unwrap();
        });
        assert_eq!(client.send(request).await.unwrap().status.as_u16(), 200);
        task.await.unwrap();
    }
}

#[tokio::test]
async fn certificate_and_hostname_are_verified_before_http() {
    for (trusted, host) in [(false, "127.0.0.1"), (true, "wrong-name.invalid")] {
        let (listener, acceptor, client) = fixture(trusted, host, Duration::from_secs(2)).await;
        let task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            assert!(acceptor.accept(stream).await.is_err());
        });
        let error = failure(client.send(Request::Balance("synthetic")).await);
        assert_eq!(error.constraint, Constraint::ConnectUnknown);
        assert_eq!(error.status, None);
        task.await.unwrap();
    }
}

#[tokio::test]
async fn response_limits_encoding_and_json_keep_observed_status_without_payload() {
    for (reply, expected) in [
        (
            "HTTP/1.1 503 Unavailable\r\nContent-Length: 16385\r\n\r\n".into(),
            Constraint::ResponseLimit,
        ),
        (
            "HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: 2\r\n\r\n{}".into(),
            Constraint::ResponseEncoding,
        ),
        (
            format!(
                "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4001\r\n{}\r\n0\r\n\r\n",
                "x".repeat(16385)
            ),
            Constraint::ResponseLimit,
        ),
        (
            "HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nHOSTILE_PRIVATE_SENTINEL".into(),
            Constraint::ResponseReadUnknown,
        ),
    ] {
        let status = if reply.contains("503") { 503 } else { 200 };
        let (listener, acceptor, client) = fixture(true, "127.0.0.1", Duration::from_secs(2)).await;
        let task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut stream = acceptor.accept(stream).await.unwrap();
            capture(&mut stream).await;
            let _ = stream.write_all(reply.as_bytes()).await;
            let _ = stream.shutdown().await;
        });
        let error = failure(
            client
                .send(Request::Renew {
                    reservation: ID,
                    expected_expires_at: EXPECTED_EXPIRY,
                })
                .await,
        );
        assert_eq!(error.constraint, expected);
        assert_eq!(error.status, Some(status));
        assert!(error.dispatch_attempted);
        let diagnostic = format!("{error:?} {error}");
        for forbidden in [KEY, ID, "HOSTILE_PRIVATE_SENTINEL", "127.0.0.1"] {
            assert!(!diagnostic.contains(forbidden));
        }
        task.await.unwrap();
    }
}

#[tokio::test]
async fn non_success_preserves_body_without_redirect_or_retry() {
    for status in [200, 307, 401, 409, 410, 503] {
        let (listener, acceptor, client) = fixture(true, "127.0.0.1", Duration::from_secs(2)).await;
        let task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut stream = acceptor.accept(stream).await.unwrap();
            capture(&mut stream).await;
            let location = format!("https://{}/not-authorized", listener.local_addr().unwrap());
            let body = "{\"reservation\":{\"state\":\"expired\"}}";
            stream.write_all(format!("HTTP/1.1 {status} Fixture\r\nLocation: {location}\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
            stream.shutdown().await.unwrap();
            // No second connection after a redirect/refusal/unknown outcome.
            assert!(
                tokio::time::timeout(Duration::from_millis(75), listener.accept())
                    .await
                    .is_err()
            );
        });
        let response = client
            .send(Request::Renew {
                reservation: ID,
                expected_expires_at: EXPECTED_EXPIRY,
            })
            .await
            .unwrap();
        assert_eq!(response.status.as_u16(), status);
        assert_eq!(
            response.decode::<Value>().unwrap()["reservation"]["state"],
            "expired"
        );
        #[derive(serde::Deserialize)]
        struct Required {
            #[serde(rename = "required")]
            _required: bool,
        }
        let error = failure(response.decode::<Required>());
        assert_eq!(error.constraint, Constraint::ResponseJson);
        assert_eq!(error.status, Some(status));
        task.await.unwrap();
    }
}

#[tokio::test]
async fn deadline_covers_body_and_retains_observed_status() {
    let (listener, acceptor, client) = fixture(true, "127.0.0.1", Duration::from_millis(250)).await;
    let task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut stream = acceptor.accept(stream).await.unwrap();
        capture(&mut stream).await;
        stream
            .write_all(b"HTTP/1.1 409 Conflict\r\nContent-Length: 100\r\n\r\n{")
            .await
            .unwrap();
        // Client cancels its owned stream rather than spawning a retry/background read.
        let mut byte = [0];
        let closed = tokio::time::timeout(Duration::from_secs(2), stream.read(&mut byte))
            .await
            .unwrap();
        assert!(matches!(closed, Ok(0) | Err(_)));
    });
    let error = failure(
        client
            .send(Request::Renew {
                reservation: ID,
                expected_expires_at: EXPECTED_EXPIRY,
            })
            .await,
    );
    assert_eq!(error.constraint, Constraint::Deadline);
    assert_eq!(error.status, Some(409));
    task.await.unwrap();
}

/// Launched only by the gateway-owned Go overlay, never against a live service.
#[tokio::test]
async fn invalid_request_never_reaches_listener() {
    let (listener, _acceptor, client) = fixture(true, "127.0.0.1", Duration::from_secs(2)).await;
    for request in [
        Request::Reserve {
            account: "x",
            maximum: 0,
            timeout_seconds: 1,
            key: IDEMPOTENCY,
        },
        Request::Reserve {
            account: "x",
            maximum: 1,
            timeout_seconds: 1801,
            key: IDEMPOTENCY,
        },
        Request::Reserve {
            account: "x",
            maximum: 1,
            timeout_seconds: 1,
            key: "v1.invalid",
        },
        Request::Renew {
            reservation: "not-hex",
            expected_expires_at: EXPECTED_EXPIRY,
        },
        Request::Renew {
            reservation: ID,
            expected_expires_at: "2023-11-14T22:13:50Z",
        },
        Request::Renew {
            reservation: ID,
            expected_expires_at: "1970-01-01T00:00:00.000Z",
        },
        Request::Renew {
            reservation: ID,
            expected_expires_at: "9999-01-01T00:00:00.000Z",
        },
        Request::Balance(""),
        Request::Refund("not-hex"),
        Request::Refresh("secret\r\ninjection"),
    ] {
        let error = failure(client.send(request).await);
        assert_eq!(error.constraint, Constraint::Input);
        assert!(!error.dispatch_attempted);
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(75), listener.accept())
            .await
            .is_err()
    );
}
