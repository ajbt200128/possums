//! Bounded raw fixtures for the real parser, shared by later renderer/worker tests.
//! No helper here is compiled into the production library.
#![allow(dead_code)]
use possums::inference::stream::{ProtocolParser, StreamError, StreamUsage, MAX_FRAME_BYTES};
use serde_json::{json, Value};

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
