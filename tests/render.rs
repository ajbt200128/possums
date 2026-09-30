use possums::{
    inference::{stream, InferenceError, Message},
    render::{self, IncrementalRenderer, RenderOutcome, HISTORY_BLOCK_BYTES},
    web::{decode_continuation, BODY_LIMIT},
};
#[path = "support/stream.rs"]
mod wire;

const TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HOSTILE: &str = "\0\r\n\n\r\"'\\🐾é</pre><script>bad()</script><img src=https://invalid/>";

fn message(role: &str, content: &str) -> Message {
    Message {
        role: role.into(),
        content: content.into(),
    }
}

fn capture(output: &mut String) -> impl FnMut(&str) -> render::RenderSinkResult + '_ {
    |part| {
        assert!(part.len() <= 8192);
        assert!(output.len() + part.len() <= 20 * 1024 * 1024); // Test-only capture.
        output.push_str(part);
        Ok(())
    }
}

fn fields(html: &str) -> Vec<(String, String)> {
    html.split("<form method=post action=/chat>")
        .nth(1)
        .unwrap()
        .split("<input type=hidden name=")
        .skip(1)
        .map(|part| {
            let (name, value) = part.split_once(" value=\"").unwrap();
            (name.to_owned(), value.split('"').next().unwrap().to_owned())
        })
        .collect()
}

fn form(fields: &[(String, String)]) -> String {
    fields
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .chain(["prompt=next".into()])
        .collect::<Vec<_>>()
        .join("&")
}

fn render_answer(answer: &str, small: bool) -> (String, RenderOutcome) {
    let mut html = String::new();
    let history = [message("user", HOSTILE), message("assistant", "")];
    let mut renderer =
        IncrementalRenderer::open(&history, HOSTILE, TOKEN, "m", capture(&mut html)).unwrap();
    if small {
        for character in answer.chars() {
            renderer.delta(character.encode_utf8(&mut [0; 4]), capture(&mut html));
        }
    } else {
        renderer.delta(answer, capture(&mut html));
    }
    assert!(!html.contains("history_manifest"));
    let result = renderer.complete(Ok(usage()), || Some(TOKEN.into()), capture(&mut html));
    (html, result)
}

fn staged_answer(
    history: &[Message],
    prompt: &str,
    answer: &str,
    fail_after: Option<usize>,
    staged: bool,
) -> (String, RenderOutcome) {
    let mut html = String::new();
    let mut delivered = 0;
    let mut sink = |part: &str| {
        assert!(part.len() <= 8192);
        if fail_after.is_some_and(|limit| delivered + part.len() > limit) {
            return Err(RenderOutcome::DeliveryFailed);
        }
        delivered += part.len();
        html.push_str(part);
        Ok(())
    };
    let mut renderer = if staged {
        let mut renderer =
            IncrementalRenderer::start(history, prompt, TOKEN, "m", &mut sink).unwrap();
        while renderer.emit_next_history(&mut sink).unwrap() {}
        renderer.finish_start(&mut sink).unwrap();
        renderer
    } else {
        IncrementalRenderer::open(history, prompt, TOKEN, "m", &mut sink).unwrap()
    };
    renderer.delta(answer, &mut sink);
    let outcome = renderer.complete(Ok(usage()), || Some(TOKEN.into()), &mut sink);
    (html, outcome)
}

fn assert_pre_send_disclosure(html: &str) {
    let (before_send, _) = html.split_once("<form method=post action=/chat>").unwrap();
    for text in [
        "Before Send: An accepted generation keeps running after your browser disconnects.",
        "A successful authenticated completion may charge you even if you do not receive the answer.",
        "Submitting again may charge twice.",
        "The live authenticated Tinfoil model catalog contents are outside the gateway's attested measurement.",
    ] {
        assert_eq!(html.matches(text).count(), 1);
        assert!(before_send.contains(text));
    }
    assert!(!before_send.contains("<script"));
}

#[test]
fn empty_history_home_discloses_before_send() {
    let html = render::chat_page(
        render::CatalogSnapshot {
            quotes: &[],
            available_microunits: 0,
        },
        "csrf",
        TOKEN,
        &[],
        None,
        None,
        100_000,
    )
    .unwrap();
    assert_pre_send_disclosure(&html);
    assert_eq!(html.matches("<form method=post action=/chat>").count(), 1);
}

#[test]
fn streaming_controls_disclose_before_assistant_output() {
    let mut html = String::new();
    let _renderer =
        IncrementalRenderer::start(&[], "prompt", TOKEN, "m", capture(&mut html)).unwrap();
    assert_pre_send_disclosure(&html);
    assert!(!html.contains("prompt"));
    assert_eq!(html.matches("<form id=new-chat").count(), 1);
}

