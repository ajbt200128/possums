use crate::catalog::{Catalog, Model, MAX_CATALOG_BYTES};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use thiserror::Error;
use tinfoil::Client;
use tokio::time::Instant;

mod errors;
pub use errors::InferenceFailure;
pub mod stream;
pub mod tools;
use tools::{CompletionDelta, InvocationRequest, ToolInvocation, ToolProfile};
#[cfg(test)]
#[path = "../tests/support/stream.rs"]
pub(crate) mod stream_support;
mod upload;
#[cfg(test)]
mod upload_tests;

// Bounds apply to BORROWED SDK state, before any request-owned clone. The
// schema has only fixed objects, two register vectors and these bounded strings.
const MAX_EVIDENCE_FIELD_BYTES: usize = 4 * 1024;
const MAX_EVIDENCE_STRING_BYTES: usize = 16 * 1024;
const MAX_EVIDENCE_REGISTERS: usize = 32;
const MAX_VERIFICATION_DOCUMENT_BYTES: usize = 128 * 1024;
const MAX_TOKENIZER_REQUEST_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Error)]
pub enum InferenceError {
    #[error("verified inference unavailable")]
    Unavailable,
    #[error("authenticated upstream response is invalid")]
    InvalidResponse,
    #[error(transparent)]
    Detailed(#[from] InferenceFailure),
}

impl InferenceError {
    pub fn failure(&self) -> InferenceFailure {
        match self {
            Self::Unavailable => InferenceFailure::InferenceUnavailable,
            Self::InvalidResponse => InferenceFailure::UpstreamResponseInvalid,
            Self::Detailed(failure) => *failure,
        }
    }
}

#[async_trait]
pub trait Inference: Send + Sync {
    async fn catalog(&self) -> Result<Vec<u8>, InferenceError>;
    /// Success also certifies the serialized tokenizer upload has been released.
    /// Pass the SAME admitted heavy lease through both prompt-bearing calls.
    /// Implementations must attach it to serialized DATA, including transport
    /// clones/slices, not only to the request future or outer body.
    async fn count_tokens(
        &self,
        model: &str,
        messages: &[Message],
        heavy: Arc<crate::telemetry::hooks::Lease>,
    ) -> Result<u64, InferenceError>;
    /// Stream only after trust/catalog/reservation and context preflight.
    /// `model` carries the full context-legal output allowance. Successful usage
    /// is terminal, withheld until validated finish, [DONE], and transport EOF.
    /// Serialized upload DATA must retain `heavy` even beyond this call's return.
    /// Call `heavy.dispatch_generation()` just before the actual generation send
    /// (after encoding/trust), never for tokenizer or role/usage-only events.
    ///
    /// The borrowed synchronous callback must not block, panic, or collect
    /// unbounded output. On delivery failure, detach delivery in the callback and
    /// keep polling this future to completion; never cancel upstream or retry an
    /// uncertain generation. Callback return values cannot cancel consumption.
    /// Implementations must not expose prompt text or raw upstream diagnostics.
    /// The default fails closed for implementations without streaming support.
    async fn generate_stream(
        &self,
        _model: &Model,
        _messages: &[Message],
        _heavy: Arc<crate::telemetry::hooks::Lease>,
        _on_delta: &mut (dyn for<'delta> FnMut(&'delta str) + Send),
    ) -> Result<stream::StreamUsage, InferenceError> {
        Err(InferenceError::Unavailable)
    }

    /// API adapter preserves authenticated finish metadata as well as final usage.
    /// Implementations without a qualified completion protocol fail closed.
    async fn generate_completion_stream(
        &self,
        _model: &Model,
        _messages: &[Message],
        _heavy: Arc<crate::telemetry::hooks::Lease>,
        _on_delta: &mut (dyn for<'delta> FnMut(&'delta str) + Send),
    ) -> Result<stream::StreamCompletion, InferenceError> {
        Err(InferenceError::Unavailable)
    }

    /// Mock implementations opt in explicitly; production assumes a shared wire
    /// profile. Catalog validation and caller-side execution remain separate.
    fn tool_profile(&self, _model: &str) -> Option<ToolProfile> {
        None
    }

    async fn count_invocation_tokens(
        &self,
        _model: &str,
        _invocation: &ToolInvocation,
        _heavy: Arc<crate::telemetry::hooks::Lease>,
    ) -> Result<u64, InferenceError> {
        Err(InferenceError::Unavailable)
    }

    /// The callback borrows bounded fragments; never collect complete arguments.
    /// Delivery failure must detach only, not cancel this settling operation.
    async fn generate_invocation_stream(
        &self,
        _model: &Model,
        _invocation: &ToolInvocation,
        _heavy: Arc<crate::telemetry::hooks::Lease>,
        _on_delta: &mut (dyn for<'delta> FnMut(CompletionDelta<'delta>) + Send),
    ) -> Result<stream::StreamCompletion, InferenceError> {
        Err(InferenceError::Unavailable)
    }

    fn verification_document(&self) -> Result<serde_json::Value, InferenceError>;
}

