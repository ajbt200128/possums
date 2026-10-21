//! Isolated scripted service for ownership tests, not a production transport.
//! HTTPS/protocol qualification remains the parent's separate integration gate.
use super::*;
use crate::intake::{Reservation, TerminalOutcome, TerminalReceipt};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::oneshot;

type ReserveReply = Result<Reservation, AccountingError>;
type FinishReply = Result<TerminalReceipt, AccountingError>;
pub(crate) type RenewReply = Result<crate::intake::RenewalReply, AccountingError>;
pub(crate) struct Service {
    pub(crate) reserves: AtomicUsize,
    pub(crate) finishes: AtomicUsize,
    pub(crate) renews: AtomicUsize,
    pub(crate) panic_renew: std::sync::atomic::AtomicBool,
    renew: Mutex<std::collections::VecDeque<oneshot::Receiver<RenewReply>>>,
    renew_started: tokio::sync::mpsc::UnboundedSender<SystemTime>,
    reserve: Mutex<Option<oneshot::Receiver<ReserveReply>>>,
    reserve_started: Mutex<Option<oneshot::Sender<(String, u64)>>>,
    finish: Mutex<Option<oneshot::Receiver<FinishReply>>>,
    finish_started: Mutex<Option<oneshot::Sender<Option<u64>>>>,
}
impl Service {
    pub(super) async fn renew(&self, reservation: &Reservation) -> RenewReply {
        self.renews.fetch_add(1, Ordering::SeqCst);
        assert!(
            !self.panic_renew.load(Ordering::SeqCst),
            "synthetic actor fault"
        );
        let receiver = self.renew.lock().unwrap().pop_front();
        let _ = self.renew_started.send(reservation.expires_at());
        match receiver {
            Some(receiver) => receiver
                .await
                .unwrap_or(Err(AccountingError::StateUnavailable)),
            None => Err(AccountingError::StateUnavailable),
        }
    }
    pub(super) async fn reserve(&self, key: &str, maximum: u64) -> ReserveReply {
        self.reserves.fetch_add(1, Ordering::SeqCst);
        let receiver = self.reserve.lock().unwrap().take().expect("one reserve");
        let _ = self
            .reserve_started
            .lock()
            .unwrap()
            .take()
            .unwrap()
            .send((key.to_owned(), maximum));
        receiver
            .await
            .unwrap_or(Err(AccountingError::StateUnavailable))
    }
    pub(super) async fn finish(&self, charge: Option<u64>) -> FinishReply {
        self.finishes.fetch_add(1, Ordering::SeqCst);
        let receiver = self.finish.lock().unwrap().take().expect("one finish");
        let _ = self
            .finish_started
            .lock()
            .unwrap()
            .take()
            .unwrap()
            .send(charge);
        receiver
            .await
            .unwrap_or(Err(AccountingError::StateUnavailable))
    }
}
pub(crate) struct Script {
    pub(crate) accounting: Arc<Accounting>,
    pub(crate) service: Arc<Service>,
    pub(crate) reserve: oneshot::Sender<ReserveReply>,
    pub(crate) reserve_started: oneshot::Receiver<(String, u64)>,
    pub(crate) finish: oneshot::Sender<FinishReply>,
    pub(crate) finish_started: oneshot::Receiver<Option<u64>>,
    pub(crate) renew: std::collections::VecDeque<oneshot::Sender<RenewReply>>,
    pub(crate) renew_started: tokio::sync::mpsc::UnboundedReceiver<SystemTime>,
}
pub(crate) fn script() -> Script {
    script_with_renewals(0)
}
pub(crate) fn renewing_script() -> Script {
    script_with_renewals(4)
}
fn script_with_renewals(steps: usize) -> Script {
    let (reserve, reserved) = oneshot::channel();
    let (reserve_started, started_reserve) = oneshot::channel();
    let (finish, finished) = oneshot::channel();
    let (finish_started, started_finish) = oneshot::channel();
    let (renew_started, started_renew) = tokio::sync::mpsc::unbounded_channel();
    let (renew, receivers) = (0..steps).map(|_| oneshot::channel()).unzip();
    let service = Arc::new(Service {
        reserves: AtomicUsize::new(0),
        finishes: AtomicUsize::new(0),
        renews: AtomicUsize::new(0),
        panic_renew: std::sync::atomic::AtomicBool::new(false),
        renew: Mutex::new(receivers),
        renew_started,
        reserve: Mutex::new(Some(reserved)),
        reserve_started: Mutex::new(Some(reserve_started)),
        finish: Mutex::new(Some(finished)),
        finish_started: Mutex::new(Some(finish_started)),
    });
    // Constructing HTTPS configuration never makes a request. The test-only
    // service intercepts every RPC; there is no fixture origin in production.
    let mut remote = Remote::new(Arc::new(
        crate::intake::Intake::new("https://synthetic.invalid", &"x".repeat(32)).unwrap(),
    ));
    remote.service = Some(service.clone());
    Script {
        accounting: Arc::new(Accounting(Backend::Remote(remote))),
        service,
        reserve,
        reserve_started: started_reserve,
        finish,
        finish_started: started_finish,
        renew,
        renew_started: started_renew,
    }
}
pub(crate) fn reservation(key: &str, remaining: u64, state: &str) -> Reservation {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let date = |millis: u64| {
        let format = time::format_description::parse_borrowed::<2>(
            "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z",
        )
        .unwrap();
        time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(millis) * 1_000_000)
            .unwrap()
            .format(&format)
            .unwrap()
    };
    serde_json::from_value(serde_json::json!({
        "reservation_id":"0123456789abcdef0123456789abcdef", "account_id":"a", "maximum":"52",
        "timeout_seconds":1800, "idempotency_key":key,
        "initial_expires_at":date(now + remaining * 1000), "expires_at":date(now + remaining * 1000),
        "state":state, "charged_amount": match state { "settled" => Some("29"), "active" => None, _ => Some("0") },
        "terminal_at": if state == "active" { None } else { Some(date(now)) },
        "retain_until": if state == "active" { None } else { Some(date(now + 86400000)) },
    })).unwrap_or_else(|_| panic!("synthetic reservation schema"))
}
pub(crate) fn receipt(outcome: TerminalOutcome, matches_requested: bool) -> FinishReply {
    Ok(TerminalReceipt {
        outcome,
        matches_requested,
        diagnostic: None,
    })
}

