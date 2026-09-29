use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use possums::{
    accounting::{Accounting, AccountingError, Outcome, ReserveResult},
    auth::Auth,
    catalog::{Model, Quote},
};
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};

fn auth_config() -> String {
    let credential = URL_SAFE_NO_PAD.encode([7_u8; 32]);
    let hash = URL_SAFE_NO_PAD.encode(Sha256::digest(credential.as_bytes()));
    format!(r#"[{{"id":"a","credential_sha256":"{hash}","demo_microunits":100}}]"#)
}

fn quote() -> Quote {
    Quote {
        model: Model {
            id: "m".into(),
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
fn process_epoch_invalidates_old_submission_tokens() {
    let credential = URL_SAFE_NO_PAD.encode([7_u8; 32]);
    let first = Auth::from_json(&auth_config()).unwrap();
    let challenge = first.issue_login_challenge().unwrap();
    let (session_id, session) = first.authenticate(&credential, &challenge).unwrap();
    let token = first.issue_submission(&session_id).unwrap();
    let submission = first
        .bind_submission(&session_id, &session.account_id, &token, "m")
        .unwrap();
    let ledger = Accounting::new(first.account_budgets());
    ledger
        .reserve(
            &session.account_id,
            submission.id,
            [2; 32],
            quote(),
            submission.expires_at,
        )
        .unwrap();
    assert_eq!(ledger.available("a"), Some(74));
    drop(ledger); // Demo restart loses reservations; it is not durable accounting.

    let restarted = Auth::from_json(&auth_config()).unwrap();
    let restarted_ledger = Accounting::new(restarted.account_budgets());
    assert_eq!(restarted_ledger.available("a"), Some(100));
    assert_eq!(
        restarted_ledger.finish(submission.id, None),
        Err(AccountingError::InvalidTransition)
    );
    let challenge = restarted.issue_login_challenge().unwrap();
    let (new_session_id, new_session) = restarted.authenticate(&credential, &challenge).unwrap();
    assert!(restarted
        .bind_submission(&new_session_id, &new_session.account_id, &token, "m")
        .is_err());
}

#[test]
fn abandoned_delivery_refund_is_terminal_and_idempotent() {
    let ledger = Accounting::new([("a".into(), 100)]);
    ledger
        .reserve(
            "a",
            [1; 32],
            [2; 32],
            quote(),
            Instant::now() + Duration::from_secs(60),
        )
        .unwrap();
    ledger.refund([1; 32]).unwrap();
    ledger.refund([1; 32]).unwrap();
    assert_eq!(ledger.available("a"), Some(100));
    assert_eq!(
        ledger
            .reserve(
                "a",
                [1; 32],
                [2; 32],
                quote(),
                Instant::now() + Duration::from_secs(60),
            )
            .unwrap(),
        ReserveResult::Duplicate(Outcome::Refunded)
    );
}
