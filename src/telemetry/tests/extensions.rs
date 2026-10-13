use super::*;
use crate::failures::InterfaceError;

pub(in crate::telemetry) fn qualify(m: &Metrics) {
    m.state
        .lock()
        .unwrap()
        .requests
        .active
        .extension_qualification = [true; 2];
}
pub(in crate::telemetry) fn observe(m: &Metrics) {
    m.interface_error(InterfaceError::BodyLimit);
    m.observed_usage(QualifiedModel(2), 100, 20, 120, Some(30));
}

#[test]
fn qualified_extensions_close_independently_without_request_lifecycles() {
    for defect in 0..4 {
        let m = new();
        ready(&m);
        qualify(&m);
        observe(&m);
        {
            let mut state = m.state.lock().unwrap();
            let t = &mut state.requests.active;
            match defect {
                0 => (),
                1 => t.valid = false,
                2 => t.http_completed[0] = 1,
                _ => t.tokens.observe(QualifiedModel(0), 1, 1, 3, Some(0)),
            }
        }
        drive(&m, 600 * SECOND);
        let view = m.request().unwrap();
        let t = view.tables().unwrap();
        assert_eq!(t.errors.get(InterfaceError::BodyLimit), Some(1));
        assert_eq!(
            t.tokens.input(QualifiedModel(2)),
            (defect != 3).then_some(100)
        );
        assert!(!t.request_families_releasable());
        assert!(t.complete());
        drop(view);
        assert!(m.request().is_none());
    }
    let m = new();
    ready(&m);
    observe(&m);
    drive(&m, 600 * SECOND);
    assert!(m.request().is_none()); // fixed production qualification remains withheld
}

#[test]
fn observers_share_warmup_off_restart_and_clock_fences() {
    let m = Metrics::new(Deployment::IsolatedSynthetic, FakeClock::new(125));
    qualify(&m);
    observe(&m);
    assert_eq!(
        m.state
            .lock()
            .unwrap()
            .requests
            .active
            .errors
            .get(InterfaceError::BodyLimit),
        None
    );
    drive(&m, 300 * SECOND);
    m.poll();
    qualify(&m);
    observe(&m);
    drive(&m, 450 * SECOND);
    assert!(m.off());
    observe(&m);
    assert!(m.request().is_none());
    assert!(m.enable(Deployment::IsolatedSynthetic));
    qualify(&m);
    observe(&m); // restart partial window stays ineligible
    drive(&m, 600 * SECOND);
    assert!(m.request().is_none());
    qualify(&m);
    observe(&m);
    drive(&m, 900 * SECOND);
    assert!(m.request().is_some());
    assert!(m.request().is_none());
    assert!(m.enable(Deployment::IsolatedSynthetic));
    drive(&m, 1200 * SECOND);
    m.poll();
    qualify(&m);
    observe(&m);
    m.clock.pair(1201 * SECOND, 1203 * SECOND);
    m.interface_error(InterfaceError::HeaderLimit);
    assert_eq!(m.epoch.load(Ordering::SeqCst) % 2, 0);
    assert!(m.request().is_none());
    assert_eq!(
        m.state
            .lock()
            .unwrap()
            .requests
            .active
            .tokens
            .input(QualifiedModel(2)),
        None
    );
}

#[test]
fn timing_loss_is_global_even_if_tick_count_is_later_complete() {
    let m = new();
    ready(&m);
    qualify(&m);
    observe(&m);
    m.clock.set(301 * SECOND + SECOND / 2); // first poll interval was missed
    m.poll();
    {
        let mut state = m.state.lock().unwrap();
        let t = &mut state.requests.active;
        assert!(!t.window_valid);
        assert_eq!(t.ticks, 0);
        // Explicit regression: ticks alone cannot rehabilitate known lost timing.
        t.ticks = 300;
        t.valid = true;
        assert!(!t.releasable());
    }
    drive(&m, 600 * SECOND);
    assert!(m.request().is_none());
    for delay in [0, SECOND, SECOND + 1] {
        let m = new();
        ready(&m);
        qualify(&m);
        observe(&m);
        drive(&m, 599 * SECOND + SECOND / 2);
        m.clock.set(600 * SECOND + delay);
        assert_eq!(m.request().is_some(), delay <= SECOND);
        assert!(m.request().is_none());
    }
}

#[test]
fn bounded_extension_observations_allocate_nothing() {
    let m = new();
    ready(&m);
    qualify(&m);
    let isolated = std::env::var_os("POSSUMS_TELEMETRY_ALLOCATION_TEST").is_some();
    let before = if isolated {
        crate::process_alloc_tests::ALLOCATOR.begin_phase()
    } else {
        crate::process_alloc_tests::ALLOCATOR.snapshot()
    };
    for _ in 0..1000 {
        observe(&m);
    }
    let after = crate::process_alloc_tests::ALLOCATOR.snapshot();
    if isolated {
        assert_eq!(after.live, before.live);
        assert_eq!(after.phase_peak, before.live);
        println!("extension observers requested allocation delta=0 bytes");
    }
    drive(&m, 600 * SECOND);
    let view = m.request().unwrap();
    let t = view.tables().unwrap();
    assert_eq!(t.errors.get(InterfaceError::BodyLimit), Some(1000));
    assert_eq!(t.tokens.input(QualifiedModel(2)), Some(100_000));
}

#[test]
fn either_observer_contention_invalidates_all_pending_families() {
    for usage in [false, true] {
        let m = new();
        ready(&m);
        qualify(&m);
        observe(&m);
        drive(&m, 600 * SECOND);
        m.poll();
        let guard = m.state.lock().unwrap();
        std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    if usage {
                        m.observed_usage(QualifiedModel(0), 0, 0, 0, Some(0));
                    } else {
                        m.interface_error(InterfaceError::HeaderLimit);
                    }
                })
                .join()
                .unwrap();
        });
        drop(guard);
        assert_eq!(m.epoch.load(Ordering::SeqCst) % 2, 0);
        assert!(m.request().is_none());
        assert!(m.off());
    }
}

#[test]
fn pending_expiry_skips_and_process_restart_never_replay_extensions() {
    let m = new();
    ready(&m);
    qualify(&m);
    observe(&m);
    drive(&m, 600 * SECOND);
    m.poll();
    drive(&m, 602 * SECOND);
    assert!(m.request().is_none());
    assert!(m.enable(Deployment::IsolatedSynthetic));
    m.clock.set(100 * SECOND);
    assert!(m.enable(Deployment::IsolatedSynthetic));
    assert!(m.state.lock().unwrap().requests.eligible >= 600 * SECOND);
    assert!(m.request().is_none());
    let restarted = Metrics::new(Deployment::IsolatedSynthetic, FakeClock::new(460));
    qualify(&restarted);
    observe(&restarted);
    drive(&restarted, 600 * SECOND);
    assert!(restarted.request().is_none());
    let m = new();
    ready(&m);
    qualify(&m);
    observe(&m);
    m.clock.set(901 * SECOND);
    m.poll();
    assert!(m.request().is_none());
    assert_eq!(m.state.lock().unwrap().requests.eligible, 1200 * SECOND);
}