#[test]
fn buffered_chat_reset_form_is_separate_and_carries_only_escaped_csrf() {
    let csrf = "token&\"<";
    let html = render::chat_page(
        render::CatalogSnapshot {
            quotes: &[],
            available_microunits: 0,
        },
        csrf,
        TOKEN,
        &[message("user", "previous prompt")],
        None,
        None,
        100_000,
    )
    .unwrap();
    let (_, after_chat) = html.split_once("<form method=post action=/chat>").unwrap();
    let (chat, after_chat) = after_chat.split_once("</form>").unwrap();
    let (before_reset, after_reset) = after_chat
        .split_once("<form method=post action=/chat/new>")
        .unwrap();
    assert!(before_reset.is_empty());
    let (reset, after_reset) = after_reset.split_once("</form>").unwrap();
    assert_eq!(
        reset,
        "<input type=hidden name=csrf value=\"token&amp;&quot;&lt;\"><button type=submit>New chat</button>"
    );
    assert!(chat.contains("name=history"));
    assert!(chat.contains("name=token"));
    assert!(after_reset.contains("<form method=post action=/logout>"));
    assert_eq!(html.matches("<form ").count(), 3);
}

#[test]
fn streamed_success_exposes_new_chat_outside_the_continuation_form() {
    let (html, outcome) = render_answer("safe response", false);
    assert_eq!(outcome, RenderOutcome::Ready);
    let (chat, after_chat) = html.split_once("<form method=post action=/chat>").unwrap();
    assert!(chat.contains("<form id=new-chat method=post action=/chat/new>"));
    let (continuation, after_form) = after_chat.split_once("</form>").unwrap();
    assert!(continuation.contains("history_manifest"));
    assert!(!continuation.contains("New chat"));
    assert_eq!(after_form, "</main></body></html>");
    assert!(chat.contains("<button type=submit>New chat</button></form>"));
    assert!(chat.contains("<form method=post action=/logout>"));
    assert!(chat.contains("<a href=/recovery>"));
    assert!(chat.contains("Selected model:"));
    assert_eq!(html.matches("New chat</button>").count(), 1);
}

#[test]
fn selected_model_chrome_is_escaped_before_any_transcript() {
    let mut html = String::new();
    let model = "m<&\"'>";
    let _renderer =
        IncrementalRenderer::start(&[], "private", TOKEN, model, capture(&mut html)).unwrap();
    assert!(html.contains("Selected model: m&lt;&amp;&quot;&#39;&gt;"));
    assert!(!html.contains(model));
    assert!(!html.contains("private"));
    assert!(html.find("action=/logout").unwrap() < html.find("action=/chat>").unwrap());
}

#[test]
fn finished_startup_detaches_borrowed_history_without_changing_streamed_html() {
    let mut html = String::new();
    let mut renderer: IncrementalRenderer<'static> = {
        let history = vec![message("user", "first"), message("assistant", "answer")];
        let prompt = String::from("next");
        let mut renderer =
            IncrementalRenderer::start(&history, &prompt, TOKEN, "m", capture(&mut html)).unwrap();
        assert!(matches!(
            IncrementalRenderer::start(&history, &prompt, TOKEN, "m", |_| Ok(()))
                .unwrap()
                .into_streaming(),
            Err(RenderOutcome::InvalidInput)
        ));
        while renderer.emit_next_history(capture(&mut html)).unwrap() {}
        renderer.finish_start(capture(&mut html)).unwrap();
        renderer.into_streaming().unwrap()
    };
    renderer.delta("response", capture(&mut html));
    assert_eq!(
        renderer.complete(Ok(usage()), || Some(TOKEN.into()), capture(&mut html)),
        RenderOutcome::Ready
    );
    assert_eq!(
        html,
        staged_answer(
            &[message("user", "first"), message("assistant", "answer")],
            "next",
            "response",
            None,
            true
        )
        .0
    );
}

#[test]
fn staged_start_matches_open_including_failures_and_split_utf8() {
    let cases = [
        (
            vec![message("user", "hello"), message("assistant", "response")],
            "next",
            "ok",
        ),
        (
            vec![message("user", HOSTILE), message("assistant", "")],
            HOSTILE,
            HOSTILE,
        ),
        (
            vec![
                message("user", &format!("{}🐾", "a".repeat(1023))),
                message("assistant", "é"),
            ],
            "🐾é",
            "",
        ),
    ];
    for (history, prompt, answer) in cases {
        for fail_after in [None, Some(1200)] {
            assert_eq!(
                staged_answer(&history, prompt, answer, fail_after, true),
                staged_answer(&history, prompt, answer, fail_after, false)
            );
        }
    }
    let exhausted = "a".repeat(render::max_history_decoded_bytes() + 1);
    let staged = staged_answer(&[], "next", &exhausted, None, true);
    assert_eq!(staged.1, RenderOutcome::TransportLimit);
    assert_eq!(staged, staged_answer(&[], "next", &exhausted, None, false));
}

