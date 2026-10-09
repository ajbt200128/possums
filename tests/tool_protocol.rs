//! Synthetic protocol/schema coverage; not production model qualification.
use futures_util::StreamExt;
use possums::inference::{
    stream::{FinishReason, ProtocolParser, SdkToolValidator, StreamError},
    tools::{CompletionDelta, Tool, ToolChoice, ToolInvocation, ToolMessage},
};
use serde_json::{json, Value};

fn invocation(
    messages: Value,
    tools: Option<Value>,
    choice: Option<Value>,
) -> Result<ToolInvocation, ()> {
    let messages: Vec<ToolMessage> = serde_json::from_value(messages).map_err(|_| ())?;
    let tools: Option<Vec<Tool>> = tools
        .map(serde_json::from_value)
        .transpose()
        .map_err(|_| ())?;
    let choice: Option<ToolChoice> = choice
        .map(serde_json::from_value)
        .transpose()
        .map_err(|_| ())?;
    ToolInvocation::new(messages, tools, choice).map_err(|_| ())
}
fn tools() -> Value {
    json!([{"type":"function","function":{"name":"lookup","description":"fixture","parameters":{"type":"object","properties":{"q":{"type":"string"}}}}}])
}
fn parser() -> SdkToolValidator {
    SdkToolValidator::default()
}
fn event(value: Value) -> Vec<u8> {
    format!("data: {value}\n\n").into_bytes()
}
fn delta(value: Value) -> Vec<u8> {
    event(json!({"choices":[{"index":0,"delta":value,"finish_reason":null}]}))
}
fn call(index: usize, id: &str, arguments: &str) -> Value {
    json!({"index":index,"id":id,"type":"function","function":{"name":"lookup","arguments":arguments}})
}
fn finish(reason: &str) -> Vec<u8> {
    event(json!({"choices":[{"index":0,"delta":{},"finish_reason":reason}]}))
}
fn usage() -> Vec<u8> {
    event(json!({"choices":[],"usage":{"prompt_tokens":2,"completion_tokens":3,"total_tokens":5}}))
}
fn history() -> Value {
    json!([
        {"role":"user","content":"fixture"},
        {"role":"assistant","content":null,"tool_calls":[{"id":"call_0","type":"function","function":{"name":"lookup","arguments":"{\"q\":\"x\"}"}}]},
        {"role":"tool","tool_call_id":"call_0","content":"result"}
    ])
}

#[test]
fn strict_schema_and_completed_history_without_fake_user() {
    let input = invocation(history(), Some(tools()), Some(json!("auto"))).unwrap();
    assert_eq!(serde_json::to_value(input).unwrap()["messages"], history());
    // Historical declarations may be absent; their presence never bypasses qualification.
    assert!(invocation(history(), None, None).is_ok());
    for choice in [
        json!("auto"),
        json!("none"),
        json!("required"),
        json!({"type":"function","function":{"name":"lookup"}}),
    ] {
        assert!(invocation(history(), Some(tools()), Some(choice)).is_ok());
    }
    for messages in [
        json!([]),
        json!([{"role":"user","content":""}]),
        json!([{"role":"assistant","content":null}]),
        json!([{"role":"assistant","tool_calls":[]}]),
        json!([{"role":"user","content":[{"type":"image_url"}]}]),
        json!([{"role":"tool","tool_call_id":"missing","content":"result"}]),
        json!([{"role":"assistant","content":"x","tool_calls":null},{"role":"user","content":"ok"}]),
    ] {
        assert!(invocation(messages, Some(tools()), None).is_err());
    }
    for mutate in 0..6 {
        let mut messages = history();
        match mutate {
            0 => {
                messages.as_array_mut().unwrap().pop();
            }
            1 => {
                messages[2]["tool_call_id"] = json!("missing");
            }
            2 => {
                messages[1]["tool_calls"][0]["function"]["name"] = json!("n".repeat(65));
            }
            3 => {
                messages[1]["tool_calls"][0]["function"]["arguments"] = json!({});
            }
            4 => {
                messages
                    .as_array_mut()
                    .unwrap()
                    .insert(2, json!({"role":"user","content":"interruption"}));
            }
            _ => {
                let call = messages[1]["tool_calls"][0].clone();
                messages[1]["tool_calls"].as_array_mut().unwrap().push(call);
            }
        }
        assert!(invocation(messages, Some(tools()), None).is_err());
    }
    let mut duplicate = tools();
    duplicate.as_array_mut().unwrap().push(tools()[0].clone());
    assert!(invocation(history(), Some(duplicate), None).is_err());
    assert!(invocation(history(), Some(json!([])), None).is_err());
    assert!(invocation(history(), None, Some(json!("auto"))).is_err());
    assert!(invocation(
        history(),
        Some(tools()),
        Some(json!({"type":"function","function":{"name":"other"}}))
    )
    .is_ok());
    let mut messages = history();
    messages[1]["tool_calls"][0]["function"]["name"] = json!("historical");
    assert!(invocation(messages, Some(tools()), None).is_ok());
    for arguments in ["[]", "{\"x\":1,\"x\":2}", "{", "", "not JSON"] {
        let mut messages = history();
        messages[1]["tool_calls"][0]["function"]["arguments"] = json!(arguments);
        let input = invocation(messages.clone(), Some(tools()), None).unwrap();
        assert_eq!(serde_json::to_value(input).unwrap()["messages"], messages);
    }
    for name in ["".to_owned(), "n".repeat(65), "bad name".to_owned()] {
        assert!(invocation(
            history(),
            Some(tools()),
            Some(json!({"type":"function","function":{"name":name}}))
        )
        .is_err());
    }
}

