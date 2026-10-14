use super::*;

fn image_parts() -> Value {
    json!([
        {"type":"text","text":"synthetic before"},
        {"type":"image_url","image_url":{"url":"data:image/png;base64,YWJj","detail":"auto"}},
        {"type":"text","text":"synthetic between"},
        {"type":"image_url","image_url":{"url":"data:image/jpeg;base64,YQ=="}},
        {"type":"image_url","image_url":{"url":"data:image/gif;base64,Yg==","detail":"low"}},
        {"type":"image_url","image_url":{"url":"data:image/webp;base64,Yw==","detail":"high"}},
        {"type":"text","text":"synthetic after"}
    ])
}
fn image_input(submission: &str, model: &str, tools: bool) -> Value {
    let mut input = if tools {
        structured_input(submission)
    } else {
        json!({"model":model,"stream":true,"submission":submission,"messages":[]})
    };
    input["model"] = json!(model);
    if tools {
        input["messages"][0]["content"] = image_parts();
    } else {
        input["messages"] = json!([{"role":"user","content":image_parts()}]);
    }
    input
}
fn vision_fixture(credit: u64, model: &str) -> (AppState, Arc<Provider>, String, String) {
    let (state, provider, bearer, _) = fixture(credit);
    provider.qualified.store(true, Ordering::SeqCst);
    provider.vision_catalog.store(true, Ordering::SeqCst);
    let submission = state
        .auth
        .issue_api_submission(&bearer, model, true)
        .unwrap();
    (state, provider, bearer, submission)
}
async fn send_image(state: &AppState, bearer: &str, input: &Value) -> Response {
    router(state.clone())
        .oneshot(json_request(bearer, input.clone()))
        .await
        .unwrap()
}
async fn image_duplicate(state: &AppState, bearer: &str, input: &Value, outcome: &str) {
    let response = send_image(state, bearer, input).await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["error"]["code"], "duplicate_request");
    assert_eq!(value["error"]["outcome"], outcome);
}

#[tokio::test]
async fn images_pure_user_and_mixed_tools_preserve_order_and_never_replay() {
    for model in ["deepseek-v4-1-flash", "kimi-k3", "gemma4-31b"] {
        for mode in ["user", "image_only", "tools"] {
            let (state, provider, bearer, submission) = vision_fixture(5_000_000, model);
            let mut input = image_input(&submission, model, mode == "tools");
            if mode == "image_only" {
                input["messages"][0]["content"] = json!([image_parts()[1].clone()]);
            }
            let response = send_image(&state, &bearer, &input).await;
            assert_eq!(response.status(), StatusCode::OK);
            wait(&provider.started).await;
            assert_eq!(
                provider.invocation.lock().unwrap().as_ref().unwrap()["messages"],
                input["messages"]
            );
            // Generation fixture compares the entire invocation with tokenization.
            assert_eq!(provider.tokenizer_calls.load(Ordering::SeqCst), 1);
            assert_eq!(provider.generation_calls.load(Ordering::SeqCst), 1);
            assert_eq!(provider.output_allowance.load(Ordering::SeqCst), 18);
            assert_eq!(state.accounting.available("demo"), Some(5_000_000 - 52));
            image_duplicate(&state, &bearer, &input, "in_flight").await;
            provider.finish.notify_one();
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            assert!(bytes.ends_with(b"data: [DONE]\n\n"));
            terminal(&state, 5_000_000 - 7).await;
            image_duplicate(&state, &bearer, &input, "settled").await;
            assert_eq!(provider.tokenizer_calls.load(Ordering::SeqCst), 1);
            assert_eq!(provider.generation_calls.load(Ordering::SeqCst), 1);
        }
    }
}

#[tokio::test]
async fn images_text_model_refusal_is_local_before_reservation_or_prompt() {
    let (state, provider, bearer, submission) = fixture(5_000_000);
    provider.qualified.store(true, Ordering::SeqCst);
    let input = image_input(&submission, "m", false);
    let response = send_image(&state, &bearer, &input).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["error"]["code"], "invalid_request");
    assert_eq!(value["error"]["detail"], "images_unsupported");
    assert!(value["error"]["message"]
        .as_str()
        .unwrap()
        .contains("reserved no credit and sent no upstream prompt"));
    assert!(!std::str::from_utf8(&bytes).unwrap().contains("YWJj"));
    assert_eq!(state.accounting.available("demo"), Some(5_000_000));
    assert_eq!(provider.tokenizer_calls.load(Ordering::SeqCst), 0);
    assert_eq!(provider.generation_calls.load(Ordering::SeqCst), 0);
    // Reusing the still-unused ticket for text proves no reservation was created.
    let response = send(&state, &bearer, &submission).await;
    assert_eq!(response.status(), StatusCode::OK);
    wait(&provider.started).await;
    provider.finish.notify_one();
    response.into_body().collect().await.unwrap();
    terminal(&state, 5_000_000 - 7).await;
}