pub type SharedInference = Arc<dyn Inference>;

pub struct TinfoilInference {
    client: Client,
    origin: String,
}

impl TinfoilInference {
    pub async fn connect(
        host: &str,
        repository: &str,
        api_key: String,
    ) -> Result<Self, InferenceError> {
        let client = tokio::time::timeout(
            Duration::from_secs(30),
            Client::new(host, repository, api_key),
        )
        .await
        .map_err(|_| InferenceError::Detailed(InferenceFailure::VerificationFailed))?
        .map_err(|_| InferenceError::Detailed(InferenceFailure::VerificationFailed))?;
        let inference = Self {
            origin: format!("https://{host}"),
            client,
        };
        inference.verification_document()?;
        Ok(inference)
    }

    /// Verified streaming adapter.
    ///
    /// Call only after trust/catalog/reservation and context preflight. `model`
    /// carries the full context-legal output allowance, not a credit-reduced cap.
    /// Deltas are borrowed, bounded, and validated before this synchronous callback;
    /// it must not block or collect unbounded output. The one returned Result is
    /// terminal: successful usage is withheld until finish, DONE and transport EOF.
    /// Delivery failure is not a reason to stop calling/consuming this future.
    pub async fn generate_stream(
        &self,
        model: &Model,
        messages: &[Message],
        heavy: Arc<crate::telemetry::hooks::Lease>,
        on_delta: impl FnMut(&str) + Send,
    ) -> Result<stream::StreamUsage, InferenceError> {
        self.generate_completion_stream(model, messages, heavy, on_delta)
            .await
            .map(|completion| completion.usage)
    }

    async fn generate_completion_stream(
        &self,
        model: &Model,
        messages: &[Message],
        heavy: Arc<crate::telemetry::hooks::Lease>,
        on_delta: impl FnMut(&str) + Send,
    ) -> Result<stream::StreamCompletion, InferenceError> {
        let http = self.http()?;
        let url = format!("{}/v1/chat/completions", self.origin);
        let deadline = Instant::now() + stream::STREAM_DEADLINE;
        let request = self
            .authenticate(http.post(&url))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .body(stream::request_body(
                &model.id,
                model.max_output_tokens,
                messages,
                Some(heavy.clone()),
            )?);
        // Use the origin-bound raw transport, not the SDK's retrying chat layer.
        heavy.dispatch_generation();
        let response = tokio::time::timeout_at(deadline, request.send())
            .await
            .map_err(|_| InferenceError::Detailed(InferenceFailure::GenerationSendFailed))?
            .map_err(|_| InferenceError::Detailed(InferenceFailure::GenerationSendFailed))?;
        if response.url().as_str() != url {
            return Err(InferenceError::Detailed(
                InferenceFailure::EndpointBindingFailed,
            ));
        }
        stream::consume_completion_response(
            response,
            deadline,
            stream::STREAM_IDLE_TIMEOUT,
            on_delta,
        )
        .await
    }

    /// Funded diagnostic only: fixed non-user input, no gateway accounting or route.
    /// Observations are NOT a validated completion or a billing contract.
    #[doc(hidden)]
    pub async fn probe_streaming_contract(
        &self,
        model: &str,
    ) -> Result<StreamProbe, InferenceError> {
        tokio::time::timeout(Duration::from_secs(120), async {
            let messages = [Message {
                role: "user".into(),
                content: "Reply with exactly: possums-live-canary".into(),
            }];
            let input_tokens = self.count_tokens_owned(model, &messages, None).await?;
            let request = self
                .authenticate(
                    self.http()?
                        .post(format!("{}/v1/chat/completions", self.origin)),
                )
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .header(reqwest::header::ACCEPT, "text/event-stream")
                .body(stream::request_body(model, 64, &messages, None)?);
            let deadline = Instant::now() + Duration::from_secs(90);
            let mut probe = StreamProbe {
                tokenizer_tokens: input_tokens,
                ..Default::default()
            };
            match tokio::time::timeout_at(deadline, request.send()).await {
                Ok(Ok(response)) => probe.consume(response, deadline).await,
                _ => probe.failure = Some(ProbeFailure::TransportOrDeadline),
            }
            Ok(probe)
        })
        .await
        .map_err(|_| InferenceError::Unavailable)?
    }

