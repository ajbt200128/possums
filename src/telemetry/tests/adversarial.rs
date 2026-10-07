use super::*;

fn wait(flag: &AtomicBool) {
    let deadline = Instant::now() + std::time::Duration::from_secs(5);
    while !flag.load(Ordering::SeqCst) {
        assert!(
            Instant::now() < deadline,
            "telemetry test rendezvous timeout"
        );
        std::thread::yield_now();
    }
}

#[test]
fn registered_loser_cannot_publish_before_or_after_freeze() {
    for before_freeze in [true, false] {
        let metrics = new();
        ready(&metrics);
        ten_starts(&metrics);
        drive(&metrics, 599 * SECOND + SECOND / 2);
        metrics.clock.set(600 * SECOND);
        let mut state = metrics.lock().unwrap();
        let (_, mapped) = metrics.time(&mut state).unwrap();
        // Deterministic pause at the losing entrant's FIRST atomic instruction,
        // before it can run invalidate(). This is the preemption gap at issue.
        if before_freeze {
            assert_eq!(metrics.entrants.fetch_add(1, Ordering::SeqCst), 1);
        }
        metrics.advance(&mut state, mapped);
        if !before_freeze {
            assert_eq!(metrics.entrants.fetch_add(1, Ordering::SeqCst), 1);
        }
        assert!(state.requests.pending.is_some()); // staged, not releasable
        assert!(!metrics.eligible_token(state.epoch));
        drop(state);
        metrics.invalidate();
        assert_eq!(metrics.entrants.fetch_sub(1, Ordering::SeqCst), 1);
        assert!(metrics.request().is_none());
        assert!(metrics.off());
        assert!(metrics.state.lock().unwrap().requests.pending.is_none());
    }
}

#[test]
fn concurrent_contention_is_nonblocking_and_allocation_free() {
    let isolated = std::env::var_os("POSSUMS_TELEMETRY_ALLOCATION_TEST").is_some();
    let metrics = new();
    ready(&metrics);
    let held = metrics.lock().unwrap();
    let ready = AtomicBool::new(false);
    let go = AtomicBool::new(false);
    let done = AtomicBool::new(false);
    let release = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            ready.store(true, Ordering::SeqCst);
            wait(&go);
            // Must return while the main thread still holds state.
            drop(metrics.http(Endpoint::ChatApi));
            done.store(true, Ordering::SeqCst);
            wait(&release);
        });
        wait(&ready);
        let before = if isolated {
            crate::process_alloc_tests::ALLOCATOR.begin_phase()
        } else {
            crate::process_alloc_tests::ALLOCATOR.snapshot()
        };
        go.store(true, Ordering::SeqCst);
        wait(&done);
        let after = crate::process_alloc_tests::ALLOCATOR.snapshot();
        release.store(true, Ordering::SeqCst);
        worker.join().unwrap();
        assert_eq!(metrics.epoch.load(Ordering::SeqCst) % 2, 0);
        if isolated {
            assert_eq!(after.live, before.live);
            assert_eq!(after.phase_peak, before.live);
        }
    });
    drop(held);
    assert!(metrics.off());
    assert!(metrics.request().is_none());
}

#[test]
fn frozen_slot_pressure_discards_new_output_without_allocation() {
    let isolated = std::env::var_os("POSSUMS_TELEMETRY_ALLOCATION_TEST").is_some();
    let mut slots = Slots::<RequestTables>::default();
    slots.active.rejected[1176] = 10;
    slots.freeze(1, 300 * SECOND, true);
    let before = if isolated {
        crate::process_alloc_tests::ALLOCATOR.begin_phase()
    } else {
        crate::process_alloc_tests::ALLOCATOR.snapshot()
    };
    for i in 2..100 {
        slots.active.rejected[1176] = 11;
        slots.freeze(1, i * 300 * SECOND, true);
        assert_eq!(slots.frozen.as_ref().unwrap().rejected[1176], 10);
        assert_eq!(slots.pending.unwrap().0.end_ns, 300 * SECOND);
        assert_eq!(slots.attempted, i * 300 * SECOND);
        assert_eq!(slots.active.rejected[1176], 0);
    }
    let after = crate::process_alloc_tests::ALLOCATOR.snapshot();
    if isolated {
        assert_eq!(after.live, before.live);
        assert_eq!(after.phase_peak, before.live);
    }
}

