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
