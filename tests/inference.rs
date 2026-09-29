use possums::inference::{collect_bounded_response, InferenceError};
#[path = "support/stream.rs"]
mod stream;
use possums::inference::stream::{
    ProtocolParser, StreamError, MAX_FRAME_BYTES, MAX_JSON_DEPTH, MAX_JSON_NODES, MAX_LINE_BYTES,
    MAX_OPTIONAL_BYTES, MAX_TRAILER_BYTES,
};
use serde_json::json;
use std::time::Duration;
use stream::{choice, event, parse, successful, usage};
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

#[test]
fn arbitrary_utf8_and_crlf_splits_multiline_data_comments_and_final_content() {
    let raw = concat!(
        ": comment 😀\r\n",
        "event: message\r\n",
        "data: {\r\n",
        "data: \"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"é🐾\",\"reasoning\":\"hidden\"},\r\n",
        "data: \"finish_reason\":\"length\"}],\r\n",
        "data: \"usage\":{\"prompt_tokens\":0,\"completion_tokens\":0,\"total_tokens\":0}}\r\n\r\n",
        "data: [DONE]\r\n\r\n: trailer\r\n \t\r\n\n"
    ).as_bytes();
    for split in 0..=raw.len() {
        let mut parser = ProtocolParser::default();
        let mut text = String::new();
        parser.feed(&raw[..split], |s| text.push_str(s)).unwrap();
        parser.feed(&raw[split..], |s| text.push_str(s)).unwrap();
        assert_eq!(text, "é🐾");
        assert_eq!(parser.eof().unwrap().total_tokens, 0);
    }
    assert_eq!(parse(raw, 1).unwrap().0, "é🐾");
    assert_eq!(parse(&successful(""), 1).unwrap().0, "");
}

#[test]
fn last_usage_including_decreases_and_null_placeholders_wins() {
    let mut start = choice(None, None);
    start["usage"] = usage(90, 80)["usage"].clone();
    let mut placeholder = choice(Some("visible"), None);
    placeholder["usage"] = serde_json::Value::Null;
    let mut finish = choice(Some("!"), Some("stop"));
    finish["usage"] = usage(8, 9)["usage"].clone();
    let mut last = usage(1, 0);
    last["usage"]["completion_tokens_details"] = json!({"reasoning_tokens":999});
    let bytes = [
        event(start),
        event(usage(100, 100)),
        event(placeholder),
        event(finish),
        event(last),
        b"data: [DONE]\n\n".to_vec(),
    ]
    .concat();
    let (text, counts) = parse(&bytes, 3).unwrap();
    assert_eq!(text, "visible!");
    assert_eq!(
        (
            counts.input_tokens,
            counts.output_tokens,
            counts.total_tokens
        ),
        (1, 0, 1)
    );
    // Usage before finish remains a candidate even when finishing usage is null/absent.
    let bytes = [
        event(usage(u64::MAX, 0)),
        event(choice(None, Some("stop"))),
        b"data: [DONE]\n\n".to_vec(),
    ]
    .concat();
    assert_eq!(parse(&bytes, 7).unwrap().1.total_tokens, u64::MAX);
}