#[test]
fn startup_sequence_cannot_be_skipped_or_replayed_into_continuation() {
    let history = [message("user", "prior"), message("assistant", "")];
    let mut html = String::new();
    let mut renderer =
        IncrementalRenderer::start(&history, "next", TOKEN, "m", capture(&mut html)).unwrap();
    assert_eq!(
        renderer.finish_start(capture(&mut html)),
        Err(RenderOutcome::InvalidInput)
    );
    renderer.delta("ignored", capture(&mut html));
    assert!(renderer.emit_next_history(capture(&mut html)).unwrap());
    assert_eq!(
        renderer.finish_start(capture(&mut html)),
        Err(RenderOutcome::InvalidInput)
    );
    assert!(renderer.emit_next_history(capture(&mut html)).unwrap());
    assert!(!renderer.emit_next_history(capture(&mut html)).unwrap());
    renderer.finish_start(capture(&mut html)).unwrap();
    assert_eq!(
        renderer.finish_start(capture(&mut html)),
        Err(RenderOutcome::InvalidInput)
    );
    assert_eq!(
        renderer.emit_next_history(capture(&mut html)),
        Err(RenderOutcome::InvalidInput)
    );
    assert_eq!(
        renderer.complete(Ok(usage()), || Some(TOKEN.into()), capture(&mut html)),
        RenderOutcome::Ready
    );
    assert!(!html.contains("ignored"));
    assert!(html.contains(&format!("<form id=new-chat method=post action=/chat/new><input type=hidden name=csrf value=\"{TOKEN}\"><button type=submit>New chat</button></form>")));
    let renderer = IncrementalRenderer::start(&history, "next", TOKEN, "m", |_| Ok(())).unwrap();
    assert_eq!(
        renderer.complete(Ok(usage()), || panic!("must not issue"), |_| Ok(())),
        RenderOutcome::InvalidInput
    );
}

#[test]
fn staged_start_emits_bounded_chunks_for_simulated_drain() {
    const QUEUE: usize = 16 * 1024 * 1024;
    let history = [
        message("user", &"&".repeat(4 * 1024 * 1024)),
        message("assistant", ""),
    ];
    assert!(serde_json::to_vec(&history).unwrap().len() <= render::max_history_decoded_bytes());
    // Accounting-only sink: this is not a channel or a real consumer.
    let mut pending = 0;
    let mut drained = 0;
    let mut total = 0;
    {
        let mut sink = |part: &str| {
            assert!(part.len() <= 8192);
            if pending + part.len() > QUEUE {
                drained += pending;
                pending = 0;
            }
            pending += part.len();
            total += part.len();
            Ok(())
        };
        let mut renderer =
            IncrementalRenderer::start(&history, "next", TOKEN, "m", &mut sink).unwrap();
        while renderer.emit_next_history(&mut sink).unwrap() {}
        renderer.finish_start(&mut sink).unwrap();
        assert_eq!(renderer.outcome(), RenderOutcome::Ready);
        assert_eq!(
            renderer.complete(Ok(usage()), || Some(TOKEN.into()), &mut sink),
            RenderOutcome::Ready
        );
    }
    assert!(total > QUEUE);
    assert!(drained > 0);
    assert!(pending <= QUEUE);
}

fn usage() -> stream::StreamUsage {
    stream::StreamUsage {
        input_tokens: 1,
        output_tokens: 1,
        total_tokens: 2,
    }
}

#[test]
fn canonical_history_is_fragment_independent_and_exact_including_empty_assistant() {
    for answer in [String::new(), format!("{}{}", "a".repeat(9000), HOSTILE)] {
        let (large, status) = render_answer(&answer, false);
        let (small, _) = render_answer(&answer, true);
        assert_eq!(status, RenderOutcome::Ready);
        assert_eq!(fields(&large), fields(&small));
        assert!(!large.contains("<script>"));
        assert!(!large.contains("<img "));
        let decoded = decode_continuation(form(&fields(&large)).as_bytes()).unwrap();
        assert_eq!(decoded.history.len(), 4);
        assert_eq!(decoded.history[0].content, HOSTILE);
        assert_eq!(decoded.history[1].content, "");
        assert_eq!(decoded.history[2].content, HOSTILE);
        assert_eq!(decoded.history[3].content, answer);
        assert_eq!(decoded.model, "m");
        let canonical = serde_json::to_vec(&decoded.history).unwrap();
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
        let blocks: Vec<_> = fields(&large)
            .into_iter()
            .filter(|(k, _)| k.starts_with('h') && k != "history_manifest")
            .map(|(_, v)| URL_SAFE_NO_PAD.decode(v).unwrap())
            .collect();
        assert_eq!(blocks.concat(), canonical);
        assert!(blocks[..blocks.len() - 1]
            .iter()
            .all(|v| v.len() == HISTORY_BLOCK_BYTES));
    }
}