    async fn count_tokens_owned(
        &self,
        model: &str,
        messages: &[Message],
        heavy: Option<Arc<crate::telemetry::hooks::Lease>>,
    ) -> Result<u64, InferenceError> {
        let (body, released) = tokenizer_request_body(model, messages, heavy)?;
        let deadline = Instant::now() + Duration::from_secs(300);
        let url = format!("{}/v1/chat/completions/input_tokens", self.origin);
        let request = self
            .authenticate(self.http()?.post(&url))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body);
        let response = tokio::time::timeout_at(deadline, request.send())
            .await
            .map_err(|_| InferenceError::Detailed(InferenceFailure::TokenizerSendFailed))?
            .map_err(|_| InferenceError::Detailed(InferenceFailure::TokenizerSendFailed))?;
        if response.url().as_str() != url {
            return Err(InferenceFailure::EndpointBindingFailed.into());
        }
        finish_tokenizer(response, released, deadline).await
    }

    async fn bounded_response(
        &self,
        request: tinfoil::verifier::tls::OriginBoundRequestBuilder,
        limit: usize,
    ) -> Result<Vec<u8>, InferenceError> {
        let deadline = Instant::now() + Duration::from_secs(300);
        let response = tokio::time::timeout_at(deadline, request.send())
            .await
            .map_err(|_| InferenceError::Detailed(InferenceFailure::CatalogFailed))?
            .map_err(|_| InferenceError::Detailed(InferenceFailure::CatalogFailed))?;
        collect_bounded_response(response, limit, deadline).await
    }

    fn http(&self) -> Result<&tinfoil::verifier::tls::OriginBoundClient, InferenceError> {
        self.client
            .http_client()
            .map_err(|_| InferenceError::Detailed(InferenceFailure::VerificationFailed))
    }

    fn authenticate(
        &self,
        request: tinfoil::verifier::tls::OriginBoundRequestBuilder,
    ) -> tinfoil::verifier::tls::OriginBoundRequestBuilder {
        request.bearer_auth(self.client.secure_client().api_key())
    }
}

