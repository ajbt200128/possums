// Offline regressions in the existing SDK publisher module. No real network,
// introduction manager, tracing subscriber or new production observation hook.

#[derive(Clone, Copy)]
enum SelectiveTrigger {
    ReuploadTimer,
    Consensus,
    AllCleanConsensus,
}

// Step the actual reactor event selector and its workers without advancing time.
// A pending turn is dropped only after all ready work has been drained. Tests
// explicitly retain a pending turn when exercising a timer or broadcast wakeup.
async fn drain_ready_publisher(
    reactor: &mut Reactor<MockRuntime, RejectionMock>,
    runtime: &MockRuntime,
) {
    for _ in 0..128 {
        runtime.progress_until_stalled().await;
        match reactor.run_once().now_or_never() {
            Some(Ok(ShutdownStatus::Continue)) => (),
            Some(_) => panic!("publisher terminated during offline event regression"),
            None => return,
        }
    }
    panic!("publisher failed to drain within the bounded event-turn budget");
}

fn selective_period_fixture(trigger: SelectiveTrigger) {
    let runtime = MockRuntime::new();
    runtime.clone().block_on(async move {
        let root = tempfile::tempdir().unwrap();
        let keys = tempfile::tempdir().unwrap();
        let netdir = Arc::new(construct_netdir());
        runtime.jump_wallclock(netdir.lifetime().valid_after());
        let nickname = HsNickname::try_from("offline-selective-period".to_string()).unwrap();
        let config = build_test_config(nickname.clone());
        let (_config_tx, config_rx) = watch::channel_with(Arc::new(config.clone()));
        let (_, blind, keymgr) = init_keymgr(&keys, &nickname, &netdir);
        let (mut ipts_mgr, ipts_publisher) =
            ipts_channel(&runtime, crate::test::create_storage_handles(root.path()).1).unwrap();
        let selected = *netdir
            .hs_dirs_upload(blind, netdir.hs_time_period())
            .unwrap()
            .next()
            .unwrap()
            .rsa_identity()
            .unwrap();
        let mock = RejectionMock {
            selected,
            script: Arc::new(Mutex::new(RejectionScript::Accept)),
            attempts: Default::default(),
            selected_requests: Default::default(),
        };
        let status_tx = StatusSender::new(OnionServiceStatus::new_shutdown());
        let ipt_status: IptMgrStatusSender = status_tx.clone().into();
        ipt_status.send(State::Running, None);
        let provider = Arc::new(TestNetDirProvider::new());
        provider.set_netdir(netdir.clone());
        let dir_provider: Arc<dyn NetDirProvider> = provider.clone();
        let mistrust = fs_mistrust::Mistrust::new_dangerously_trust_everyone();
        let state_dir = StateDirectory::new(root.path().join("pow"), &mistrust).unwrap();
        let state_handle = state_dir.acquire_instance(&nickname).unwrap();
        let NewPowManager {
            pow_manager,
            publisher_update_rx,
            ..
        } = PowManager::new(
            runtime.clone(),
            nickname.clone(),
            state_handle.raw_subdir("pow_nonces").unwrap(),
            keymgr.clone(),
            state_handle.storage_handle("pow_manager").unwrap(),
            dir_provider.clone(),
            status_tx.clone().into(),
            config_rx.clone(),
        )
        .unwrap();
        let mut reactor = Reactor::new(
            runtime.clone(),
            nickname,
            dir_provider,
            mock.clone(),
            &config,
            ipts_publisher,
            config_rx,
            status_tx.into(),
            keymgr,
            Arc::new(CfgPathResolver::default()),
            pow_manager,
            publisher_update_rx,
        );
        reactor.replace_netdir(netdir.clone());
        reactor.recompute_hs_dirs().unwrap();
        // Keep every returned context AND its complete ring. Consensus recompute
        // must not silently turn omitted ring members into additional Dirty work.
        let periods = netdir.hs_all_time_periods();
        assert!(periods.len() >= 2);
        assert_eq!(periods[0].time_period(), netdir.hs_time_period());
        let later_period = periods.last().unwrap().time_period();
        assert_ne!(later_period, netdir.hs_time_period());
        {
            let inner = reactor.inner.lock().unwrap();
            assert_eq!(inner.time_periods.len(), periods.len());
            assert!(
                inner
                    .time_periods
                    .iter()
                    .zip(&periods)
                    .all(|(ctx, p)| ctx.params == *p)
            );
            assert!(inner.time_periods.iter().all(|ctx| !ctx.hs_dirs.is_empty()));
        }
        // Existing no-IPTs semantics: the real selector does not publish yet.
        drain_ready_publisher(&mut reactor, &runtime).await;
        assert_eq!(reactor.status(), PublishStatus::AwaitingIpts);
        assert!(mock.attempts.lock().unwrap().is_empty());
        ipts_mgr.borrow_for_update(runtime.clone()).ipts = Some(IptSet {
            ipts: tor_netdoc::doc::hsdesc::test_data::test_parsed_hsdesc()
                .unwrap()
                .intro_points()
                .iter()
                .enumerate()
                .map(|(i, ipt)| IptInSet {
                    ipt: ipt.clone(),
                    lid: [i.try_into().unwrap(); 32].into(),
                })
                .collect(),
            lifetime: Duration::from_secs(3600),
        });
        // Initial IPT event -> scheduling -> upload -> completions is real too.
        drain_ready_publisher(&mut reactor, &runtime).await;
        assert_eq!(reactor.status(), PublishStatus::Idle);
        let (before_results, before_timers, expected_extra) = {
            let inner = reactor.inner.lock().unwrap();
            assert!(inner.time_periods.iter().all(|ctx| {
                ctx.hs_dirs
                    .iter()
                    .all(|(_, s)| *s == DescriptorStatus::Clean)
            }));
            assert_eq!(inner.reupload_timers.len(), periods.len());
            let results = inner
                .time_periods
                .iter()
                .map(|ctx| {
                    (
                        ctx.last_successful,
                        ctx.upload_results
                            .iter()
                            .map(|r| (r.relay_ids.clone(), r.revision_counter))
                            .collect::<Vec<_>>(),
                    )
                })
                .collect::<Vec<_>>();
            let mut extra = HashMap::<rsa::RsaIdentity, usize>::new();
            if !matches!(trigger, SelectiveTrigger::AllCleanConsensus) {
                for (id, _) in &inner.time_periods.last().unwrap().hs_dirs {
                    *extra.entry(*id.rsa_identity().unwrap()).or_default() += 1;
                }
            }
            (results, inner.reupload_timers.len(), extra)
        };
        let before_attempts = mock.attempts.lock().unwrap().clone();
        match trigger {
            SelectiveTrigger::ReuploadTimer => {
                // Move only the later period's existing timer forward. The real
                // wakeup, timer owner, scheduling and rate limit remain in charge.
                {
                    let mut inner = reactor.inner.lock().unwrap();
                    inner.reupload_timers = std::mem::take(&mut inner.reupload_timers)
                        .into_iter()
                        .map(|mut timer| {
                            if timer.period == later_period {
                                timer.when = runtime.now() + Duration::from_secs(1);
                            }
                            timer
                        })
                        .collect();
                }
                let mut turn = Box::pin(reactor.run_once());
                assert!(turn.as_mut().now_or_never().is_none());
                runtime.advance_by(Duration::from_secs(2)).await;
                assert!(matches!(
                    turn.as_mut().now_or_never(),
                    Some(Ok(ShutdownStatus::Continue))
                ));
                drop(turn);
                drain_ready_publisher(&mut reactor, &runtime).await;
                assert!(matches!(reactor.status(), PublishStatus::RateLimited(_)));
                assert!(*mock.attempts.lock().unwrap() == before_attempts);
                {
                    let inner = reactor.inner.lock().unwrap();
                    for ctx in &inner.time_periods {
                        let expected = if ctx.params.time_period() == later_period {
                            DescriptorStatus::Dirty
                        } else {
                            DescriptorStatus::Clean
                        };
                        assert!(ctx.hs_dirs.iter().all(|(_, s)| *s == expected));
                    }
                }
                let mut rate_turn = Box::pin(reactor.run_once());
                assert!(rate_turn.as_mut().now_or_never().is_none());
                runtime
                    .advance_by(UPLOAD_RATE_LIM_THRESHOLD + Duration::from_secs(1))
                    .await;
                assert!(matches!(
                    rate_turn.as_mut().now_or_never(),
                    Some(Ok(ShutdownStatus::Continue))
                ));
                drop(rate_turn);
            }
            SelectiveTrigger::Consensus | SelectiveTrigger::AllCleanConsensus => {
                runtime
                    .advance_by(UPLOAD_RATE_LIM_THRESHOLD + Duration::from_secs(1))
                    .await;
                if matches!(trigger, SelectiveTrigger::Consensus) {
                    assert!(reactor.mark_dirty(&later_period));
                }
                // Preserve the subscribed, pending selector while the supported
                // provider sends NewConsensus; do not call its handler directly.
                let mut turn = Box::pin(reactor.run_once());
                assert!(turn.as_mut().now_or_never().is_none());
                provider.set_netdir_and_notify(netdir.clone()).await;
                assert!(matches!(
                    turn.as_mut().now_or_never(),
                    Some(Ok(ShutdownStatus::Continue))
                ));
                drop(turn);
                assert_eq!(reactor.status(), PublishStatus::UploadScheduled);
                let inner = reactor.inner.lock().unwrap();
                assert_eq!(inner.time_periods.len(), periods.len());
                for (ctx, params) in inner.time_periods.iter().zip(&periods) {
                    assert_eq!(ctx.params, *params);
                    let expected = if matches!(trigger, SelectiveTrigger::Consensus)
                        && ctx.params.time_period() == later_period
                    {
                        DescriptorStatus::Dirty
                    } else {
                        DescriptorStatus::Clean
                    };
                    assert!(ctx.hs_dirs.iter().all(|(_, s)| *s == expected));
                }
            }
        }
        drain_ready_publisher(&mut reactor, &runtime).await;
        assert_eq!(reactor.status(), PublishStatus::Idle);
        let attempts = mock.attempts.lock().unwrap();
        assert_eq!(attempts.len(), before_attempts.len());
        for (id, count) in &before_attempts {
            assert_eq!(
                attempts[id],
                count + expected_extra.get(id).copied().unwrap_or(0)
            );
        }
        assert_eq!(
            attempts.values().sum::<usize>() - before_attempts.values().sum::<usize>(),
            expected_extra.values().sum::<usize>()
        );
        drop(attempts);
        let inner = reactor.inner.lock().unwrap();
        assert_eq!(inner.time_periods.len(), periods.len());
        for (i, ctx) in inner.time_periods.iter().enumerate() {
            assert!(
                ctx.hs_dirs
                    .iter()
                    .all(|(_, s)| *s == DescriptorStatus::Clean)
            );
            assert!(ctx.upload_results.iter().all(|r| r.upload_res.is_ok()));
            assert_eq!(ctx.upload_results.len(), ctx.hs_dirs.len());
            if !matches!(trigger, SelectiveTrigger::AllCleanConsensus)
                && ctx.params.time_period() == later_period
            {
                assert!(ctx.last_successful > before_results[i].0);
                assert!(
                    ctx.upload_results
                        .iter()
                        .all(|r| r.revision_counter > before_results[i].0.unwrap())
                );
            } else {
                if matches!(trigger, SelectiveTrigger::ReuploadTimer) {
                    assert_eq!(ctx.last_successful, before_results[i].0);
                } else {
                    // Consensus rebuilds contexts through TimePeriodContext::new,
                    // which resets this counter but retains matched upload results.
                    assert!(ctx.last_successful.is_none());
                }
                assert!(
                    ctx.upload_results
                        .iter()
                        .map(|r| (r.relay_ids.clone(), r.revision_counter))
                        .collect::<Vec<_>>()
                        == before_results[i].1
                );
            }
        }
        // Timer handling popped one timer; consensus did not. Only the target
        // period may add a completion/reupload timer. All-Clean adds nothing.
        let expected_timers = match trigger {
            SelectiveTrigger::ReuploadTimer => before_timers,
            SelectiveTrigger::Consensus => before_timers + 1,
            SelectiveTrigger::AllCleanConsensus => before_timers,
        };
        assert_eq!(inner.reupload_timers.len(), expected_timers);
    });
}

