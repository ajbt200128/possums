use possums::{
    accounting::{Accounting, AccountingError, FinalUsage, Outcome, ReserveResult},
    catalog::{actual_cost, Catalog, Model, Quote},
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
            input_microunits_per_million_tokens: 2_000_000,
            output_microunits_per_million_tokens: 3_000_000,
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
    assert_eq!(
        ledger.finish([1; 32], usage(5, 4)).unwrap(),
        Outcome::Settled { charged: 29 }
    );
    assert_eq!(ledger.available("a"), Some(71));
    assert_eq!(
        ledger
            .reserve("a", [1; 32], [2; 32], quote(), token_expiry())
            .unwrap(),
        ReserveResult::Duplicate(Outcome::Settled { charged: 29 })
    );
    for terminal in [usage(5, 4), None] {
        assert_eq!(
            ledger.finish([1; 32], terminal).unwrap(),
            Outcome::Settled { charged: 29 }
        );
        assert_eq!(ledger.available("a"), Some(71));
    }
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
    assert_eq!(ledger.finish([1; 32], None).unwrap(), Outcome::Refunded);
    assert_eq!(ledger.finish([1; 32], None).unwrap(), Outcome::Refunded);
    assert_eq!(ledger.available("a"), Some(100));
}

#[test]
fn terminal_state_is_reclaimed_only_after_token_expiry() {
    let ledger = Accounting::new([("a".into(), 200)]);
    let expires = Instant::now() + Duration::from_millis(5);
    ledger
        .reserve("a", [1; 32], [2; 32], quote(), expires)
        .unwrap();
    assert_eq!(ledger.finish([1; 32], None).unwrap(), Outcome::Refunded);
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
fn conflicting_stream_completions_race_has_one_absorbing_terminal_balance() {
    let ledger = Arc::new(Accounting::new([("a".into(), 100)]));
    ledger
        .reserve("a", [1; 32], [2; 32], quote(), token_expiry())
        .unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let settle_ledger = Arc::clone(&ledger);
    let ready = Arc::clone(&barrier);
    let settle = std::thread::spawn(move || {
        ready.wait();
        settle_ledger.finish([1; 32], usage(5, 4)).unwrap()
    });
    let refund_ledger = Arc::clone(&ledger);
    let refund = std::thread::spawn(move || {
        barrier.wait();
        refund_ledger.finish([1; 32], None).unwrap()
    });
    let outcome = settle.join().unwrap();
    assert_eq!(refund.join().unwrap(), outcome);
    let balance = match outcome {
        Outcome::Settled { charged: 29 } => 71,
        Outcome::Refunded => 100,
        _ => panic!("unexpected outcome"),
    };
    for terminal in [None, usage(5, 4)] {
        assert_eq!(ledger.finish([1; 32], terminal).unwrap(), outcome);
        assert_eq!(ledger.available("a"), Some(balance));
    }
    assert_eq!(
        ledger
            .reserve("a", [1; 32], [2; 32], quote(), token_expiry())
            .unwrap(),
        ReserveResult::Duplicate(outcome)
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

fn operational_quote() -> Quote {
    let mut model = quote().model;
    model.max_output_tokens = 100;
    Catalog {
        models: vec![model],
    }
    .reservation_quote("m")
    .unwrap()
}

fn usage(input_tokens: u64, output_tokens: u64) -> Option<FinalUsage> {
    Some(FinalUsage {
        input_tokens,
        output_tokens,
        total_tokens: input_tokens.checked_add(output_tokens).unwrap(),
    })
}

#[test]
fn usage_above_operational_bounds_is_capped_not_refunded_and_never_reversed() {
    for (input, calculated) in [(100, 654), (101, 657)] {
        let ledger = Accounting::new([("a".into(), 1000)]);
        let reservation = operational_quote();
        assert_eq!(reservation.reserved_microunits, 650);
        // Check the same cost arithmetic without the old output-bound veto.
        let mut expanded = reservation.clone();
        expanded.input_tokens = input;
        expanded.model.max_output_tokens = 101;
        assert_eq!(actual_cost(&expanded, 101).unwrap(), calculated);
        ledger
            .reserve("a", [1; 32], [2; 32], reservation, token_expiry())
            .unwrap();
        assert_eq!(ledger.available("a"), Some(350));
        let settled = Outcome::Settled { charged: 650 };
        for terminal in [usage(input, 101), usage(input, 101), usage(0, 0), None] {
            assert_eq!(ledger.finish([1; 32], terminal).unwrap(), settled);
            assert_eq!(ledger.available("a"), Some(350));
        }
        assert_eq!(
            ledger
                .reserve("a", [1; 32], [2; 32], operational_quote(), token_expiry())
                .unwrap(),
            ReserveResult::Duplicate(settled)
        );
        assert_eq!(ledger.available("a"), Some(350));
    }
}

#[test]
fn terminal_partial_refunds_zero_completion_and_price_snapshots() {
    for (input, output, charge) in [(5, 4, 29), (5, 0, 13), (0, 0, 0)] {
        let ledger = Accounting::new([("a".into(), 1000)]);
        let mut catalog = Catalog {
            models: vec![operational_quote().model],
        };
        let reservation = catalog.reservation_quote("m").unwrap();
        ledger
            .reserve("a", [1; 32], [2; 32], reservation, token_expiry())
            .unwrap();
        catalog.models[0].input_microunits_per_million_tokens *= 10;
        catalog.models[0].output_microunits_per_million_tokens *= 10;
        assert_eq!(
            catalog.reservation_quote("m").unwrap().reserved_microunits,
            6500
        );
        assert_eq!(
            ledger.finish([1; 32], usage(input, output)).unwrap(),
            Outcome::Settled { charged: charge }
        );
        assert_eq!(ledger.available("a"), Some(1000 - charge));
        assert_eq!(
            ledger.finish([1; 32], None).unwrap(),
            Outcome::Settled { charged: charge }
        );
    }
}

#[test]
fn malformed_totals_and_charge_overflow_refund_once_even_if_later_usage_is_valid() {
    for (price, terminal) in [
        (2_000_000, None),
        (
            2_000_000,
            Some(FinalUsage {
                input_tokens: 5,
                output_tokens: 4,
                total_tokens: 10,
            }),
        ),
        (
            2_000_000,
            Some(FinalUsage {
                input_tokens: u64::MAX,
                output_tokens: 1,
                total_tokens: 0,
            }),
        ),
        // Representable total, but monetary u64 result overflow.
        (1_000_000, usage(u64::MAX, 0)),
        // Representable total, but u128 markup numerator overflow.
        (u64::MAX, usage(u64::MAX, 0)),
    ] {
        let mut model = operational_quote().model;
        model.context_tokens = 1;
        model.max_output_tokens = 1;
        model.input_microunits_per_million_tokens = price;
        model.output_microunits_per_million_tokens = price;
        let reservation = Catalog {
            models: vec![model],
        }
        .reservation_quote("m")
        .unwrap();
        let budget = reservation.reserved_microunits;
        let ledger = Accounting::new([("a".into(), budget)]);
        ledger
            .reserve("a", [1; 32], [2; 32], reservation, token_expiry())
            .unwrap();
        assert_eq!(ledger.available("a"), Some(0));
        for terminal in [terminal, terminal, usage(1, 0)] {
            assert_eq!(ledger.finish([1; 32], terminal).unwrap(), Outcome::Refunded);
            assert_eq!(ledger.available("a"), Some(budget));
        }
        assert_eq!(ledger.finish([1; 32], None).unwrap(), Outcome::Refunded);
        assert_eq!(ledger.available("a"), Some(budget));
    }
}

#[test]
fn conflicting_terminal_calls_return_one_outcome_and_refund_once() {
    let ledger = Arc::new(Accounting::new([("a".into(), 10_000)]));
    for id in 1..=4 {
        ledger
            .reserve("a", [id; 32], [2; 32], operational_quote(), token_expiry())
            .unwrap();
    }
    let barrier = Arc::new(std::sync::Barrier::new(12));
    let handles: Vec<_> = (0..12)
        .map(|n| {
            let ledger = Arc::clone(&ledger);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                ledger
                    .finish([1; 32], if n % 2 == 0 { usage(5, 4) } else { None })
                    .unwrap()
            })
        })
        .collect();
    let outcomes: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    assert!(outcomes.iter().all(|outcome| outcome == &outcomes[0]));
    let charge = match outcomes[0] {
        Outcome::Settled { charged: 29 } => 29,
        Outcome::Refunded => 0,
        _ => panic!("unexpected outcome"),
    };
    assert_eq!(ledger.available("a"), Some(10_000 - 3 * 650 - charge));
    ledger
        .reserve("a", [5; 32], [2; 32], operational_quote(), token_expiry())
        .unwrap();
    for id in 2..=5 {
        assert_eq!(ledger.finish([id; 32], None).unwrap(), Outcome::Refunded);
        assert_eq!(
            ledger.finish([id; 32], usage(5, 4)).unwrap(),
            Outcome::Refunded
        );
    }
    assert_eq!(ledger.available("a"), Some(10_000 - charge));
}

#[test]
fn reserve_racing_terminal_transition_conserves_credit_without_account_quota() {
    for _ in 0..32 {
        let ledger = Arc::new(Accounting::new([("a".into(), 10_000)]));
        for id in 1..=4 {
            ledger
                .reserve("a", [id; 32], [2; 32], operational_quote(), token_expiry())
                .unwrap();
        }
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let finishing = Arc::clone(&ledger);
        let ready = Arc::clone(&barrier);
        let terminal = std::thread::spawn(move || {
            ready.wait();
            finishing.finish([1; 32], usage(5, 4)).unwrap()
        });
        barrier.wait();
        let reserve = ledger.reserve("a", [5; 32], [2; 32], operational_quote(), token_expiry());
        assert_eq!(terminal.join().unwrap(), Outcome::Settled { charged: 29 });
        assert_eq!(reserve, Ok(ReserveResult::Reserved));
        assert_eq!(ledger.available("a"), Some(10_000 - 4 * 650 - 29));
        for id in 2..=5 {
            assert_eq!(ledger.finish([id; 32], None).unwrap(), Outcome::Refunded);
        }
        assert_eq!(ledger.available("a"), Some(10_000 - 29));
        assert_eq!(
            ledger
                .reserve("a", [1; 32], [2; 32], operational_quote(), token_expiry())
                .unwrap(),
            ReserveResult::Duplicate(Outcome::Settled { charged: 29 })
        );
    }
}

#[test]
fn terminal_cannot_create_a_reservation_and_later_usage_cannot_override_it() {
    let ledger = Accounting::new([("a".into(), 1000)]);
    assert_eq!(
        ledger.finish([1; 32], usage(5, 4)),
        Err(AccountingError::InvalidTransition)
    );
    assert_eq!(ledger.available("a"), Some(1000));
    ledger
        .reserve("a", [1; 32], [2; 32], operational_quote(), token_expiry())
        .unwrap();
    assert_eq!(
        ledger.finish([1; 32], usage(100, 101)).unwrap(),
        Outcome::Settled { charged: 650 }
    );
    assert_eq!(
        ledger.finish([1; 32], usage(5, 4)).unwrap(),
        Outcome::Settled { charged: 650 }
    );
    assert_eq!(ledger.available("a"), Some(350));
}
