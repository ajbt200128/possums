use async_trait::async_trait;
use possums::{
    catalog::Model,
    inference::{stream::StreamUsage, Inference, InferenceError, Message, SharedInference},
};
use std::sync::Arc;

struct NoStreaming;

#[async_trait]
impl Inference for NoStreaming {
    async fn catalog(&self) -> Result<Vec<u8>, InferenceError> {
        panic!("unexpected catalog call")
    }

    async fn count_tokens(
        &self,
        _: &str,
        _: &[Message],
        _heavy: std::sync::Arc<possums::telemetry::hooks::Lease>,
    ) -> Result<u64, InferenceError> {
        panic!("unexpected tokenizer call")
    }

    fn verification_document(&self) -> Result<serde_json::Value, InferenceError> {
        panic!("unexpected verification document call")
    }
}

struct StreamingMock;

const USAGE: StreamUsage = StreamUsage {
    input_tokens: 3,
    output_tokens: 2,
    total_tokens: 5,
};

#[async_trait]
impl Inference for StreamingMock {
    async fn catalog(&self) -> Result<Vec<u8>, InferenceError> {
        NoStreaming.catalog().await
    }

    async fn count_tokens(
        &self,
        model: &str,
        messages: &[Message],
        heavy: std::sync::Arc<possums::telemetry::hooks::Lease>,
    ) -> Result<u64, InferenceError> {
        NoStreaming.count_tokens(model, messages, heavy).await
    }

    async fn generate_stream(
        &self,
        model: &Model,
        messages: &[Message],
        _heavy: std::sync::Arc<possums::telemetry::hooks::Lease>,
        on_delta: &mut (dyn for<'delta> FnMut(&'delta str) + Send),
    ) -> Result<StreamUsage, InferenceError> {
        assert_eq!(model.id, "fixture");
        assert_eq!(model.max_output_tokens, 16);
        assert_eq!(messages.len(), 1);
        for delta in ["first", "second"] {
            on_delta(delta);
            tokio::task::yield_now().await;
        }
        Ok(USAGE)
    }

    fn verification_document(&self) -> Result<serde_json::Value, InferenceError> {
        NoStreaming.verification_document()
    }
}

fn request() -> (Model, [Message; 1]) {
    (
        Model {
            id: "fixture".into(),
            context_tokens: 32,
            max_output_tokens: 16,
            input_microunits_per_million_tokens: 1,
            output_microunits_per_million_tokens: 1,
        },
        [Message {
            role: "user".into(),
            content: "fixture input".into(),
        }],
    )
}

fn assert_send<T: Send>(value: T) -> T {
    value
}

#[tokio::test]
async fn trait_object_dispatch_borrows_callback_and_returns_terminal_usage() {
    let inference: SharedInference = Arc::new(StreamingMock);
    let (model, messages) = request();
    let mut deltas = Vec::new();
    let mut on_delta = |delta: &str| deltas.push(delta.to_owned());
    let usage = assert_send(inference.generate_stream(&model, &messages, heavy(), &mut on_delta))
        .await
        .unwrap();
    assert_eq!(deltas, ["first", "second"]);
    assert_eq!(usage, USAGE);
}

#[tokio::test]
async fn detached_delivery_still_consumes_to_terminal_usage() {
    let inference: SharedInference = Arc::new(StreamingMock);
    let (model, messages) = request();
    let (sender, receiver) = tokio::sync::mpsc::channel::<String>(1);
    drop(receiver);
    let mut delivery = Some(sender);
    let mut callbacks = 0;
    let mut failed_deliveries = 0;
    let usage = inference
        .generate_stream(&model, &messages, heavy(), &mut |delta| {
            callbacks += 1;
            if let Some(sender) = &delivery {
                if sender.try_send(delta.to_owned()).is_err() {
                    failed_deliveries += 1;
                    delivery = None;
                }
            }
        })
        .await
        .unwrap();
    assert_eq!(callbacks, 2);
    assert_eq!(failed_deliveries, 1);
    assert!(delivery.is_none());
    assert_eq!(usage, USAGE);
}

#[tokio::test]
async fn default_streaming_fails_closed_without_callback() {
    let inference: SharedInference = Arc::new(NoStreaming);
    let (model, messages) = request();
    let result = inference
        .generate_stream(&model, &messages, heavy(), &mut |_| {
            panic!("default streaming must not deliver deltas")
        })
        .await;
    assert!(matches!(result, Err(InferenceError::Unavailable)));
}

fn heavy() -> Arc<possums::telemetry::hooks::Lease> {
    Arc::new(
        Arc::new(tokio::sync::Semaphore::new(1))
            .try_acquire_owned()
            .unwrap()
            .into(),
    )
}

#[tokio::test]
async fn default_structured_methods_fail_closed_without_text_fallback() {
    use possums::inference::tools::{ToolInvocation, ToolMessage};
    let inference: SharedInference = Arc::new(NoStreaming);
    let (model, _) = request();
    let input = ToolInvocation::new(
        vec![ToolMessage::User {
            content: "fixture".into(),
        }],
        None,
        None,
    )
    .unwrap();
    assert!(inference.tool_profile(&model.id).is_none());
    assert!(matches!(
        inference
            .count_invocation_tokens(&model.id, &input, heavy())
            .await,
        Err(InferenceError::Unavailable)
    ));
    assert!(matches!(
        inference
            .generate_invocation_stream(&model, &input, heavy(), &mut |_| panic!(
                "default must not emit"
            ))
            .await,
        Err(InferenceError::Unavailable)
    ));
}
