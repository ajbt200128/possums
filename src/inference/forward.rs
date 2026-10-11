//! Native authenticated DATA/TRAILERS -> SDK decoder -> gateway acceptance.
use super::{
    stream::{self, SdkToolValidator, StreamCompletion, StreamUsage},
    InferenceError, InferenceFailure,
};
use async_trait::async_trait;
use futures_util::StreamExt;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Clone)]
pub struct Evidence {
    pub usage: StreamUsage,
    pub cached_tokens: u64,
    pub source: &'static str,
    pub provider_cost_usd: Option<String>,
    /// Bounded allowlisted native summary; publish only after authenticated EOF.
    pub final_metrics: Option<http::HeaderValue>,
}
impl Evidence {
    pub fn sse(usage: StreamUsage) -> Self {
        Self {
            usage,
            cached_tokens: 0,
            source: "running_sse",
            provider_cost_usd: None,
            final_metrics: None,
        }
    }
}

#[async_trait]
pub trait ForwardSink: Send {
    async fn event(&mut self, event: Value) -> Result<(), InferenceError>;
    fn evidence(&mut self, evidence: Evidence);
}

fn invalid() -> InferenceError {
    InferenceFailure::StreamUsageInvalid.into()
}
fn count(value: &str) -> Result<u64, InferenceError> {
    if value.is_empty() || value.len() > 20 || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(invalid());
    }
    value.parse().map_err(|_| invalid())
}
fn money(value: &str) -> bool {
    value.len() <= 64
        && !value.is_empty()
        && value.bytes().all(|b| b.is_ascii_digit() || b == b'.')
        && value.bytes().filter(|b| *b == b'.').count() <= 1
        && value.bytes().any(|b| b.is_ascii_digit())
}

pub fn metrics(headers: &http::HeaderMap, model: &str) -> Result<Option<Evidence>, InferenceError> {
    let mut values = headers.get_all("x-tinfoil-usage-metrics").iter();
    let Some(value) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() || value.as_bytes().len() > 4096 {
        return Err(invalid());
    }
    let value = value.to_str().map_err(|_| invalid())?;
    let mut fields = BTreeMap::new();
    for pair in value.split(',') {
        let (key, value) = pair.trim().split_once('=').ok_or_else(invalid)?;
        if fields.len() >= 32
            || key.is_empty()
            || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            || fields.insert(key, value).is_some()
        {
            return Err(invalid());
        }
    }
    if fields.get("model").copied() != Some(model) {
        return Err(invalid());
    }
    let get = |key| fields.get(key).copied().ok_or_else(invalid).and_then(count);
    let usage = StreamUsage {
        input_tokens: get("prompt")?,
        output_tokens: get("completion")?,
        total_tokens: get("total")?,
    };
    if usage.input_tokens.checked_add(usage.output_tokens) != Some(usage.total_tokens) {
        return Err(invalid());
    }
    let cached_tokens = match (
        fields.get("cached_prompt_tokens"),
        fields.get("uncached_prompt_tokens"),
    ) {
        (None, None) => 0,
        (Some(cached), Some(uncached)) => {
            let cached = count(cached)?;
            if cached.checked_add(count(uncached)?) != Some(usage.input_tokens) {
                return Err(invalid());
            }
            cached
        }
        _ => return Err(invalid()),
    };
    let provider_cost_usd = fields
        .get("cost_usd")
        .map(|cost| {
            if money(cost) {
                Ok((*cost).to_owned())
            } else {
                Err(invalid())
            }
        })
        .transpose()?;
    // Auxiliary fees are provider metadata/operator liability, never debited.
    if fields.get("other_cost_usd").is_some_and(|v| !money(v)) {
        return Err(invalid());
    }
    if let Some(calls) = fields.get("web_search_calls") {
        count(calls)?;
    }
    // Never forward arbitrary upstream fields or manufacture provider cost from
    // the submitted quote. All selected values were validated above.
    let summary = [
        "prompt",
        "completion",
        "total",
        "model",
        "cached_prompt_tokens",
        "uncached_prompt_tokens",
        "cost_usd",
        "other_cost_usd",
        "web_search_calls",
    ]
    .into_iter()
    .filter_map(|key| fields.get(key).map(|value| format!("{key}={value}")))
    .collect::<Vec<_>>()
    .join(",");
    let final_metrics = Some(summary.parse().map_err(|_| invalid())?);
    Ok(Some(Evidence {
        usage,
        cached_tokens,
        source: "final_metrics",
        provider_cost_usd,
        final_metrics,
    }))
}

