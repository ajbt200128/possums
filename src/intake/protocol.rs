//! Typed adapter for intake's existing native auth and accounting endpoints.
//!
//! HTTPS/native auth is the authority. JWT decoding below inspects metadata of
//! that response only; it neither verifies signatures nor replaces auth-refresh.
//! Privacy: PRIVACY.md#mandatory-typed-interface-failures and intake's
//! PRIVACY.md#forbidden-collection-and-defaults. No private value implements Debug.
use super::{
    transport::{self, IntakeClient, Operation, Request, Response},
    LoginMaterial,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{de, Deserialize, Deserializer, Serialize};
use std::{
    fmt,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const COLLECTION: &str = "customer_keys";
const COLLECTION_ID: &str = "customerkeys001";
const TOKEN_LIMIT: usize = 64 * 1024;
const CLAIMS_LIMIT: usize = 16 * 1024;
const DAY_MS: i64 = 86_400_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ErrorKind {
    Authentication,
    MfaUnsupported,
    BindingChanged,
    Input,
    AccountUnavailable,
    CreditInsufficient,
    AdmissionDisabled,
    Conflict,
    EvidenceUnavailable,
    RemoteUnavailable,
    Protocol,
    Transport,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Stage {
    Transport,
    Decode,
    AuthMethods,
    TokenMetadata,
    Binding,
    Receipt,
    Remote,
    Validation,
    Account,
    Admission,
    Key,
    Arithmetic,
    Settlement,
    Renewal,
    Lookup,
    WriterGate,
    TransactionBegin,
    TransactionWork,
    TransactionCompletion,
    ReservationIdentity,
    StateValidation,
    Readiness,
    Shutdown,
    Startup,
    Init,
    Routing,
    RequestRead,
    RequestDispatch,
    ResponseEncoding,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Constraint {
    UsableHold,
    ImmutableHoldExpiry,
    ActiveReservation,
    ExpectedNotAfterAuthoritativeDeadline,
    StrictlyLaterDeadline,
    RenewalObservation,
    FirstTerminalWins,
    Input,
    Transport,
    RequiredSchema,
    PasswordMethod,
    Mfa,
    TokenMetadata,
    OriginalExpiry,
    PrincipalBinding,
    ReservationBinding,
    ReservationState,
    Account,
    Credit,
    Admission,
    Idempotency,
    Charge,
    KeyWindow,
    Evidence,
    RemoteCauseUnknown,
    KeySyntax,
    MoneyOverflow,
    ImmutableDeadline,
    WriterWait,
    WorkBudget,
    TransactionStarted,
    UsableStorage,
    UniqueIdentifier,
    ConsistentState,
    CommitEvidence,
    OpenStorage,
    StorageClosure,
    OriginalStorage,
    ExistingState,
    EmptyDirectory,
    SupportedSchema,
    StatePath,
    Routes,
    Query,
    ContentEncoding,
    ContentType,
    SingletonHeaders,
    IdempotencyHeader,
    CompleteBody,
    ClosedResponse,
}

/// Contains only closed projections, never an SDK error/body or private value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Error {
    pub(crate) kind: ErrorKind,
    pub(crate) stage: Stage,
    pub(crate) constraint: Constraint,
    pub(crate) status: Option<u16>,
    pub(crate) dispatch_attempted: bool,
    operation: Operation,
    transport: Option<transport::Failure>,
}

impl Error {
    fn local(operation: Operation, kind: ErrorKind, stage: Stage, constraint: Constraint) -> Self {
        Self {
            kind,
            stage,
            constraint,
            status: None,
            dispatch_attempted: false,
            operation,
            transport: None,
        }
    }

    fn response(
        operation: Operation,
        status: u16,
        kind: ErrorKind,
        stage: Stage,
        constraint: Constraint,
    ) -> Self {
        Self {
            status: Some(status),
            dispatch_attempted: true,
            ..Self::local(operation, kind, stage, constraint)
        }
    }

    pub(crate) fn diagnostic(&self) -> super::diagnostics::Diagnostic {
        let diagnostic = super::diagnostics::Diagnostic::protocol(
            self.kind,
            self.operation,
            self.stage,
            self.constraint,
            self.status,
        );
        match self.transport {
            Some(failure) => diagnostic.with_transport_constraint(failure.constraint),
            None => diagnostic,
        }
    }

    pub(crate) fn configuration_invalid(&self) -> bool {
        self.transport
            .is_some_and(|failure| failure.constraint == transport::Constraint::Configuration)
    }

    pub(crate) fn operation(&self) -> &'static str {
        match self.operation {
            Operation::Configure => "intake_configuration",
            Operation::Login => "intake_login",
            Operation::Refresh => "intake_refresh",
            Operation::AuthMethods => "intake_auth_methods",
            Operation::Reserve => "intake_reserve",
            Operation::Renew => "intake_renew",
            Operation::Settle => "intake_settle",
            Operation::Refund => "intake_refund",
            Operation::Lookup => "intake_lookup",
            Operation::Balance => "intake_balance",
        }
    }

    pub(crate) fn code(&self) -> &'static str {
        match self.kind {
            ErrorKind::Authentication => "INTAKE_AUTHENTICATION_UNCONFIRMED",
            ErrorKind::MfaUnsupported => "INTAKE_MFA_UNSUPPORTED",
            ErrorKind::BindingChanged => "INTAKE_BINDING_CHANGED",
            ErrorKind::Input => "INTAKE_INPUT_INVALID",
            ErrorKind::AccountUnavailable => "INTAKE_ACCOUNT_UNAVAILABLE",
            ErrorKind::CreditInsufficient => "INTAKE_CREDIT_INSUFFICIENT",
            ErrorKind::AdmissionDisabled => "INTAKE_ADMISSION_DISABLED",
            ErrorKind::Conflict => "INTAKE_REQUEST_CONFLICT",
            ErrorKind::EvidenceUnavailable => "INTAKE_EVIDENCE_UNAVAILABLE",
            ErrorKind::RemoteUnavailable => "INTAKE_REMOTE_UNAVAILABLE",
            ErrorKind::Protocol => "INTAKE_PROTOCOL_UNCONFIRMED",
            ErrorKind::Transport => "INTAKE_REQUEST_UNAVAILABLE",
        }
    }
}

impl From<transport::Failure> for Error {
    fn from(failure: transport::Failure) -> Self {
        Self {
            kind: match failure.constraint {
                transport::Constraint::Input => ErrorKind::Input,
                transport::Constraint::ResponseJson => ErrorKind::Protocol,
                _ => ErrorKind::Transport,
            },
            stage: if failure.constraint == transport::Constraint::ResponseJson {
                Stage::Decode
            } else {
                Stage::Transport
            },
            constraint: if failure.constraint == transport::Constraint::ResponseJson {
                Constraint::RequiredSchema
            } else {
                Constraint::Transport
            },
            status: failure.status,
            dispatch_attempted: failure.dispatch_attempted,
            operation: failure.operation,
            transport: Some(failure),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let action = match self.kind {
            ErrorKind::Authentication if self.constraint == Constraint::OriginalExpiry => "Original intake authority expired. Log in deliberately; this session cannot renew its authority.",
            ErrorKind::Authentication if self.constraint == Constraint::PasswordMethod => "The fixed customer key password method is unavailable. Ask the operator to check native authentication configuration privately.",
            ErrorKind::Authentication => "Current native authentication is unconfirmed; cause unknown. Log in deliberately after checking intake access privately.",
            ErrorKind::MfaUnsupported => "Gateway authentication does not support customer MFA. Use a supported native authentication configuration; do not bypass MFA.",
            ErrorKind::BindingChanged => "The captured customer binding no longer matches current intake authority. Stop using this session and log in deliberately.",
            ErrorKind::Input => "Check the locally constructed request against the intake contract.",
            ErrorKind::AccountUnavailable => "Ask the operator to check account availability privately.",
            ErrorKind::CreditInsufficient => "The requested maximum hold exceeds available credit. Add credit before a deliberate new submission.",
            ErrorKind::AdmissionDisabled => "New admissions are disabled for this account. Ask the operator to check privately.",
            ErrorKind::Conflict => "The request conflicts with intake constraints. Inspect the original operation privately; do not invent a new identity to replay it.",
            ErrorKind::EvidenceUnavailable => "Intake no longer supplies the requested accounting evidence. Do not infer a charge or refund.",
            _ => "Intake evidence is unconfirmed; underlying cause unknown. Check intake availability and protocol compatibility privately.",
        };
        write!(f, "code={} operation={} stage={:?} constraint={:?} status={} transport={:?} outcome={} action={} No automatic replay occurred. Do not infer a charge or refund. A deliberate new submission may incur another charge.",
            self.code(), self.operation(), self.stage, self.constraint,
            self.status.map_or_else(|| "unobserved".into(), |s| s.to_string()), self.transport,
            if self.dispatch_attempted { "unknown" } else { "not_dispatched" }, action)
    }
}
impl std::error::Error for Error {}

pub(crate) struct Principal {
    pub(crate) account_id: String,
    pub(crate) record_id: String,
    pub(crate) expires_at: SystemTime,
    token: String,
}

pub(crate) struct Intake {
    client: IntakeClient,
}

impl Intake {
    #[cfg(test)]
    pub(super) fn fixture(client: IntakeClient) -> Self {
        Self { client }
    }

    pub(crate) fn new(origin: &str, key: &str) -> Result<Self, Error> {
        Ok(Self {
            client: IntakeClient::new(origin, key)?,
        })
    }

    async fn methods(&self) -> Result<(), Error> {
        let response = self.client.send(Request::AuthMethods).await?;
        if response.status.as_u16() != 200 {
            return Err(remote(&response, Operation::AuthMethods));
        }
        check_methods(decode(&response)?, response.status.as_u16())
    }

    pub(crate) async fn login(&self, raw_key: &str) -> Result<Principal, Error> {
        let material = LoginMaterial::derive(raw_key)?;
        self.methods().await?;
        let response = self.client.send(Request::Login(&material)).await?;
        if response.status.as_u16() != 200 {
            return Err(remote(&response, Operation::Login));
        }
        principal(decode(&response)?, Operation::Login, SystemTime::now())
    }

    pub(crate) async fn validate(&self, captured: &Principal) -> Result<(), Error> {
        if captured.expires_at <= SystemTime::now() {
            return Err(Error::local(
                Operation::Refresh,
                ErrorKind::Authentication,
                Stage::TokenMetadata,
                Constraint::OriginalExpiry,
            ));
        }
        self.methods().await?;
        // Never adopt the renewed token or extend the original expiry.
        let response = self.client.send(Request::Refresh(&captured.token)).await?;
        if response.status.as_u16() != 200 {
            return Err(remote(&response, Operation::Refresh));
        }
        let current = principal(decode(&response)?, Operation::Refresh, SystemTime::now())?;
        check_binding(captured, &current, response.status.as_u16())?;
        if captured.expires_at <= SystemTime::now() {
            return Err(Error::response(
                Operation::Refresh,
                200,
                ErrorKind::Authentication,
                Stage::TokenMetadata,
                Constraint::OriginalExpiry,
            ));
        }
        Ok(())
    }

    pub(crate) async fn reserve(
        &self,
        account: &str,
        maximum: u64,
        timeout_seconds: u16,
        key: &str,
    ) -> Result<Reservation, Error> {
        let response = self
            .client
            .send(Request::Reserve {
                account,
                maximum,
                timeout_seconds,
                key,
            })
            .await?;
        if response.status.as_u16() != 200 {
            return Err(remote(&response, Operation::Reserve));
        }
        let wire: ReservationResponse = decode(&response)?;
        check_reserve(wire.reservation, account, maximum, timeout_seconds, key)
    }

    /// One conditional attempt; the accepted-work owner alone schedules renewals.
    pub(crate) async fn renew(&self, reservation: &Reservation) -> Result<RenewalReply, Error> {
        let invalid = || {
            Error::local(
                Operation::Renew,
                ErrorKind::Input,
                Stage::Validation,
                Constraint::ReservationState,
            )
        };
        if !reservation.valid() {
            return Err(invalid());
        }
        let expected = reservation.expires_at.canonical().ok_or_else(invalid)?;
        let response = self
            .client
            .send(Request::Renew {
                reservation: &reservation.reservation_id,
                expected_expires_at: &expected,
            })
            .await?;
        check_renew_response(reservation, &response)
    }

    pub(crate) async fn finish(
        &self,
        reservation: &Reservation,
        charge: Option<u64>,
    ) -> Result<TerminalReceipt, Error> {
        let operation = if charge.is_some() {
            Operation::Settle
        } else {
            Operation::Refund
        };
        let request = match charge {
            Some(charged) => Request::Settle {
                reservation: &reservation.reservation_id,
                charged,
            },
            None => Request::Refund(&reservation.reservation_id),
        };
        let response = self.client.send(request).await?; // exactly one terminal attempt
        let status = response.status.as_u16();
        if status != 200 && !(status == 409 && charge.is_some()) {
            return Err(remote(&response, operation));
        }
        if status == 409 {
            // Only the documented expired-settlement code/status carries evidence.
            let diagnostic: Diagnostic = decode(&response)?;
            if diagnostic.code != RemoteCode::Expired {
                return Err(remote(&response, operation));
            }
        }
        let wire: ReservationResponse = decode(&response)?;
        check_terminal(reservation, wire.reservation, charge, status)
    }
}

fn decode<T: serde::de::DeserializeOwned>(response: &Response) -> Result<T, Error> {
    response.decode().map_err(Error::from)
}

#[derive(Deserialize)]
struct AuthMethods {
    password: PasswordMethod,
    mfa: EnabledMethod,
}
#[derive(Deserialize)]
struct PasswordMethod {
    enabled: bool,
    #[serde(rename = "identityFields")]
    identity_fields: Vec<String>,
}
#[derive(Deserialize)]
struct EnabledMethod {
    enabled: bool,
}

fn check_methods(methods: AuthMethods, status: u16) -> Result<(), Error> {
    if methods.mfa.enabled {
        return Err(Error::response(
            Operation::AuthMethods,
            status,
            ErrorKind::MfaUnsupported,
            Stage::AuthMethods,
            Constraint::Mfa,
        ));
    }
    if !methods.password.enabled
        || !methods
            .password
            .identity_fields
            .iter()
            .any(|field| field == "key_lookup")
    {
        return Err(Error::response(
            Operation::AuthMethods,
            status,
            ErrorKind::Authentication,
            Stage::AuthMethods,
            Constraint::PasswordMethod,
        ));
    }
    Ok(())
}

#[derive(Deserialize)]
struct AuthResponse {
    token: String,
    record: AuthRecord,
}
#[derive(Deserialize)]
struct AuthRecord {
    id: String,
    #[serde(rename = "collectionId")]
    collection_id: String,
    #[serde(rename = "collectionName")]
    collection_name: String,
    account_id: String,
}
#[derive(Deserialize)]
struct Claims {
    id: String,
    #[serde(rename = "collectionId")]
    collection_id: String,
    #[serde(rename = "type")]
    token_type: String,
    refreshable: bool,
    exp: u64,
}

fn principal(
    auth: AuthResponse,
    operation: Operation,
    now: SystemTime,
) -> Result<Principal, Error> {
    let invalid = |constraint| {
        Error::response(
            operation,
            200,
            ErrorKind::Protocol,
            Stage::TokenMetadata,
            constraint,
        )
    };
    let record = auth.record;
    if !valid_account(&record.account_id) || record.id.is_empty() || record.id.len() > 128 {
        return Err(invalid(Constraint::PrincipalBinding));
    }
    if record.collection_id != COLLECTION_ID || record.collection_name != COLLECTION {
        return Err(Error::response(
            operation,
            200,
            if operation == Operation::Refresh {
                ErrorKind::BindingChanged
            } else {
                ErrorKind::Protocol
            },
            Stage::Binding,
            Constraint::PrincipalBinding,
        ));
    }
    let claims = token_claims(&auth.token).ok_or_else(|| invalid(Constraint::TokenMetadata))?;
    if claims.id != record.id
        || claims.collection_id != record.collection_id
        || claims.token_type != "auth"
        || !claims.refreshable
    {
        return Err(invalid(Constraint::PrincipalBinding));
    }
    let expires_at = UNIX_EPOCH
        .checked_add(Duration::from_secs(claims.exp))
        .filter(|expiry| *expiry > now)
        .ok_or_else(|| invalid(Constraint::TokenMetadata))?;
    Ok(Principal {
        account_id: record.account_id,
        record_id: record.id,
        token: auth.token,
        expires_at,
    })
}

fn token_claims(token: &str) -> Option<Claims> {
    if token.len() > TOKEN_LIMIT {
        return None;
    }
    let mut parts = token.split('.');
    let header = parts.next()?;
    let payload = parts.next()?;
    let signature = parts.next()?;
    if parts.next().is_some()
        || [header, payload, signature].iter().any(|p| {
            p.is_empty()
                || !p
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
        || payload.len() > CLAIMS_LIMIT * 4 / 3 + 4
    {
        return None;
    }
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    if bytes.len() > CLAIMS_LIMIT {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

fn check_binding(captured: &Principal, current: &Principal, status: u16) -> Result<(), Error> {
    if current.account_id != captured.account_id || current.record_id != captured.record_id {
        return Err(Error::response(
            Operation::Refresh,
            status,
            ErrorKind::BindingChanged,
            Stage::Binding,
            Constraint::PrincipalBinding,
        ));
    }
    Ok(())
}

// Serde struct decoding rejects duplicate known fields. Required nullable fields
// use deserialize_with without default so absent is distinct from explicit null.
pub(crate) struct Reservation {
    pub(crate) reservation_id: String,
    pub(crate) account_id: String,
    pub(crate) maximum: u64,
    pub(crate) timeout_seconds: u16,
    idempotency_key: String,
    initial_expires_at: Millis,
    expires_at: Millis,
    state: State,
    charged_amount: Option<u64>,
    terminal_at: Option<Millis>,
    retain_until: Option<Millis>,
}

// Only private wire envelopes can deserialize financial rows in production.
// Each operation validates the decoded row before returning it to its caller.
#[derive(Deserialize)]
#[serde(remote = "Reservation")]
struct ReservationWire {
    reservation_id: String,
    account_id: String,
    #[serde(deserialize_with = "money")]
    maximum: u64,
    timeout_seconds: u16,
    idempotency_key: String,
    initial_expires_at: Millis,
    expires_at: Millis,
    state: State,
    #[serde(deserialize_with = "nullable_money")]
    charged_amount: Option<u64>,
    #[serde(deserialize_with = "nullable")]
    terminal_at: Option<Millis>,
    #[serde(deserialize_with = "nullable")]
    retain_until: Option<Millis>,
}
#[cfg(test)]
impl<'de> Deserialize<'de> for Reservation {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        ReservationWire::deserialize(d)
    }
}
#[derive(Deserialize)]
struct ReservationResponse {
    #[serde(deserialize_with = "ReservationWire::deserialize")]
    reservation: Reservation,
}
#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum State {
    Active,
    Settled,
    Refunded,
    Expired,
}
#[derive(Clone, Copy, PartialEq, Eq)]
struct Millis(i64);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum TerminalOutcome {
    Settled { charged: u64 },
    Refunded,
    Expired,
}
impl TerminalOutcome {
    /// Compare already observed terminal evidence without another ledger call.
    pub(crate) fn matches_requested(self, requested: Option<u64>) -> bool {
        match (requested, self) {
            (Some(expected), Self::Settled { charged }) => expected == charged,
            (None, Self::Refunded) => true,
            _ => false,
        }
    }
}

pub(crate) struct TerminalReceipt {
    pub(crate) outcome: TerminalOutcome,
    pub(crate) matches_requested: bool,
    pub(crate) diagnostic: Option<super::diagnostics::Diagnostic>,
}

/// A validated observation, not necessarily a renewal confirmation. Only a
/// reply without refusal confirms active progress. Private provenance preserves
/// the actual operation/status when comparing a closed row at natural completion.
pub(crate) struct RenewalReply {
    pub(crate) reservation: Reservation,
    pub(crate) observed_at: SystemTime,
    pub(crate) refusal: Option<Error>,
    status: u16,
}

impl RenewalReply {
    pub(crate) fn terminal_receipt(&self, requested: Option<u64>) -> Option<TerminalReceipt> {
        terminal_receipt(&self.reservation, requested, Operation::Renew, self.status)
    }
}

#[derive(Deserialize)]
struct RenewalResponse {
    #[serde(deserialize_with = "ReservationWire::deserialize")]
    reservation: Reservation,
    observed_at: Millis,
}

#[derive(Deserialize)]
struct RenewalRefusal {
    code: RemoteCode,
    // Validate the complete known schema, but discard all remote prose. Only
    // the pinned code/status and independently validated row authorize meaning.
    #[serde(rename = "stage", deserialize_with = "discard_string")]
    _stage: (),
    #[serde(rename = "constraint", deserialize_with = "discard_string")]
    _constraint: (),
    #[serde(rename = "outcome", deserialize_with = "discard_string")]
    _outcome: (),
    #[serde(rename = "next_step", deserialize_with = "discard_string")]
    _next_step: (),
    #[serde(deserialize_with = "ReservationWire::deserialize")]
    reservation: Reservation,
    observed_at: Millis,
}

fn discard_string<'de, D: Deserializer<'de>>(d: D) -> Result<(), D::Error> {
    String::deserialize(d).map(|_| ())
}

fn check_renew_response(
    original: &Reservation,
    response: &Response,
) -> Result<RenewalReply, Error> {
    let status = response.status.as_u16();
    match status {
        200 => {
            let wire: RenewalResponse = decode(response)?;
            check_renew(original, wire.reservation, wire.observed_at, None, status)
        }
        409 => {
            let diagnostic: Diagnostic = decode(response)?;
            if !matches!(
                diagnostic.code,
                RemoteCode::RenewalClosed
                    | RemoteCode::RenewalPrecondition
                    | RemoteCode::RenewalNoProgress
                    | RemoteCode::Disabled
            ) {
                return Err(Error::response(
                    Operation::Renew,
                    status,
                    ErrorKind::Protocol,
                    Stage::Renewal,
                    Constraint::RequiredSchema,
                ));
            }
            let wire: RenewalRefusal = decode(response)?;
            check_renew(
                original,
                wire.reservation,
                wire.observed_at,
                Some(wire.code),
                status,
            )
        }
        _ => Err(remote(response, Operation::Renew)),
    }
}

fn check_renew(
    original: &Reservation,
    current: Reservation,
    observed: Millis,
    refusal: Option<RemoteCode>,
    status: u16,
) -> Result<RenewalReply, Error> {
    let error = |constraint| {
        Error::response(
            Operation::Renew,
            status,
            ErrorKind::Protocol,
            Stage::Renewal,
            constraint,
        )
    };
    if !current.valid() {
        return Err(error(Constraint::ReservationState));
    }
    if !original.same_reservation(&current) {
        return Err(error(Constraint::ReservationBinding));
    }
    let admitted = current.initial_expires_at.0 - i64::from(current.timeout_seconds) * 1000;
    let latest = observed.0 + i64::from(current.timeout_seconds) * 1000;
    if observed.0 < admitted
        || current.expires_at.0 > latest
        || current
            .terminal_at
            .is_some_and(|terminal| terminal.0 > observed.0)
        || (current.is_active() && current.expires_at.0 <= observed.0)
    {
        return Err(error(Constraint::RenewalObservation));
    }
    match (status, refusal) {
        (200, None) if current.is_active() && current.expires_at.0 > original.expires_at.0 => {}
        (409, Some(RemoteCode::RenewalClosed)) if !current.is_active() => {}
        (409, Some(RemoteCode::Disabled))
            if current.is_active() && original.expires_at.0 <= current.expires_at.0 => {}
        (409, Some(RemoteCode::RenewalNoProgress))
            if current.is_active()
                && current.expires_at == original.expires_at
                && latest <= current.expires_at.0 => {}
        (409, Some(RemoteCode::RenewalPrecondition))
            if current.is_active() && original.expires_at.0 > current.expires_at.0 =>
        {
            // A well-formed precondition refusal is typed, but its older
            // row cannot replace a validated captured deadline.
            return Err(classify_remote(
                RemoteCode::RenewalPrecondition,
                status,
                Operation::Renew,
            ));
        }
        _ => return Err(error(Constraint::StrictlyLaterDeadline)),
    }
    check_evolution(original, &current).map_err(error)?;
    Ok(RenewalReply {
        reservation: current,
        observed_at: observed.system_time(),
        refusal: refusal.map(|code| classify_remote(code, status, Operation::Renew)),
        status,
    })
}

impl Reservation {
    /// Reserve replays can return a previously terminal row. Only an active
    /// receipt can admit new inference; a 200 alone is not admission evidence.
    pub(crate) fn is_active(&self) -> bool {
        self.state == State::Active
    }

    pub(crate) fn terminal_outcome(&self) -> Option<TerminalOutcome> {
        match self.state {
            State::Active => None,
            State::Settled => self
                .charged_amount
                .map(|charged| TerminalOutcome::Settled { charged }),
            State::Refunded => Some(TerminalOutcome::Refunded),
            State::Expired => Some(TerminalOutcome::Expired),
        }
    }

    pub(crate) fn expires_at(&self) -> SystemTime {
        // Only validated positive, bounded dates leave the decoder boundary.
        self.expires_at.system_time()
    }

    fn valid(&self) -> bool {
        if !valid_account(&self.account_id)
            || !valid_id(&self.reservation_id)
            || self.maximum == 0
            || !(1..=1800).contains(&self.timeout_seconds)
        {
            return false;
        }
        let Some(issued) = key_issued(&self.idempotency_key) else {
            return false;
        };
        let admitted = self.initial_expires_at.0 - i64::from(self.timeout_seconds) * 1000;
        if self.expires_at.0 < self.initial_expires_at.0
            || admitted <= 0
            || issued > admitted + 60_000
            || admitted >= issued + DAY_MS
        {
            return false;
        }
        if self.state == State::Active {
            return self.charged_amount.is_none()
                && self.terminal_at.is_none()
                && self.retain_until.is_none();
        }
        let (Some(charge), Some(terminal), Some(retain)) =
            (self.charged_amount, self.terminal_at, self.retain_until)
        else {
            return false;
        };
        if charge > self.maximum
            || terminal.0 < admitted
            || retain.0 != issued.max(terminal.0) + DAY_MS
        {
            return false;
        }
        match self.state {
            State::Expired => charge == 0 && terminal == self.expires_at,
            State::Refunded => charge == 0 && terminal.0 < self.expires_at.0,
            State::Settled => terminal.0 < self.expires_at.0,
            State::Active => false,
        }
    }

    fn same_reservation(&self, other: &Self) -> bool {
        self.reservation_id == other.reservation_id
            && self.account_id == other.account_id
            && self.maximum == other.maximum
            && self.timeout_seconds == other.timeout_seconds
            && self.idempotency_key == other.idempotency_key
            && self.initial_expires_at == other.initial_expires_at
    }
}

fn check_reserve(
    reservation: Reservation,
    account: &str,
    maximum: u64,
    timeout: u16,
    key: &str,
) -> Result<Reservation, Error> {
    let error = |constraint| {
        Error::response(
            Operation::Reserve,
            200,
            ErrorKind::Protocol,
            Stage::Receipt,
            constraint,
        )
    };
    if !reservation.valid() {
        return Err(error(Constraint::ReservationState));
    }
    if reservation.account_id != account
        || reservation.maximum != maximum
        || reservation.timeout_seconds != timeout
        || reservation.idempotency_key != key
    {
        return Err(error(Constraint::ReservationBinding));
    }
    Ok(reservation)
}

fn check_terminal(
    original: &Reservation,
    current: Reservation,
    requested: Option<u64>,
    status: u16,
) -> Result<TerminalReceipt, Error> {
    let operation = if requested.is_some() {
        Operation::Settle
    } else {
        Operation::Refund
    };
    let error = |constraint| {
        Error::response(
            operation,
            status,
            ErrorKind::Protocol,
            Stage::Receipt,
            constraint,
        )
    };
    if !current.valid() {
        return Err(error(Constraint::ReservationState));
    }
    check_evolution(original, &current).map_err(error)?;
    let outcome = current
        .terminal_outcome()
        .ok_or_else(|| error(Constraint::ReservationState))?;
    if (status == 409 && outcome != TerminalOutcome::Expired)
        || (status == 200 && requested.is_some() && outcome == TerminalOutcome::Expired)
    {
        return Err(error(Constraint::ReservationState));
    }
    terminal_receipt(&current, requested, operation, status)
        .ok_or_else(|| error(Constraint::ReservationState))
}

fn check_evolution(original: &Reservation, current: &Reservation) -> Result<(), Constraint> {
    if !original.same_reservation(current) || current.expires_at.0 < original.expires_at.0 {
        return Err(Constraint::ReservationBinding);
    }
    // A known terminal row is immutable, including expiry and retention.
    if !original.is_active()
        && (original.expires_at != current.expires_at
            || original.terminal_outcome() != current.terminal_outcome()
            || original.terminal_at != current.terminal_at
            || original.retain_until != current.retain_until)
    {
        return Err(Constraint::ReservationState);
    }
    Ok(())
}

fn terminal_receipt(
    reservation: &Reservation,
    requested: Option<u64>,
    operation: Operation,
    status: u16,
) -> Option<TerminalReceipt> {
    let outcome = reservation.terminal_outcome()?;
    let matches_requested = outcome.matches_requested(requested);
    Some(TerminalReceipt {
        outcome,
        matches_requested,
        diagnostic: super::diagnostics::Diagnostic::terminal(
            outcome,
            matches_requested,
            operation,
            status,
        ),
    })
}

fn valid_account(account: &str) -> bool {
    !account.is_empty() && account.len() <= 128
}
fn valid_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(lower_hex)
}
fn lower_hex(b: u8) -> bool {
    b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
}
fn key_issued(key: &str) -> Option<i64> {
    let bytes = key.as_bytes();
    if bytes.len() != 49
        || !bytes.starts_with(b"v1.")
        || bytes[16] != b'.'
        || !bytes[3..16].iter().all(u8::is_ascii_digit)
        || !bytes[17..].iter().copied().all(lower_hex)
    {
        return None;
    }
    key[3..16].parse().ok()
}

fn parse_money(value: &str) -> Option<u64> {
    if value.is_empty()
        || value.len() > 20
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    value.parse().ok()
}
fn money<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    parse_money(&String::deserialize(d)?).ok_or_else(|| de::Error::custom("canonical_u64_money"))
}
fn nullable_money<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
    Option::<String>::deserialize(d)?
        .map(|s| parse_money(&s).ok_or_else(|| de::Error::custom("canonical_u64_money")))
        .transpose()
}
fn nullable<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(d)
}

