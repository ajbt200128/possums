use super::*;
use serde_json::{json, Value};

// All values in this file are disposable synthetic fixtures, never credentials.
const ACCOUNT: &str = "synthetic-account";
const KEY: &str = "v1.1700000000000.0123456789abcdef0123456789abcdef";
const RECORD: &str = "customerrecord1";
const HOSTILE: &str = "SYNTHETIC_PRIVATE_SENTINEL";

fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_700_000_000)
}
fn claims() -> Value {
    json!({"id": RECORD, "collectionId": COLLECTION_ID, "type": "auth", "refreshable": true, "exp": 1700003600_u64})
}
fn token_raw(payload: &[u8]) -> String {
    // Deliberately not a signed JWT: tests metadata decoding, not local auth.
    format!("e30.{}.AA", URL_SAFE_NO_PAD.encode(payload))
}
fn token(payload: &Value) -> String {
    token_raw(&serde_json::to_vec(payload).unwrap())
}
fn auth() -> Value {
    json!({"token": token(&claims()), "record": {
        "id": RECORD, "collectionId": COLLECTION_ID, "collectionName": COLLECTION, "account_id": ACCOUNT
    }})
}
fn read_auth(value: Value) -> AuthResponse {
    serde_json::from_value(value).unwrap_or_else(|_| panic!("synthetic auth schema"))
}
fn active() -> Value {
    json!({
        "reservation_id": "0123456789abcdef0123456789abcdef", "idempotency_key": KEY,
        "account_id": ACCOUNT, "maximum": "100", "timeout_seconds": 30,
        "initial_expires_at": "2023-11-14T22:13:50.000Z",
        "expires_at": "2023-11-14T22:13:50.000Z", "state": "active",
        "charged_amount": null, "terminal_at": null, "retain_until": null
    })
}
fn terminal(state: &str, charged: &str) -> Value {
    let mut value = active();
    value["state"] = json!(state);
    value["charged_amount"] = json!(charged);
    let (at, until) = if state == "expired" {
        ("2023-11-14T22:13:50.000Z", "2023-11-15T22:13:50.000Z")
    } else {
        ("2023-11-14T22:13:21.000Z", "2023-11-15T22:13:21.000Z")
    };
    value["terminal_at"] = json!(at);
    value["retain_until"] = json!(until);
    value
}
fn row(value: Value) -> Reservation {
    serde_json::from_value(value).unwrap_or_else(|_| panic!("synthetic reservation schema"))
}
fn valid_row(value: Value) -> bool {
    serde_json::from_value::<Reservation>(value).is_ok_and(|r| r.valid())
}
fn duplicate(value: Value, field: &str) -> String {
    let duplicate = serde_json::to_string(&value[field]).unwrap();
    let original = serde_json::to_string(&value).unwrap();
    format!("{{\"{field}\":{duplicate},{}", &original[1..])
}
fn failure<T>(result: Result<T, Error>) -> Error {
    match result {
        Ok(_) => panic!("expected closed refusal"),
        Err(error) => error,
    }
}

