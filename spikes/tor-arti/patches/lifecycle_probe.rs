//! Test-only probe inserted into the real released arti-client crate in scratch.
//! This is NOT a replacement fatal handler and never constructs a TorClient.
use super::*;
use futures::{future::pending, poll, stream, stream::BoxStream};
use possums_tor_arti_spike::{run_bounded, End, Failure};
use std::{
    num::NonZeroUsize,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    },
    time::Duration,
};
use tokio_util::sync::CancellationToken;
use tor_netdir::{params::NetParameters, NetDir, NetDirProvider, Timeliness};

// Synthetic output of the authenticated-status boundary, NOT an authenticated
// consensus. Real signature checking, timestamp checks and production wiring are
// unchanged. None of these trait methods can bootstrap or open a socket.
struct Fixture {
    timestamp: SystemTime,
    statuses: Arc<ProtoStatuses>,
}

impl Fixture {
    fn new(required: &str, recommended: &str, timestamp: SystemTime) -> Arc<Self> {
        let statuses = serde_json::from_value(serde_json::json!({
            "client": {"required": required, "recommended": recommended},
            "relay": {"required": "", "recommended": ""}
        }))
        .unwrap();
        Arc::new(Self {
            timestamp,
            statuses: Arc::new(statuses),
        })
    }
}

#[async_trait::async_trait]
impl NetDirProvider for Fixture {
    fn netdir(&self, _: Timeliness) -> tor_netdir::Result<Arc<NetDir>> {
        Err(tor_netdir::Error::NoInfo)
    }
    fn events(&self) -> BoxStream<'static, DirEvent> {
        Box::pin(stream::empty())
    }
    fn params(&self) -> Arc<dyn AsRef<NetParameters>> {
        Arc::new(NetParameters::default())
    }
    fn protocol_statuses(&self) -> Option<(SystemTime, Arc<ProtoStatuses>)> {
        Some((self.timestamp, Arc::clone(&self.statuses)))
    }
}

