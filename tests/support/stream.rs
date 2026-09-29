//! Bounded raw fixtures for the real parser, shared by later renderer/worker tests.
//! No helper here is compiled into the production library.
#![allow(dead_code)]
// Including test modules import the production module as `stream` in the parent.
use super::stream::{
    ProtocolParser, StreamError, StreamUsage, MAX_FRAME_BYTES, MAX_TRANSPORT_BUFFER_BYTES,
};
use serde_json::{json, Value};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::mpsc,
};

/// Held-open raw HTTP/1 chunked peer. Only two bounded fragments may queue;
/// EOF/fault are explicit and Drop tears down the server, including on test panic.
pub struct RawPeer {
    sender: mpsc::Sender<Step>,
    task: tokio::task::JoinHandle<()>,
}

enum Step {
    Fragment(Vec<u8>),
    Eof,
    Fault,
}

impl Drop for RawPeer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl RawPeer {
    pub async fn send(&self, bytes: &[u8], fragment: usize) {
        assert!(fragment > 0 && fragment <= MAX_TRANSPORT_BUFFER_BYTES);
        for part in bytes.chunks(fragment) {
            self.sender
                .send(Step::Fragment(part.to_vec()))
                .await
                .unwrap();
        }
    }

    pub async fn eof(&self) {
        self.sender.send(Step::Eof).await.unwrap();
    }

    pub async fn fault(&self) {
        self.sender.send(Step::Fault).await.unwrap();
    }
}

pub async fn raw_response() -> (reqwest::Response, RawPeer) {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (sender, mut receiver) = mpsc::channel(2);
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0; 1024];
        let mut received = 0;
        loop {
            let size = socket.read(&mut request[received..]).await.unwrap();
            assert!(size > 0);
            received += size;
            if request[..received].ends_with(b"\r\n\r\n") {
                break;
            }
            assert!(received < request.len());
        }
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n").await.unwrap();
        while let Some(step) = receiver.recv().await {
            match step {
                Step::Fragment(bytes) => {
                    socket
                        .write_all(format!("{:x}\r\n", bytes.len()).as_bytes())
                        .await
                        .unwrap();
                    socket.write_all(&bytes).await.unwrap();
                    socket.write_all(b"\r\n").await.unwrap();
                }
                Step::Eof => {
                    socket.write_all(b"0\r\n\r\n").await.unwrap();
                    break;
                }
                Step::Fault => break, // Missing HTTP chunk terminator is a transport fault.
            }
        }
    });
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .get(format!("http://{address}/"))
        .send()
        .await
        .unwrap();
    (response, RawPeer { sender, task })
}

pub fn event(value: Value) -> Vec<u8> {
    let bytes = format!("data: {value}\n\n").into_bytes();
    assert!(bytes.len() <= MAX_FRAME_BYTES);
    bytes
}

pub fn choice(content: Option<&str>, finish: Option<&str>) -> Value {
    json!({"choices":[{"index":0,"delta":{"content":content},"finish_reason":finish}]})
}

pub fn usage(input: u64, output: u64) -> Value {
    json!({"choices":[],"usage":{
        "prompt_tokens":input,"completion_tokens":output,"total_tokens":input.checked_add(output).unwrap()
    }})
}

pub fn successful(content: &str) -> Vec<u8> {
    [
        event(choice(Some(content), Some("stop"))),
        event(usage(2, 3)),
        b"data: [DONE]\n\n".to_vec(),
    ]
    .concat()
}

/// Capture is intentionally capped and test-only, unlike the production adapter.
/// Long-stream tests should count bytes in a callback instead of collecting them.
pub fn parse(bytes: &[u8], fragment: usize) -> Result<(String, StreamUsage), StreamError> {
    assert!(fragment > 0);
    let mut parser = ProtocolParser::default();
    let mut text = String::new();
    for part in bytes.chunks(fragment) {
        parser.feed(part, |delta| {
            assert!(text.len().checked_add(delta.len()).unwrap() <= MAX_FRAME_BYTES);
            text.push_str(delta);
        })?;
    }
    Ok((text, parser.eof()?))
}