#[test]
fn native_methods_are_required_typed_and_fail_closed() {
    let methods = || json!({"password": {"enabled": true, "identityFields": ["key_lookup"]}, "mfa": {"enabled": false}});
    let decode = |value| {
        serde_json::from_value::<AuthMethods>(value)
            .unwrap_or_else(|_| panic!("synthetic methods schema"))
    };
    assert!(check_methods(decode(methods()), 200).is_ok());
    let mut value = methods();
    value["oauth2"] = json!({"ignored": HOSTILE});
    assert!(check_methods(decode(value), 200).is_ok());
    let mut value = methods();
    value["password"]["identityFields"] = json!(["email", "key_lookup"]);
    assert!(check_methods(decode(value), 200).is_ok());
    let mut value = methods();
    value["mfa"]["enabled"] = json!(true);
    let error = failure(check_methods(decode(value), 200));
    assert_eq!(error.kind, ErrorKind::MfaUnsupported);
    assert_eq!(error.status, Some(200));
    for field in ["enabled", "identityFields"] {
        let mut value = methods();
        value["password"].as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<AuthMethods>(value).is_err());
    }
    for (field, replacement) in [
        ("enabled", json!(false)),
        ("identityFields", json!(["email"])),
    ] {
        let mut value = methods();
        value["password"][field] = replacement;
        assert_eq!(
            failure(check_methods(decode(value), 200)).kind,
            ErrorKind::Authentication
        );
    }
    assert!(serde_json::from_str::<AuthMethods>(&duplicate(methods(), "password")).is_err());
    assert!(serde_json::from_str::<AuthMethods>(r#"{"password":{"enabled":true,"enabled":true,"identityFields":["key_lookup"]},"mfa":{"enabled":false}}"#).is_err());
    for payload in [
        r#"{"mfaId":"synthetic"}"#,
        r#"{"mfaId":"synthetic","extra":true}"#,
    ] {
        assert!(serde_json::from_str::<MfaChallenge>(payload).is_ok());
    }
    for payload in [
        r#"{}"#,
        r#"{"mfaId":null}"#,
        r#"{"mfaId":""}"#,
        r#"{"mfaId":12}"#,
        r#"{"mfaId":"a","mfaId":"b"}"#,
    ] {
        assert!(serde_json::from_str::<MfaChallenge>(payload).is_err());
    }
}

#[test]
fn native_principal_metadata_is_bound_without_local_signature_authority() {
    let captured = principal(read_auth(auth()), Operation::Login, now()).unwrap();
    assert_eq!(captured.account_id, ACCOUNT);
    assert_eq!(captured.record_id, RECORD);
    assert_eq!(captured.expires_at, now() + Duration::from_secs(3600));
    let original_token = captured.token.clone();
    let mut renewed = auth();
    let mut renewed_claims = claims();
    renewed_claims["exp"] = json!(1700010000_u64);
    renewed["token"] = json!(token(&renewed_claims));
    let current = principal(read_auth(renewed), Operation::Refresh, now()).unwrap();
    assert!(check_binding(&captured, &current, 200).is_ok());
    assert_eq!(captured.token, original_token);
    assert_eq!(captured.expires_at, now() + Duration::from_secs(3600));
    for (field, value) in [
        ("id", json!("other")),
        ("collectionId", json!("other")),
        ("type", json!("file")),
        ("refreshable", json!(false)),
        ("exp", json!(1700000000_u64)),
    ] {
        let mut payload = claims();
        payload[field] = value;
        let mut value = auth();
        value["token"] = json!(token(&payload));
        assert!(principal(read_auth(value), Operation::Login, now()).is_err());
    }
    for field in ["account_id", "id"] {
        let mut value = auth();
        value["record"][field] = json!("other");
        if field == "id" {
            let mut payload = claims();
            payload["id"] = json!("other");
            value["token"] = json!(token(&payload));
        }
        let current = principal(read_auth(value), Operation::Refresh, now()).unwrap();
        let error = failure(check_binding(&captured, &current, 200));
        assert_eq!(error.kind, ErrorKind::BindingChanged);
        assert_eq!(error.status, Some(200));
    }
    for field in ["collectionId", "collectionName"] {
        let mut value = auth();
        value["record"][field] = json!("other");
        assert_eq!(
            failure(principal(read_auth(value), Operation::Refresh, now())).kind,
            ErrorKind::BindingChanged
        );
    }
}

#[test]
fn account_identity_is_exact_nonempty_and_byte_bounded() {
    for account in [" \u{0}é ".to_owned(), "é".repeat(64), "e\u{301}".to_owned()] {
        let mut value = auth();
        value["record"]["account_id"] = json!(account);
        let principal = principal(read_auth(value), Operation::Login, now()).unwrap();
        assert_eq!(principal.account_id, account);
    }
    for account in [String::new(), "é".repeat(65)] {
        let mut value = auth();
        value["record"]["account_id"] = json!(account);
        assert!(principal(read_auth(value), Operation::Login, now()).is_err());
    }
    let mut value = auth();
    value["record"]["extra"] = json!(HOSTILE);
    value["extra"] = json!(HOSTILE);
    assert!(principal(read_auth(value), Operation::Login, now()).is_ok());
}

#[test]
fn auth_required_fields_and_bounded_claims_reject_hostile_or_ambiguous_evidence() {
    for field in ["token", "record"] {
        assert!(serde_json::from_str::<AuthResponse>(&duplicate(auth(), field)).is_err());
        let mut value = auth();
        value.as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<AuthResponse>(value).is_err());
    }
    for field in ["id", "collectionId", "collectionName", "account_id"] {
        let mut value = auth();
        value["record"].as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<AuthResponse>(value).is_err());
        let record = duplicate(auth()["record"].clone(), field);
        let payload = format!("{{\"token\":\"synthetic\",\"record\":{record}}}");
        assert!(serde_json::from_str::<AuthResponse>(&payload).is_err());
    }
    for field in ["id", "collectionId", "type", "refreshable", "exp"] {
        assert!(token_claims(&token_raw(duplicate(claims(), field).as_bytes())).is_none());
        let mut payload = claims();
        payload.as_object_mut().unwrap().remove(field);
        assert!(token_claims(&token(&payload)).is_none());
        payload[field] = Value::Null;
        assert!(token_claims(&token(&payload)).is_none());
    }
    for payload in [
        b"null".as_slice(),
        b"{}",
        b"[]",
        b"{} {}",
        b"\xff",
        br#"{"id":"\ud800"}"#,
    ] {
        assert!(token_claims(&token_raw(payload)).is_none());
    }
    for expiry in [json!(-1), json!(1.5), json!("1700003600"), json!(HOSTILE)] {
        let mut payload = claims();
        payload["exp"] = expiry;
        assert!(token_claims(&token(&payload)).is_none());
    }
    for raw in [
        String::new(),
        "a.b.c.d".into(),
        "a..c".into(),
        "a.b.c=".into(),
        "a.b. c".into(),
        "a".repeat(TOKEN_LIMIT + 1),
    ] {
        assert!(token_claims(&raw).is_none());
    }
    let mut payload = claims();
    payload["extra"] = json!("x".repeat(CLAIMS_LIMIT));
    assert!(token_claims(&token(&payload)).is_none());
}

