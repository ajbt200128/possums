use crate::catalog::{Catalog, Model, MAX_CATALOG_BYTES};
use async_trait::async_trait;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use thiserror::Error;
use tinfoil::Client;
use tokio::time::Instant;

pub mod stream;
#[cfg(test)]
#[path = "../tests/support/stream.rs"]
mod stream_support;

const MAX_UPSTREAM_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Generation {
    pub content: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Error)]
pub enum InferenceError {
    #[error("verified inference unavailable")]
    Unavailable,
    #[error("authenticated upstream response is invalid")]
    InvalidResponse,
}

#[async_trait]
pub trait Inference: Send + Sync {
    async fn catalog(&self) -> Result<Vec<u8>, InferenceError>;
    async fn count_tokens(&self, model: &str, messages: &[Message]) -> Result<u64, InferenceError>;
    async fn generate(
        &self,
        model: &Model,
        messages: &[Message],
    ) -> Result<Generation, InferenceError>;
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
        .map_err(|_| InferenceError::Unavailable)?
        .map_err(|_| InferenceError::Unavailable)?;
        if client.secure_client().verification_document().is_none() {
            return Err(InferenceError::Unavailable);
        }
        Ok(Self {
            origin: format!("https://{host}"),
            client,
        })
    }

    /// Additive streaming entry point; the buffered route/trait is not migrated.
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
        on_delta: impl FnMut(&str) + Send,
    ) -> Result<stream::StreamUsage, InferenceError> {
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
            )?);
        // Use the origin-bound raw transport, not the SDK's retrying chat layer.
        let response = tokio::time::timeout_at(deadline, request.send())
            .await
            .map_err(|_| InferenceError::Unavailable)?
            .map_err(|_| InferenceError::Unavailable)?;
        if response.url().as_str() != url {
            return Err(InferenceError::InvalidResponse);
        }
        stream::consume_response(response, deadline, stream::STREAM_IDLE_TIMEOUT, on_delta).await
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
            let input_tokens = self.count_tokens(model, &messages).await?;
            let request = self
                .authenticate(
                    self.http()?
                        .post(format!("{}/v1/chat/completions", self.origin)),
                )
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .header(reqwest::header::ACCEPT, "text/event-stream")
                .body(stream::request_body(model, 64, &messages)?);
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

    async fn bounded_response(
        &self,
        request: tinfoil::verifier::tls::OriginBoundRequestBuilder,
        limit: usize,
    ) -> Result<Vec<u8>, InferenceError> {
        let deadline = Instant::now() + Duration::from_secs(300);
        let response = tokio::time::timeout_at(deadline, request.send())
            .await
            .map_err(|_| InferenceError::Unavailable)?
            .map_err(|_| InferenceError::Unavailable)?;
        collect_bounded_response(response, limit, deadline).await
    }

    fn http(&self) -> Result<&tinfoil::verifier::tls::OriginBoundClient, InferenceError> {
        self.client
            .http_client()
            .map_err(|_| InferenceError::Unavailable)
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
}

#[derive(Serialize)]
struct TokenCountRequest<'a> {
    model: &'a str,
    messages: &'a [Message],
}

#[derive(Serialize)]
struct BufferedRequest<'a> {
    model: &'a str,
    messages: &'a [Message],
    max_tokens: u64,
    stream: bool,
    user_cache_secret: &'a str,
}

#[derive(Deserialize)]
struct TokenCount {
    input_tokens: u64,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    usage: Usage,
}

#[derive(Deserialize)]
struct Choice {
    message: AssistantMessage,
}

#[derive(Deserialize)]
struct AssistantMessage {
    content: String,
}

#[derive(Deserialize)]
struct Usage {
    prompt_tokens: u64,
    completion_tokens: u64,
}

#[async_trait]
impl Inference for TinfoilInference {
    async fn catalog(&self) -> Result<Vec<u8>, InferenceError> {
        let request = self.authenticate(self.http()?.get(format!("{}/v1/models", self.origin)));
        self.bounded_response(request, MAX_CATALOG_BYTES).await
    }

    async fn count_tokens(&self, model: &str, messages: &[Message]) -> Result<u64, InferenceError> {
        let request = self
            .authenticate(
                self.http()?
                    .post(format!("{}/v1/chat/completions/input_tokens", self.origin)),
            )
            .json(&TokenCountRequest { model, messages });
        let bytes = self.bounded_response(request, 64 * 1024).await?;
        let count: TokenCount =
            serde_json::from_slice(&bytes).map_err(|_| InferenceError::InvalidResponse)?;
        if count.input_tokens == 0 {
            return Err(InferenceError::InvalidResponse);
        }
        Ok(count.input_tokens)
    }

    async fn generate(
        &self,
        model: &Model,
        messages: &[Message],
    ) -> Result<Generation, InferenceError> {
        let mut cache_scope = [0_u8; 32];
        rand::rng().fill_bytes(&mut cache_scope);
        let cache_secret = base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            cache_scope,
        );
        let request = self
            .authenticate(
                self.http()?
                    .post(format!("{}/v1/chat/completions", self.origin)),
            )
            .json(&BufferedRequest {
                model: &model.id,
                messages,
                max_tokens: model.max_output_tokens,
                stream: false,
                user_cache_secret: &cache_secret,
            });
        let bytes = self
            .bounded_response(request, MAX_UPSTREAM_RESPONSE_BYTES)
            .await?;
        let response: ChatResponse =
            serde_json::from_slice(&bytes).map_err(|_| InferenceError::InvalidResponse)?;
        if response.choices.len() != 1
            || response.usage.prompt_tokens == 0
            || response.usage.completion_tokens > model.max_output_tokens
        {
            return Err(InferenceError::InvalidResponse);
        }
        let choice = response
            .choices
            .into_iter()
            .next()
            .ok_or(InferenceError::InvalidResponse)?;
        Ok(Generation {
            content: choice.message.content,
            input_tokens: response.usage.prompt_tokens,
            output_tokens: response.usage.completion_tokens,
        })
    }

    fn verification_document(&self) -> Result<serde_json::Value, InferenceError> {
        let document = self
            .client
            .secure_client()
            .verification_document()
            .ok_or(InferenceError::Unavailable)?;
        serde_json::to_value(document).map_err(|_| InferenceError::InvalidResponse)
    }
}

pub async fn authenticated_catalog(
    inference: &dyn Inference,
    now_unix: u64,
) -> Result<Catalog, InferenceError> {
    let bytes = inference.catalog().await?;
    Catalog::parse_authenticated(&bytes, now_unix).map_err(|_| InferenceError::InvalidResponse)
}