#[test]
fn historical_calls_catalog_and_messages_use_body_bounds_not_stream_quotas() {
    let calls: Vec<_> = (0..66).map(|i| json!({"id":format!("call_{i}"),"type":"function","function":{"name":"lookup","arguments":serde_json::to_string(&json!({"x":"a".repeat(65 * 1024)})).unwrap()}})).collect();
    let mut messages = vec![json!({"role":"assistant","content":null,"tool_calls":calls})];
    messages.extend(
        (0..66).map(
            |i| json!({"role":"tool","tool_call_id":format!("call_{i}"),"content":"synthetic"}),
        ),
    );
    messages.extend((0..4097).map(|_| json!({"role":"user","content":"synthetic"})));
    let catalog = (0..65).map(|i| json!({"type":"function","function":{"name":format!("tool_{i}"),"parameters":{"type":"object"}}})).collect::<Vec<_>>();
    let messages = json!(messages);
    let catalog = json!(catalog);
    assert!(
        serde_json::to_vec(&json!({"messages":messages,"tools":catalog}))
            .unwrap()
            .len()
            < 8 * 1024 * 1024
    );
    let input = invocation(messages.clone(), Some(catalog), None).unwrap();
    assert_eq!(serde_json::to_value(input).unwrap()["messages"], messages);
    let sequential: Vec<_> = (0..66)
        .flat_map(|i| {
            let mut batch = history().as_array().unwrap().clone();
            batch[1]["tool_calls"][0]["id"] = json!(format!("prior_{i}"));
            batch[2]["tool_call_id"] = json!(format!("prior_{i}"));
            batch
        })
        .collect();
    assert!(invocation(json!(sequential), Some(tools()), None).is_ok());
}

