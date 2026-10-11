//! Offline contract investigation: synthetic native DATA/TRAILERS through the
//! production SDK consumer, API delivery and exactly-once settlement owner.
//! These are not captured provider responses or live-provider qualification.
use super::*;
use axum::body::Bytes;
use http_body::Frame;

fn usage() -> Value {
    json!({"prompt_tokens":2,"completion_tokens":3,"total_tokens":5})
}
fn event(value: Value) -> Frame<Bytes> {
    Frame::data(Bytes::from(format!("data: {value}\n\n")))
}
fn response(terminal_usage: Value, metrics: Option<&str>) -> reqwest::Response {
    let mut frames = vec![
        event(
            json!({"model":"m","choices":[{"index":0,"delta":{"content":"synthetic-progress"},"finish_reason":null}],"usage":{"prompt_tokens":2,"completion_tokens":1,"total_tokens":3}}),
        ),
        event(
            json!({"model":"m","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":terminal_usage}),
        ),
        Frame::data(Bytes::from_static(b"data: [DONE]\n\n")),
    ];
    response_frames(&mut frames, metrics)
}
fn response_frames(frames: &mut Vec<Frame<Bytes>>, metrics: Option<&str>) -> reqwest::Response {
    if let Some(metrics) = metrics {
        let mut headers = http::HeaderMap::new();
        headers.insert("x-tinfoil-usage-metrics", metrics.parse().unwrap());
        frames.push(Frame::trailers(headers));
    }
    http::Response::builder()
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(reqwest::Body::wrap(http_body_util::StreamBody::new(
            futures_util::stream::iter(
                std::mem::take(frames)
                    .into_iter()
                    .map(Ok::<_, std::io::Error>),
            ),
        )))
        .unwrap()
        .into()
}

pub(super) fn duplicate_response(values: &[&str], initial_header: bool) -> reqwest::Response {
    let mut frames = vec![
        event(
            json!({"model":"m","choices":[{"index":0,"delta":{"content":"synthetic-progress"},"finish_reason":null}],"usage":{"prompt_tokens":2,"completion_tokens":1,"total_tokens":3}}),
        ),
        event(
            json!({"model":"m","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":usage()}),
        ),
        Frame::data(Bytes::from_static(b"data: [DONE]\n\n")),
    ];
    let mut headers = http::HeaderMap::new();
    for value in values {
        headers.append("x-tinfoil-usage-metrics", value.parse().unwrap());
    }
    if !initial_header {
        frames.push(Frame::trailers(headers.clone()));
    }
    let mut response: http::Response<reqwest::Body> = response_frames(&mut frames, None).into();
    if initial_header {
        response.headers_mut().extend(headers);
    }
    response.into()
}

