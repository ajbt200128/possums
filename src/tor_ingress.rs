//! Optional, unwired ingress. The host owns bootstrap, protected key custody,
//! exclusive publication and transient reporting; this module grants no trust.
use std::{future::Future, io, panic::AssertUnwindSafe, sync::Arc, time::Duration};

use arti_client::TorClient;
use futures_util::{stream, FutureExt, Stream, StreamExt};
use tokio::{net::TcpListener, sync::watch};
use tokio_util::compat::FuturesAsyncReadCompatExt;
use tor_cell::relaycell::msg::{Connected, End, EndReason};
use tor_hscrypto::pk::{HsId, HsIdKey, HsIdKeypair};
use tor_hsservice::{
    status::{Problem, State},
    HsIdKeypairSpecifier, HsIdPublicKeySpecifier, HsNickname, OnionServiceConfig, RendRequest,
    StreamRequest,
};
use tor_keymgr::KeyMgr;
use tor_proto::stream::IncomingStreamRequest;
use tor_rtcompat::PreferredRuntime;

use crate::{
    server::{serve_connections_until, AppState, Incoming, ServerFailure},
    telemetry::startup::readiness::{self, Observer, ProblemKind, Terminal},
};

/// Closed diagnostics only. No SDK error, peer, key, argument or payload is kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Failure {
    #[error("code=tor_identity_read_unknown stage=identity constraint=keymgr_read cause=unknown action=check_protected_state_offline; no service launched")]
    IdentityRead,
    #[error("code=tor_identity_missing stage=identity constraint=existing_private_public_pair action=restore_approved_keys_offline; no replacement generated or service launched")]
    IdentityMissing,
    #[error("code=tor_identity_mismatch stage=identity constraint=pair_public_approved_equality action=review_protected_state_offline; no service launched")]
    IdentityMismatch,
    #[error("code=tor_launch_unknown stage=launch constraint=service_launch cause=unknown action=check_host_service_offline; launch outcome unknown; not replayed")]
    Launch,
    #[error("code=tor_service_disabled stage=launch constraint=enabled_service action=review_approved_host_configuration; no service launched")]
    Disabled,
    #[error("code=tor_rendezvous_accept_unknown stage=rendezvous constraint=peer_accept cause=unknown action=check_connection_before_deliberate_new_submission; peer dropped; not replayed; prior billing unknown without authenticated receipts")]
    RendezvousAccept,
    #[error("code=tor_rendezvous_reject_unknown stage=rendezvous_reject constraint=peer_reject cause=unknown action=check_host_service; peer dropped; no HTTP request admitted")]
    RendezvousReject,
    #[error("code=tor_circuit_unknown stage=circuit constraint=peer_source_poll cause=unknown action=check_connection_before_deliberate_new_submission; peer dropped; not replayed; accepted-work billing unknown without authenticated receipts")]
    Circuit,
    #[error("code=tor_port_refused stage=stream constraint=begin_port_80 action=use_approved_service_port; stream rejected; no HTTP request admitted")]
    PortRefused,
    #[error("code=tor_stream_accept_unknown stage=stream_accept constraint=peer_accept cause=unknown action=check_connection_before_deliberate_new_submission; peer dropped; not replayed; prior billing unknown without authenticated receipts")]
    StreamAccept,
    #[error("code=tor_stream_reject_unknown stage=stream_reject constraint=peer_reject cause=unknown action=check_host_service; peer dropped; no HTTP request admitted")]
    StreamReject,
    #[error("code=tor_source_ended stage=rendezvous_source constraint=live_source action=check_host_service; admission stopped; shared drain required; accepted-work outcomes unknown without authenticated receipts")]
    SourceEnded,
    #[error("code=tor_source_unknown stage=rendezvous_source constraint=source_poll cause=unknown action=check_host_service; admission stopped; shared drain required; accepted-work outcomes unknown without authenticated receipts")]
    SourceUnknown,
    #[error("code=tor_engine_unknown stage=engine constraint=service_liveness cause=unknown action=check_host_service; admission stopped; shared drain required; accepted-work outcomes unknown without authenticated receipts")]
    Engine,
    #[error("code=tcp_source_unknown stage=tcp_source constraint=listener_accept cause=unknown action=check_host_listener; admission stopped; shared drain required; accepted-work outcomes unknown without authenticated receipts")]
    TcpSource,
    #[error("code=tor_bridge_connect_unknown stage=bridge_connect constraint=fixed_public_shim cause=unknown action=check_gateway_connectivity_and_reconcile_authenticated_receipts; not replayed; billing unknown; a deliberate new submission may incur another charge")]
    BridgeConnect,
    #[error("code=tor_bridge_io_unknown stage=bridge_delivery constraint=opaque_bidirectional_transport cause=unknown action=check_connection_and_reconcile_authenticated_receipts; not replayed; billing unknown; a deliberate new submission may incur another charge")]
    BridgeIo,
    #[error("code=tor_bridge_initial_deadline stage=bridge_connect constraint=first_byte_connect_initial_write_10s action=check_gateway_connectivity_and_reconcile_authenticated_receipts; not replayed; billing unknown; a deliberate new submission may incur another charge")]
    BridgeDeadline,
}