fn millis_format() -> Result<
    Vec<time::format_description::BorrowedFormatItem<'static>>,
    time::error::InvalidFormatDescription,
> {
    time::format_description::parse_borrowed::<2>(
        "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z",
    )
}

pub(super) fn canonical_millis(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    if bytes.len() != 24
        || bytes.iter().enumerate().any(|(i, b)| match i {
            4 | 7 => *b != b'-',
            10 => *b != b'T',
            13 | 16 => *b != b':',
            19 => *b != b'.',
            23 => *b != b'Z',
            _ => !b.is_ascii_digit(),
        })
    {
        return None;
    }
    let date = time::PrimitiveDateTime::parse(value, &millis_format().ok()?)
        .ok()?
        .assume_utc();
    let millis = date.unix_timestamp_nanos() / 1_000_000;
    (1..=9_999_999_999_999)
        .contains(&millis)
        .then_some(millis as i64)
}

impl Millis {
    fn system_time(self) -> SystemTime {
        UNIX_EPOCH + Duration::from_millis(self.0 as u64)
    }

    fn canonical(self) -> Option<String> {
        time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(self.0) * 1_000_000)
            .ok()?
            .format(&millis_format().ok()?)
            .ok()
    }
}

impl<'de> Deserialize<'de> for Millis {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        canonical_millis(&String::deserialize(d)?)
            .map(Self)
            .ok_or_else(|| de::Error::custom("fixed_utc_millisecond_date"))
    }
}