#[test]
fn decoder_rejects_missing_duplicate_alias_malformed_unknown_and_role_fields() {
    let (html, _) = render_answer("ok", false);
    let good = fields(&html);
    for index in 0..good.len() {
        let mut bad = good.clone();
        bad.remove(index);
        assert!(decode_continuation(form(&bad).as_bytes()).is_err());
        let mut bad = good.clone();
        bad.push(good[index].clone());
        assert!(decode_continuation(form(&bad).as_bytes()).is_err());
    }
    let original = form(&good);
    for suffix in [
        "&prompt=duplicate",
        "&unknown=x",
        "&%63srf=x",
        "&h999999=x",
        "&history_manifest=1.000001.00000001",
    ] {
        assert!(decode_continuation(format!("{original}{suffix}").as_bytes()).is_err());
    }
    for (key, value) in [
        ("h000000", "===="),
        ("h000000", "AA+_"),
        ("history_manifest", "1.1.1"),
        ("history_manifest", "2.000001.00000001"),
        ("csrf", "%zz"),
        ("model", "%00"),
        ("token", "a"),
        ("model", "%"),
        ("model", "%ff"),
    ] {
        let mut bad = good.clone();
        bad.iter_mut().find(|(k, _)| k == key).unwrap().1 = value.into();
        assert!(decode_continuation(form(&bad).as_bytes()).is_err(), "{key}");
    }
    for json in [
        r#"[{"role":"system","content":"x"}]"#,
        r#"[{"role":"user","content":"x"}]"#,
        r#"[{"role":"user","role":"user","content":"x"},{"role":"assistant","content":""}]"#,
        r#"[{"role":"user","content":"x","extra":1},{"role":"assistant","content":""}]"#,
        r#"[{"role":"assistant","content":"x"},{"role":"user","content":"x"}]"#,
        r#"[{"role":"user","content":""},{"role":"assistant","content":""}]"#,
    ] {
        assert!(decode_continuation(&json_form(json.as_bytes())).is_err());
    }
}

fn json_form(json: &[u8]) -> Vec<u8> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    let mut fields = vec![
        ("csrf".into(), TOKEN.into()),
        ("token".into(), TOKEN.into()),
        ("model".into(), "m".into()),
    ];
    for (i, block) in json.chunks(HISTORY_BLOCK_BYTES).enumerate() {
        fields.push((format!("h{i:06}"), URL_SAFE_NO_PAD.encode(block)));
    }
    fields.push((
        "history_manifest".into(),
        format!(
            "1.{:06}.{:08}",
            json.len().div_ceil(HISTORY_BLOCK_BYTES),
            json.len()
        ),
    ));
    form(&fields).into_bytes()
}

#[test]
fn exact_body_decoded_and_field_boundaries() {
    let max = render::max_history_decoded_bytes();
    assert!(render::continuation_wire_bytes(max).unwrap() <= BODY_LIMIT);
    assert!(render::continuation_wire_bytes(max + 1).unwrap() > BODY_LIMIT);
    assert_eq!(
        max.div_ceil(HISTORY_BLOCK_BYTES),
        render::MAX_HISTORY_FIELDS
    );
    assert_eq!(render::continuation_wire_bytes(usize::MAX), None);
    let shell = serde_json::to_vec(&[message("user", "x"), message("assistant", "")]).unwrap();
    for length in [
        HISTORY_BLOCK_BYTES - 1,
        HISTORY_BLOCK_BYTES,
        HISTORY_BLOCK_BYTES + 1,
        max,
        max + 1,
    ] {
        let json = serde_json::to_vec(&[
            message("user", "x"),
            message("assistant", &"a".repeat(length - shell.len())),
        ])
        .unwrap();
        assert_eq!(json.len(), length);
        let body = json_form(&json);
        assert_eq!(decode_continuation(&body).is_ok(), length <= max);
    }
    // Worst-case fixed fields plus the promised minimum next prompt exactly
    // fill the predicted form ceiling, including browser LF -> CRLF expansion.
    let maximum = serde_json::to_vec(&[
        message("user", "x"),
        message("assistant", &"a".repeat(max - shell.len())),
    ])
    .unwrap();
    let worst = String::from_utf8(json_form(&maximum))
        .unwrap()
        .replace(
            "model=m&",
            &format!("model={}&", "%25".repeat(render::MAX_MODEL_FIELD_BYTES)),
        )
        .replace(
            "prompt=next",
            &format!("prompt={}", "%0D%0A".repeat(render::MIN_NEXT_PROMPT_BYTES)),
        );
    assert_eq!(worst.len(), render::continuation_wire_bytes(max).unwrap());
    assert_eq!(worst.len(), BODY_LIMIT);
    assert!(decode_continuation(worst.as_bytes()).is_ok());
    assert!(decode_continuation(format!("{worst}x").as_bytes()).is_err());
    let mut body = json_form(&shell);
    body.extend(std::iter::repeat_n(b'a', BODY_LIMIT - body.len()));
    assert_eq!(body.len(), BODY_LIMIT);
    assert!(decode_continuation(&body).is_ok());
    body.push(b'a');
    assert!(decode_continuation(&body).is_err());
    let mut many = json_form(&shell);
    for _ in 0..render::MAX_HISTORY_FIELDS {
        many.extend_from_slice(b"&h000001=YQ");
    }
    assert!(decode_continuation(&many).is_err());
    // Noncanonical trailing base64 bits, padding and encoded aliases are rejected.
    let canonical = json_form(&shell);
    for replacement in ["%", "=", "+"] {
        let mut fields = fields(&render_answer("", false).0);
        fields
            .iter_mut()
            .find(|(k, _)| k == "h000000")
            .unwrap()
            .1
            .push_str(replacement);
        assert!(decode_continuation(form(&fields).as_bytes()).is_err());
    }
    assert!(decode_continuation(&canonical).is_ok());
}

