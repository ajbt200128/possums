use serde::{Deserialize, Serialize};
use serde_json::Number;
use std::collections::HashSet;
use thiserror::Error;

pub const MAX_CATALOG_BYTES: usize = 256 * 1024;
pub const MAX_MODELS: usize = 256;
const MAX_DISPLAY_NAME_CHARS: usize = 128;
const PRICE_SCALE: u128 = 1_000_000;
const MARKUP_PERCENT: u128 = 130;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Model {
    pub id: String,
    /// Authenticated upstream display name, already bounded and sanitized. It is
    /// presentation-only and never authorizes availability, pricing or tools.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_params: Option<ReasoningParams>,
    pub context_tokens: u64,
    pub max_output_tokens: u64,
    pub input_microunits_per_million_tokens: u64,
    pub output_microunits_per_million_tokens: u64,
    #[serde(default)]
    pub cached_input_microunits_per_million_tokens: Option<u64>,
}

/// Only the supported chat-completions vocabulary; unknown catalog shapes do
/// not prevent ordinary inference and never authorize arbitrary passthrough.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReasoningParams {
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub effort_map: Option<std::collections::BTreeMap<String, String>>,
    pub params: std::collections::BTreeMap<String, ReasoningTemplates>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReasoningTemplates {
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub enable: Option<ReasoningOptions>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub disable: Option<ReasoningOptions>,
}

#[derive(Clone, Default, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReasoningOptions {
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub reasoning_effort: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub chat_template_kwargs: Option<std::collections::BTreeMap<String, serde_json::Value>>,
}

fn non_null<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    decoder: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(decoder).map(Some)
}

/// An unexpected display-name type must not invalidate the whole authenticated
/// catalog: it is treated as absent and consumers fall back to the model id.
fn optional_name<'de, D: serde::Deserializer<'de>>(decoder: D) -> Result<Option<String>, D::Error> {
    Ok(match serde_json::Value::deserialize(decoder)? {
        serde_json::Value::String(value) => Some(value),
        _ => None,
    })
}

