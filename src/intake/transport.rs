use super::LoginMaterial;
use crate::bounded_json;
use reqwest::{header, Client, Method, StatusCode, Url};
use serde::de::DeserializeOwned;
use serde_json::json;
use std::{fmt, sync::Arc, time::Duration};

const REQUEST_LIMIT: usize = 4096;
const AUTH_LIMIT: usize = 64 * 1024;
const TERMINAL_LIMIT: usize = 16 * 1024;
const BALANCE_LIMIT: usize = 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Operation {
    Configure,
    Login,
    Refresh,
    AuthMethods,
    Reserve,
    Renew,
    Settle,
    Refund,
    Lookup,
    Balance,
}

impl Operation {
    fn path(self) -> &'static str {
        match self {
            Self::Configure => unreachable!("configuration is not an HTTP operation"),
            Self::Login => "/api/collections/accounts/auth-with-password",
            Self::Refresh => "/api/collections/accounts/auth-refresh",
            Self::AuthMethods => "/api/collections/accounts/auth-methods",
            Self::Reserve => "/reservations",
            Self::Renew => "/reservations/renew",
            Self::Settle => "/reservations/settle",
            Self::Refund => "/reservations/refund",
            Self::Lookup => "/reservations/lookup",
            Self::Balance => "/balance",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Configure => "intake_configuration",
            Self::Login => "intake_login",
            Self::Refresh => "intake_refresh",
            Self::AuthMethods => "intake_auth_methods",
            Self::Reserve => "intake_reserve",
            Self::Renew => "intake_renew",
            Self::Settle => "intake_settle",
            Self::Refund => "intake_refund",
            Self::Lookup => "intake_lookup",
            Self::Balance => "intake_balance",
        }
    }

    fn response_limit(self, status: StatusCode) -> usize {
        if !status.is_success() {
            return TERMINAL_LIMIT;
        }
        match self {
            Self::Login | Self::Refresh | Self::AuthMethods => AUTH_LIMIT,
            Self::Balance => BALANCE_LIMIT,
            _ => TERMINAL_LIMIT,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Constraint {
    Configuration,
    Input,
    EncodingLimit,
    EncodingUnknown,
    ClientUnknown,
    RequestUnknown,
    ConnectUnknown,
    TransportUnknown,
    Deadline,
    ResponseEncoding,
    ResponseLimit,
    ResponseReadUnknown,
    ResponseJson,
}

impl Constraint {
    fn label(self) -> &'static str {
        match self {
            Self::Configuration => "https_origin_or_runtime_key",
            Self::Input => "request_input",
            Self::EncodingLimit => "request_size",
            Self::EncodingUnknown => "request_encoding_cause_unknown",
            Self::ClientUnknown => "client_construction_cause_unknown",
            Self::RequestUnknown => "request_construction_cause_unknown",
            Self::ConnectUnknown => "verified_connection_cause_unknown",
            Self::TransportUnknown => "transport_cause_unknown",
            Self::Deadline => "attempt_deadline",
            Self::ResponseEncoding => "response_content_encoding",
            Self::ResponseLimit => "response_size",
            Self::ResponseReadUnknown => "response_read_cause_unknown",
            Self::ResponseJson => "response_json_schema",
        }
    }
}

/// Only closed categories and observed status survive the SDK boundary. No raw
/// source error, request, credential, URL, identifier or diagnostic body is kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Failure {
    pub operation: Operation,
    pub constraint: Constraint,
    pub status: Option<u16>,
    pub dispatch_attempted: bool,
}

impl Failure {
    fn new(operation: Operation, constraint: Constraint) -> Self {
        Self {
            operation,
            constraint,
            status: None,
            dispatch_attempted: false,
        }
    }

    pub(super) fn input(operation: Operation) -> Self {
        Self::new(operation, Constraint::Input)
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "code=INTAKE_REQUEST_UNAVAILABLE stage={} constraint={} status={} outcome={} action=Report only this code/stage/constraint/status. No automatic replay occurred; do not infer a charge or refund. A deliberate new submission may incur another charge.",
            self.operation.label(), self.constraint.label(),
            self.status.map_or_else(|| "unobserved".into(), |s| s.to_string()),
            if self.dispatch_attempted { "unknown" } else { "not_dispatched" })
    }
}
impl std::error::Error for Failure {}

