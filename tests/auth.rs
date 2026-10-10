use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use possums::{
    accounting::{Accounting, AccountingError, Outcome, ReserveResult},
    auth::{AdmissionError, Auth, AuthError},
    catalog::{Model, Quote},
};
use sha2::{Digest, Sha256};

fn fixture() -> (Auth, String) {
    let credential = URL_SAFE_NO_PAD.encode([7_u8; 32]);
    let hash = URL_SAFE_NO_PAD.encode(Sha256::digest(credential.as_bytes()));
    let auth = Auth::from_json(&format!(
        r#"[{{"id":"demo","credential_sha256":"{hash}","demo_microunits":1000}}]"#
    ))
    .unwrap();
    (auth, credential)
}

fn quote(model: &str) -> Quote {
    Quote {
        model: Model {
            id: model.into(),
            context_tokens: 20,
            max_output_tokens: 10,
            input_microunits_per_million_tokens: 1_000_000,
            output_microunits_per_million_tokens: 1_000_000,
        },
        input_tokens: 10,
        reserved_microunits: 26,
    }
}

#[test]
fn api_credential_and_reset_reject_stale_submissions() {
    let (auth, credential) = fixture();
    let challenge = auth.issue_api_challenge().unwrap();
    let (id, session) = auth.authenticate_api(&credential, &challenge).unwrap();
    assert_eq!(auth.api_session(&id).unwrap().account_id, "demo");
    assert!(Auth::verify_admission_binding(
        &session,
        &session.admission_binding
    ));
    let ledger = Accounting::new(auth.account_budgets());
    let token = auth.issue_api_submission(&id, "a", false).unwrap();
    let accepted = auth
        .admit_submission(&ledger, &id, &session.admission_binding, &token, quote("a"))
        .unwrap();
    let next = auth.issue_api_submission(&id, "b", true).unwrap();
    assert_ne!(
        auth.api_session(&id).unwrap().conversation,
        session.conversation
    );
    assert!(auth
        .admit_submission(&ledger, &id, &session.admission_binding, &token, quote("a"))
        .is_err());
    assert!(auth
        .admit_submission(&ledger, &id, &session.admission_binding, &next, quote("b"))
        .is_ok());
    assert_eq!(
        ledger.finish(accepted.submission.id, None).unwrap(),
        Outcome::Refunded
    );
    auth.logout_api(&id).unwrap();
    assert!(auth.api_session(&id).is_none());
}

#[test]
fn failed_admission_does_not_bind_token_or_model_and_duplicates_do_not_reserve() {
    let (auth, credential) = fixture();
    let (id, session) = auth
        .authenticate_api(&credential, &auth.issue_api_challenge().unwrap())
        .unwrap();
    let ledger = Accounting::new(auth.account_budgets());
    let token = auth.issue_api_submission(&id, "b", false).unwrap();
    assert!(auth
        .admit_submission(&ledger, &id, "forged", &token, quote("a"))
        .is_err());
    let mut expensive = quote("b");
    expensive.reserved_microunits = 1001;
    assert_eq!(
        auth.admit_submission(&ledger, &id, &session.admission_binding, &token, expensive)
            .unwrap_err(),
        AdmissionError::Accounting(AccountingError::InsufficientCredit)
    );
    assert_eq!(
        auth.api_session(&id).unwrap().selected_model.as_deref(),
        Some("b")
    );
    assert_eq!(ledger.available("demo"), Some(1000));
    let accepted = auth
        .admit_submission(&ledger, &id, &session.admission_binding, &token, quote("b"))
        .unwrap();
    assert_eq!(accepted.result, ReserveResult::Reserved);
    let duplicate = auth
        .admit_submission(&ledger, &id, &session.admission_binding, &token, quote("b"))
        .unwrap();
    assert_eq!(
        duplicate.result,
        ReserveResult::Duplicate(Outcome::InFlight)
    );
    assert_eq!(accepted.submission.id, duplicate.submission.id);
    assert_eq!(ledger.available("demo"), Some(974));
}

#[test]
fn api_session_debug_is_redacted() {
    let (auth, credential) = fixture();
    let (id, session) = auth
        .authenticate_api(&credential, &auth.issue_api_challenge().unwrap())
        .unwrap();
    assert_eq!(format!("{session:?}"), "Session { [redacted] }");
    assert!(auth.api_session(&id).is_some());
}

