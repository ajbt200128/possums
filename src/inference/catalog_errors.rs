//! Catalog-source evidence only; never retain provider errors or response values.
use super::{InferenceError, InferenceFailure};
use crate::catalog::CatalogError;
use std::fmt;

/// Rich source diagnostics are internal to the gateway, not new wire details.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatalogFailure {
    Legacy(InferenceFailure),
    Validation(CatalogError),
    SendUnknown,
    SendDeadline,
    HttpRefused(u16),
    BodyUnknown,
    BodyDeadline,
    BodyLimit,
}

impl CatalogFailure {
    pub fn wire(self) -> InferenceFailure {
        match self {
            Self::Legacy(failure) => failure,
            _ => InferenceFailure::CatalogFailed,
        }
    }

    pub fn stage(self) -> &'static str {
        match self {
            Self::Legacy(InferenceFailure::VerificationFailed) => "catalog_verification",
            Self::Legacy(InferenceFailure::UpstreamResponseInvalid) | Self::Validation(_) => {
                "catalog_validation"
            }
            Self::Legacy(_) => "catalog_acquisition",
            Self::SendUnknown | Self::SendDeadline => "catalog_send",
            Self::HttpRefused(_) => "catalog_http",
            Self::BodyUnknown | Self::BodyDeadline | Self::BodyLimit => "catalog_body",
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::Legacy(InferenceFailure::InferenceUnavailable) => "inference_unavailable",
            Self::Legacy(InferenceFailure::UpstreamResponseInvalid) => "upstream_response_invalid",
            Self::Legacy(InferenceFailure::VerificationFailed) => "verification_failed",
            Self::Legacy(InferenceFailure::CatalogFailed) => "catalog_failed",
            Self::Legacy(_) => "catalog_legacy_failure",
            Self::Validation(CatalogError::Invalid) => "catalog_validation_invalid",
            Self::Validation(CatalogError::UnknownModel) => "catalog_validation_unknown_model",
            Self::Validation(CatalogError::ContextExceeded) => {
                "catalog_validation_context_exceeded"
            }
            Self::Validation(CatalogError::Overflow) => "catalog_validation_overflow",
            Self::SendUnknown => "catalog_send_unknown",
            Self::SendDeadline => "catalog_send_deadline",
            Self::HttpRefused(_) => "catalog_http_refused",
            Self::BodyUnknown => "catalog_body_unknown",
            Self::BodyDeadline => "catalog_body_deadline",
            Self::BodyLimit => "catalog_body_limit",
        }
    }

    pub fn message(self) -> String {
        let explanation: String = match self {
            Self::Legacy(InferenceFailure::InferenceUnavailable) =>
                "Catalog acquisition is unavailable; the underlying cause is unknown. Report this code if it persists.".into(),
            Self::Legacy(InferenceFailure::CatalogFailed) =>
                "Legacy catalog acquisition failed; the underlying cause is unknown and finer source evidence is unavailable. Report this code if it persists.".into(),
            Self::Legacy(failure) => format!(
                "Catalog source reported: {failure} Check the source constraint or report this code."
            ),
            Self::Validation(CatalogError::Invalid) =>
                "Catalog validation rejected malformed or out-of-limit data. Report this code to the operator.".into(),
            Self::Validation(CatalogError::UnknownModel) =>
                "Catalog validation could not find the selected model. Refresh model discovery before selecting a model.".into(),
            Self::Validation(CatalogError::ContextExceeded) =>
                "Catalog validation found the model context exceeded. Reduce the submitted context.".into(),
            Self::Validation(CatalogError::Overflow) =>
                "Catalog validation found a price or cost outside the numeric range. Report this code to the operator.".into(),
            Self::SendUnknown =>
                "Catalog send failed; the underlying cause is unknown. Report this code if it persists.".into(),
            Self::SendDeadline =>
                "Catalog send exceeded its deadline. Check service availability or report this code.".into(),
            Self::HttpRefused(status) => format!(
                "Catalog HTTP acquisition returned unsuccessful status {status}; its body was not read. Report this code and status to the operator."
            ),
            Self::BodyUnknown =>
                "Catalog body acquisition failed; the underlying cause is unknown. Report this code if it persists.".into(),
            Self::BodyDeadline =>
                "Catalog body acquisition exceeded its deadline. Check service availability or report this code.".into(),
            Self::BodyLimit =>
                "Catalog body acquisition exceeded the defensive byte limit. Report this code to the operator.".into(),
        };
        format!("[{}] Stage: {}; constraint: {}. {explanation} Not replayed. Billing/outcome unknown without an authenticated receipt. A new submission may incur another charge.", self.code(), self.stage(), self.code())
    }
}

impl From<InferenceError> for CatalogFailure {
    fn from(error: InferenceError) -> Self {
        Self::Legacy(error.failure())
    }
}

impl fmt::Display for CatalogFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for CatalogFailure {}