#[test]
fn selective_period_reupload_timer_rate_limit_then_upload() {
    selective_period_fixture(SelectiveTrigger::ReuploadTimer);
}

#[test]
fn selective_period_consensus_preserves_dirty_then_uploads() {
    selective_period_fixture(SelectiveTrigger::Consensus);
}

#[test]
fn selective_period_all_clean_consensus_uploads_nothing() {
    selective_period_fixture(SelectiveTrigger::AllCleanConsensus);
}

struct ResponseClockStream {
    headers: Cursor<Vec<u8>>,
    delay_headers: bool,
    delay_done: bool,
    delay: Pin<Box<dyn std::future::Future<Output = ()> + Send>>,
}

impl AsyncRead for ResponseClockStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        out: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        if !self.delay_done
            && (self.delay_headers
                || self.headers.position() as usize == self.headers.get_ref().len())
        {
            futures::ready!(self.delay.as_mut().poll(cx));
            self.delay_done = true;
        }
        Pin::new(&mut self.headers).poll_read(cx, out)
    }
}
impl AsyncWrite for ResponseClockStream {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(bytes.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

// Qualify the exact clean dirclient overlay in this same SDK attempt. The public
// generic API retains its clocks, while an opted-out response still has a caller
// deadline. Existing MockRuntime and in-memory I/O only; no new features/deps.
#[test]
fn response_clock_defaults_and_finite_caller_opt_out() {
    use tor_rtcompat::{SleepProvider as _, SleepProviderExt as _};
    let runtime = MockRuntime::new();
    runtime.clone().block_on(async move {
        for (opt_out, delay_headers, seconds) in [
            (false, true, 11),
            (false, false, 11),
            (true, true, 11),
            (true, false, 11),
            (true, true, 40),
        ] {
            let mut stream = ResponseClockStream {
                headers: Cursor::new(b"HTTP/1.0 200 OK\r\n\r\n".to_vec()),
                delay_headers,
                delay_done: false,
                delay: Box::pin(runtime.sleep(Duration::from_secs(seconds))),
            };
            let request = tor_dirclient::request::HsDescUploadRequest::new(Arc::from(
                "offline synthetic descriptor",
            ));
            let (tx, rx) = futures::channel::oneshot::channel();
            let task_runtime = runtime.clone();
            runtime
                .spawn(async move {
                    let result = task_runtime
                        .timeout(Duration::from_secs(30), async {
                            if opt_out {
                                tor_dirclient::send_request_with_response_timeout(
                                    &task_runtime,
                                    &request,
                                    &mut stream,
                                    None,
                                    None,
                                )
                                .await
                            } else {
                                tor_dirclient::send_request(
                                    &task_runtime,
                                    &request,
                                    &mut stream,
                                    None,
                                )
                                .await
                            }
                        })
                        .await;
                    assert!(tx.send(result).is_ok());
                })
                .unwrap();
            runtime.advance_by(Duration::from_secs(31)).await;
            let result = rx
                .now_or_never()
                .expect("bounded response-clock task did not complete")
                .expect("response-clock task lost its result");
            if seconds == 40 {
                assert!(result.is_err());
            } else if opt_out {
                assert_eq!(result.unwrap().unwrap().status_code(), 200);
            } else {
                assert!(
                    matches!(result, Ok(Err(tor_dirclient::Error::RequestFailed(e)))
                    if matches!(e.error, tor_dirclient::RequestError::DirTimeout))
                );
            }
        }
    });
}