#[doc(hidden)]
pub async fn collect_bounded_response(
    mut response: reqwest::Response,
    limit: usize,
    deadline: Instant,
) -> Result<Vec<u8>, InferenceError> {
    if !response.status().is_success() {
        return Err(InferenceError::Unavailable);
    }
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(InferenceError::InvalidResponse);
    }
    let mut bytes = Vec::with_capacity(
        response
            .content_length()
            .unwrap_or_default()
            .min(limit as u64) as usize,
    );
    loop {
        let chunk = tokio::time::timeout_at(deadline, response.chunk())
            .await
            .map_err(|_| InferenceError::Unavailable)?
            .map_err(|_| InferenceError::Unavailable)?;
        let Some(chunk) = chunk else {
            break;
        };
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err(InferenceError::InvalidResponse);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

// Deliberately separate from the production adapter: strict diagnostic framing,
// not a promise that these are the provider's only valid stream shapes. No text,
// identifiers, raw field names, headers or upstream errors enter the report.
const PROBE_EVENT_BYTES: usize = 16 * 1024;
const PROBE_TOTAL_BYTES: usize = 1024 * 1024;
const PROBE_MAX_EVENTS: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeFailure {
    TransportOrDeadline,
    HttpStatus(u16),
    FramingOrLimit,
    InvalidJsonOrShape,
    ProviderError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeFinish {
    Stop,
    Length,
    Other,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct ProbeUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub completion_tokens_details: Option<ProbeCompletionDetails>,
    pub prompt_tokens_details: Option<ProbePromptDetails>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct ProbeCompletionDetails {
    pub reasoning_tokens: Option<u64>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct ProbePromptDetails {
    pub cached_tokens: Option<u64>,
}

#[derive(Default)]
pub struct StreamProbe {
    pub tokenizer_tokens: u64,
    pub sse_content_type: bool,
    pub events: usize,
    pub content_events: usize,
    pub reasoning_events: usize,
    pub finish_events: usize,
    pub finish: Option<ProbeFinish>,
    pub finish_event: Option<usize>,
    pub usage_events: usize,
    pub usage: Option<ProbeUsage>,
    pub usage_event: Option<usize>,
    pub unknown_usage_fields: bool,
    pub done_event: Option<usize>,
    pub eof: bool,
    pub crlf: bool,
    pub failure: Option<ProbeFailure>,
    line: Vec<u8>,
    payload: Vec<u8>,
    event_bytes: usize,
    total_bytes: usize,
}

// Never derive Debug over parser buffers: even a rejected event may contain secrets.
impl std::fmt::Debug for StreamProbe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamProbe")
            .field("tokenizer_tokens", &self.tokenizer_tokens)
            .field("sse_content_type", &self.sse_content_type)
            .field("events", &self.events)
            .field("content_events", &self.content_events)
            .field("reasoning_events", &self.reasoning_events)
            .field("finish_events", &self.finish_events)
            .field("finish", &self.finish)
            .field("finish_event", &self.finish_event)
            .field("usage_events", &self.usage_events)
            .field("usage", &self.usage)
            .field("usage_event", &self.usage_event)
            .field("unknown_usage_fields", &self.unknown_usage_fields)
            .field("done_event", &self.done_event)
            .field("eof", &self.eof)
            .field("crlf", &self.crlf)
            .field("failure", &self.failure)
            .finish()
    }
}

impl StreamProbe {
    async fn consume(&mut self, mut response: reqwest::Response, deadline: Instant) {
        if !response.status().is_success() {
            self.failure = Some(ProbeFailure::HttpStatus(response.status().as_u16()));
            return; // Never collect provider error bodies.
        }
        self.sse_content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.split(';').next() == Some("text/event-stream"));
        loop {
            match tokio::time::timeout_at(deadline, response.chunk()).await {
                Ok(Ok(Some(chunk))) => {
                    if let Err(error) = self.feed(&chunk) {
                        self.failure = Some(error);
                        break;
                    }
                }
                Ok(Ok(None)) => {
                    self.eof = true;
                    if !self.line.is_empty() || !self.payload.is_empty() {
                        self.failure = Some(ProbeFailure::FramingOrLimit);
                    }
                    break;
                }
                _ => {
                    self.failure = Some(ProbeFailure::TransportOrDeadline);
                    break;
                }
            }
        }
        self.line.clear();
        self.payload.clear();
    }

    fn feed(&mut self, bytes: &[u8]) -> Result<(), ProbeFailure> {
        self.total_bytes = self.total_bytes.saturating_add(bytes.len());
        if self.total_bytes > PROBE_TOTAL_BYTES {
            return Err(ProbeFailure::FramingOrLimit);
        }
        for &byte in bytes {
            self.event_bytes += 1;
            if self.event_bytes > PROBE_EVENT_BYTES {
                return Err(ProbeFailure::FramingOrLimit);
            }
            if byte != b'\n' {
                self.line.push(byte);
                continue;
            }
            if self.line.last() == Some(&b'\r') {
                self.crlf = true;
                self.line.pop();
            }
            if self.line.is_empty() {
                if !self.payload.is_empty() {
                    self.event()?;
                    self.payload.clear();
                }
                self.event_bytes = 0;
            } else if let Some(data) = self.line.strip_prefix(b"data:") {
                if !self.payload.is_empty() {
                    self.payload.push(b'\n');
                }
                self.payload
                    .extend_from_slice(data.strip_prefix(b" ").unwrap_or(data));
            } else if !self.line.starts_with(b":") {
                return Err(ProbeFailure::FramingOrLimit);
            }
            self.line.clear();
        }
        Ok(())
    }

    fn event(&mut self) -> Result<(), ProbeFailure> {
        self.events += 1;
        if self.events > PROBE_MAX_EVENTS || self.done_event.is_some() {
            return Err(ProbeFailure::FramingOrLimit);
        }
        if self.payload == b"[DONE]" {
            self.done_event = Some(self.events);
            return Ok(());
        }
        let value: serde_json::Value =
            serde_json::from_slice(&self.payload).map_err(|_| ProbeFailure::InvalidJsonOrShape)?;
        if value.get("error").is_some() {
            return Err(ProbeFailure::ProviderError);
        }
        let choices = value
            .get("choices")
            .and_then(|value| value.as_array())
            .ok_or(ProbeFailure::InvalidJsonOrShape)?;
        if choices.len() > 1 {
            return Err(ProbeFailure::InvalidJsonOrShape);
        }
        for choice in choices {
            if choice
                .pointer("/delta/content")
                .and_then(|v| v.as_str())
                .is_some_and(|s| !s.is_empty())
            {
                self.content_events += 1;
            }
            if ["/delta/reasoning", "/delta/reasoning_content"]
                .iter()
                .any(|path| {
                    choice
                        .pointer(path)
                        .and_then(|v| v.as_str())
                        .is_some_and(|s| !s.is_empty())
                })
            {
                self.reasoning_events += 1;
            }
            if let Some(reason) = choice.get("finish_reason").filter(|v| !v.is_null()) {
                self.finish_events += 1;
                self.finish_event = Some(self.events);
                self.finish = Some(match reason.as_str() {
                    Some("stop") => ProbeFinish::Stop,
                    Some("length") => ProbeFinish::Length,
                    _ => ProbeFinish::Other,
                });
            }
        }
        if let Some(usage) = value.get("usage").filter(|v| !v.is_null()) {
            self.usage_events += 1;
            self.usage_event = Some(self.events);
            self.unknown_usage_fields |= has_unknown_fields(
                usage,
                &[
                    "prompt_tokens",
                    "completion_tokens",
                    "total_tokens",
                    "completion_tokens_details",
                    "prompt_tokens_details",
                ],
            ) || usage
                .get("completion_tokens_details")
                .filter(|v| !v.is_null())
                .is_some_and(|v| has_unknown_fields(v, &["reasoning_tokens"]))
                || usage
                    .get("prompt_tokens_details")
                    .filter(|v| !v.is_null())
                    .is_some_and(|v| has_unknown_fields(v, &["cached_tokens"]));
            self.usage = Some(
                serde_json::from_value(usage.clone())
                    .map_err(|_| ProbeFailure::InvalidJsonOrShape)?,
            );
        }
        Ok(())
    }
}

fn has_unknown_fields(value: &serde_json::Value, allowed: &[&str]) -> bool {
    value
        .as_object()
        .is_none_or(|object| object.keys().any(|key| !allowed.contains(&key.as_str())))
}

#[cfg(test)]
mod stream_probe_tests {
    use super::*;

    async fn peer(raw: &'static [u8], hold_open: bool) -> reqwest::Response {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let _ = rustls::crypto::ring::default_provider().install_default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 1024];
            let _ = socket.read(&mut request).await.unwrap();
            socket.write_all(raw).await.unwrap();
            if hold_open {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });
        reqwest::get(format!("http://{address}/")).await.unwrap()
    }

    #[tokio::test]
    async fn observes_eof_without_treating_it_as_billing_success_and_bounds_stalls() {
        let response = peer(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: [DONE]\n\n", false).await;
        let mut probe = StreamProbe::default();
        probe
            .consume(response, Instant::now() + Duration::from_secs(1))
            .await;
        assert!(probe.eof && probe.sse_content_type);
        assert_eq!(probe.done_event, Some(1));
        assert!(probe.usage.is_none()); // EOF + DONE alone proves no billing semantics.

        let response = peer(
            b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\ndata: [DONE]\n\n",
            true,
        )
        .await;
        let mut probe = StreamProbe::default();
        probe
            .consume(response, Instant::now() + Duration::from_millis(20))
            .await;
        assert_eq!(probe.failure, Some(ProbeFailure::TransportOrDeadline));
        assert!(!probe.eof);
    }

    #[tokio::test]
    async fn rejects_http_errors_without_reading_bodies_and_marks_partial_frames() {
        let response = peer(
            b"HTTP/1.1 402 Payment Required\r\nContent-Length: 999999\r\n\r\nsecret",
            true,
        )
        .await;
        let mut probe = StreamProbe::default();
        probe
            .consume(response, Instant::now() + Duration::from_secs(1))
            .await;
        assert_eq!(probe.failure, Some(ProbeFailure::HttpStatus(402)));
        assert_eq!(probe.total_bytes, 0);
        assert!(!format!("{probe:?}").contains("secret"));

        let response = peer(
            b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\ndata: {",
            false,
        )
        .await;
        let mut probe = StreamProbe::default();
        probe
            .consume(response, Instant::now() + Duration::from_secs(1))
            .await;
        assert!(probe.eof);
        assert_eq!(probe.failure, Some(ProbeFailure::FramingOrLimit));
        assert!(probe.line.is_empty() && probe.payload.is_empty());
    }

    #[test]
    fn split_utf8_crlf_and_terminal_observations_without_retaining_text() {
        let bytes = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"é\"},\"finish_reason\":null}]}\r\n\r\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":8,\"completion_tokens\":64,\"total_tokens\":72}}\n\n",
            "data: [DONE]\n\n"
        ).as_bytes();
        for split in 0..=bytes.len() {
            let mut probe = StreamProbe::default();
            probe.feed(&bytes[..split]).unwrap();
            probe.feed(&bytes[split..]).unwrap();
            assert_eq!(probe.content_events, 1);
            assert_eq!(probe.finish, Some(ProbeFinish::Length));
            assert_eq!(probe.usage_event, Some(3));
            assert_eq!(probe.done_event, Some(4));
            assert!(probe.crlf && probe.line.is_empty() && probe.payload.is_empty());
        }
    }

    #[test]
    fn diagnostic_retains_last_decreasing_usage_without_tokenizer_or_reasoning_equality() {
        let mut probe = StreamProbe {
            tokenizer_tokens: 99,
            ..Default::default()
        };
        probe.feed(concat!(
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":8,\"completion_tokens\":9,\"total_tokens\":17}}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":0,\"total_tokens\":1,\"completion_tokens_details\":{\"reasoning_tokens\":30}}}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":null}\n\n",
            "data: [DONE]\n\n"
        ).as_bytes()).unwrap();
        assert_eq!(probe.usage_events, 2);
        let usage = probe.usage.unwrap();
        assert_eq!(
            (
                usage.prompt_tokens,
                usage.completion_tokens,
                usage.total_tokens
            ),
            (1, 0, 1)
        );
        assert!(probe.usage_event < probe.finish_event);
    }

    #[test]
    fn bounds_and_sanitizes_hostile_events_and_errors_after_partial_output() {
        let mut probe = StreamProbe::default();
        assert_eq!(
            probe.feed(&vec![b'x'; PROBE_EVENT_BYTES + 1]),
            Err(ProbeFailure::FramingOrLimit)
        );
        assert!(probe.line.len() <= PROBE_EVENT_BYTES);
        assert_eq!(
            StreamProbe::default().feed(&vec![b'x'; PROBE_TOTAL_BYTES + 1]),
            Err(ProbeFailure::FramingOrLimit)
        );
        assert_eq!(
            StreamProbe::default().feed(b"data: not-json\n\n"),
            Err(ProbeFailure::InvalidJsonOrShape)
        );
        assert_eq!(
            StreamProbe::default().feed(b"data: \xff\n\n"),
            Err(ProbeFailure::InvalidJsonOrShape)
        );
        let mut probe = StreamProbe::default();
        probe
            .feed(b"data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n")
            .unwrap();
        assert_eq!(
            probe.feed(b"data: {\"error\":{\"message\":\"secret\"}}\n\n"),
            Err(ProbeFailure::ProviderError)
        );
        assert_eq!(probe.content_events, 1);
        assert_eq!(probe.usage_events, 0);
        assert!(!format!("{probe:?}").contains("secret"));
        let mut probe = StreamProbe::default();
        probe.feed(b"data: [DONE]\n\n").unwrap();
        assert_eq!(
            probe.feed(b"data: [DONE]\n\n"),
            Err(ProbeFailure::FramingOrLimit)
        );
    }

    #[tokio::test]
    async fn tokenizer_failures_preserve_only_stage_and_release_constraint() {
        use InferenceFailure::*;
        for (status, payload, hold_upload, expected) in [
            (503, "private-provider-marker", false, TokenizerHttpFailed),
            (
                200,
                "private-provider-marker",
                false,
                TokenizerResponseInvalid,
            ),
            (
                200,
                "{\"input_tokens\":\"private-provider-marker\"}",
                false,
                TokenizerResponseInvalid,
            ),
            (200, "{\"input_tokens\":0}", false, TokenizerResponseInvalid),
            (200, "{\"input_tokens\":2}", true, TokenizerUploadIncomplete),
        ] {
            let (body, released) = tokenizer_request_body("fixture", &[], None).unwrap();
            let mut body = Some(body);
            if !hold_upload {
                drop(body.take());
            }
            let response = http::Response::builder()
                .status(status)
                .body(payload)
                .unwrap()
                .into();
            let error = finish_tokenizer(
                response,
                released,
                Instant::now() + Duration::from_millis(20),
            )
            .await
            .unwrap_err();
            assert_eq!(error.failure(), expected);
            assert!(!format!("{error:?} {error}").contains("private-provider-marker"));
            drop(body);
        }
    }

    #[test]
    fn tokenizer_request_is_bounded_before_transport() {
        let messages = [Message {
            role: "user".into(),
            content: "x".repeat(MAX_TOKENIZER_REQUEST_BYTES),
        }];
        assert!(tokenizer_request_bytes("fixture", &messages).is_err());
    }
}

