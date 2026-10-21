//! The built binary's fixed-loopback health mode, with no gateway or provider setup.
use std::{
    process::{Output, Stdio},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    process::Command,
};

fn probe() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_possums"));
    command
        .arg("--healthcheck")
        .env_clear()
        .env("POSSUMS_INTAKE_ORIGIN", "malformed synthetic origin")
        .env("POSSUMS_INTAKE_LEDGER_KEY", "malformed synthetic key")
        .env("POSSUMS_TINFOIL_HOST", "invalid synthetic host")
        .env("POSSUMS_BIND", "127.0.0.1:1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    command
}

async fn bounded<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(5), future)
        .await
        .expect("local binary health test exceeded its bound")
}

async fn with_reply(listener: &TcpListener, reply: &[u8]) -> Output {
    let child = probe().spawn().unwrap();
    let (mut connection, _) = bounded(listener.accept()).await.unwrap();
    let expected = b"GET /healthz HTTP/1.1\r\n";
    let mut request_line = vec![0; expected.len()];
    bounded(connection.read_exact(&mut request_line))
        .await
        .unwrap();
    assert_eq!(request_line, expected);
    connection.write_all(reply).await.unwrap();
    drop(connection);
    bounded(child.wait_with_output()).await.unwrap()
}

fn assert_failed_with(label: &str, output: Output, code: &str, stage: &str) {
    assert!(!output.status.success(), "{label}: status");
    assert!(output.stdout.is_empty(), "{label}: stdout");
    let stderr = String::from_utf8(output.stderr).expect("stderr is closed ascii diagnostics");
    let diagnostic = stderr
        .strip_prefix("healthcheck_failed: ")
        .and_then(|line| line.strip_suffix('\n'))
        .expect("closed healthcheck diagnostic prefix");
    assert!(
        diagnostic.contains(&format!("code={code}")),
        "{label}: {diagnostic}"
    );
    assert!(
        diagnostic.contains(&format!("stage={stage}")),
        "{label}: {diagnostic}"
    );
    assert!(diagnostic.contains("constraint="), "{label}: {diagnostic}");
    assert!(diagnostic.contains("action="), "{label}: {diagnostic}");
    for sentinel in ["synthetic", "invalid", "malformed response"] {
        assert!(!diagnostic.contains(sentinel), "{label}: {diagnostic}");
    }
}

#[tokio::test]
async fn built_healthcheck_uses_fixed_loopback_before_config_or_secrets() {
    // Never connect to an existing process: lack of exclusive port ownership is
    // an environment blocker, not a reason to skip or change the probe target.
    let listener = TcpListener::bind("127.0.0.1:8080")
        .await
        .expect("port 8080 unavailable; isolated health fixture required");
    let ready = with_reply(
        &listener,
        b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n",
    )
    .await;
    assert!(ready.status.success());
    assert!(ready.stdout.is_empty());
    assert!(ready.stderr.is_empty());

    assert_failed_with(
        "non-204",
        with_reply(
            &listener,
            b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n",
        )
        .await,
        "not_ready",
        "status",
    );
    assert_failed_with(
        "malformed",
        with_reply(&listener, b"malformed response\r\n").await,
        "request_send_unknown",
        "request",
    );
    assert_failed_with(
        "declared-body",
        with_reply(
            &listener,
            b"HTTP/1.1 204 No Content\r\nContent-Length: 1\r\n\r\nx",
        )
        .await,
        "protocol_constraint",
        "response",
    );
    drop(listener);
    // A foreign listener on fixed port 8080 is an environment blocker: the
    // probe can no longer observe a refused connection, so only require the
    // typed connect stage when loopback is actually closed. The ephemeral-port
    // unit test always covers the refused classification.
    let refused = bounded(probe().output()).await.unwrap();
    if std::net::TcpStream::connect_timeout(
        &"127.0.0.1:8080".parse().unwrap(),
        Duration::from_millis(250),
    )
    .is_err()
    {
        assert_failed_with("refused", refused, "connect_unknown", "connect");
    } else {
        eprintln!("Health fixture limit: an external fixed-port listener prevents refused-connection coverage here; the ephemeral-port unit test covers it.");
        assert!(!refused.status.success(), "refused: status");
        assert!(refused.stdout.is_empty(), "refused: stdout");
        assert!(
            refused.stderr.starts_with(b"healthcheck_failed: code="),
            "refused: closed code"
        );
    }
}