pub fn sse_evidence(event: &Value) -> Result<Option<Evidence>, InferenceError> {
    let Some(usage) = event.get("usage").filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let get = |key| usage.get(key).and_then(Value::as_u64).ok_or_else(invalid);
    let counts = StreamUsage {
        input_tokens: get("prompt_tokens")?,
        output_tokens: get("completion_tokens")?,
        total_tokens: get("total_tokens")?,
    };
    if counts.input_tokens.checked_add(counts.output_tokens) != Some(counts.total_tokens) {
        return Err(invalid());
    }
    let cached = usage
        .pointer("/prompt_tokens_details/cached_tokens")
        .filter(|v| !v.is_null())
        .map(|v| v.as_u64().ok_or_else(invalid))
        .transpose()?
        .unwrap_or(0);
    if cached > counts.input_tokens {
        return Err(invalid());
    }
    if let Some(reasoning) = usage
        .pointer("/completion_tokens_details/reasoning_tokens")
        .filter(|v| !v.is_null())
    {
        if reasoning.as_u64().is_none_or(|n| n > counts.output_tokens) {
            return Err(invalid());
        }
    }
    Ok(Some(Evidence {
        cached_tokens: cached,
        ..Evidence::sse(counts)
    }))
}

fn supported(mut event: Value, model: &str) -> Result<Value, InferenceError> {
    if event.get("possums").is_some() {
        return Err(InferenceFailure::StreamEventSchemaInvalid.into());
    }
    if !model.is_empty()
        && event
            .get("model")
            .is_some_and(|v| v.as_str() != Some(model))
    {
        return Err(InferenceFailure::EndpointBindingFailed.into());
    }
    let bytes = crate::bounded_json::to_vec(&event, stream::MAX_FRAME_BYTES)
        .map_err(|_| InferenceFailure::StreamEventSchemaInvalid)?;
    stream::validate_json(&bytes).map_err(|_| InferenceFailure::StreamEventSchemaInvalid)?;
    let object = event
        .as_object_mut()
        .ok_or(InferenceFailure::StreamEventSchemaInvalid)?;
    object.retain(|key, _| ["choices", "usage"].contains(&key.as_str()));
    if let Some(choices) = object.get_mut("choices").and_then(Value::as_array_mut) {
        for choice in choices {
            if let Some(choice) = choice.as_object_mut() {
                choice.retain(|key, _| ["index", "delta", "finish_reason"].contains(&key.as_str()));
                if let Some(delta) = choice.get_mut("delta").and_then(Value::as_object_mut) {
                    delta.retain(|key, _| {
                        [
                            "role",
                            "content",
                            "tool_calls",
                            "reasoning",
                            "reasoning_content",
                        ]
                        .contains(&key.as_str())
                    });
                    for key in ["reasoning", "reasoning_content"] {
                        if delta
                            .get(key)
                            .is_some_and(|v| !v.is_null() && !v.is_string())
                        {
                            return Err(InferenceFailure::StreamDeltaUnsupported.into());
                        }
                    }
                    if let Some(calls) = delta.get_mut("tool_calls").and_then(Value::as_array_mut) {
                        for call in calls {
                            if let Some(call) = call.as_object_mut() {
                                call.retain(|key, _| {
                                    ["index", "id", "type", "function"].contains(&key.as_str())
                                });
                                if let Some(function) =
                                    call.get_mut("function").and_then(Value::as_object_mut)
                                {
                                    function.retain(|key, _| {
                                        ["name", "arguments"].contains(&key.as_str())
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    if let Some(usage) = object.get_mut("usage").and_then(Value::as_object_mut) {
        usage.retain(|key, _| {
            [
                "prompt_tokens",
                "completion_tokens",
                "total_tokens",
                "prompt_tokens_details",
                "completion_tokens_details",
                "cost_usd",
            ]
            .contains(&key.as_str())
        });
        for (key, allowed) in [
            ("prompt_tokens_details", "cached_tokens"),
            ("completion_tokens_details", "reasoning_tokens"),
        ] {
            if let Some(detail) = usage.get_mut(key).and_then(Value::as_object_mut) {
                detail.retain(|key, _| key == allowed);
            }
        }
        if usage
            .get("cost_usd")
            .is_some_and(|v| !v.as_str().is_some_and(money) && !v.is_number())
        {
            return Err(invalid());
        }
    }
    Ok(event)
}

/// Decode an already authenticated response; this function establishes no trust.
/// Production calls it only after verified origin-bound request/response checks.
pub async fn consume(
    response: reqwest::Response,
    model: &str,
    idle: Duration,
    sink: &mut dyn ForwardSink,
) -> Result<StreamCompletion, InferenceError> {
    if !response.status().is_success() {
        return Err(InferenceFailure::GenerationHttpFailed.into());
    }
    if !response
        .headers()
        .get(http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.split(';')
                .next()
                .is_some_and(|v| v.trim().eq_ignore_ascii_case("text/event-stream"))
        })
    {
        return Err(InferenceFailure::StreamContentTypeInvalid.into());
    }
    let initial_metrics = metrics(response.headers(), model)?;
    let response: http::Response<reqwest::Body> = response.into();
    let state = Arc::new(Mutex::new((None, initial_metrics, false)));
    let observed = state.clone();
    let selected = model.to_owned();
    let bytes = futures_util::stream::unfold(
        (response.into_body(), false, None::<axum::body::Bytes>),
        move |(mut body, ended, pending)| {
            let observed = observed.clone();
            let selected = selected.clone();
            async move {
                if ended {
                    return None;
                }
                let split = |mut bytes: axum::body::Bytes, body| {
                    let fragment = bytes.split_to(bytes.len().min(16 * 1024));
                    (
                        Ok::<_, InferenceFailure>(fragment),
                        (body, false, (!bytes.is_empty()).then_some(bytes)),
                    )
                };
                if let Some(bytes) = pending {
                    return Some(split(bytes, body));
                }
                loop {
                    let frame = tokio::time::timeout(idle, body.frame()).await;
                    let failure = match frame {
                        Ok(Some(Ok(frame))) => match frame.into_data() {
                            Ok(bytes) => return Some(split(bytes, body)),
                            Err(frame) => {
                                let result = frame
                                    .into_trailers()
                                    .map_err(|_| invalid())
                                    .and_then(|headers| metrics(&headers, &selected));
                                let mut state = observed.lock().unwrap();
                                if state.2 {
                                    state.0 = Some(InferenceFailure::StreamUsageInvalid);
                                }
                                state.2 = true;
                                match result {
                                    Ok(evidence) => {
                                        if evidence.is_some() {
                                            state.1 = evidence;
                                        }
                                    }
                                    Err(_) => state.0 = Some(InferenceFailure::StreamUsageInvalid),
                                }
                                if let Some(failure) = state.0 {
                                    return Some((Err(failure), (body, true, None)));
                                }
                                continue;
                            }
                        },
                        Ok(None) => return None,
                        Ok(Some(Err(_))) => InferenceFailure::StreamTransportFailed,
                        Err(_) => InferenceFailure::StreamIdleTimeout,
                    };
                    observed.lock().unwrap().0 = Some(failure);
                    return Some((Err(failure), (body, true, None)));
                }
            }
        },
    );
    let validation_state = state.clone();
    let mut events = tinfoil::sse::parse_event_stream_checked(bytes, move |payload| {
        if payload.len() > stream::MAX_FRAME_BYTES || stream::validate_json(payload).is_err() {
            return false;
        }
        match stream::validate_native_numbers(payload) {
            Ok(()) => true,
            Err(failure) => {
                validation_state.lock().unwrap().0 = Some(failure);
                false
            }
        }
    });
    let mut validator = SdkToolValidator::default();
    let mut last = None;
    while let Some(event) = events.next().await {
        let event = event.map_err(|_| {
            InferenceError::from(
                state
                    .lock()
                    .unwrap()
                    .0
                    .unwrap_or(InferenceFailure::SdkStreamDecodeFailed),
            )
        })?;
        // Validate the full native event before forwarding or committing usage.
        validator
            .accept(event.clone(), |_| {})
            .map_err(|e| InferenceError::from(e.failure()))?;
        let evidence = sse_evidence(&event)?;
        let event = supported(event, model)?;
        if let Some(evidence) = evidence {
            sink.evidence(evidence.clone());
            last = Some(evidence);
        }
        sink.event(event).await?;
    }
    // Actual transport EOF commits final evidence; a later body drop cannot undo it.
    let final_metrics = state.lock().unwrap().1.clone();
    let completion = validator.eof_completion_with_usage(final_metrics.as_ref().map(|e| e.usage));
    if let Some(evidence) = final_metrics {
        sink.evidence(evidence.clone());
        return completion
            .map(|mut c| {
                c.usage = evidence.usage;
                c
            })
            .map_err(|e| e.failure().into());
    }
    let completion = completion.map_err(|e| InferenceError::from(e.failure()))?;
    if let Some(mut evidence) = last {
        evidence.source = "final_sse";
        sink.evidence(evidence);
    }
    Ok(completion)
}

/// Legacy callbacks decode through the same native consumer. The production
/// API uses the async sink directly, without reconstructing native events.
pub(super) struct DeltaSinkAdapter<'a>(pub &'a mut dyn super::DeltaSink);
#[async_trait]
impl ForwardSink for DeltaSinkAdapter<'_> {
    async fn event(&mut self, event: Value) -> Result<(), InferenceError> {
        if let Some(choice) = event
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|c| c.first())
        {
            let choice: tinfoil::chat::ChatChoiceStream = serde_json::from_value(choice.clone())
                .map_err(|_| InferenceFailure::StreamEventSchemaInvalid)?;
            if let Some(text) = choice.delta.content.as_deref().filter(|s| !s.is_empty()) {
                self.0.emit(super::tools::CompletionDelta::Text(text)).await;
            }
            for call in choice.delta.tool_calls.as_deref().unwrap_or_default() {
                self.0
                    .emit(super::tools::CompletionDelta::ToolCall {
                        index: call.index as usize,
                        id: call.id.as_deref().filter(|v| !v.is_empty()),
                        name: call
                            .function
                            .as_ref()
                            .and_then(|f| f.name.as_deref())
                            .filter(|v| !v.is_empty()),
                        arguments: call.function.as_ref().and_then(|f| f.arguments.as_deref()),
                    })
                    .await;
            }
        }
        Ok(())
    }
    fn evidence(&mut self, _: Evidence) {}
}

// Synthetic compatibility only: borrow one fragment and await the same bounded
// sink. No delivery bridge, queue-full detachment or second provider decoder.
pub(super) struct LegacySink<'a> {
    pub sink: &'a mut dyn ForwardSink,
    pub failure: Option<InferenceFailure>,
}
#[async_trait]
impl super::DeltaSink for LegacySink<'_> {
    async fn emit(&mut self, delta: super::tools::CompletionDelta<'_>) {
        if self.failure.is_some() {
            return;
        }
        let event = delta_event(delta);
        if crate::bounded_json::to_vec(&event, stream::MAX_FRAME_BYTES).is_err() {
            self.failure = Some(InferenceFailure::StreamEventSchemaInvalid);
        } else if let Err(error) = self.sink.event(event).await {
            self.failure = Some(error.failure());
        }
    }
}
#[async_trait]
impl super::TextDeltaSink for LegacySink<'_> {
    async fn emit(&mut self, text: &str) {
        super::DeltaSink::emit(self, super::tools::CompletionDelta::Text(text)).await;
    }
}

pub(super) fn delta_event(delta: super::tools::CompletionDelta<'_>) -> Value {
    use super::tools::CompletionDelta;
    let delta = match delta {
        CompletionDelta::Text(text) => json!({"content":text}),
        CompletionDelta::ToolCall {
            index,
            id,
            name,
            arguments,
        } => {
            let mut call = json!({"index":index});
            if let Some(id) = id {
                call["id"] = json!(id);
                call["type"] = json!("function");
            }
            if name.is_some() || arguments.is_some() {
                call["function"] = json!({});
                if let Some(name) = name {
                    call["function"]["name"] = json!(name);
                }
                if let Some(arguments) = arguments {
                    call["function"]["arguments"] = json!(arguments);
                }
            }
            json!({"tool_calls":[call]})
        }
    };
    json!({"choices":[{"index":0,"delta":delta,"finish_reason":null}]})
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Bytes;
    use http_body::Frame;
    #[derive(Default)]
    struct Sink {
        events: Vec<Value>,
        evidence: Option<Evidence>,
    }
    #[async_trait]
    impl ForwardSink for Sink {
        async fn event(&mut self, event: Value) -> Result<(), InferenceError> {
            self.events.push(event);
            Ok(())
        }
        fn evidence(&mut self, evidence: Evidence) {
            self.evidence = Some(evidence);
        }
    }
    fn event(value: Value) -> Frame<Bytes> {
        Frame::data(Bytes::from(format!("data: {value}\n\n")))
    }
    fn usage(input: u64, output: u64) -> Value {
        json!({"prompt_tokens":input,"completion_tokens":output,"total_tokens":input+output})
    }
    fn choice(delta: Value, finish: Value, usage: Value) -> Value {
        json!({"model":"m","id":"private-id","choices":[{"index":0,"delta":delta,"finish_reason":finish}],"usage":usage})
    }
    fn response(frames: Vec<Result<Frame<Bytes>, std::io::Error>>) -> reqwest::Response {
        http::Response::builder()
            .header("content-type", "text/event-stream")
            .body(reqwest::Body::wrap(http_body_util::StreamBody::new(
                futures_util::stream::iter(frames),
            )))
            .unwrap()
            .into()
    }
    fn trailer(value: &str) -> Frame<Bytes> {
        let mut headers = http::HeaderMap::new();
        headers.insert("x-tinfoil-usage-metrics", value.parse().unwrap());
        Frame::trailers(headers)
    }
    #[tokio::test]
    async fn native_grouping_and_final_metrics_override_sse_at_actual_eof() {
        let frames = vec![
            Ok(event(choice(json!({"role":"assistant","content":"hello"}), Value::Null, usage(10, 1)))),
            Ok(event(choice(json!({"content":"!"}), json!("stop"), usage(10, 3)))),
            Ok(event(json!({"choices":[],"usage":usage(10, 4)}))),
            Ok(Frame::data(Bytes::from_static(b"data: [DONE]\n\n"))),
            Ok(trailer("cost_usd=999,prompt=8,completion=2,total=10,model=m,cached_prompt_tokens=3,uncached_prompt_tokens=5")),
        ];
        let mut sink = Sink::default();
        let completion = consume(response(frames), "m", Duration::from_secs(1), &mut sink)
            .await
            .unwrap();
        assert_eq!(completion.usage.input_tokens, 8);
        let evidence = sink.evidence.unwrap();
        assert_eq!(evidence.source, "final_metrics");
        assert_eq!(evidence.cached_tokens, 3);
        assert_eq!(evidence.provider_cost_usd.as_deref(), Some("999"));
        assert_eq!(
            sink.events[0]["choices"][0]["delta"],
            json!({"role":"assistant","content":"hello"})
        );
        assert_eq!(sink.events[1]["choices"][0]["delta"]["content"], "!");
        assert!(sink.events.iter().all(|event| event.get("id").is_none()));
    }
    #[tokio::test]
    async fn cumulative_interruption_keeps_last_snapshot_and_no_usage_keeps_none() {
        for with_usage in [false, true] {
            let mut frames = vec![Ok(event(choice(
                json!({"content":"partial"}),
                Value::Null,
                if with_usage { usage(2, 1) } else { Value::Null },
            )))];
            if with_usage {
                frames.push(Ok(event(choice(json!({"tool_calls":[{"index":0,"id":"call","type":"function","function":{"name":"lookup","arguments":"{"}}]}), Value::Null, usage(2, 3)))));
            }
            frames.push(Err(std::io::Error::other("private-error-marker")));
            let mut sink = Sink::default();
            let error = consume(response(frames), "m", Duration::from_secs(1), &mut sink)
                .await
                .unwrap_err();
            assert_eq!(error.failure(), InferenceFailure::StreamTransportFailed);
            assert!(!format!("{error:?} {error}").contains("private-error-marker"));
            assert_eq!(
                sink.evidence.as_ref().map(|e| e.usage.total_tokens),
                with_usage.then_some(5)
            );
            assert_eq!(
                sink.evidence.as_ref().map(|e| e.source),
                with_usage.then_some("running_sse")
            );
        }
    }
    #[tokio::test]
    async fn native_metrics_are_not_committed_before_clean_transport_eof() {
        for in_headers in [false, true] {
            let mut frames = vec![Ok(event(choice(
                json!({"content":"first"}),
                Value::Null,
                usage(2, 1),
            )))];
            if !in_headers {
                frames.push(Ok(trailer(
                    "prompt=20,completion=30,total=50,model=m,cost_usd=999",
                )));
            }
            frames.push(Err(std::io::Error::other("private-error-marker")));
            let mut response = response(frames);
            if in_headers {
                response.headers_mut().insert(
                    "x-tinfoil-usage-metrics",
                    "prompt=20,completion=30,total=50,model=m,cost_usd=999"
                        .parse()
                        .unwrap(),
                );
            }
            let mut sink = Sink::default();
            let error = consume(response, "m", Duration::from_secs(1), &mut sink)
                .await
                .unwrap_err();
            assert_eq!(error.failure(), InferenceFailure::StreamTransportFailed);
            let evidence = sink.evidence.unwrap();
            assert_eq!(evidence.usage.total_tokens, 3);
            assert_eq!(evidence.source, "running_sse");
            assert!(evidence.final_metrics.is_none());
            assert!(evidence.provider_cost_usd.is_none());
        }
    }
    #[tokio::test]
    async fn clean_metrics_can_supply_final_usage_but_not_missing_finish() {
        for finished in [false, true] {
            let frames = vec![
                Ok(event(choice(
                    json!({"content":"partial"}),
                    if finished { json!("stop") } else { Value::Null },
                    Value::Null,
                ))),
                Ok(trailer("prompt=2,completion=3,total=5,model=m")),
            ];
            let mut sink = Sink::default();
            let result = consume(response(frames), "m", Duration::from_secs(1), &mut sink).await;
            assert_eq!(result.is_ok(), finished);
            assert_eq!(sink.evidence.unwrap().source, "final_metrics");
        }
    }
    #[tokio::test]
    async fn invalid_native_events_cannot_replace_committed_usage_or_inject_receipt() {
        let invalid = [
            choice(
                json!({"content":"private-output-marker"}),
                Value::Null,
                json!({"prompt_tokens":2,"completion_tokens":1,"total_tokens":9}),
            ),
            json!({"model":"wrong","choices":[{"index":0,"delta":{"content":"private-output-marker"},"finish_reason":null}]}),
            json!({"possums":{"outcome":"settled"},"choices":[{"index":0,"delta":{},"finish_reason":null}]}),
            json!({"choices":[{"index":0,"delta":{},"finish_reason":null}],"usage":{"prompt_tokens":2,"completion_tokens":1,"total_tokens":3,"prompt_tokens_details":{"cached_tokens":3}}}),
        ];
        for invalid in invalid {
            let mut sink = Sink::default();
            let error = consume(
                response(vec![
                    Ok(event(choice(
                        json!({"content":"first"}),
                        Value::Null,
                        usage(2, 1),
                    ))),
                    Ok(event(invalid)),
                ]),
                "m",
                Duration::from_secs(1),
                &mut sink,
            )
            .await
            .unwrap_err();
            assert_eq!(sink.events.len(), 1);
            assert_eq!(sink.evidence.unwrap().usage.total_tokens, 3);
            assert!(!format!("{error:?} {error}").contains("private-output-marker"));
        }
        let mut sink = Sink::default();
        assert!(consume(
            response(vec![Ok(Frame::data(Bytes::from_static(
                b"data: {\"choices\":[],\"choices\":[]}\n\n"
            )))]),
            "m",
            Duration::from_secs(1),
            &mut sink
        )
        .await
        .is_err());
        assert!(sink.events.is_empty() && sink.evidence.is_none());
    }
    #[tokio::test]
    async fn rejected_model_or_namespace_usage_never_reaches_settlement_sink() {
        for earlier in [false, true] {
            for wrong_model in [false, true] {
                let mut rejected = choice(
                    json!({"content":"private-output-marker"}),
                    Value::Null,
                    usage(20, 30),
                );
                let failure = if wrong_model {
                    rejected["model"] = json!("wrong");
                    InferenceFailure::EndpointBindingFailed
                } else {
                    rejected["possums"] = json!({"outcome":"settled"});
                    InferenceFailure::StreamEventSchemaInvalid
                };
                let mut frames = Vec::new();
                if earlier {
                    frames.push(Ok(event(choice(
                        json!({"content":"first"}),
                        Value::Null,
                        usage(2, 1),
                    ))));
                }
                // The candidate is plausible, larger, and would increase a charge
                // if committed before model/namespace acceptance.
                frames.push(Ok(event(rejected)));
                let mut sink = Sink::default();
                let error = consume(response(frames), "m", Duration::from_secs(1), &mut sink)
                    .await
                    .unwrap_err();
                assert_eq!(error.failure(), failure);
                assert_eq!(sink.events.len(), usize::from(earlier));
                assert_eq!(
                    sink.evidence.as_ref().map(|e| e.usage.total_tokens),
                    earlier.then_some(3)
                );
                assert_eq!(
                    sink.evidence.as_ref().map(|e| e.source),
                    earlier.then_some("running_sse")
                );
                assert!(!format!("{error:?} {error}").contains("private-output-marker"));
            }
        }
    }
    #[test]
    fn native_summary_is_allowlisted_and_does_not_invent_cost_or_cache() {
        let mut headers = http::HeaderMap::new();
        headers.insert(
            "x-tinfoil-usage-metrics",
            "prompt=2,completion=1,total=3,model=m,unknown=private-header-marker"
                .parse()
                .unwrap(),
        );
        let evidence = metrics(&headers, "m").unwrap().unwrap();
        assert_eq!(
            evidence.final_metrics.unwrap(),
            "prompt=2,completion=1,total=3,model=m"
        );
        assert!(evidence.provider_cost_usd.is_none());
        headers.insert("x-tinfoil-usage-metrics", "prompt=2,completion=1,total=3,model=m,cost_usd=999.00,other_cost_usd=1.20,web_search_calls=2,cached_prompt_tokens=1,uncached_prompt_tokens=1,unknown=private-header-marker".parse().unwrap());
        let evidence = metrics(&headers, "m").unwrap().unwrap();
        assert_eq!(evidence.final_metrics.unwrap(), "prompt=2,completion=1,total=3,model=m,cached_prompt_tokens=1,uncached_prompt_tokens=1,cost_usd=999.00,other_cost_usd=1.20,web_search_calls=2");
    }
    #[test]
    fn metrics_reject_duplicates_oversize_model_and_cache_arithmetic() {
        for value in [
            "prompt=1,completion=1,total=2,model=wrong",
            "prompt=1,prompt=1,completion=1,total=2,model=m",
            "prompt=1,completion=1,total=3,model=m",
            "prompt=1,completion=1,total=2,model=m,cached_prompt_tokens=2,uncached_prompt_tokens=0",
            "prompt=1,completion=1,total=2,model=m,cached_prompt_tokens=0",
            "prompt=1,completion=1,total=2,model=m,cost_usd=private-header-marker",
        ] {
            let mut headers = http::HeaderMap::new();
            headers.insert("x-tinfoil-usage-metrics", value.parse().unwrap());
            let error = metrics(&headers, "m").err().unwrap();
            assert!(!format!("{error:?} {error}").contains("private-header-marker"));
        }
        let mut headers = http::HeaderMap::new();
        headers.insert("x-tinfoil-usage-metrics", "x".repeat(4097).parse().unwrap());
        assert!(metrics(&headers, "m").is_err());
        headers.insert(
            "x-tinfoil-usage-metrics",
            "prompt=1,completion=1,total=2,model=m".parse().unwrap(),
        );
        headers.append(
            "x-tinfoil-usage-metrics",
            "private-header-marker".parse().unwrap(),
        );
        assert!(metrics(&headers, "m").is_err());
    }
}