fn text_request(
    model: &str,
    messages: &[Message],
) -> Result<tinfoil::relaxed::RelaxedChatRequestBuilder, InferenceError> {
    let messages = messages
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| InferenceError::Detailed(InferenceFailure::RequestEncodingFailed))?;
    Ok(tinfoil::relaxed::RelaxedChatRequestBuilder::new()
        .model(model)
        .messages(messages))
}

fn invocation_request(
    model: &str,
    invocation: &ToolInvocation,
) -> Result<tinfoil::relaxed::RelaxedChatRequestBuilder, InferenceError> {
    // This serializer maps admitted gateway input, not a provider wire request.
    let mut input = serde_json::to_value(InvocationRequest { model, invocation })
        .map_err(|_| InferenceError::Detailed(InferenceFailure::RequestEncodingFailed))?;
    let messages = input["messages"]
        .take()
        .as_array_mut()
        .map(std::mem::take)
        .ok_or(InferenceError::Detailed(
            InferenceFailure::RequestEncodingFailed,
        ))?;
    let mut builder = tinfoil::relaxed::RelaxedChatRequestBuilder::new()
        .model(model)
        .messages(messages);
    for field in ["tools", "tool_choice"] {
        if let Some(value) = input.get_mut(field) {
            builder = builder.set(field, value.take());
        }
    }
    Ok(builder)
}