#[async_trait::async_trait]
impl DirProvider for Fixture {
    fn reconfigure(
        &self,
        _: &tor_dirmgr::DirMgrConfig,
        _: tor_config::Reconfigure,
    ) -> Result<(), tor_config::ReconfigureError> {
        Ok(())
    }
    async fn bootstrap(&self) -> tor_dirmgr::Result<()> {
        panic!("fixture must never bootstrap")
    }
    fn bootstrap_events(&self) -> BoxStream<'static, tor_dirmgr::DirBootstrapStatus> {
        Box::pin(stream::empty())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fatal {
    RequiredProtocol,
    CauseUnknown,
}
fn closed_failure(error: ErrorDetail) -> Fatal {
    match error {
        ErrorDetail::MissingProtocol(ProtocolSupportError::MissingRequired(_)) => {
            Fatal::RequiredProtocol
        }
        _ => Fatal::CauseUnknown,
    }
}
struct Active(Arc<AtomicUsize>);
impl Active {
    fn new(count: &Arc<AtomicUsize>) -> Self {
        count.fetch_add(1, Ordering::SeqCst);
        Self(Arc::clone(count))
    }
}
impl Drop for Active {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
fn publication() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(10)
}

#[tokio_crate::test(crate = "tokio_crate")]
async fn cached_required_protocol_is_rejected_without_callback_or_spawn() {
    let provider = Fixture::new("Relay=3", "", publication());
    let calls = Arc::new(AtomicUsize::new(0));
    let called = Arc::clone(&calls);
    let runtime = tor_rtcompat::PreferredRuntime::current().unwrap();
    let result = enforce_protocol_recommendations(
        &runtime,
        provider,
        publication(),
        "Relay=1".parse().unwrap(),
        move |_| async move {
            called.fetch_add(1, Ordering::SeqCst);
        },
    );
    assert_eq!(closed_failure(result.unwrap_err()), Fatal::RequiredProtocol);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio_crate::test(crate = "tokio_crate")]
async fn later_fatal_closes_bounded_application_work_but_not_other_runtime_owners() {
    // This test demonstrates why callback-only shutdown is insufficient. It
    // does NOT claim that this synthetic task is an actual channel reactor.
    let runtime = tor_rtcompat::PreferredRuntime::current().unwrap();
    let subsystem_active = Arc::new(AtomicUsize::new(0));
    let guard = Active::new(&subsystem_active);
    let subsystem_stop = CancellationToken::new();
    let subsystem_signal = subsystem_stop.clone();
    let (finished_tx, finished_rx) = futures::channel::oneshot::channel();
    // Actual Arti Runtime::spawn API: returns no join/abort handle.
    runtime
        .spawn(async move {
            subsystem_signal.cancelled().await;
            drop(guard);
            let _ = finished_tx.send(());
        })
        .unwrap();
    let billing_outcome: Option<bool> = None;
    let host_api = async { 7 };
    let application_active = Arc::new(AtomicUsize::new(0));
    let admitted = AtomicUsize::new(0);
    let stop = CancellationToken::new();
    let app = run_bounded(
        stream::repeat(()).inspect(|_| {
            admitted.fetch_add(1, Ordering::SeqCst);
        }),
        NonZeroUsize::new(2).unwrap(),
        stop.clone(),
        |_| {
            let guard = Active::new(&application_active);
            async move {
                let _guard = guard;
                pending::<Result<(), Failure>>().await
            }
        },
    );
    tokio_crate::pin!(app);
    assert!(poll!(&mut app).is_pending());
    assert_eq!(admitted.load(Ordering::SeqCst), 2);

    let terminal = Arc::new(Mutex::new(None));
    let notification = Arc::clone(&terminal);
    let events_polled = AtomicUsize::new(0);
    let events = stream::iter([DirEvent::NewProtocolRecommendation; 3]).inspect(|_| {
        events_polled.fetch_add(1, Ordering::SeqCst);
    });
    // Provider ownership is retained through the watcher, as in real use.
    let provider = Fixture::new("HostileSecret=3", "", publication());
    watch_protocol_statuses(
        provider.clone(),
        events,
        None,
        publication(),
        "Relay=1".parse().unwrap(),
        move |error| async move {
            *notification.lock().unwrap() = Some(closed_failure(error));
            stop.cancel();
        },
    )
    .await;
    assert_eq!(*terminal.lock().unwrap(), Some(Fatal::RequiredProtocol));
    assert!(!format!("{:?}", terminal.lock().unwrap()).contains("HostileSecret"));
    assert_eq!(events_polled.load(Ordering::SeqCst), 1);
    assert_eq!(app.await, Ok(End::Cancelled));
    assert_eq!(application_active.load(Ordering::SeqCst), 0);
    assert_eq!(admitted.load(Ordering::SeqCst), 2);

    // Host API/accounting-like owners survive. No synthetic accounting action
    // or receipt was observed, so interruption must not manufacture an outcome.
    assert_eq!(host_api.await, 7);
    assert_eq!(billing_outcome, None);
    assert_eq!(subsystem_active.load(Ordering::SeqCst), 1);
    assert!(!subsystem_stop.is_cancelled());
    // The runtime also admits a new task after the protocol monitor returned.
    let (new_tx, new_rx) = futures::channel::oneshot::channel();
    runtime
        .spawn(async move {
            let _ = new_tx.send(());
        })
        .unwrap();
    new_rx.await.unwrap();
    // Explicit test cleanup, not a library shutdown guarantee.
    subsystem_stop.cancel();
    finished_rx.await.unwrap();
    assert_eq!(subsystem_active.load(Ordering::SeqCst), 0);
    // No detached test owner remains, even though callback alone didn't stop it.
}

#[tokio_crate::test(crate = "tokio_crate")]
async fn stale_status_and_missing_recommendation_are_not_fatal() {
    for (required, recommended, timestamp) in [
        ("Relay=3", "", SystemTime::UNIX_EPOCH),
        ("Relay=1", "Relay=3", publication()),
    ] {
        let provider = Fixture::new(required, recommended, timestamp);
        let calls = AtomicUsize::new(0);
        watch_protocol_statuses(
            provider.clone(),
            stream::iter([DirEvent::NewProtocolRecommendation; 2]),
            None,
            publication(),
            "Relay=1".parse().unwrap(),
            |_| async {
                calls.fetch_add(1, Ordering::SeqCst);
            },
        )
        .await;
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
}