#[tokio::test]
async fn images_remote_malformed_and_unknown_shapes_stop_before_prompt() {
    let invalid = [
        json!([]),
        json!([{"type":"image_url","image_url":{"url":"https://private.invalid/synthetic"}}]),
        json!([{"type":"image_url","image_url":{"url":"file:///synthetic"}}]),
        json!([{"type":"image_url","image_url":{"url":"data:image/png;base64,"}}]),
        json!([{"type":"image_url","image_url":{"url":"data:image/png,abc"}}]),
        json!([{"type":"image_url","image_url":{"url":"data:image/bmp;base64,YQ=="}}]),
        json!([{"type":"image_url","image_url":{"url":"data:image/png;base64,YQ==","extra":"hostile"}}]),
        json!([{"type":"image_url","image_url":{"url":"data:image/png;base64,YQ==","detail":null}}]),
        json!([{"type":"image_url","image_url":{"url":"data:image/png;base64,YQ=="},"extra":true}]),
        json!([{"type":"text","text":"synthetic","extra":true}]),
        json!([{"type":"input_audio","input_audio":{"data":"YQ==","format":"wav"}}]),
        json!([{"type":"unknown","text":"synthetic"}]),
    ];
    for parts in invalid {
        let (state, provider, bearer, submission) = vision_fixture(5_000_000, "kimi-k3");
        let mut input = image_input(&submission, "kimi-k3", false);
        input["messages"][0]["content"] = parts;
        let response = send_image(&state, &bearer, &input).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(state.accounting.available("demo"), Some(5_000_000));
        assert_eq!(provider.tokenizer_calls.load(Ordering::SeqCst), 0);
        assert_eq!(provider.generation_calls.load(Ordering::SeqCst), 0);
    }
    for role in ["system", "assistant", "tool"] {
        let (state, provider, bearer, submission) = vision_fixture(5_000_000, "kimi-k3");
        let mut input = image_input(&submission, "kimi-k3", false);
        input["messages"][0]["role"] = json!(role);
        assert_eq!(
            send_image(&state, &bearer, &input).await.status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(provider.tokenizer_calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn images_still_require_authenticated_catalog_membership() {
    let (state, provider, bearer, submission) = vision_fixture(5_000_000, "kimi-k3");
    provider.vision_catalog.store(false, Ordering::SeqCst);
    let input = image_input(&submission, "kimi-k3", false);
    let response = send_image(&state, &bearer, &input).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(state.accounting.available("demo"), Some(5_000_000));
    assert_eq!(provider.tokenizer_calls.load(Ordering::SeqCst), 0);
    assert_eq!(provider.generation_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn images_do_not_relax_matching_tool_transcript_or_final_user_validation() {
    for failure in ["unmatched", "pending", "empty_final"] {
        let (state, provider, bearer, submission) = vision_fixture(5_000_000, "kimi-k3");
        let mut input = image_input(&submission, "kimi-k3", true);
        match failure {
            "unmatched" => input["messages"][2]["tool_call_id"] = json!("not_prior"),
            "pending" => {
                input["messages"].as_array_mut().unwrap().pop();
            }
            _ => input["messages"]
                .as_array_mut()
                .unwrap()
                .push(json!({"role":"user","content":[]})),
        }
        assert_eq!(
            send_image(&state, &bearer, &input).await.status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(state.accounting.available("demo"), Some(5_000_000));
        assert_eq!(provider.tokenizer_calls.load(Ordering::SeqCst), 0);
        assert_eq!(provider.generation_calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn images_credit_tokenizer_and_context_failures_keep_once_only_accounting() {
    for failure in ["credit", "tokenizer", "context"] {
        let credit = if failure == "credit" { 1 } else { 5_000_000 };
        let (state, provider, bearer, submission) = vision_fixture(credit, "gemma4-31b");
        provider
            .tokenizer_error
            .store(failure == "tokenizer", Ordering::SeqCst);
        provider
            .tokens
            .store(if failure == "context" { 20 } else { 2 }, Ordering::SeqCst);
        let input = image_input(&submission, "gemma4-31b", false);
        let response = send_image(&state, &bearer, &input).await;
        assert_eq!(
            response.status(),
            match failure {
                "credit" => StatusCode::PAYMENT_REQUIRED,
                "tokenizer" => StatusCode::SERVICE_UNAVAILABLE,
                _ => StatusCode::BAD_REQUEST,
            }
        );
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        if failure == "credit" {
            assert_eq!(value["error"]["code"], "insufficient_credit");
            assert!(provider.invocation.lock().unwrap().is_none());
        } else {
            terminal(&state, credit).await;
            image_duplicate(&state, &bearer, &input, "refunded").await;
        }
        assert_eq!(state.accounting.available("demo"), Some(credit));
        assert_eq!(
            provider.tokenizer_calls.load(Ordering::SeqCst),
            usize::from(failure != "credit")
        );
        assert_eq!(provider.generation_calls.load(Ordering::SeqCst), 0);
    }
}