/// Sole host-owned transient callback. No automatic logs, metrics or persistence.
/// The host must not attach identifiers, retry inference, or launch diagnostic work.
pub type Reporter = Arc<dyn Fn(Failure) + Send + Sync>;

/// The original closed diagnostic survives a host presentation fault.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{original}; code=tor_reporter_unknown stage=reporter constraint=host_callback cause=unknown action=check_host_reporting_offline_and_reconcile_authenticated_receipts; admission stopped; accepted-work billing unknown without authenticated receipts; not replayed; a deliberate new submission may incur another charge")]
pub struct ReportingFailure {
    pub original: Failure,
}

#[derive(Clone)]
pub(crate) struct ReportOwner {
    callback: Reporter,
    failed: watch::Sender<Option<Failure>>,
}

impl ReportOwner {
    pub(crate) fn new(callback: Reporter) -> Self {
        let (failed, _) = watch::channel(None);
        Self { callback, failed }
    }

    fn report(&self, failure: Failure) {
        if self.failed.borrow().is_some() {
            return;
        }
        // Contain only host presentation, never the serving/drain future. The
        // process panic hook runs before this boundary and must be host-suppressed.
        if std::panic::catch_unwind(AssertUnwindSafe(|| (self.callback)(failure))).is_err() {
            self.failed.send_if_modified(|first| {
                if first.is_some() {
                    return false;
                }
                *first = Some(failure);
                true
            });
        }
    }

    fn failure(&self) -> Option<ReportingFailure> {
        self.failed
            .borrow()
            .map(|original| ReportingFailure { original })
    }
}

/// Keep source, shared serving/drain and host presentation evidence separately.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServeFailure {
    pub source: Option<Failure>,
    pub shared: Option<ServerFailure>,
    pub reporting: Option<ReportingFailure>,
}

impl std::fmt::Display for ServeFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(source) = self.source {
            write!(f, "{source}; ")?;
        }
        if let Some(shared) = self.shared {
            write!(f, "{shared}; ")?;
        }
        if let Some(reporting) = self.reporting {
            write!(f, "{reporting}; ")?;
        }
        write!(f, "code=tor_ingress_stopped stage=shared_serving constraint=source_or_drain_or_reporting action=reconcile_accepted_work_from_authenticated_receipts; not replayed; a deliberate new submission may incur another charge")
    }
}
impl std::error::Error for ServeFailure {}

/// Read-only gate. KeyMgr may derive the public entry from the stored pair.
/// Exclusive custody between this check and launch is a host prerequisite, not
/// a distributed lock or protection against concurrent key-manager mutation.
pub fn require_identity(
    manager: &KeyMgr,
    nickname: &HsNickname,
    approved: &HsId,
) -> Result<(), Failure> {
    let pair = manager
        .get::<HsIdKeypair>(&HsIdKeypairSpecifier::new(nickname.clone()))
        .map_err(|_| Failure::IdentityRead)?
        .ok_or(Failure::IdentityMissing)?;
    let public = manager
        .get::<HsIdKey>(&HsIdPublicKeySpecifier::new(nickname.clone()))
        .map_err(|_| Failure::IdentityRead)?
        .ok_or(Failure::IdentityMissing)?;
    if HsIdKey::from(&pair).id() != public.id() || &public.id() != approved {
        return Err(Failure::IdentityMismatch);
    }
    Ok(())
}

pub(crate) trait TokioIo:
    tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send
{
}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> TokioIo for T {}
type BoxIo = Box<dyn TokioIo>;

/// An owned reporting handle, never a caller-selected production destination.
pub(crate) struct BridgeMetadata {
    report: ReportOwner,
    #[cfg(test)]
    fixture: Option<std::sync::Mutex<BridgeFixture>>,
}

