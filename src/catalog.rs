use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use thiserror::Error;

pub const MAX_CATALOG_BYTES: usize = 256 * 1024;
pub const MAX_MODELS: usize = 256;
pub const MAX_CATALOG_AGE_SECONDS: u64 = 300;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Model {
    pub id: String,
    pub context_tokens: u64,
    pub max_output_tokens: u64,
    pub input_microunits_per_token: u64,
    pub output_microunits_per_token: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Catalog {
    pub issued_at_unix: u64,
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
    #[error("catalog is stale")]
    Stale,
    #[error("unknown model")]
    UnknownModel,
    #[error("conversation exceeds model context")]
    ContextExceeded,
    #[error("cost cannot be represented")]
    Overflow,
}

impl Catalog {
    pub fn parse_authenticated(bytes: &[u8], now_unix: u64) -> Result<Self, CatalogError> {
        if bytes.is_empty() || bytes.len() > MAX_CATALOG_BYTES {
            return Err(CatalogError::Invalid);
        }
        let catalog: Self = serde_json::from_slice(bytes).map_err(|_| CatalogError::Invalid)?;
        catalog.validate(now_unix)?;
        Ok(catalog)
    }

    pub fn validate(&self, now_unix: u64) -> Result<(), CatalogError> {
        let age = now_unix
            .checked_sub(self.issued_at_unix)
            .ok_or(CatalogError::Invalid)?;
        if age > MAX_CATALOG_AGE_SECONDS {
            return Err(CatalogError::Stale);
        }
        if self.models.is_empty() || self.models.len() > MAX_MODELS {
            return Err(CatalogError::Invalid);
        }
        let mut ids = HashSet::new();
        for model in &self.models {
            if model.id.is_empty()
                || model.id.len() > 128
                || !model
                    .id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"._-:/".contains(&byte))
                || !ids.insert(&model.id)
                || model.context_tokens == 0
                || model.max_output_tokens == 0
                || model.max_output_tokens > model.context_tokens
                || model.input_microunits_per_token == 0
                || model.output_microunits_per_token == 0
            {
                return Err(CatalogError::Invalid);
            }
        }
        Ok(())
    }

    pub fn quote(&self, model_id: &str, input_tokens: u64) -> Result<Quote, CatalogError> {
        let model = self
            .models
            .iter()
            .find(|model| model.id == model_id)
            .ok_or(CatalogError::UnknownModel)?
            .clone();
        if input_tokens
            .checked_add(model.max_output_tokens)
            .ok_or(CatalogError::Overflow)?
            > model.context_tokens
        {
            return Err(CatalogError::ContextExceeded);
        }
        let input = input_tokens
            .checked_mul(model.input_microunits_per_token)
            .ok_or(CatalogError::Overflow)?;
        let output = model
            .max_output_tokens
            .checked_mul(model.output_microunits_per_token)
            .ok_or(CatalogError::Overflow)?;
        let upstream = input.checked_add(output).ok_or(CatalogError::Overflow)?;
        let marked_up = upstream.checked_mul(130).ok_or(CatalogError::Overflow)?;
        let reserved_microunits = marked_up.checked_add(99).ok_or(CatalogError::Overflow)? / 100;
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
    let input = quote
        .input_tokens
        .checked_mul(quote.model.input_microunits_per_token)
        .ok_or(CatalogError::Overflow)?;
    let output = output_tokens
        .checked_mul(quote.model.output_microunits_per_token)
        .ok_or(CatalogError::Overflow)?;
    input
        .checked_add(output)
        .and_then(|value| value.checked_mul(130))
        .and_then(|value| value.checked_add(99))
        .map(|value| value / 100)
        .ok_or(CatalogError::Overflow)
}