#[test]
fn renderer_exact_ceiling_accounts_for_already_submitted_history() {
    let prior = [
        message("user", HOSTILE),
        message("assistant", &"p".repeat(8000)),
    ];
    let empty = [
        message("user", HOSTILE),
        message("assistant", &"p".repeat(8000)),
        message("user", "x"),
        message("assistant", ""),
    ];
    let available = render::max_history_decoded_bytes() - serde_json::to_vec(&empty).unwrap().len();
    for extra in [0, 1] {
        let mut html = String::new();
        let mut renderer =
            IncrementalRenderer::open(&prior, "x", TOKEN, "m", capture(&mut html)).unwrap();
        let block = "a".repeat(4096);
        let size = available + extra;
        for _ in 0..size / block.len() {
            renderer.delta(&block, capture(&mut html));
        }
        renderer.delta(&block[..size % block.len()], capture(&mut html));
        let mut issued = false;
        let result = renderer.complete(
            Ok(usage()),
            || {
                issued = true;
                Some(TOKEN.into())
            },
            capture(&mut html),
        );
        assert_eq!(issued, extra == 0);
        assert_eq!(
            result,
            if extra == 0 {
                RenderOutcome::Ready
            } else {
                RenderOutcome::TransportLimit
            }
        );
        assert_eq!(
            decode_continuation(form(&fields(&html)).as_bytes()).is_ok(),
            extra == 0
        );
    }
}

#[test]
fn chunk_order_size_and_unused_encoding_bits_are_strict() {
    let (html, _) = render_answer(&"x".repeat(9000), false);
    let good = fields(&html);
    let positions: Vec<_> = good
        .iter()
        .enumerate()
        .filter(|(_, (k, _))| k.starts_with("h0"))
        .map(|(i, _)| i)
        .collect();
    let mut bad = good.clone();
    bad.swap(positions[0], positions[1]);
    assert!(decode_continuation(form(&bad).as_bytes()).is_err());
    let mut bad = good.clone();
    bad[positions[1]].0 = "h000002".into();
    assert!(decode_continuation(form(&bad).as_bytes()).is_err());
    let mut bad = good.clone();
    bad[positions[0]].1.pop();
    assert!(decode_continuation(form(&bad).as_bytes()).is_err());
    let body = String::from_utf8(json_form(b"[]"))
        .unwrap()
        .replace("=W10", "=W11");
    assert!(decode_continuation(body.as_bytes()).is_err());
}

#[test]
fn parser_fragmentation_does_not_change_history_blocks() {
    let wire = wire::successful(&format!("{}{}", "a".repeat(9000), HOSTILE));
    let mut outputs = Vec::new();
    for fragment in [1, wire.len()] {
        let mut parser = stream::ProtocolParser::default();
        let mut html = String::new();
        let mut renderer =
            IncrementalRenderer::open(&[], HOSTILE, TOKEN, "m", capture(&mut html)).unwrap();
        for bytes in wire.chunks(fragment) {
            parser
                .feed(bytes, |delta| renderer.delta(delta, capture(&mut html)))
                .unwrap();
        }
        assert!(!html.contains("history_manifest"));
        renderer.complete(
            Ok(parser.eof().unwrap()),
            || Some(TOKEN.into()),
            capture(&mut html),
        );
        outputs.push(fields(&html));
    }
    assert_eq!(outputs[0], outputs[1]);
}