impl BridgeMetadata {
    fn new(report: ReportOwner) -> Self {
        Self {
            report,
            #[cfg(test)]
            fixture: None,
        }
    }

    pub(crate) fn report(&self, failure: Failure) {
        self.report.report(failure);
    }

    pub(crate) async fn connect(&self) -> io::Result<BoxIo> {
        #[cfg(test)]
        if let Some(fixture) = &self.fixture {
            let fixture = fixture.lock().unwrap().take();
            return match fixture {
                BridgeFixture::Tcp(address, calls) => {
                    calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    tokio::net::TcpStream::connect(address)
                        .await
                        .map(|io| Box::new(io) as BoxIo)
                }
                BridgeFixture::Duplex(io, calls) => {
                    calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Ok(Box::new(io))
                }
                BridgeFixture::Fail(calls) => {
                    calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Err(io::Error::other("synthetic dial failure"))
                }
                BridgeFixture::Pending(calls) => {
                    calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    std::future::pending().await
                }
                BridgeFixture::Used => unreachable!("fixture dial is one-shot"),
            };
        }
        // The public Tinfoil shim must perform TLS/EHBP before returning HTTP to
        // this process. No private-host shortcut, TLS adapter or arbitrary host.
        tokio::net::TcpStream::connect(("gateway.possums.dev", 443))
            .await
            .map(|io| Box::new(io) as BoxIo)
    }

    #[cfg(test)]
    pub(crate) fn fixture(report: ReportOwner, fixture: BridgeFixture) -> Self {
        Self {
            report,
            fixture: Some(std::sync::Mutex::new(fixture)),
        }
    }
}

/// Offline maintained transports only, not a caller-work/dialer framework.
#[cfg(test)]
pub(crate) enum BridgeFixture {
    Tcp(std::net::SocketAddr, Arc<std::sync::atomic::AtomicUsize>),
    Duplex(tokio::io::DuplexStream, Arc<std::sync::atomic::AtomicUsize>),
    Fail(Arc<std::sync::atomic::AtomicUsize>),
    Pending(Arc<std::sync::atomic::AtomicUsize>),
    Used,
}
#[cfg(test)]
impl BridgeFixture {
    fn take(&mut self) -> Self {
        std::mem::replace(self, Self::Used)
    }
}

// Derive from the same MAX_CONNECTIONS used by the core, without a second quota
// or changing visibility in server.rs. These are pre-HTTP defensive work bounds.
fn work_bound() -> usize {
    crate::server::admission_capacities()[0].1 as usize
}

/// At most the existing connection bound in combined handshake/circuit owners.
/// Each circuit accepts streams serially, but never waits for HTTP handling;
/// idle/bad peers cannot serialize or starve the other circuits.
fn bounded_peers<R, C, IO, Reject>(
    requests: impl Stream<Item = Result<R, Failure>>,
    accept: impl Fn(R) -> C,
    reject: impl Fn(R) -> Reject,
) -> impl Stream<Item = Result<IO, Failure>>
where
    C: Stream<Item = Result<IO, Failure>> + Unpin,
    Reject: Future<Output = ()>,
{
    // Unlike bounded flattening, keep polling the root even when every circuit
    // is idle/full: source EOF must remain observable. Overflow uses Arti's
    // rejection API, not a second HTTP admission pool or a queued handshake.
    stream::unfold(
        (
            Box::pin(requests.fuse()),
            stream::SelectAll::<C>::new(),
            accept,
            reject,
        ),
        |(mut requests, mut peers, accept, reject)| async move {
            let item = loop {
                tokio::select! {
                    request = requests.next() => {
                        match request {
                            Some(Ok(request)) if peers.len() < work_bound() => peers.push(accept(request)),
                            Some(Ok(request)) => reject(request).await,
                            Some(Err(failure)) => break Err(failure),
                            None => break Err(Failure::SourceEnded),
                        }
                        // A continuously ready rejected source must not monopolize
                        // the executor (or delay shutdown) in this unfold loop.
                        tokio::task::yield_now().await;
                    }
                    item = peers.next(), if !peers.is_empty() => {
                        if let Some(item) = item {
                            break item;
                        }
                    }
                }
            };
            Some((item, (requests, peers, accept, reject)))
        },
    )
}

async fn peer_result<T, E>(
    work: impl Future<Output = Result<T, E>>,
    failure: Failure,
    report: &ReportOwner,
) -> Option<T> {
    match AssertUnwindSafe(work).catch_unwind().await {
        Ok(Ok(value)) => Some(value),
        _ => {
            report.report(failure);
            None
        }
    }
}

