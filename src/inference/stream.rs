//! Bounded gateway acceptance rules, not provider or invoice guarantees.
//!
//! `ProtocolParser` authenticates nothing. Only the origin-bound adapter may
//! treat its terminal counts as authenticated. No answer or event list is kept.
use serde::{
    de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor},
    Deserialize,
};
use serde_json::Value;
use std::{collections::BTreeSet, fmt};
use thiserror::Error;

pub const MAX_LINE_BYTES: usize = 64 * 1024;
pub const MAX_FRAME_BYTES: usize = 256 * 1024;
pub const MAX_JSON_DEPTH: usize = 16;
pub const MAX_JSON_NODES: usize = 8192;
pub const MAX_OPTIONAL_BYTES: usize = 16 * 1024;
pub const MAX_TRAILER_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum StreamError {
    #[error("upstream stream exceeds transport limits")]
    Limit,
    #[error("upstream stream is invalid or incomplete")]
    Protocol,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Receiving,
    Finished,
    Done,
    SuccessfulEOF,
    Failed,
}

pub struct ProtocolParser {
    state: State,
    usage: Option<StreamUsage>,
    line: Vec<u8>,
    payload: Vec<u8>,
    has_data: bool,
    has_event: bool,
    frame_bytes: usize,
    trailer_bytes: usize,
}

impl fmt::Debug for ProtocolParser {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProtocolParser")
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl Default for ProtocolParser {
    fn default() -> Self {
        Self {
            state: State::Receiving,
            usage: None,
            // Fixed capacities: no geometric growth, independently of answer length.
            line: Vec::with_capacity(MAX_LINE_BYTES),
            payload: Vec::with_capacity(MAX_FRAME_BYTES),
            has_data: false,
            has_event: false,
            frame_bytes: 0,
            trailer_bytes: 0,
        }
    }
}

impl ProtocolParser {
    /// Validate a whole event before calling `delta`. The callback must not block
    /// or retain unbounded output. Errors are absorbing; earlier deltas are not a
    /// successful answer. This method never publishes a usage candidate.
    pub fn feed(&mut self, bytes: &[u8], mut delta: impl FnMut(&str)) -> Result<(), StreamError> {
        if self.state == State::Failed || self.state == State::SuccessfulEOF {
            return Err(StreamError::Protocol);
        }
        let result = self.feed_inner(bytes, &mut delta);
        if result.is_err() {
            self.state = State::Failed;
            self.usage = None;
            self.line.clear();
            self.payload.clear();
        }
        result
    }

    /// Consuming the parser makes exactly one successful terminal return possible.
    /// The caller must invoke this only on transport EOF, never on `[DONE]`.
    pub fn eof(mut self) -> Result<StreamUsage, StreamError> {
        if self.state != State::Done || !self.line.is_empty() {
            return Err(StreamError::Protocol);
        }
        self.state = State::SuccessfulEOF;
        self.usage.ok_or(StreamError::Protocol)
    }

    /// Parser byte-buffer capacity only; excludes the bounded temporary JSON tree
    /// and transport/runtime allocations. Useful for long-stream component tests.
    pub fn retained_buffer_capacity(&self) -> usize {
        self.line.capacity() + self.payload.capacity()
    }

    fn feed_inner(
        &mut self,
        bytes: &[u8],
        delta: &mut impl FnMut(&str),
    ) -> Result<(), StreamError> {
        for &byte in bytes {
            if self.state == State::Done {
                add_bounded(&mut self.trailer_bytes, 1, MAX_TRAILER_BYTES)?;
            } else {
                add_bounded(&mut self.frame_bytes, 1, MAX_FRAME_BYTES)?;
            }
            if byte != b'\n' {
                if self.line.len() == MAX_LINE_BYTES {
                    return Err(StreamError::Limit);
                }
                self.line.push(byte);
                continue;
            }
            if self.line.last() == Some(&b'\r') {
                self.line.pop();
            }
            self.line(delta)?;
            self.line.clear();
        }
        Ok(())
    }

