use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use possums::{
    accounting::{Accounting, Outcome, ReserveResult},
    auth::Auth,
    catalog::{Model, Quote},
};
use sha2::{Digest, Sha256};

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
            input_microunits_per_token: 1,
            output_microunits_per_token: 1,
        },
        input_tokens: 10,
        reserved_microunits: 26,
    }
}

#[test]
fn process_epoch_invalidates_old_submission_tokens() {
    let first = Auth::from_json(&auth_config()).unwrap();
    let restarted = Auth::from_json(&auth_config()).unwrap();
    assert_ne!(
        first.bind_submission("a", "submission"),
        restarted.bind_submission("a", "submission")
    );
}

#[test]
fn uncertain_delivery_refund_is_terminal_and_idempotent() {
    let ledger = Accounting::new([("a".into(), 100)]);
    ledger.reserve("a", [1; 32], [2; 32], quote()).unwrap();
    ledger.refund([1; 32]).unwrap();
    ledger.refund([1; 32]).unwrap();
    assert_eq!(ledger.available("a"), Some(100));
    assert_eq!(
        ledger.reserve("a", [1; 32], [2; 32], quote()).unwrap(),
        ReserveResult::Duplicate(Outcome::Refunded)
    );
}
