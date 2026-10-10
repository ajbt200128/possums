//! Closed schema dimensions. No runtime strings are retained.
use crate::inference::{authenticated_catalog, Inference, TinfoilInference};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Endpoint {
    Attestation,
    ApiChallenge,
    ApiSession,
    ApiSubmission,
    ApiLogout,
    Models,
    ChatApi,
    Other,
}
impl Endpoint {
    /// Supply the parsed URI path, not a URL or query. Nothing is retained.
    pub fn route(method: &str, path: &str) -> Self {
        match (method, path) {
            ("GET" | "HEAD", "/attestation") => Self::Attestation,
            ("GET", "/v1/auth/challenge") => Self::ApiChallenge,
            ("POST", "/v1/sessions") => Self::ApiSession,
            ("POST", "/v1/submissions") => Self::ApiSubmission,
            ("DELETE", "/v1/sessions/current") => Self::ApiLogout,
            ("GET", "/v1/models") => Self::Models,
            ("POST", "/v1/chat/completions") => Self::ChatApi,
            _ => Self::Other,
        }
    }
    pub(super) fn chat(self) -> Option<usize> {
        match self {
            Self::ChatApi => Some(0),
            _ => None,
        }
    }
}

/// The only slot in contract v1; deployment configuration, never request input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Gateway01,
}

