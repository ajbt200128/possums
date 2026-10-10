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
        .env("POSSUMS_ACCOUNTS_JSON", "malformed synthetic config")
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

fn assert_failed(output: Output) {
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert_eq!(
        output.stderr,
        b"healthcheck_failed: local readiness unavailable\n"
    );
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

    assert_failed(
        with_reply(
            &listener,
            b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n",
        )
        .await,
    );
    assert_failed(with_reply(&listener, b"malformed response\r\n").await);
    drop(listener);
    assert_failed(bounded(probe().output()).await.unwrap());
}