// Fixed variants prevent accidentally sending prompts, model/token metadata or
// customer credentials to accounting. No provision/credit/admin route exists.
pub(super) enum Request<'a> {
    Login(&'a LoginMaterial),
    Refresh(&'a str),
    AuthMethods,
    Reserve {
        account: &'a str,
        maximum: u64,
        timeout_seconds: u16,
        key: &'a str,
    },
    Renew {
        reservation: &'a str,
        expected_expires_at: &'a str,
    },
    Settle {
        reservation: &'a str,
        charged: u64,
    },
    Refund(&'a str),
    Lookup(&'a str),
    Balance(&'a str),
}

impl Request<'_> {
    fn operation(&self) -> Operation {
        match self {
            Self::Login(_) => Operation::Login,
            Self::Refresh(_) => Operation::Refresh,
            Self::AuthMethods => Operation::AuthMethods,
            Self::Reserve { .. } => Operation::Reserve,
            Self::Renew { .. } => Operation::Renew,
            Self::Settle { .. } => Operation::Settle,
            Self::Refund(_) => Operation::Refund,
            Self::Lookup(_) => Operation::Lookup,
            Self::Balance(_) => Operation::Balance,
        }
    }

    fn body(&self) -> Result<Option<Vec<u8>>, Failure> {
        let operation = self.operation();
        let account_valid = |s: &str| !s.is_empty() && s.len() <= 128;
        let id_valid = |s: &str| s.len() == 32 && s.bytes().all(lower_hex);
        let value = match self {
            Self::Login(material) => return encode(material, operation).map(Some),
            Self::Refresh(_) => json!({}),
            Self::AuthMethods => return Ok(None),
            Self::Reserve {
                account,
                maximum,
                timeout_seconds,
                key,
            } => {
                if !account_valid(account)
                    || *maximum == 0
                    || !(1..=1800).contains(timeout_seconds)
                    || !valid_key(key)
                {
                    return Err(Failure::input(operation));
                }
                json!({"account_id": account, "maximum": maximum.to_string(), "timeout_seconds": timeout_seconds})
            }
            Self::Renew {
                reservation,
                expected_expires_at,
            } => {
                if !id_valid(reservation)
                    || super::protocol::canonical_millis(expected_expires_at).is_none()
                {
                    return Err(Failure::input(operation));
                }
                json!({"reservation_id": reservation, "expected_expires_at": expected_expires_at})
            }
            Self::Settle {
                reservation,
                charged,
            } => {
                if !id_valid(reservation) {
                    return Err(Failure::input(operation));
                }
                json!({"reservation_id": reservation, "charged_amount": charged.to_string()})
            }
            Self::Refund(id) | Self::Lookup(id) => {
                if !id_valid(id) {
                    return Err(Failure::input(operation));
                }
                json!({"reservation_id": id})
            }
            Self::Balance(account) => {
                if !account_valid(account) {
                    return Err(Failure::input(operation));
                }
                json!({"account_id": account})
            }
        };
        encode(&value, operation).map(Some)
    }
}

fn lower_hex(b: u8) -> bool {
    b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
}
fn valid_key(key: &str) -> bool {
    let b = key.as_bytes();
    b.len() == 49
        && b.starts_with(b"v1.")
        && b[3..16].iter().all(u8::is_ascii_digit)
        && b[16] == b'.'
        && b[17..].iter().copied().all(lower_hex)
}

fn encode(value: &impl serde::Serialize, operation: Operation) -> Result<Vec<u8>, Failure> {
    bounded_json::to_vec(value, REQUEST_LIMIT).map_err(|error| {
        Failure::new(
            operation,
            match error {
                bounded_json::EncodingFailure::LimitExceeded => Constraint::EncodingLimit,
                bounded_json::EncodingFailure::CauseUnknown => Constraint::EncodingUnknown,
            },
        )
    })
}

// No Debug/Display on the client or response: these contain private values.
pub(super) struct IntakeClient {
    origin: Url,
    client: Client,
    ledger_authorization: header::HeaderValue,
}

pub(super) struct Response {
    pub status: StatusCode,
    operation: Operation,
    body: Vec<u8>,
}

impl Response {
    #[cfg(test)]
    pub(super) fn fixture(operation: Operation, status: u16, body: Vec<u8>) -> Self {
        Self {
            status: StatusCode::from_u16(status).unwrap(),
            operation,
            body,
        }
    }

    // The caller still must validate bindings, amounts, terminal evidence and
    // status semantics. Deserialization alone never authorizes inference/tools.
    pub(super) fn decode<T: DeserializeOwned>(&self) -> Result<T, Failure> {
        serde_json::from_slice(&self.body).map_err(|_| Failure {
            operation: self.operation,
            constraint: Constraint::ResponseJson,
            status: Some(self.status.as_u16()),
            dispatch_attempted: true,
        })
    }
}

impl IntakeClient {
    pub(super) fn new(origin: &str, ledger_key: &str) -> Result<Self, Failure> {
        Self::build(
            origin,
            ledger_key,
            tls_config(webpki_roots::TLS_SERVER_ROOTS.iter().cloned().collect())?,
            ATTEMPT_TIMEOUT,
        )
    }

    fn build(
        origin: &str,
        ledger_key: &str,
        tls: rustls::ClientConfig,
        timeout: Duration,
    ) -> Result<Self, Failure> {
        let invalid = || Failure::new(Operation::Configure, Constraint::Configuration);
        let url = Url::parse(origin).map_err(|_| invalid())?;
        // Do not silently normalize path/userinfo/whitespace into a trusted origin.
        let canonical = url.origin().ascii_serialization();
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !(origin == canonical || origin == format!("{canonical}/"))
            || ledger_key.len() < 32
            || ledger_key.len() > REQUEST_LIMIT
            || !ledger_key.bytes().all(|b| (0x21..=0x7e).contains(&b))
        {
            return Err(invalid());
        }
        let ledger_authorization =
            sensitive_header(&format!("Bearer {ledger_key}")).map_err(|_| invalid())?;
        let client = Client::builder()
            .tls_backend_preconfigured(tls)
            .https_only(true)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(timeout)
            .build()
            .map_err(|_| Failure::new(Operation::Configure, Constraint::ClientUnknown))?;
        Ok(Self {
            origin: url,
            client,
            ledger_authorization,
        })
    }

    pub(super) async fn send(&self, request: Request<'_>) -> Result<Response, Failure> {
        let operation = request.operation();
        let body = request.body()?;
        let method = if matches!(request, Request::AuthMethods) {
            Method::GET
        } else {
            Method::POST
        };
        let mut url = self.origin.clone();
        url.set_path(operation.path());
        let mut builder = self
            .client
            .request(method, url)
            .header(header::ACCEPT, "application/json")
            .header(header::ACCEPT_ENCODING, "identity");
        builder = match &request {
            Request::Login(_) | Request::AuthMethods => builder,
            Request::Refresh(token) => {
                if token.is_empty()
                    || token.len() > AUTH_LIMIT
                    || !token.bytes().all(|b| (0x21..=0x7e).contains(&b))
                {
                    return Err(Failure::input(operation));
                }
                builder.header(
                    header::AUTHORIZATION,
                    sensitive_header(token).map_err(|_| Failure::input(operation))?,
                )
            }
            _ => builder.header(header::AUTHORIZATION, self.ledger_authorization.clone()),
        };
        if let Request::Reserve { key, .. } = request {
            builder = builder.header(
                "Idempotency-Key",
                sensitive_header(key).map_err(|_| Failure::input(operation))?,
            );
        }
        if let Some(body) = body {
            builder = builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(body);
        }
        let request = builder
            .build()
            .map_err(|_| Failure::new(operation, Constraint::RequestUnknown))?;
        let mut response = self
            .client
            .execute(request)
            .await
            .map_err(|error| Failure {
                operation,
                constraint: if error.is_timeout() {
                    Constraint::Deadline
                } else if error.is_connect() {
                    Constraint::ConnectUnknown
                } else {
                    Constraint::TransportUnknown
                },
                status: error.status().map(|s| s.as_u16()),
                dispatch_attempted: true,
            })?;
        let status = response.status();
        let failure = |constraint| Failure {
            operation,
            constraint,
            status: Some(status.as_u16()),
            dispatch_attempted: true,
        };
        let encodings = response.headers().get_all(header::CONTENT_ENCODING);
        if encodings
            .iter()
            .any(|encoding| encoding.as_bytes() != b"identity")
        {
            return Err(failure(Constraint::ResponseEncoding));
        }
        let limit = operation.response_limit(status);
        if response
            .content_length()
            .is_some_and(|length| length > limit as u64)
        {
            return Err(failure(Constraint::ResponseLimit));
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|error| {
            failure(if error.is_timeout() {
                Constraint::Deadline
            } else {
                Constraint::ResponseReadUnknown
            })
        })? {
            if chunk.len() > limit.saturating_sub(body.len()) {
                return Err(failure(Constraint::ResponseLimit));
            }
            body.extend_from_slice(&chunk);
        }
        // Non-2xx bodies (including the expired-settlement receipt) belong to the
        // typed protocol decoder. Never discard them with error_for_status().
        Ok(Response {
            status,
            operation,
            body,
        })
    }

    #[cfg(test)]
    pub(super) fn fixture(
        origin: &str,
        key: &str,
        roots: rustls::RootCertStore,
        timeout: Duration,
    ) -> Self {
        Self::build(origin, key, tls_config(roots).unwrap(), timeout).unwrap()
    }
}

fn sensitive_header(value: &str) -> Result<header::HeaderValue, header::InvalidHeaderValue> {
    let mut header = header::HeaderValue::from_str(value)?;
    header.set_sensitive(true);
    Ok(header)
}

fn tls_config(roots: rustls::RootCertStore) -> Result<rustls::ClientConfig, Failure> {
    rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .map(|builder| {
            let mut config = builder.with_root_certificates(roots).with_no_client_auth();
            // Reqwest leaves ALPN to preconfigured TLS. Prefer HTTP/2's framed
            // completion; the live intake HTTP/1 chunked path can end early.
            config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
            config
        })
        .map_err(|_| Failure::new(Operation::Configure, Constraint::ClientUnknown))
}
