//! Closed, content-free public diagnostics. Never retain SDK/provider error text.
use serde::Serialize;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InferenceFailure {
    #[error("Verified inference is unavailable.")]
    InferenceUnavailable,
    #[error("The upstream response is invalid or incomplete.")]
    UpstreamResponseInvalid,
    #[error("Verified inference connection could not be established.")]
    VerificationFailed,
    #[error("The authenticated model catalog is unavailable or invalid.")]
    CatalogFailed,
    #[error("The inference request could not be encoded within limits.")]
    RequestEncodingFailed,
    #[error("The model has no qualified tool profile.")]
    ToolProfileUnqualified,
    #[error("The tokenizer request could not be sent or timed out.")]
    TokenizerSendFailed,
    #[error("The tokenizer returned an unsuccessful HTTP status.")]
    TokenizerHttpFailed,
    #[error("The tokenizer response is invalid or incomplete.")]
    TokenizerResponseInvalid,
    #[error("The tokenizer upload was not released before its deadline.")]
    TokenizerUploadIncomplete,
    #[error("The generation request could not be sent or timed out.")]
    GenerationSendFailed,
    #[error("Generation returned an unsuccessful HTTP status.")]
    GenerationHttpFailed,
    #[error("The response endpoint does not match the verified request.")]
    EndpointBindingFailed,
    #[error("The generation response is not an event stream.")]
    StreamContentTypeInvalid,
    #[error("The generation stream transport failed.")]
    StreamTransportFailed,
    #[error("The generation stream exceeded its idle timeout.")]
    StreamIdleTimeout,
    #[error("The generation stream exceeded its deadline.")]
    StreamDeadlineExceeded,
    #[error("The SDK could not decode the generation stream.")]
    SdkStreamDecodeFailed,
    #[error("The upstream stream reported an error.")]
    UpstreamErrorEvent,
    #[error("A stream event does not match the required schema.")]
    StreamEventSchemaInvalid,
    #[error("A stream event has an invalid choice count or index.")]
    StreamChoiceInvalid,
    #[error("A stream delta uses an unsupported role or field.")]
    StreamDeltaUnsupported,
    #[error("A tool call index is repeated, out of order or exceeds limits.")]
    ToolIndexInvalid,
    #[error("A tool call identity is invalid, repeated or changed.")]
    ToolIdentityInvalid,
    #[error("A tool call name is not allowed by this request.")]
    ToolNameNotAllowed,
    #[error("The completion violates the requested tool choice.")]
    ToolChoiceViolated,
    #[error("The stream finished with an incomplete tool call.")]
    ToolCallIncomplete,
    #[error("Tool arguments exceed the stream limit.")]
    ToolArgumentsTooLarge,
    #[error("The stream finish reason is unsupported or inconsistent.")]
    StreamFinishInvalid,
    #[error("The stream ended without a finish reason.")]
    StreamFinishMissing,
    #[error("Stream usage has invalid fields or arithmetic.")]
    StreamUsageInvalid,
    #[error("Stream usage is repeated or precedes completion.")]
    StreamUsageUnexpected,
    #[error("The stream ended without final usage.")]
    StreamUsageMissing,
    #[error("The stream contains a choice after completion.")]
    StreamOutputAfterFinish,
    #[error("Generation settlement could not be confirmed.")]
    SettlementFailed,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_codes_are_closed_and_content_free() {
        let cases = [
            (
                InferenceFailure::InferenceUnavailable,
                "inference_unavailable",
            ),
            (
                InferenceFailure::UpstreamResponseInvalid,
                "upstream_response_invalid",
            ),
            (InferenceFailure::VerificationFailed, "verification_failed"),
            (InferenceFailure::CatalogFailed, "catalog_failed"),
            (
                InferenceFailure::RequestEncodingFailed,
                "request_encoding_failed",
            ),
            (
                InferenceFailure::ToolProfileUnqualified,
                "tool_profile_unqualified",
            ),
            (
                InferenceFailure::TokenizerSendFailed,
                "tokenizer_send_failed",
            ),
            (
                InferenceFailure::TokenizerHttpFailed,
                "tokenizer_http_failed",
            ),
            (
                InferenceFailure::TokenizerResponseInvalid,
                "tokenizer_response_invalid",
            ),
            (
                InferenceFailure::TokenizerUploadIncomplete,
                "tokenizer_upload_incomplete",
            ),
            (
                InferenceFailure::GenerationSendFailed,
                "generation_send_failed",
            ),
            (
                InferenceFailure::GenerationHttpFailed,
                "generation_http_failed",
            ),
            (
                InferenceFailure::EndpointBindingFailed,
                "endpoint_binding_failed",
            ),
            (
                InferenceFailure::StreamContentTypeInvalid,
                "stream_content_type_invalid",
            ),
            (
                InferenceFailure::StreamTransportFailed,
                "stream_transport_failed",
            ),
            (InferenceFailure::StreamIdleTimeout, "stream_idle_timeout"),
            (
                InferenceFailure::StreamDeadlineExceeded,
                "stream_deadline_exceeded",
            ),
            (
                InferenceFailure::SdkStreamDecodeFailed,
                "sdk_stream_decode_failed",
            ),
            (InferenceFailure::UpstreamErrorEvent, "upstream_error_event"),
            (
                InferenceFailure::StreamEventSchemaInvalid,
                "stream_event_schema_invalid",
            ),
            (
                InferenceFailure::StreamChoiceInvalid,
                "stream_choice_invalid",
            ),
            (
                InferenceFailure::StreamDeltaUnsupported,
                "stream_delta_unsupported",
            ),
            (InferenceFailure::ToolIndexInvalid, "tool_index_invalid"),
            (
                InferenceFailure::ToolIdentityInvalid,
                "tool_identity_invalid",
            ),
            (
                InferenceFailure::ToolNameNotAllowed,
                "tool_name_not_allowed",
            ),
            (InferenceFailure::ToolChoiceViolated, "tool_choice_violated"),
            (InferenceFailure::ToolCallIncomplete, "tool_call_incomplete"),
            (
                InferenceFailure::ToolArgumentsTooLarge,
                "tool_arguments_too_large",
            ),
            (
                InferenceFailure::StreamFinishInvalid,
                "stream_finish_invalid",
            ),
            (
                InferenceFailure::StreamFinishMissing,
                "stream_finish_missing",
            ),
            (InferenceFailure::StreamUsageInvalid, "stream_usage_invalid"),
            (
                InferenceFailure::StreamUsageUnexpected,
                "stream_usage_unexpected",
            ),
            (InferenceFailure::StreamUsageMissing, "stream_usage_missing"),
            (
                InferenceFailure::StreamOutputAfterFinish,
                "stream_output_after_finish",
            ),
            (InferenceFailure::SettlementFailed, "settlement_failed"),
        ];
        assert_eq!(cases.len(), 35);
        for (failure, code) in cases {
            assert_eq!(serde_json::to_value(failure).unwrap(), code);
            let error = crate::inference::InferenceError::from(failure);
            assert_eq!(error.failure(), failure);
            assert!(!error.to_string().is_empty());
            assert!(!format!("{error:?} {error}").contains("refunded"));
        }
        assert_eq!(
            crate::inference::InferenceError::Unavailable.failure(),
            InferenceFailure::InferenceUnavailable
        );
        assert_eq!(
            crate::inference::InferenceError::InvalidResponse.failure(),
            InferenceFailure::UpstreamResponseInvalid
        );
    }
}