#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
enum RemoteCode {
    #[serde(rename = "LEDGER_HTTP_UNAUTHORIZED")]
    Unauthorized,
    #[serde(rename = "LEDGER_INPUT_INVALID")]
    Input,
    #[serde(rename = "LEDGER_KEY_INVALID")]
    Key,
    #[serde(rename = "LEDGER_ACCOUNT_UNKNOWN")]
    Account,
    #[serde(rename = "LEDGER_CREDIT_INSUFFICIENT")]
    Credit,
    #[serde(rename = "LEDGER_ADMISSION_DISABLED")]
    Disabled,
    #[serde(rename = "LEDGER_KEY_CONFLICT")]
    Conflict,
    #[serde(rename = "LEDGER_CHARGE_EXCEEDS_HOLD")]
    Charge,
    #[serde(rename = "LEDGER_KEY_WINDOW")]
    KeyWindow,
    #[serde(rename = "LEDGER_SETTLEMENT_EXPIRED")]
    Expired,
    #[serde(rename = "LEDGER_RENEWAL_CLOSED")]
    RenewalClosed,
    #[serde(rename = "LEDGER_RENEWAL_PRECONDITION")]
    RenewalPrecondition,
    #[serde(rename = "LEDGER_RENEWAL_NO_PROGRESS")]
    RenewalNoProgress,
    #[serde(rename = "LEDGER_EVIDENCE_UNAVAILABLE")]
    Evidence,
    #[serde(rename = "LEDGER_AMOUNT_OVERFLOW")]
    Overflow,
    #[serde(rename = "LEDGER_WRITER_UNAVAILABLE")]
    Writer,
    #[serde(rename = "LEDGER_WORK_CANCELLED")]
    Work,
    #[serde(rename = "LEDGER_TRANSACTION_BEGIN_UNAVAILABLE")]
    Begin,
    #[serde(rename = "LEDGER_STORAGE_UNAVAILABLE")]
    Storage,
    #[serde(rename = "LEDGER_RESERVATION_ID_UNAVAILABLE")]
    Random,
    #[serde(rename = "LEDGER_STATE_INVALID")]
    Corrupt,
    #[serde(rename = "LEDGER_COMMIT_UNKNOWN")]
    Commit,
    #[serde(rename = "LEDGER_NOT_READY")]
    Closed,
    #[serde(rename = "LEDGER_STORAGE_CLOSE_UNAVAILABLE")]
    Close,
    #[serde(rename = "LEDGER_STARTUP_STORAGE")]
    Startup,
    #[serde(rename = "LEDGER_STARTUP_MISSING_STATE")]
    Missing,
    #[serde(rename = "LEDGER_INIT_NONEMPTY")]
    Existing,
    #[serde(rename = "LEDGER_STARTUP_SCHEMA")]
    Schema,
    #[serde(rename = "LEDGER_STATE_PATH_INVALID")]
    Path,
    #[serde(rename = "LEDGER_HTTP_NOT_IMPLEMENTED")]
    Routes,
    #[serde(rename = "LEDGER_HTTP_NOT_FOUND")]
    NotFound,
    #[serde(rename = "LEDGER_HTTP_QUERY_REJECTED")]
    Query,
    #[serde(rename = "LEDGER_HTTP_ENCODING_REJECTED")]
    Encoding,
    #[serde(rename = "LEDGER_HTTP_CONTENT_TYPE_REJECTED")]
    ContentType,
    #[serde(rename = "LEDGER_HTTP_HEADER_REJECTED")]
    Headers,
    #[serde(rename = "LEDGER_HTTP_IDEMPOTENCY_REQUIRED")]
    Idempotency,
    #[serde(rename = "LEDGER_HTTP_INPUT_REJECTED")]
    HttpInput,
    #[serde(rename = "LEDGER_HTTP_READ_UNAVAILABLE")]
    Read,
    #[serde(rename = "LEDGER_HTTP_OUTCOME_UNKNOWN")]
    Dispatch,
    #[serde(rename = "LEDGER_HTTP_RESPONSE_UNAVAILABLE")]
    Response,
    #[serde(rename = "OPERATOR_NATIVE_REQUEST_UNAVAILABLE")]
    Native,
    #[serde(other)]
    Unknown,
}
#[derive(Deserialize)]
struct Diagnostic {
    code: RemoteCode,
}
#[derive(Deserialize)]
struct MfaChallenge {
    #[serde(rename = "mfaId", deserialize_with = "challenge_present")]
    _present: (),
}
fn challenge_present<'de, D: Deserializer<'de>>(d: D) -> Result<(), D::Error> {
    if String::deserialize(d)?.is_empty() {
        return Err(de::Error::custom("native_mfa_challenge"));
    }
    Ok(())
}