pub(super) async fn check(
    upstream: reqwest::Response,
    failure: Option<InferenceFailure>,
    source: &str,
    total: u64,
    charge: u64,
) {
    let (state, provider, bearer, submission) = fixture(5_000_000);
    *provider.native_response.lock().unwrap() = Some(upstream);
    let mut request = request(&bearer, &submission);
    request
        .headers_mut()
        .insert("x-tinfoil-request-usage-metrics", "true".parse().unwrap());
    let response = router(state.clone()).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body();
    let mut data = Vec::new();
    let mut trailers = 0;
    while let Some(frame) = body.frame().await {
        let frame = frame.unwrap();
        match frame.into_data() {
            Ok(bytes) => data.extend_from_slice(&bytes),
            Err(frame) => {
                assert!(data.ends_with(b"data: [DONE]\n\n"));
                let headers = frame.into_trailers().unwrap();
                assert_eq!(headers.len(), 1);
                assert!(!headers["x-tinfoil-usage-metrics"]
                    .to_str()
                    .unwrap()
                    .contains("private-marker"));
                trailers += 1;
            }
        }
    }
    assert_eq!(
        trailers,
        usize::from(failure.is_none() && source == "final_metrics")
    );
    assert!(data.ends_with(b"data: [DONE]\n\n"));
    let text = std::str::from_utf8(&data).unwrap();
    assert!(text.contains("synthetic-progress"));
    assert!(!text.contains("private-marker"));
    let events: Vec<Value> = text
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter(|data| *data != "[DONE]")
        .map(|data| serde_json::from_str(data).unwrap())
        .collect();
    let receipts: Vec<_> = events.iter().filter_map(|e| e.get("possums")).collect();
    assert_eq!(receipts.len(), 1);
    let receipt = receipts[0];
    assert_eq!(
        receipt["status"],
        if failure.is_some() {
            "incomplete"
        } else {
            "complete"
        }
    );
    assert_eq!(receipt["outcome"], "settled");
    assert_eq!(receipt["evidence"], source);
    assert_eq!(receipt["usage"]["total_tokens"], total);
    assert_eq!(receipt["reserved_microunits"], "52");
    assert_eq!(
        receipt["quoted_input_microunits_per_million_tokens"],
        "1000000"
    );
    assert_eq!(
        receipt["quoted_output_microunits_per_million_tokens"],
        "1000000"
    );
    assert_eq!(receipt["markup_percent"], 130);
    assert_eq!(receipt["charged_microunits"], charge.to_string());
    assert_eq!(receipt["refunded_microunits"], (52 - charge).to_string());
    let errors: Vec<_> = events.iter().filter_map(|e| e.get("error")).collect();
    if let Some(failure) = failure {
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0]["code"], "generation_failed");
        assert_eq!(errors[0]["detail"], serde_json::to_value(failure).unwrap());
        assert_eq!(errors[0]["billing"], "unknown");
    } else {
        assert!(errors.is_empty());
    }
    terminal(&state, 5_000_000 - charge).await;
    let snapshot = state.accounting.snapshot("demo").unwrap();
    assert_eq!(snapshot.in_flight, 0);
    assert_eq!(snapshot.completed_requests, 1);
    duplicate(&state, &bearer, &submission, "settled").await;
    assert_eq!(provider.generation_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn valid_native_usage_shapes_survive_sdk_and_settlement() {
    let variants = [
        usage(),
        json!({"prompt_tokens":2,"completion_tokens":3,"total_tokens":5,"prompt_tokens_details":null,"completion_tokens_details":null}),
        json!({"prompt_tokens":2,"completion_tokens":3,"total_tokens":5,"prompt_tokens_details":{"cached_tokens":null},"completion_tokens_details":{"reasoning_tokens":null}}),
        json!({"prompt_tokens":2,"completion_tokens":3,"total_tokens":5,"prompt_tokens_details":{"cached_tokens":1},"completion_tokens_details":{"reasoning_tokens":2}}),
        json!({"prompt_tokens":2,"completion_tokens":3,"total_tokens":5,"prompt_cache_hit_tokens":1,"prompt_cache_miss_tokens":1,"completion_tokens_details":{"reasoning_tokens":3,"accepted_prediction_tokens":null},"extra":{"value":"private-marker"}}),
        json!({"prompt_tokens":2,"completion_tokens":3,"total_tokens":5,"cost_usd":0.0000001}),
        json!({"prompt_tokens":2,"completion_tokens":3,"total_tokens":5,"cost_usd":"0.0000001"}),
    ];
    for terminal_usage in variants {
        // Running snapshots require terminal metrics here, not finish usage
        // alone. Router-style final metrics must not be compared to raw SSE
        // snapshots: its extractor can normalize/accumulate them differently.
        for metrics in [
            "prompt=2,completion=3,total=5,model=m",
            "cost_usd=0.000000001,uncached_prompt_tokens=1,model=m,prompt=2,cached_prompt_tokens=1,total=5,completion=3,unknown=private-marker",
            "prompt=2,completion=3,total=5,model=m,cost_usd=0,other_cost_usd=1.2,web_search_calls=2",
        ] {
            check(response(terminal_usage.clone(), Some(metrics)), None, "final_metrics", 5, 7).await;
        }
    }
    check(
        response(usage(), Some("prompt=2,completion=4,total=6,model=m")),
        None,
        "final_metrics",
        6,
        8,
    )
    .await;
}

#[tokio::test]
async fn final_usage_groupings_do_not_require_running_counters_or_trailers() {
    for usage_only in [false, true] {
        let mut frames = vec![event(
            json!({"model":"m","choices":[{"index":0,"delta":{"content":"synthetic-progress","role":"assistant","p":"private-marker"},"finish_reason":null}],"usage":null}),
        )];
        frames.push(event(json!({"model":"m","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":if usage_only { Value::Null } else { usage() }})));
        if usage_only {
            frames.push(event(json!({"choices":[],"usage":usage()})));
        }
        frames.push(Frame::data(Bytes::from_static(b"data: [DONE]\n\n")));
        check(response_frames(&mut frames, None), None, "final_sse", 5, 7).await;
    }
    // Running counters plus finish usage alone remain insufficient for final
    // success. A missing trailer is NOT diagnosed as invalid arithmetic.
    check(
        response(usage(), None),
        Some(InferenceFailure::StreamUsageMissing),
        "running_sse",
        5,
        7,
    )
    .await;
}

#[tokio::test]
async fn distinct_sse_rejections_share_closed_diagnostic_and_keep_prior_evidence() {
    let invalid = [
        json!({"prompt_tokens":2.0,"completion_tokens":3,"total_tokens":5}),
        json!({"prompt_tokens":"2","completion_tokens":3,"total_tokens":5}),
        json!({"input_tokens":2,"output_tokens":3,"total_tokens":5}),
        json!({"prompt_tokens":2,"completion_tokens":3,"total_tokens":6}),
        json!({"prompt_tokens":2,"completion_tokens":3,"total_tokens":5,"prompt_tokens_details":{"cached_tokens":3}}),
        json!({"prompt_tokens":2,"completion_tokens":3,"total_tokens":5,"completion_tokens_details":{"reasoning_tokens":4}}),
        json!({"prompt_tokens":2,"completion_tokens":3,"total_tokens":5,"completion_tokens_details":{"reasoning_tokens":-1}}),
        json!({"prompt_tokens":2,"completion_tokens":3,"total_tokens":5,"cost_usd":null}),
        json!({"prompt_tokens":2,"completion_tokens":3,"total_tokens":5,"cost_usd":"private-marker"}),
    ];
    for terminal_usage in invalid {
        // A valid pending trailer is not authority to mask an invalid SSE event.
        check(
            response(
                terminal_usage,
                Some("prompt=2,completion=3,total=5,model=m"),
            ),
            Some(InferenceFailure::StreamUsageInvalid),
            "running_sse",
            3,
            4,
        )
        .await;
    }
}

#[tokio::test]
async fn distinct_trailer_rejections_share_closed_diagnostic_and_keep_prior_evidence() {
    for metrics in [
        "prompt=2,completion=3,total=5,model=wrong",
        "prompt=2,completion=3,total=5",
        "prompt=2,completion=3,total=6,model=m",
        "prompt=2,prompt=2,completion=3,total=5,model=m",
        "prompt=2,completion=3,total=5,model=m,cached_prompt_tokens=1",
        "prompt=2,completion=3,total=5,model=m,cached_prompt_tokens=1,uncached_prompt_tokens=2",
        "prompt=2,completion=3,total=5,model=m,cost_usd=1e-9",
        "prompt=2,completion=3,total=5,model=m,cost_usd=private-marker",
    ] {
        check(
            response(usage(), Some(metrics)),
            Some(InferenceFailure::StreamUsageInvalid),
            "running_sse",
            5,
            7,
        )
        .await;
    }
}