fn onion_streams(
    requests: impl Stream<Item = RendRequest>,
    report: ReportOwner,
) -> impl Stream<Item = Result<BoxIo, Failure>> {
    let requests = AssertUnwindSafe(requests)
        .catch_unwind()
        .map(|item| item.map_err(|_| Failure::SourceUnknown));
    let rejecting = report.clone();
    bounded_peers(
        requests,
        move |request| {
            let report = report.clone();
            stream::once(async move {
                let Some(streams) =
                    peer_result(request.accept(), Failure::RendezvousAccept, &report).await
                else {
                    return stream::empty().boxed();
                };
                AssertUnwindSafe(streams)
                    .catch_unwind()
                    .then(move |request| {
                        let report = report.clone();
                        async move {
                            // Repeated ready rejections must yield to TCP and
                            // other circuits instead of spinning in filter_map.
                            tokio::task::yield_now().await;
                            match request {
                                Ok(request) => accept_stream(request, report).await,
                                Err(_) => {
                                    report.report(Failure::Circuit);
                                    None
                                }
                            }
                        }
                    })
                    .filter_map(std::future::ready)
                    .map(Ok)
                    .boxed()
            })
            .flatten()
            .boxed()
        },
        move |request| {
            let report = rejecting.clone();
            async move {
                peer_result(request.reject(), Failure::RendezvousReject, &report).await;
            }
        },
    )
}

fn allowed(request: &IncomingStreamRequest) -> bool {
    matches!(request, IncomingStreamRequest::Begin(begin) if begin.port() == 80)
}

async fn accept_stream(request: StreamRequest, report: ReportOwner) -> Option<BoxIo> {
    if !allowed(request.request()) {
        report.report(Failure::PortRefused);
        peer_result(
            request.reject(End::new_with_reason(EndReason::DONE)),
            Failure::StreamReject,
            &report,
        )
        .await;
        return None;
    }
    peer_result(
        request.accept(Connected::new_empty()),
        Failure::StreamAccept,
        &report,
    )
    .await
    .map(|io| Box::new(io.compat()) as BoxIo)
}

#[cfg(test)]
async fn engine_failure(statuses: impl Stream<Item = (State, bool)>) -> Failure {
    engine_status(
        statuses.map(|(state, fatal)| (state, fatal, ProblemKind::None)),
        None,
        None,
    )
    .await
}

async fn engine_status(
    statuses: impl Stream<Item = (State, bool, ProblemKind)>,
    ready: Option<Arc<std::sync::atomic::AtomicBool>>,
    observer: Option<Observer>,
) -> Failure {
    let mut statuses = Box::pin(AssertUnwindSafe(statuses).catch_unwind());
    let mut started = false;
    while let Some(status) = statuses.next().await {
        let Ok((state, fatal, problem)) = status else {
            if let Some(ready) = &ready {
                ready.store(false, std::sync::atomic::Ordering::Release);
            }
            if let Some(observer) = &observer {
                observer.failure(Terminal::StatusPanicked);
            }
            return Failure::Engine;
        };
        if let Some(ready) = &ready {
            ready.store(
                state.is_fully_reachable() && !fatal,
                std::sync::atomic::Ordering::Release,
            );
        }
        let failed = fatal || state == State::Broken || started && state == State::Shutdown;
        if let Some(observer) = &observer {
            let terminal = if failed {
                Some(Terminal::EngineFailure)
            } else if state == State::Running {
                Some(Terminal::FirstLocalReady)
            } else {
                None
            };
            observer.observe(state, problem, terminal);
        }
        if failed {
            return Failure::Engine;
        }
        // Arti starts in Shutdown before background tasks report progress.
        // Initial Shutdown alone cannot prove death; source EOF remains watched.
        started |= state != State::Shutdown;
        tokio::task::yield_now().await;
    }
    if let Some(ready) = &ready {
        ready.store(false, std::sync::atomic::Ordering::Release);
    }
    if let Some(observer) = &observer {
        observer.failure(Terminal::StatusEnded);
    }
    Failure::Engine
}

// No spawn: the host owns both futures through serving/drain. Diagnostic I/O
// can yield without delaying source polling, readiness updates or shutdown;
// returning from serving drops any pending upload immediately.
async fn with_readiness_diagnostics<T>(
    serving: impl Future<Output = T>,
    diagnostics: impl Future<Output = ()>,
) -> T {
    tokio::pin!(serving, diagnostics);
    tokio::select! {
        biased;
        _ = &mut diagnostics => serving.await,
        result = &mut serving => result,
    }
}

