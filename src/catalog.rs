use serde::{Deserialize, Serialize};
use serde_json::Number;
use std::collections::HashSet;
use thiserror::Error;

pub const MAX_CATALOG_BYTES: usize = 256 * 1024;
pub const MAX_MODELS: usize = 256;
const PRICE_SCALE: u128 = 1_000_000;
const MARKUP_PERCENT: u128 = 130;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Model {
    pub id: String,
    pub context_tokens: u64,
    pub max_output_tokens: u64,
    pub input_microunits_per_million_tokens: u64,
    pub output_microunits_per_million_tokens: u64,
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

#[derive(Debug, Error, PartialEq, Eq)]
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
    #[serde(rename = "type")]
    kind: String,
    context_window: Option<u64>,
    #[serde(default)]
    endpoints: Vec<String>,
    pricing: TinfoilPricing,
}

#[derive(Deserialize)]
struct TinfoilPricing {
    #[serde(rename = "inputTokenPricePer1M")]
    input: Option<Number>,
    #[serde(rename = "outputTokenPricePer1M")]
    output: Option<Number>,
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
            let model = Model {
                id: upstream_model.id,
                context_tokens,
                max_output_tokens: context_tokens,
                input_microunits_per_million_tokens: scaled_price(&input_price)?,
                output_microunits_per_million_tokens: scaled_price(&output_price)?,
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
        let maximum_input_cost = marked_up_cost(
            model.context_tokens,
            0,
            model.input_microunits_per_million_tokens,
            model.output_microunits_per_million_tokens,
        )?;
        let maximum_output_cost = marked_up_cost(
            0,
            model.max_output_tokens,
            model.input_microunits_per_million_tokens,
            model.output_microunits_per_million_tokens,
        )?;
        Ok(Quote {
            input_tokens: model.context_tokens,
            model,
            reserved_microunits: maximum_input_cost.max(maximum_output_cost),
        })
    }

    pub fn quote(&self, model_id: &str, input_tokens: u64) -> Result<Quote, CatalogError> {
        let mut model = self
            .models
            .iter()
            .find(|model| model.id == model_id)
            .ok_or(CatalogError::UnknownModel)?
            .clone();
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
}

fn marked_up_cost(
    input_tokens: u64,
    output_tokens: u64,
    input_price: u64,
    output_price: u64,
) -> Result<u64, CatalogError> {
    let input = u128::from(input_tokens)
        .checked_mul(u128::from(input_price))
        .ok_or(CatalogError::Overflow)?;
    let output = u128::from(output_tokens)
        .checked_mul(u128::from(output_price))
        .ok_or(CatalogError::Overflow)?;
    let numerator = input
        .checked_add(output)
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
