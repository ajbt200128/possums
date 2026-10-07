mod adversarial;

use super::*;
use std::sync::atomic::AtomicBool;

pub(super) struct FakeClock {
    mono: AtomicU64,
    wall: AtomicU64,
    available: AtomicBool,
}
impl FakeClock {
    fn new(seconds: u64) -> Self {
        Self {
            mono: AtomicU64::new(seconds * SECOND),
            wall: AtomicU64::new(seconds * SECOND),
            available: AtomicBool::new(true),
        }
    }
    pub(super) fn set(&self, ns: u64) {
        self.pair(ns, ns);
    }
    pub(super) fn pair(&self, mono: u64, wall: u64) {
        self.mono.store(mono, Ordering::SeqCst);
        self.wall.store(wall, Ordering::SeqCst);
    }
}
impl Clock for FakeClock {
    fn now(&self) -> Option<ClockReading> {
        self.available.load(Ordering::SeqCst).then(|| ClockReading {
            monotonic_ns: self.mono.load(Ordering::SeqCst),
            wall_ns: self.wall.load(Ordering::SeqCst),
        })
    }
}
pub(super) type Metrics = AggregateMetrics<FakeClock>;
pub(super) fn new() -> Metrics {
    Metrics::new(Deployment::IsolatedSynthetic, FakeClock::new(0))
}
pub(super) fn drive(metrics: &Metrics, ns: u64) {
    let now = metrics.clock.now().unwrap().wall_ns;
    assert!(ns >= now);
    let mut tick = now / SECOND * SECOND + SECOND / 2;
    if tick <= now {
        tick += SECOND;
    }
    while tick <= ns {
        metrics.clock.set(tick);
        metrics.poll();
        tick += SECOND;
    }
    metrics.clock.set(ns);
}
pub(super) fn ready(metrics: &Metrics) {
    drive(metrics, 300 * SECOND);
    metrics.poll();
}
fn finish_http(http: &mut Observation<'_, FakeClock>) {
    http.finish_http(
        Status::Success,
        HttpTerminal::Eof,
        Disposition::NewGeneration,
    );
}
fn root_generation(metrics: &Metrics) -> Observation<'_, FakeClock> {
    // Vunknown / Vsplit are table-only fixtures, not serving lane evidence.
    metrics.create(
        Kind::Generation,
        Endpoint::ChatApi,
        QualifiedModel(0),
        Lane::Control,
        None,
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Vector {
    Plain,
    RareError,
    RareModel,
    RareEndpoint,
    RareBin,
    Mixed,
    MissingRare,
    MissingRelease,
    MissingAll,
    OutputFailure,
    Duplicate,
}
pub(super) fn cohort(metrics: &Metrics, n: u64, vector: Vector) {
    cohort_before_close(metrics, n, vector);
    drive(metrics, 600 * SECOND);
}
pub(super) fn cohort_before_close(metrics: &Metrics, n: u64, vector: Vector) {
    for j in 0..n {
        let t = (300 + 10 * j) * SECOND;
        drive(metrics, t);
        let endpoint = if vector == Vector::RareEndpoint && j == 10 {
            Endpoint::ChatWeb
        } else {
            Endpoint::ChatApi
        };
        let model = QualifiedModel(if vector == Vector::RareModel && j == 10 {
            1
        } else {
            0
        });
        let mut http = metrics.http(endpoint);
        let connection = metrics.lease(Lane::Connection);
        let heavy = metrics.lease(Lane::Heavy);
        let ingress = metrics.lease(Lane::Ingress);
        drive(metrics, t + SECOND);
        drop(ingress);
        if vector == Vector::Duplicate && j == 9 {
            drive(metrics, t + 3 * SECOND);
            http.finish_http(Status::Success, HttpTerminal::Eof, Disposition::Duplicate);
        } else {
            let generation_lease = metrics.lease(Lane::Generation);
            let mut generation = metrics.generation(&http, model);
            let mut delivery = metrics.delivery(&http, model);
            generation.dispatch();
            generation.dispatch();
            let missing = vector == Vector::MissingAll
                || (vector == Vector::MissingRare && j == 10)
                || (vector == Vector::MissingRelease && j >= 10);
            let delay = if vector == Vector::RareBin && j == 10 {
                600_000_000
            } else {
                200_000_000
            };
            drive(metrics, t + SECOND + delay);
            if !missing {
                generation.first_output();
                generation.first_output();
            }
            drive(metrics, t + 2 * SECOND);
            let terminal = if vector == Vector::OutputFailure {
                GenerationTerminal::TerminalUsage
            } else if (vector == Vector::RareError && j == 10)
                || (vector == Vector::Mixed && j >= 10)
            {
                GenerationTerminal::Transport
            } else {
                GenerationTerminal::Success
            };
            generation.finish_generation(terminal);
            generation.finish_generation(GenerationTerminal::Unknown);
            drop(generation_lease);
            drive(metrics, t + 3 * SECOND);
            delivery.finish_delivery(DeliveryTerminal::Completed);
            delivery.finish_delivery(DeliveryTerminal::Unknown);
            finish_http(&mut http);
            finish_http(&mut http);
        }
        drop((http, connection, heavy));
    }
}

/// Independent literal expansion of contract C+Q, not derived by observing the
/// implementation. Compare every exported cell, including absence elsewhere.
fn expected(n: u64, vector: Vector) -> RequestTables {
    let mut out = RequestTables::default();
    out.http_starts[14] = n;
    out.http_completed[255] = n; // (chat_api=14, 2xx=1, eof=0)
    out.http_duration[255].0[3] = n;
    out.generation_starts[3] = n; // (chat_api=1, kimi=0)
    out.delivery[9] = n;
    if vector == Vector::Mixed {
        for cell in [36, 41] {
            out.generation_completed[cell] = 10;
            out.generation_duration[cell].0[2] = 10;
            out.first_output[cell].0[1] = 10;
        }
    } else {
        let cell = if vector == Vector::OutputFailure {
            44
        } else {
            36
        };
        out.generation_completed[cell] = n;
        out.generation_duration[cell].0[2] = n;
        out.first_output[cell].0[1] = match vector {
            Vector::MissingAll => 0,
            Vector::MissingRelease => 10,
            _ => n,
        };
    }
    for lane in [0, 2] {
        out.occupancy[lane].0 = [300 - 3 * n, 3 * n, 0, 0, 0, 0, 0, 0, 0, 0];
    }
    for lane in [1, 3] {
        out.occupancy[lane].0 = [300 - n, n, 0, 0, 0, 0, 0, 0, 0, 0];
    }
    out.contributors = [n, n, n, n, 0, 0];
    out
}
fn assert_export(actual: &RequestTables, expected: &RequestTables) {
    assert_eq!(actual.http_starts, expected.http_starts);
    assert_eq!(actual.http_completed, expected.http_completed);
    assert_eq!(actual.http_duration, expected.http_duration);
    assert_eq!(actual.generation_starts, expected.generation_starts);
    assert_eq!(actual.generation_completed, expected.generation_completed);
    assert_eq!(actual.generation_duration, expected.generation_duration);
    assert_eq!(actual.first_output, expected.first_output);
    assert_eq!(actual.delivery, expected.delivery);
    assert_eq!(actual.rejected, expected.rejected);
    for lane in 0..6 {
        if expected.contributors[lane] == 0 {
            assert_eq!(actual.contributors[lane], 0);
        } else {
            assert_eq!(actual.occupancy[lane], expected.occupancy[lane]);
        }
    }
    assert_eq!(actual.series_count(), expected.series_count());
}
#[test]
fn frozen_cohort_vectors() {
    for (n, vector, release, series) in [
        (0, Vector::Plain, false, 0),
        (1, Vector::Plain, false, 0),
        (9, Vector::Plain, false, 0),
        (10, Vector::Plain, true, 12),
        (11, Vector::Plain, true, 12),
        (11, Vector::RareError, false, 0),
        (11, Vector::RareModel, false, 0),
        (11, Vector::RareEndpoint, false, 0),
        (11, Vector::RareBin, false, 0),
        (20, Vector::Mixed, true, 15),
        (11, Vector::MissingRare, false, 0),
        (20, Vector::MissingRelease, true, 12),
        (10, Vector::MissingAll, true, 11),
        (10, Vector::OutputFailure, true, 12),
        (10, Vector::Duplicate, false, 0),
    ] {
        let metrics = new();
        ready(&metrics);
        cohort(&metrics, n, vector);
        let view = metrics.request();
        assert_eq!(view.is_some(), release, "{vector:?}/{n}");
        if let Some(view) = view {
            assert_eq!(
                view.window(),
                Window {
                    start_ns: 300 * SECOND,
                    end_ns: 600 * SECOND
                }
            );
            let actual = view.tables().unwrap();
            assert_export(actual, &expected(n, vector));
            assert_eq!(actual.series_count(), series);
        }
        assert!(metrics.request().is_none());
    }
}
#[test]
fn frozen_rejection_and_unknown_vectors() {
    for n in [10, 11] {
        let metrics = new();
        ready(&metrics);
        for _ in 0..n {
            metrics.reject(
                None,
                AdmissionModel::NotApplicable,
                Rejection::ConnectionCapacity,
            );
        }
        drive(&metrics, 600 * SECOND);
        let view = metrics.request().unwrap();
        let mut expected = RequestTables::default();
        expected.rejected[1176] = n;
        assert_export(view.tables().unwrap(), &expected);
    }
    let metrics = new();
    ready(&metrics);
    drive(&metrics, 301 * SECOND);
    let mut generations: [_; 10] = std::array::from_fn(|_| root_generation(&metrics));
    drive(&metrics, 302 * SECOND);
    for g in &mut generations {
        g.finish_generation(GenerationTerminal::Unknown);
    }
    drop(generations);
    drive(&metrics, 600 * SECOND);
    let view = metrics.request().unwrap();
    let mut expected = RequestTables::default();
    expected.generation_starts[3] = 10;
    expected.generation_completed[47] = 10;
    expected.generation_duration[47].0[2] = 10;
    assert_export(view.tables().unwrap(), &expected);
}
#[test]
fn frozen_split_and_split_failure_vectors() {
    for terminal in [
        GenerationTerminal::Success,
        GenerationTerminal::TerminalUsage,
    ] {
        let metrics = new();
        ready(&metrics);
        drive(&metrics, 599 * SECOND);
        let mut http: [_; 10] = std::array::from_fn(|_| metrics.http(Endpoint::ChatApi));
        let mut generations: [_; 10] =
            std::array::from_fn(|i| metrics.generation(&http[i], QualifiedModel(0)));
        let mut delivery: [_; 10] =
            std::array::from_fn(|i| metrics.delivery(&http[i], QualifiedModel(0)));
        for g in &mut generations {
            g.dispatch();
        }
        drive(&metrics, 599 * SECOND + 200_000_000);
        for g in &mut generations {
            g.first_output();
        }
        drive(&metrics, 600 * SECOND);
        {
            let view = metrics.request().unwrap();
            let mut expected = RequestTables::default();
            expected.http_starts[14] = 10;
            expected.generation_starts[3] = 10;
            assert_export(view.tables().unwrap(), &expected);
        }
        assert!(metrics.request().is_none());
        drive(&metrics, 601 * SECOND);
        for g in &mut generations {
            g.finish_generation(terminal);
        }
        for d in &mut delivery {
            d.finish_delivery(DeliveryTerminal::Completed);
        }
        for h in &mut http {
            finish_http(h);
        }
        drop((generations, delivery, http));
        drive(&metrics, 900 * SECOND);
        let view = metrics.request().unwrap();
        let mut expected = RequestTables::default();
        expected.http_completed[255] = 10;
        expected.http_duration[255].0[3] = 10;
        expected.delivery[9] = 10;
        let cell = if terminal == GenerationTerminal::Success {
            36
        } else {
            44
        };
        expected.generation_completed[cell] = 10;
        expected.generation_duration[cell].0[3] = 10;
        expected.first_output[cell].0[1] = 10;
        assert_export(view.tables().unwrap(), &expected);
    }
}

#[test]
fn prompt_quantization_and_invalid_lifecycle_updates() {
    for (elapsed, bucket) in [
        (0, 0),
        (100_000_000, 0),
        (100_000_001, 1),
        (500_000_000, 1),
        (SECOND, 2),
        (600 * SECOND, 9),
        (600 * SECOND + 1, 10),
    ] {
        assert_eq!(duration_bucket(7, 7 + elapsed), Some(bucket));
    }
    assert_eq!(duration_bucket(2, 1), None);
    let metrics = new();
    ready(&metrics);
    let mut g = root_generation(&metrics);
    g.dispatch();
    drive(&metrics, 300 * SECOND + 200_000_000);
    g.first_output();
    g.dispatch();
    {
        let state = metrics.state.lock().unwrap();
        let r = state.record(g.key).unwrap();
        assert_eq!(r.first, Some(1));
        assert_eq!(r.dispatch, None);
    }
    g.finish_generation(GenerationTerminal::Success);
    drop(g);
    // No dispatch => sanitizer failure, never a missing-output zero sample.
    let mut g = root_generation(&metrics);
    g.first_output();
    assert!(!metrics.state.lock().unwrap().requests.active.valid);
    g.finish_generation(GenerationTerminal::Unknown);
}
#[test]
fn release_predicate_checks_every_atom_and_complement() {
    let mut table = expected(10, Vector::Plain);
    table.ticks = 300;
    table.dispositions[255][0] = 10;
    assert!(table.releasable());
    table.dispositions[255] = [9, 1, 0, 0, 0];
    assert!(!table.releasable());
    table.dispositions[255] = [10, 0, 0, 0, 0];
    table.first_output[36].0[1] = 9;
    assert!(!table.releasable());
    table.first_output[36].0[1] = 11;
    assert!(!table.releasable());
    table.first_output[36].0[1] = 10;
    table.http_duration[255].0[3] = 11;
    assert!(!table.releasable());
    table.http_duration[255].0[3] = 10;
    table.generation_duration[36].0[2] = 11;
    assert!(!table.releasable());
    table.generation_duration[36].0[2] = 10;
    table.contributors[0] = 1;
    assert!(!table.releasable());
    table.contributors[0] = 10;
    table.occupancy[0].0[0] -= 1;
    table.occupancy[0].0[2] = 1;
    assert!(!table.releasable());
    table.occupancy[0].0[0] += 1;
    table.occupancy[0].0[2] = 0;
    assert!(table.releasable());
    for index in 0..1190 {
        table.rejected[index] = 1;
        assert!(!table.releasable());
        table.rejected[index] = 0;
    }
    table.rejected[0] = u64::MAX;
    increment(&mut table.rejected[0], &mut table.valid);
    assert!(!table.releasable());
}

#[test]
fn warmup_boundary_startup_125_and_old_children_are_ineligible() {
    let metrics = Metrics::new(Deployment::IsolatedSynthetic, FakeClock::new(125));
    assert_eq!(
        metrics.state.lock().unwrap().requests.eligible,
        300 * SECOND
    );
    assert_eq!(
        metrics.state.lock().unwrap().infrastructure.eligible,
        180 * SECOND
    );
    drive(&metrics, 299 * SECOND);
    let mut parent = metrics.http(Endpoint::ChatApi);
    drive(&metrics, 301 * SECOND);
    let mut child = metrics.generation(&parent, QualifiedModel(0));
    let mut delivery = metrics.delivery(&parent, QualifiedModel(0));
    finish_http(&mut parent);
    child.finish_generation(GenerationTerminal::Success);
    delivery.finish_delivery(DeliveryTerminal::Completed);
    drop((parent, child, delivery));
    drive(&metrics, 600 * SECOND);
    assert!(metrics.request().is_none());
    assert!(metrics.enable(Deployment::IsolatedSynthetic));
    assert_eq!(
        metrics.state.lock().unwrap().requests.eligible,
        900 * SECOND
    );
}
#[test]
fn old_epoch_contexts_cannot_resurrect_but_old_leases_measure_actual_occupancy() {
    for n in [1, 10] {
        let metrics = new();
        ready(&metrics);
        drive(&metrics, 310 * SECOND);
        let mut http = metrics.http(Endpoint::ChatApi);
        let mut generation = metrics.generation(&http, QualifiedModel(0));
        generation.dispatch();
        let mut leases: [Option<Observation<'_, FakeClock>>; 10] =
            std::array::from_fn(|i| (i < n).then(|| metrics.lease(Lane::Connection)));
        drive(&metrics, 320 * SECOND);
        assert!(metrics.off());
        drive(&metrics, 325 * SECOND);
        assert!(metrics.enable(Deployment::IsolatedSynthetic));
        drive(&metrics, 610 * SECOND);
        for lease in &mut leases {
            drop(lease.take());
        }
        drive(&metrics, 650 * SECOND);
        let mut child = metrics.generation(&http, QualifiedModel(0));
        child.finish_generation(GenerationTerminal::Success);
        generation.first_output();
        generation.finish_generation(GenerationTerminal::Success);
        finish_http(&mut http);
        drop((child, generation, http));
        drive(&metrics, 900 * SECOND);
        let view = metrics.request();
        assert_eq!(view.is_some(), n == 10);
        if let Some(view) = view {
            let mut expected = RequestTables::default();
            expected.contributors[0] = 10;
            expected.occupancy[0].0 = [290, 0, 0, 0, 0, 0, 10, 0, 0, 0];
            assert_export(view.tables().unwrap(), &expected);
        }
    }
}
#[test]
fn briefly_held_leases_count_and_capacity_is_not_a_sample_count() {
    let metrics = new();
    ready(&metrics);
    for j in 0..10 {
        drive(&metrics, 300 * SECOND + j * 10_000_000);
        let lease = metrics.lease(Lane::Connection);
        drive(&metrics, 300 * SECOND + j * 10_000_000 + 1);
        drop(lease);
    }
    drive(&metrics, 600 * SECOND);
    let view = metrics.request().unwrap();
    let actual = view.tables().unwrap();
    assert_eq!(actual.series_count(), 1);
    assert_eq!(
        actual.occupancy(Lane::Connection).unwrap().bins(),
        &[300, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    );
    drop(view);
    let leases: [_; 5] = std::array::from_fn(|_| metrics.lease(Lane::Generation));
    drive(&metrics, 601 * SECOND);
    assert!(!metrics.state.lock().unwrap().requests.active.valid);
    drop(leases);
}

fn ten_starts(metrics: &Metrics) {
    // Ten independent pre-router attempts need no fabricated HTTP lifecycle.
    for _ in 0..10 {
        metrics.reject(
            None,
            AdmissionModel::NotApplicable,
            Rejection::ConnectionCapacity,
        );
    }
}
#[test]
fn clock_faults_latch_off_and_watermarks_survive_off_on() {
    for (mono, wall) in [
        (610 * SECOND, 608 * SECOND),
        (610 * SECOND, 612 * SECOND),
        (609 * SECOND, 610 * SECOND),
    ] {
        let metrics = new();
        ready(&metrics);
        ten_starts(&metrics);
        drive(&metrics, 600 * SECOND);
        assert!(metrics.request().is_some());
        drive(&metrics, 610 * SECOND);
        metrics.poll();
        metrics.clock.pair(mono, wall);
        metrics.poll();
        assert_eq!(metrics.epoch.load(Ordering::SeqCst) % 2, 0);
        metrics.clock.set(620 * SECOND);
        metrics.poll();
        assert!(metrics.request().is_none());
        metrics.clock.set(100 * SECOND);
        assert!(metrics.enable(Deployment::IsolatedSynthetic));
        assert!(metrics.state.lock().unwrap().requests.eligible >= 600 * SECOND);
    }
    let metrics = new();
    ready(&metrics);
    metrics.clock.available.store(false, Ordering::SeqCst);
    metrics.poll();
    assert_eq!(metrics.epoch.load(Ordering::SeqCst) % 2, 0);
}
#[test]
fn exact_one_second_clock_discrepancy_preserves_fixed_boundaries() {
    let metrics = new();
    ready(&metrics);
    ten_starts(&metrics);
    drive(&metrics, 599 * SECOND + SECOND / 2);
    metrics.clock.pair(600 * SECOND, 601 * SECOND);
    let view = metrics.request().unwrap();
    assert_eq!(view.window().end_ns, 600 * SECOND);
}
#[test]
fn skipped_windows_invalidate_contexts_and_require_fresh_warmup() {
    let metrics = new();
    ready(&metrics);
    let mut context = root_generation(&metrics);
    drive(&metrics, 599 * SECOND);
    metrics.poll();
    metrics.clock.set(901 * SECOND);
    metrics.poll();
    assert!(metrics.request().is_none());
    assert_eq!(
        metrics.state.lock().unwrap().requests.eligible,
        1200 * SECOND
    );
    drive(&metrics, 1201 * SECOND);
    context.finish_generation(GenerationTerminal::Success);
    drop(context);
    ten_starts(&metrics);
    drive(&metrics, 1500 * SECOND);
    let view = metrics.request().unwrap();
    assert_eq!(view.tables().unwrap().series_count(), 1);
    assert_eq!(
        view.window(),
        Window {
            start_ns: 1200 * SECOND,
            end_ns: 1500 * SECOND
        }
    );
}
#[test]
fn missing_tick_delayed_close_duplicate_collect_and_restart() {
    for delay in [0, SECOND / 2, SECOND, SECOND + 1] {
        let metrics = new();
        ready(&metrics);
        ten_starts(&metrics);
        drive(&metrics, 599 * SECOND + SECOND / 2);
        metrics.clock.set(600 * SECOND + delay);
        assert_eq!(metrics.request().is_some(), delay <= SECOND);
        assert!(metrics.request().is_none());
    }
    let metrics = new();
    ready(&metrics);
    ten_starts(&metrics);
    metrics.clock.set(301 * SECOND);
    drive(&metrics, 600 * SECOND);
    assert!(metrics.request().is_none());
    let restarted = Metrics::new(Deployment::IsolatedSynthetic, FakeClock::new(460));
    assert_eq!(
        restarted.state.lock().unwrap().requests.eligible,
        600 * SECOND
    );
    assert!(restarted.request().is_none());
}
#[test]
fn regression_and_overflow_fail_closed_without_panicking() {
    let metrics = new();
    ready(&metrics);
    metrics.clock.pair(299 * SECOND, 300 * SECOND);
    metrics.poll();
    assert_eq!(metrics.epoch.load(Ordering::SeqCst) % 2, 0);
    let metrics = Metrics::new(Deployment::IsolatedSynthetic, FakeClock::new(0));
    metrics.clock.pair(0, u64::MAX);
    assert!(!metrics.enable(Deployment::IsolatedSynthetic));
    metrics.clock.pair(0, u64::MAX - REQUEST_WINDOW);
    assert!(metrics.enable(Deployment::IsolatedSynthetic));
    metrics.clock.pair(REQUEST_WINDOW + 1, u64::MAX);
    metrics.poll();
    assert_eq!(metrics.epoch.load(Ordering::SeqCst) % 2, 0);
}

#[test]
fn contention_invalidates_pending_and_local_kill_acknowledges_disposal() {
    let metrics = new();
    ready(&metrics);
    ten_starts(&metrics);
    drive(&metrics, 600 * SECOND);
    metrics.poll();
    let guard = metrics.state.lock().unwrap();
    std::thread::scope(|scope| {
        scope
            .spawn(|| drop(metrics.http(Endpoint::Home)))
            .join()
            .unwrap();
        assert!(!metrics.off());
    });
    drop(guard);
    assert!(metrics.off());
    assert!(metrics.request().is_none());
    let state = metrics.state.lock().unwrap();
    assert!(state.requests.pending.is_none());
    assert!(state.infrastructure.pending.is_none());
}
#[test]
fn pressure_stale_handles_lost_observations_and_sanitizer_failure() {
    let metrics = new();
    ready(&metrics);
    let handles: [_; 256] = std::array::from_fn(|_| metrics.http(Endpoint::Home));
    let key = handles[0].key;
    let extra = metrics.http(Endpoint::Home);
    assert_eq!(extra.key, 0);
    assert_eq!(metrics.epoch.load(Ordering::SeqCst) % 2, 0);
    drop((extra, handles));
    assert!(metrics.enable(Deployment::IsolatedSynthetic));
    drive(&metrics, 600 * SECOND);
    metrics.poll();
    let mut current = metrics.http(Endpoint::Home);
    assert_ne!(current.key, key);
    metrics.update(
        key,
        Update::Http(
            Status::Success,
            HttpTerminal::Eof,
            Disposition::ControlOrOther,
        ),
    );
    assert!(
        !metrics
            .state
            .lock()
            .unwrap()
            .record(current.key)
            .unwrap()
            .terminal
    );
    current.finish_http(
        Status::Success,
        HttpTerminal::Eof,
        Disposition::ControlOrOther,
    );
    drop(current);
    drop(metrics.http(Endpoint::Home));
    assert!(!metrics.state.lock().unwrap().requests.active.valid);
    metrics.reject(None, AdmissionModel::Unknown, Rejection::ConnectionCapacity);
    assert!(!metrics.state.lock().unwrap().requests.active.valid);
}
#[test]
fn lost_off_epoch_lease_cannot_be_forgotten_on_enable() {
    let metrics = Metrics::new(Deployment::Off, FakeClock::new(0));
    let leases: [_; 256] = std::array::from_fn(|_| metrics.lease(Lane::Connection));
    let lost = metrics.lease(Lane::Control);
    assert_eq!(lost.key, 0);
    drop((leases, lost));
    // No out-of-pool marker or guessed release time. Only a new controller can
    // establish a complete lease inventory after this fail-closed condition.
    assert!(!metrics.enable(Deployment::IsolatedSynthetic));
    ready(&metrics);
    ten_starts(&metrics);
    drive(&metrics, 600 * SECOND);
    assert!(metrics.request().is_none());
}

#[test]
fn infrastructure_six_intervals_and_availability() {
    use ResourceMetric::*;
    use ResourceScope::*;
    for defect in 0..6 {
        let metrics = new();
        drive(&metrics, 60 * SECOND);
        metrics.poll();
        for i in 1..=6 {
            let end = (60 + i * 10) * SECOND;
            drive(&metrics, end);
            if defect == 1 && i == 3 {
                continue;
            }
            for (metric, value) in [
                (CpuUsed, 1.),
                (MemoryUsed, 1048576.),
                (CpuCapacity, 2.),
                (MemoryCapacity, 8388608.),
            ] {
                let value = if defect == 3 && metric == CpuCapacity && i == 3 {
                    3.
                } else {
                    value
                };
                metrics.resource(end, TinfoilEnclave, metric, value);
            }
            if defect == 2 && i == 3 {
                metrics.resource(end, TinfoilEnclave, CpuUsed, 2.);
            }
            if defect == 4 && i == 3 {
                metrics.resource(end, TinfoilEnclave, MemoryUsed, f64::NAN);
            }
            for lane in [
                Lane::Connection,
                Lane::Generation,
                Lane::Heavy,
                Lane::Ingress,
                Lane::NewChat,
                Lane::Control,
            ] {
                metrics.configured_capacity(
                    end,
                    lane,
                    if defect == 5 && i == 3 {
                        99
                    } else {
                        CAPACITIES[lane as usize]
                    },
                );
            }
        }
        let view = metrics.infrastructure();
        if defect == 1 {
            assert!(view.is_none());
            continue;
        }
        let view = view.unwrap();
        let table = view.tables().unwrap();
        assert_eq!(
            view.window(),
            Window {
                start_ns: 60 * SECOND,
                end_ns: 120 * SECOND
            }
        );
        assert_eq!(
            table.resource(TinfoilEnclave, CpuUsed),
            if defect == 2 { None } else { Some(1.) }
        );
        assert_eq!(
            table.resource(TinfoilEnclave, MemoryUsed),
            if defect == 4 { None } else { Some(1048576.) }
        );
        assert_eq!(
            table.resource(TinfoilEnclave, CpuCapacity),
            if defect == 3 { None } else { Some(2.) }
        );
        assert_eq!(
            table.resource(TinfoilEnclave, MemoryCapacity),
            Some(8388608.)
        );
        assert_eq!(table.resource(Process, CpuCapacity), None);
        assert_eq!(table.resource(TinfoilWorkload, CpuUsed), None);
        assert_eq!(
            table.capacity(Lane::Connection),
            if defect == 5 { None } else { Some(64.) }
        );
        drop(view);
        metrics.resource(120 * SECOND, TinfoilEnclave, CpuUsed, 9.);
        assert!(metrics.infrastructure().is_none());
    }
}

#[test]
fn frozen_cardinality_and_layout_ceilings() {
    assert_eq!(REQUEST_SERIES, 2028);
    assert_eq!(REQUEST_CELLS, 6402);
    assert_eq!(INFRASTRUCTURE_SERIES, 20);
    assert!(std::mem::size_of::<RequestTables>() <= 67_456);
    assert!(std::mem::size_of::<Infrastructure>() <= 2048);
    assert!(std::mem::size_of::<Record>() <= 128);
    assert!(std::mem::size_of::<Observation<'_, FakeClock>>() <= 16);
    assert!(std::mem::size_of::<Metrics>() <= 65_536);
    let mut table = RequestTables {
        ticks: 300,
        ..RequestTables::default()
    };
    table.http_starts.fill(110);
    table.http_completed.fill(110);
    for h in &mut table.http_duration {
        h.0.fill(10);
    }
    for d in &mut table.dispositions {
        *d = [110, 0, 0, 0, 0];
    }
    table.generation_starts.fill(110);
    table.generation_completed.fill(110);
    for h in table
        .generation_duration
        .iter_mut()
        .chain(&mut table.first_output)
    {
        h.0.fill(10);
    }
    table.delivery.fill(10);
    table.rejected.fill(10);
    table.contributors.fill(10);
    for h in &mut table.occupancy {
        h.0.fill(30);
    }
    assert!(table.releasable());
    assert_eq!(table.series_count(), 2028);
    let mut infrastructure = Infrastructure::default();
    for interval in 0..6 {
        for scope in [
            ResourceScope::TinfoilEnclave,
            ResourceScope::TinfoilWorkload,
            ResourceScope::Process,
            ResourceScope::Cgroup,
        ] {
            for metric in [
                ResourceMetric::CpuUsed,
                ResourceMetric::MemoryUsed,
                ResourceMetric::CpuCapacity,
                ResourceMetric::MemoryCapacity,
            ] {
                infrastructure.observe(scope, metric, interval, 1.);
            }
        }
        for lane in [
            Lane::Connection,
            Lane::Generation,
            Lane::Heavy,
            Lane::Ingress,
            Lane::NewChat,
            Lane::Control,
        ] {
            infrastructure.configuration(lane, interval, CAPACITIES[lane as usize]);
        }
    }
    assert_eq!(infrastructure.series_count(), 20);
}

#[test]
fn bounded_allocation_repeated_windows_pressure_and_off_on() {
    // Run exactly this test alone before interpreting GLOBAL allocation peaks.
    // In parallel regression runs only structural bounds remain meaningful.
    let isolated = std::env::var_os("POSSUMS_TELEMETRY_ALLOCATION_TEST").is_some();
    let before = if isolated {
        crate::process_alloc_tests::ALLOCATOR.begin_phase()
    } else {
        crate::process_alloc_tests::ALLOCATOR.snapshot()
    };
    let metrics = new();
    let allocated = crate::process_alloc_tests::ALLOCATOR.snapshot();
    let base = allocated.live;
    for window in 0..12 {
        let boundary = (window * 600 + 300) * SECOND;
        drive(&metrics, boundary);
        metrics.poll();
        let handles: [_; 256] = std::array::from_fn(|_| metrics.http(Endpoint::Home));
        drop(metrics.http(Endpoint::Home));
        drop(handles);
        assert!(metrics.off());
        assert!(metrics.enable(Deployment::IsolatedSynthetic));
        drive(&metrics, (window * 600 + 600) * SECOND);
        metrics.poll();
        ten_starts(&metrics);
    }
    let after = crate::process_alloc_tests::ALLOCATOR.snapshot();
    if isolated {
        assert_eq!(after.live, base);
        assert_eq!(after.phase_peak, allocated.phase_peak);
        assert!(allocated.phase_peak - before.live <= 2 * 67_456 + 2 * 2048);
    }
    drop(metrics);
    if isolated {
        assert_eq!(
            crate::process_alloc_tests::ALLOCATOR.snapshot().live,
            before.live
        );
        println!("aggregation requested allocation delta={} bytes; record={} handle={} controller={} request={} infrastructure={}",allocated.live-before.live,std::mem::size_of::<Record>(),std::mem::size_of::<Observation<'_,FakeClock>>(),std::mem::size_of::<Metrics>(),std::mem::size_of::<RequestTables>(),std::mem::size_of::<Infrastructure>());
    }
}
