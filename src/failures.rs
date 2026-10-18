//! Closed aggregate projection of locally typed interface failures.
//! Detailed public diagnostic codes and accounting evidence stay separate.
use crate::inference::InferenceFailure;

// The foundation is deliberately withheld until boundary ownership is qualified.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum InterfaceError {
    InputRefused,
    RouteRefused,
    AuthenticationRefused,
    ConflictRefused,
    CreditRefused,
    AccountLimitRefused,
    StateCapacityRefused,
    AccountUnavailable,
    QuoteInvalid,
    QuoteOverflow,
    ModelUnavailable,
    ContextRefused,
    EvidenceUnavailable,
    EvidenceInvalid,
    VerificationFailed,
    CatalogFailed,
    RequestEncodingFailed,
    TokenizerTransportFailed,
    TokenizerHttpRefused,
    TokenizerResponseInvalid,
    TokenizerUploadIncomplete,
    GenerationTransportFailed,
    GenerationHttpRefused,
    StreamTransportFailed,
    StreamDeadlineExceeded,
    StreamIdleTimeout,
    StreamDecodeFailed,
    StreamValidationFailed,
    TerminalEvidenceMissing,
    TerminalEvidenceInvalid,
    SettlementFailed,
    HeaderProtocol,
    HeaderLimit,
    BodyLimit,
    BodyDeadline,
    ConnectionCapacity,
    ConnectionDeadline,
    AuthStateUnknown,
    RequestReadUnknown,
    RequestDispatchUnknown,
    ResponseEncodingUnknown,
    ResponseDeliveryUnknown,
    PreflightDeadline,
    PreflightUnknown,
    GenerationUnknown,
    ConnectionUnknown,
    LifecycleSetupUnknown,
    LifecycleServeUnknown,
    LifecycleCleanupUnknown,
    ConfigurationInvalid,
    AccountingStateUnknown,
    UpstreamErrorReported,
    CatalogAcquisitionUnknown,
    ResponseEncodingLimit,
}

#[allow(dead_code)]
impl InterfaceError {
    pub(crate) const ALL: [Self; 54] = [
        Self::InputRefused,
        Self::RouteRefused,
        Self::AuthenticationRefused,
        Self::ConflictRefused,
        Self::CreditRefused,
        Self::AccountLimitRefused,
        Self::StateCapacityRefused,
        Self::AccountUnavailable,
        Self::QuoteInvalid,
        Self::QuoteOverflow,
        Self::ModelUnavailable,
        Self::ContextRefused,
        Self::EvidenceUnavailable,
        Self::EvidenceInvalid,
        Self::VerificationFailed,
        Self::CatalogFailed,
        Self::RequestEncodingFailed,
        Self::TokenizerTransportFailed,
        Self::TokenizerHttpRefused,
        Self::TokenizerResponseInvalid,
        Self::TokenizerUploadIncomplete,
        Self::GenerationTransportFailed,
        Self::GenerationHttpRefused,
        Self::StreamTransportFailed,
        Self::StreamDeadlineExceeded,
        Self::StreamIdleTimeout,
        Self::StreamDecodeFailed,
        Self::StreamValidationFailed,
        Self::TerminalEvidenceMissing,
        Self::TerminalEvidenceInvalid,
        Self::SettlementFailed,
        Self::HeaderProtocol,
        Self::HeaderLimit,
        Self::BodyLimit,
        Self::BodyDeadline,
        Self::ConnectionCapacity,
        Self::ConnectionDeadline,
        Self::AuthStateUnknown,
        Self::RequestReadUnknown,
        Self::RequestDispatchUnknown,
        Self::ResponseEncodingUnknown,
        Self::ResponseDeliveryUnknown,
        Self::PreflightDeadline,
        Self::PreflightUnknown,
        Self::GenerationUnknown,
        Self::ConnectionUnknown,
        Self::LifecycleSetupUnknown,
        Self::LifecycleServeUnknown,
        Self::LifecycleCleanupUnknown,
        Self::ConfigurationInvalid,
        Self::AccountingStateUnknown,
        Self::UpstreamErrorReported,
        Self::CatalogAcquisitionUnknown,
        Self::ResponseEncodingLimit,
    ];