#[test]
fn money_is_canonical_exact_u64_not_a_number_or_normalized_string() {
    for (value, expected) in [("0", 0), ("1", 1), ("18446744073709551615", u64::MAX)] {
        assert_eq!(parse_money(value), Some(expected));
    }
    for value in [
        "",
        "00",
        "01",
        "+1",
        "-0",
        " 1",
        "1 ",
        "1.0",
        "1e2",
        "١",
        "18446744073709551616",
        HOSTILE,
    ] {
        assert_eq!(parse_money(value), None);
        let mut fixture = active();
        fixture["maximum"] = json!(value);
        assert!(serde_json::from_value::<Reservation>(fixture).is_err());
    }
    for value in [json!(100), json!(100.0), json!(true), Value::Null] {
        let mut fixture = active();
        fixture["maximum"] = value;
        assert!(serde_json::from_value::<Reservation>(fixture).is_err());
    }
    let mut fixture = active();
    fixture["maximum"] = json!("18446744073709551615");
    assert!(valid_row(fixture));
    let mut fixture = active();
    fixture["maximum"] = json!("0");
    assert!(!valid_row(fixture));
}

#[test]
fn reservation_requires_every_field_and_rejects_duplicate_required_fields() {
    let fixture = active();
    for field in fixture.as_object().unwrap().keys() {
        let mut absent = fixture.clone();
        absent.as_object_mut().unwrap().remove(field);
        assert!(
            serde_json::from_value::<Reservation>(absent).is_err(),
            "required field missing"
        );
        assert!(
            serde_json::from_str::<Reservation>(&duplicate(fixture.clone(), field)).is_err(),
            "required field duplicated"
        );
    }
    let mut fixture = active();
    fixture["extra"] = json!({"hostile": HOSTILE});
    assert!(valid_row(fixture));
    assert!(serde_json::from_str::<ReservationResponse>(r#"{"reservation":null}"#).is_err());
    assert!(serde_json::from_str::<ReservationResponse>(r#"{}"#).is_err());
    let wrapped = json!({"reservation": active()});
    assert!(
        serde_json::from_str::<ReservationResponse>(&duplicate(wrapped, "reservation")).is_err()
    );
    for state in [json!("ACTIVE"), json!("unknown"), Value::Null, json!(42)] {
        let mut fixture = active();
        fixture["state"] = state;
        assert!(serde_json::from_value::<Reservation>(fixture).is_err());
    }
}

#[test]
fn reservation_dates_and_ids_match_native_fixed_formats() {
    for date in [
        "2023-11-14T22:13:50Z",
        "2023-11-14T22:13:50.00Z",
        "2023-11-14T22:13:50.0000Z",
        "2023-11-14T22:13:50.000+00:00",
        "2023-11-14t22:13:50.000z",
        "2023-02-30T22:13:50.000Z",
        "2023-11-14T22:13:60.000Z",
        "1970-01-01T00:00:00.000Z",
        "9999-01-01T00:00:00.000Z",
        HOSTILE,
    ] {
        let mut fixture = active();
        fixture["expires_at"] = json!(date);
        assert!(serde_json::from_value::<Reservation>(fixture).is_err());
    }
    for (field, value) in [
        (
            "reservation_id",
            json!("ABCDEF0123456789abcdef0123456789ab"),
        ),
        ("reservation_id", json!("short")),
        ("account_id", json!("")),
        ("account_id", json!("é".repeat(65))),
        (
            "idempotency_key",
            json!("v1.1700000000000.A123456789abcdef0123456789abcdef"),
        ),
        (
            "idempotency_key",
            json!("v1.1700000000000.0123456789abcdef0123456789abcdefx"),
        ),
        (
            "idempotency_key",
            json!("v1.1700000060001.0123456789abcdef0123456789abcdef"),
        ),
        (
            "idempotency_key",
            json!("v1.1699913600000.0123456789abcdef0123456789abcdef"),
        ),
        ("timeout_seconds", json!(0)),
        ("timeout_seconds", json!(1801)),
        ("timeout_seconds", json!("30")),
        ("timeout_seconds", json!(30.5)),
    ] {
        let mut fixture = active();
        fixture[field] = value;
        assert!(!valid_row(fixture));
    }
    let receipt = row(active());
    assert!(receipt.valid());
    assert_eq!(receipt.expires_at(), now() + Duration::from_secs(30));
}

#[test]
fn active_and_terminal_null_charge_date_and_retention_invariants() {
    assert!(valid_row(active()));
    for (state, charge) in [
        ("settled", "0"),
        ("settled", "100"),
        ("refunded", "0"),
        ("expired", "0"),
    ] {
        assert!(valid_row(terminal(state, charge)));
    }
    for (field, value) in [
        ("charged_amount", json!("0")),
        ("terminal_at", json!("2023-11-14T22:13:21.000Z")),
        ("retain_until", json!("2023-11-15T22:13:21.000Z")),
    ] {
        let mut fixture = active();
        fixture[field] = value;
        assert!(!valid_row(fixture));
    }
    for state in ["settled", "refunded", "expired"] {
        for field in ["charged_amount", "terminal_at", "retain_until"] {
            let mut fixture = terminal(state, "0");
            fixture[field] = Value::Null;
            assert!(!valid_row(fixture));
        }
    }
    for (state, charge) in [("settled", "101"), ("refunded", "1"), ("expired", "1")] {
        assert!(!valid_row(terminal(state, charge)));
    }
    for (field, value) in [
        ("terminal_at", "2023-11-14T22:13:19.999Z"),
        ("terminal_at", "2023-11-14T22:13:50.000Z"),
        ("retain_until", "2023-11-15T22:13:21.001Z"),
    ] {
        let mut fixture = terminal("settled", "50");
        fixture[field] = json!(value);
        assert!(!valid_row(fixture));
    }
    let mut fixture = terminal("expired", "0");
    fixture["terminal_at"] = json!("2023-11-14T22:13:49.999Z");
    assert!(!valid_row(fixture));
}

#[test]
fn reserve_binds_exact_inputs_and_preserves_terminal_replay_evidence() {
    let receipt = check_reserve(row(active()), ACCOUNT, 100, 30, KEY).unwrap();
    assert!(receipt.is_active());
    assert!(receipt.terminal_outcome().is_none());
    for (account, maximum, timeout, key) in [
        ("other", 100, 30, KEY),
        (ACCOUNT, 99, 30, KEY),
        (ACCOUNT, 100, 29, KEY),
        (ACCOUNT, 100, 30, "other"),
    ] {
        let error = failure(check_reserve(row(active()), account, maximum, timeout, key));
        assert_eq!(error.constraint, Constraint::ReservationBinding);
    }
    let receipt = check_reserve(row(terminal("settled", "50")), ACCOUNT, 100, 30, KEY).unwrap();
    assert!(!receipt.is_active());
    assert!(receipt.terminal_outcome() == Some(TerminalOutcome::Settled { charged: 50 }));
}

#[test]
fn terminal_first_wins_is_not_silently_requested_success() {
    let original = row(active());
    for (requested, state, charge, status, expected_match) in [
        (Some(50), "settled", "50", 200, true),
        (Some(50), "settled", "51", 200, false),
        (None, "settled", "50", 200, false),
        (Some(0), "refunded", "0", 200, false),
        (None, "refunded", "0", 200, true),
        (Some(50), "expired", "0", 409, false),
        (None, "expired", "0", 200, false),
    ] {
        let receipt =
            check_terminal(&original, row(terminal(state, charge)), requested, status).unwrap();
        assert_eq!(receipt.matches_requested, expected_match);
        let expected = match state {
            "settled" => TerminalOutcome::Settled {
                charged: charge.parse().unwrap(),
            },
            "refunded" => TerminalOutcome::Refunded,
            _ => TerminalOutcome::Expired,
        };
        assert!(receipt.outcome == expected);
    }
    assert!(check_terminal(&original, row(active()), Some(50), 200).is_err());
    assert!(check_terminal(&original, row(terminal("settled", "50")), Some(50), 409).is_err());
    assert!(check_terminal(&original, row(terminal("expired", "0")), Some(50), 200).is_err());
    let prior = row(terminal("settled", "50"));
    assert!(check_terminal(&prior, row(terminal("settled", "51")), Some(51), 200).is_err());
}

#[test]
fn expired_envelope_retains_evidence_separate_from_the_closed_diagnostic() {
    let envelope = json!({
        "code": "LEDGER_SETTLEMENT_EXPIRED", "stage": HOSTILE,
        "constraint": HOSTILE, "outcome": "expired", "next_step": HOSTILE,
        "reservation": terminal("expired", "0"),
    });
    let bytes = serde_json::to_vec(&envelope).unwrap();
    let diagnostic: Diagnostic =
        serde_json::from_slice(&bytes).unwrap_or_else(|_| panic!("synthetic expiry code"));
    assert!(diagnostic.code == RemoteCode::Expired);
    let evidence: ReservationResponse =
        serde_json::from_slice(&bytes).unwrap_or_else(|_| panic!("synthetic expiry row"));
    let receipt = check_terminal(&row(active()), evidence.reservation, Some(50), 409).unwrap();
    assert!(receipt.outcome == TerminalOutcome::Expired);
    assert!(!receipt.matches_requested);
}

#[test]
fn terminal_receipt_keeps_all_immutable_reservation_fields() {
    for (field, value) in [
        ("reservation_id", json!("abcdef0123456789abcdef0123456789")),
        ("account_id", json!("other")),
        ("maximum", json!("101")),
        ("timeout_seconds", json!(31)),
        (
            "idempotency_key",
            json!("v1.1700000000000.abcdef0123456789abcdef0123456789"),
        ),
        ("initial_expires_at", json!("2023-11-14T22:13:49.000Z")),
    ] {
        let mut fixture = terminal("settled", "50");
        fixture[field] = value;
        let error = failure(check_terminal(&row(active()), row(fixture), Some(50), 200));
        assert_eq!(error.constraint, Constraint::ReservationBinding);
        assert_eq!(error.status, Some(200));
    }
}

fn renewed() -> Value {
    let mut value = active();
    value["expires_at"] = json!("2023-11-14T22:14:00.000Z");
    value
}
fn renewal_envelope(code: Option<&str>, reservation: Value, observed: &str) -> Value {
    let mut value = json!({"reservation":reservation, "observed_at":observed});
    if let Some(code) = code {
        value["code"] = json!(code);
        for field in ["stage", "constraint", "outcome", "next_step"] {
            value[field] = json!(HOSTILE);
        }
    }
    value
}
fn renewal_reply(original: &Reservation, value: Value, status: u16) -> Result<RenewalReply, Error> {
    check_renew_response(
        original,
        &Response::fixture(
            Operation::Renew,
            status,
            serde_json::to_vec(&value).unwrap(),
        ),
    )
}

#[test]
fn initial_expiry_is_required_canonical_and_anchors_long_lived_key_admission() {
    for value in [
        Value::Null,
        json!(42),
        json!("2023-11-14T22:13:50Z"),
        json!("9999-01-01T00:00:00.000Z"),
        json!(HOSTILE),
    ] {
        let mut fixture = active();
        fixture["initial_expires_at"] = value;
        assert!(!valid_row(fixture));
    }
    let mut fixture = active();
    fixture["initial_expires_at"] = json!("2023-11-14T22:13:50.001Z");
    assert!(!valid_row(fixture)); // current cannot precede initial
    let mut fixture = active();
    fixture["initial_expires_at"] = json!("2023-11-15T22:13:50.000Z");
    fixture["expires_at"] = fixture["initial_expires_at"].clone();
    assert!(!valid_row(fixture)); // new admission cannot use a >24h key

    let mut long_lived = active();
    long_lived["expires_at"] = json!("2023-11-16T22:13:50.000Z");
    let captured = check_reserve(row(long_lived.clone()), ACCOUNT, 100, 30, KEY).unwrap();
    long_lived["expires_at"] = json!("2023-11-16T22:14:00.000Z");
    let reply = renewal_reply(
        &captured,
        renewal_envelope(None, long_lived, "2023-11-16T22:13:30.000Z"),
        200,
    )
    .unwrap();
    assert!(reply.refusal.is_none());
    assert!(reply.reservation.is_active());
}

#[test]
fn canonical_millisecond_formatting_preserves_the_exact_conditional_token() {
    for value in [
        "1970-01-01T00:00:00.001Z",
        "2023-11-14T22:13:50.000Z",
        "2024-02-29T23:59:59.999Z",
        "2286-11-20T17:46:39.999Z",
    ] {
        let parsed = Millis(canonical_millis(value).unwrap());
        assert_eq!(parsed.canonical().as_deref(), Some(value));
    }
}

#[tokio::test]
async fn invalid_captured_expiry_never_dispatches_a_renewal() {
    // Construction is local-only; the invalid row is refused before networking.
    let intake = Intake::new(
        "https://example.invalid",
        "synthetic-accounting-key-000000000000",
    )
    .unwrap();
    let mut invalid = active();
    invalid["expires_at"] = json!("2023-11-14T22:13:49.999Z");
    let error = failure(intake.renew(&row(invalid)).await);
    assert_eq!(error.kind, ErrorKind::Input);
    assert_eq!(error.operation(), "intake_renew");
    assert!(!error.dispatch_attempted);
    assert_eq!(error.status, None);
}

#[test]
fn renewal_confirms_strict_progress_and_reordered_current_observations() {
    for observed in [
        "2023-11-14T22:13:30.000Z",
        "2023-11-14T22:13:35.000Z",
        "2023-11-14T22:13:59.999Z",
    ] {
        let reply = renewal_reply(
            &row(active()),
            renewal_envelope(None, renewed(), observed),
            200,
        )
        .unwrap();
        assert!(reply.refusal.is_none());
        assert!(reply.terminal_receipt(Some(50)).is_none());
        assert!(reply.observed_at < reply.reservation.expires_at());
        assert_eq!(
            reply
                .reservation
                .expires_at()
                .duration_since(now())
                .unwrap(),
            Duration::from_secs(40)
        );
    }
    // Seeing the same expiry again cannot confirm progress or move a local deadline.
    assert!(renewal_reply(
        &row(renewed()),
        renewal_envelope(None, renewed(), "2023-11-14T22:13:35.000Z"),
        200
    )
    .is_err());
    assert!(renewal_reply(
        &row(active()),
        renewal_envelope(None, active(), "2023-11-14T22:13:21.000Z"),
        200
    )
    .is_err());
    for observed in [
        "2023-11-14T22:13:29.999Z",
        "2023-11-14T22:14:00.000Z",
        "2023-11-14T22:14:00.001Z",
        "2023-11-14T22:13:19.999Z",
    ] {
        let error = failure(renewal_reply(
            &row(active()),
            renewal_envelope(None, renewed(), observed),
            200,
        ));
        assert_eq!(error.constraint, Constraint::RenewalObservation);
    }
}

#[test]
fn renewal_binds_original_inputs_and_never_regresses_or_changes_known_terminals() {
    for (field, value) in [
        ("reservation_id", json!("abcdef0123456789abcdef0123456789")),
        ("account_id", json!("other")),
        ("maximum", json!("101")),
        ("timeout_seconds", json!(31)),
        (
            "idempotency_key",
            json!("v1.1700000000000.abcdef0123456789abcdef0123456789"),
        ),
        ("initial_expires_at", json!("2023-11-14T22:13:49.000Z")),
    ] {
        let mut fixture = renewed();
        fixture[field] = value;
        let error = failure(renewal_reply(
            &row(active()),
            renewal_envelope(None, fixture, "2023-11-14T22:13:30.000Z"),
            200,
        ));
        assert_eq!(error.constraint, Constraint::ReservationBinding);
    }
    assert!(renewal_reply(
        &row(renewed()),
        renewal_envelope(
            Some("LEDGER_RENEWAL_CLOSED"),
            terminal("settled", "50"),
            "2023-11-14T22:13:30.000Z"
        ),
        409
    )
    .is_err());
    let prior = row(terminal("settled", "50"));
    for (field, value) in [
        ("charged_amount", json!("51")),
        ("expires_at", json!("2023-11-14T22:13:51.000Z")),
        ("terminal_at", json!("2023-11-14T22:13:22.000Z")),
        ("retain_until", json!("2023-11-15T22:13:22.000Z")),
    ] {
        let mut changed = terminal("settled", "50");
        changed[field] = value;
        assert!(renewal_reply(
            &prior,
            renewal_envelope(
                Some("LEDGER_RENEWAL_CLOSED"),
                changed,
                "2023-11-14T22:13:30.000Z"
            ),
            409
        )
        .is_err());
    }
}

#[test]
fn terminal_receipts_accept_unconfirmed_extension_but_never_rollback_or_change_terminal() {
    let mut extended = terminal("settled", "50");
    extended["expires_at"] = json!("2023-11-14T22:14:00.000Z");
    extended["terminal_at"] = json!("2023-11-14T22:13:55.000Z");
    extended["retain_until"] = json!("2023-11-15T22:13:55.000Z");
    assert!(
        check_terminal(&row(active()), row(extended.clone()), Some(50), 200)
            .unwrap()
            .matches_requested
    );
    assert!(check_terminal(
        &row(renewed()),
        row(terminal("settled", "50")),
        Some(50),
        200
    )
    .is_err());
    let prior = row(extended.clone());
    assert!(check_terminal(&prior, row(extended.clone()), Some(50), 200).is_ok());
    extended["expires_at"] = json!("2023-11-14T22:14:00.001Z");
    assert!(check_terminal(&prior, row(extended), Some(50), 200).is_err());
}

#[test]
fn renewal_closed_preserves_all_terminal_outcomes_for_later_natural_completion() {
    for (state, charge, requested, matches) in [
        ("settled", "50", Some(50), true),
        ("settled", "50", Some(51), false),
        ("settled", "50", None, false),
        ("refunded", "0", None, true),
        ("refunded", "0", Some(0), false),
        ("expired", "0", Some(50), false),
        ("expired", "0", None, false),
    ] {
        let reply = renewal_reply(
            &row(active()),
            renewal_envelope(
                Some("LEDGER_RENEWAL_CLOSED"),
                terminal(state, charge),
                "2023-11-14T22:13:50.000Z",
            ),
            409,
        )
        .unwrap();
        let error = reply.refusal.unwrap();
        assert_eq!(error.kind, ErrorKind::Conflict);
        assert_eq!(error.constraint, Constraint::ActiveReservation);
        let receipt = reply.terminal_receipt(requested).unwrap();
        assert_eq!(receipt.matches_requested, matches);
        assert!(receipt.outcome == reply.reservation.terminal_outcome().unwrap());
        if let Some(diagnostic) = receipt.diagnostic {
            let value = serde_json::to_value(diagnostic).unwrap();
            assert_eq!(value["operation"], "intake_renew");
            assert_eq!(value["status"], 409);
            assert_eq!(value["stage"], "renewal");
        }
        assert!(!format!("{error:?} {error}").contains(HOSTILE));
    }
    let mut extended = terminal("settled", "50");
    extended["expires_at"] = json!("2023-11-14T22:14:00.000Z");
    extended["terminal_at"] = json!("2023-11-14T22:13:55.000Z");
    extended["retain_until"] = json!("2023-11-15T22:13:55.000Z");
    let reply = renewal_reply(
        &row(active()),
        renewal_envelope(
            Some("LEDGER_RENEWAL_CLOSED"),
            extended,
            "2023-11-14T22:13:56.000Z",
        ),
        409,
    )
    .unwrap();
    assert!(reply.terminal_receipt(Some(50)).unwrap().matches_requested);

    // An already observed terminal is absorbing, including its expiry.
    let prior = terminal("settled", "50");
    assert!(renewal_reply(
        &row(prior.clone()),
        renewal_envelope(
            Some("LEDGER_RENEWAL_CLOSED"),
            prior,
            "2023-11-14T22:13:50.000Z"
        ),
        409
    )
    .is_ok());
    assert!(renewal_reply(
        &row(active()),
        renewal_envelope(
            Some("LEDGER_RENEWAL_CLOSED"),
            terminal("settled", "50"),
            "2023-11-14T22:13:20.999Z"
        ),
        409
    )
    .is_err());
}

#[test]
fn renewal_active_refusals_never_grant_confirmation_and_precondition_never_adopts_regression() {
    for (code, fixture, observed, kind, constraint) in [
        (
            "LEDGER_RENEWAL_NO_PROGRESS",
            active(),
            "2023-11-14T22:13:20.000Z",
            ErrorKind::Conflict,
            Constraint::StrictlyLaterDeadline,
        ),
        (
            "LEDGER_ADMISSION_DISABLED",
            active(),
            "2023-11-14T22:13:21.000Z",
            ErrorKind::AdmissionDisabled,
            Constraint::Admission,
        ),
        (
            "LEDGER_ADMISSION_DISABLED",
            renewed(),
            "2023-11-14T22:13:30.000Z",
            ErrorKind::AdmissionDisabled,
            Constraint::Admission,
        ),
    ] {
        let reply = renewal_reply(
            &row(active()),
            renewal_envelope(Some(code), fixture, observed),
            409,
        )
        .unwrap();
        let error = reply.refusal.unwrap();
        assert_eq!(error.kind, kind);
        assert_eq!(error.constraint, constraint);
        assert!(reply.reservation.is_active());
        assert!(reply.terminal_receipt(None).is_none());
    }
    let error = failure(renewal_reply(
        &row(renewed()),
        renewal_envelope(
            Some("LEDGER_RENEWAL_PRECONDITION"),
            active(),
            "2023-11-14T22:13:21.000Z",
        ),
        409,
    ));
    assert_eq!(error.kind, ErrorKind::Conflict);
    assert_eq!(
        error.constraint,
        Constraint::ExpectedNotAfterAuthoritativeDeadline
    );
    // Future expected is not permitted for disabled, closed or no-progress decisions.
    for code in ["LEDGER_ADMISSION_DISABLED", "LEDGER_RENEWAL_NO_PROGRESS"] {
        assert!(renewal_reply(
            &row(renewed()),
            renewal_envelope(Some(code), active(), "2023-11-14T22:13:20.000Z"),
            409
        )
        .is_err());
    }
}

#[test]
fn renewal_status_code_state_and_observation_must_agree() {
    for (code, fixture, observed, status) in [
        (
            None,
            terminal("settled", "50"),
            "2023-11-14T22:13:30.000Z",
            200,
        ),
        (
            Some("LEDGER_RENEWAL_CLOSED"),
            active(),
            "2023-11-14T22:13:21.000Z",
            409,
        ),
        (
            Some("LEDGER_RENEWAL_PRECONDITION"),
            renewed(),
            "2023-11-14T22:13:30.000Z",
            409,
        ),
        (
            Some("LEDGER_RENEWAL_NO_PROGRESS"),
            active(),
            "2023-11-14T22:13:21.000Z",
            409,
        ),
        (
            Some("LEDGER_RENEWAL_NO_PROGRESS"),
            renewed(),
            "2023-11-14T22:13:30.000Z",
            409,
        ),
        (
            Some("LEDGER_ADMISSION_DISABLED"),
            terminal("refunded", "0"),
            "2023-11-14T22:13:30.000Z",
            409,
        ),
        (
            Some("LEDGER_ADMISSION_DISABLED"),
            active(),
            "2023-11-14T22:13:50.000Z",
            409,
        ),
        (
            Some("LEDGER_RENEWAL_CLOSED"),
            terminal("expired", "0"),
            "2023-11-14T22:13:49.999Z",
            409,
        ),
        (
            Some("LEDGER_RENEWAL_CLOSED"),
            terminal("expired", "0"),
            "2023-11-14T22:13:50.000Z",
            201,
        ),
        (
            Some("LEDGER_SETTLEMENT_EXPIRED"),
            terminal("expired", "0"),
            "2023-11-14T22:13:50.000Z",
            409,
        ),
        (Some(HOSTILE), renewed(), "2023-11-14T22:13:30.000Z", 409),
    ] {
        let error = failure(renewal_reply(
            &row(active()),
            renewal_envelope(code, fixture, observed),
            status,
        ));
        assert_eq!(error.kind, ErrorKind::Protocol);
        assert_eq!(error.status, Some(status));
    }
}

#[test]
fn renewal_requires_complete_known_schema_and_discards_remote_strings() {
    for code in [None, Some("LEDGER_RENEWAL_NO_PROGRESS")] {
        let fixture = renewal_envelope(
            code,
            if code.is_none() { renewed() } else { active() },
            if code.is_none() {
                "2023-11-14T22:13:30.000Z"
            } else {
                "2023-11-14T22:13:20.000Z"
            },
        );
        let status = if code.is_none() { 200 } else { 409 };
        for field in fixture.as_object().unwrap().keys() {
            let mut missing = fixture.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(renewal_reply(&row(active()), missing, status).is_err());
            let mut null = fixture.clone();
            null[field] = Value::Null;
            assert!(renewal_reply(&row(active()), null, status).is_err());
            let response = Response::fixture(
                Operation::Renew,
                status,
                duplicate(fixture.clone(), field).into_bytes(),
            );
            assert!(check_renew_response(&row(active()), &response).is_err());
        }
        for bad in [
            json!(42),
            json!("2023-11-14T22:13:30Z"),
            json!("9999-01-01T00:00:00.000Z"),
        ] {
            let mut malformed = fixture.clone();
            malformed["observed_at"] = bad;
            assert!(renewal_reply(&row(active()), malformed, status).is_err());
        }
        let mut additive = fixture;
        additive["ignored"] = json!({"credential": HOSTILE});
        let reply = renewal_reply(&row(active()), additive, status).unwrap();
        if let Some(error) = reply.refusal {
            assert!(!format!("{error:?} {error}").contains(HOSTILE));
        }
    }
    // Invalid input/evidence/writer/completion errors cannot manufacture a row.
    for (code, status, kind, constraint) in [
        (
            "LEDGER_INPUT_INVALID",
            400,
            ErrorKind::Input,
            Constraint::Input,
        ),
        (
            "LEDGER_EVIDENCE_UNAVAILABLE",
            410,
            ErrorKind::EvidenceUnavailable,
            Constraint::Evidence,
        ),
        (
            "LEDGER_WRITER_UNAVAILABLE",
            503,
            ErrorKind::RemoteUnavailable,
            Constraint::WriterWait,
        ),
        (
            "LEDGER_WORK_CANCELLED",
            503,
            ErrorKind::RemoteUnavailable,
            Constraint::WorkBudget,
        ),
        (
            "LEDGER_STORAGE_UNAVAILABLE",
            503,
            ErrorKind::RemoteUnavailable,
            Constraint::UsableStorage,
        ),
        (
            "LEDGER_COMMIT_UNKNOWN",
            503,
            ErrorKind::RemoteUnavailable,
            Constraint::CommitEvidence,
        ),
    ] {
        let error = failure(renewal_reply(
            &row(active()),
            json!({"code":code, "message":HOSTILE}),
            status,
        ));
        assert_eq!(error.kind, kind);
        assert_eq!(error.constraint, constraint);
        assert_eq!(error.operation(), "intake_renew");
        assert!(!format!("{error:?} {error}").contains(HOSTILE));
    }
}

#[test]
fn documented_code_status_pairs_are_closed_and_hostile_text_never_escapes() {
    for (code, status, kind, constraint) in [
        (
            "LEDGER_HTTP_UNAUTHORIZED",
            401,
            ErrorKind::Authentication,
            Constraint::RemoteCauseUnknown,
        ),
        (
            "LEDGER_ACCOUNT_UNKNOWN",
            404,
            ErrorKind::AccountUnavailable,
            Constraint::Account,
        ),
        (
            "LEDGER_CREDIT_INSUFFICIENT",
            409,
            ErrorKind::CreditInsufficient,
            Constraint::Credit,
        ),
        (
            "LEDGER_ADMISSION_DISABLED",
            409,
            ErrorKind::AdmissionDisabled,
            Constraint::Admission,
        ),
        (
            "LEDGER_KEY_CONFLICT",
            409,
            ErrorKind::Conflict,
            Constraint::Idempotency,
        ),
        (
            "LEDGER_KEY_WINDOW",
            409,
            ErrorKind::Conflict,
            Constraint::KeyWindow,
        ),
        (
            "LEDGER_EVIDENCE_UNAVAILABLE",
            410,
            ErrorKind::EvidenceUnavailable,
            Constraint::Evidence,
        ),
        (
            "LEDGER_COMMIT_UNKNOWN",
            503,
            ErrorKind::RemoteUnavailable,
            Constraint::CommitEvidence,
        ),
        (
            "LEDGER_STORAGE_UNAVAILABLE",
            503,
            ErrorKind::RemoteUnavailable,
            Constraint::UsableStorage,
        ),
        (
            "LEDGER_WRITER_UNAVAILABLE",
            503,
            ErrorKind::RemoteUnavailable,
            Constraint::WriterWait,
        ),
        (
            "LEDGER_HTTP_HEADER_REJECTED",
            400,
            ErrorKind::Input,
            Constraint::SingletonHeaders,
        ),
        (
            HOSTILE,
            502,
            ErrorKind::RemoteUnavailable,
            Constraint::RemoteCauseUnknown,
        ),
        (
            HOSTILE,
            409,
            ErrorKind::Protocol,
            Constraint::RemoteCauseUnknown,
        ),
        (
            "LEDGER_CREDIT_INSUFFICIENT",
            403,
            ErrorKind::Protocol,
            Constraint::RemoteCauseUnknown,
        ),
    ] {
        let value = json!({"code": code, "message": HOSTILE, "stage": HOSTILE, "constraint": HOSTILE, "outcome": HOSTILE, "next_step": HOSTILE, "token": HOSTILE});
        let diagnostic: Diagnostic =
            serde_json::from_value(value).unwrap_or_else(|_| panic!("synthetic diagnostic schema"));
        let error = classify_remote(diagnostic.code, status, Operation::Reserve);
        assert_eq!(error.kind, kind);
        assert_eq!(error.constraint, constraint);
        assert_eq!(error.status, Some(status));
        let display = error.to_string();
        let debug = format!("{error:?}");
        assert!(!display.contains(HOSTILE) && !debug.contains(HOSTILE));
        assert!(display.contains("No automatic replay") && display.contains("another charge"));
        assert!(display.contains("outcome=unknown"));
    }
    assert!(serde_json::from_str::<Diagnostic>(
        r#"{"code":"LEDGER_COMMIT_UNKNOWN","code":"LEDGER_CREDIT_INSUFFICIENT"}"#
    )
    .is_err());
    assert!(serde_json::from_str::<Diagnostic>(r#"{"code":null}"#).is_err());
    let native = classify_remote(RemoteCode::Native, 401, Operation::Refresh);
    assert_eq!(native.kind, ErrorKind::Authentication);
    assert_eq!(native.constraint, Constraint::RemoteCauseUnknown);
    let native = classify_remote(RemoteCode::Native, 400, Operation::Login);
    assert_eq!(native.kind, ErrorKind::Authentication);
    assert_eq!(native.constraint, Constraint::RemoteCauseUnknown);
}

#[test]
fn transport_projection_survives_without_raw_source_errors() {
    for constraint in [
        transport::Constraint::Deadline,
        transport::Constraint::ResponseLimit,
        transport::Constraint::ResponseJson,
        transport::Constraint::ResponseReadUnknown,
    ] {
        let failure = transport::Failure {
            operation: Operation::Settle,
            constraint,
            status: Some(409),
            dispatch_attempted: true,
        };
        let error = Error::from(failure);
        assert_eq!(error.transport, Some(failure));
        assert_eq!(error.status, Some(409));
        assert_eq!(error.operation(), "intake_settle");
        assert_eq!(
            error.kind,
            if constraint == transport::Constraint::ResponseJson {
                ErrorKind::Protocol
            } else {
                ErrorKind::Transport
            }
        );
        assert!(error.to_string().contains("outcome=unknown"));
    }
    let error = Error::from(transport::Failure::input(Operation::Reserve));
    assert!(!error.dispatch_attempted);
    assert_eq!(error.status, None);
    assert_eq!(error.kind, ErrorKind::Input);
}
