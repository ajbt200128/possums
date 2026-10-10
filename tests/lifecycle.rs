use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use possums::{
    accounting::{Accounting, AccountingError, FinalUsage, Outcome, ReserveResult},
    auth::{AdmissionError, Auth},
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

fn usage() -> FinalUsage {
    FinalUsage {
        input_tokens: 1,
        output_tokens: 1,
        total_tokens: 2,
    }
}

#[test]
fn process_epoch_invalidates_old_submission_tokens() {
    let credential = URL_SAFE_NO_PAD.encode([7_u8; 32]);
    let first = Auth::from_json(&auth_config()).unwrap();
    let challenge = first.issue_api_challenge().unwrap();
    let (session_id, session) = first.authenticate_api(&credential, &challenge).unwrap();
    let token = first.issue_api_submission(&session_id, "m", false).unwrap();
    let ledger = Accounting::new(first.account_budgets());
    let submission = first
        .admit_submission(
            &ledger,
            &session_id,
            &session.admission_binding,
            &token,
            quote(),
        )
        .unwrap()
        .submission;
    assert_eq!(ledger.available("a"), Some(74));
    drop(ledger); // Demo restart loses reservations; it is not durable accounting.

    let restarted = Auth::from_json(&auth_config()).unwrap();
    let restarted_ledger = Accounting::new(restarted.account_budgets());
    assert_eq!(restarted_ledger.available("a"), Some(100));
    assert_eq!(
        restarted_ledger.finish(submission.id, None),
        Err(AccountingError::InvalidTransition)
    );
    let challenge = restarted.issue_api_challenge().unwrap();
    let (new_session_id, new_session) =
        restarted.authenticate_api(&credential, &challenge).unwrap();
    assert!(restarted
        .admit_submission(
            &restarted_ledger,
            &new_session_id,
            &new_session.admission_binding,
            &token,
            quote()
        )
        .is_err());
}

#[test]
fn four_reservations_survive_reset_and_logout_until_old_work_finishes_once() {
    for logout in [false, true] {
        let auth = Auth::from_json(&auth_config()).unwrap();
        let ledger = Accounting::new([("a".into(), 130)]);
        let credential = URL_SAFE_NO_PAD.encode([7_u8; 32]);
        let (mut id, mut session) = auth
            .authenticate_api(&credential, &auth.issue_api_challenge().unwrap())
            .unwrap();
        let mut accepted = Vec::new();
        for _ in 0..4 {
            let token = auth.issue_api_submission(&id, "m", false).unwrap();
            accepted.push(
                auth.admit_submission(&ledger, &id, &session.admission_binding, &token, quote())
                    .unwrap()
                    .submission,
            );
            if logout {
                auth.logout_api(&id).unwrap();
                (id, session) = auth
                    .authenticate_api(&credential, &auth.issue_api_challenge().unwrap())
                    .unwrap();
            } else {
                auth.issue_api_submission(&id, "m", true).unwrap();
                session = auth.api_session(&id).unwrap();
            }
        }
        assert_eq!(ledger.available("a"), Some(26));
        // A fifth credit-backed reservation is accepted despite old obligations.
        let fifth_token = auth.issue_api_submission(&id, "m", false).unwrap();
        let fifth = auth
            .admit_submission(
                &ledger,
                &id,
                &session.admission_binding,
                &fifth_token,
                quote(),
            )
            .unwrap()
            .submission;
        accepted.push(fifth);
        auth.issue_api_submission(&id, "m", true).unwrap();
        let token = auth.issue_api_submission(&id, "new", true).unwrap();
        session = auth.api_session(&id).unwrap();
        let mut new_quote = quote();
        new_quote.model.id = "new".into();
        assert_eq!(
            auth.admit_submission(
                &ledger,
                &id,
                &session.admission_binding,
                &token,
                new_quote.clone()
            )
            .unwrap_err(),
            AdmissionError::Accounting(AccountingError::InsufficientCredit)
        );
        assert_eq!(
            auth.api_session(&id).unwrap().selected_model.as_deref(),
            Some("new")
        );
        // An old accepted reservation may settle after reset/logout. Auth state
        // is not needed for either terminal outcome and cannot erase obligations.
        for terminal in [Some(usage()), Some(usage()), None] {
            assert_eq!(
                ledger.finish(accepted[0].id, terminal).unwrap(),
                Outcome::Settled { charged: 3 }
            );
        }
        assert_eq!(ledger.available("a"), Some(23));
        // The previously rejected token/model was not half-bound: another model
        // can now use it, without needing a reset to clear failed admission.
        assert_eq!(
            ledger.finish(accepted[1].id, None).unwrap(),
            Outcome::Refunded
        );
        new_quote.model.id = "new".into();
        let new = auth
            .admit_submission(&ledger, &id, &session.admission_binding, &token, new_quote)
            .unwrap();
        assert_eq!(new.result, ReserveResult::Reserved);
        for old in &accepted[1..] {
            assert_eq!(ledger.finish(old.id, None).unwrap(), Outcome::Refunded);
            assert_eq!(ledger.finish(old.id, None).unwrap(), Outcome::Refunded);
        }
        assert_eq!(
            ledger.finish(new.submission.id, None).unwrap(),
            Outcome::Refunded
        );
        assert_eq!(ledger.available("a"), Some(127));
        let current = auth.api_session(&id).unwrap();
        assert_eq!(current.conversation, session.conversation);
        assert_eq!(current.selected_model.as_deref(), Some("new"));
    }
}

#[test]
fn api_restart_and_reauthentication_never_revive_old_tokens() {
    let credential = URL_SAFE_NO_PAD.encode([7_u8; 32]);
    let auth = Auth::from_json(&auth_config()).unwrap();
    let (id, session) = auth
        .authenticate_api(&credential, &auth.issue_api_challenge().unwrap())
        .unwrap();
    let token = auth.issue_api_submission(&id, "m", false).unwrap();
    auth.logout_api(&id).unwrap();
    for current in [&auth, &Auth::from_json(&auth_config()).unwrap()] {
        let ledger = Accounting::new(current.account_budgets());
        assert!(current.api_session(&id).is_none());
        let (next_id, next_session) = current
            .authenticate_api(&credential, &current.issue_api_challenge().unwrap())
            .unwrap();
        assert!(current
            .admit_submission(&ledger, &id, &session.admission_binding, &token, quote())
            .is_err());
        assert!(current
            .admit_submission(
                &ledger,
                &next_id,
                &next_session.admission_binding,
                &token,
                quote()
            )
            .is_err());
        assert_eq!(ledger.available("a"), Some(100));
    }
}

#[test]
fn api_credit_backed_reservations_survive_reset_logout_and_absorbing_terminal_outcomes() {
    for logout in [false, true] {
        let auth = Auth::from_json(&auth_config()).unwrap();
        let credential = URL_SAFE_NO_PAD.encode([7_u8; 32]);
        let (mut id, mut session) = auth
            .authenticate_api(&credential, &auth.issue_api_challenge().unwrap())
            .unwrap();
        let ledger = Accounting::new([("a".into(), 104)]);
        let mut accepted = Vec::new();
        for _ in 0..4 {
            let token = auth.issue_api_submission(&id, "m", true).unwrap();
            accepted.push(
                auth.admit_submission(&ledger, &id, &session.admission_binding, &token, quote())
                    .unwrap()
                    .submission,
            );
            if logout {
                auth.logout_api(&id).unwrap();
                (id, session) = auth
                    .authenticate_api(&credential, &auth.issue_api_challenge().unwrap())
                    .unwrap();
            }
        }
        let token = auth.issue_api_submission(&id, "m", true).unwrap();
        assert_eq!(
            auth.admit_submission(&ledger, &id, &session.admission_binding, &token, quote())
                .unwrap_err(),
            AdmissionError::Accounting(AccountingError::InsufficientCredit)
        );
        for (index, submission) in accepted.into_iter().enumerate() {
            let terminal = if index == 0 { Some(usage()) } else { None };
            let outcome = ledger.finish(submission.id, terminal).unwrap();
            assert_eq!(
                ledger.finish(submission.id, Some(usage())).unwrap(),
                outcome
            );
            assert_eq!(ledger.finish(submission.id, None).unwrap(), outcome);
        }
        assert_eq!(ledger.available("a"), Some(101));
    }
}

#[test]
fn failed_stream_refund_is_terminal_and_idempotent() {
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
    for terminal in [None, None, Some(usage())] {
        assert_eq!(ledger.finish([1; 32], terminal).unwrap(), Outcome::Refunded);
    }
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
