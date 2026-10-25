//! Paired public adapter calls against exact committed Go/PocketBase source.
//! Only disposable loopback TLS; PRIVACY.md data boundaries/synthetic exception.
use crate::intake::{
    protocol::{Constraint, ErrorKind, Intake, Reservation, Stage, TerminalOutcome},
    transport::{IntakeClient, Request},
};
use base64::Engine;
use serde_json::Value;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const KEY: &str = "synthetic-ledger-key-0000000000000000";

async fn wire(client: &IntakeClient, reservation: &Reservation) -> Value {
    let response = client
        .send(Request::Lookup(&reservation.reservation_id))
        .await
        .unwrap();
    assert_eq!(response.status.as_u16(), 200);
    response.decode::<Value>().unwrap()["reservation"].clone()
}

fn immutable(original: &Value, current: &Value) {
    for field in [
        "reservation_id",
        "account_id",
        "maximum",
        "timeout_seconds",
        "idempotency_key",
        "initial_expires_at",
    ] {
        assert!(
            original[field] == current[field],
            "immutable renewal binding"
        );
    }
}

#[tokio::test]
#[ignore = "requires gateway-owned disposable Go/PocketBase TLS fixture"]
async fn actual_intake_renewal_http() {
    let origin = std::env::var("GATEWAY_INTAKE_FIXTURE_ORIGIN").unwrap();
    assert_eq!(
        reqwest::Url::parse(&origin).unwrap().host_str(),
        Some("127.0.0.1")
    );
    let certificate = base64::engine::general_purpose::STANDARD
        .decode(std::env::var("GATEWAY_INTAKE_FIXTURE_CA").unwrap())
        .unwrap();
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(rustls::pki_types::CertificateDer::from(certificate))
        .unwrap();
    let intake = Intake::fixture(IntakeClient::fixture(
        &origin,
        KEY,
        roots.clone(),
        Duration::from_secs(5),
    ));
    let client = IntakeClient::fixture(&origin, KEY, roots, Duration::from_secs(5));
    let issued = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let key = |n| format!("v1.{issued:013}.{n:032x}");

    let original = intake
        .reserve(
            &super::fixture_account("RENEWAL_SUCCESS"),
            40,
            1800,
            &key(1),
        )
        .await
        .unwrap();
    let original_wire = wire(&client, &original).await;
    assert!(original_wire["initial_expires_at"] == original_wire["expires_at"]);
    let first = intake.renew(&original).await.unwrap();
    assert!(first.refusal.is_none());
    assert!(first.terminal_receipt(Some(17)).is_none());
    assert_eq!(
        first
            .reservation
            .expires_at()
            .duration_since(first.observed_at)
            .unwrap(),
        Duration::from_secs(1800)
    );
    assert_eq!(
        first
            .reservation
            .expires_at()
            .duration_since(original.expires_at())
            .unwrap(),
        Duration::from_secs(600)
    );
    immutable(&original_wire, &wire(&client, &first.reservation).await);

    // Effective SQLite time is max(wall clock, persisted high-water mark).
    // The fixture leaves wall time behind that mark; no sleeping or resetting
    // the watermark to simulate progress, and no fabricated local confirmation.
    let no_progress = intake.renew(&first.reservation).await.unwrap();
    let refusal = no_progress.refusal.unwrap();
    assert_eq!(refusal.kind, ErrorKind::Conflict);
    assert_eq!(refusal.stage, Stage::Renewal);
    assert_eq!(refusal.constraint, Constraint::StrictlyLaterDeadline);
    assert_eq!(refusal.status, Some(409));
    assert_eq!(no_progress.observed_at, first.observed_at);
    assert_eq!(
        no_progress.reservation.expires_at(),
        first.reservation.expires_at()
    );
    assert!(no_progress.terminal_receipt(None).is_none());

    let second = intake.renew(&first.reservation).await.unwrap();
    assert!(second.refusal.is_none());
    assert_eq!(
        second
            .reservation
            .expires_at()
            .duration_since(first.reservation.expires_at())
            .unwrap(),
        Duration::from_secs(600)
    );
    let latest_wire = wire(&client, &second.reservation).await;
    immutable(&original_wire, &latest_wire);
    // Both original and reordered intermediate requests return the newest row,
    // not an old cached reply and not another extension.
    for captured in [&original, &first.reservation] {
        let duplicate = intake.renew(captured).await.unwrap();
        assert!(duplicate.refusal.is_none());
        assert_eq!(
            duplicate.reservation.expires_at(),
            second.reservation.expires_at()
        );
        assert!(wire(&client, &duplicate.reservation).await == latest_wire);
    }
    let settled = intake.finish(&second.reservation, Some(17)).await.unwrap();
    assert!(settled.matches_requested);
    assert!(matches!(
        settled.outcome,
        TerminalOutcome::Settled { charged: 17 }
    ));
    let closed = intake.renew(&second.reservation).await.unwrap();
    assert!(closed.refusal.is_some());
    let actual = closed.terminal_receipt(None).unwrap();
    assert!(!actual.matches_requested);
    assert!(matches!(
        actual.outcome,
        TerminalOutcome::Settled { charged: 17 }
    ));
    assert!(closed.terminal_receipt(Some(17)).unwrap().matches_requested);
    immutable(&original_wire, &wire(&client, &closed.reservation).await);

    let disabled = intake
        .reserve(
            &super::fixture_account("RENEWAL_DISABLED"),
            40,
            1800,
            &key(2),
        )
        .await
        .unwrap();
    let refused = intake.renew(&disabled).await.unwrap();
    let error = refused.refusal.unwrap();
    assert_eq!(error.kind, ErrorKind::AdmissionDisabled);
    assert_eq!(error.status, Some(409));
    assert!(refused.reservation.is_active());
    assert_eq!(refused.reservation.expires_at(), disabled.expires_at());
    assert!(refused.terminal_receipt(None).is_none());
    let settled = intake.finish(&disabled, Some(17)).await.unwrap();
    assert!(settled.matches_requested);
    let closed = intake.renew(&disabled).await.unwrap();
    assert!(matches!(
        closed.terminal_receipt(Some(17)).unwrap().outcome,
        TerminalOutcome::Settled { charged: 17 }
    ));

    let refund = intake
        .reserve(&super::fixture_account("RENEWAL_REFUND"), 40, 1800, &key(3))
        .await
        .unwrap();
    assert!(
        intake
            .finish(&refund, None)
            .await
            .unwrap()
            .matches_requested
    );
    let closed = intake.renew(&refund).await.unwrap();
    assert!(closed.refusal.is_some());
    let actual = closed.terminal_receipt(Some(17)).unwrap();
    assert!(!actual.matches_requested);
    assert!(matches!(actual.outcome, TerminalOutcome::Refunded));
    assert!(closed.terminal_receipt(None).unwrap().matches_requested);

    let lost = intake
        .reserve(&super::fixture_account("RENEWAL_LOST"), 40, 1800, &key(4))
        .await
        .unwrap();
    let error = match intake.renew(&lost).await {
        Ok(_) => panic!("lost renewal must not fabricate a usable reply"),
        Err(error) => error,
    };
    assert_eq!(error.status, Some(503));
    assert!(error.dispatch_attempted);
    assert_eq!(error.kind, ErrorKind::RemoteUnavailable);
    assert!(!error.to_string().contains("SYNTHETIC_PRIVATE_SENTINEL"));
    // No retry/recovery is initiated. At natural completion the single finish
    // call still works with the last confirmed snapshot; Go separately proves
    // the renewal really committed and was attempted only once.
    assert!(intake.finish(&lost, None).await.unwrap().matches_requested);

    // Last: the controlled forward jump expires the current lease exactly.
    let expired = intake
        .reserve(
            &super::fixture_account("RENEWAL_EXPIRED"),
            40,
            1800,
            &key(5),
        )
        .await
        .unwrap();
    let closed = intake.renew(&expired).await.unwrap();
    assert!(closed.refusal.is_some());
    let actual = closed.terminal_receipt(Some(17)).unwrap();
    assert!(!actual.matches_requested);
    assert!(matches!(actual.outcome, TerminalOutcome::Expired));
    let late = intake.finish(&expired, Some(17)).await.unwrap();
    assert!(!late.matches_requested);
    assert!(matches!(late.outcome, TerminalOutcome::Expired));
}