#[test]
fn claims_have_one_independent_payload_and_no_balance_authority() {
    let Script { accounting, .. } = script();
    let quote = crate::catalog::Quote {
        model: crate::catalog::Model {
            id: "m".into(),
            public_name: None,
            reasoning: None,
            reasoning_params: None,
            context_tokens: 100,
            max_output_tokens: 10,
            input_microunits_per_million_tokens: 2_000_000,
            output_microunits_per_million_tokens: 3_000_000,
            cached_input_microunits_per_million_tokens: Some(0),
        },
        input_tokens: 5,
        reserved_microunits: 52,
    };
    let expiry = Instant::now() + std::time::Duration::from_secs(60);
    assert_eq!(
        accounting.reserve("a", [1; 32], [2; 32], quote.clone(), expiry),
        Ok(ReserveResult::Claimed)
    );
    let first = accounting.take_attempt([1; 32]).unwrap();
    let parts: Vec<_> = first.key.split('.').collect();
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0], "v1");
    assert_eq!(parts[1].len(), 13);
    assert!(parts[1].bytes().all(|b| b.is_ascii_digit()));
    assert_eq!(parts[2].len(), 32);
    assert!(parts[2]
        .bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
    assert!(accounting.take_attempt([1; 32]).is_err());
    assert_eq!(
        accounting.reserve("a", [1; 32], [2; 32], quote.clone(), expiry),
        Ok(ReserveResult::Duplicate(Outcome::InFlight))
    );
    assert_eq!(
        accounting.reserve("a", [1; 32], [3; 32], quote.clone(), expiry),
        Err(AccountingError::AlteredDuplicate)
    );
    assert_eq!(
        accounting.reserve("b", [1; 32], [2; 32], quote.clone(), expiry),
        Err(AccountingError::AlteredDuplicate)
    );
    assert_eq!(
        accounting.reserve("a", [4; 32], [2; 32], quote, expiry),
        Ok(ReserveResult::Claimed)
    );
    assert_ne!(first.key, accounting.take_attempt([4; 32]).unwrap().key);
    assert_eq!(accounting.available("a"), None);
    assert_eq!(
        accounting.snapshot("a"),
        Err(AccountingError::InvalidTransition)
    );
    assert_eq!(
        accounting.finish([1; 32], None),
        Err(AccountingError::InvalidTransition)
    );
    let usage = Some(FinalUsage {
        input_tokens: 5,
        output_tokens: 4,
        total_tokens: 9,
    });
    assert_eq!(first.charge(usage, 2), Some(24));
    assert_eq!(first.charge(usage, 6), None);
    assert_eq!(
        first.charge(
            Some(FinalUsage {
                input_tokens: 100,
                output_tokens: 100,
                total_tokens: 200
            }),
            0
        ),
        Some(52)
    );
    accounting.record_remote([1; 32], Outcome::Unknown).unwrap();
    accounting
        .record_remote([1; 32], Outcome::Refunded)
        .unwrap();
    // First local terminal also absorbs later bookkeeping attempts.
    let Backend::Remote(remote) = &accounting.0 else {
        unreachable!()
    };
    assert_eq!(
        remote.claims.lock().unwrap()[&[1; 32]].outcome,
        Outcome::Unknown
    );
}