fn remote(response: &Response, operation: Operation) -> Error {
    let status = response.status.as_u16();
    // MFA can change after auth-methods. Recognize the stock challenge without
    // exposing or retaining its identifier, and never attempt another factor.
    if operation == Operation::Login && status == 401 && response.decode::<MfaChallenge>().is_ok() {
        return Error::response(
            operation,
            status,
            ErrorKind::MfaUnsupported,
            Stage::AuthMethods,
            Constraint::Mfa,
        );
    }
    let code = match response.decode::<Diagnostic>() {
        Ok(diagnostic) => diagnostic.code,
        Err(failure) => return failure.into(),
    };
    classify_remote(code, status, operation)
}

fn classify_remote(code: RemoteCode, status: u16, operation: Operation) -> Error {
    use ErrorKind as K;
    use {Constraint as C, RemoteCode as R, Stage as S};
    let (kind, stage, constraint) = match (status, code) {
        (401, R::Unauthorized) => (K::Authentication, S::Remote, C::RemoteCauseUnknown),
        // Stock password auth uses 400 for an unconfirmed login; the closed
        // native wrapper does not disclose credentials-vs-validation causes.
        (400, R::Native) if operation == Operation::Login => {
            (K::Authentication, S::Remote, C::RemoteCauseUnknown)
        }
        (401 | 403, R::Native) if matches!(operation, Operation::Login | Operation::Refresh) => {
            (K::Authentication, S::Remote, C::RemoteCauseUnknown)
        }
        (400, R::Input | R::HttpInput) => (K::Input, S::Validation, C::Input),
        (400, R::Key) => (K::Input, S::Validation, C::KeySyntax),
        (400, R::Query) => (K::Input, S::Validation, C::Query),
        (400, R::Encoding) => (K::Input, S::Validation, C::ContentEncoding),
        (400, R::ContentType) => (K::Input, S::Validation, C::ContentType),
        (400, R::Headers) => (K::Input, S::Validation, C::SingletonHeaders),
        (400, R::Idempotency) => (K::Input, S::Validation, C::IdempotencyHeader),
        (404, R::NotFound) => (K::Protocol, S::Routing, C::Routes),
        (404, R::Account) => (K::AccountUnavailable, S::Account, C::Account),
        (409, R::Credit) => (K::CreditInsufficient, S::Admission, C::Credit),
        (409, R::Disabled) => (K::AdmissionDisabled, S::Admission, C::Admission),
        (409, R::Conflict) => (K::Conflict, S::Key, C::Idempotency),
        (409, R::Charge) => (K::Conflict, S::Settlement, C::Charge),
        (409, R::KeyWindow) => (K::Conflict, S::Key, C::KeyWindow),
        (409, R::Overflow) => (K::Conflict, S::Arithmetic, C::MoneyOverflow),
        (409, R::RenewalClosed) if operation == Operation::Renew => {
            (K::Conflict, S::Renewal, C::ActiveReservation)
        }
        (409, R::RenewalPrecondition) if operation == Operation::Renew => (
            K::Conflict,
            S::Renewal,
            C::ExpectedNotAfterAuthoritativeDeadline,
        ),
        (409, R::RenewalNoProgress) if operation == Operation::Renew => {
            (K::Conflict, S::Renewal, C::StrictlyLaterDeadline)
        }
        // An expired code without a validated bound row is not a receipt.
        (409, R::Expired) => (K::EvidenceUnavailable, S::Settlement, C::ImmutableDeadline),
        (410, R::Evidence) => (K::EvidenceUnavailable, S::Lookup, C::Evidence),
        (503, R::Writer) => (K::RemoteUnavailable, S::WriterGate, C::WriterWait),
        (503, R::Work) => (K::RemoteUnavailable, S::TransactionWork, C::WorkBudget),
        (503, R::Begin) => (
            K::RemoteUnavailable,
            S::TransactionBegin,
            C::TransactionStarted,
        ),
        (503, R::Storage) => (K::RemoteUnavailable, S::TransactionWork, C::UsableStorage),
        (503, R::Random) => (
            K::RemoteUnavailable,
            S::ReservationIdentity,
            C::UniqueIdentifier,
        ),
        (503, R::Corrupt) => (K::RemoteUnavailable, S::StateValidation, C::ConsistentState),
        (503, R::Commit) => (
            K::RemoteUnavailable,
            S::TransactionCompletion,
            C::CommitEvidence,
        ),
        (503, R::Closed) => (K::RemoteUnavailable, S::Readiness, C::OpenStorage),
        (503, R::Close) => (K::RemoteUnavailable, S::Shutdown, C::StorageClosure),
        (503, R::Startup) => (K::RemoteUnavailable, S::Startup, C::OriginalStorage),
        (503, R::Missing) => (K::RemoteUnavailable, S::Startup, C::ExistingState),
        (503, R::Existing) => (K::RemoteUnavailable, S::Init, C::EmptyDirectory),
        (503, R::Schema) => (K::RemoteUnavailable, S::StateValidation, C::SupportedSchema),
        (503, R::Path) => (K::RemoteUnavailable, S::Startup, C::StatePath),
        (503, R::Routes) => (K::RemoteUnavailable, S::Startup, C::Routes),
        (503, R::Read) => (K::RemoteUnavailable, S::RequestRead, C::CompleteBody),
        (503, R::Dispatch) => (
            K::RemoteUnavailable,
            S::RequestDispatch,
            C::RemoteCauseUnknown,
        ),
        (503, R::Response) => (K::RemoteUnavailable, S::ResponseEncoding, C::ClosedResponse),
        _ if (500..600).contains(&status) => {
            (K::RemoteUnavailable, S::Remote, C::RemoteCauseUnknown)
        }
        _ => (K::Protocol, S::Remote, C::RemoteCauseUnknown),
    };
    Error::response(operation, status, kind, stage, constraint)
}