    pub(crate) fn stage(self) -> &'static str {
        match self {
            Self::InputRefused => "request_validation",
            Self::RouteRefused => "routing",
            Self::AuthenticationRefused | Self::AuthStateUnknown => "authentication",
            Self::ConflictRefused => "conversation",
            Self::CreditRefused
            | Self::AccountLimitRefused
            | Self::StateCapacityRefused
            | Self::AccountUnavailable
            | Self::AccountingStateUnknown => "admission_state",
            Self::QuoteInvalid
            | Self::QuoteOverflow
            | Self::ModelUnavailable
            | Self::ContextRefused => "quote_validation",
            Self::EvidenceUnavailable | Self::EvidenceInvalid | Self::VerificationFailed => {
                "verification"
            }
            Self::CatalogFailed | Self::CatalogAcquisitionUnknown => "catalog",
            Self::RequestEncodingFailed => "request_encoding",
            Self::TokenizerTransportFailed
            | Self::TokenizerHttpRefused
            | Self::TokenizerResponseInvalid
            | Self::TokenizerUploadIncomplete => "tokenizer",
            Self::GenerationTransportFailed
            | Self::GenerationHttpRefused
            | Self::GenerationUnknown => "generation",
            Self::StreamTransportFailed
            | Self::StreamDeadlineExceeded
            | Self::StreamIdleTimeout
            | Self::StreamDecodeFailed
            | Self::UpstreamErrorReported => "upstream_stream",
            Self::StreamValidationFailed => "generation_validation",
            Self::TerminalEvidenceMissing | Self::TerminalEvidenceInvalid => "terminal_evidence",
            Self::SettlementFailed => "settlement",
            Self::HeaderProtocol | Self::HeaderLimit => "request_headers",
            Self::BodyLimit | Self::BodyDeadline | Self::RequestReadUnknown => "request_body",
            Self::ConnectionCapacity | Self::ConnectionDeadline | Self::ConnectionUnknown => {
                "connection"
            }
            Self::RequestDispatchUnknown => "request_dispatch",
            Self::ResponseEncodingUnknown | Self::ResponseEncodingLimit => "response_encoding",
            Self::ResponseDeliveryUnknown => "response_delivery",
            Self::PreflightDeadline | Self::PreflightUnknown => "preflight",
            Self::LifecycleSetupUnknown | Self::ConfigurationInvalid => "startup",
            Self::LifecycleServeUnknown => "serving",
            Self::LifecycleCleanupUnknown => "cleanup",
        }
    }

    pub(crate) fn message(self) -> String {
        let action = match self {
            Self::InputRefused | Self::HeaderProtocol | Self::HeaderLimit
            | Self::BodyLimit | Self::RouteRefused => "Check the documented request method, headers, schema and transport limits.",
            Self::AuthenticationRefused => "Check the authentication session and log in deliberately if needed.",
            Self::ConflictRefused => "Use the conversation's selected model or deliberately start a new conversation.",
            Self::CreditRefused => "Check available credit against the selected model's maximum reservation.",
            Self::ContextRefused | Self::ModelUnavailable => "Check the authenticated model catalog and selected model's context window.",
            Self::EvidenceUnavailable | Self::EvidenceInvalid | Self::VerificationFailed => "Do not send further prompts until backend identity and authenticity are confirmed.",
            Self::SettlementFailed | Self::AccountingStateUnknown => "Check account reconciliation through the verified channel; do not assume settlement or refund.",
            _ => "Check service availability and share only this content-free code/stage/constraint for support.",
        };
        let unknown = matches!(
            self,
            Self::AuthStateUnknown
                | Self::AccountingStateUnknown
                | Self::RequestReadUnknown
                | Self::RequestDispatchUnknown
                | Self::ResponseEncodingUnknown
                | Self::ResponseDeliveryUnknown
                | Self::PreflightUnknown
                | Self::GenerationUnknown
                | Self::ConnectionUnknown
                | Self::LifecycleSetupUnknown
                | Self::LifecycleServeUnknown
                | Self::LifecycleCleanupUnknown
                | Self::CatalogAcquisitionUnknown
                | Self::UpstreamErrorReported
        );
        let cause = if unknown {
            "The underlying cause is unknown. "
        } else {
            ""
        };
        format!("[{}] Stage: {}; constraint: {}. {cause}{action} Not replayed. Billing/outcome require an authenticated receipt; otherwise they are unknown. A new submission may incur another charge.", self.label(), self.stage(), self.label())
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::InputRefused => "input_refused",
            Self::RouteRefused => "route_refused",
            Self::AuthenticationRefused => "authentication_refused",
            Self::ConflictRefused => "conflict_refused",
            Self::CreditRefused => "credit_refused",
            Self::AccountLimitRefused => "account_limit_refused",
            Self::StateCapacityRefused => "state_capacity_refused",
            Self::AccountUnavailable => "account_unavailable",
            Self::QuoteInvalid => "quote_invalid",
            Self::QuoteOverflow => "quote_overflow",
            Self::ModelUnavailable => "model_unavailable",
            Self::ContextRefused => "context_refused",
            Self::EvidenceUnavailable => "evidence_unavailable",
            Self::EvidenceInvalid => "evidence_invalid",
            Self::VerificationFailed => "verification_failed",
            Self::CatalogFailed => "catalog_failed",
            Self::RequestEncodingFailed => "request_encoding_failed",
            Self::TokenizerTransportFailed => "tokenizer_transport_failed",
            Self::TokenizerHttpRefused => "tokenizer_http_refused",
            Self::TokenizerResponseInvalid => "tokenizer_response_invalid",
            Self::TokenizerUploadIncomplete => "tokenizer_upload_incomplete",
            Self::GenerationTransportFailed => "generation_transport_failed",
            Self::GenerationHttpRefused => "generation_http_refused",
            Self::StreamTransportFailed => "stream_transport_failed",
            Self::StreamDeadlineExceeded => "stream_deadline_exceeded",
            Self::StreamIdleTimeout => "stream_idle_timeout",
            Self::StreamDecodeFailed => "stream_decode_failed",
            Self::StreamValidationFailed => "stream_validation_failed",
            Self::TerminalEvidenceMissing => "terminal_evidence_missing",
            Self::TerminalEvidenceInvalid => "terminal_evidence_invalid",
            Self::SettlementFailed => "settlement_failed",
            Self::HeaderProtocol => "header_protocol",
            Self::HeaderLimit => "header_limit",
            Self::BodyLimit => "body_limit",
            Self::BodyDeadline => "body_deadline",
            Self::ConnectionCapacity => "connection_capacity",
            Self::ConnectionDeadline => "connection_deadline",
            Self::AuthStateUnknown => "auth_state_unknown",
            Self::RequestReadUnknown => "request_read_unknown",
            Self::RequestDispatchUnknown => "request_dispatch_unknown",
            Self::ResponseEncodingUnknown => "response_encoding_unknown",
            Self::ResponseDeliveryUnknown => "response_delivery_unknown",
            Self::PreflightDeadline => "preflight_deadline",
            Self::PreflightUnknown => "preflight_unknown",
            Self::GenerationUnknown => "generation_unknown",
            Self::ConnectionUnknown => "connection_unknown",
            Self::LifecycleSetupUnknown => "lifecycle_setup_unknown",
            Self::LifecycleServeUnknown => "lifecycle_serve_unknown",
            Self::LifecycleCleanupUnknown => "lifecycle_cleanup_unknown",
            Self::ConfigurationInvalid => "configuration_invalid",
            Self::AccountingStateUnknown => "accounting_state_unknown",
            Self::UpstreamErrorReported => "upstream_error_reported",
            Self::CatalogAcquisitionUnknown => "catalog_acquisition_unknown",
            Self::ResponseEncodingLimit => "response_encoding_limit",
        }
    }
}