fn tokenizer_request_bytes(model: &str, messages: &[Message]) -> Result<Vec<u8>, InferenceError> {
    crate::bounded_json::to_vec(
        &text_request(model, messages)?.build(),
        MAX_TOKENIZER_REQUEST_BYTES,
    )
    .map_err(|_| InferenceError::Detailed(InferenceFailure::RequestEncodingFailed))
}

// The private diagnostic alone may omit admission. Route APIs require a lease.
fn tokenizer_request_body(
    model: &str,
    messages: &[Message],
    heavy: Option<Arc<crate::telemetry::hooks::Lease>>,
) -> Result<(reqwest::Body, upload::UploadReleased), InferenceError> {
    Ok(upload::body(
        tokenizer_request_bytes(model, messages)?,
        heavy,
    ))
}

fn invocation_tokenizer_body(
    model: &str,
    invocation: &ToolInvocation,
    heavy: Arc<crate::telemetry::hooks::Lease>,
) -> Result<(reqwest::Body, upload::UploadReleased), InferenceError> {
    let bytes = crate::bounded_json::to_vec(
        &invocation_request(model, invocation)?.build(),
        MAX_TOKENIZER_REQUEST_BYTES,
    )
    .map_err(|_| InferenceError::Detailed(InferenceFailure::RequestEncodingFailed))?;
    Ok(upload::body(bytes, Some(heavy)))
}

