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
    notice: Option<&str>,
    confirmation: Option<(&str, &str)>,
) -> String {
    let options = models
        .iter()
        .map(|model| {
            format!(
                "<option value=\"{}\">{}</option>",
                escape(&model.id),
                escape(&model.id)
            )
        })
        .collect::<String>();
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
    let confirmation = confirmation
        .map(|(token, model)| {
            format!(
                "<form method=post action=/confirm><input type=hidden name=csrf value=\"{}\"><input type=hidden name=token value=\"{}\"><input type=hidden name=model value=\"{}\"><button type=submit>Confirm response delivery</button></form>",
                escape(csrf),
                escape(token),
                escape(model)
            )
        })
        .unwrap_or_default();
    page(&format!(
        "<h1>Possums demo</h1>{notice}{transcript}{confirmation}<form method=post action=/chat><input type=hidden name=csrf value=\"{}\"><input type=hidden name=token value=\"{}\"><input type=hidden name=history value=\"{}\"><label>Model <select name=model>{options}</select></label><label>Message <textarea name=prompt required></textarea></label><button type=submit>Send</button></form><nav><a href=/recovery>Recovery credential</a> <a href=/claims>Claims</a></nav><form method=post action=/logout><input type=hidden name=csrf value=\"{}\"><button type=submit>Log out</button></form>",
        escape(csrf),
        escape(submission_token),
        encoded_history,
        escape(csrf)
    ))
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