/// Closed synthetic outcomes for composed gateway cutoff tests only.
#[cfg(test)]
pub(crate) mod cutoff_fixture {
    use super::*;

    pub(crate) fn commit_unknown() -> Error {
        classify_remote(RemoteCode::Commit, 503, Operation::Settle)
    }

    pub(crate) fn renewal_commit_unknown() -> Error {
        classify_remote(RemoteCode::Commit, 503, Operation::Renew)
    }

    pub(crate) fn renewal(
        original: &Reservation,
        elapsed: u64,
        outcome: Option<TerminalOutcome>,
        disabled: bool,
    ) -> RenewalReply {
        let observed = Millis(
            original.initial_expires_at.0 - i64::from(original.timeout_seconds) * 1000
                + elapsed as i64 * 1000,
        );
        let expires = if outcome.is_some() {
            original.expires_at
        } else {
            Millis(observed.0 + i64::from(original.timeout_seconds) * 1000)
        };
        let terminal = outcome.map(|o| {
            if o == TerminalOutcome::Expired {
                expires
            } else {
                observed
            }
        });
        let current = Reservation {
            reservation_id: original.reservation_id.clone(),
            account_id: original.account_id.clone(),
            maximum: original.maximum,
            timeout_seconds: original.timeout_seconds,
            idempotency_key: original.idempotency_key.clone(),
            initial_expires_at: original.initial_expires_at,
            expires_at: expires,
            state: match outcome {
                None => State::Active,
                Some(TerminalOutcome::Expired) => State::Expired,
                Some(TerminalOutcome::Refunded) => State::Refunded,
                Some(TerminalOutcome::Settled { .. }) => State::Settled,
            },
            charged_amount: outcome.map(|o| match o {
                TerminalOutcome::Settled { charged } => charged,
                _ => 0,
            }),
            terminal_at: terminal,
            retain_until: terminal
                .map(|t| Millis(key_issued(&original.idempotency_key).unwrap().max(t.0) + DAY_MS)),
        };
        let refusal = if outcome.is_some() {
            Some(RemoteCode::RenewalClosed)
        } else if disabled {
            Some(RemoteCode::Disabled)
        } else {
            None
        };
        check_renew(
            original,
            current,
            observed,
            refusal,
            if refusal.is_some() { 409 } else { 200 },
        )
        .unwrap_or_else(|_| panic!("synthetic renewal evidence"))
    }

    pub(crate) fn expired(original: &Reservation) -> TerminalReceipt {
        assert!(original.valid() && original.is_active());
        let current = Reservation {
            reservation_id: original.reservation_id.clone(),
            account_id: original.account_id.clone(),
            maximum: original.maximum,
            timeout_seconds: original.timeout_seconds,
            idempotency_key: original.idempotency_key.clone(),
            initial_expires_at: original.initial_expires_at,
            expires_at: original.expires_at,
            state: State::Expired,
            charged_amount: Some(0),
            terminal_at: Some(original.expires_at),
            retain_until: Some(Millis(
                key_issued(&original.idempotency_key)
                    .unwrap()
                    .max(original.expires_at.0)
                    + DAY_MS,
            )),
        };
        // Exercise the existing bound-row validator and terminal diagnostic;
        // never deserialize unchecked diagnostic metadata into a receipt.
        check_terminal(original, current, Some(29), 409).unwrap()
    }
}

#[cfg(test)]
#[path = "protocol_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "diagnostic_protocol_tests.rs"]
mod diagnostic_tests;