async fn finish_tokenizer(
    response: reqwest::Response,
    released: upload::UploadReleased,
    deadline: Instant,
) -> Result<u64, InferenceError> {
    if !response.status().is_success() {
        return Err(InferenceFailure::TokenizerHttpFailed.into());
    }
    let bytes = collect_bounded_response(response, 64 * 1024, deadline)
        .await
        .map_err(|_| InferenceFailure::TokenizerResponseInvalid)?;
    let count: TokenCount = serde_json::from_slice(&bytes)
        .map_err(|_| InferenceError::Detailed(InferenceFailure::TokenizerResponseInvalid))?;
    if count.input_tokens == 0 {
        return Err(InferenceError::Detailed(
            InferenceFailure::TokenizerResponseInvalid,
        ));
    }
    // An early 200/EOF is NOT an upload-release receipt. Avoid overlap between
    // the tokenizer allocation and generation serializer's growth peak. The
    // route's existing 30-second preflight timeout also encloses this wait.
    tokio::time::timeout_at(deadline, released.wait())
        .await
        .map_err(|_| InferenceError::Detailed(InferenceFailure::TokenizerUploadIncomplete))?;
    Ok(count.input_tokens)
}

// Synthetic transport access only: this neither bypasses nor proves production
// provider authentication, endpoint binding or release provenance.
#[cfg(test)]
pub(crate) mod resource_fixtures {
    use super::*;

    // Same pre-clone export as the production adapter; synthetic data here is
    // NOT evidence of live provider signatures or verification acceptance.
    pub(crate) fn verification_value(
        ground_truth: &tinfoil::GroundTruth,
        host: &str,
    ) -> Result<serde_json::Value, InferenceError> {
        super::verification_value(ground_truth, host)
    }

    pub(crate) fn tokenizer_body(
        model: &str,
        messages: &[Message],
        heavy: Arc<crate::telemetry::hooks::Lease>,
    ) -> Result<(reqwest::Body, impl std::future::Future<Output = ()>), InferenceError> {
        let (body, released) = tokenizer_request_body(model, messages, Some(heavy))?;
        Ok((body, released.wait()))
    }

    pub(crate) fn stream_body(
        model: &str,
        max_output_tokens: u64,
        messages: &[Message],
        heavy: Arc<crate::telemetry::hooks::Lease>,
    ) -> Result<reqwest::Body, InferenceError> {
        stream::request_body(model, max_output_tokens, messages, Some(heavy))
    }

    pub(crate) async fn consume_response(
        response: reqwest::Response,
        deadline: Instant,
        idle_timeout: Duration,
        on_delta: impl FnMut(&str),
    ) -> Result<stream::StreamUsage, InferenceError> {
        stream::consume_response(response, deadline, idle_timeout, on_delta).await
    }
}

#[derive(Deserialize)]
struct TokenCount {
    input_tokens: u64,
}

#[async_trait]
impl Inference for TinfoilInference {
    async fn catalog(&self) -> Result<Vec<u8>, InferenceError> {
        let request = self.authenticate(self.http()?.get(format!("{}/v1/models", self.origin)));
        self.bounded_response(request, MAX_CATALOG_BYTES)
            .await
            .map_err(|_| InferenceFailure::CatalogFailed.into())
    }

    async fn count_tokens(
        &self,
        model: &str,
        messages: &[Message],
        heavy: Arc<crate::telemetry::hooks::Lease>,
    ) -> Result<u64, InferenceError> {
        self.count_tokens_owned(model, messages, Some(heavy)).await
    }