impl From<crate::inference::CatalogFailure> for InterfaceError {
    fn from(failure: crate::inference::CatalogFailure) -> Self {
        use crate::inference::CatalogFailure as C;
        match failure {
            C::Legacy(InferenceFailure::InferenceUnavailable) | C::SendUnknown | C::BodyUnknown => {
                Self::CatalogAcquisitionUnknown
            }
            C::Legacy(InferenceFailure::UpstreamResponseInvalid) => Self::CatalogFailed,
            C::Legacy(failure) => failure.into(),
            C::Validation(_)
            | C::SendDeadline
            | C::HttpRefused(_)
            | C::BodyDeadline
            | C::BodyLimit => Self::CatalogFailed,
        }
    }
}

impl From<InferenceFailure> for InterfaceError {
    fn from(failure: InferenceFailure) -> Self {
        use InferenceFailure::*;
        match failure {
            InferenceUnavailable => Self::GenerationUnknown,
            VerificationFailed | EndpointBindingFailed => Self::VerificationFailed,
            CatalogFailed => Self::CatalogFailed,
            RequestEncodingFailed => Self::RequestEncodingFailed,
            TokenizerSendFailed => Self::TokenizerTransportFailed,
            TokenizerHttpFailed => Self::TokenizerHttpRefused,
            TokenizerResponseInvalid => Self::TokenizerResponseInvalid,
            TokenizerUploadIncomplete => Self::TokenizerUploadIncomplete,
            GenerationSendFailed => Self::GenerationTransportFailed,
            GenerationHttpFailed => Self::GenerationHttpRefused,
            StreamTransportFailed => Self::StreamTransportFailed,
            StreamIdleTimeout => Self::StreamIdleTimeout,
            StreamDeadlineExceeded => Self::StreamDeadlineExceeded,
            SdkStreamDecodeFailed => Self::StreamDecodeFailed,
            UpstreamErrorEvent => Self::UpstreamErrorReported,
            StreamFinishMissing | StreamUsageMissing => Self::TerminalEvidenceMissing,
            StreamUsageInvalid | StreamUsageUnexpected => Self::TerminalEvidenceInvalid,
            SettlementFailed => Self::SettlementFailed,
            UpstreamResponseInvalid
            | ToolProfileUnqualified
            | StreamContentTypeInvalid
            | StreamEventSchemaInvalid
            | StreamChoiceInvalid
            | StreamDeltaUnsupported
            | ToolIndexInvalid
            | ToolIdentityInvalid
            | ToolNameNotAllowed
            | ToolChoiceViolated
            | ToolCallIncomplete
            | ToolStateTooLarge
            | StreamFinishInvalid
            | StreamOutputAfterFinish => Self::StreamValidationFailed,
        }
    }
}

