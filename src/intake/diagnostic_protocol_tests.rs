use super::*;
use serde_json::json;

#[test]
fn error_conversion_preserves_protocol_codes_and_remote_constraints() {
    for kind in [
        ErrorKind::Authentication,
        ErrorKind::MfaUnsupported,
        ErrorKind::BindingChanged,
        ErrorKind::Input,
        ErrorKind::AccountUnavailable,
        ErrorKind::CreditInsufficient,
        ErrorKind::AdmissionDisabled,
        ErrorKind::Conflict,
        ErrorKind::EvidenceUnavailable,
        ErrorKind::RemoteUnavailable,
        ErrorKind::Protocol,
        ErrorKind::Transport,
    ] {
        let error = Error::response(
            Operation::Refresh,
            200,
            kind,
            Stage::Binding,
            Constraint::PrincipalBinding,
        );
        let value = serde_json::to_value(error.diagnostic()).unwrap();
        assert_eq!(value["code"], error.code());
        assert_eq!(value["operation"], error.operation());
        assert_eq!(value["stage"], "binding");
        assert_eq!(value["constraint"], "principal_binding");
        assert_eq!(value["status"], 200);
        assert_eq!(value.as_object().unwrap().len(), 5);
    }
    for (code, stage, constraint) in [
        (RemoteCode::Writer, "writer_gate", "writer_wait"),
        (
            RemoteCode::Commit,
            "transaction_completion",
            "commit_evidence",
        ),
    ] {
        let error = classify_remote(code, 503, Operation::Reserve);
        assert_eq!(
            serde_json::to_value(error.diagnostic()).unwrap(),
            json!({
                "code":"INTAKE_REMOTE_UNAVAILABLE", "operation":"intake_reserve",
                "stage":stage, "constraint":constraint, "status":503
            })
        );
    }
}

#[test]
fn renewal_diagnostics_keep_actual_operation_status_and_closed_constraints() {
    for (code, constraint) in [
        (RemoteCode::RenewalClosed, "active_reservation"),
        (
            RemoteCode::RenewalPrecondition,
            "expected_not_after_authoritative_deadline",
        ),
        (RemoteCode::RenewalNoProgress, "strictly_later_deadline"),
    ] {
        let error = classify_remote(code, 409, Operation::Renew);
        assert_eq!(
            serde_json::to_value(error.diagnostic()).unwrap(),
            json!({
                "code":"INTAKE_REQUEST_CONFLICT", "operation":"intake_renew",
                "stage":"renewal", "constraint":constraint, "status":409
            })
        );
        assert_eq!(
            classify_remote(code, 400, Operation::Renew).kind,
            ErrorKind::Protocol
        );
        assert_eq!(
            classify_remote(code, 409, Operation::Settle).kind,
            ErrorKind::Protocol
        );
    }
    let error = Error::response(
        Operation::Renew,
        200,
        ErrorKind::Protocol,
        Stage::Renewal,
        Constraint::RenewalObservation,
    );
    let value = serde_json::to_value(error.diagnostic()).unwrap();
    assert_eq!(value["constraint"], "renewal_observation");
    for outcome in [
        TerminalOutcome::Settled { charged: 7 },
        TerminalOutcome::Refunded,
        TerminalOutcome::Expired,
    ] {
        let diagnostic =
            crate::intake::diagnostics::Diagnostic::terminal(outcome, false, Operation::Renew, 409)
                .unwrap();
        let value = serde_json::to_value(diagnostic).unwrap();
        assert_eq!(value["operation"], "intake_renew");
        assert_eq!(value["stage"], "renewal");
        assert_eq!(value["status"], 409);
        assert_eq!(value.as_object().unwrap().len(), 5);
    }
}

#[test]
fn every_transport_constraint_survives_the_public_projection() {
    use super::transport::Constraint as C;
    for (constraint, label) in [
        (C::Configuration, "configuration"),
        (C::Input, "input"),
        (C::EncodingLimit, "encoding_limit"),
        (C::EncodingUnknown, "encoding_unknown"),
        (C::ClientUnknown, "client_unknown"),
        (C::RequestUnknown, "request_unknown"),
        (C::ConnectUnknown, "connect_unknown"),
        (C::TransportUnknown, "transport_unknown"),
        (C::Deadline, "deadline"),
        (C::ResponseEncoding, "response_encoding"),
        (C::ResponseLimit, "response_limit"),
        (C::ResponseReadUnknown, "response_read_unknown"),
        (C::ResponseJson, "response_json"),
    ] {
        let status = matches!(
            constraint,
            C::ResponseEncoding | C::ResponseLimit | C::ResponseReadUnknown | C::ResponseJson
        )
        .then_some(200);
        let error = Error::from(transport::Failure {
            operation: Operation::Reserve,
            constraint,
            status,
            dispatch_attempted: status.is_some(),
        });
        let value = serde_json::to_value(error.diagnostic()).unwrap();
        assert_eq!(value["constraint"], label);
        assert_eq!(value["status"], serde_json::to_value(status).unwrap());
        assert_eq!(value["code"], error.code());
        assert_eq!(value["operation"], "intake_reserve");
        assert_eq!(value.as_object().unwrap().len(), 5);
    }
}

#[test]
fn terminal_diagnostics_keep_observed_status_and_never_change_outcomes() {
    use crate::intake::diagnostics::Diagnostic;
    for (outcome, operation, status, code, constraint) in [
        (
            TerminalOutcome::Expired,
            Operation::Settle,
            409,
            "INTAKE_HOLD_EXPIRED",
            "immutable_hold_expiry",
        ),
        (
            TerminalOutcome::Settled { charged: 7 },
            Operation::Refund,
            200,
            "INTAKE_TERMINAL_MISMATCH",
            "first_terminal_wins",
        ),
    ] {
        let diagnostic = Diagnostic::terminal(outcome, false, operation, status).unwrap();
        let value = serde_json::to_value(diagnostic).unwrap();
        assert_eq!(value["code"], code);
        assert_eq!(value["constraint"], constraint);
        assert_eq!(value["status"], status);
        assert_eq!(value["stage"], "settlement");
        assert_eq!(value.as_object().unwrap().len(), 5);
    }
    assert!(
        Diagnostic::terminal(TerminalOutcome::Refunded, true, Operation::Refund, 200).is_none()
    );
}
