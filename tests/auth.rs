use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use possums::{
    accounting::{Accounting, AccountingError, Outcome, ReserveResult},
    auth::{session_cookie, AdmissionError, Auth, AuthError},
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
fn accepts_high_entropy_credential_and_sets_hardened_cookie() {
    let (auth, credential) = fixture();
    let challenge = auth.issue_login_challenge().unwrap();
    let (id, session) = auth.authenticate(&credential, &challenge).unwrap();
    assert_eq!(auth.session(&id).unwrap().account_id, "demo");
    assert!(Auth::verify_csrf(&session, &session.csrf));
    let cookie = session_cookie(&id);
    assert!(cookie.contains("Secure"));
    assert!(cookie.contains("HttpOnly"));
    assert!(cookie.contains("SameSite=Strict"));
}

#[test]
fn reset_rotates_conversation_without_reauthentication_or_old_continuations() {
    let (auth, credential) = fixture();
    let (id, original) = auth
        .authenticate(&credential, &auth.issue_login_challenge().unwrap())
        .unwrap();
    let ledger = Accounting::new(auth.account_budgets());
    let token = auth.issue_submission(&id).unwrap();
    let old = auth
        .admit_submission(&ledger, &id, &original.csrf, &token, quote("a"))
        .unwrap();
    let continuation = auth
        .issue_submission_for(&id, old.submission.conversation, Some("a"))
        .unwrap();
    for csrf in ["", "forged"] {
        assert_eq!(auth.new_chat(&id, csrf), Err(AuthError::Invalid));
        assert_eq!(auth.logout(&id, csrf), Err(AuthError::Invalid));
        assert_eq!(
            auth.session(&id).unwrap().conversation,
            original.conversation
        );
    }
    auth.new_chat(&id, &original.csrf).unwrap();
    let current = auth.session(&id).unwrap();
    assert_ne!(current.conversation, original.conversation);
    assert_eq!(current.csrf, original.csrf);
    assert_eq!(current.selected_model, None);
    assert_eq!(ledger.available("demo"), Some(974));
    for stale in [&token, &continuation] {
        assert!(auth
            .admit_submission(&ledger, &id, &current.csrf, stale, quote("a"))
            .is_err());
    }
    assert_eq!(
        auth.issue_submission_for(&id, original.conversation, Some("a")),
        Err(AuthError::Invalid)
    );
    assert_eq!(
        auth.issue_submission_for(&id, original.conversation, None),
        Err(AuthError::Invalid)
    );
    let next = auth
        .issue_submission_for(&id, current.conversation, None)
        .unwrap();
    assert!(auth
        .admit_submission(&ledger, &id, &current.csrf, &next, quote("b"))
        .is_ok());
    assert_eq!(
        ledger.finish(old.submission.id, None).unwrap(),
        Outcome::Refunded
    );
    assert_eq!(
        ledger.finish(old.submission.id, None).unwrap(),
        Outcome::Refunded
    );
    assert_eq!(
        auth.session(&id).unwrap().selected_model.as_deref(),
        Some("b")
    );
    auth.logout(&id, &current.csrf).unwrap();
    assert!(auth
        .issue_submission_for(&id, current.conversation, Some("b"))
        .is_err());
}

#[test]
fn failed_admission_does_not_bind_token_or_model_and_duplicates_do_not_reserve() {
    let (auth, credential) = fixture();
    let (id, session) = auth
        .authenticate(&credential, &auth.issue_login_challenge().unwrap())
        .unwrap();
    let ledger = Accounting::new(auth.account_budgets());
    let token = auth.issue_submission(&id).unwrap();
    assert!(auth
        .admit_submission(&ledger, &id, "forged", &token, quote("a"))
        .is_err());
    let mut expensive = quote("a");
    expensive.reserved_microunits = 1001;
    assert_eq!(
        auth.admit_submission(&ledger, &id, &session.csrf, &token, expensive)
            .unwrap_err(),
        AdmissionError::Accounting(AccountingError::InsufficientCredit)
    );
    assert_eq!(auth.session(&id).unwrap().selected_model, None);
    assert_eq!(ledger.available("demo"), Some(1000));
    let accepted = auth
        .admit_submission(&ledger, &id, &session.csrf, &token, quote("b"))
        .unwrap();
    assert_eq!(accepted.result, ReserveResult::Reserved);
    let duplicate = auth
        .admit_submission(&ledger, &id, &session.csrf, &token, quote("b"))
        .unwrap();
    assert_eq!(
        duplicate.result,
        ReserveResult::Duplicate(Outcome::InFlight)
    );
    assert_eq!(accepted.submission.id, duplicate.submission.id);
    assert_eq!(ledger.available("demo"), Some(974));
}

#[test]
fn rejects_short_oversized_or_wrong_credentials() {
    let (auth, _) = fixture();
    let challenge = auth.issue_login_challenge().unwrap();
    assert_eq!(
        auth.authenticate("short", &challenge).unwrap_err(),
        AuthError::Invalid
    );
    let challenge = auth.issue_login_challenge().unwrap();
    assert_eq!(
        auth.authenticate(&"A".repeat(1_000_000), &challenge)
            .unwrap_err(),
        AuthError::Invalid
    );
    let challenge = auth.issue_login_challenge().unwrap();
    assert_eq!(
        auth.authenticate(&URL_SAFE_NO_PAD.encode([9_u8; 32]), &challenge)
            .unwrap_err(),
        AuthError::Invalid
    );
}

#[test]
fn rejects_replayed_login_and_forged_cross_session_or_altered_model_tokens() {
    let (auth, credential) = fixture();
    let challenge = auth.issue_login_challenge().unwrap();
    let (first_id, first) = auth.authenticate(&credential, &challenge).unwrap();
    assert_eq!(
        auth.authenticate(&credential, &challenge).unwrap_err(),
        AuthError::Invalid
    );

    let ledger = Accounting::new(auth.account_budgets());
    let token = auth.issue_submission(&first_id).unwrap();
    let admit = |id: &str, csrf: &str, token: &str, model: &str| {
        auth.admit_submission(&ledger, id, csrf, token, quote(model))
    };
    assert_eq!(
        admit(&first_id, &first.csrf, &token, "model-a")
            .unwrap()
            .result,
        ReserveResult::Reserved
    );
    assert_eq!(
        admit(&first_id, &first.csrf, &token, "model-b").unwrap_err(),
        AdmissionError::Auth(AuthError::Invalid)
    );
    assert_eq!(
        admit(&first_id, &first.csrf, "forged", "model-a").unwrap_err(),
        AdmissionError::Auth(AuthError::Invalid)
    );
    let next_token = auth.issue_submission(&first_id).unwrap();
    assert_eq!(
        admit(&first_id, &first.csrf, &next_token, "model-b").unwrap_err(),
        AdmissionError::Auth(AuthError::Invalid)
    );
    assert_eq!(
        auth.session(&first_id).unwrap().selected_model.as_deref(),
        Some("model-a")
    );

    let challenge = auth.issue_login_challenge().unwrap();
    let (second_id, second) = auth.authenticate(&credential, &challenge).unwrap();
    assert_eq!(
        admit(&second_id, &second.csrf, &token, "model-a").unwrap_err(),
        AdmissionError::Auth(AuthError::Invalid)
    );
}