impl From<crate::catalog::CatalogError> for InterfaceError {
    fn from(failure: crate::catalog::CatalogError) -> Self {
        use crate::catalog::CatalogError::*;
        match failure {
            Invalid => Self::QuoteInvalid,
            UnknownModel => Self::ModelUnavailable,
            ContextExceeded => Self::ContextRefused,
            Overflow => Self::QuoteOverflow,
        }
    }
}

impl From<crate::attestation::EvidenceError> for InterfaceError {
    fn from(failure: crate::attestation::EvidenceError) -> Self {
        match failure {
            crate::attestation::EvidenceError::Unavailable => Self::EvidenceUnavailable,
            crate::attestation::EvidenceError::Invalid => Self::EvidenceInvalid,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_closed_unique_and_indexed() {
        let mut labels = std::collections::HashSet::new();
        for (index, failure) in InterfaceError::ALL.into_iter().enumerate() {
            assert_eq!(failure as usize, index);
            assert!(labels.insert(failure.label()));
            assert!(!failure.stage().is_empty());
            assert!(failure.message().contains(failure.label()));
            assert!(failure.message().contains("Not replayed"));
            assert!(failure.message().len() < 1024);
            assert!(failure
                .label()
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b == b'_'));
        }
        assert_eq!(labels.len(), InterfaceError::ALL.len());
    }

    #[test]
    fn legacy_catalog_validation_is_not_a_generation_stream_failure() {
        let source = crate::inference::CatalogFailure::from(
            crate::inference::InferenceError::InvalidResponse,
        );
        let category: InterfaceError = source.into();
        assert_eq!(category, InterfaceError::CatalogFailed);
        assert_eq!(category.stage(), "catalog");
        assert_eq!(source.stage(), "catalog_validation");
        assert_eq!(source.wire(), InferenceFailure::UpstreamResponseInvalid);
    }

    #[test]
    fn inference_projection_keeps_known_stages_and_does_not_guess_causes() {
        for (failure, category) in [
            (
                InferenceFailure::VerificationFailed,
                InterfaceError::VerificationFailed,
            ),
            (
                InferenceFailure::CatalogFailed,
                InterfaceError::CatalogFailed,
            ),
            (
                InferenceFailure::RequestEncodingFailed,
                InterfaceError::RequestEncodingFailed,
            ),
            (
                InferenceFailure::TokenizerSendFailed,
                InterfaceError::TokenizerTransportFailed,
            ),
            (
                InferenceFailure::TokenizerHttpFailed,
                InterfaceError::TokenizerHttpRefused,
            ),
            (
                InferenceFailure::GenerationHttpFailed,
                InterfaceError::GenerationHttpRefused,
            ),
            (
                InferenceFailure::SdkStreamDecodeFailed,
                InterfaceError::StreamDecodeFailed,
            ),
            (
                InferenceFailure::StreamUsageMissing,
                InterfaceError::TerminalEvidenceMissing,
            ),
            (
                InferenceFailure::StreamUsageInvalid,
                InterfaceError::TerminalEvidenceInvalid,
            ),
            (
                InferenceFailure::SettlementFailed,
                InterfaceError::SettlementFailed,
            ),
            (
                InferenceFailure::InferenceUnavailable,
                InterfaceError::GenerationUnknown,
            ),
            (
                InferenceFailure::UpstreamErrorEvent,
                InterfaceError::UpstreamErrorReported,
            ),
        ] {
            assert_eq!(InterfaceError::from(failure), category);
        }
    }
}