#[test]
fn failures_and_budget_exhaustion_never_issue_a_continuation() {
    for mode in 0..4 {
        let mut html = String::new();
        let mut renderer =
            IncrementalRenderer::open(&[], "x", TOKEN, "m", capture(&mut html)).unwrap();
        if mode == 1 {
            let block = "a".repeat(4096);
            for _ in 0..2048 {
                renderer.delta(&block, capture(&mut html));
            }
            assert_eq!(renderer.outcome(), RenderOutcome::TransportLimit);
        }
        if mode == 2 {
            renderer.delta("visible", |_| Err(RenderOutcome::DeliveryFailed));
        }
        let mut issued = false;
        let outcome = renderer.complete(
            if mode == 0 {
                Err(InferenceError::InvalidResponse)
            } else {
                Ok(usage())
            },
            || {
                issued = true;
                None
            },
            capture(&mut html),
        );
        assert_eq!(issued, mode == 3);
        assert_eq!(
            outcome,
            [
                RenderOutcome::UpstreamFailed,
                RenderOutcome::TransportLimit,
                RenderOutcome::DeliveryFailed,
                RenderOutcome::ContinuationUnavailable
            ][mode]
        );
        assert!(!html.contains("history_manifest"));
        assert!(!html.contains("name=token"));
        assert!(!html.contains(">Send</button>"));
        let controls = html
            .split("<form method=post action=/chat>")
            .next()
            .unwrap();
        assert!(controls.contains("<button type=submit>New chat</button></form>"));
        assert!(controls.contains("<button type=submit>Log out</button></form>"));
        assert!(controls.contains("<a href=/recovery>"));
        assert_eq!(controls.matches("name=csrf").count(), 2);
        assert!(!controls.contains("name=h000000"));
        assert!(!controls.contains("name=token"));
        assert!(decode_continuation(form(&fields(&html)).as_bytes()).is_err());
    }
}

#[test]
fn production_parser_visible_before_done_but_success_only_after_eof() {
    for ending in [0, 1, 2, 3] {
        let mut parser = stream::ProtocolParser::default();
        let mut html = String::new();
        let mut renderer =
            IncrementalRenderer::open(&[], "x", TOKEN, "m", capture(&mut html)).unwrap();
        for byte in wire::event(wire::choice(Some("visible🐾"), None)) {
            parser
                .feed(&[byte], |delta| renderer.delta(delta, capture(&mut html)))
                .unwrap();
        }
        assert!(html.contains("visible🐾"));
        assert!(!html.contains("history_manifest"));
        parser
            .feed(
                &wire::event(wire::choice(None, Some("stop"))),
                |_| unreachable!(),
            )
            .unwrap();
        parser
            .feed(&wire::event(wire::usage(1, 2)), |_| unreachable!())
            .unwrap();
        if ending != 1 {
            parser
                .feed(b"data: [DONE]\n\n", |_| unreachable!())
                .unwrap();
        }
        assert!(!html.contains("history_manifest"));
        assert!(!html.contains(">Send</button>"));
        if ending == 2 {
            assert!(parser.feed(b"data: {}\n\n", |_| {}).is_err());
        }
        if ending == 3 {
            parser.feed(b":partial", |_| {}).unwrap();
        }
        let terminal = parser.eof().map_err(|_| InferenceError::InvalidResponse);
        assert_eq!(terminal.is_ok(), ending == 0);
        let result = renderer.complete(terminal, || Some(TOKEN.into()), capture(&mut html));
        assert_eq!(result == RenderOutcome::Ready, ending == 0);
        assert_eq!(html.contains("history_manifest"), ending == 0);
    }
}

#[test]
fn completion_uses_conditional_original_conversation_issuance_without_settlement() {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use possums::{
        accounting::Accounting,
        auth::Auth,
        catalog::{Model, Quote},
    };
    use sha2::{Digest, Sha256};
    for invalidation in 0..3 {
        let credential = possums::auth::random_token();
        let hash = URL_SAFE_NO_PAD.encode(Sha256::digest(credential.as_bytes()));
        let auth = Auth::from_json(&format!(
            r#"[{{"id":"a","credential_sha256":"{hash}","demo_microunits":1000}}]"#
        ))
        .unwrap();
        let (id, session) = auth
            .authenticate(&credential, &auth.issue_login_challenge().unwrap())
            .unwrap();
        let ledger = Accounting::new(auth.account_budgets());
        let token = auth.issue_submission(&id).unwrap();
        let admission = auth
            .admit_submission(
                &ledger,
                &id,
                &session.csrf,
                &token,
                Quote {
                    model: Model {
                        id: "m".into(),
                        context_tokens: 20,
                        max_output_tokens: 10,
                        input_microunits_per_million_tokens: 1_000_000,
                        output_microunits_per_million_tokens: 1_000_000,
                    },
                    input_tokens: 10,
                    reserved_microunits: 26,
                },
            )
            .unwrap();
        let mut html = String::new();
        let renderer =
            IncrementalRenderer::open(&[], "x", &session.csrf, "m", capture(&mut html)).unwrap();
        match invalidation {
            1 => auth.new_chat(&id, &session.csrf).unwrap(),
            2 => auth.logout(&id, &session.csrf).unwrap(),
            _ => {}
        }
        let balance = ledger.available("a");
        let outcome = renderer.complete(
            Ok(usage()),
            || {
                auth.issue_submission_for(&id, admission.submission.conversation, Some("m"))
                    .ok()
            },
            capture(&mut html),
        );
        assert_eq!(outcome == RenderOutcome::Ready, invalidation == 0);
        assert_eq!(html.contains("history_manifest"), invalidation == 0);
        // The caller must settle separately; rendering cannot charge or refund.
        assert_eq!(ledger.available("a"), balance);
    }
}

