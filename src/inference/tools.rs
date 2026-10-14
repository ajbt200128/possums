//! Structured protocol with a shared, assumed production wire profile.
//! Catalog membership and caller-side execution validation remain separate.
use super::{InferenceError, Message};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolProfile {
    OpenAiFunctionsV1,
}
impl ToolProfile {
    pub fn protocol(self) -> &'static str {
        "openai-functions-v1"
    }
}

pub(super) fn production_profile(_model: &str) -> Option<ToolProfile> {
    // Caller opts into a common wire profile; this is not per-model compatibility
    // evidence. The authenticated catalog still gates model selection upstream.
    Some(ToolProfile::OpenAiFunctionsV1)
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Tool {
    Function { function: Function },
}
impl Tool {
    pub fn name(&self) -> &str {
        let Self::Function { function } = self;
        &function.name
    }
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Function {
    pub name: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub description: Option<String>,
    pub parameters: serde_json::Map<String, Value>,
}

#[derive(Deserialize, Serialize)]
#[serde(untagged)]
pub enum ToolChoice {
    Mode(ToolMode),
    Function(NamedTool),
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolMode {
    Auto,
    None,
    Required,
}
#[derive(Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum NamedTool {
    Function { function: FunctionName },
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FunctionName {
    pub name: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: FunctionType,
    pub function: CallArguments,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FunctionType {
    Function,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CallArguments {
    pub name: String,
    pub arguments: String,
}
impl ToolCall {
    pub fn function(&self) -> &CallArguments {
        &self.function
    }
}

// Strict admission wrappers; the supported SDK owns the upstream image value.
// No Debug/Clone: these values contain the transcript and inline attachments.
#[derive(Deserialize, Serialize)]
#[serde(untagged)]
pub enum UserContent {
    Text(String),
    Parts(Vec<UserContentPart>),
}
impl UserContent {
    fn is_empty(&self) -> bool {
        match self {
            Self::Text(text) => text.is_empty(),
            Self::Parts(parts) => parts.is_empty(),
        }
    }
}
#[derive(Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum UserContentPart {
    Text {
        text: String,
    },
    ImageUrl {
        #[serde(deserialize_with = "inline_image")]
        image_url: tinfoil::chat::ImageUrl,
    },
}
fn inline_image<'de, D: serde::Deserializer<'de>>(
    decoder: D,
) -> Result<tinfoil::chat::ImageUrl, D::Error> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct InlineImage {
        url: String,
        #[serde(default, deserialize_with = "present")]
        detail: Option<tinfoil::chat::ImageDetail>,
    }
    let image = InlineImage::deserialize(decoder)?;
    Ok(tinfoil::chat::ImageUrl {
        url: image.url,
        detail: image.detail,
    })
}
fn valid_inline_image(url: &str) -> bool {
    let Some((header, data)) = url.split_once(',') else {
        return false;
    };
    matches!(
        header,
        "data:image/jpeg;base64"
            | "data:image/png;base64"
            | "data:image/gif;base64"
            | "data:image/webp;base64"
    ) && !data.is_empty()
}

// Capability policy is deliberately separate from authenticated availability/prices.
pub(crate) fn supports_images(model: &str) -> bool {
    matches!(model, "deepseek-v4-1-flash" | "kimi-k3" | "gemma4-31b")
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "role", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolMessage {
    System {
        content: String,
    },
    User {
        content: UserContent,
    },
    Assistant {
        // Required key, permitting null only for a nonempty call batch.
        #[serde(deserialize_with = "Deserialize::deserialize")]
        content: Option<String>,
        #[serde(
            default,
            deserialize_with = "present",
            skip_serializing_if = "Option::is_none"
        )]
        tool_calls: Option<Vec<ToolCall>>,
    },
    Tool {
        content: String,
        tool_call_id: String,
    },
}
impl ToolMessage {
    pub fn is_structured(&self) -> bool {
        matches!(
            self,
            Self::Tool { .. }
                | Self::User {
                    content: UserContent::Parts(_),
                }
                | Self::Assistant {
                    tool_calls: Some(_),
                    ..
                }
        )
    }
    pub fn into_text(self) -> Result<Message, InferenceError> {
        let (role, content) = match self {
            Self::System { content } => ("system", content),
            Self::User {
                content: UserContent::Text(content),
            } => ("user", content),
            Self::Assistant {
                content: Some(content),
                tool_calls: None,
            } => ("assistant", content),
            _ => return Err(InferenceError::InvalidResponse),
        };
        Ok(Message {
            role: role.into(),
            content,
        })
    }
}

// No Debug and no Clone: this owns the sole structured transcript. Both upstream
// serializers flatten a borrow of precisely this input, without normalization.
#[derive(Serialize)]
pub struct ToolInvocation {
    messages: Vec<ToolMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<Tool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<ToolChoice>,
}
impl ToolInvocation {
    pub fn new(
        messages: Vec<ToolMessage>,
        tools: Option<Vec<Tool>>,
        tool_choice: Option<ToolChoice>,
    ) -> Result<Self, InferenceError> {
        let invocation = Self {
            messages,
            tools,
            tool_choice,
        };
        invocation.validate()?;
        Ok(invocation)
    }
    pub fn tools(&self) -> &[Tool] {
        self.tools.as_deref().unwrap_or_default()
    }
    pub fn messages(&self) -> &[ToolMessage] {
        &self.messages
    }
    pub(crate) fn has_images(&self) -> bool {
        self.messages.iter().any(|message| {
            matches!(message, ToolMessage::User { content: UserContent::Parts(parts) }
                if parts.iter().any(|part| matches!(part, UserContentPart::ImageUrl { .. })))
        })
    }
    fn validate(&self) -> Result<(), InferenceError> {
        let invalid = || InferenceError::InvalidResponse;
        if self.messages.is_empty()
            || self.tools.as_ref().is_some_and(Vec::is_empty)
            || self.tool_choice.is_some() && self.tools.is_none()
        {
            return Err(invalid());
        }
        let mut names = BTreeSet::new();
        for tool in self.tools() {
            let Tool::Function { function } = tool;
            if !valid_name(&function.name) || !names.insert(function.name.as_str()) {
                return Err(invalid());
            }
        }
        match &self.tool_choice {
            Some(ToolChoice::Function(NamedTool::Function { function }))
                if !valid_name(&function.name) =>
            {
                return Err(invalid())
            }
            _ => {}
        }
        let mut ids = BTreeSet::new();
        let mut pending = BTreeSet::new();
        for message in &self.messages {
            if !pending.is_empty() && !matches!(message, ToolMessage::Tool { .. }) {
                return Err(invalid());
            }
            match message {
                ToolMessage::User {
                    content: UserContent::Parts(parts),
                } => {
                    if parts.is_empty()
                        || parts.iter().any(|part| match part {
                            UserContentPart::Text { text } => text.is_empty(),
                            UserContentPart::ImageUrl { image_url } => {
                                !valid_inline_image(&image_url.url)
                            }
                        })
                    {
                        return Err(invalid());
                    }
                }
                ToolMessage::Assistant {
                    content,
                    tool_calls,
                } => {
                    if content.is_none() && tool_calls.as_ref().is_none_or(Vec::is_empty) {
                        return Err(invalid());
                    }
                    if let Some(calls) = tool_calls {
                        if calls.is_empty() {
                            return Err(invalid());
                        }
                        for call in calls {
                            let function = call.function();
                            if !valid_id(&call.id)
                                || !ids.insert(call.id.as_str())
                                || !valid_name(&function.name)
                            {
                                return Err(invalid());
                            }
                            // Arguments are opaque transcript data within the body bound;
                            // executable-object validation belongs to the caller.
                            pending.insert(call.id.as_str());
                        }
                    }
                }
                ToolMessage::Tool { tool_call_id, .. } => {
                    if !pending.remove(tool_call_id.as_str()) {
                        return Err(invalid());
                    }
                }
                _ => {}
            }
        }
        if !pending.is_empty()
            || !matches!(self.messages.last(), Some(ToolMessage::User { content }) if !content.is_empty())
                && !matches!(self.messages.last(), Some(ToolMessage::Tool { .. }))
        {
            return Err(invalid());
        }
        Ok(())
    }
}
pub(crate) fn present<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    decoder: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(decoder).map(Some)
}

pub(crate) fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
pub(crate) fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

/// Borrowed fragments only; argument strings are opaque and never accumulated.
#[derive(Clone, Copy)]
pub enum CompletionDelta<'a> {
    Text(&'a str),
    ToolCall {
        index: usize,
        id: Option<&'a str>,
        name: Option<&'a str>,
        arguments: Option<&'a str>,
    },
}

#[derive(Serialize)]
pub(super) struct InvocationRequest<'a> {
    pub model: &'a str,
    #[serde(flatten)]
    pub invocation: &'a ToolInvocation,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inference::{invocation_tokenizer_body, stream};
    use http_body_util::BodyExt;
    use serde_json::json;
    use std::{sync::Arc, time::Duration};
    use tokio::sync::Semaphore;

    fn input(choice: Value) -> ToolInvocation {
        ToolInvocation::new(
            serde_json::from_value(json!([
                {"role":"user","content":"fixture"},
                {"role":"assistant","content":null,"tool_calls":[{"id":"prior","type":"function","function":{"name":"lookup","arguments":"{}"}}]},
                {"role":"tool","content":"fixture result","tool_call_id":"prior"}
            ])).unwrap(),
            Some(serde_json::from_value(json!([{"type":"function","function":{"name":"lookup","description":"🐾".repeat(16 * 1024 + 1),"parameters":{"type":"object"}}}])).unwrap()),
            Some(serde_json::from_value(choice).unwrap()),
        ).unwrap()
    }

    #[test]
    fn image_capabilities_are_exact_ids_and_text_wire_is_unchanged() {
        for id in ["deepseek-v4-1-flash", "kimi-k3", "gemma4-31b"] {
            assert!(supports_images(id));
        }
        for id in [
            "m",
            "deepseek-v4-1-flash-latest",
            "Kimi-k3",
            "gemma4-31b ",
            "",
        ] {
            assert!(!supports_images(id));
        }
        let wire = json!({"role":"user","content":"fixture"});
        let message: ToolMessage = serde_json::from_value(wire.clone()).unwrap();
        assert!(!message.is_structured());
        assert_eq!(serde_json::to_value(&message).unwrap(), wire);
        assert_eq!(message.into_text().unwrap().content, "fixture");
        let array: ToolMessage = serde_json::from_value(
            json!({"role":"user","content":[{"type":"text","text":"fixture"}]}),
        )
        .unwrap();
        assert!(array.is_structured());
        assert!(array.into_text().is_err());
    }

    #[tokio::test]
    async fn ordered_images_reach_official_tokenizer_and_generation_serializers_unchanged() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let messages = json!([{"role":"user","content":[
            {"type":"text","text":"before"},
            {"type":"image_url","image_url":{"url":"data:image/png;base64,YWJj"}},
            {"type":"text","text":"between"},
            {"type":"image_url","image_url":{"url":"data:image/jpeg;base64,YQ==","detail":"auto"}},
            {"type":"text","text":"after"}
        ]}]);
        let invocation = ToolInvocation::new(
            serde_json::from_value(messages.clone()).unwrap(),
            None,
            None,
        )
        .unwrap();
        let heavy: Arc<crate::telemetry::hooks::Lease> = Arc::new(
            Arc::new(Semaphore::new(1))
                .try_acquire_owned()
                .unwrap()
                .into(),
        );
        let (tokenizer, _) =
            invocation_tokenizer_body("kimi-k3", &invocation, heavy.clone()).unwrap();
        let generation = stream::builder_body(
            crate::inference::invocation_request("kimi-k3", &invocation).unwrap(),
            123,
            &crate::auth::CacheScope::default(),
            heavy,
        )
        .unwrap();
        let mut values = Vec::new();
        for body in [tokenizer, generation] {
            let mut request = reqwest::Client::new()
                .post("http://127.0.0.1/")
                .body(body)
                .build()
                .unwrap();
            assert!(request.try_clone().is_none());
            let bytes = request
                .body_mut()
                .take()
                .unwrap()
                .collect()
                .await
                .unwrap()
                .to_bytes();
            let value: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(value["messages"], messages);
            values.push(value);
        }
        assert_eq!(values[0]["messages"], values[1]["messages"]);
        assert_eq!(values[0].as_object().unwrap().len(), 2);
        assert_eq!(values[1]["max_tokens"], 123);
    }

    #[test]
    fn inline_admission_does_not_decode_or_inspect_image_payloads() {
        // Transport/parser bounds remain authoritative; the upstream decoder
        // owns base64 and image validity, including this synthetic non-image.
        for mime in ["jpeg", "png", "gif", "webp"] {
            assert!(valid_inline_image(&format!(
                "data:image/{mime};base64,not decoded"
            )));
        }
    }

    #[test]
    fn production_profile_is_shared_assumption_not_per_model_evidence() {
        // Profile assignment is independent of catalog membership; submission
        // still requires an authenticated catalog match before prompt uploads.
        for id in ["kimi-k3", "qwen3-coder", "fixture", ""] {
            assert_eq!(production_profile(id), Some(ToolProfile::OpenAiFunctionsV1));
        }
    }

    #[tokio::test]
    async fn tokenizer_and_generation_share_exact_borrowed_input_and_noncloneable_leased_data() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        for choice in [
            json!("auto"),
            json!("none"),
            json!("required"),
            json!({"type":"function","function":{"name":"lookup"}}),
        ] {
            let invocation = input(choice);
            let slots = Arc::new(Semaphore::new(1));
            let heavy: Arc<crate::telemetry::hooks::Lease> =
                Arc::new(slots.clone().try_acquire_owned().unwrap().into());
            let (tokenizer, released) =
                invocation_tokenizer_body("fixture", &invocation, heavy.clone()).unwrap();
            let request = reqwest::Client::new()
                .post("http://127.0.0.1/")
                .body(tokenizer)
                .build()
                .unwrap();
            assert!(request.try_clone().is_none());
            let tokenizer_bytes = request.body().is_some();
            assert!(tokenizer_bytes);
            // Collect DATA without networking; body and clones/slices retain admission.
            let mut request = request;
            let tokenizer = request
                .body_mut()
                .take()
                .unwrap()
                .collect()
                .await
                .unwrap()
                .to_bytes();
            let mut released = Box::pin(released.wait());
            assert!(
                tokio::time::timeout(Duration::from_millis(1), &mut released)
                    .await
                    .is_err()
            );
            let token_value: Value = serde_json::from_slice(&tokenizer).unwrap();
            let generation = stream::builder_body(
                crate::inference::invocation_request("fixture", &invocation).unwrap(),
                123,
                &crate::auth::CacheScope::default(),
                heavy.clone(),
            )
            .unwrap();
            let generation_request = reqwest::Client::new()
                .post("http://127.0.0.1/")
                .body(generation)
                .build()
                .unwrap();
            assert!(generation_request.try_clone().is_none());
            let mut generation_request = generation_request;
            let generation = generation_request
                .body_mut()
                .take()
                .unwrap()
                .collect()
                .await
                .unwrap()
                .to_bytes();
            let generation_value: Value = serde_json::from_slice(&generation).unwrap();
            for key in ["model", "messages", "tools", "tool_choice"] {
                assert_eq!(token_value[key], generation_value[key]);
            }
            assert_eq!(
                token_value["tools"][0]["function"]["description"],
                "🐾".repeat(16 * 1024 + 1)
            );
            assert_eq!(token_value.as_object().unwrap().len(), 4);
            assert_eq!(generation_value["max_tokens"], 123);
            assert_eq!(generation_value["stream_options"]["include_usage"], true);
            let retained = tokenizer.slice(..1);
            drop(tokenizer);
            assert!(
                tokio::time::timeout(Duration::from_millis(1), &mut released)
                    .await
                    .is_err()
            );
            drop(retained);
            released.await;
            drop(heavy);
            assert_eq!(slots.available_permits(), 0);
            drop(generation);
            assert_eq!(slots.available_permits(), 1);
        }
    }
}