/// One fair combined incoming stream, one invocation of the existing core.
/// Keep the underlying stream (including circuit handles) through shared drain:
/// the core drops only its borrow at quiesce, never our circuit owners.
#[cfg(test)]
async fn serve_sources_until(
    tcp: impl Stream<Item = Result<BoxIo, Failure>>,
    onion: impl Stream<Item = Result<BoxIo, Failure>>,
    state: AppState,
    header_deadline: Duration,
    shutdown: impl Future<Output = ()>,
    report: ReportOwner,
) -> Result<(), ServeFailure> {
    // Historical HTTP-core fixtures only. Production onion streams are opaque.
    fn http(io: Result<BoxIo, Failure>) -> Result<Incoming<BoxIo>, Failure> {
        io.map(Incoming::Http)
    }
    serve_connection_sources_until(
        tcp.map(http),
        onion.map(http),
        state,
        header_deadline,
        shutdown,
        report,
    )
    .await
}

pub(crate) async fn serve_connection_sources_until<IO>(
    tcp: impl Stream<Item = Result<Incoming<IO>, Failure>>,
    onion: impl Stream<Item = Result<Incoming<IO>, Failure>>,
    state: AppState,
    header_deadline: Duration,
    shutdown: impl Future<Output = ()>,
    report: ReportOwner,
) -> Result<(), ServeFailure>
where
    IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let mut reporting = report.failed.subscribe();
    let shutdown = async {
        tokio::pin!(shutdown);
        tokio::select! {
            biased;
            _ = shutdown => {},
            _ = reporting.wait_for(|failure| failure.is_some()) => {},
        }
    };
    let mut primary = None;
    let mut incoming = Box::pin(
        stream::select(tcp, onion)
            .take_while(|item| {
                if let Err(failure) = item {
                    primary = Some(*failure);
                    report.report(*failure);
                    std::future::ready(false)
                } else {
                    std::future::ready(report.failure().is_none())
                }
            })
            .map(|item| item.map_err(io::Error::other)),
    );
    let shared = serve_connections_until(&mut incoming, state, header_deadline, shutdown)
        .await
        .err()
        .map(|error| {
            error
                .get_ref()
                .and_then(|error| error.downcast_ref::<ServerFailure>())
                .copied()
                .unwrap_or(ServerFailure::ShutdownIntegrity)
        });
    drop(incoming);
    let reporting = report.failure();
    if primary.is_some() || shared.is_some() || reporting.is_some() {
        Err(ServeFailure {
            source: primary,
            shared,
            reporting,
        })
    } else {
        Ok(())
    }
}

async fn launch_checked<T, F: Future<Output = ()>>(
    identity: impl FnOnce() -> Result<(), Failure>,
    launch: impl FnOnce() -> Result<T, Failure>,
    mut shutdown: std::pin::Pin<&mut impl Future<Output = ()>>,
    report: &ReportOwner,
    initialized: impl FnOnce() -> F,
) -> Result<Option<T>, ServeFailure> {
    if shutdown.as_mut().now_or_never().is_some() {
        return Ok(None);
    }
    let failure = |failure| {
        report.report(failure);
        ServeFailure {
            source: Some(failure),
            shared: None,
            reporting: report.failure(),
        }
    };
    identity().map_err(failure)?;
    // Identity reads are synchronous: observe shutdown again immediately before
    // the launch side effect. A later signal is not a process-exit guarantee.
    let launched = tokio::select! {
        biased;
        _ = shutdown => Ok(None),
        result = async { launch() } => result.map(Some).map_err(failure),
    }?;
    if launched.is_some() {
        initialized().await;
    }
    Ok(launched)
}

/// Runtime opt-in only. Invoking this explicitly launches a service and may
/// trigger Arti bootstrap. Tests never invoke it or construct a TorClient.
/// Host prerequisites: approved exclusively owned keys/state and upstream log
/// suppression. Arti can terminate the whole process on unsupported protocols.
pub async fn serve_tcp_and_onion_until(
    listener: TcpListener,
    state: AppState,
    client: Arc<TorClient<PreferredRuntime>>,
    config: OnionServiceConfig,
    approved: HsId,
    shutdown: impl Future<Output = ()>,
    report: Reporter,
) -> Result<(), ServeFailure> {
    serve_tcp_and_onion_until_initialized(
        listener,
        state,
        client,
        config,
        approved,
        shutdown,
        (report, || std::future::ready(())),
    )
    .await
}