#[test]
fn fixture_capture_is_resettable_capped_and_model_specific() {
    let mut capture = wire::FixtureCapture::default();
    for model in wire::FIXTURE_MODELS {
        capture
            .record(model, [("user", HOSTILE), ("assistant", "")].into_iter())
            .unwrap();
        assert_eq!(capture.get().0, Some(model));
        assert_eq!(capture.get().1[0].1, HOSTILE);
        assert!(capture.retained_bytes() <= wire::MAX_CAPTURE_BYTES);
        capture.reset();
        assert_eq!(capture.retained_bytes(), 0);
        assert!(capture.get().1.is_empty());
    }
    let content = "a".repeat(wire::MAX_CAPTURE_BYTES - std::mem::size_of::<(String, String)>() - 4);
    capture
        .record(
            wire::FIXTURE_MODELS[0],
            [("user", content.as_str())].into_iter(),
        )
        .unwrap();
    assert_eq!(capture.retained_bytes(), wire::MAX_CAPTURE_BYTES);
    assert!(capture
        .record(
            wire::FIXTURE_MODELS[0],
            [("user", format!("{content}x").as_str())].into_iter()
        )
        .is_err());
    assert_eq!(capture.retained_bytes(), 0);
    assert!(capture.get().0.is_none());
    assert!(capture
        .record("unknown", [("user", "x")].into_iter())
        .is_err());
    capture
        .record(
            wire::FIXTURE_MODELS[1],
            std::iter::repeat_n(("user", "x"), wire::MAX_CAPTURE_MESSAGES),
        )
        .unwrap();
    assert!(capture
        .record(
            wire::FIXTURE_MODELS[1],
            std::iter::repeat_n(("user", "x"), wire::MAX_CAPTURE_MESSAGES + 1)
        )
        .is_err());
    assert!(capture.get().1.is_empty());
}

#[tokio::test]
async fn bounded_fixture_controls_hold_release_finish_eof_and_fault() {
    use std::time::Duration;
    use wire::{FixtureAnswer, FixtureControl};
    let (controls, mut commands) = wire::fixture_controls();
    assert!(
        tokio::time::timeout(Duration::from_millis(10), commands.recv())
            .await
            .is_err()
    );
    controls.send(FixtureControl::Release).unwrap();
    controls.send(FixtureControl::Finish).unwrap();
    assert!(controls.send(FixtureControl::Eof).is_err());
    assert_eq!(commands.recv().await, Some(FixtureControl::Release));
    assert_eq!(commands.recv().await, Some(FixtureControl::Finish));
    drop(commands);
    assert!(controls.send(FixtureControl::Fail).is_err());

    for (answer, fail) in [
        (FixtureAnswer::Text, false),
        (FixtureAnswer::Empty, false),
        (FixtureAnswer::Text, true),
    ] {
        let (mut response, peer) = wire::raw_response().await;
        let (controls, mut commands) = wire::fixture_controls();
        let driver = tokio::spawn(async move {
            while let Some(control) = commands.recv().await {
                match control {
                    FixtureControl::Release => peer.send(&answer.release(), 1).await,
                    FixtureControl::Finish => peer.send(&answer.finish(), 32).await,
                    FixtureControl::Eof => {
                        peer.eof().await;
                        break;
                    }
                    FixtureControl::Fail => {
                        peer.fault().await;
                        break;
                    }
                }
            }
            // Wait for the raw server to consume its terminal command before
            // dropping its abort guard. The test below owns the receive deadline.
            peer.finished().await;
        });
        assert!(
            tokio::time::timeout(Duration::from_millis(10), response.chunk())
                .await
                .is_err()
        );
        controls.send(FixtureControl::Release).unwrap();
        controls.send(FixtureControl::Finish).unwrap();
        let mut parser = stream::ProtocolParser::default();
        let mut visible_bytes = 0;
        // The complete DONE frame arrives while raw transport EOF stays held.
        let mut received = 0;
        let expected = answer.release().len() + answer.finish().len();
        while received < expected {
            let bytes = tokio::time::timeout(Duration::from_secs(2), response.chunk())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            received += bytes.len();
            parser
                .feed(&bytes, |delta| visible_bytes += delta.len())
                .unwrap();
        }
        assert_eq!(
            visible_bytes,
            if matches!(answer, FixtureAnswer::Empty) {
                0
            } else {
                wire::FIXTURE_TEXT.len()
            }
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(10), response.chunk())
                .await
                .is_err()
        );
        controls
            .send(if fail {
                FixtureControl::Fail
            } else {
                FixtureControl::Eof
            })
            .unwrap();
        let terminal = tokio::time::timeout(Duration::from_secs(2), response.chunk())
            .await
            .unwrap();
        if fail {
            assert!(terminal.is_err());
        } else {
            assert!(terminal.unwrap().is_none());
            assert!(parser.eof().is_ok());
        }
        driver.await.unwrap();
    }
}

