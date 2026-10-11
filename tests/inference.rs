use async_trait::async_trait;
use possums::inference::{
    collect_bounded_response,
    forward::{self, Evidence, ForwardSink},
    stream::{StreamUsage, MAX_FRAME_BYTES, MAX_JSON_DEPTH, MAX_JSON_NODES},
    InferenceError,
};
use serde_json::{json, Value};
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
async fn real_upstream_client_exposes_no_transport_before_verification_and_refuses_http() {
    let client =
        tinfoil::SecureClient::new("127.0.0.1", "invalid/test-fixture", "credential-canary");
    assert!(!client.is_verified());
    assert!(client.verification_document().is_none());
    assert!(client.http_client().is_err());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = tinfoil::verifier::tls::create_pinned_client(&"00".repeat(32)).unwrap();
    assert!(client
        .post(format!(
            "http://{}/v1/chat/completions",
            listener.local_addr().unwrap()
        ))
        .body("prompt-canary")
        .send()
        .await
        .is_err());
    assert!(
        tokio::time::timeout(Duration::from_millis(20), listener.accept())
            .await
            .is_err()
    );
}

#[derive(Default)]
struct Sink {
    text: String,
    bytes: usize,
    evidence: Option<Evidence>,
    capture: bool,
}
#[async_trait]
impl ForwardSink for Sink {
    async fn event(&mut self, event: Value) -> Result<(), InferenceError> {
        if let Some(text) = event
            .pointer("/choices/0/delta/content")
            .and_then(Value::as_str)
        {
            self.bytes += text.len();
            if self.capture {
                assert!(self.text.len() + text.len() <= MAX_FRAME_BYTES);
                self.text.push_str(text);
            }
        }
        Ok(())
    }
    fn evidence(&mut self, evidence: Evidence) {
        self.evidence = Some(evidence);
    }
}
fn event(value: Value) -> Vec<u8> {
    format!("data: {value}\n\n").into_bytes()
}
fn choice(content: Option<&str>, finish: Option<&str>) -> Value {
    json!({"choices":[{"index":0,"delta":{"content":content},"finish_reason":finish}]})
}
fn usage(input: u64, output: u64) -> Value {
    json!({"choices":[],"usage":{"prompt_tokens":input,"completion_tokens":output,"total_tokens":input.checked_add(output).unwrap()}})
}
async fn consume(
    bytes: &[u8],
    fragment: usize,
    sink: &mut Sink,
) -> Result<StreamUsage, InferenceError> {
    let chunks = bytes
        .chunks(fragment)
        .map(|part| Ok::<_, std::io::Error>(part.to_vec()))
        .collect::<Vec<_>>();
    let response = http::Response::builder()
        .header("content-type", "text/event-stream")
        .body(reqwest::Body::wrap_stream(futures_util::stream::iter(
            chunks,
        )))
        .unwrap()
        .into();
    forward::consume(response, "m", Duration::from_secs(1), sink)
        .await
        .map(|c| c.usage)
}

#[tokio::test]
async fn sdk_native_utf8_crlf_grouping_and_optional_done_are_fragment_independent() {
    let raw = concat!(": comment 😀\r\nevent: message\r\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"é🐾\",\"reasoning\":\"hidden\"},\"finish_reason\":\"length\"}],\"usage\":{\"prompt_tokens\":0,\"completion_tokens\":0,\"total_tokens\":0}}\r\n\r\n",
        "data: [DONE]\r\n\r\n: trailer\r\n").as_bytes();
    for fragment in 1..=raw.len() {
        let mut sink = Sink {
            capture: true,
            ..Sink::default()
        };
        assert_eq!(
            consume(raw, fragment, &mut sink)
                .await
                .unwrap()
                .total_tokens,
            0
        );
        assert_eq!(sink.text, "é🐾");
    }
    // The SDK suppresses DONE: validated finish, usage and actual EOF suffice.
    let bytes = [event(choice(None, Some("stop"))), event(usage(0, 0))].concat();
    assert!(consume(&bytes, 1, &mut Sink::default()).await.is_ok());
}

#[tokio::test]
async fn final_usage_wins_without_summing_running_snapshots_or_reasoning() {
    let mut first = choice(Some("visible"), None);
    first["usage"] = usage(90, 80)["usage"].clone();
    let mut finish = choice(Some("!"), Some("stop"));
    finish["usage"] = usage(8, 9)["usage"].clone();
    let bytes = [event(first), event(finish), event(usage(1, 0))].concat();
    let mut sink = Sink {
        capture: true,
        ..Sink::default()
    };
    assert_eq!(consume(&bytes, 3, &mut sink).await.unwrap().total_tokens, 1);
    assert_eq!(sink.text, "visible!");
    assert_eq!(sink.evidence.unwrap().source, "final_sse");
    let bytes = [event(choice(None, Some("stop"))), event(usage(u64::MAX, 0))].concat();
    assert_eq!(
        consume(&bytes, 7, &mut Sink::default())
            .await
            .unwrap()
            .total_tokens,
        u64::MAX
    );
}