#[test]
fn api_tokens_bind_model_session_and_account_before_first_admission() {
    let credential = URL_SAFE_NO_PAD.encode([7_u8; 32]);
    let other = URL_SAFE_NO_PAD.encode([8_u8; 32]);
    let config = serde_json::json!([credential.clone(), other.clone()].iter().enumerate().map(|(i, c)| serde_json::json!({
        "id": i.to_string(), "credential_sha256": URL_SAFE_NO_PAD.encode(Sha256::digest(c.as_bytes())), "demo_microunits": 1000
    })).collect::<Vec<_>>()).to_string();
    let auth = Auth::from_json(&config).unwrap();
    let ledger = Accounting::new(auth.account_budgets());
    let (id, session) = auth
        .authenticate_api(&credential, &auth.issue_api_challenge().unwrap())
        .unwrap();
    let token = auth.issue_api_submission(&id, "m", false).unwrap();
    assert!(auth
        .admit_submission(
            &ledger,
            &id,
            &session.admission_binding,
            &token,
            quote("other")
        )
        .is_err());
    for credential in [credential, other] {
        let (other_id, other_session) = auth
            .authenticate_api(&credential, &auth.issue_api_challenge().unwrap())
            .unwrap();
        assert!(auth
            .admit_submission(
                &ledger,
                &other_id,
                &other_session.admission_binding,
                &token,
                quote("m")
            )
            .is_err());
    }
    let accepted = auth
        .admit_submission(&ledger, &id, &session.admission_binding, &token, quote("m"))
        .unwrap();
    assert_eq!(accepted.result, ReserveResult::Reserved);
    assert_eq!(
        auth.admit_submission(&ledger, &id, &session.admission_binding, &token, quote("m"))
            .unwrap()
            .result,
        ReserveResult::Duplicate(Outcome::InFlight)
    );
    ledger.finish(accepted.submission.id, None).unwrap();
    assert_eq!(
        auth.admit_submission(&ledger, &id, &session.admission_binding, &token, quote("m"))
            .unwrap()
            .result,
        ReserveResult::Duplicate(Outcome::Refunded)
    );
}

#[test]
fn rejects_short_oversized_or_wrong_credentials() {
    let (auth, _) = fixture();
    let challenge = auth.issue_api_challenge().unwrap();
    assert_eq!(
        auth.authenticate_api("short", &challenge).unwrap_err(),
        AuthError::Invalid
    );
    let challenge = auth.issue_api_challenge().unwrap();
    assert_eq!(
        auth.authenticate_api(&"A".repeat(1_000_000), &challenge)
            .unwrap_err(),
        AuthError::Invalid
    );
    let challenge = auth.issue_api_challenge().unwrap();
    assert_eq!(
        auth.authenticate_api(&URL_SAFE_NO_PAD.encode([9_u8; 32]), &challenge)
            .unwrap_err(),
        AuthError::Invalid
    );
}

#[test]
fn rejects_replayed_login_and_forged_cross_session_or_altered_model_tokens() {
    let (auth, credential) = fixture();
    let challenge = auth.issue_api_challenge().unwrap();
    let (first_id, first) = auth.authenticate_api(&credential, &challenge).unwrap();
    assert_eq!(
        auth.authenticate_api(&credential, &challenge).unwrap_err(),
        AuthError::Invalid
    );

    let ledger = Accounting::new(auth.account_budgets());
    let token = auth
        .issue_api_submission(&first_id, "model-a", false)
        .unwrap();
    let admit = |id: &str, admission_binding: &str, token: &str, model: &str| {
        auth.admit_submission(&ledger, id, admission_binding, token, quote(model))
    };
    assert_eq!(
        admit(&first_id, &first.admission_binding, &token, "model-a")
            .unwrap()
            .result,
        ReserveResult::Reserved
    );
    assert_eq!(
        admit(&first_id, &first.admission_binding, &token, "model-b").unwrap_err(),
        AdmissionError::Auth(AuthError::Invalid)
    );
    assert_eq!(
        admit(&first_id, &first.admission_binding, "forged", "model-a").unwrap_err(),
        AdmissionError::Auth(AuthError::Invalid)
    );
    let next_token = auth
        .issue_api_submission(&first_id, "model-a", false)
        .unwrap();
    assert_eq!(
        admit(&first_id, &first.admission_binding, &next_token, "model-b").unwrap_err(),
        AdmissionError::Auth(AuthError::Invalid)
    );
    assert_eq!(
        auth.api_session(&first_id)
            .unwrap()
            .selected_model
            .as_deref(),
        Some("model-a")
    );

    let challenge = auth.issue_api_challenge().unwrap();
    let (second_id, second) = auth.authenticate_api(&credential, &challenge).unwrap();
    assert_eq!(
        admit(&second_id, &second.admission_binding, &token, "model-a").unwrap_err(),
        AdmissionError::Auth(AuthError::Invalid)
    );
}