    fn line(&mut self, delta: &mut impl FnMut(&str)) -> Result<(), StreamError> {
        let line = std::str::from_utf8(&self.line).map_err(|_| StreamError::Protocol)?;
        if self.state == State::Done {
            if line.starts_with(':') || line.bytes().all(|b| b.is_ascii_whitespace()) {
                return Ok(());
            }
            return Err(StreamError::Protocol);
        }
        if line.is_empty() {
            if self.has_data {
                self.event(delta)?;
            } else if self.has_event {
                return Err(StreamError::Protocol);
            }
            self.payload.clear();
            self.has_data = false;
            self.has_event = false;
            self.frame_bytes = 0;
            return Ok(());
        }
        if line.starts_with(':') {
            return Ok(());
        }
        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "data" => {
                let mut size = self.payload.len();
                add_bounded(&mut size, usize::from(self.has_data), MAX_FRAME_BYTES)?;
                add_bounded(&mut size, value.len(), MAX_FRAME_BYTES)?;
                if self.has_data {
                    self.payload.push(b'\n');
                }
                self.payload.extend_from_slice(value.as_bytes());
                self.has_data = true;
            }
            "event" if !self.has_event && (value.is_empty() || value == "message") => {
                self.has_event = true;
            }
            _ => return Err(StreamError::Protocol),
        }
        Ok(())
    }

    fn event(&mut self, delta: &mut impl FnMut(&str)) -> Result<(), StreamError> {
        if self.payload == b"[DONE]" {
            if self.state != State::Finished || self.usage.is_none() {
                return Err(StreamError::Protocol);
            }
            self.state = State::Done;
            return Ok(());
        }
        validate_json(&self.payload)?;
        // Deserialize wire integers directly as well: arbitrary_precision Value
        // accepts serde's private number-map representation, not just JSON numbers.
        let wire: WireCounts =
            serde_json::from_slice(&self.payload).map_err(|_| StreamError::Protocol)?;
        if wire.choices.iter().any(|choice| choice.index != 0) {
            return Err(StreamError::Protocol);
        }
        let value: Value =
            serde_json::from_slice(&self.payload).map_err(|_| StreamError::Protocol)?;
        let object = value.as_object().ok_or(StreamError::Protocol)?;
        if object.contains_key("error") {
            return Err(StreamError::Protocol);
        }
        let mut optional_bytes = 0;
        optional_fields(object, &["choices", "usage"], &mut optional_bytes)?;
        let usage = wire
            .usage
            .map(|usage| {
                optional_fields(
                    object["usage"].as_object().ok_or(StreamError::Protocol)?,
                    &["prompt_tokens", "completion_tokens", "total_tokens"],
                    &mut optional_bytes,
                )?;
                usage.validate()
            })
            .transpose()?;
        let choices = object
            .get("choices")
            .and_then(Value::as_array)
            .ok_or(StreamError::Protocol)?;
        let mut content = None;
        let mut finished = false;
        match choices.as_slice() {
            [] if usage.is_some() => {}
            [choice] if self.state == State::Receiving => {
                let choice = choice.as_object().ok_or(StreamError::Protocol)?;
                if choice
                    .keys()
                    .any(|k| !["index", "delta", "finish_reason", "logprobs"].contains(&k.as_str()))
                    || choice.get("index").and_then(Value::as_u64) != Some(0)
                {
                    return Err(StreamError::Protocol);
                }
                optional_fields(
                    choice,
                    &["index", "delta", "finish_reason"],
                    &mut optional_bytes,
                )?;
                finished = match choice.get("finish_reason") {
                    Some(Value::Null) => false,
                    Some(Value::String(reason)) if reason == "stop" || reason == "length" => true,
                    _ => return Err(StreamError::Protocol),
                };
                let fields = choice
                    .get("delta")
                    .and_then(Value::as_object)
                    .ok_or(StreamError::Protocol)?;
                for (key, value) in fields {
                    match key.as_str() {
                        "content" => {
                            if !value.is_null() {
                                content = Some(value.as_str().ok_or(StreamError::Protocol)?);
                            }
                        }
                        "role" if value.as_str() == Some("assistant") => {}
                        "reasoning" | "reasoning_content"
                            if value.is_null() || value.is_string() => {}
                        _ => return Err(StreamError::Protocol),
                    }
                }
                optional_fields(fields, &["content"], &mut optional_bytes)?;
            }
            _ => return Err(StreamError::Protocol),
        }
        // Commit state and expose content only after validating all fields.
        if let Some(usage) = usage {
            self.usage = Some(usage);
        }
        if finished {
            self.state = State::Finished;
        }
        if let Some(content) = content.filter(|s| !s.is_empty()) {
            delta(content);
        }
        Ok(())
    }
}