#[cfg(feature = "debug-tor-readiness")]
pub use readiness::Observer as DebugReadiness;

/// Begin the debug grace clock before Tor preparation/bootstrap, not at launch.
#[cfg(feature = "debug-tor-readiness")]
pub fn debug_readiness() -> (DebugReadiness, impl Future<Output = ()>) {
    readiness::new(crate::telemetry::runtime::Config::from_env())
}

/// Runs the startup-only handoff after local service launch, before accepting
/// connections. This is not evidence of onion publication or remote serving.
/// The handoff is separate from the reporter used by later peer callbacks.
pub async fn serve_tcp_and_onion_until_initialized<F: Future<Output = ()>>(
    listener: TcpListener,
    state: AppState,
    client: Arc<TorClient<PreferredRuntime>>,
    config: OnionServiceConfig,
    approved: HsId,
    shutdown: impl Future<Output = ()>,
    hooks: (Reporter, impl FnOnce() -> F),
) -> Result<(), ServeFailure> {
    let (report, initialized) = hooks;
    tokio::pin!(shutdown);
    let readiness = state.tor_ready.clone();
    let report = ReportOwner::new(report);
    let nickname = config.nickname().clone();
    let Some((service, requests)) = launch_checked(
        || {
            client
                .keymgr()
                .map_err(|_| Failure::IdentityRead)
                .and_then(|manager| require_identity(manager, &nickname, &approved))
        },
        || {
            client
                .launch_onion_service(config)
                .map_err(|_| Failure::Launch)?
                .ok_or(Failure::Disabled)
        },
        shutdown.as_mut(),
        &report,
        initialized,
    )
    .await?
    else {
        return Ok(());
    };
    let tcp = stream::unfold(listener, |listener| async {
        let accepted = listener
            .accept()
            .await
            .map(|(io, _)| Box::new(io) as BoxIo)
            .map_err(|_| Failure::TcpSource);
        Some((accepted, listener))
    });
    #[cfg(not(feature = "debug-tor-readiness"))]
    let (observer, diagnostics) = readiness::new(crate::telemetry::runtime::Config::from_env());
    #[cfg(feature = "debug-tor-readiness")]
    let (observer, diagnostics): (_, std::pin::Pin<Box<dyn Future<Output = ()> + Send>>) =
        if let Some(observer) = state.debug_tor_readiness.clone() {
            // Main owns and polls diagnostics across preparation, bootstrap,
            // serving and drain. No second observer or detached task.
            (observer, Box::pin(std::future::pending()))
        } else {
            let (observer, worker) = debug_readiness();
            (observer, Box::pin(worker))
        };
    #[cfg(feature = "debug-tor-readiness")]
    let diagnostics = {
        let bootstrap_observer = observer.clone();
        let bootstrap_client = client.clone();
        async move {
            let bootstrap = async move {
                let statuses = bootstrap_client.bootstrap_events();
                tokio::pin!(statuses);
                while let Some(status) = statuses.next().await {
                    bootstrap_observer.observe_bootstrap(&status);
                }
            };
            tokio::join!(diagnostics, bootstrap);
        }
    };
    let engine = stream::once(async {
        #[cfg(feature = "debug-tor-readiness")]
        let status_observer = observer.clone();
        let statuses = service.status_events().map(move |status| {
            #[cfg(feature = "debug-tor-readiness")]
            status_observer.observe_sdk_status(&status);
            (
                status.state(),
                matches!(status.current_problem(), Some(Problem::Runtime(_))),
                ProblemKind::from_sdk(status.current_problem()),
            )
        });
        Err(engine_status(statuses, readiness, Some(observer)).await)
    });
    let onion = stream::select(onion_streams(requests, report.clone()), engine);
    let bridge_report = report.clone();
    let onion = onion.map(move |io| {
        io.map(|io| Incoming::OpaqueTorBridge(io, BridgeMetadata::new(bridge_report.clone())))
    });
    let tcp = tcp.map(|io| io.map(Incoming::Http));
    // The public shim's returning HTTP connection shares this same admission
    // pool; opaque onion traffic can never be parsed directly by Hyper here.
    let result = with_readiness_diagnostics(
        serve_connection_sources_until(
            tcp,
            onion,
            state,
            Duration::from_secs(10),
            shutdown,
            report,
        ),
        diagnostics,
    )
    .await;
    drop(service);
    drop(client);
    result
}

#[cfg(test)]
mod tests;
