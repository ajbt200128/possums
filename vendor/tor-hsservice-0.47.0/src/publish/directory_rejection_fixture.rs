// Offline fixture only. Included inside reactor::test; never a runtime module.
use crate::ipt_set::{IptInSet, IptSet, ipts_channel};
use crate::pow::NewPowManager;
use crate::publish::test::{build_test_config, init_keymgr};
use crate::status::{IptMgrStatusSender, OnionServiceStatus, StatusSender};
use async_trait::async_trait;
use futures::io::{AsyncRead, AsyncWrite, Cursor};
use std::collections::HashMap;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use tor_basic_utils::test_rng::{TestingRng, testing_rng};
use tor_llcrypto::pk::rsa;
use tor_netdir::testprovider::TestNetDirProvider;
use tor_rtcompat::ToplevelBlockOn;
use tor_rtmock::MockRuntime;

const REJECTION_RESPONSE: &str =
    "HTTP/1.1 400 synthetic-untrusted-reason\r\n\r\nsynthetic-untrusted-body";
const DIRS_PER_PERIOD: usize = 2;
// Immediate mocked responses plus decorrelated backoff can overshoot the 300s
// overall timer: the final sleep is at most three times a preceding delay.
const RETRY_DRAIN_BUDGET: Duration = Duration::from_secs(1500);

#[derive(Clone, Copy)]
enum RejectionScript {
    Persistent400,
    Once400,
    Once500,
    OnceIo,
    Accept,
}

#[derive(Clone)]
struct RejectionMock {
    selected: rsa::RsaIdentity,
    script: Arc<Mutex<RejectionScript>>,
    attempts: Arc<Mutex<HashMap<rsa::RsaIdentity, usize>>>,
    // Synthetic request bytes stay in test memory; assertions never print them.
    selected_requests: Arc<Mutex<Vec<Vec<u8>>>>,
}

struct RejectionTunnel {
    response: &'static str,
    io_failure: bool,
    selected_request: Option<(Arc<Mutex<Vec<Vec<u8>>>>, usize)>,
}
struct RejectionStream {
    response: Cursor<Vec<u8>>,
    io_failure: bool,
    selected_request: Option<(Arc<Mutex<Vec<Vec<u8>>>>, usize)>,
}