#[tokio::test]
async fn whole_event_failures_do_not_publish_text_or_invalid_evidence() {
    let bad_events = [
        json!({"choices":[]}),
        json!({"choices":[],"usage":null}),
        json!({"choices":[{"index":1,"delta":{},"finish_reason":null}]}),
        json!({"choices":[{"index":0,"delta":{},"finish_reason":"content_filter"}]}),
        json!({"choices":[{"index":0,"delta":{"role":"user","content":"private-marker"},"finish_reason":null}]}),
        json!({"choices":[{"index":0,"delta":{"content":42},"finish_reason":null}]}),
        json!({"choices":[{"index":0,"delta":{"content":"private-marker","reasoning":[]},"finish_reason":null}]}),
        json!({"error":{"message":"private-marker"}}),
        json!({"choices":[],"usage":{"prompt_tokens":1,"completion_tokens":2,"total_tokens":2}}),
    ];
    for bad in bad_events {
        let mut sink = Sink::default();
        let error = consume(&event(bad), 1, &mut sink).await.unwrap_err();
        assert_eq!(sink.bytes, 0);
        assert!(sink.evidence.is_none());
        assert!(!format!("{error:?} {error}").contains("private-marker"));
    }
    for invalid in [
        json!(-1),
        json!(1.5),
        json!("1"),
        json!(null),
        json!({}),
        json!([]),
        json!(true),
        json!({"$serde_json::private::Number":"1"}),
    ] {
        for field in ["prompt_tokens", "completion_tokens", "total_tokens"] {
            let mut bad = choice(Some("must not escape"), Some("stop"));
            bad["usage"] = usage(1, 1)["usage"].clone();
            bad["usage"][field] = invalid.clone();
            let mut sink = Sink::default();
            assert!(consume(&event(bad), 1, &mut sink).await.is_err());
            assert_eq!(sink.bytes, 0);
        }
    }
    for raw in [b"data: {\"choices\":[],\"choices\":[]}\n\n".as_slice(), b"data: not-json\n\n", b"data: \xff\n\n", b"data: {\"choices\":[],\"usage\":{\"prompt_tokens\":18446744073709551616,\"completion_tokens\":0,\"total_tokens\":18446744073709551616}}\n\n"] {
        assert!(consume(raw,1,&mut Sink::default()).await.is_err());
    }
    let finish = event(choice(None, Some("stop")));
    for suffix in [
        event(choice(Some("post-finish"), None)),
        finish.clone(),
        event(json!({"error":"private-marker"})),
    ] {
        assert!(consume(
            &[finish.clone(), event(usage(2, 3)), suffix].concat(),
            1,
            &mut Sink::default()
        )
        .await
        .is_err());
    }
    for bytes in [Vec::new(), event(choice(Some("unfinished"), None)), finish] {
        assert!(consume(&bytes, 1, &mut Sink::default()).await.is_err());
    }
}

#[tokio::test]
async fn bounded_sdk_payloads_and_long_answers_use_the_actual_native_consumer() {
    let nested = format!(
        "data: {{\"choices\":[],\"x\":{}0{}}}\n\n",
        "[".repeat(MAX_JSON_DEPTH + 1),
        "]".repeat(MAX_JSON_DEPTH + 1)
    );
    let wide = format!(
        "data: {{\"choices\":[],\"x\":[{}]}}\n\n",
        vec!["0"; MAX_JSON_NODES].join(",")
    );
    for bytes in [
        nested.into_bytes(),
        wide.into_bytes(),
        event(choice(Some(&"x".repeat(MAX_FRAME_BYTES)), None)),
    ] {
        let mut sink = Sink::default();
        assert!(consume(&bytes, 16 * 1024, &mut sink).await.is_err());
        assert_eq!(sink.bytes, 0);
    }
    // A 32 MiB answer is consumed without retained output or a full response tree.
    let part = event(choice(Some(&"🐾".repeat(1024)), None));
    let chunks = futures_util::stream::iter(
        (0..8192)
            .map(move |_| Ok::<_, std::io::Error>(part.clone()))
            .chain([
                Ok(event(choice(None, Some("stop")))),
                Ok(event(usage(2, 3))),
            ]),
    );
    let response = http::Response::builder()
        .header("content-type", "text/event-stream")
        .body(reqwest::Body::wrap_stream(chunks))
        .unwrap()
        .into();
    let mut sink = Sink::default();
    forward::consume(response, "m", Duration::from_secs(1), &mut sink)
        .await
        .unwrap();
    assert_eq!(sink.bytes, 32 * 1024 * 1024);
    assert!(sink.text.is_empty());
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
