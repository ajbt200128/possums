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

    pub async fn finished(mut self) {
        (&mut self.task).await.unwrap();
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

/// Synthetic authenticated-channel fixture completion through the production
/// parser, never a fabricated terminal usage. Caller controls the final counts.
pub fn terminal(
    content: &str,
    input: u64,
    output: u64,
    on_delta: &mut (dyn FnMut(&str) + Send),
) -> Result<StreamUsage, StreamError> {
    let mut parser = ProtocolParser::default();
    for bytes in [
        event(choice(Some(content), Some("stop"))),
        event(usage(input, output)),
        b"data: [DONE]\n\n".to_vec(),
    ] {
        parser.feed(&bytes, &mut *on_delta)?;
    }
    parser.eof()
}

pub fn successful(content: &str) -> Vec<u8> {
    [
        event(choice(Some(content), Some("stop"))),
        event(usage(2, 3)),
        b"data: [DONE]\n\n".to_vec(),
    ]
    .concat()
}

pub const FIXTURE_MODELS: [&str; 2] = ["fixture-model", "fixture-model-two"];
pub const MAX_CAPTURE_BYTES: usize = 64 * 1024;
pub const MAX_CAPTURE_MESSAGES: usize = 64;
pub const FIXTURE_TEXT: &str = "<script>fetch('https://browser-canary.invalid')</script> ![pixel](https://browser-canary.invalid/pixel) **safe response**";

/// Empty/text scenario data for later streaming fixture wiring. Protocol faults
/// still use the raw peer/events above, not a parallel parser or alternate route.
#[derive(Clone, Copy)]
pub enum FixtureAnswer {
    Text,
    Empty,
}

impl FixtureAnswer {
    pub fn release(self) -> Vec<u8> {
        event(choice(
            Some(match self {
                Self::Text => FIXTURE_TEXT,
                Self::Empty => "",
            }),
            None,
        ))
    }

    pub fn finish(self) -> Vec<u8> {
        [
            event(choice(None, Some("stop"))),
            event(usage(1, 2)),
            b"data: [DONE]\n\n".to_vec(),
        ]
        .concat()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FixtureControl {
    Release,
    Finish,
    Eof,
    Fail,
}

/// Holding means no command has been sent. Nonblocking two-command control
/// queue: callers cannot accumulate pending send futures or content in controls.
/// These primitives are test/example-only and expose no HTTP control route.
pub struct FixtureController(mpsc::Sender<FixtureControl>);

impl FixtureController {
    pub fn send(&self, control: FixtureControl) -> Result<(), &'static str> {
        self.0
            .try_send(control)
            .map_err(|_| "fixture control unavailable")
    }
}

pub fn fixture_controls() -> (FixtureController, mpsc::Receiver<FixtureControl>) {
    let (sender, receiver) = mpsc::channel(2);
    (FixtureController(sender), receiver)
}

/// Resettable, capped capture of one upstream request. Never Debug/log this
/// content; production does not compile this helper. Reset also drops capacity.
#[derive(Default)]
pub struct FixtureCapture {
    model: Option<&'static str>,
    messages: Vec<(String, String)>,
    bytes: usize,
}

impl FixtureCapture {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn record<'a>(
        &mut self,
        model: &str,
        messages: impl ExactSizeIterator<Item = (&'a str, &'a str)> + Clone,
    ) -> Result<(), &'static str> {
        self.reset(); // A failed capture must not leave stale successful evidence.
        let model = FIXTURE_MODELS
            .into_iter()
            .find(|id| *id == model)
            .ok_or("invalid fixture model")?;
        if messages.len() > MAX_CAPTURE_MESSAGES {
            return Err("fixture capture full");
        }
        let slots = messages.len() * std::mem::size_of::<(String, String)>();
        let bytes = messages
            .clone()
            .try_fold(slots, |total, (role, content)| {
                total.checked_add(role.len())?.checked_add(content.len())
            })
            .ok_or("fixture capture full")?;
        if bytes > MAX_CAPTURE_BYTES {
            return Err("fixture capture full");
        }
        self.messages = messages
            .map(|(role, content)| (role.to_owned(), content.to_owned()))
            .collect();
        self.model = Some(model);
        self.bytes = bytes;
        Ok(())
    }

    pub fn get(&self) -> (Option<&str>, &[(String, String)]) {
        (self.model, &self.messages)
    }

    pub fn retained_bytes(&self) -> usize {
        self.bytes
    }
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