/// Construction requires an authenticated catalog lookup AND a representable quote.
/// The raw ID is discarded. This is not a model availability filter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QualifiedModel(pub(super) u8);
impl QualifiedModel {
    /// Standalone qualification boundary. Uses the existing verified provider client;
    /// no credentials, catalog bytes or submitted strings enter aggregation state.
    pub async fn authenticate(client: &TinfoilInference, selected: &str, now: u64) -> Option<Self> {
        qualify(client, selected, now).await
    }
}
async fn qualify(client: &dyn Inference, selected: &str, now: u64) -> Option<QualifiedModel> {
    let catalog = authenticated_catalog(client, now).await.ok()?;
    let quote = catalog.reservation_quote(selected).ok()?;
    Some(QualifiedModel::from_authenticated_quote(&quote))
}
impl QualifiedModel {
    /// Only call with the successful quote from the authenticated admission catalog.
    pub(crate) fn from_authenticated_quote(quote: &crate::catalog::Quote) -> Self {
        Self(match quote.model.id.as_str() {
            "kimi-k3" => 0,
            "glm-5-3" => 1,
            _ => 2,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdmissionModel {
    Qualified(QualifiedModel),
    Unknown,
    NotApplicable,
}
impl AdmissionModel {
    pub(super) fn index(self) -> usize {
        match self {
            Self::Qualified(m) => m.0 as usize,
            Self::Unknown => 3,
            Self::NotApplicable => 4,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Status {
    Informational,
    Success,
    Redirect,
    ClientError,
    ServerError,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum HttpTerminal {
    Eof,
    Error,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Disposition {
    NewGeneration,
    Duplicate,
    PreReservationRejected,
    ControlOrOther,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum GenerationTerminal {
    Success,
    Admission,
    Verification,
    Catalog,
    Encoding,
    Transport,
    Decoding,
    Validation,
    TerminalUsage,
    Settlement,
    Internal,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum DeliveryTerminal {
    Completed,
    Interrupted,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Rejection {
    ConnectionCapacity,
    HeaderProtocol,
    ConnectionDeadline,
    TransportUnknown,
    RequestCapacity,
    IngressCapacity,
    GenerationCapacity,
    AccountLimit,
    Credit,
    Auth,
    Input,
    Verification,
    Catalog,
    Internal,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Lane {
    Connection,
    Generation,
    Heavy,
    Ingress,
    Control,
}
pub const CAPACITIES: [u64; 5] = [64, 4, 4, 4, 1];
pub const DURATION_BOUNDS_NS: [u64; 10] = [
    100_000_000,
    500_000_000,
    1_000_000_000,
    5_000_000_000,
    15_000_000_000,
    30_000_000_000,
    60_000_000_000,
    120_000_000_000,
    300_000_000_000,
    600_000_000_000,
];
pub const OCCUPANCY_BOUNDS: [u64; 9] = [0, 1, 2, 3, 4, 8, 16, 32, 64];
pub(super) fn duration_bucket(start: u64, end: u64) -> Option<u8> {
    let elapsed = end.checked_sub(start)?;
    Some(DURATION_BOUNDS_NS.partition_point(|bound| *bound < elapsed) as u8)
}

impl From<crate::inference::InferenceFailure> for GenerationTerminal {
    fn from(failure: crate::inference::InferenceFailure) -> Self {
        use crate::inference::InferenceFailure::*;
        match failure {
            VerificationFailed | EndpointBindingFailed => Self::Verification,
            CatalogFailed => Self::Catalog,
            RequestEncodingFailed => Self::Encoding,
            TokenizerSendFailed
            | TokenizerHttpFailed
            | TokenizerUploadIncomplete
            | GenerationSendFailed
            | GenerationHttpFailed
            | StreamTransportFailed
            | StreamIdleTimeout
            | StreamDeadlineExceeded
            | UpstreamErrorEvent => Self::Transport,
            SdkStreamDecodeFailed => Self::Decoding,
            StreamFinishMissing
            | StreamUsageMissing
            | StreamUsageInvalid
            | StreamUsageUnexpected => Self::TerminalUsage,
            SettlementFailed => Self::Settlement,
            InferenceUnavailable => Self::Internal,
            ToolProfileUnqualified
            | TokenizerResponseInvalid
            | StreamContentTypeInvalid
            | StreamEventSchemaInvalid
            | StreamChoiceInvalid
            | StreamDeltaUnsupported
            | ToolIndexInvalid
            | ToolIdentityInvalid
            | ToolNameNotAllowed
            | ToolChoiceViolated
            | ToolCallIncomplete
            | ToolArgumentsTooLarge
            | StreamFinishInvalid
            | StreamOutputAfterFinish
            | UpstreamResponseInvalid => Self::Validation,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inference::InferenceError;
    struct CatalogFixture(&'static [u8]);
    #[async_trait::async_trait]
    impl Inference for CatalogFixture {
        async fn catalog(&self) -> Result<Vec<u8>, InferenceError> {
            Ok(self.0.to_vec())
        }
        async fn count_tokens(
            &self,
            _: &str,
            _: &[crate::inference::Message],
            _: std::sync::Arc<crate::telemetry::hooks::Lease>,
        ) -> Result<u64, InferenceError> {
            Err(InferenceError::Unavailable)
        }
        fn verification_document(&self) -> Result<serde_json::Value, InferenceError> {
            Err(InferenceError::Unavailable)
        }
    }
    #[tokio::test]
    async fn named_labels_require_valid_catalog_selection_and_quote() {
        let catalog = CatalogFixture(br#"{"object":"list","data":[{"id":"kimi-k3","type":"chat","context_window":100,"endpoints":["/v1/chat/completions"],"pricing":{"inputTokenPricePer1M":1,"outputTokenPricePer1M":1,"requestPrice":0}},{"id":"supported-new-model","type":"chat","context_window":100,"endpoints":["/v1/chat/completions"],"pricing":{"inputTokenPricePer1M":1,"outputTokenPricePer1M":1,"requestPrice":0}}]}"#);
        assert_eq!(
            qualify(&catalog, "kimi-k3", 0).await,
            Some(QualifiedModel(0))
        );
        assert_eq!(
            qualify(&catalog, "supported-new-model", 0).await,
            Some(QualifiedModel(2))
        );
        assert_eq!(qualify(&catalog, "glm-5-3", 0).await, None);
        assert_eq!(qualify(&catalog, "credential-canary", 0).await, None);
        assert_eq!(
            qualify(&CatalogFixture(b"hostile-error-canary"), "kimi-k3", 0).await,
            None
        );
        let overflow = CatalogFixture(br#"{"object":"list","data":[{"id":"kimi-k3","type":"chat","context_window":18446744073709551615,"endpoints":["/v1/chat/completions"],"pricing":{"inputTokenPricePer1M":18446744073709,"outputTokenPricePer1M":18446744073709,"requestPrice":0}}]}"#);
        assert_eq!(qualify(&overflow, "kimi-k3", 0).await, None);
    }
}
