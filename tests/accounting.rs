use possums::{
    accounting::{Accounting, AccountingError, Outcome, ReserveResult},
    catalog::{Model, Quote},
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

fn token_expiry() -> Instant {
    Instant::now() + Duration::from_secs(60)
}

fn quote() -> Quote {
    Quote {
        model: Model {
            id: "m".into(),
            context_tokens: 100,
            max_output_tokens: 10,
            input_microunits_per_token: 2,
            output_microunits_per_token: 3,
        },
        input_tokens: 5,
        reserved_microunits: 52,
    }
}

#[test]
fn reserves_settles_and_refunds_remainder_once() {
    let ledger = Accounting::new([("a".into(), 100)]);
    assert_eq!(
        ledger
            .reserve("a", [1; 32], [2; 32], quote(), token_expiry())
            .unwrap(),
        ReserveResult::Reserved
    );
    assert_eq!(ledger.available("a"), Some(48));
    assert_eq!(ledger.prepare_settlement([1; 32], 5, 4).unwrap(), 29);
    assert_eq!(ledger.available("a"), Some(48));
    assert_eq!(ledger.settle([1; 32]).unwrap(), 29);
    assert_eq!(ledger.available("a"), Some(71));
    assert_eq!(
        ledger
            .reserve("a", [1; 32], [2; 32], quote(), token_expiry())
            .unwrap(),
        ReserveResult::Duplicate(Outcome::Settled { charged: 29 })
    );
    assert_eq!(
        ledger.refund([1; 32]),
        Err(AccountingError::InvalidTransition)
    );
}

#[test]
fn altered_duplicate_never_reserves_again() {
    let ledger = Accounting::new([("a".into(), 100)]);
    ledger
        .reserve("a", [1; 32], [2; 32], quote(), token_expiry())
        .unwrap();
    assert_eq!(
        ledger
            .reserve("a", [1; 32], [3; 32], quote(), token_expiry())
            .unwrap_err(),
        AccountingError::AlteredDuplicate
    );
    ledger.refund([1; 32]).unwrap();
    ledger.refund([1; 32]).unwrap();
    assert_eq!(ledger.available("a"), Some(100));
}

#[test]
fn terminal_state_is_reclaimed_only_after_token_expiry() {
    let ledger = Accounting::new([("a".into(), 200)]);
    let expires = Instant::now() + Duration::from_millis(5);
    ledger
        .reserve("a", [1; 32], [2; 32], quote(), expires)
        .unwrap();
    ledger.refund([1; 32]).unwrap();
    assert_eq!(
        ledger
            .reserve("a", [1; 32], [2; 32], quote(), token_expiry())
            .unwrap(),
        ReserveResult::Duplicate(Outcome::Refunded)
    );
    std::thread::sleep(Duration::from_millis(10));
    assert_eq!(
        ledger
            .reserve("a", [1; 32], [2; 32], quote(), token_expiry())
            .unwrap(),
        ReserveResult::Reserved
    );
}

#[test]
fn concurrent_reservations_cannot_overspend() {
    let ledger = Arc::new(Accounting::new([("a".into(), 52)]));
    let handles: Vec<_> = (0..8)
        .map(|n| {
            let ledger = Arc::clone(&ledger);
            std::thread::spawn(move || {
                ledger.reserve("a", [n; 32], [n; 32], quote(), token_expiry())
            })
        })
        .collect();
    let successes = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .filter(|result| matches!(result, Ok(ReserveResult::Reserved)))
        .count();
    assert_eq!(successes, 1);
    assert_eq!(ledger.available("a"), Some(0));
}