#[test]
fn long_stream_parser_and_renderer_retained_capacity_is_constant() {
    let mut parser = stream::ProtocolParser::default();
    let mut emitted = 0;
    let mut renderer =
        IncrementalRenderer::open(&[], "bounded incoming history", TOKEN, "m", |_| Ok(())).unwrap();
    let parser_capacity = parser.retained_buffer_capacity();
    let event = wire::event(wire::choice(Some(&"a".repeat(4096)), None));
    for _ in 0..8192 {
        // 32 MiB generated, never captured.
        parser
            .feed(&event, |delta| {
                renderer.delta(delta, |part| {
                    emitted += part.len();
                    Ok(())
                })
            })
            .unwrap();
        assert_eq!(renderer.retained_buffer_capacity(), 4096);
        assert_eq!(parser.retained_buffer_capacity(), parser_capacity);
    }
    assert!(emitted > 32 * 1024 * 1024);
    assert_eq!(renderer.outcome(), RenderOutcome::TransportLimit);
    parser
        .feed(&wire::event(wire::choice(None, Some("stop"))), |_| {})
        .unwrap();
    parser
        .feed(&wire::event(wire::usage(1, 2)), |_| {})
        .unwrap();
    parser.feed(b"data: [DONE]\n\n", |_| {}).unwrap();
    assert_eq!(
        renderer.complete(
            Ok(parser.eof().unwrap()),
            || panic!("must not issue"),
            |_| Ok(())
        ),
        RenderOutcome::TransportLimit
    );
}

#[test]
fn catalog_displays_exact_six_decimal_usd_without_float_rounding_or_filtering() {
    use possums::catalog::{Model, Quote};
    let quotes: Vec<_> = [1, 1_000_000, u64::MAX]
        .into_iter()
        .map(|reserved_microunits| Quote {
            model: Model {
                id: format!("m{reserved_microunits}"),
                context_tokens: 20,
                max_output_tokens: 20,
                input_microunits_per_million_tokens: 1,
                output_microunits_per_million_tokens: 1,
            },
            input_tokens: 20,
            reserved_microunits,
        })
        .collect();
    let html = render::chat_page(
        render::CatalogSnapshot {
            quotes: &quotes,
            available_microunits: 999_999,
        },
        TOKEN,
        TOKEN,
        &[],
        None,
        None,
        100_000,
    )
    .unwrap();
    for amount in ["USD 0.000001", "USD 1.000000", "USD 18446744073709.551615"] {
        assert!(html.contains(&format!(
            "indicative maximum reservation: {amount}</option>"
        )));
    }
    assert!(html.contains("Available demo credit: USD 0.999999"));
    assert_eq!(html.matches("<option ").count(), 3);
    assert!(!html.contains("disabled"));
}

#[test]
fn failed_stream_never_discloses_success_credit_or_issues_continuation() {
    let mut html = String::new();
    let renderer =
        IncrementalRenderer::open(&[], "prompt", TOKEN, "m", capture(&mut html)).unwrap();
    assert_eq!(
        renderer.complete_with_credit(
            Err(InferenceError::InvalidResponse),
            Some(render::CreditSnapshot {
                reserved_microunits: 52,
                available_microunits: 97
            }),
            || panic!("failed generation must not issue continuation"),
            capture(&mut html),
        ),
        RenderOutcome::UpstreamFailed
    );
    assert!(!html.contains("Available demo credit"));
    assert!(!html.contains("original authenticated reservation snapshot"));
    assert!(!html.contains("<button type=submit>Send"));
}
