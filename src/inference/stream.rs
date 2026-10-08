//! Bounded gateway acceptance rules, not provider or invoice guarantees.
//!
//! `ProtocolParser` authenticates nothing. Only the origin-bound adapter may
//! treat its terminal counts as authenticated. No answer or event list is kept.
use super::{
    tools::{
        CompletionDelta, ToolInvocation, ToolProfile, MAX_ARGUMENT_BYTES, MAX_CALLS,
        MAX_TOTAL_ARGUMENT_BYTES,
    },
    InferenceError, InferenceFailure, Message,
};
use futures_util::StreamExt;
use rand::RngCore;
use serde::{
    de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor},
    Deserialize, Serialize,
};
use serde_json::Value;
use std::{collections::BTreeSet, fmt, time::Duration};
use thiserror::Error;
use tinfoil::chat::{ChatChoiceStream, ChatCompletionMessageToolCallChunk};
use tokio::time::Instant;

pub const MAX_LINE_BYTES: usize = 64 * 1024;
pub const MAX_FRAME_BYTES: usize = 256 * 1024;
pub const MAX_JSON_DEPTH: usize = 16;
pub const MAX_JSON_NODES: usize = 8192;
pub const MAX_OPTIONAL_BYTES: usize = 16 * 1024;
pub const MAX_TRAILER_BYTES: usize = 64 * 1024;
/// Maximum borrowed parser feed slice. Reqwest may yield a larger HTTP/1 DATA
/// backing; retain the whole chunk while feeding bounded slices, not a full answer.
/// Producer/TLS allocations are separate from this parser limit.
pub const MAX_TRANSPORT_BUFFER_BYTES: usize = 256 * 1024;
const MAX_REQUEST_BODY_BYTES: usize = 16 * 1024 * 1024;
pub const STREAM_DEADLINE: Duration = Duration::from_secs(300);
pub const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(60);

fn generation_bytes(
    builder: tinfoil::relaxed::RelaxedChatRequestBuilder,
    max_tokens: u64,
) -> Result<Vec<u8>, InferenceError> {
    let mut scope = [0u8; 32];
    rand::rng().fill_bytes(&mut scope);
    // The SDK constructs the provider body; transport remains verified, leased
    // and noncloneable. Drop its temporary JSON before transferring upload DATA.
    let request = builder
        .set("max_tokens", max_tokens)
        .set("stream", true)
        .set("stream_options", serde_json::json!({"include_usage": true}))
        .set("n", 1)
        .set(
            "user_cache_secret",
            base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, scope),
        )
        .build();
    crate::bounded_json::to_vec(&request, MAX_REQUEST_BODY_BYTES)
        .map_err(|_| InferenceFailure::RequestEncodingFailed.into())
}

pub(super) fn request_body(
    model: &str,
    max_output_tokens: u64,
    messages: &[Message],
    heavy: Option<std::sync::Arc<crate::telemetry::hooks::Lease>>,
) -> Result<reqwest::Body, InferenceError> {
    let bytes = generation_bytes(super::text_request(model, messages)?, max_output_tokens)?;
    Ok(super::upload::body(bytes, heavy).0)
}

pub(super) fn invocation_body(
    model: &str,
    max_tokens: u64,
    invocation: &ToolInvocation,
    heavy: std::sync::Arc<crate::telemetry::hooks::Lease>,
) -> Result<reqwest::Body, InferenceError> {
    let bytes = generation_bytes(super::invocation_request(model, invocation)?, max_tokens)?;
    Ok(super::upload::body(bytes, Some(heavy)).0)
}

// Private: arbitrary unauthenticated Responses must never become an inference
// entry point. Unit tests alone may inject raw HTTP here; ProtocolParser itself
// remains reusable but makes no authentication claim.
#[cfg(test)]
pub(super) async fn consume_response(
    response: reqwest::Response,
    deadline: Instant,
    idle_timeout: Duration,
    on_delta: impl FnMut(&str),
) -> Result<StreamUsage, InferenceError> {
    consume_completion_response(response, deadline, idle_timeout, on_delta)
        .await
        .map(|completion| completion.usage)
}

pub(super) async fn consume_completion_response(
    response: reqwest::Response,
    deadline: Instant,
    idle_timeout: Duration,
    mut on_delta: impl FnMut(&str),
) -> Result<StreamCompletion, InferenceError> {
    consume_with_parser(
        response,
        deadline,
        idle_timeout,
        ProtocolParser::default(),
        |event| {
            if let CompletionDelta::Text(text) = event {
                on_delta(text);
            }
        },
    )
    .await
}

pub(super) async fn consume_invocation_response(
    response: reqwest::Response,
    deadline: Instant,
    idle_timeout: Duration,
    _profile: ToolProfile,
    _invocation: &ToolInvocation,
    on_delta: impl FnMut(CompletionDelta<'_>),
) -> Result<StreamCompletion, InferenceError> {
    if !response.status().is_success() {
        return Err(InferenceFailure::GenerationHttpFailed.into()); // Never read an error body.
    }
    let sse = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.split(';')
                .next()
                .is_some_and(|v| v.trim().eq_ignore_ascii_case("text/event-stream"))
        });
    if !sse {
        return Err(InferenceFailure::StreamContentTypeInvalid.into());
    }
    // Idle applies to HTTP reads, not SDK events: comments and fragmented JSON
    // are progress too. Keep the origin-bound, non-replayable request upstream.
    // SDK erases byte-stream errors. Keep only a fixed category, never bytes or
    // error text; this single-slot side channel lives only for this consumption.
    let transport_failure = std::sync::Arc::new(std::sync::Mutex::new(None));
    let byte_failure = transport_failure.clone();
    let bytes = futures_util::stream::unfold(response, move |mut response| {
        let byte_failure = byte_failure.clone();
        async move {
            let next_deadline = deadline.min(Instant::now() + idle_timeout);
            let chunk = tokio::time::timeout_at(next_deadline, response.chunk()).await;
            let failure = match chunk {
                Ok(Ok(chunk)) if Instant::now() < next_deadline => {
                    return chunk.map(|chunk| (Ok::<_, InferenceFailure>(chunk), response));
                }
                Ok(Err(_)) => InferenceFailure::StreamTransportFailed,
                _ => timeout_failure(deadline),
            };
            *byte_failure.lock().unwrap() = Some(failure);
            Some((Err(failure), response))
        }
    });
    let mut events = tinfoil::sse::parse_event_stream(bytes);
    let mut validator = SdkToolValidator::default();
    let mut on_delta = on_delta;
    loop {
        // Also bound processing of events already buffered inside the SDK.
        if Instant::now() >= deadline {
            return Err(InferenceError::Detailed(
                InferenceFailure::StreamDeadlineExceeded,
            ));
        }
        let event = tokio::time::timeout_at(deadline, events.next())
            .await
            .map_err(|_| InferenceError::Detailed(InferenceFailure::StreamDeadlineExceeded))?;
        if Instant::now() >= deadline {
            return Err(InferenceError::Detailed(
                InferenceFailure::StreamDeadlineExceeded,
            ));
        }
        match event {
            Some(event) => {
                // SDK errors can contain upstream payloads. Never display them.
                let event = event.map_err(|_| {
                    InferenceError::from(
                        transport_failure
                            .lock()
                            .unwrap()
                            .unwrap_or(InferenceFailure::SdkStreamDecodeFailed),
                    )
                })?;
                validator
                    .accept(event, &mut on_delta)
                    .map_err(|error| InferenceError::from(error.failure()))?;
            }
            None => {
                return validator
                    .eof_completion()
                    .map_err(|error| InferenceError::from(error.failure()))
            }
        }
    }
}