    async fn generate_stream(
        &self,
        model: &Model,
        messages: &[Message],
        heavy: Arc<crate::telemetry::hooks::Lease>,
        on_delta: &mut (dyn for<'delta> FnMut(&'delta str) + Send),
    ) -> Result<stream::StreamUsage, InferenceError> {
        TinfoilInference::generate_stream(self, model, messages, heavy, on_delta).await
    }

    async fn generate_completion_stream(
        &self,
        model: &Model,
        messages: &[Message],
        heavy: Arc<crate::telemetry::hooks::Lease>,
        on_delta: &mut (dyn for<'delta> FnMut(&'delta str) + Send),
    ) -> Result<stream::StreamCompletion, InferenceError> {
        TinfoilInference::generate_completion_stream(self, model, messages, heavy, on_delta).await
    }

    fn tool_profile(&self, model: &str) -> Option<ToolProfile> {
        tools::production_profile(model)
    }

    async fn count_invocation_tokens(
        &self,
        model: &str,
        invocation: &ToolInvocation,
        heavy: Arc<crate::telemetry::hooks::Lease>,
    ) -> Result<u64, InferenceError> {
        self.tool_profile(model)
            .ok_or(InferenceFailure::ToolProfileUnqualified)?;
        let (body, released) = invocation_tokenizer_body(model, invocation, heavy)?;
        let deadline = Instant::now() + Duration::from_secs(300);
        let url = format!("{}/v1/chat/completions/input_tokens", self.origin);
        let request = self
            .authenticate(self.http()?.post(&url))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body);
        let response = tokio::time::timeout_at(deadline, request.send())
            .await
            .map_err(|_| InferenceError::Detailed(InferenceFailure::TokenizerSendFailed))?
            .map_err(|_| InferenceError::Detailed(InferenceFailure::TokenizerSendFailed))?;
        if response.url().as_str() != url {
            return Err(InferenceError::Detailed(
                InferenceFailure::EndpointBindingFailed,
            ));
        }
        finish_tokenizer(response, released, deadline).await
    }

    async fn generate_invocation_stream(
        &self,
        model: &Model,
        invocation: &ToolInvocation,
        heavy: Arc<crate::telemetry::hooks::Lease>,
        on_delta: &mut (dyn for<'delta> FnMut(CompletionDelta<'delta>) + Send),
    ) -> Result<stream::StreamCompletion, InferenceError> {
        let profile = self
            .tool_profile(&model.id)
            .ok_or(InferenceFailure::ToolProfileUnqualified)?;
        let url = format!("{}/v1/chat/completions", self.origin);
        let deadline = Instant::now() + stream::STREAM_DEADLINE;
        let request = self
            .authenticate(self.http()?.post(&url))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .body(stream::invocation_body(
                &model.id,
                model.max_output_tokens,
                invocation,
                heavy.clone(),
            )?);
        heavy.dispatch_generation();
        let response = tokio::time::timeout_at(deadline, request.send())
            .await
            .map_err(|_| InferenceError::Detailed(InferenceFailure::GenerationSendFailed))?
            .map_err(|_| InferenceError::Detailed(InferenceFailure::GenerationSendFailed))?;
        if response.url().as_str() != url {
            return Err(InferenceError::Detailed(
                InferenceFailure::EndpointBindingFailed,
            ));
        }
        stream::consume_invocation_response(
            response,
            deadline,
            stream::STREAM_IDLE_TIMEOUT,
            profile,
            invocation,
            on_delta,
        )
        .await
    }

    fn verification_document(&self) -> Result<serde_json::Value, InferenceError> {
        let secure = self.client.secure_client();
        secure
            .with_verified_ground_truth(|ground_truth| {
                verification_value(ground_truth, secure.host())
            })
            .ok_or(InferenceFailure::VerificationFailed)?
    }
}

// Exhaustive destructuring intentionally makes SDK schema additions a compile
// error here. Source capacities are irrelevant: String/Vec clone copies lengths.
// Count duplicated document fields too, not only the source GroundTruth.
fn evidence_fits(ground_truth: &tinfoil::GroundTruth, host: &str) -> bool {
    let tinfoil::GroundTruth {
        config_repo,
        release_tag,
        digest,
        tls_public_key,
        hpke_public_key,
        code_measurement,
        enclave_measurement,
        code_fingerprint,
        enclave_fingerprint,
        verifier: tinfoil::SoftwareIdentity { name, version },
        verified_at,
    } = ground_truth;
    let mut remaining = MAX_EVIDENCE_STRING_BYTES;
    let mut accept = |field: &str| {
        if field.len() > MAX_EVIDENCE_FIELD_BYTES || field.len() > remaining {
            return false;
        }
        remaining -= field.len();
        true
    };
    let (Some(tls), Some(hpke)) = (tls_public_key, hpke_public_key) else {
        return false;
    };
    for field in [
        config_repo.as_str(),
        release_tag.as_deref().unwrap_or_default(),
        digest,
        tls,
        tls,
        hpke,
        hpke,
        code_fingerprint,
        enclave_fingerprint,
        name,
        version,
        verified_at,
        host,
        host,
    ] {
        if !accept(field) {
            return false;
        }
    }
    for tinfoil::Measurement {
        type_: _,
        registers,
    } in [code_measurement, enclave_measurement]
    {
        if registers.len() > MAX_EVIDENCE_REGISTERS {
            return false;
        }
        for register in registers {
            if !accept(register) {
                return false;
            }
        }
    }
    true
}

fn verification_value(
    ground_truth: &tinfoil::GroundTruth,
    host: &str,
) -> Result<serde_json::Value, InferenceError> {
    if !evidence_fits(ground_truth, host) {
        return Err(InferenceError::Detailed(
            InferenceFailure::VerificationFailed,
        ));
    }
    // The direct SDK borrow is immutable; proxy export keeps the channel read
    // lock through this callback. No check/export race, unbounded SDK export,
    // stale cache, or change to attestation/provenance/key authentication.
    let document =
        tinfoil::VerificationDocument::from_ground_truth(ground_truth.clone(), host.to_owned())
            .ok_or(InferenceError::Detailed(
                InferenceFailure::VerificationFailed,
            ))?;
    let bytes = crate::bounded_json::to_vec(&document, MAX_VERIFICATION_DOCUMENT_BYTES)
        .map_err(|_| InferenceError::Detailed(InferenceFailure::VerificationFailed))?;
    drop(document);
    serde_json::from_slice(&bytes)
        .map_err(|_| InferenceError::Detailed(InferenceFailure::VerificationFailed))
}

pub async fn authenticated_catalog(
    inference: &dyn Inference,
    now_unix: u64,
) -> Result<Catalog, InferenceError> {
    let bytes = inference.catalog().await?;
    Catalog::parse_authenticated(&bytes, now_unix)
        .map_err(|_| InferenceFailure::CatalogFailed.into())
}