// Every fixture goes through the actual pinned SDK decoder, not a shadow SSE parser.
async fn feed_chunks(
    parser: &mut SdkToolValidator,
    chunks: Vec<Vec<u8>>,
    mut callback: impl FnMut(CompletionDelta<'_>),
) -> Result<(), StreamError> {
    let bytes = futures_util::stream::iter(
        chunks
            .into_iter()
            .map(|b| Ok::<_, std::io::Error>(b.into())),
    );
    let mut events = tinfoil::sse::parse_event_stream(bytes);
    while let Some(event) = events.next().await {
        parser.accept(event.map_err(|_| StreamError::Protocol)?, &mut callback)?;
    }
    Ok(())
}
async fn feed(
    parser: &mut SdkToolValidator,
    bytes: &[u8],
    callback: impl FnMut(CompletionDelta<'_>),
) -> Result<(), StreamError> {
    feed_chunks(parser, vec![bytes.to_vec()], callback).await
}

#[tokio::test]
async fn sdk_fragmentation_nullable_metadata_and_optional_done() {
    // No full response envelope, optional finish omitted; SDK metadata is ignored.
    let prefix = [
        event(json!({"choices":[{"index":0,"delta":{"role":"assistant","tool_calls":null}}],"usage":null})),
        delta(json!({"tool_calls":[]})),
        delta(json!({"content":"preface","tool_calls":[call(0,"id_0","{\"q\":\"")]})),
        delta(json!({"tool_calls":[{"index":0,"id":null,"type":null,
            "function":{"name":null,"arguments":"é🐾"}}]})),
        delta(json!({"tool_calls":[{"index":0,"id":"","type":null,
            "function":{"name":"","arguments":"\"}"}}]})),
        delta(json!({"tool_calls":[{"index":0,"id":null,"type":null,"function":null}]})),
    ]
    .concat();
    for (reason, expected) in [
        ("stop", FinishReason::Stop),
        ("tool_calls", FinishReason::ToolUse),
        ("length", FinishReason::Length),
    ] {
        for done in [false, true] {
            let mut bytes = [prefix.clone(), finish(reason), usage()].concat();
            if done {
                bytes.extend(b"data: [DONE]\n\n");
            }
            for split in 0..=bytes.len() {
                let mut p = parser();
                let mut arguments = String::new();
                let mut text = String::new();
                feed_chunks(
                    &mut p,
                    vec![bytes[..split].to_vec(), bytes[split..].to_vec()],
                    |d| match d {
                        CompletionDelta::Text(t) => text.push_str(t),
                        CompletionDelta::ToolCall {
                            arguments: Some(a), ..
                        } => arguments.push_str(a),
                        _ => {}
                    },
                )
                .await
                .unwrap();
                let receipt = p.eof_completion().unwrap();
                assert_eq!(arguments, "{\"q\":\"é🐾\"}");
                assert_eq!(text, "preface");
                assert_eq!(receipt.finish_reason, expected);
                assert_eq!(receipt.usage.total_tokens, 5);
            }
        }
    }
    let mut p = parser();
    feed(&mut p, &prefix, |_| {}).await.unwrap();
    assert!(p.eof_completion().is_err()); // Never synthesize a finish or usage.
    let mut legacy = ProtocolParser::default();
    assert!(legacy
        .feed(&prefix, |_| panic!("legacy rejects omitted finish"))
        .is_err());
}

#[tokio::test]
async fn sdk_accepts_noncritical_extras_and_its_trailing_frame_semantics() {
    let mut c = call(0, "id", "not executable JSON");
    c["function"]["extra"] = json!("x".repeat(20_000));
    c["extra"] = json!(true);
    let mut chunk = json!({"choices":[{"index":0,"delta":{"tool_calls":[c],"backend_extra":true},
        "choice_extra":true}], "metadata": {"unknown":true}});
    chunk["choices"][0]["finish_reason"] = Value::Null;
    let bytes = [event(chunk), finish("tool_calls"), usage()].concat();
    // SDK accepts SSE metadata and a trailing event without a blank line.
    let mut p = parser();
    let mut framed = b"event: arbitrary\nid: ignored\n".to_vec();
    framed.extend(bytes);
    framed.truncate(framed.len() - 2);
    feed_chunks(
        &mut p,
        framed.chunks(1).map(<[u8]>::to_vec).collect(),
        |_| {},
    )
    .await
    .unwrap();
    assert!(p.eof_completion().is_ok());
}

#[tokio::test]
async fn invalid_mixed_events_are_atomic_and_semantic_failure_is_absorbing() {
    for tool_call in [
        call(64, "id", "{}"),
        call(1, "id", "{}"),
        json!({"index":0,"id":"x","type":"function","function":{"name":"n".repeat(65),"arguments":"{}"}}),
        json!({"index":0,"id":"x","type":"function","function":{"name":"lookup","arguments":{}}}),
        json!({"index":0,"id":"x","type":"not_function","function":{"name":"lookup","arguments":"{}"}}),
        call(0, &"i".repeat(129), "{}"),
        json!({"index":-1}),
    ] {
        let mut p = parser();
        assert!(feed(
            &mut p,
            &delta(json!({"content":"not delivered","tool_calls":[tool_call]})),
            |_| panic!("invalid event leaked")
        )
        .await
        .is_err());
        assert!(feed(&mut p, &delta(json!({"content":"later"})), |_| panic!(
            "absorbing failure"
        ))
        .await
        .is_err());
        assert!(p.eof_completion().is_err());
    }
    for conflict in [
        json!({"index":0,"id":"changed"}),
        json!({"index":0,"function":{"name":"other"}}),
        call(1, "first", "{}"),
    ] {
        let mut p = parser();
        feed(
            &mut p,
            &delta(json!({"tool_calls":[call(0,"first","{")]})),
            |_| {},
        )
        .await
        .unwrap();
        assert!(feed(
            &mut p,
            &delta(json!({"content":"not delivered","tool_calls":[conflict]})),
            |_| panic!("conflict leaked")
        )
        .await
        .is_err());
    }
    for chunk in [
        json!({"choices":[{"index":0,"delta":{"content":"not delivered","tool_calls":[call(0,"x","{}"),call(0,"x","{}")]}}]}),
        json!({"choices":[{"index":1,"delta":{"content":"not delivered"}}]}),
        json!({"choices":[{"index":0,"delta":{"content":"not delivered"}},{"index":1,"delta":{}}]}),
        json!({"choices":[]}),
    ] {
        assert!(feed(&mut parser(), &event(chunk), |_| panic!(
            "invalid choice leaked"
        ))
        .await
        .is_err());
    }
}

#[tokio::test]
async fn existing_call_and_argument_bounds() {
    let mut p = parser();
    for index in 0..64 {
        feed(
            &mut p,
            &delta(json!({"tool_calls":[call(index,&format!("id_{index}"),"{}")]})),
            |_| {},
        )
        .await
        .unwrap();
    }
    assert!(feed(
        &mut p,
        &delta(json!({"tool_calls":[call(64,"extra","{}")]})),
        |_| panic!()
    )
    .await
    .is_err());
    for total_limit in [false, true] {
        let mut p = parser();
        for index in 0..if total_limit { 4 } else { 1 } {
            for part in 0..16 {
                let c = if part == 0 {
                    call(index, &format!("id_{index}"), &"x".repeat(4096))
                } else {
                    json!({"index":index,"function":{"arguments":"x".repeat(4096)}})
                };
                feed(&mut p, &delta(json!({"tool_calls":[c]})), |_| {})
                    .await
                    .unwrap();
            }
        }
        let c = if total_limit {
            call(4, "extra", "x")
        } else {
            json!({"index":0,"function":{"arguments":"x"}})
        };
        assert!(feed(&mut p, &delta(json!({"tool_calls":[c]})), |_| panic!(
            "byte limit leaked"
        ))
        .await
        .is_err());
    }
}

#[tokio::test]
async fn incomplete_calls_and_empty_batches_preserve_all_finish_reasons() {
    for missing in ["id", "name", "type", "arguments", "function", "all"] {
        for absent in [false, true] {
            let mut c = call(0, "id", "{");
            let target = if matches!(missing, "name" | "arguments") {
                c["function"].as_object_mut().unwrap()
            } else {
                c.as_object_mut().unwrap()
            };
            if missing == "all" {
                c = json!({"index":0});
            } else if absent {
                target.remove(missing);
            } else {
                target.insert(missing.into(), Value::Null);
            }
            for (reason, expected) in [
                ("stop", FinishReason::Stop),
                ("tool_calls", FinishReason::ToolUse),
                ("length", FinishReason::Length),
            ] {
                let mut p = parser();
                let bytes = [delta(json!({"tool_calls":[c]})), finish(reason), usage()].concat();
                feed(&mut p, &bytes, |_| {}).await.unwrap();
                assert_eq!(p.eof_completion().unwrap().finish_reason, expected);
            }
        }
    }
    for (reason, expected) in [
        ("stop", FinishReason::Stop),
        ("tool_calls", FinishReason::ToolUse),
        ("length", FinishReason::Length),
    ] {
        for calls in [vec![], vec![call(0, "", "")]] {
            let mut p = parser();
            feed(
                &mut p,
                &[delta(json!({"tool_calls":calls})), finish(reason), usage()].concat(),
                |_| {},
            )
            .await
            .unwrap();
            assert_eq!(p.eof_completion().unwrap().finish_reason, expected);
        }
    }
}

#[tokio::test]
async fn returned_identity_is_bounded_not_authorized_or_grammar_checked() {
    for name in ["undeclared".to_owned(), "é/🐾 ?".to_owned(), "é".repeat(32)] {
        let id = "🐾".repeat(32); // The existing 128-byte ceiling, not a character cap.
        let mut c = call(0, &id, "not executable JSON");
        c["function"]["name"] = json!(name);
        let mut p = parser();
        let mut emitted = 0;
        feed(
            &mut p,
            &[delta(json!({"tool_calls":[c]})), finish("stop"), usage()].concat(),
            |delta| {
                if let CompletionDelta::ToolCall {
                    id: returned_id,
                    name: returned_name,
                    ..
                } = delta
                {
                    assert_eq!(returned_id, Some(id.as_str()));
                    assert_eq!(returned_name, Some(name.as_str()));
                    emitted += 1;
                }
            },
        )
        .await
        .unwrap();
        assert_eq!(emitted, 1);
        assert_eq!(
            p.eof_completion().unwrap().finish_reason,
            FinishReason::Stop
        );
    }
}

#[tokio::test]
async fn sdk_continuous_usage_is_metadata_until_the_final_usage_chunk() {
    let prefix = [
        event(json!({"choices":[{"index":0,"delta":{"role":"assistant"}}],
            "usage":{"prompt_tokens":2,"completion_tokens":0,"total_tokens":2}})),
        event(
            json!({"choices":[{"index":0,"delta":{"tool_calls":[call(0,"id","{\"q\":\"é🐾\"}")]}}],
            "usage":{"prompt_tokens":2,"completion_tokens":1,"total_tokens":3}}),
        ),
        event(
            json!({"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],
            "usage":{"prompt_tokens":2,"completion_tokens":2,"total_tokens":4}}),
        ),
    ]
    .concat();
    let bytes = [prefix.clone(), usage(), b"data: [DONE]\n\n".to_vec()].concat();
    for split in 0..=bytes.len() {
        let mut p = parser();
        let mut calls = 0;
        feed_chunks(
            &mut p,
            vec![bytes[..split].to_vec(), bytes[split..].to_vec()],
            |delta| {
                assert!(matches!(delta, CompletionDelta::ToolCall { .. }));
                calls += 1;
            },
        )
        .await
        .unwrap();
        let completion = p.eof_completion().unwrap();
        assert_eq!(calls, 1);
        assert_eq!(completion.finish_reason, FinishReason::ToolUse);
        assert_eq!(completion.usage.input_tokens, 2);
        assert_eq!(completion.usage.output_tokens, 3); // Final, not interim/finish counts.
        assert_eq!(completion.usage.total_tokens, 5);
    }
    // Continuous finish-chunk counts are still a snapshot, not a final receipt.
    let mut p = parser();
    feed(&mut p, &prefix, |_| {}).await.unwrap();
    assert!(p.eof_completion().is_err());
}

fn choice_usage(delta: Value, reason: Option<&str>, output: u64) -> Vec<u8> {
    event(
        json!({"choices":[{"index":0,"delta":delta,"finish_reason":reason}],
        "usage":{"prompt_tokens":2,"completion_tokens":output,"total_tokens":2 + output}}),
    )
}

#[tokio::test]
async fn sdk_finish_usage_candidate_is_compatible_but_separate_final_wins() {
    for separate_final in [false, true] {
        let mut p = parser();
        feed(
            &mut p,
            &choice_usage(json!({}), Some("stop"), 1),
            |_| panic!(),
        )
        .await
        .unwrap();
        if separate_final {
            feed(&mut p, &usage(), |_| panic!()).await.unwrap();
        }
        let completion = p.eof_completion().unwrap();
        assert_eq!(completion.finish_reason, FinishReason::Stop);
        assert_eq!(completion.usage.input_tokens, 2);
        assert_eq!(
            completion.usage.output_tokens,
            if separate_final { 3 } else { 1 }
        );
    }
}

#[tokio::test]
async fn sdk_invalid_interim_usage_is_atomic_and_absorbing() {
    let mut invalid = Vec::new();
    for field in ["prompt_tokens", "completion_tokens", "total_tokens"] {
        for value in [
            json!(-1),
            json!(1.5),
            json!("2"),
            json!(null),
            json!(18446744073709551616_u128),
        ] {
            let mut usage = json!({"prompt_tokens":2,"completion_tokens":1,"total_tokens":3});
            usage[field] = value;
            invalid.push(usage);
        }
    }
    invalid.extend([
        json!({"prompt_tokens":u64::MAX,"completion_tokens":1,"total_tokens":0}),
        json!({"prompt_tokens":2,"completion_tokens":1,"total_tokens":4}),
    ]);
    for usage in invalid {
        let mut p = parser();
        let bytes = event(json!({"choices":[{"index":0,"delta":{
            "content":"must not publish","tool_calls":[call(0,"id","{}")]
        }}],"usage":usage}));
        let error = feed(&mut p, &bytes, |_| {
            panic!("invalid usage leaked mixed output")
        })
        .await
        .unwrap_err();
        assert_eq!(
            error.failure(),
            possums::inference::InferenceFailure::StreamUsageInvalid
        );
        assert!(feed(
            &mut p,
            &choice_usage(json!({}), Some("stop"), 3),
            |_| panic!()
        )
        .await
        .is_err());
        assert!(p.eof_completion().is_err());
    }
}

#[tokio::test]
async fn sdk_continuous_usage_preserves_terminal_failures() {
    use possums::inference::InferenceFailure::*;
    let snapshot = choice_usage(json!({"role":"assistant"}), None, 0);
    let terminal = choice_usage(json!({}), Some("stop"), 1);
    for (suffix, failure, at_eof) in [
        (vec![], StreamFinishMissing, true),
        (usage(), StreamUsageUnexpected, false),
        (terminal.clone(), StreamUsageMissing, true),
        (
            [terminal.clone(), usage(), usage()].concat(),
            StreamUsageUnexpected,
            false,
        ),
        (
            [
                terminal.clone(),
                usage(),
                event(json!({"choices":[],
                "usage":{"prompt_tokens":2,"completion_tokens":9,"total_tokens":11}})),
            ]
            .concat(),
            StreamUsageUnexpected,
            false,
        ),
        (
            [terminal.clone(), usage(), delta(json!({"content":"late"}))].concat(),
            StreamOutputAfterFinish,
            false,
        ),
        (
            [terminal, usage(), event(json!({"error":"private-marker"}))].concat(),
            UpstreamErrorEvent,
            false,
        ),
    ] {
        let mut p = parser();
        feed(&mut p, &snapshot, |_| panic!()).await.unwrap();
        let result = feed(&mut p, &suffix, |_| panic!("invalid stream leaked output")).await;
        let error = if at_eof {
            result.unwrap();
            p.eof_completion().unwrap_err()
        } else {
            let error = result.unwrap_err();
            assert!(feed(&mut p, &usage(), |_| panic!()).await.is_err());
            assert!(p.eof_completion().is_err());
            error
        };
        assert_eq!(error.failure(), failure);
    }
}

#[tokio::test]
async fn final_usage_exactly_once_u64_checked_and_no_output_after_finish() {
    for bytes in [
        usage(),
        [finish("stop"), usage(), usage()].concat(),
        [finish("stop"), delta(json!({"content":"late"}))].concat(),
        [
            finish("stop"),
            usage(),
            b"data: [DONE]\n\n".to_vec(),
            delta(json!({"content":"late"})),
        ]
        .concat(),
        event(
            json!({"choices":[{"index":0,"delta":{"content":"not delivered"},"finish_reason":"unknown"}]}),
        ),
    ] {
        assert!(
            feed(&mut parser(), &bytes, |_| panic!("invalid event leaked"))
                .await
                .is_err()
        );
    }
    for bytes in [finish("stop"), b"data: [DONE]\n\n".to_vec()] {
        let mut p = parser();
        feed(&mut p, &bytes, |_| {}).await.unwrap();
        assert!(p.eof_completion().is_err());
    }
    for (input, output, total, valid) in [
        (json!(u64::MAX), json!(0), json!(u64::MAX), true),
        (json!(u64::MAX), json!(1), json!(0), false),
        (json!(-1), json!(1), json!(0), false),
        (json!(1.5), json!(1), json!(2), false),
        (json!("2"), json!(3), json!(5), false),
        (json!(2), json!(3), json!(6), false),
    ] {
        let mut p = parser();
        let chunk = json!({"choices":[{"index":0,"delta":{"content":"final"},"finish_reason":"stop"}],
            "usage":{"prompt_tokens":input,"completion_tokens":output,"total_tokens":total}});
        let mut emitted = false;
        assert_eq!(
            feed(&mut p, &event(chunk), |_| emitted = true)
                .await
                .is_ok(),
            valid
        );
        assert_eq!(emitted, valid);
        assert_eq!(p.eof_completion().is_ok(), valid);
    }
}