fn effort(value: &str) -> bool {
    matches!(
        value,
        "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max"
    )
}
impl ReasoningOptions {
    pub fn is_empty(&self) -> bool {
        self.reasoning_effort.is_none() && self.chat_template_kwargs.is_none()
    }
    fn supported(&self, placeholder: bool) -> bool {
        let valid_effort = |value: &str| effort(value) || (placeholder && value == "$EFFORT");
        !self.is_empty()
            && self.reasoning_effort.as_deref().is_none_or(valid_effort)
            && self.chat_template_kwargs.as_ref().is_none_or(|kwargs| {
                !kwargs.is_empty()
                    && kwargs.iter().all(|(key, value)| match key.as_str() {
                        "thinking" | "enable_thinking" => value.is_boolean(),
                        "reasoning_effort" => value.as_str().is_some_and(valid_effort),
                        _ => false,
                    })
            })
    }
    pub fn resolve(&self, value: Option<&str>) -> Option<Self> {
        let mut resolved = self.clone();
        if resolved.reasoning_effort.as_deref() == Some("$EFFORT") {
            resolved.reasoning_effort = Some(value?.to_owned());
        }
        if let Some(kwargs) = &mut resolved.chat_template_kwargs {
            if kwargs
                .get("reasoning_effort")
                .and_then(serde_json::Value::as_str)
                == Some("$EFFORT")
            {
                kwargs.insert("reasoning_effort".into(), serde_json::json!(value?));
            }
        }
        Some(resolved)
    }
    pub fn apply(
        &self,
        mut builder: tinfoil::relaxed::RelaxedChatRequestBuilder,
    ) -> tinfoil::relaxed::RelaxedChatRequestBuilder {
        if let Some(value) = &self.reasoning_effort {
            builder = builder.set("reasoning_effort", serde_json::json!(value));
        }
        if let Some(value) = &self.chat_template_kwargs {
            builder = builder.set("chat_template_kwargs", serde_json::json!(value));
        }
        builder
    }
}
impl ReasoningParams {
    fn supported(mut value: serde_json::Value) -> Option<Self> {
        // Other endpoints are outside this gateway's request contract.
        let params = value.get_mut("params")?.as_object_mut()?;
        params.retain(|key, _| key == "/v1/chat/completions");
        let parsed: Self = serde_json::from_value(value).ok()?;
        if parsed.params.len() != 1
            || parsed.effort_map.as_ref().is_some_and(|map| {
                map.is_empty()
                    || map.iter().any(|(key, value)| {
                        !matches!(key.as_str(), "low" | "medium" | "high") || !effort(value)
                    })
            })
        {
            return None;
        }
        let templates = parsed.params.get("/v1/chat/completions")?;
        if templates.enable.is_none() && templates.disable.is_none() {
            return None;
        }
        if templates
            .enable
            .as_ref()
            .is_some_and(|v| !v.supported(true))
            || templates
                .disable
                .as_ref()
                .is_some_and(|v| !v.supported(false))
        {
            return None;
        }
        Some(parsed)
    }
    pub fn allows(&self, options: &ReasoningOptions) -> bool {
        if !options.supported(false) {
            return false;
        }
        let Some(templates) = self.params.get("/v1/chat/completions") else {
            return false;
        };
        [templates.enable.as_ref(), templates.disable.as_ref()]
            .into_iter()
            .flatten()
            .any(|template| {
                template.resolve(None).as_ref() == Some(options)
                    || self.effort_map.as_ref().is_some_and(|map| {
                        map.values()
                            .any(|effort| template.resolve(Some(effort)).as_ref() == Some(options))
                    })
            })
    }
}
impl Model {
    pub fn allows_reasoning(&self, options: &ReasoningOptions) -> bool {
        options.is_empty()
            || (self.reasoning == Some(true)
                && self
                    .reasoning_params
                    .as_ref()
                    .is_some_and(|params| params.allows(options)))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Catalog {
    pub models: Vec<Model>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Quote {
    pub model: Model,
    pub input_tokens: u64,
    pub reserved_microunits: u64,
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum CatalogError {
    #[error("catalog is malformed or outside limits")]
    Invalid,
    #[error("unknown model")]
    UnknownModel,
    #[error("conversation exceeds model context")]
    ContextExceeded,
    #[error("cost cannot be represented")]
    Overflow,
}

#[derive(Deserialize)]
struct TinfoilCatalog {
    object: String,
    data: Vec<TinfoilModel>,
}

#[derive(Deserialize)]
struct TinfoilModel {
    id: String,
    #[serde(default, deserialize_with = "optional_name")]
    name: Option<String>,
    #[serde(rename = "type")]
    kind: String,
    context_window: Option<u64>,
    #[serde(default)]
    endpoints: Vec<String>,
    pricing: TinfoilPricing,
    reasoning: Option<serde_json::Value>,
    reasoning_params: Option<serde_json::Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TinfoilPricing {
    #[serde(rename = "inputTokenPricePer1M")]
    input: Option<Number>,
    #[serde(rename = "outputTokenPricePer1M")]
    output: Option<Number>,
    #[serde(rename = "cachedInputTokenPricePer1M")]
    cached_input: Option<Number>,
    #[serde(rename = "requestPrice")]
    request: Option<Number>,
}

impl Catalog {
    pub fn parse_authenticated(bytes: &[u8], _now_unix: u64) -> Result<Self, CatalogError> {
        if bytes.is_empty() || bytes.len() > MAX_CATALOG_BYTES {
            return Err(CatalogError::Invalid);
        }
        let upstream: TinfoilCatalog =
            serde_json::from_slice(bytes).map_err(|_| CatalogError::Invalid)?;
        if upstream.object != "list" || upstream.data.len() > MAX_MODELS {
            return Err(CatalogError::Invalid);
        }

        let mut ids = HashSet::new();
        let mut models = Vec::new();
        for upstream_model in upstream.data {
            if upstream_model.kind != "chat"
                || !upstream_model
                    .endpoints
                    .iter()
                    .any(|endpoint| endpoint == "/v1/chat/completions")
            {
                continue;
            }
            let context_tokens = upstream_model.context_window.ok_or(CatalogError::Invalid)?;
            let input_price = upstream_model.pricing.input.ok_or(CatalogError::Invalid)?;
            let output_price = upstream_model.pricing.output.ok_or(CatalogError::Invalid)?;
            let request_price = upstream_model
                .pricing
                .request
                .ok_or(CatalogError::Invalid)?;
            if scaled_amount(&request_price)? != 0 {
                return Err(CatalogError::Invalid);
            }
            let input_price = scaled_price(&input_price)?;
            let cached_input = upstream_model
                .pricing
                .cached_input
                .as_ref()
                .map(scaled_amount)
                .transpose()?;
            if cached_input.is_some_and(|cached| cached > input_price) {
                return Err(CatalogError::Invalid);
            }
            // A missing or malformed display name must not affect availability or
            // trust: it is dropped and consumers fall back to the model id.
            let public_name = upstream_model.name.filter(|name| valid_display_name(name));
            let model = Model {
                id: upstream_model.id,
                public_name,
                reasoning: upstream_model.reasoning.and_then(|value| value.as_bool()),
                reasoning_params: upstream_model
                    .reasoning_params
                    .and_then(ReasoningParams::supported),
                context_tokens,
                // No separate output bound is advertised by this catalog schema.
                // M=C is an operational assumption, not a proven billing bound.
                max_output_tokens: context_tokens,
                input_microunits_per_million_tokens: input_price,
                output_microunits_per_million_tokens: scaled_price(&output_price)?,
                cached_input_microunits_per_million_tokens: cached_input,
            };
            if !valid_model(&model) || !ids.insert(model.id.clone()) {
                return Err(CatalogError::Invalid);
            }
            models.push(model);
        }
        if models.is_empty() {
            return Err(CatalogError::Invalid);
        }
        Ok(Self { models })
    }

    pub fn reservation_quote(&self, model_id: &str) -> Result<Quote, CatalogError> {
        let model = self
            .models
            .iter()
            .find(|model| model.id == model_id)
            .ok_or(CatalogError::UnknownModel)?
            .clone();
        if !valid_model(&model) {
            return Err(CatalogError::Invalid);
        }
        let reserved_microunits = marked_up_cost(
            model.context_tokens,
            model.max_output_tokens,
            model.input_microunits_per_million_tokens,
            model.output_microunits_per_million_tokens,
        )?;
        Ok(Quote {
            input_tokens: model.context_tokens,
            model,
            reserved_microunits,
        })
    }

    pub fn quote(&self, model_id: &str, input_tokens: u64) -> Result<Quote, CatalogError> {
        let mut model = self
            .models
            .iter()
            .find(|model| model.id == model_id)
            .ok_or(CatalogError::UnknownModel)?
            .clone();
        if !valid_model(&model) {
            return Err(CatalogError::Invalid);
        }
        let remaining_context = model
            .context_tokens
            .checked_sub(input_tokens)
            .filter(|remaining| *remaining > 0)
            .ok_or(CatalogError::ContextExceeded)?;
        model.max_output_tokens = model.max_output_tokens.min(remaining_context);
        let reserved_microunits = marked_up_cost(
            input_tokens,
            model.max_output_tokens,
            model.input_microunits_per_million_tokens,
            model.output_microunits_per_million_tokens,
        )?;
        Ok(Quote {
            model,
            input_tokens,
            reserved_microunits,
        })
    }
}

pub fn actual_cost(quote: &Quote, output_tokens: u64) -> Result<u64, CatalogError> {
    if output_tokens > quote.model.max_output_tokens {
        return Err(CatalogError::Invalid);
    }
    marked_up_cost(
        quote.input_tokens,
        output_tokens,
        quote.model.input_microunits_per_million_tokens,
        quote.model.output_microunits_per_million_tokens,
    )
}

/// Display names are untrusted presentation text. Reject empty, oversized or
/// control/escape/bidi-format strings so they cannot spoof terminal output or
/// silently reorder surrounding text; invalid names fall back to the model id.
fn valid_display_name(value: &str) -> bool {
    let mut count = 0;
    for character in value.chars() {
        count += 1;
        if count > MAX_DISPLAY_NAME_CHARS
            || character.is_control()
            || matches!(
                character,
                '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
            )
        {
            return false;
        }
    }
    count > 0
}

fn valid_model(model: &Model) -> bool {
    !model.id.is_empty()
        && model.id.len() <= 128
        && model
            .id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-:/".contains(&byte))
        && model.context_tokens > 0
        && model.max_output_tokens > 0
        && model.max_output_tokens <= model.context_tokens
        && model.input_microunits_per_million_tokens > 0
        && model.output_microunits_per_million_tokens > 0
        && model
            .cached_input_microunits_per_million_tokens
            .is_none_or(|cached| cached <= model.input_microunits_per_million_tokens)
}

pub(crate) fn marked_up_cost(
    input_tokens: u64,
    output_tokens: u64,
    input_price: u64,
    output_price: u64,
) -> Result<u64, CatalogError> {
    cached_cost(
        input_tokens,
        output_tokens,
        0,
        input_price,
        output_price,
        input_price,
    )
}

pub(crate) fn cached_cost(
    input_tokens: u64,
    output_tokens: u64,
    cached_tokens: u64,
    input_price: u64,
    output_price: u64,
    cached_price: u64,
) -> Result<u64, CatalogError> {
    let uncached = input_tokens
        .checked_sub(cached_tokens)
        .ok_or(CatalogError::Invalid)?;
    let cached = u128::from(cached_tokens)
        .checked_mul(u128::from(cached_price))
        .ok_or(CatalogError::Overflow)?;
    let input = u128::from(uncached)
        .checked_mul(u128::from(input_price))
        .ok_or(CatalogError::Overflow)?;
    let output = u128::from(output_tokens)
        .checked_mul(u128::from(output_price))
        .ok_or(CatalogError::Overflow)?;
    let numerator = input
        .checked_add(cached)
        .and_then(|value| value.checked_add(output))
        .and_then(|value| value.checked_mul(MARKUP_PERCENT))
        .ok_or(CatalogError::Overflow)?;
    let denominator = PRICE_SCALE * 100;
    let rounded = numerator
        .checked_add(denominator - 1)
        .ok_or(CatalogError::Overflow)?
        / denominator;
    u64::try_from(rounded).map_err(|_| CatalogError::Overflow)
}

fn scaled_price(number: &Number) -> Result<u64, CatalogError> {
    let scaled = scaled_amount(number)?;
    if scaled == 0 {
        return Err(CatalogError::Invalid);
    }
    Ok(scaled)
}

fn scaled_amount(number: &Number) -> Result<u64, CatalogError> {
    let text = number.to_string();
    let (mantissa, exponent) =
        text.split_once(['e', 'E'])
            .map_or((text.as_str(), 0_i32), |(mantissa, exponent)| {
                exponent
                    .parse::<i32>()
                    .map(|value| (mantissa, value))
                    .unwrap_or((mantissa, i32::MIN))
            });
    if exponent == i32::MIN || mantissa.starts_with('-') {
        return Err(CatalogError::Invalid);
    }
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(CatalogError::Invalid);
    }
    let digits = format!("{whole}{fraction}")
        .parse::<u128>()
        .map_err(|_| CatalogError::Overflow)?;
    let decimal_shift = exponent
        .checked_sub(i32::try_from(fraction.len()).map_err(|_| CatalogError::Overflow)?)
        .and_then(|value| value.checked_add(6))
        .ok_or(CatalogError::Overflow)?;
    let scaled = if decimal_shift >= 0 {
        digits
            .checked_mul(
                10_u128
                    .checked_pow(decimal_shift as u32)
                    .ok_or(CatalogError::Overflow)?,
            )
            .ok_or(CatalogError::Overflow)?
    } else {
        let divisor = 10_u128
            .checked_pow(decimal_shift.unsigned_abs())
            .ok_or(CatalogError::Overflow)?;
        digits
            .checked_add(divisor - 1)
            .ok_or(CatalogError::Overflow)?
            / divisor
    };
    u64::try_from(scaled).map_err(|_| CatalogError::Overflow)
}
