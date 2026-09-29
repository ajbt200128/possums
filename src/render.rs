use crate::{catalog::Model, inference::Message};
use pulldown_cmark::{html, Options, Parser};
use std::collections::HashSet;

pub fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

pub fn markdown(value: &str) -> String {
    let mut rendered = String::new();
    html::push_html(
        &mut rendered,
        Parser::new_ext(value, Options::ENABLE_STRIKETHROUGH),
    );
    let tags: HashSet<&str> = [
        "p",
        "br",
        "strong",
        "em",
        "del",
        "blockquote",
        "pre",
        "code",
        "ul",
        "ol",
        "li",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
    ]
    .into_iter()
    .collect();
    ammonia::Builder::default()
        .tags(tags)
        .url_schemes(HashSet::new())
        .clean(&rendered)
        .to_string()
}

pub fn page(body: &str) -> String {
    format!(
        "<!doctype html><html lang=en><head><meta charset=utf-8><meta name=viewport content=\"width=device-width,initial-scale=1\"><title>Possums</title></head><body><main>{body}</main></body></html>"
    )
}

pub fn login_page(login_challenge: &str, error: Option<&str>) -> String {
    let error = error
        .map(|message| format!("<p role=alert>{}</p>", escape(message)))
        .unwrap_or_default();
    page(&format!(
        "<h1>Possums demo</h1>{error}<form method=post action=/login><input type=hidden name=csrf value=\"{}\"><label>Recovery credential <input name=credential type=password required autocomplete=current-password></label><button type=submit>Log in</button></form><p><a href=/claims>Claims and limitations</a></p>",
        escape(login_challenge)
    ))
}

pub fn chat_page(
    models: &[Model],
    csrf: &str,
    submission_token: &str,
    history: &[Message],
    selected_model: Option<&str>,
    notice: Option<&str>,
    limit: usize,
) -> Option<String> {
    // JSON and HTML escaping can expand hostile input substantially. Reject it
    // before constructing any rendering intermediates.
    let input_bytes = models
        .iter()
        .try_fold(0_usize, |total, model| total.checked_add(model.id.len()))?
        .checked_add(csrf.len())?
        .checked_add(submission_token.len())?
        .checked_add(selected_model.map_or(0, str::len))?
        .checked_add(history.iter().try_fold(0_usize, |total, message| {
            total
                .checked_add(message.role.len())?
                .checked_add(message.content.len())
        })?)?
        .checked_add(notice.map_or(0, str::len))?;
    if input_bytes.checked_mul(128)?.checked_add(4096)? > limit {
        return None;
    }
    if selected_model.is_some_and(|selected| !models.iter().any(|model| model.id == selected)) {
        return None;
    }
    let options = models
        .iter()
        .map(|model| {
            let selected = if selected_model == Some(model.id.as_str()) {
                " selected"
            } else {
                ""
            };
            format!(
                "<option value=\"{}\"{selected}>{}</option>",
                escape(&model.id),
                escape(&model.id)
            )
        })
        .collect::<String>();
    let model_field = selected_model.map_or_else(
        || format!("<select name=model>{options}</select>"),
        |selected| {
            format!(
                "<select disabled>{options}</select><input type=hidden name=model value=\"{}\">",
                escape(selected)
            )
        },
    );
    let transcript = history
        .iter()
        .map(|message| {
            format!(
                "<article><h2>{}</h2><div>{}</div></article>",
                escape(&message.role),
                if message.role == "assistant" {
                    markdown(&message.content)
                } else {
                    format!("<p>{}</p>", escape(&message.content))
                }
            )
        })
        .collect::<String>();
    let encoded_history = escape(&serde_json::to_string(history).unwrap_or_else(|_| "[]".into()));
    let notice = notice
        .map(|value| format!("<p role=status>{}</p>", escape(value)))
        .unwrap_or_default();
    let rendered = page(&format!(
        "<h1>Possums demo</h1>{notice}{transcript}<form method=post action=/chat><input type=hidden name=csrf value=\"{}\"><input type=hidden name=token value=\"{}\"><input type=hidden name=history value=\"{}\"><label>Model {model_field}</label><label>Message <textarea name=prompt required></textarea></label><button type=submit>Send</button></form><form method=post action=/chat/new><input type=hidden name=csrf value=\"{}\"><button type=submit>New chat</button></form><nav><a href=/recovery>Recovery credential</a> <a href=/claims>Claims</a></nav><form method=post action=/logout><input type=hidden name=csrf value=\"{}\"><button type=submit>Log out</button></form>",
        escape(csrf),
        escape(submission_token),
        encoded_history,
        escape(csrf),
        escape(csrf)
    ));
    (rendered.len() <= limit).then_some(rendered)
}