impl AsyncRead for RejectionStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        out: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        if self.io_failure {
            return Poll::Ready(Err(io::Error::other("synthetic transport failure")));
        }
        Pin::new(&mut self.response).poll_read(cx, out)
    }
}
impl AsyncWrite for RejectionStream {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        if let Some((requests, index)) = &self.selected_request {
            requests.lock().unwrap()[*index].extend_from_slice(bytes);
        }
        Poll::Ready(Ok(bytes.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}
#[async_trait]
impl MockableDirTunnel for RejectionTunnel {
    type DataStream = RejectionStream;
    async fn begin_dir_stream(&self) -> Result<Self::DataStream, tor_circmgr::Error> {
        Ok(RejectionStream {
            response: Cursor::new(self.response.as_bytes().to_vec()),
            io_failure: self.io_failure,
            selected_request: self.selected_request.clone(),
        })
    }
    fn source_info(&self) -> tor_proto::Result<Option<SourceInfo>> {
        Ok(None)
    }
}
#[async_trait]
impl Mockable for RejectionMock {
    type Rng = TestingRng;
    type Tunnel = RejectionTunnel;
    fn thread_rng(&self) -> TestingRng {
        testing_rng()
    }
    fn estimate_upload_timeout(&self) -> Duration {
        Duration::from_secs(30)
    }
    async fn get_or_launch_hs_dir<T>(
        &self,
        _: &NetDir,
        target: T,
    ) -> Result<Self::Tunnel, tor_circmgr::Error>
    where
        T: tor_linkspec::CircTarget + Send + Sync,
    {
        let id = *target.rsa_identity().unwrap();
        let mut attempts = self.attempts.lock().unwrap();
        let count = attempts.entry(id).or_default();
        *count += 1;
        let first = *count == 1;
        let mut response = "HTTP/1.1 200 OK\r\n\r\n";
        let mut io_failure = false;
        if id == self.selected {
            match *self.script.lock().unwrap() {
                RejectionScript::Persistent400 => response = REJECTION_RESPONSE,
                RejectionScript::Once400 if first => response = REJECTION_RESPONSE,
                RejectionScript::Once500 if first => response = "HTTP/1.1 500 synthetic\r\n\r\n",
                RejectionScript::OnceIo if first => io_failure = true,
                _ => (),
            }
        }
        let selected_request = if id == self.selected {
            let mut requests = self.selected_requests.lock().unwrap();
            let index = requests.len();
            requests.push(Vec::new());
            Some((self.selected_requests.clone(), index))
        } else {
            None
        };
        Ok(RejectionTunnel {
            response,
            io_failure,
            selected_request,
        })
    }
}

// This drives the real per-period worker, descriptor construction, HTTP decoder,
// retry runner, completion handler and publisher status recomputation. Only the
// circuit/stream and network directory are synthetic. Two actual ring members per
// period are selected to keep this fixture small; no production quorum is changed.
fn rejection_fixture(script: RejectionScript, project_job_fatal: bool) {
    let runtime = MockRuntime::new();
    runtime.clone().block_on(async move {
        let root = tempfile::tempdir().unwrap();
        let keys = tempfile::tempdir().unwrap();
        let netdir = Arc::new(construct_netdir());
        runtime.jump_wallclock(netdir.lifetime().valid_after());
        let nickname = HsNickname::try_from("offline-rejection".to_string()).unwrap();
        let config = build_test_config(nickname.clone());
        let (_config_tx, config_rx) = watch::channel_with(Arc::new(config.clone()));
        let (_, _, keymgr) = init_keymgr(&keys, &nickname, &netdir);
        let (mut ipts_mgr, ipts_publisher) = ipts_channel(&runtime, crate::test::create_storage_handles(root.path()).1).unwrap();
        ipts_mgr.borrow_for_update(runtime.clone()).ipts = Some(IptSet {
            ipts: tor_netdoc::doc::hsdesc::test_data::test_parsed_hsdesc().unwrap().intro_points().iter().enumerate().map(|(i, ipt)| IptInSet {
                ipt: ipt.clone(), lid: [i.try_into().unwrap(); 32].into(),
            }).collect(),
            lifetime: Duration::from_secs(3600),
        });
        let periods = netdir.hs_all_time_periods();
        let expected_period_count = periods.len();
        assert!(expected_period_count >= 2);
        let expected_destination_count = expected_period_count * DIRS_PER_PERIOD;
        // NetDir yields current first, then every available previous/next role.
        // Both secondary roles are valid in this synthetic directory; retain all.
        assert_eq!(periods[0].time_period(), netdir.hs_time_period());
        assert_eq!(periods.iter().filter(|p| p.time_period() == netdir.hs_time_period()).count(), 1);
        let current = periods.iter().find(|p| p.time_period() == netdir.hs_time_period()).unwrap();
        let blind = read_blind_id_keypair(&keymgr, &nickname, current.time_period()).unwrap().unwrap();
        let selected = *netdir.hs_dirs_upload(HsBlindIdKey::from(&blind).into(), current.time_period()).unwrap().next().unwrap().rsa_identity().unwrap();
        let mock = RejectionMock {
            selected, script: Arc::new(Mutex::new(script)), attempts: Default::default(),
            selected_requests: Default::default(),
        };
        let status_tx = StatusSender::new(OnionServiceStatus::new_shutdown());
        let ipt_status: IptMgrStatusSender = status_tx.clone().into();
        ipt_status.send(State::Running, None);
        let mut status_rx = status_tx.subscribe();
        let provider = TestNetDirProvider::new();
        provider.set_netdir(netdir.clone());
        let dir_provider: Arc<dyn NetDirProvider> = Arc::new(provider);
        let mistrust = fs_mistrust::Mistrust::new_dangerously_trust_everyone();
        let state_dir = StateDirectory::new(root.path().join("pow"), &mistrust).unwrap();
        let state_handle = state_dir.acquire_instance(&nickname).unwrap();
        let NewPowManager { pow_manager, publisher_update_rx, .. } = PowManager::new(
            runtime.clone(), nickname.clone(), state_handle.raw_subdir("pow_nonces").unwrap(), keymgr.clone(),
            state_handle.storage_handle("pow_manager").unwrap(), dir_provider.clone(), status_tx.clone().into(), config_rx.clone(),
        ).unwrap();
        let mut reactor = Reactor::new(runtime.clone(), nickname, dir_provider, mock.clone(), &config, ipts_publisher,
            config_rx, status_tx.into(), keymgr, Arc::new(CfgPathResolver::default()), pow_manager, publisher_update_rx);
        reactor.replace_netdir(netdir.clone());
        reactor.recompute_hs_dirs().unwrap();
        {
            let mut inner = reactor.inner.lock().unwrap();
            assert_eq!(inner.time_periods.len(), expected_period_count);
            assert!(inner.time_periods.iter().zip(&periods).all(|(ctx, params)| ctx.params == *params));
            let mut used = HashSet::new();
            for ctx in &mut inner.time_periods {
                if ctx.params.time_period() == netdir.hs_time_period() {
                    ctx.hs_dirs.sort_by_key(|(ids, _)| ids.rsa_identity() != Some(&selected));
                    ctx.hs_dirs.truncate(DIRS_PER_PERIOD);
                    assert!(ctx.hs_dirs.iter().any(|(ids, _)| ids.rsa_identity() == Some(&selected)));
                } else {
                    ctx.hs_dirs.retain(|(ids, _)| !used.contains(ids.rsa_identity().unwrap()));
                    ctx.hs_dirs.truncate(DIRS_PER_PERIOD);
                }
                assert_eq!(ctx.hs_dirs.len(), DIRS_PER_PERIOD);
                used.extend(ctx.hs_dirs.iter().map(|(ids, _)| *ids.rsa_identity().unwrap()));
            }
        }
        reactor.upload_all().await.unwrap();
        // MockRuntime advances timers in order; there is no wall-clock sleep or network.
        runtime.advance_by(RETRY_DRAIN_BUDGET).await;
        let persistent = matches!(script, RejectionScript::Persistent400);
        let mut completions = 0;
        for _ in 0..expected_period_count {
            runtime.progress_until_stalled().await;
            let mut result = reactor.upload_task_complete_rx.next().now_or_never().flatten()
                .expect("missing upload result after bounded virtual-time drain");
            if project_job_fatal {
                for status in &mut result.hsdir_result {
                    if let Err(DescUploadRetryError::Timeout(errors)) = &status.upload_res {
                        // Projection only: prove the misleadingly named retry FatalError is
                        // still job-local at the actual batch/status boundary. No policy change.
                        status.upload_res = Err(DescUploadRetryError::FatalError(errors.clone()));
                    }
                }
            }
            reactor.handle_upload_results(result);
            reactor.upload_result_to_svc_status().unwrap();
            completions += 1;
        }
        assert_eq!(completions, expected_period_count);
        assert!(reactor.upload_task_complete_rx.next().now_or_never().is_none());
        runtime.progress_until_stalled().await;
        let snapshot = status_rx.next().now_or_never().flatten().unwrap();
        assert_eq!(snapshot.publisher_status().state(), if persistent { State::DegradedReachable } else { State::Running });
        assert_eq!(snapshot.state(), if persistent { State::DegradedReachable } else { State::Running });
        assert_eq!(snapshot.current_problem().is_some(), persistent);
        if persistent { assert!(matches!(snapshot.current_problem(), Some(Problem::DescriptorUpload(_)))); }
        let attempts = mock.attempts.lock().unwrap();
        assert_eq!(attempts.len(), expected_destination_count);
        assert_eq!(attempts.values().sum::<usize>(), expected_destination_count - 1 + attempts[&selected]);
        assert!(*attempts.get(&selected).unwrap() >= 2);
        for (id, count) in attempts.iter() { if id != &selected { assert_eq!(*count, 1); } }
        if !persistent { assert_eq!(*attempts.get(&selected).unwrap(), 2); }
        let selected_attempts = attempts[&selected];
        drop(attempts);
        {
            let requests = mock.selected_requests.lock().unwrap();
            assert_eq!(requests.len(), selected_attempts);
            assert!(requests.iter().all(|r| r.starts_with(b"POST /tor/hs/3/publish HTTP/1.0\r\n")));
            assert!(requests.windows(2).all(|pair| pair[0] == pair[1]), "a job must retry the same immutable request");
        }
        {
            let inner = reactor.inner.lock().unwrap();
            assert_eq!(inner.reupload_timers.len(), expected_period_count);
            let mut failures = 0;
            for ctx in &inner.time_periods {
                assert_eq!(ctx.upload_results.len(), DIRS_PER_PERIOD);
                assert!(ctx.upload_results.iter().any(|r| r.upload_res.is_ok()));
                for result in &ctx.upload_results {
                    if let Err(error) = &result.upload_res {
                        failures += 1;
                        let sources = match error {
                            DescUploadRetryError::Timeout(errors) if !project_job_fatal => errors,
                            DescUploadRetryError::FatalError(errors) if project_job_fatal => errors,
                            _ => panic!("unexpected wrapper category"),
                        };
                        assert!(sources.sources().count() >= 2);
                        assert!(sources.sources().all(|e| matches!(e, UploadError::Request(RequestFailedError { source: None, error: tor_dirclient::RequestError::HttpStatus(400, phrase) }) if phrase == "synthetic-untrusted-reason")));
                    }
                }
                for (id, status) in &ctx.hs_dirs {
                    assert_eq!(*status, if persistent && id.rsa_identity() == Some(&selected) { DescriptorStatus::Dirty } else { DescriptorStatus::Clean });
                }
            }
            assert_eq!(failures, usize::from(persistent));

            // Status projection only, AFTER all actual period jobs have finished:
            // secondary successes are pooled, not required separately per role.
            let mut views = inner.time_periods.iter().map(|ctx| create_time_period_ctx(
                &ctx.params,
                if ctx.params.time_period() == netdir.hs_time_period() {
                    ctx.upload_results.iter().filter(|r| r.upload_res.is_ok()).cloned().collect()
                } else { Vec::new() },
            )).collect::<Vec<_>>();
            assert_eq!(upload_result_state(&netdir, &views).0, State::Bootstrapping);
            let secondary = inner.time_periods.iter().position(|ctx| ctx.params.time_period() != netdir.hs_time_period()).unwrap();
            views[secondary].upload_results = inner.time_periods[secondary].upload_results.clone();
            assert_eq!(upload_result_state(&netdir, &views).0, State::Running);
        }
        let selective_dirty_probe = matches!(script, RejectionScript::Once500);
        if selective_dirty_probe {
            // Desired behavior for the exact r2 clean-prefix probe: only the
            // later Dirty period is uploaded; earlier Clean destinations get none.
            runtime.advance_by(UPLOAD_RATE_LIM_THRESHOLD + Duration::from_secs(1)).await;
            let before = mock.attempts.lock().unwrap().clone();
            let period = periods.last().unwrap().time_period();
            assert!(reactor.mark_dirty(&period));
            let dirty_ids = {
                let inner = reactor.inner.lock().unwrap();
                assert!(inner.time_periods[0].hs_dirs.iter().all(|(_, s)| *s == DescriptorStatus::Clean));
                assert!(inner.time_periods.last().unwrap().hs_dirs.iter().all(|(_, s)| *s == DescriptorStatus::Dirty));
                inner.time_periods.last().unwrap().hs_dirs.iter().map(|(id, _)| *id.rsa_identity().unwrap()).collect::<HashSet<_>>()
            };
            reactor.upload_all().await.unwrap();
            runtime.progress_until_stalled().await;
            let result = reactor.upload_task_complete_rx.next().now_or_never().flatten()
                .expect("later Dirty period must complete despite earlier Clean periods");
            assert_eq!(result.time_period, period);
            assert_eq!(result.hsdir_result.len(), DIRS_PER_PERIOD);
            assert!(result.hsdir_result.iter().all(|r| r.upload_res.is_ok()));
            reactor.handle_upload_results(result);
            reactor.upload_result_to_svc_status().unwrap();
            runtime.progress_until_stalled().await;
            assert!(reactor.upload_task_complete_rx.next().now_or_never().is_none());
            let attempts = mock.attempts.lock().unwrap();
            assert_eq!(attempts.len(), before.len());
            for (id, count) in &before {
                assert_eq!(attempts[id], count + usize::from(dirty_ids.contains(id)));
            }
            assert!(reactor.inner.lock().unwrap().time_periods.iter().all(|ctx| ctx.hs_dirs.iter().all(|(_, s)| *s == DescriptorStatus::Clean)));
        }
        if persistent || selective_dirty_probe {
            // Invoke the real IPT notification handler, then its scheduled upload
            // operation. The indefinite reactor/timer event loop is not launched.
            // Respect the unchanged rate limit after the selective-dirty probe.
            runtime.advance_by(UPLOAD_RATE_LIM_THRESHOLD + Duration::from_secs(1)).await;
            *mock.script.lock().unwrap() = RejectionScript::Accept;
            let before = *mock.attempts.lock().unwrap().get(&selected).unwrap();
            let before_total = mock.attempts.lock().unwrap().values().sum::<usize>();
            let before_revision = reactor.inner.lock().unwrap().time_periods.iter()
                .flat_map(|ctx| &ctx.upload_results)
                .find(|result| result.relay_ids.rsa_identity() == Some(&selected)).unwrap().revision_counter;
            reactor.handle_ipt_change(Some(Ok(()))).await.unwrap();
            reactor.upload_all().await.unwrap();
            runtime.advance_by(Duration::from_secs(1)).await;
            let mut recovered = 0;
            for _ in 0..expected_period_count {
                runtime.progress_until_stalled().await;
                let result = reactor.upload_task_complete_rx.next().now_or_never().flatten()
                    .expect("missing recovery result after IPT publication event");
                assert_eq!(result.hsdir_result.len(), DIRS_PER_PERIOD);
                assert!(result.hsdir_result.iter().all(|r| r.upload_res.is_ok()));
                for status in &result.hsdir_result {
                    if status.relay_ids.rsa_identity() == Some(&selected) {
                        assert!(status.revision_counter > before_revision);
                    }
                }
                reactor.handle_upload_results(result);
                reactor.upload_result_to_svc_status().unwrap();
                recovered += 1;
            }
            assert_eq!(recovered, expected_period_count);
            assert!(reactor.upload_task_complete_rx.next().now_or_never().is_none());
            let snapshot = status_rx.next().now_or_never().flatten().unwrap();
            assert_eq!(snapshot.state(), State::Running);
            assert!(snapshot.current_problem().is_none());
            assert_eq!(*mock.attempts.lock().unwrap().get(&selected).unwrap(), before + 1);
            assert_eq!(mock.attempts.lock().unwrap().values().sum::<usize>(), before_total + expected_destination_count);
            assert!(reactor.inner.lock().unwrap().time_periods.iter().all(|ctx| ctx.hs_dirs.iter().all(|(_, s)| *s == DescriptorStatus::Clean)));
            let requests = mock.selected_requests.lock().unwrap();
            assert!(requests.last().unwrap() != &requests[0], "later publication must build a fresh request");
        }
    });
}

#[test]
fn directory_rejection_persistent_400() {
    rejection_fixture(RejectionScript::Persistent400, false);
}
#[test]
fn directory_rejection_400_then_success() {
    rejection_fixture(RejectionScript::Once400, false);
}
#[test]
fn directory_rejection_500_then_success() {
    rejection_fixture(RejectionScript::Once500, false);
}
#[test]
fn directory_rejection_transport_then_success() {
    rejection_fixture(RejectionScript::OnceIo, false);
}
#[test]
fn directory_rejection_job_fatal_projection() {
    rejection_fixture(RejectionScript::Persistent400, true);
}
