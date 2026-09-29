//! Accepted-input counterexamples, not allocation measurements or a memory proof.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use possums::{
    inference::{stream::StreamUsage, Message},
    render::{IncrementalRenderer, RenderOutcome, HISTORY_BLOCK_BYTES},
    web::{decode_continuation, BODY_LIMIT},
};

const TOKEN: &str = "ccccccccccccccccccccccccccccccccccccccccccc";

fn pair_history(pairs: usize) -> String {
    let pair = r#"{"role":"user","content":"x"},{"role":"assistant","content":""}"#;
    format!("[{}]", vec![pair; pairs].join(","))
}

fn history_form(history: &str, prompt: &str) -> Vec<u8> {
    let blocks: Vec<_> = history.as_bytes().chunks(HISTORY_BLOCK_BYTES).collect();
    let mut body = format!(
        "csrf={TOKEN}&token={TOKEN}&model=m&prompt={prompt}&history_manifest=1.{:06}.{:08}",
        blocks.len(),
        history.len()
    );
    for (index, block) in blocks.iter().enumerate() {
        body.push_str(&format!("&h{index:06}={}", URL_SAFE_NO_PAD.encode(block)));
    }
    body.into_bytes()
}

#[test]
fn many_tiny_messages_fit_body_limit() {
    let body = history_form(&pair_history(98_047), "x");
    // 6,275,009 decoded JSON bytes; 8,381,638 canonical form bytes.
    assert_eq!(body.len(), 8_381_638);
    assert!(body.len() <= BODY_LIMIT);
    let form = decode_continuation(&body).unwrap_or_else(|_| panic!("valid history rejected"));
    assert_eq!(form.history.len(), 196_094);
    assert!(form.history.chunks_exact(2).all(|pair| {
        pair[0].role == "user"
            && pair[0].content == "x"
            && pair[1].role == "assistant"
            && pair[1].content.is_empty()
    }));
    assert_eq!(form.prompt, "x");
}

#[test]
fn exact_body_limit_accepts_and_one_extra_byte_rejects() {
    let history = pair_history(1);
    let prefix = history_form(&history, "");
    let mut prompt = "x".repeat(BODY_LIMIT - prefix.len());
    let body = history_form(&history, &prompt);
    assert_eq!(body.len(), BODY_LIMIT);
    let form = decode_continuation(&body).unwrap_or_else(|_| panic!("limit rejected"));
    assert_eq!(form.prompt.len(), prompt.len());
    assert_eq!(form.history.len(), 2);
    prompt.push('x');
    let body = history_form(&history, &prompt);
    assert_eq!(body.len(), BODY_LIMIT + 1);
    assert!(decode_continuation(&body).is_err());
}

fn escape_history() -> Vec<Message> {
    let history = vec![
        Message {
            role: "user".into(),
            content: "<".repeat(4 * 1024 * 1024),
        },
        Message {
            role: "assistant".into(),
            content: String::new(),
        },
    ];
    let json = serde_json::to_string(&history).unwrap();
    let body = history_form(&json, "x");
    assert_eq!(body.len(), 5_602_549);
    assert!(body.len() < BODY_LIMIT);
    // Exercise the real input boundary: this is accepted, not just shaped like
    // accepted history. Drop fixture/raw allocations before rendering.
    decode_continuation(&body)
        .unwrap_or_else(|_| panic!("escape-heavy history rejected"))
        .history
}

#[test]
fn escaped_history_open_emits_more_than_sixteen_mib() {
    let history = escape_history();
    let mut emitted = 0_usize;
    let renderer = IncrementalRenderer::open(&history, "x", TOKEN, "m", |chunk| {
        emitted += chunk.len();
        Ok(())
    })
    .unwrap_or_else(|_| panic!("valid history rejected"));
    // Visible '<' alone expands from 4 MiB to 16 MiB; count, never collect, emissions.
    assert!(emitted > 16 * 1024 * 1024, "emitted {emitted} bytes");
    assert_eq!(renderer.outcome(), RenderOutcome::Ready);
    println!("escaped history open emitted {emitted} bytes");
}

#[test]
fn bounded_sink_failure_prevents_continuation() {
    let history = escape_history();
    let mut emitted = 0_usize;
    let mut renderer = IncrementalRenderer::open(&history, "x", TOKEN, "m", |chunk| {
        if chunk.len() > 16 * 1024 * 1024 - emitted {
            return Err(RenderOutcome::DeliveryFailed);
        }
        emitted += chunk.len();
        Ok(())
    })
    .unwrap_or_else(|_| panic!("valid history rejected"));
    assert_eq!(renderer.outcome(), RenderOutcome::DeliveryFailed);
    renderer.delta("later output", |_| panic!("failed sink must not be called"));
    let outcome = renderer.complete(
        Ok(StreamUsage {
            input_tokens: 1,
            output_tokens: 1,
            total_tokens: 2,
        }),
        || panic!("failed delivery must not issue continuation"),
        |_| panic!("failed sink must not be called"),
    );
    assert_eq!(outcome, RenderOutcome::DeliveryFailed);
    println!("16 MiB counting sink accepted {emitted} bytes before failure");
}