/// Additive continuation format; production ChatForm remains buffered for now.
/// All limits are transport defenses, not model output or product history caps.
pub const HISTORY_BLOCK_BYTES: usize = 4096;
pub const MAX_MODEL_FIELD_BYTES: usize = 256;
pub const TOKEN_FIELD_BYTES: usize = 43;
/// Space for at least 1024 next-prompt UTF-8 bytes, even if each byte is a LF
/// normalized to CRLF and then percent encoded. Larger prompts may not fit.
pub const MIN_NEXT_PROMPT_BYTES: usize = 1024;
// Manifest: 1.NNNNNN.LLLLLLLL. First field has no separator; each subsequent
// field includes '&'. Model percent expansion is <=3 (no CR/LF/NUL/non-ASCII).
const MANIFEST_BYTES: usize = 17;
const FIXED_FORM_WIRE_BYTES: usize = 5
    + TOKEN_FIELD_BYTES
    + 7
    + TOKEN_FIELD_BYTES
    + 7
    + 3 * MAX_MODEL_FIELD_BYTES
    + 8
    + 6 * MIN_NEXT_PROMPT_BYTES
    + 18
    + MANIFEST_BYTES;
const FULL_BLOCK_WIRE_BYTES: usize = 9 + HISTORY_BLOCK_BYTES.div_ceil(3) * 4 - 2;
pub const MAX_HISTORY_FIELDS: usize =
    (crate::web::BODY_LIMIT - FIXED_FORM_WIRE_BYTES).div_ceil(FULL_BLOCK_WIRE_BYTES);

/// Conservative predicted URL-encoded next form size, including metadata,
/// separators, fixed fields and the minimum prompt allowance. Checked even
/// though current constants fit usize. No generated content is retained here.
pub fn continuation_wire_bytes(decoded: usize) -> Option<usize> {
    let full = decoded / HISTORY_BLOCK_BYTES;
    let partial = decoded % HISTORY_BLOCK_BYTES;
    let partial_wire = if partial == 0 {
        0
    } else {
        9_usize.checked_add(partial.checked_mul(4)?.div_ceil(3))?
    };
    FIXED_FORM_WIRE_BYTES
        .checked_add(full.checked_mul(FULL_BLOCK_WIRE_BYTES)?)?
        .checked_add(partial_wire)
}

