use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use possums::{
    accounting::{Accounting, AccountingError, FinalUsage, Outcome, ReserveResult},
    auth::{AdmissionError, Auth},
    catalog::{Model, Quote},
};
use sha2::{Digest, Sha256};
use std::{
    sync::Barrier,
    thread,
    time::{Duration, Instant},
};

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
    let challenge = first.issue_login_challenge().unwrap();
    let (session_id, session) = first.authenticate(&credential, &challenge).unwrap();
    let token = first.issue_submission(&session_id).unwrap();
    let ledger = Accounting::new(first.account_budgets());
    let submission = first
        .admit_submission(&ledger, &session_id, &session.csrf, &token, quote())
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
    let challenge = restarted.issue_login_challenge().unwrap();
    let (new_session_id, new_session) = restarted.authenticate(&credential, &challenge).unwrap();
    assert!(restarted
        .admit_submission(
            &restarted_ledger,
            &new_session_id,
            &new_session.csrf,
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
            .authenticate(&credential, &auth.issue_login_challenge().unwrap())
            .unwrap();
        let mut accepted = Vec::new();
        for _ in 0..4 {
            let token = auth.issue_submission(&id).unwrap();
            accepted.push(
                auth.admit_submission(&ledger, &id, &session.csrf, &token, quote())
                    .unwrap()
                    .submission,
            );
            if logout {
                auth.logout(&id, &session.csrf).unwrap();
                (id, session) = auth
                    .authenticate(&credential, &auth.issue_login_challenge().unwrap())
                    .unwrap();
            } else {
                auth.new_chat(&id, &session.csrf).unwrap();
                session = auth.session(&id).unwrap();
            }
        }
        assert_eq!(ledger.available("a"), Some(26));
        // A fifth credit-backed reservation is accepted despite old obligations.
        let fifth_token = auth.issue_submission(&id).unwrap();
        let fifth = auth
            .admit_submission(&ledger, &id, &session.csrf, &fifth_token, quote())
            .unwrap()
            .submission;
        accepted.push(fifth);
        auth.new_chat(&id, &session.csrf).unwrap();
        session = auth.session(&id).unwrap();
        let token = auth.issue_submission(&id).unwrap();
        let mut new_quote = quote();
        new_quote.model.id = "new".into();
        assert_eq!(
            auth.admit_submission(&ledger, &id, &session.csrf, &token, new_quote.clone())
                .unwrap_err(),
            AdmissionError::Accounting(AccountingError::InsufficientCredit)
        );
        assert_eq!(auth.session(&id).unwrap().selected_model, None);
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
        new_quote.model.id = "other".into();
        let new = auth
            .admit_submission(&ledger, &id, &session.csrf, &token, new_quote)
            .unwrap();
        assert_eq!(new.result, ReserveResult::Reserved);
        for old in &accepted[1..] {
            assert_eq!(ledger.finish(old.id, None).unwrap(), Outcome::Refunded);
            assert_eq!(ledger.finish(old.id, None).unwrap(), Outcome::Refunded);
            assert!(auth
                .issue_submission_for(&id, old.conversation, Some("m"))
                .is_err());
        }
        assert_eq!(
            ledger.finish(new.submission.id, None).unwrap(),
            Outcome::Refunded
        );
        assert_eq!(ledger.available("a"), Some(127));
        let current = auth.session(&id).unwrap();
        assert_eq!(current.conversation, session.conversation);
        assert_eq!(current.selected_model.as_deref(), Some("other"));
    }
}

#[test]
fn concurrent_completions_issue_only_original_conversation_continuations() {
    let auth = Auth::from_json(&auth_config()).unwrap();
    let ledger = Accounting::new(auth.account_budgets());
    let credential = URL_SAFE_NO_PAD.encode([7_u8; 32]);
    let (id, session) = auth
        .authenticate(&credential, &auth.issue_login_challenge().unwrap())
        .unwrap();
    let accepted: Vec<_> = (0..3)
        .map(|_| {
            let token = auth.issue_submission(&id).unwrap();
            auth.admit_submission(&ledger, &id, &session.csrf, &token, quote())
                .unwrap()
                .submission
        })
        .collect();
    let start = Barrier::new(4);
    let tokens = thread::scope(|scope| {
        let handles: Vec<_> = accepted
            .iter()
            .map(|submission| {
                scope.spawn(|| {
                    assert_eq!(
                        ledger.finish(submission.id, Some(usage())).unwrap(),
                        Outcome::Settled { charged: 3 }
                    );
                    start.wait();
                    auth.issue_submission_for(&id, submission.conversation, Some("m"))
                        .unwrap()
                })
            })
            .collect();
        start.wait();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(
        tokens
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        3
    );
    assert_eq!(ledger.available("a"), Some(91));
    auth.new_chat(&id, &session.csrf).unwrap();
    for token in tokens {
        assert!(auth
            .admit_submission(&ledger, &id, &session.csrf, &token, quote())
            .is_err());
    }
    assert_eq!(ledger.available("a"), Some(91));
    assert_eq!(auth.session(&id).unwrap().selected_model, None);
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
            .admit_submission(&ledger, &id, &session.csrf, &token, quote())
            .is_err());
        assert!(current
            .admit_submission(&ledger, &next_id, &next_session.csrf, &token, quote())
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
                auth.admit_submission(&ledger, &id, &session.csrf, &token, quote())
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
            auth.admit_submission(&ledger, &id, &session.csrf, &token, quote())
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
            assert!(auth
                .issue_submission_for(&id, submission.conversation, Some("m"))
                .is_err());
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