fn timeout_failure(deadline: Instant) -> InferenceFailure {
    if Instant::now() >= deadline {
        InferenceFailure::StreamDeadlineExceeded
    } else {
        InferenceFailure::StreamIdleTimeout
    }
}

async fn consume_with_parser(
    mut response: reqwest::Response,
    deadline: Instant,
    idle_timeout: Duration,
    mut parser: ProtocolParser,
    mut on_delta: impl FnMut(CompletionDelta<'_>),
) -> Result<StreamCompletion, InferenceError> {
    if !response.status().is_success() {
        return Err(InferenceFailure::GenerationHttpFailed.into()); // Do not read error bodies.
    }
    let sse = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.split(';')
                .next()
                .is_some_and(|v| v.trim().eq_ignore_ascii_case("text/event-stream"))
        });
    if !sse {
        return Err(InferenceFailure::StreamContentTypeInvalid.into());
    }
    loop {
        let next_deadline = deadline.min(Instant::now() + idle_timeout);
        let chunk = tokio::time::timeout_at(next_deadline, response.chunk())
            .await
            .map_err(|_| timeout_failure(deadline))?
            .map_err(|_| InferenceFailure::StreamTransportFailed)?;
        if Instant::now() >= next_deadline {
            return Err(timeout_failure(deadline).into());
        }
        match chunk {
            Some(bytes) => {
                for fragment in bytes.chunks(MAX_TRANSPORT_BUFFER_BYTES) {
                    parser
                        .feed_events(fragment, &mut on_delta)
                        .map_err(|_| InferenceError::InvalidResponse)?;
                }
            }
            None => {
                return parser
                    .eof_completion()
                    .map_err(|_| InferenceError::InvalidResponse)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    Length,
    #[serde(rename = "tool_calls")]
    ToolUse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamCompletion {
    pub usage: StreamUsage,
    pub finish_reason: FinishReason,
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum StreamError {
    #[error("upstream stream exceeds transport limits")]
    Limit,
    #[error("upstream stream is invalid or incomplete")]
    Protocol,
    #[error(transparent)]
    Detailed(#[from] InferenceFailure),
}

impl StreamError {
    pub fn failure(&self) -> InferenceFailure {
        match self {
            Self::Detailed(failure) => *failure,
            Self::Limit | Self::Protocol => InferenceFailure::UpstreamResponseInvalid,
        }
    }
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
    finish_reason: Option<FinishReason>,
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
            finish_reason: None,
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

#[derive(Clone, Default)]
struct CallState {
    id: Option<String>,
    name: Option<String>,
    argument_bytes: usize,
}
#[derive(Clone, Default)]
struct ToolState {
    calls: Vec<CallState>,
    argument_bytes: usize,
}
impl ToolState {
    fn proposed<'a>(
        &mut self,
        calls: &'a [ChatCompletionMessageToolCallChunk],
    ) -> Result<Vec<CompletionDelta<'a>>, StreamError> {
        if calls.len() > MAX_CALLS {
            return Err(InferenceFailure::ToolIndexInvalid.into());
        }
        let mut seen = BTreeSet::new();
        let mut deltas = Vec::with_capacity(calls.len());
        for call in calls {
            let index = u64::from(call.index);
            if index >= MAX_CALLS as u64 {
                return Err(InferenceFailure::ToolIndexInvalid.into());
            }
            let index = index as usize;
            if !seen.insert(index) || index > self.calls.len() {
                return Err(InferenceFailure::ToolIndexInvalid.into());
            }
            if index == self.calls.len() {
                self.calls.push(CallState::default());
            }
            // SDK-style placeholders contribute nothing; never erase identity.
            let id = call.id.as_deref().filter(|id| !id.is_empty());
            if let Some(id) = id {
                if id.len() > 128
                    || self
                        .calls
                        .iter()
                        .enumerate()
                        .any(|(i, c)| i != index && c.id.as_deref() == Some(id))
                {
                    return Err(InferenceFailure::ToolIdentityInvalid.into());
                }
            }
            let state = &mut self.calls[index];
            if let Some(id) = id {
                if state.id.as_ref().is_some_and(|old| old != id) {
                    return Err(InferenceFailure::ToolIdentityInvalid.into());
                }
                state.id = Some(id.to_owned());
            }
            let mut name = None;
            let mut arguments = None;
            if let Some(function) = &call.function {
                name = function.name.as_deref().filter(|name| !name.is_empty());
                arguments = function.arguments.as_deref();
                if let Some(name) = name {
                    if name.len() > 64 || state.name.as_ref().is_some_and(|old| old != name) {
                        return Err(InferenceFailure::ToolIdentityInvalid.into());
                    }
                    state.name = Some(name.to_owned());
                }
                if let Some(arguments) = arguments {
                    add_bounded(
                        &mut state.argument_bytes,
                        arguments.len(),
                        MAX_ARGUMENT_BYTES,
                    )
                    .map_err(|_| InferenceFailure::ToolArgumentsTooLarge)?;
                    add_bounded(
                        &mut self.argument_bytes,
                        arguments.len(),
                        MAX_TOTAL_ARGUMENT_BYTES,
                    )
                    .map_err(|_| InferenceFailure::ToolArgumentsTooLarge)?;
                }
            }
            deltas.push(CompletionDelta::ToolCall {
                index,
                id,
                name,
                arguments,
            });
        }
        Ok(deltas)
    }
}

/// Application acceptance of SDK-decoded tool events; authenticates nothing.
/// Only the origin-bound adapter may treat the resulting receipt as authenticated.
/// Call `eof_completion` only after actual successful SDK/transport exhaustion.
/// Request policy and executable-call completeness belong to the caller; finish
/// reasons are forwarded unchanged, including `stop` with partial tool calls.
#[derive(Default)]
pub struct SdkToolValidator {
    tools: ToolState,
    usage: Option<StreamUsage>,
    finish_usage: Option<StreamUsage>,
    continuous_usage: bool,
    finish_reason: Option<FinishReason>,
    failed: bool,
}

impl SdkToolValidator {
    /// Validate the entire event before any callbacks; errors are absorbing.
    pub fn accept(
        &mut self,
        event: Value,
        delta: impl FnMut(CompletionDelta<'_>),
    ) -> Result<(), StreamError> {
        if self.failed {
            return Err(StreamError::Protocol);
        }
        let result = self.accept_inner(event, delta);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    fn accept_inner(
        &mut self,
        event: Value,
        mut delta: impl FnMut(CompletionDelta<'_>),
    ) -> Result<(), StreamError> {
        // Use SDK choice/delta/tool types without requiring envelope metadata.
        // Usage is u64 here (the SDK's CompletionUsage is only u32).
        #[derive(Deserialize)]
        struct Chunk {
            choices: Vec<ChatChoiceStream>,
            usage: Option<WireUsage>,
        }
        if event.get("error").is_some() {
            return Err(InferenceFailure::UpstreamErrorEvent.into());
        }
        // Validate usage separately so serde shape failures stay stage-specific.
        if let Some(usage) = event.get("usage").filter(|usage| !usage.is_null()) {
            serde_json::from_value::<WireUsage>(usage.clone())
                .map_err(|_| InferenceFailure::StreamUsageInvalid)?;
        }
        let chunk: Chunk = serde_json::from_value(event)
            .map_err(|_| InferenceFailure::StreamEventSchemaInvalid)?;
        let usage = chunk
            .usage
            .map(WireUsage::validate)
            .transpose()
            .map_err(|_| InferenceFailure::StreamUsageInvalid)?;
        let mut tools = self.tools.clone();
        let mut content = None;
        let mut tool_deltas = Vec::new();
        let mut finish = None;
        match chunk.choices.as_slice() {
            [] if usage.is_some() => {}
            [choice] if choice.index == 0 && self.finish_reason.is_none() => {
                use tinfoil::chat::{FinishReason as SdkFinish, Role};
                let fields = &choice.delta;
                #[allow(deprecated)]
                if fields.function_call.is_some()
                    || fields.refusal.is_some()
                    || fields.role.as_ref().is_some_and(|r| *r != Role::Assistant)
                {
                    return Err(InferenceFailure::StreamDeltaUnsupported.into());
                }
                content = fields.content.as_deref();
                if let Some(calls) = &fields.tool_calls {
                    tool_deltas = tools.proposed(calls)?;
                }
                finish = match choice.finish_reason {
                    None => None,
                    Some(SdkFinish::Stop) => Some(FinishReason::Stop),
                    Some(SdkFinish::Length) => Some(FinishReason::Length),
                    Some(SdkFinish::ToolCalls) => Some(FinishReason::ToolUse),
                    _ => return Err(InferenceFailure::StreamFinishInvalid.into()),
                };
            }
            _ if self.finish_reason.is_some() && !chunk.choices.is_empty() => {
                return Err(InferenceFailure::StreamOutputAfterFinish.into());
            }
            _ => return Err(InferenceFailure::StreamChoiceInvalid.into()),
        }
        if chunk.choices.is_empty() && (self.usage.is_some() || self.finish_reason.is_none()) {
            return Err(InferenceFailure::StreamUsageUnexpected.into());
        }
        self.tools = tools;
        if let Some(usage) = usage {
            if chunk.choices.is_empty() {
                // Exactly one post-finish usage event is definitive, even when
                // its counts differ from the terminal choice's snapshot.
                self.usage = Some(usage);
            } else if finish.is_some() {
                self.finish_usage = Some(usage);
            } else {
                // Valid continuous choice usage is metadata, never billable.
                self.continuous_usage = true;
            }
        }
        if let Some(finish) = finish {
            self.finish_reason = Some(finish);
        }
        if let Some(content) = content.filter(|s| !s.is_empty()) {
            delta(CompletionDelta::Text(content));
        }
        for fragment in tool_deltas {
            delta(fragment);
        }
        Ok(())
    }

    pub fn eof_completion(self) -> Result<StreamCompletion, StreamError> {
        if self.failed {
            return Err(StreamError::Protocol);
        }
        // The SDK suppresses [DONE]; real finish, final usage and EOF suffice.
        Ok(StreamCompletion {
            finish_reason: self
                .finish_reason
                .ok_or(InferenceFailure::StreamFinishMissing)?,
            // Preserve finish-only usage for non-continuous streams. Once an
            // interim snapshot appeared, only separate final usage can suffice.
            usage: self
                .usage
                .or(self.finish_usage.filter(|_| !self.continuous_usage))
                .ok_or(InferenceFailure::StreamUsageMissing)?,
        })
    }
}

impl ProtocolParser {
    /// Validate a whole event before calling `delta`. The callback must not block
    /// or retain unbounded output. Errors are absorbing; earlier deltas are not a
    /// successful answer. This method never publishes a usage candidate.
    pub fn feed(&mut self, bytes: &[u8], mut delta: impl FnMut(&str)) -> Result<(), StreamError> {
        self.feed_events(bytes, |event| {
            if let CompletionDelta::Text(text) = event {
                delta(text);
            }
        })
    }

    fn feed_events(
        &mut self,
        bytes: &[u8],
        mut delta: impl FnMut(CompletionDelta<'_>),
    ) -> Result<(), StreamError> {
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
    pub fn eof(self) -> Result<StreamUsage, StreamError> {
        self.eof_completion().map(|completion| completion.usage)
    }

    pub fn eof_completion(mut self) -> Result<StreamCompletion, StreamError> {
        if self.state != State::Done || !self.line.is_empty() {
            return Err(StreamError::Protocol);
        }
        self.state = State::SuccessfulEOF;
        Ok(StreamCompletion {
            usage: self.usage.ok_or(StreamError::Protocol)?,
            finish_reason: self.finish_reason.ok_or(StreamError::Protocol)?,
        })
    }

    /// Parser byte-buffer capacity only; excludes the bounded temporary JSON tree
    /// and transport/runtime allocations. Useful for long-stream component tests.
    pub fn retained_buffer_capacity(&self) -> usize {
        self.line.capacity() + self.payload.capacity()
    }

    fn feed_inner(
        &mut self,
        bytes: &[u8],
        delta: &mut impl FnMut(CompletionDelta<'_>),
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

    fn line(&mut self, delta: &mut impl FnMut(CompletionDelta<'_>)) -> Result<(), StreamError> {
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

    fn event(&mut self, delta: &mut impl FnMut(CompletionDelta<'_>)) -> Result<(), StreamError> {
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
        let mut finish_reason = None;
        match choices.as_slice() {
            [] if usage.is_some() => {}
            [choice] if self.state == State::Receiving => {
                let choice = choice.as_object().ok_or(StreamError::Protocol)?;
                if choice.keys().any(|k| {
                    ![
                        "index",
                        "delta",
                        "finish_reason",
                        "logprobs",
                        "token_ids",
                        "stop_reason",
                    ]
                    .contains(&k.as_str())
                }) || choice.get("index").and_then(Value::as_u64) != Some(0)
                {
                    return Err(StreamError::Protocol);
                }
                optional_fields(
                    choice,
                    &["index", "delta", "finish_reason"],
                    &mut optional_bytes,
                )?;
                finish_reason = match choice.get("finish_reason") {
                    Some(Value::Null) => None,
                    Some(Value::String(reason)) if reason == "stop" => Some(FinishReason::Stop),
                    Some(Value::String(reason)) if reason == "length" => Some(FinishReason::Length),
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
                        "p" => {} // Authenticated backend metadata; bounded and never rendered.
                        _ => return Err(StreamError::Protocol),
                    }
                }
                optional_fields(fields, &["content", "tool_calls"], &mut optional_bytes)?;
            }
            _ => return Err(StreamError::Protocol),
        }
        // Commit state and expose content only after validating all fields.
        if let Some(usage) = usage {
            self.usage = Some(usage);
        }
        if let Some(reason) = finish_reason {
            self.finish_reason = Some(reason);
            self.state = State::Finished;
        }
        if let Some(content) = content.filter(|s| !s.is_empty()) {
            delta(CompletionDelta::Text(content));
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
pub(super) fn validate_json(bytes: &[u8]) -> Result<(), StreamError> {
    validate_json_limits(bytes, MAX_JSON_DEPTH, MAX_JSON_NODES)
}

// Input bodies already have a byte bound and a heavy lease. Guard opaque schema
// objects before Value can silently discard duplicate keys or expand a deep tree.
pub(crate) fn validate_request_json(bytes: &[u8]) -> Result<(), StreamError> {
    validate_json_limits(bytes, 64, 128 * 1024)
}

fn validate_json_limits(
    bytes: &[u8],
    max_depth: usize,
    max_nodes: usize,
) -> Result<(), StreamError> {
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let mut nodes = 0;
    JsonGuard {
        depth: 0,
        nodes: &mut nodes,
        max_depth,
        max_nodes,
    }
    .deserialize(&mut deserializer)
    .map_err(|_| StreamError::Protocol)?;
    deserializer.end().map_err(|_| StreamError::Protocol)
}

struct JsonGuard<'a> {
    depth: usize,
    nodes: &'a mut usize,
    max_depth: usize,
    max_nodes: usize,
}

impl<'de> DeserializeSeed<'de> for JsonGuard<'_> {
    type Value = ();
    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        add_bounded(self.nodes, 1, self.max_nodes).map_err(de::Error::custom)?;
        if self.depth > self.max_depth {
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
                max_depth: self.max_depth,
                max_nodes: self.max_nodes,
            })?
            .is_some()
        {}
        Ok(())
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        let mut keys = BTreeSet::new();
        while let Some(key) = map.next_key::<String>()? {
            // serde_json's RawValue key reparses a string without this guard.
            if key == "$serde_json::private::RawValue" {
                return Err(de::Error::custom("reserved JSON key"));
            }
            if !keys.insert(key) {
                return Err(de::Error::custom("duplicate JSON key"));
            }
            map.next_value_seed(JsonGuard {
                depth: self.depth + 1,
                nodes: self.nodes,
                max_depth: self.max_depth,
                max_nodes: self.max_nodes,
            })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inference::stream_support::{choice, event, raw_response, successful, usage};
    use http_body_util::BodyExt;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn heavy() -> Arc<crate::telemetry::hooks::Lease> {
        Arc::new(
            Arc::new(tokio::sync::Semaphore::new(1))
                .try_acquire_owned()
                .unwrap()
                .into(),
        )
    }

    #[tokio::test]
    async fn sdk_consumer_distinguishes_decode_semantics_eof_and_transport_without_content() {
        use InferenceFailure::*;
        let invocation = tool_input();
        for (body, failure) in [
            ("data: private-marker\n\n", SdkStreamDecodeFailed),
            (
                "data: {\"error\":{\"message\":\"private-marker\"}}\n\n",
                UpstreamErrorEvent,
            ),
            (
                "data: {\"choices\":\"private-marker\"}\n\n",
                StreamEventSchemaInvalid,
            ),
            ("data: {\"choices\":[]}\n\n", StreamChoiceInvalid),
            ("data: [DONE]\n\n", StreamFinishMissing),
            (
                "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                StreamUsageMissing,
            ),
        ] {
            let (response, peer) = raw_response().await;
            peer.send(body.as_bytes(), 4096).await;
            peer.eof().await;
            let error = consume_invocation_response(
                response,
                Instant::now() + Duration::from_secs(2),
                Duration::from_secs(1),
                ToolProfile::OpenAiFunctionsV1,
                &invocation,
                |_| {},
            )
            .await
            .unwrap_err();
            assert_eq!(error.failure(), failure);
            assert!(!format!(
                "{error:?} {error} {}",
                serde_json::to_string(&error.failure()).unwrap()
            )
            .contains("private-marker"));
            peer.finished().await;
        }
        for failure in [
            StreamTransportFailed,
            StreamIdleTimeout,
            StreamDeadlineExceeded,
        ] {
            let (response, peer) = raw_response().await;
            if failure == StreamTransportFailed {
                peer.fault().await;
            }
            let deadline = Instant::now()
                + if failure == StreamDeadlineExceeded {
                    Duration::from_millis(20)
                } else {
                    Duration::from_secs(2)
                };
            let idle = if failure == StreamIdleTimeout {
                Duration::from_millis(20)
            } else {
                Duration::from_secs(1)
            };
            let error = consume_invocation_response(
                response,
                deadline,
                idle,
                ToolProfile::OpenAiFunctionsV1,
                &invocation,
                |_| {},
            )
            .await
            .unwrap_err();
            assert_eq!(error.failure(), failure);
        }
        for (status, content_type, failure) in [
            (503, "text/event-stream", GenerationHttpFailed),
            (200, "private-marker", StreamContentTypeInvalid),
        ] {
            let response = http::Response::builder()
                .status(status)
                .header("content-type", content_type)
                .body("private-marker")
                .unwrap()
                .into();
            let error = consume_invocation_response(
                response,
                Instant::now() + Duration::from_secs(1),
                Duration::from_secs(1),
                ToolProfile::OpenAiFunctionsV1,
                &invocation,
                |_| {},
            )
            .await
            .unwrap_err();
            assert_eq!(error.failure(), failure);
            assert!(!error.to_string().contains("private-marker"));
        }
    }

    #[tokio::test]
    async fn sdk_consumer_preserves_stop_independently_of_request_policy() {
        use serde_json::json;
        for policy in [
            None,
            Some(json!("none")),
            Some(json!("required")),
            Some(json!({"type":"function","function":{"name":"named"}})),
        ] {
            let input = serde_json::to_value(tool_input()).unwrap();
            let invocation = ToolInvocation::new(
                serde_json::from_value(input["messages"].clone()).unwrap(),
                policy
                    .as_ref()
                    .map(|_| serde_json::from_value(input["tools"].clone()).unwrap()),
                policy.map(|policy| serde_json::from_value(policy).unwrap()),
            )
            .unwrap();
            for with_call in [false, true] {
                let delta = if with_call {
                    json!({"tool_calls":[{"index":0,"function":{"name":"undeclared"}}]})
                } else {
                    json!({})
                };
                let bytes = [
                    event(json!({"choices":[{"index":0,"delta":delta,"finish_reason":"stop"}]})),
                    event(usage(2, 3)),
                ]
                .concat();
                let response = http::Response::builder()
                    .header("content-type", "text/event-stream")
                    .body(bytes)
                    .unwrap()
                    .into();
                let mut delivered = 0;
                let completion = consume_invocation_response(
                    response,
                    Instant::now() + Duration::from_secs(1),
                    STREAM_IDLE_TIMEOUT,
                    ToolProfile::OpenAiFunctionsV1,
                    &invocation,
                    |_| delivered += 1,
                )
                .await
                .unwrap();
                assert_eq!(delivered, usize::from(with_call));
                assert_eq!(completion.finish_reason, FinishReason::Stop);
                assert_eq!(completion.usage.total_tokens, 5);
            }
        }
    }

    #[test]
    fn sdk_semantic_categories_are_atomic_and_absorbing() {
        use serde_json::json;
        use InferenceFailure::*;
        let delta = |v: Value| json!({"choices":[{"index":0,"delta":v,"finish_reason":null}]});
        let call = |index, id: &str, name: &str, args: String| json!({"index":index,"id":id,"type":"function","function":{"name":name,"arguments":args}});
        let finish = |reason| json!({"choices":[{"index":0,"delta":{},"finish_reason":reason}]});
        let first = delta(json!({"tool_calls":[call(0, "a", "lookup", "{}".into())]}));
        let final_usage = json!({"choices":[],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}});
        let cases = vec![
            (
                vec![],
                delta(json!({"refusal":"private-marker"})),
                StreamDeltaUnsupported,
            ),
            (
                vec![],
                delta(json!({"tool_calls":[call(1,"a","lookup","{}".into())]})),
                ToolIndexInvalid,
            ),
            (
                vec![first.clone()],
                delta(json!({"tool_calls":[call(0,"b","lookup","{}".into())]})),
                ToolIdentityInvalid,
            ),
            (
                vec![],
                delta(json!({"tool_calls":[call(0,"a",&"n".repeat(65),"{}".into())]})),
                ToolIdentityInvalid,
            ),
            (
                vec![],
                delta(
                    json!({"tool_calls":[call(0,"a","lookup","x".repeat(MAX_ARGUMENT_BYTES + 1))]}),
                ),
                ToolArgumentsTooLarge,
            ),
            (vec![], finish("content_filter"), StreamFinishInvalid),
            (
                vec![],
                json!({"choices":[],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":3}}),
                StreamUsageInvalid,
            ),
            (
                vec![],
                json!({"choices":[],"usage":{"prompt_tokens":"private-marker"}}),
                StreamUsageInvalid,
            ),
            (vec![], final_usage.clone(), StreamUsageUnexpected),
            (
                vec![finish("stop"), final_usage.clone()],
                final_usage,
                StreamUsageUnexpected,
            ),
            (
                vec![finish("stop")],
                delta(json!({"content":"private-marker"})),
                StreamOutputAfterFinish,
            ),
        ];
        for (prefix, event, failure) in cases {
            let mut validator = SdkToolValidator::default();
            for event in prefix {
                validator.accept(event, |_| {}).unwrap();
            }
            let mut delivered = 0;
            let error = validator.accept(event, |_| delivered += 1).unwrap_err();
            assert_eq!(delivered, 0);
            assert_eq!(error.failure(), failure);
            assert!(!format!("{error:?} {error}").contains("private-marker"));
            assert!(validator.accept(finish("length"), |_| {}).is_err());
            assert!(validator.eof_completion().is_err());
        }
    }

    #[tokio::test]
    async fn streaming_request_has_full_allowance_usage_single_choice_and_fresh_cache_scope() {
        let messages = [Message {
            role: "user".into(),
            content: "private request".into(),
        }];
        let mut scopes = Vec::new();
        for _ in 0..2 {
            let body = request_body("fixture", 99, &messages, Some(heavy()))
                .unwrap()
                .collect()
                .await
                .unwrap()
                .to_bytes();
            let value: Value = serde_json::from_slice(&body).unwrap();
            let expected = tinfoil::relaxed::RelaxedChatRequestBuilder::new()
                .model("fixture")
                .messages([serde_json::json!({"role":"user","content":"private request"})])
                .set("max_tokens", 99)
                .set("stream", true)
                .set("stream_options", serde_json::json!({"include_usage":true}))
                .set("n", 1)
                .set("user_cache_secret", value["user_cache_secret"].clone())
                .build();
            assert_eq!(value, expected); // No extra envelope or defaults.
            assert_eq!(value["stream"], true);
            assert_eq!(value["stream_options"]["include_usage"], true);
            assert_eq!(value["n"], 1);
            assert_eq!(value["max_tokens"], 99);
            assert_eq!(value["messages"][0]["content"], "private request");
            scopes.push(value["user_cache_secret"].as_str().unwrap().to_owned());
        }
        assert_eq!(scopes[0].len(), 43);
        assert_ne!(scopes[0], scopes[1]);
    }

    #[test]
    fn oversized_streaming_request_is_rejected_before_transport() {
        let messages = [Message {
            role: "user".into(),
            content: "x".repeat(MAX_REQUEST_BODY_BYTES),
        }];
        assert!(matches!(
            request_body("fixture", 99, &messages, Some(heavy())),
            Err(InferenceError::Detailed(
                InferenceFailure::RequestEncodingFailed
            ))
        ));
    }

    #[tokio::test]
    async fn noncloneable_prompt_body_is_not_replayed_on_redirect_or_service_error() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        for status in [307, 308, 503] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let url = format!("http://{address}/");
            let location = url.clone();
            let calls = Arc::new(AtomicUsize::new(0));
            let observed = calls.clone();
            let server = tokio::spawn(async move {
                while let Ok(Ok((mut socket, _))) =
                    tokio::time::timeout(Duration::from_millis(100), listener.accept()).await
                {
                    observed.fetch_add(1, Ordering::SeqCst);
                    let mut request = Vec::new();
                    let mut buffer = [0; 1024];
                    loop {
                        let size = socket.read(&mut buffer).await.unwrap();
                        assert!(size > 0 && request.len() + size <= 4096);
                        request.extend_from_slice(&buffer[..size]);
                        if let Some(end) = request.windows(4).position(|b| b == b"\r\n\r\n") {
                            let headers = std::str::from_utf8(&request[..end]).unwrap();
                            let length: usize = headers
                                .lines()
                                .find_map(|line| line.strip_prefix("content-length: "))
                                .unwrap()
                                .parse()
                                .unwrap();
                            if request.len() >= end + 4 + length {
                                break;
                            }
                        }
                    }
                    socket.write_all(format!("HTTP/1.1 {status} Test\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
                }
            });
            let body = request_body("fixture", 99, &[], Some(heavy())).unwrap();
            let request = reqwest::Client::builder()
                .no_proxy()
                .build()
                .unwrap()
                .post(url)
                .body(body)
                .build()
                .unwrap();
            assert!(request.try_clone().is_none());
            let response = reqwest::Client::builder()
                .no_proxy()
                .build()
                .unwrap()
                .execute(request)
                .await
                .unwrap();
            assert_eq!(response.status().as_u16(), status);
            server.await.unwrap();
            assert_eq!(calls.load(Ordering::SeqCst), 1);
        }
    }

    #[tokio::test]
    async fn raw_adapter_emits_before_done_and_withholds_success_until_eof() {
        let (response, peer) = raw_response().await;
        let seen = Arc::new(tokio::sync::Notify::new());
        let notified = seen.clone();
        let mut task = tokio::spawn(async move {
            consume_response(
                response,
                Instant::now() + Duration::from_secs(2),
                Duration::from_secs(1),
                |delta| {
                    assert_eq!(delta, "é🐾");
                    notified.notify_one();
                },
            )
            .await
        });
        let progress = event(choice(Some("é🐾"), None));
        peer.send(&progress, 1).await;
        seen.notified().await;
        assert!(!task.is_finished());
        peer.send(&successful(""), 2).await;
        assert!(tokio::time::timeout(Duration::from_millis(25), &mut task)
            .await
            .is_err());
        peer.eof().await;
        assert_eq!(task.await.unwrap().unwrap().total_tokens, 5);
    }

    fn tool_input() -> ToolInvocation {
        ToolInvocation::new(
            serde_json::from_value(serde_json::json!([{"role":"user","content":"fixture"}])).unwrap(),
            Some(serde_json::from_value(serde_json::json!([{"type":"function","function":{"name":"lookup","parameters":{}}}])).unwrap()),
            None,
        ).unwrap()
    }

    #[tokio::test]
    async fn sdk_consumer_admits_headers_without_reading_error_body() {
        for (status, mime) in [(500, "text/event-stream"), (200, "application/json")] {
            let body = reqwest::Body::wrap_stream(futures_util::stream::poll_fn(
                |_| -> std::task::Poll<Option<Result<Vec<u8>, std::io::Error>>> {
                    panic!("error body was read");
                },
            ));
            let response = reqwest::Response::from(
                http::Response::builder()
                    .status(status)
                    .header("content-type", mime)
                    .body(body)
                    .unwrap(),
            );
            assert!(consume_invocation_response(
                response,
                Instant::now() + Duration::from_secs(1),
                STREAM_IDLE_TIMEOUT,
                ToolProfile::OpenAiFunctionsV1,
                &tool_input(),
                |_| panic!()
            )
            .await
            .is_err());
        }
    }

    #[tokio::test]
    async fn sdk_consumer_requires_finish_usage_and_sanitizes_payload_errors() {
        let finish = event(choice(None, Some("stop")));
        let final_usage = event(usage(2, 3));
        for (bytes, valid) in [
            ([finish.clone(), final_usage.clone()].concat(), true), // No DONE needed.
            (
                [
                    finish.clone(),
                    final_usage.clone(),
                    b"data: [DONE]\n\n".to_vec(),
                ]
                .concat(),
                true,
            ),
            (finish.clone(), false),
            (final_usage, false),
            (
                b"data: secret-sdk-error-marker not JSON\n\n".to_vec(),
                false,
            ),
            (
                [
                    finish,
                    event(serde_json::json!({"choices":[],"usage":{
                "prompt_tokens":2,"completion_tokens":3,"total_tokens":6}})),
                ]
                .concat(),
                false,
            ),
        ] {
            let response = reqwest::Response::from(
                http::Response::builder()
                    .header("content-type", "text/event-stream")
                    .body(bytes)
                    .unwrap(),
            );
            let result = consume_invocation_response(
                response,
                Instant::now() + Duration::from_secs(1),
                STREAM_IDLE_TIMEOUT,
                ToolProfile::OpenAiFunctionsV1,
                &tool_input(),
                |_| panic!(),
            )
            .await;
            assert_eq!(result.is_ok(), valid);
            assert!(!format!("{result:?}").contains("secret-sdk-error-marker"));
            if let Err(error) = result {
                assert!(!error.to_string().contains("secret-sdk-error-marker"));
            }
        }
    }

    #[tokio::test(start_paused = true)]
    async fn sdk_consumer_idle_tracks_http_progress_not_decoded_events() {
        for stall in [false, true] {
            let mut chunks = vec![b": progress\n\n".to_vec(); 4];
            // No event appears for much longer than the idle timeout, but each
            // HTTP read advances comments or a fragmented packet within it.
            chunks.extend(
                event(choice(None, Some("stop")))
                    .chunks(10)
                    .map(<[u8]>::to_vec),
            );
            chunks.push(event(usage(2, 3)));
            let bytes =
                futures_util::stream::unfold(chunks.into_iter(), move |mut chunks| async move {
                    let chunk = chunks.next()?;
                    tokio::time::sleep(Duration::from_millis(if stall { 50 } else { 30 })).await;
                    Some((Ok::<_, std::io::Error>(chunk), chunks))
                });
            let response = reqwest::Response::from(
                http::Response::builder()
                    .header("content-type", "text/event-stream")
                    .body(reqwest::Body::wrap_stream(bytes))
                    .unwrap(),
            );
            let result = consume_invocation_response(
                response,
                Instant::now() + Duration::from_secs(2),
                Duration::from_millis(40),
                ToolProfile::OpenAiFunctionsV1,
                &tool_input(),
                |_| panic!(),
            )
            .await;
            assert_eq!(result.is_ok(), !stall);
        }
        // Frequent HTTP progress cannot extend the total deadline.
        let bytes = futures_util::stream::unfold((), |_| async {
            tokio::time::sleep(Duration::from_millis(20)).await;
            Some((Ok::<_, std::io::Error>(b": progress\n\n".to_vec()), ()))
        });
        let response = reqwest::Response::from(
            http::Response::builder()
                .header("content-type", "text/event-stream")
                .body(reqwest::Body::wrap_stream(bytes))
                .unwrap(),
        );
        assert!(consume_invocation_response(
            response,
            Instant::now() + Duration::from_millis(100),
            Duration::from_millis(40),
            ToolProfile::OpenAiFunctionsV1,
            &tool_input(),
            |_| panic!()
        )
        .await
        .is_err());
    }

    #[tokio::test]
    async fn sdk_consumer_total_deadline_also_bounds_buffered_events() {
        let bytes = [
            event(choice(Some("first"), None)),
            event(choice(Some("second"), Some("stop"))),
            event(usage(2, 3)),
        ]
        .concat();
        for expired in [false, true] {
            let response = reqwest::Response::from(
                http::Response::builder()
                    .header("content-type", "text/event-stream")
                    .body(bytes.clone())
                    .unwrap(),
            );
            let deadline = if expired {
                Instant::now() - Duration::from_secs(1)
            } else {
                Instant::now() + Duration::from_millis(10)
            };
            let mut count = 0;
            let result = consume_invocation_response(
                response,
                deadline,
                STREAM_IDLE_TIMEOUT,
                ToolProfile::OpenAiFunctionsV1,
                &tool_input(),
                |_| {
                    count += 1;
                    std::thread::sleep(Duration::from_millis(20));
                },
            )
            .await;
            assert!(result.is_err());
            assert_eq!(count, usize::from(!expired));
        }
    }

    #[tokio::test]
    async fn sdk_consumer_waits_for_eof_and_rejects_post_done_transport_failure() {
        for (continuous, fault, omitted_finish_reason) in
            [false, true].into_iter().flat_map(|continuous| {
                ["none", "transport", "idle"]
                    .into_iter()
                    .flat_map(move |fault| {
                        [false, true]
                            .into_iter()
                            .map(move |omitted| (continuous, fault, omitted))
                    })
            })
        {
            let (response, peer) = raw_response().await;
            let invocation = tool_input();
            let seen = Arc::new(tokio::sync::Notify::new());
            let notified = seen.clone();
            let mut task = tokio::spawn(async move {
                consume_invocation_response(
                    response,
                    Instant::now() + Duration::from_secs(2),
                    Duration::from_millis(if fault == "idle" { 100 } else { 1000 }),
                    ToolProfile::OpenAiFunctionsV1,
                    &invocation,
                    |fragment| {
                        if let CompletionDelta::ToolCall { arguments, .. } = fragment {
                            assert_eq!(arguments, Some("{\"q\":\"é🐾\"}"));
                            notified.notify_one();
                        }
                    },
                )
                .await
            });
            let mut chunk = serde_json::json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"fixture","type":"function","function":{"name":"lookup","arguments":"{\"q\":\"é🐾\"}"}}]}}]});
            if !omitted_finish_reason {
                chunk["choices"][0]["finish_reason"] = Value::Null;
            }
            let mut terminal = choice(None, Some("stop"));
            if continuous {
                chunk["usage"] = usage(2, 1)["usage"].clone();
                terminal["usage"] = usage(2, 2)["usage"].clone();
            }
            peer.send(&event(chunk), 1).await;
            seen.notified().await;
            peer.send(&event(terminal), 2).await;
            peer.send(&event(usage(2, 3)), 2).await;
            peer.send(b"data: [DONE]\n\n", 2).await;
            assert!(tokio::time::timeout(Duration::from_millis(20), &mut task)
                .await
                .is_err());
            match fault {
                "transport" => peer.fault().await,
                "none" => peer.eof().await,
                _ => {} // Keep HTTP open after final usage and DONE until idle expiry.
            }
            let result = task.await.unwrap();
            match fault {
                "transport" => assert!(matches!(
                    result,
                    Err(InferenceError::Detailed(
                        InferenceFailure::StreamTransportFailed
                    ))
                )),
                "idle" => assert!(matches!(
                    result,
                    Err(InferenceError::Detailed(
                        InferenceFailure::StreamIdleTimeout
                    ))
                )),
                _ => {
                    let completion = result.unwrap();
                    assert_eq!(completion.finish_reason, FinishReason::Stop);
                    assert_eq!(
                        completion.usage,
                        StreamUsage {
                            input_tokens: 2,
                            output_tokens: 3,
                            total_tokens: 5
                        }
                    );
                }
            }
        }
    }

    #[tokio::test]
    async fn raw_adapter_refuses_faults_deadlines_and_invalid_final_usage_even_after_done() {
        for fault in [
            "transport",
            "total_deadline",
            "idle_deadline",
            "protocol",
            "usage",
            "missing_done",
        ] {
            let (response, peer) = raw_response().await;
            let deadline = if fault == "total_deadline" {
                Duration::from_millis(40)
            } else {
                Duration::from_secs(2)
            };
            let idle = if fault == "idle_deadline" {
                Duration::from_millis(40)
            } else {
                Duration::from_secs(1)
            };
            let task = tokio::spawn(async move {
                consume_response(response, Instant::now() + deadline, idle, |_| {}).await
            });
            if ["usage", "missing_done"].contains(&fault) {
                peer.send(&event(choice(None, Some("stop"))), 10).await;
                peer.send(&event(usage(2, 3)), 10).await;
                if fault == "usage" {
                    peer.send(b"data: {\"choices\":[],\"usage\":{}}\n\n", 10)
                        .await;
                }
                peer.eof().await;
            } else {
                peer.send(&successful("partial"), 16).await;
                match fault {
                    "transport" => peer.fault().await,
                    "protocol" => {
                        peer.send(b"data: {\"error\":\"secret\"}\n\n", 64).await;
                        peer.eof().await;
                    }
                    _ => {} // Explicitly hold open past the ordinary transport deadline.
                }
            }
            let result = task.await.unwrap();
            assert!(result.is_err());
            assert!(!format!("{result:?}").contains("secret"));
        }
    }

    #[test]
    fn authenticated_backend_metadata_is_bounded_but_never_rendered() {
        let mut parser = ProtocolParser::default();
        let mut deltas = Vec::new();
        let first = serde_json::json!({"choices":[{"index":0,"delta":{
            "content":"safe", "p":{"ignored":"<img src=remote>"}
        },"finish_reason":null,"token_ids":[1,2]}],"usage":{
            "prompt_tokens":2,"completion_tokens":1,"total_tokens":3,
            "completion_tokens_details":{"reasoning_tokens":1}
        }});
        parser
            .feed(&event(first), |delta| deltas.push(delta.to_owned()))
            .unwrap();
        parser
            .feed(
                &event(serde_json::json!({"choices":[{
                    "index":0,"delta":{},"finish_reason":"stop",
                    "token_ids":[],"stop_reason":null
                }]})),
                |_| panic!("metadata must not render"),
            )
            .unwrap();
        parser
            .feed(&event(usage(2, 1)), |_| panic!("usage must not render"))
            .unwrap();
        parser
            .feed(b"data: [DONE]\n\n", |_| panic!("DONE must not render"))
            .unwrap();
        assert_eq!(deltas, ["safe"]);
        assert_eq!(parser.eof().unwrap().total_tokens, 3);

        for (location, field) in [("delta", "unrecognized"), ("choice", "unknownFee")] {
            let mut choice = choice(Some("safe"), None);
            let target = if location == "delta" {
                &mut choice["choices"][0]["delta"]
            } else {
                &mut choice["choices"][0]
            };
            target[field] = serde_json::json!(0);
            assert_eq!(
                ProtocolParser::default().feed(&event(choice), |_| panic!()),
                Err(StreamError::Protocol)
            );
        }
        let oversized = serde_json::json!({"choices":[{"index":0,"delta":{"content":"safe","p":"x".repeat(MAX_OPTIONAL_BYTES)},"finish_reason":null}]});
        assert_eq!(
            ProtocolParser::default().feed(&event(oversized), |_| panic!()),
            Err(StreamError::Limit)
        );
    }

    #[tokio::test]
    async fn response_headers_fail_without_error_body_collection_and_chunks_are_fragment_independent(
    ) {
        use futures_util::stream;
        for (status, mime) in [(500, "text/event-stream"), (200, "application/json")] {
            let body = reqwest::Body::wrap_stream(stream::poll_fn(
                |_| -> std::task::Poll<Option<Result<Vec<u8>, std::io::Error>>> {
                    panic!("error body was read");
                },
            ));
            let response = reqwest::Response::from(
                http::Response::builder()
                    .status(status)
                    .header("content-type", mime)
                    .body(body)
                    .unwrap(),
            );
            assert!(consume_response(
                response,
                Instant::now() + Duration::from_secs(1),
                STREAM_IDLE_TIMEOUT,
                |_| panic!()
            )
            .await
            .is_err());
        }
        for size in [
            MAX_TRANSPORT_BUFFER_BYTES,
            MAX_TRANSPORT_BUFFER_BYTES + 1,
            512 * 1024,
        ] {
            let mut bytes = successful("");
            // Blank lines before DONE do not impose a cumulative answer limit.
            bytes.splice(0..0, vec![b'\n'; size - bytes.len()]);
            let body = reqwest::Body::wrap_stream(stream::iter([Ok::<_, std::io::Error>(bytes)]));
            let response = reqwest::Response::from(
                http::Response::builder()
                    .header("content-type", "text/event-stream; charset=utf-8")
                    .body(body)
                    .unwrap(),
            );
            let result = consume_response(
                response,
                Instant::now() + Duration::from_secs(1),
                STREAM_IDLE_TIMEOUT,
                |_| {},
            )
            .await;
            assert!(
                result.is_ok(),
                "a valid stream must not depend on DATA fragmentation"
            );
        }
        // Splitting DATA must not reset the protocol's cross-slice line limit.
        let oversized = vec![b'x'; MAX_TRANSPORT_BUFFER_BYTES + 1];
        let oversized_line =
            reqwest::Body::wrap_stream(stream::iter([Ok::<_, std::io::Error>(oversized)]));
        let response = reqwest::Response::from(
            http::Response::builder()
                .header("content-type", "text/event-stream")
                .body(oversized_line)
                .unwrap(),
        );
        assert!(consume_response(
            response,
            Instant::now() + Duration::from_secs(1),
            STREAM_IDLE_TIMEOUT,
            |_| panic!("oversized line must not emit content")
        )
        .await
        .is_err());
        // An already expired total deadline rejects even an immediately ready EOF.
        let response = reqwest::Response::from(
            http::Response::builder()
                .header("content-type", "text/event-stream")
                .body(reqwest::Body::from(successful("")))
                .unwrap(),
        );
        assert!(consume_response(
            response,
            Instant::now() - Duration::from_secs(1),
            STREAM_IDLE_TIMEOUT,
            |_| panic!()
        )
        .await
        .is_err());
    }

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

    #[test]
    fn raw_value_private_keys_cannot_expand_past_guarded_node_limit() {
        let nested = format!("[{}]", vec![r#"{"":0}"#; 7_000].join(","));
        let encoded = serde_json::to_string(&nested).unwrap();
        let mut wire = String::from("data: {\"choices\":[],\n");
        for (index, name) in ["x", "y", "z"].into_iter().enumerate() {
            let separator = if index == 2 { "" } else { "," };
            wire.push_str(&format!(
                "data: \"{name}\":{{\"$serde_json::private::RawValue\":{encoded}}}{separator}\n"
            ));
        }
        wire.push_str("data: }\n\n");
        assert!(wire.lines().all(|line| line.len() <= MAX_LINE_BYTES));
        let payload = wire
            .lines()
            .filter_map(|line| line.strip_prefix("data: "))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(payload.len() <= MAX_FRAME_BYTES);
        assert!(validate_json(payload.as_bytes()).is_err());
        let mut parser = ProtocolParser::default();
        assert_eq!(
            parser.feed(wire.as_bytes(), |_| panic!("must not emit")),
            Err(StreamError::Protocol)
        );

        // Guard decoded keys, not only their literal wire spelling.
        assert!(validate_json(br#"{"$serde_json::private::Ra\u0077Value":"[]"}"#).is_err());
        assert!(validate_json(br#"{"optional":{"RawValue":"[]"}}"#).is_ok());
    }
}