pub fn max_history_decoded_bytes() -> usize {
    let mut low = 0;
    let mut high = crate::web::BODY_LIMIT;
    while low < high {
        let mid = low + (high - low).div_ceil(2);
        if continuation_wire_bytes(mid).is_some_and(|n| n <= crate::web::BODY_LIMIT) {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    low
}

pub fn valid_form_token(value: &str) -> bool {
    value.len() == TOKEN_FIELD_BYTES
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

pub fn valid_form_model(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_MODEL_FIELD_BYTES
        && value.bytes().all(|b| b.is_ascii_graphic())
}

pub fn valid_history(history: &[Message]) -> bool {
    history.len().is_multiple_of(2)
        && history.iter().enumerate().all(|(index, message)| {
            if index.is_multiple_of(2) {
                message.role == "user" && !message.content.is_empty()
            } else {
                message.role == "assistant"
            }
        })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderOutcome {
    Ready,
    TransportLimit,
    DeliveryFailed,
    UpstreamFailed,
    ContinuationUnavailable,
    InvalidInput,
}

/// A synchronous, nonblocking sink must copy into bounded delivery storage or
/// return Err. Neither this renderer nor its caller may collect a full answer.
pub type RenderSinkResult = Result<(), RenderOutcome>;

pub struct IncrementalRenderer<'a> {
    history: &'a [Message],
    prompt: &'a str,
    next_history: usize,
    stage: StartupStage,
    block: [u8; HISTORY_BLOCK_BYTES],
    used: usize,
    decoded: usize,
    blocks: usize,
    state: RenderOutcome,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StartupStage {
    History,
    Streaming,
}

impl<'a> IncrementalRenderer<'a> {
    /// Compatibility wrapper for callers that can drain the sink synchronously.
    pub fn open(
        history: &'a [Message],
        prompt: &'a str,
        csrf: &str,
        model: &str,
        mut sink: impl FnMut(&str) -> RenderSinkResult,
    ) -> Result<Self, RenderOutcome> {
        let mut renderer = Self::start(history, prompt, csrf, model, &mut sink)?;
        while renderer.emit_next_history(&mut sink)? {}
        renderer.finish_start(&mut sink)?;
        Ok(renderer)
    }

    /// Validate the accepted conversation before emitting any startup HTML.
    /// The history and prompt are borrowed, never cloned into renderer storage.
    pub fn start(
        history: &'a [Message],
        prompt: &'a str,
        csrf: &str,
        model: &str,
        mut sink: impl FnMut(&str) -> RenderSinkResult,
    ) -> Result<Self, RenderOutcome> {
        if !valid_history(history)
            || prompt.is_empty()
            || !valid_form_token(csrf)
            || !valid_form_model(model)
        {
            return Err(RenderOutcome::InvalidInput);
        }
        let mut renderer = Self {
            history,
            prompt,
            next_history: 0,
            stage: StartupStage::History,
            block: [0; HISTORY_BLOCK_BYTES],
            used: 0,
            decoded: 0,
            blocks: 0,
            state: RenderOutcome::Ready,
        };
        renderer.emit("<!doctype html><html lang=en><head><meta charset=utf-8><title>Possums</title></head><body><main>", &mut sink);
        // New chat submits only CSRF, never the potentially exhausted history.
        renderer.emit(&format!("<form id=new-chat method=post action=/chat/new><input type=hidden name=csrf value=\"{csrf}\"></form><form method=post action=/chat>"), &mut sink);
        renderer.emit(&format!("<input type=hidden name=csrf value=\"{}\"><input type=hidden name=model value=\"{}\">", escape(csrf), escape(model)), &mut sink);
        renderer.history_bytes(b"[", &mut sink);
        Ok(renderer)
    }

    /// Emit one borrowed prior message. Each visible and hidden sink call is
    /// bounded, so a blocking worker can drain between calls even for a 4 MiB
    /// message. Returns false once all prior messages have been emitted.
    pub fn emit_next_history(
        &mut self,
        mut sink: impl FnMut(&str) -> RenderSinkResult,
    ) -> Result<bool, RenderOutcome> {
        if self.stage != StartupStage::History {
            return Err(RenderOutcome::InvalidInput);
        }
        let Some(message) = self.history.get(self.next_history) else {
            return Ok(false);
        };
        self.emit("<pre>", &mut sink);
        self.visible(&message.content, &mut sink);
        self.emit("</pre>", &mut sink);
        self.message(&message.role, &message.content, &mut sink);
        self.history_bytes(b",", &mut sink);
        self.next_history += 1;
        Ok(true)
    }

    /// Finish the prompt and assistant opening only after all history was sent.
    pub fn finish_start(
        &mut self,
        mut sink: impl FnMut(&str) -> RenderSinkResult,
    ) -> Result<(), RenderOutcome> {
        if self.stage != StartupStage::History || self.next_history != self.history.len() {
            return Err(RenderOutcome::InvalidInput);
        }
        self.emit("<pre>", &mut sink);
        self.visible(self.prompt, &mut sink);
        self.emit("</pre><pre aria-label=\"Assistant\">", &mut sink);
        self.message("user", self.prompt, &mut sink);
        self.history_bytes(b",{\"role\":\"assistant\",\"content\":\"", &mut sink);
        self.stage = StartupStage::Streaming;
        Ok(())
    }

    /// A limit disables continuation encoding, not visible output or inference.
    /// Delivery failure disables all output; the worker must still consume upstream.
    pub fn delta(&mut self, content: &str, mut sink: impl FnMut(&str) -> RenderSinkResult) {
        if self.stage != StartupStage::Streaming {
            return;
        }
        self.json_content(content, &mut sink);
        self.visible(content, &mut sink);
    }

    pub fn outcome(&self) -> RenderOutcome {
        self.state
    }

    /// Retained renderer/coalescer byte storage only, not RSS, incoming history,
    /// transient escaped/encoded blocks, parser JSON trees or delivery queues.
    pub fn retained_buffer_capacity(&self) -> usize {
        self.block.len()
    }

    /// Invoke only with the authenticated adapter's terminal result, after EOF
    /// and settlement. The callback MUST call issue_submission_for with the
    /// original accepted conversation/model; it is never called on failure.
    /// Settlement deliberately lives outside this API. UI failure cannot refund.
    pub fn complete(
        mut self,
        upstream: Result<crate::inference::stream::StreamUsage, crate::inference::InferenceError>,
        issue_original_continuation: impl FnOnce() -> Option<String>,
        mut sink: impl FnMut(&str) -> RenderSinkResult,
    ) -> RenderOutcome {
        if self.stage != StartupStage::Streaming {
            return RenderOutcome::InvalidInput;
        }
        self.emit("</pre>", &mut sink);
        if upstream.is_err() {
            self.notice(
                "Generation failed; no continuation is available.",
                &mut sink,
            );
            return RenderOutcome::UpstreamFailed;
        }
        // The three closing bytes were reserved during every append.
        if self.state == RenderOutcome::Ready {
            self.write_history(b"\"}]", 0, &mut sink);
            self.flush_block(&mut sink);
        }
        if self.state != RenderOutcome::Ready {
            let outcome = self.state;
            self.notice(
                "Continuation exceeds a transport limit or delivery failed. Start a New chat.",
                &mut sink,
            );
            return outcome;
        }
        let Some(token) = issue_original_continuation().filter(|v| valid_form_token(v)) else {
            self.notice("Conversation changed; start a New chat.", &mut sink);
            return RenderOutcome::ContinuationUnavailable;
        };
        self.emit(&format!("<input type=hidden name=token value=\"{token}\"><input type=hidden name=history_manifest value=\"1.{:06}.{:08}\"><label>Message <textarea name=prompt required></textarea></label><button type=submit>Send</button></form><button type=submit form=new-chat>New chat</button></main></body></html>", self.blocks, self.decoded), &mut sink);
        self.state
    }

    fn notice(&mut self, message: &str, sink: &mut impl FnMut(&str) -> RenderSinkResult) {
        self.emit(&format!("</form><p role=status>{message}</p><button type=submit form=new-chat>New chat</button></main></body></html>"), sink);
    }

    fn emit(&mut self, html: &str, sink: &mut impl FnMut(&str) -> RenderSinkResult) {
        if self.state != RenderOutcome::DeliveryFailed && sink(html).is_err() {
            self.state = RenderOutcome::DeliveryFailed;
            self.used = 0;
        }
    }

    fn visible(&mut self, content: &str, sink: &mut impl FnMut(&str) -> RenderSinkResult) {
        // Chunk at UTF-8 boundaries before allocating escaped HTML (<= 6 KiB).
        let mut rest = content;
        while !rest.is_empty() && self.state != RenderOutcome::DeliveryFailed {
            let mut end = rest.len().min(1024);
            while !rest.is_char_boundary(end) {
                end -= 1;
            }
            self.emit(&escape(&rest[..end]), sink);
            rest = &rest[end..];
        }
    }

    fn message(
        &mut self,
        role: &str,
        content: &str,
        sink: &mut impl FnMut(&str) -> RenderSinkResult,
    ) {
        self.history_bytes(b"{\"role\":\"", sink);
        self.history_bytes(role.as_bytes(), sink);
        self.history_bytes(b"\",\"content\":\"", sink);
        self.json_content(content, sink);
        self.history_bytes(b"\"}", sink);
    }

    fn json_content(&mut self, content: &str, sink: &mut impl FnMut(&str) -> RenderSinkResult) {
        for character in content.chars() {
            if self.state != RenderOutcome::Ready {
                break;
            }
            let mut utf8 = [0; 4];
            let mut control = *b"\\u0000";
            let bytes = match character {
                '"' => b"\\\"".as_slice(),
                '\\' => b"\\\\",
                '\n' => b"\\n",
                '\r' => b"\\r",
                '\t' => b"\\t",
                '\u{08}' => b"\\b",
                '\u{0c}' => b"\\f",
                c if c <= '\u{1f}' => {
                    let byte = c as u8;
                    control[4] = b"0123456789abcdef"[(byte >> 4) as usize];
                    control[5] = b"0123456789abcdef"[(byte & 15) as usize];
                    &control
                }
                c => c.encode_utf8(&mut utf8).as_bytes(),
            };
            self.history_bytes(bytes, sink);
        }
    }

    fn history_bytes(&mut self, bytes: &[u8], sink: &mut impl FnMut(&str) -> RenderSinkResult) {
        self.write_history(bytes, 3, sink);
    }

    fn write_history(
        &mut self,
        mut bytes: &[u8],
        tail: usize,
        sink: &mut impl FnMut(&str) -> RenderSinkResult,
    ) {
        if self.state != RenderOutcome::Ready {
            return;
        }
        let Some(next) = self.decoded.checked_add(bytes.len()) else {
            self.state = RenderOutcome::TransportLimit;
            return;
        };
        if next
            .checked_add(tail)
            .and_then(continuation_wire_bytes)
            .is_none_or(|n| n > crate::web::BODY_LIMIT)
        {
            self.state = RenderOutcome::TransportLimit;
            self.used = 0;
            return;
        }
        self.decoded = next;
        while !bytes.is_empty() && self.state == RenderOutcome::Ready {
            let size = bytes.len().min(HISTORY_BLOCK_BYTES - self.used);
            self.block[self.used..self.used + size].copy_from_slice(&bytes[..size]);
            self.used += size;
            bytes = &bytes[size..];
            if self.used == HISTORY_BLOCK_BYTES {
                self.flush_block(sink);
            }
        }
    }

    fn flush_block(&mut self, sink: &mut impl FnMut(&str) -> RenderSinkResult) {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
        if self.used == 0 || self.state != RenderOutcome::Ready {
            return;
        }
        let encoded = URL_SAFE_NO_PAD.encode(&self.block[..self.used]);
        self.emit(
            &format!(
                "<input type=hidden name=h{:06} value=\"{encoded}\">",
                self.blocks
            ),
            sink,
        );
        self.blocks += 1;
        self.used = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_active_and_remote_markdown() {
        let output =
            markdown("<script>alert(1)</script> ![x](https://tracker.invalid/x) <img src=x>");
        assert!(!output.contains("script"));
        assert!(!output.contains("<img"));
        assert!(!output.contains("src="));
    }
}
