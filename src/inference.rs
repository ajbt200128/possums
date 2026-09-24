use crate::catalog::{Catalog, Model, MAX_CATALOG_BYTES};
use async_trait::async_trait;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use thiserror::Error;
use tinfoil::Client;

const MAX_UPSTREAM_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Generation {
    pub content: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Error)]
pub enum InferenceError {
    #[error("verified inference unavailable")]
    Unavailable,
    #[error("authenticated upstream response is invalid")]
    InvalidResponse,
}

#[async_trait]
pub trait Inference: Send + Sync {
    async fn catalog(&self) -> Result<Vec<u8>, InferenceError>;
    async fn count_tokens(&self, model: &str, messages: &[Message]) -> Result<u64, InferenceError>;
    async fn generate(
        &self,
        model: &Model,
        messages: &[Message],
    ) -> Result<Generation, InferenceError>;
    fn verification_document(&self) -> Result<serde_json::Value, InferenceError>;
}

pub type SharedInference = Arc<dyn Inference>;

pub struct TinfoilInference {
    client: Client,
    origin: String,
}

impl TinfoilInference {
    pub async fn connect(
        host: &str,
        repository: &str,
        api_key: String,
    ) -> Result<Self, InferenceError> {
        let client = tokio::time::timeout(
            Duration::from_secs(30),
            Client::new(host, repository, api_key),
        )
        .await
        .map_err(|_| InferenceError::Unavailable)?
        .map_err(|_| InferenceError::Unavailable)?;
        if client.secure_client().verification_document().is_none() {
            return Err(InferenceError::Unavailable);
        }
        Ok(Self {
            origin: format!("https://{host}"),
            client,
        })
    }

    async fn bounded_response(
        &self,
        request: tinfoil::verifier::tls::OriginBoundRequestBuilder,
        limit: usize,
    ) -> Result<Vec<u8>, InferenceError> {
        let response = tokio::time::timeout(Duration::from_secs(300), request.send())
            .await
            .map_err(|_| InferenceError::Unavailable)?
            .map_err(|_| InferenceError::Unavailable)?;
        if !response.status().is_success() {
            return Err(InferenceError::Unavailable);
        }
        if response
            .content_length()
            .is_some_and(|length| length > limit as u64)
        {
            return Err(InferenceError::InvalidResponse);
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|_| InferenceError::Unavailable)?;
        if bytes.len() > limit {
            return Err(InferenceError::InvalidResponse);
        }
        Ok(bytes.to_vec())
    }

    fn http(&self) -> Result<&tinfoil::verifier::tls::OriginBoundClient, InferenceError> {
        self.client
            .http_client()
            .map_err(|_| InferenceError::Unavailable)
    }
}

#[derive(Deserialize)]
struct TokenCount {
    input_tokens: u64,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    usage: Usage,
}

#[derive(Deserialize)]
struct Choice {
    message: AssistantMessage,
}

#[derive(Deserialize)]
struct AssistantMessage {
    content: String,
}

#[derive(Deserialize)]
struct Usage {
    prompt_tokens: u64,
    completion_tokens: u64,
}

#[async_trait]
impl Inference for TinfoilInference {
    async fn catalog(&self) -> Result<Vec<u8>, InferenceError> {
        let request = self.http()?.get(format!("{}/v1/models", self.origin));
        self.bounded_response(request, MAX_CATALOG_BYTES).await
    }

    async fn count_tokens(&self, model: &str, messages: &[Message]) -> Result<u64, InferenceError> {
        let request = self
            .http()?
            .post(format!("{}/v1/tokenize", self.origin))
            .json(&serde_json::json!({"model": model, "messages": messages}));
        let bytes = self.bounded_response(request, 64 * 1024).await?;
        let count: TokenCount =
            serde_json::from_slice(&bytes).map_err(|_| InferenceError::InvalidResponse)?;
        if count.input_tokens == 0 {
            return Err(InferenceError::InvalidResponse);
        }
        Ok(count.input_tokens)
    }

    async fn generate(
        &self,
        model: &Model,
        messages: &[Message],
    ) -> Result<Generation, InferenceError> {
        let mut cache_scope = [0_u8; 32];
        rand::rng().fill_bytes(&mut cache_scope);
        let request = self
            .http()?
            .post(format!("{}/v1/chat/completions", self.origin))
            .json(&serde_json::json!({
                "model": model.id,
                "messages": messages,
                "max_tokens": model.max_output_tokens,
                "stream": false,
                "user_cache_secret": base64::Engine::encode(
                    &base64::engine::general_purpose::URL_SAFE_NO_PAD,
                    cache_scope
                )
            }));
        let bytes = self
            .bounded_response(request, MAX_UPSTREAM_RESPONSE_BYTES)
            .await?;
        let response: ChatResponse =
            serde_json::from_slice(&bytes).map_err(|_| InferenceError::InvalidResponse)?;
        if response.choices.len() != 1
            || response.usage.prompt_tokens == 0
            || response.usage.completion_tokens > model.max_output_tokens
        {
            return Err(InferenceError::InvalidResponse);
        }
        let choice = response
            .choices
            .into_iter()
            .next()
            .ok_or(InferenceError::InvalidResponse)?;
        Ok(Generation {
            content: choice.message.content,
            input_tokens: response.usage.prompt_tokens,
            output_tokens: response.usage.completion_tokens,
        })
    }

    fn verification_document(&self) -> Result<serde_json::Value, InferenceError> {
        let document = self
            .client
            .secure_client()
            .verification_document()
            .ok_or(InferenceError::Unavailable)?;
        serde_json::to_value(document).map_err(|_| InferenceError::InvalidResponse)
    }
}

pub async fn authenticated_catalog(
    inference: &dyn Inference,
    now_unix: u64,
) -> Result<Catalog, InferenceError> {
    let bytes = inference.catalog().await?;
    Catalog::parse_authenticated(&bytes, now_unix).map_err(|_| InferenceError::InvalidResponse)
}
