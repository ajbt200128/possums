//! Input regressions and scoped decoder allocation checks, not a memory/RSS proof.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use possums::{
    inference::{stream::StreamUsage, Message},
    render::{IncrementalRenderer, RenderOutcome, HISTORY_BLOCK_BYTES},
    web::{decode_continuation, BODY_LIMIT},
};

const TOKEN: &str = "ccccccccccccccccccccccccccccccccccccccccccc";

// Count cumulative requested bytes only during synchronous decoding on this
// thread. Parallel tests and fixture construction cannot affect the counter.
mod allocation_probe {
    use std::{
        alloc::{GlobalAlloc, Layout, System},
        cell::Cell,
    };

    thread_local! {
        static BYTES: Cell<Option<usize>> = const { Cell::new(None) };
    }

    pub struct Allocator;

    fn record(size: usize) {
        let _ = BYTES.try_with(|bytes| {
            if let Some(total) = bytes.get() {
                bytes.set(Some(total.saturating_add(size)));
            }
        });
    }

    // SAFETY: forwards unchanged pointers/layouts to System; accounting neither
    // allocates nor dereferences pointers. Realloc counts the full new request.
    unsafe impl GlobalAlloc for Allocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let pointer = unsafe { System.alloc(layout) };
            if !pointer.is_null() {
                record(layout.size());
            }
            pointer
        }

        unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
            unsafe { System.dealloc(pointer, layout) };
        }

        unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
            let pointer = unsafe { System.realloc(pointer, layout, size) };
            if !pointer.is_null() {
                record(size);
            }
            pointer
        }
    }

    pub fn measure<T>(operation: impl FnOnce() -> T) -> (T, usize) {
        BYTES.with(|bytes| bytes.set(Some(0)));
        let result = operation();
        let allocated = BYTES.with(|bytes| bytes.replace(None).unwrap());
        (result, allocated)
    }
}

#[global_allocator]
static ALLOCATOR: allocation_probe::Allocator = allocation_probe::Allocator;

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
fn dense_invalid_sequence_history_rejects_before_vector_growth() {
    // About 6 MiB decoded / 8 MiB wire. The old collect-then-validate decoder
    // grew hundreds of thousands of 48-byte entries before rejecting these.
    for prefix in [
        r#"["",""]"#,
        r#"["user",""]"#,
        r#"["user","x"],["user","x"]"#,
    ] {
        let json = format!(
            "[{prefix},{}]",
            r#"["",""],"#.repeat(749_999).trim_end_matches(',')
        );
        let body = history_form(&json, "x");
        assert!(body.len() < BODY_LIMIT);
        let (result, allocated) = allocation_probe::measure(|| decode_continuation(&body));
        assert!(result.is_err());
        // Allows decoded JSON, per-block canonical re-encoding and small parser
        // overhead, but not a vector for the invalid tail. Not a product cap or
        // a peak/RSS bound; fixtures are deliberately outside the measurement.
        assert!(
            allocated < 3 * json.len(),
            "decoder allocated {allocated} bytes"
        );
    }
}

#[test]
fn near_body_limit_valid_sequence_history_stays_accepted() {
    // Sequence-form entries are denser than object-form entries. A long valid
    // prefix must still be accepted, even though invalid entries fail early.
    const PAIRS: usize = 208_000;
    let pair = r#"["user","x"],["assistant",""]"#;
    let json = format!("[{}]", vec![pair; PAIRS].join(","));
    let body = history_form(&json, "x");
    assert!(body.len() > BODY_LIMIT - 64 * 1024 && body.len() <= BODY_LIMIT);
    let (result, allocated) = allocation_probe::measure(|| decode_continuation(&body));
    let form = result.unwrap_or_else(|_| panic!("valid sequence history rejected"));
    assert_eq!(form.history.len(), 2 * PAIRS);
    assert_eq!(form.history.first().unwrap().content, "x");
    assert_eq!(form.history.last().unwrap().role, "assistant");
    assert_eq!(form.prompt, "x");
    // Fixture-only cumulative requested bytes, not a worst-case peak or RSS.
    println!(
        "valid sequence decoded={} body={} requested={allocated}",
        json.len(),
        body.len()
    );
}

#[test]
fn sequence_history_preserves_exact_multi_turn_messages() {
    let expected = vec![
        Message {
            role: "user".into(),
            content: " first\n<&>🦝\u{0}".into(),
        },
        Message {
            role: "assistant".into(),
            content: String::new(),
        },
        Message {
            role: "user".into(),
            content: " \t ".into(),
        },
        Message {
            role: "assistant".into(),
            content: "answer \"\\\r\n".into(),
        },
    ];
    let entries: Vec<_> = expected
        .iter()
        .map(|message| [&message.role, &message.content])
        .collect();
    let json = serde_json::to_string(&entries).unwrap();
    let form = decode_continuation(&history_form(&json, "next"))
        .unwrap_or_else(|_| panic!("valid sequence history rejected"));
    assert_eq!(form.history, expected);
    assert_eq!(form.prompt, "next");
    // Struct entries can also mix sequence and object representations.
    let mixed = r#"[["user","x"],{"role":"assistant","content":""}]"#;
    assert!(decode_continuation(&history_form(mixed, "x")).is_ok());
}

#[test]
fn sequence_history_keeps_strict_schema_and_turn_validation() {
    for json in [
        r#"[["user","x"]]"#, // Odd trailing count.
        r#"[["user","x"],["system",""]]"#,
        r#"[["assistant","x"],["user","x"]]"#,
        r#"[["user"]]"#,
        r#"[["user","x","extra"],["assistant",""]]"#,
        r#"[["user",null],["assistant",""]]"#,
        r#"[{"role":"user","role":"user","content":"x"},["assistant",""]]"#,
        r#"[{"role":"user","content":"x","content":"x"},["assistant",""]]"#,
        r#"[{"role":"user","content":"x","extra":"x"},["assistant",""]]"#,
        r#"[["user","x"],["assistant",""]] []"#,
        r#"{}"#,
    ] {
        assert!(decode_continuation(&history_form(json, "x")).is_err());
    }
    assert!(decode_continuation(&history_form("[]", "x")).is_ok());
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