#[derive(Deserialize)]
struct WireCounts {
    choices: Vec<WireIndex>,
    usage: Option<WireUsage>,
}

#[derive(Deserialize)]
struct WireIndex {
    index: u64,
}

#[derive(Deserialize)]
struct WireUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
}

impl WireUsage {
    fn validate(self) -> Result<StreamUsage, StreamError> {
        if self.prompt_tokens.checked_add(self.completion_tokens) != Some(self.total_tokens) {
            return Err(StreamError::Protocol);
        }
        Ok(StreamUsage {
            input_tokens: self.prompt_tokens,
            output_tokens: self.completion_tokens,
            total_tokens: self.total_tokens,
        })
    }
}

fn add_bounded(count: &mut usize, amount: usize, limit: usize) -> Result<(), StreamError> {
    *count = count.checked_add(amount).ok_or(StreamError::Limit)?;
    if *count > limit {
        return Err(StreamError::Limit);
    }
    Ok(())
}

// Count compact encoded optional fields without allocating another serialization.
fn optional_fields(
    object: &serde_json::Map<String, Value>,
    required: &[&str],
    count: &mut usize,
) -> Result<(), StreamError> {
    struct Counter<'a>(&'a mut usize);
    impl std::io::Write for Counter<'_> {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            add_bounded(self.0, bytes.len(), MAX_OPTIONAL_BYTES)
                .map_err(|_| std::io::Error::other("optional field limit"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    for (key, value) in object {
        if !required.contains(&key.as_str()) {
            serde_json::to_writer(Counter(count), key).map_err(|_| StreamError::Limit)?;
            serde_json::to_writer(Counter(count), value).map_err(|_| StreamError::Limit)?;
        }
    }
    Ok(())
}

// Validate before building Value (which otherwise silently overwrites duplicate
// object keys). Both passes are bounded by frame size; this pass also bounds the
// tree's node count and depth before it can be allocated.
fn validate_json(bytes: &[u8]) -> Result<(), StreamError> {
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let mut nodes = 0;
    JsonGuard {
        depth: 0,
        nodes: &mut nodes,
    }
    .deserialize(&mut deserializer)
    .map_err(|_| StreamError::Protocol)?;
    deserializer.end().map_err(|_| StreamError::Protocol)
}

struct JsonGuard<'a> {
    depth: usize,
    nodes: &'a mut usize,
}

impl<'de> DeserializeSeed<'de> for JsonGuard<'_> {
    type Value = ();
    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        add_bounded(self.nodes, 1, MAX_JSON_NODES).map_err(de::Error::custom)?;
        if self.depth > MAX_JSON_DEPTH {
            return Err(de::Error::custom("JSON depth limit"));
        }
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for JsonGuard<'_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("bounded JSON")
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> Result<(), E> {
        Ok(())
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> Result<(), E> {
        Ok(())
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> Result<(), E> {
        Ok(())
    }
    fn visit_f64<E: de::Error>(self, _: f64) -> Result<(), E> {
        Ok(())
    }
    fn visit_str<E: de::Error>(self, _: &str) -> Result<(), E> {
        Ok(())
    }
    fn visit_unit<E: de::Error>(self) -> Result<(), E> {
        Ok(())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        while seq
            .next_element_seed(JsonGuard {
                depth: self.depth + 1,
                nodes: self.nodes,
            })?
            .is_some()
        {}
        Ok(())
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        let mut keys = BTreeSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if !keys.insert(key) {
                return Err(de::Error::custom("duplicate JSON key"));
            }
            map.next_value_seed(JsonGuard {
                depth: self.depth + 1,
                nodes: self.nodes,
            })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_depth_and_node_boundaries_are_enforced_before_value_allocation() {
        for depth in [MAX_JSON_DEPTH, MAX_JSON_DEPTH + 1] {
            let json = format!("{}0{}", "[".repeat(depth), "]".repeat(depth));
            assert_eq!(
                validate_json(json.as_bytes()).is_ok(),
                depth == MAX_JSON_DEPTH
            );
        }
        for nodes in [MAX_JSON_NODES, MAX_JSON_NODES + 1] {
            let json = format!("[{}]", vec!["0"; nodes - 1].join(","));
            assert_eq!(
                validate_json(json.as_bytes()).is_ok(),
                nodes == MAX_JSON_NODES
            );
        }
        assert!(validate_json(br#"{"optional":1.5}"#).is_ok());
    }
}