#[test]
fn rejects_state_machine_and_usage_faults_without_publishing_invalid_event_delta() {
    let bad_events = [
        json!({"choices":[]}),
        json!({"choices":[],"usage":null}),
        json!({"choices":[{"index":0,"delta":{"content":"secret"}}]}),
        json!({"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}),
        json!({"choices":[{"index":0,"delta":{},"finish_reason":"content_filter"}]}),
        json!({"choices":[{"index":0,"delta":{},"finish_reason":false}]}),
        json!({"choices":[{"index":1,"delta":{},"finish_reason":null}]}),
        json!({"choices":[{"index":0.0,"delta":{},"finish_reason":null}]}),
        json!({"choices":[{"delta":{},"finish_reason":null}]}),
        json!({"choices":[{"index":0,"delta":{},"finish_reason":null},{"index":0}]}),
        json!({"choices":[{"index":0,"delta":{"tool_calls":[]},"finish_reason":null}]}),
        json!({"choices":[{"index":0,"delta":{"function_call":{}},"finish_reason":null}]}),
        json!({"choices":[{"index":0,"delta":{"role":"user"},"finish_reason":null}]}),
        json!({"choices":[{"index":0,"delta":{"content":42},"finish_reason":null}]}),
        json!({"choices":[{"index":0,"delta":{"reasoning":[]},"finish_reason":null}]}),
        json!({"error":{"message":"secret"}}),
        json!({"choices":[],"usage":{}}),
        json!({"choices":[],"usage":false}),
        json!({"choices":[],"usage":{"prompt_tokens":1,"completion_tokens":2,"total_tokens":2}}),
    ];
    for bad in bad_events {
        let mut parser = ProtocolParser::default();
        parser.feed(&event(usage(2, 3)), |_| panic!()).unwrap();
        assert!(parser
            .feed(&event(bad), |_| panic!("invalid delta escaped"))
            .is_err());
        assert!(parser.feed(&successful(""), |_| panic!()).is_err());
        assert!(parser.eof().is_err());
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
            let mut parser = ProtocolParser::default();
            assert!(parser.feed(&event(bad), |_| panic!()).is_err());
        }
    }
    for raw in [
        r#"{"choices":[],"usage":{"prompt_tokens":18446744073709551616,"completion_tokens":0,"total_tokens":18446744073709551616}}"#,
        r#"{"choices":[],"usage":{"prompt_tokens":18446744073709551615,"completion_tokens":1,"total_tokens":0}}"#,
    ] {
        assert!(parse(format!("data: {raw}\n\n").as_bytes(), 1).is_err());
    }
    let mut parser = ProtocolParser::default();
    parser.feed(&event(usage(2, 3)), |_| {}).unwrap();
    let mut malformed = usage(2, 3);
    malformed["usage"]["total_tokens"] = json!(-1);
    assert!(parser.feed(&event(malformed), |_| {}).is_err());
    assert!(parser.eof().is_err());
    let finish = event(choice(None, Some("stop")));
    for suffix in [
        event(choice(None, None)),
        finish.clone(),
        event(usage(2, 3)),
    ] {
        let bytes = [finish.clone(), b"data: [DONE]\n\n".to_vec(), suffix].concat();
        assert!(parse(&bytes, 1).is_err());
    }
    for suffix in [event(choice(None, None)), finish.clone()] {
        assert!(parse(&[finish.clone(), suffix].concat(), 1).is_err());
    }
    for bytes in [
        Vec::new(),
        event(usage(1, 2)),
        finish.clone(),
        [event(usage(1, 2)), b"data: [DONE]\n\n".to_vec()].concat(),
        [finish.clone(), event(usage(1, 2))].concat(),
        [finish, event(usage(1, 2)), b"data: [DONE]\n".to_vec()].concat(),
    ] {
        assert!(parse(&bytes, 1).is_err());
    }
}

#[test]
fn rejects_bad_framing_duplicates_and_all_post_done_data() {
    for raw in [
        "event: other\n\n",
        "id: secret\n\n",
        "retry: 5\n\n",
        "unknown: x\n\n",
        "event: message\n\n",
        "event: message\nevent: message\n\n",
        "data: not-json\n\n",
        "data: {} {}\n\n",
        "data:\n\n",
        "data: {\"choices\":[],\"choices\":[]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"index\":0}]}\n\n",
        "data: {\"meta\":{\"key\":1,\"k\\u0065y\":2},\"choices\":[]}\n\n",
        "data: {\"usage\":{\"prompt_tokens\":1,\"prompt_tokens\":2},\"choices\":[]}\n\n",
    ] {
        assert!(parse(raw.as_bytes(), 2).is_err());
    }
    for raw in [b": \xff\n\n".as_slice(), b"data: \xff\n\n"] {
        assert!(parse(raw, 1).is_err());
    }
    for suffix in [
        b"data: [DONE]\n\n".as_slice(),
        b"data: {}\n\n",
        b"event: message\n",
        b"oops\n",
        b"\xc2\xa0\n",
        b": incomplete",
        b" ",
        b"\r",
        b": \xff\n",
    ] {
        assert!(parse(&[successful(""), suffix.to_vec()].concat(), 1).is_err());
    }
    let bytes = [
        event(choice(Some("partial"), Some("stop"))),
        event(usage(2, 3)),
        event(json!({"error":"secret"})),
        b"data: [DONE]\n\n".to_vec(),
    ]
    .concat();
    assert!(parse(&bytes, 1).is_err());
    let mut parser = ProtocolParser::default();
    parser.feed(b"data: secret", |_| panic!()).unwrap();
    assert!(!format!("{parser:?}").contains("secret"));
    assert!(!format!("{} {:?}", StreamError::Protocol, StreamError::Protocol).contains("secret"));
}

#[test]
fn exact_parser_limits_and_constant_retention_over_a_long_answer() {
    let mut parser = ProtocolParser::default();
    let comment = [vec![b':'], vec![b'x'; MAX_LINE_BYTES - 1], b"\n\n".to_vec()].concat();
    parser.feed(&comment, |_| panic!()).unwrap();
    assert_eq!(
        ProtocolParser::default().feed(&vec![b'x'; MAX_LINE_BYTES + 1], |_| {}),
        Err(StreamError::Limit)
    );
    // A frame exactly at the wire limit, including its blank delimiter.
    let mut parser = ProtocolParser::default();
    parser.feed(b":x\n", |_| {}).unwrap();
    for _ in 0..(MAX_FRAME_BYTES - 4) / 2 {
        parser.feed(b":\n", |_| {}).unwrap();
    }
    parser.feed(b"\n", |_| {}).unwrap();
    let mut parser = ProtocolParser::default();
    for _ in 0..MAX_FRAME_BYTES / 2 {
        parser.feed(b":\n", |_| {}).unwrap();
    }
    assert_eq!(parser.feed(b"\n", |_| {}), Err(StreamError::Limit));
    let mut parser = ProtocolParser::default();
    parser
        .feed(&vec![b'\n'; MAX_FRAME_BYTES * 2], |_| {})
        .unwrap();
    let optional = |size| {
        let mut value = choice(None, None);
        value["x"] = json!("x".repeat(size));
        event(value)
    };
    // Encoded key "x" and value quotes cost five bytes.
    ProtocolParser::default()
        .feed(&optional(MAX_OPTIONAL_BYTES - 5), |_| {})
        .unwrap();
    assert_eq!(
        ProtocolParser::default().feed(&optional(MAX_OPTIONAL_BYTES - 4), |_| {}),
        Err(StreamError::Limit)
    );
    let nested = |depth| {
        format!(
            "data: {{\"choices\":[],\"x\":{}0{}}}\n\n",
            "[".repeat(depth),
            "]".repeat(depth)
        )
    };
    assert!(parse(nested(MAX_JSON_DEPTH + 1).as_bytes(), 1).is_err());
    assert!(parse(
        format!(
            "data: {{\"x\":[{}]}}\n\n",
            vec!["0"; MAX_JSON_NODES].join(",")
        )
        .as_bytes(),
        7
    )
    .is_err());
    let mut parser = ProtocolParser::default();
    parser.feed(&successful(""), |_| {}).unwrap();
    parser
        .feed(&vec![b'\n'; MAX_TRAILER_BYTES], |_| {})
        .unwrap();
    assert!(parser.eof().is_ok());
    let mut parser = ProtocolParser::default();
    parser.feed(&successful(""), |_| {}).unwrap();
    assert_eq!(
        parser.feed(&vec![b'\n'; MAX_TRAILER_BYTES + 1], |_| {}),
        Err(StreamError::Limit)
    );

    let mut parser = ProtocolParser::default();
    let capacity = parser.retained_buffer_capacity();
    let part = "🐾".repeat(1024);
    let bytes = event(choice(Some(&part), None));
    let mut emitted = 0usize;
    for _ in 0..8192 {
        parser
            .feed(&bytes, |s| emitted = emitted.checked_add(s.len()).unwrap())
            .unwrap();
        assert_eq!(parser.retained_buffer_capacity(), capacity);
    }
    parser.feed(&successful(""), |_| {}).unwrap();
    assert!(parser.eof().is_ok());
    assert_eq!(emitted, 32 * 1024 * 1024);
    assert_eq!(capacity, MAX_LINE_BYTES + MAX_FRAME_BYTES);
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
