//! Request-only, nonauthorizing failure metadata. No raw errors or private data.
use super::{
    protocol::{Constraint, ErrorKind, Stage, TerminalOutcome},
    transport::Operation,
};
use axum::http::HeaderMap;
use serde::Serialize;

pub(crate) const HEADER: &str = "x-possums-intake-diagnostics";

pub(crate) fn requested(headers: &HeaderMap) -> bool {
    let mut values = headers.get_all(HEADER).iter();
    values.next().is_some_and(|value| value == "v1") && values.next().is_none()
}

// Names map directly to the stable, component-prefixed public diagnostic codes.
#[allow(clippy::enum_variant_names)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum Code {
    IntakeAuthenticationUnconfirmed,
    IntakeMfaUnsupported,
    IntakeBindingChanged,
    IntakeInputInvalid,
    IntakeAccountUnavailable,
    IntakeCreditInsufficient,
    IntakeAdmissionDisabled,
    IntakeRequestConflict,
    IntakeEvidenceUnavailable,
    IntakeRemoteUnavailable,
    IntakeProtocolUnconfirmed,
    IntakeRequestUnavailable,
    IntakeHoldUnusable,
    IntakeHoldExpired,
    IntakeTerminalMismatch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(crate) enum DiagnosticOperation {
    #[serde(rename = "intake_configuration")]
    Configure,
    #[serde(rename = "intake_login")]
    Login,
    #[serde(rename = "intake_refresh")]
    Refresh,
    #[serde(rename = "intake_auth_methods")]
    AuthMethods,
    #[serde(rename = "intake_reserve")]
    Reserve,
    #[serde(rename = "intake_renew")]
    Renew,
    #[serde(rename = "intake_settle")]
    Settle,
    #[serde(rename = "intake_refund")]
    Refund,
    #[serde(rename = "intake_lookup")]
    Lookup,
    #[serde(rename = "intake_balance")]
    Balance,
}
impl From<Operation> for DiagnosticOperation {
    fn from(operation: Operation) -> Self {
        match operation {
            Operation::Configure => Self::Configure,
            Operation::Login => Self::Login,
            Operation::Refresh => Self::Refresh,
            Operation::AuthMethods => Self::AuthMethods,
            Operation::Reserve => Self::Reserve,
            Operation::Renew => Self::Renew,
            Operation::Settle => Self::Settle,
            Operation::Refund => Self::Refund,
            Operation::Lookup => Self::Lookup,
            Operation::Balance => Self::Balance,
        }
    }
}

// Preserve the original closed transport constraint instead of collapsing it
// into the protocol adapter's broader transport category.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(untagged)]
enum DiagnosticConstraint {
    Protocol(Constraint),
    Transport(super::transport::Constraint),
}

/// Safe to attach to Response extensions; not settlement or dispatch authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Diagnostic {
    code: Code,
    operation: DiagnosticOperation,
    stage: Stage,
    constraint: DiagnosticConstraint,
    status: Option<u16>,
}
impl Diagnostic {
    pub(super) fn protocol(
        kind: ErrorKind,
        operation: Operation,
        stage: Stage,
        constraint: Constraint,
        status: Option<u16>,
    ) -> Self {
        let code = match kind {
            ErrorKind::Authentication => Code::IntakeAuthenticationUnconfirmed,
            ErrorKind::MfaUnsupported => Code::IntakeMfaUnsupported,
            ErrorKind::BindingChanged => Code::IntakeBindingChanged,
            ErrorKind::Input => Code::IntakeInputInvalid,
            ErrorKind::AccountUnavailable => Code::IntakeAccountUnavailable,
            ErrorKind::CreditInsufficient => Code::IntakeCreditInsufficient,
            ErrorKind::AdmissionDisabled => Code::IntakeAdmissionDisabled,
            ErrorKind::Conflict => Code::IntakeRequestConflict,
            ErrorKind::EvidenceUnavailable => Code::IntakeEvidenceUnavailable,
            ErrorKind::RemoteUnavailable => Code::IntakeRemoteUnavailable,
            ErrorKind::Protocol => Code::IntakeProtocolUnconfirmed,
            ErrorKind::Transport => Code::IntakeRequestUnavailable,
        };
        Self {
            code,
            operation: operation.into(),
            stage,
            constraint: DiagnosticConstraint::Protocol(constraint),
            status: status.filter(|s| (100..=599).contains(s)),
        }
    }
    pub(super) fn with_transport_constraint(
        mut self,
        constraint: super::transport::Constraint,
    ) -> Self {
        self.constraint = DiagnosticConstraint::Transport(constraint);
        self
    }

    pub(crate) fn renewal_unknown() -> Self {
        Self {
            code: Code::IntakeRequestUnavailable,
            operation: DiagnosticOperation::Renew,
            stage: Stage::Renewal,
            constraint: DiagnosticConstraint::Protocol(Constraint::RemoteCauseUnknown),
            status: None,
        }
    }

    pub(crate) fn hold_unusable() -> Self {
        Self {
            code: Code::IntakeHoldUnusable,
            operation: DiagnosticOperation::Reserve,
            stage: Stage::Admission,
            constraint: DiagnosticConstraint::Protocol(Constraint::UsableHold),
            status: None,
        }
    }
    pub(super) fn terminal(
        outcome: TerminalOutcome,
        matches: bool,
        operation: Operation,
        status: u16,
    ) -> Option<Self> {
        if matches {
            return None;
        }
        let expired = outcome == TerminalOutcome::Expired;
        Some(Self {
            code: if expired {
                Code::IntakeHoldExpired
            } else {
                Code::IntakeTerminalMismatch
            },
            operation: operation.into(),
            stage: if operation == Operation::Renew {
                Stage::Renewal
            } else {
                Stage::Settlement
            },
            constraint: DiagnosticConstraint::Protocol(if expired {
                Constraint::ImmutableHoldExpiry
            } else {
                Constraint::FirstTerminalWins
            }),
            status: (100..=599).contains(&status).then_some(status),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capability_is_exact_and_singleton() {
        let mut headers = HeaderMap::new();
        assert!(!requested(&headers));
        for value in ["v2", "V1", "v1, v1", "v1 ", "true"] {
            headers.insert(HEADER, value.parse().unwrap());
            assert!(!requested(&headers));
        }
        headers.insert(HEADER, "v1".parse().unwrap());
        assert!(requested(&headers));
        headers.append(HEADER, "v1".parse().unwrap());
        assert!(!requested(&headers));
    }
    #[test]
    fn projection_has_only_closed_fields_and_valid_observed_status() {
        let diagnostic = Diagnostic::protocol(
            ErrorKind::RemoteUnavailable,
            Operation::Reserve,
            Stage::TransactionCompletion,
            Constraint::CommitEvidence,
            Some(503),
        );
        assert_eq!(
            serde_json::to_value(diagnostic).unwrap(),
            serde_json::json!({
                "code":"INTAKE_REMOTE_UNAVAILABLE", "operation":"intake_reserve",
                "stage":"transaction_completion", "constraint":"commit_evidence", "status":503
            })
        );
        let invalid = Diagnostic::protocol(
            ErrorKind::Protocol,
            Operation::Login,
            Stage::Decode,
            Constraint::RequiredSchema,
            Some(600),
        );
        assert_eq!(
            serde_json::to_value(invalid).unwrap()["status"],
            serde_json::Value::Null
        );
    }
}