#[test]
fn infrastructure_stale_intervals_true_zero_and_overflow_are_distinct() {
    for stale in [false, true] {
        let metrics = new();
        drive(&metrics, 60 * SECOND);
        metrics.poll();
        for interval in 1..=6 {
            let end = (60 + 10 * interval) * SECOND;
            drive(&metrics, end);
            metrics.resource(end, ResourceScope::Process, ResourceMetric::CpuUsed, 0.);
            metrics.resource(end, ResourceScope::Process, ResourceMetric::MemoryUsed, 0.);
            if stale && interval == 3 {
                metrics.resource(
                    end - 10 * SECOND,
                    ResourceScope::Process,
                    ResourceMetric::CpuUsed,
                    0.,
                );
            }
        }
        let view = metrics.infrastructure().unwrap();
        let table = view.tables().unwrap();
        assert_eq!(
            table.resource(ResourceScope::Process, ResourceMetric::CpuUsed),
            if stale { None } else { Some(0.) }
        );
        assert_eq!(
            table.resource(ResourceScope::Process, ResourceMetric::MemoryUsed),
            Some(0.)
        );
    }
    for invalid in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN, -1., f64::MAX] {
        let mut table = Infrastructure::default();
        for interval in 0..6 {
            table.observe(
                ResourceScope::Cgroup,
                ResourceMetric::CpuUsed,
                interval,
                invalid,
            );
            table.observe(
                ResourceScope::Cgroup,
                ResourceMetric::MemoryUsed,
                interval,
                1048576.,
            );
        }
        assert_eq!(
            table.resource(ResourceScope::Cgroup, ResourceMetric::CpuUsed),
            None
        );
        assert_eq!(
            table.resource(ResourceScope::Cgroup, ResourceMetric::MemoryUsed),
            Some(1048576.)
        );
    }
}

#[test]
fn startup_125_with_zero_monotonic_anchor_and_two_independent_grids() {
    let clock = FakeClock::new(125);
    clock.pair(0, 125 * SECOND);
    let metrics = Metrics::new(Deployment::IsolatedSynthetic, clock);
    for half in 1..=950 {
        let mono = half * SECOND / 2;
        let wall = 125 * SECOND + mono;
        metrics.clock.pair(mono, wall);
        if (190 * SECOND..=240 * SECOND).contains(&wall) && wall % (10 * SECOND) == 0 {
            metrics.resource(
                wall,
                ResourceScope::TinfoilEnclave,
                ResourceMetric::CpuUsed,
                1.,
            );
        }
        metrics.poll();
        if wall == 240 * SECOND {
            let view = metrics.infrastructure().unwrap();
            assert_eq!(
                view.window(),
                Window {
                    start_ns: 180 * SECOND,
                    end_ns: 240 * SECOND
                }
            );
            assert_eq!(
                view.tables()
                    .unwrap()
                    .resource(ResourceScope::TinfoilEnclave, ResourceMetric::CpuUsed),
                Some(1.)
            );
        }
        if wall == 300 * SECOND {
            ten_starts(&metrics);
        }
    }
    let view = metrics.request().unwrap();
    assert_eq!(
        view.window(),
        Window {
            start_ns: 300 * SECOND,
            end_ns: 600 * SECOND
        }
    );
    assert_eq!(
        view.tables().unwrap().rejected(
            None,
            AdmissionModel::NotApplicable,
            Rejection::ConnectionCapacity
        ),
        Some(10)
    );
}

#[test]
fn exact_negative_one_second_discrepancy_without_backward_wall_is_tolerated() {
    let metrics = new();
    ready(&metrics);
    ten_starts(&metrics);
    drive(&metrics, 599 * SECOND + SECOND / 2);
    metrics
        .clock
        .pair(600 * SECOND + SECOND / 2, 599 * SECOND + SECOND / 2);
    let view = metrics.request().unwrap();
    assert_eq!(view.window().end_ns, 600 * SECOND);
}

#[test]
fn off_and_nonisolated_v10_cannot_activate_observations() {
    for mode in [Deployment::Off, Deployment::NonIsolated] {
        let metrics = Metrics::new(mode, FakeClock::new(0));
        ready(&metrics);
        cohort(&metrics, 10, Vector::Plain);
        assert!(metrics.request().is_none());
        assert!(metrics.infrastructure().is_none());
        assert!(metrics.enable(Deployment::IsolatedSynthetic));
        drive(&metrics, 900 * SECOND);
        metrics.poll();
        ten_starts(&metrics);
        drive(&metrics, 1200 * SECOND);
        let view = metrics.request().unwrap();
        assert_eq!(view.tables().unwrap().series_count(), 1);
    }
}

#[test]
fn pending_expiry_and_kill_drop_both_slots_without_replacement() {
    for kill in [true, false] {
        let metrics = new();
        ready(&metrics);
        ten_starts(&metrics);
        for interval in 1..=6 {
            let end = (540 + 10 * interval) * SECOND;
            drive(&metrics, end);
            metrics.resource(end, ResourceScope::Cgroup, ResourceMetric::CpuUsed, 1.);
        }
        metrics.poll();
        {
            let state = metrics.state.lock().unwrap();
            assert!(state.requests.pending.is_some());
            assert!(state.infrastructure.pending.is_some());
        }
        if kill {
            assert!(metrics.off());
        } else {
            drive(&metrics, 602 * SECOND);
            metrics.poll();
        }
        assert!(metrics.request().is_none());
        assert!(metrics.infrastructure().is_none());
        let state = metrics.state.lock().unwrap();
        assert_eq!(state.requests.frozen.as_ref().unwrap().series_count(), 0);
        assert_eq!(
            state.infrastructure.frozen.as_ref().unwrap().series_count(),
            0
        );
    }
}

#[test]
fn pool_serial_and_epoch_exhaustion_do_not_wrap() {
    let metrics = new();
    ready(&metrics);
    metrics.state.lock().unwrap().serial = (1 << 56) - 1;
    assert_eq!(metrics.http(Endpoint::Home).key, 0);
    assert_eq!(metrics.epoch.load(Ordering::SeqCst) % 2, 0);
    let metrics = new();
    ready(&metrics);
    {
        let mut state = metrics.state.lock().unwrap();
        state.epoch = u64::MAX - 2;
        metrics.epoch.store(state.epoch, Ordering::SeqCst);
    }
    metrics.clock.set(901 * SECOND);
    metrics.poll();
    assert_eq!(metrics.epoch.load(Ordering::SeqCst), u64::MAX - 1);
    assert!(!metrics.enable(Deployment::IsolatedSynthetic));
    assert!(metrics.request().is_none());
}

#[test]
fn leases_released_exactly_on_boundary_do_not_touch_the_new_window() {
    let metrics = new();
    ready(&metrics);
    drive(&metrics, 590 * SECOND);
    let leases: [_; 10] = std::array::from_fn(|_| metrics.lease(Lane::Connection));
    drive(&metrics, 600 * SECOND);
    drop(leases);
    {
        let view = metrics.request().unwrap();
        assert_eq!(
            view.tables()
                .unwrap()
                .occupancy(Lane::Connection)
                .unwrap()
                .bins(),
            &[290, 0, 0, 0, 0, 0, 10, 0, 0, 0]
        );
    }
    drive(&metrics, 900 * SECOND);
    assert!(metrics.request().is_none());
}

#[test]
fn old_lease_markers_are_not_reused_when_a_discarded_grid_interval_recurs() {
    let metrics = new();
    ready(&metrics);
    drive(&metrics, 600 * SECOND);
    metrics.poll();
    let leases: [_; 10] = std::array::from_fn(|_| metrics.lease(Lane::Connection));
    drive(&metrics, 610 * SECOND);
    assert!(metrics.off());
    metrics.clock.set(100 * SECOND);
    assert!(metrics.enable(Deployment::IsolatedSynthetic));
    drive(&metrics, 610 * SECOND);
    drop(leases);
    drive(&metrics, 900 * SECOND);
    let view = metrics.request().unwrap();
    assert_eq!(
        view.tables()
            .unwrap()
            .occupancy(Lane::Connection)
            .unwrap()
            .bins(),
        &[290, 0, 0, 0, 0, 0, 10, 0, 0, 0]
    );
}

#[test]
fn attempt_watermarks_prevent_reopening_after_backward_reenable() {
    let metrics = new();
    ready(&metrics);
    ten_starts(&metrics);
    drive(&metrics, 600 * SECOND);
    assert!(metrics.request().is_some());
    metrics.clock.set(100 * SECOND);
    metrics.poll(); // backwards => off
    assert!(metrics.enable(Deployment::IsolatedSynthetic));
    drive(&metrics, 300 * SECOND);
    metrics.poll();
    ten_starts(&metrics);
    drive(&metrics, 600 * SECOND);
    assert!(metrics.request().is_none());
    ten_starts(&metrics);
    drive(&metrics, 900 * SECOND);
    let view = metrics.request().unwrap();
    assert_eq!(
        view.window(),
        Window {
            start_ns: 600 * SECOND,
            end_ns: 900 * SECOND
        }
    );
}
