use super::*;
use crate::telemetry::{
    tests::{drive, new},
    Deployment, Endpoint, Lane, Window,
};

fn reading(base: Instant, seconds: f64, cpu: Option<f64>, rss: Option<usize>) -> Reading {
    Reading {
        at: base + Duration::from_secs_f64(seconds),
        cpu: cpu.map(Duration::from_secs_f64),
        rss,
    }
}

#[test]
fn cpu_cores_use_actual_elapsed_time_and_never_a_capacity_divisor() {
    let base = Instant::now();
    let mut sampler = Sampler::new(crate::web::admission_capacities());
    assert_eq!(
        sampler.read(0, || reading(base, 0., Some(3.), Some(4096))),
        Some((0, None, Some(4096.)))
    );
    // Ten-second grid, but 10.2 seconds actually elapsed; 20.4 CPU seconds
    // represent two cores, not 204%, nor a machine-normalized fraction.
    assert_eq!(
        sampler.read(INTERVAL + SECOND / 5, || reading(
            base,
            10.2,
            Some(23.4),
            Some(8192)
        )),
        Some((INTERVAL, Some(2.), Some(8192.)))
    );
    assert!(sampler
        .read(INTERVAL + SECOND / 2, || panic!("duplicate read"))
        .is_none());
    assert_eq!(
        sampler.read(2 * INTERVAL, || reading(base, 20., Some(23.4), None)),
        Some((2 * INTERVAL, Some(0.), None))
    );
}

#[test]
fn missed_failed_regressed_and_zero_elapsed_readings_do_not_backfill() {
    let base = Instant::now();
    for defect in 0..6 {
        let mut sampler = Sampler::new(crate::web::admission_capacities());
        sampler.read(0, || reading(base, 0., Some(10.), Some(1)));
        let point = match defect {
            0 => sampler.read(INTERVAL + SECOND + 1, || panic!("late source read")),
            1 => sampler.read(2 * INTERVAL, || reading(base, 20., Some(12.), None)),
            2 => sampler.read(INTERVAL, || reading(base, 10., None, None)),
            3 => sampler.read(INTERVAL, || reading(base, 10., Some(9.), None)),
            4 => sampler.read(INTERVAL, || reading(base, 0., Some(12.), None)),
            _ => sampler.read(INTERVAL, || Reading {
                at: base - Duration::from_secs(1),
                cpu: Some(Duration::from_secs(12)),
                rss: None,
            }),
        };
        assert!(point.is_none_or(|(_, cpu, _)| cpu.is_none()));
        let next = if defect == 1 { 3 } else { 2 };
        let (_, cpu, _) = sampler
            .read(next * INTERVAL, || {
                reading(base, next as f64 * 10., Some(14.), Some(1))
            })
            .unwrap();
        assert_eq!(cpu.is_some(), !matches!(defect, 0 | 2));
        assert!(sampler.read(0, || panic!("regressed grid read")).is_none());
        assert!(sampler.previous.is_none());
    }
}

#[test]
fn jittered_batch_survives_request_observations_at_minute_close() {
    let metrics = new();
    assert!(metrics.enable(Deployment::Production));
    let base = Instant::now();
    let mut sampler = Sampler::new(crate::web::admission_capacities());
    drive(&metrics, 300 * SECOND);
    // Baseline at the beginning of the first complete minute.
    sampler.sample_with(&metrics, || reading(base, 300., Some(30.), Some(8192)));
    metrics.poll();
    for i in 1..=6 {
        let end = (300 + i * 10) * SECOND;
        drive(&metrics, end - SECOND);
        let jitter = i * 100_000_000;
        metrics.clock.set(end + jitter);
        // The HTTP observer wins the ordering race, including at minute end.
        std::thread::scope(|scope| {
            scope
                .spawn(|| drop(metrics.http(Endpoint::Home)))
                .join()
                .unwrap();
        });
        assert!(metrics
            .state
            .lock()
            .unwrap()
            .infrastructure
            .pending
            .is_none());
        sampler.sample_with(&metrics, || {
            // Also simulate a request while the source reader is outside the lock.
            drop(metrics.http(Endpoint::Home));
            let elapsed = (end + jitter) as f64 / SECOND as f64;
            reading(base, elapsed, Some(elapsed / 10.), Some(8192))
        });
        sampler.sample_with(&metrics, || panic!("duplicate tick read"));
        metrics.poll();
    }
    let view = metrics.infrastructure().unwrap();
    assert_eq!(
        view.window(),
        Window {
            start_ns: 300 * SECOND,
            end_ns: 360 * SECOND
        }
    );
    let table = view.tables().unwrap();
    assert!(
        (table
            .resource(ResourceScope::Process, ResourceMetric::CpuUsed)
            .unwrap()
            - 0.1)
            .abs()
            < 1e-9
    );
    assert_eq!(
        table.resource(ResourceScope::Process, ResourceMetric::MemoryUsed),
        Some(8192.)
    );
    assert_eq!(table.series_count(), 8);
    let encoded = crate::telemetry::export::encoded_infrastructure(table, view.window());
    let points = &encoded.resource_metrics[0].scope_metrics[0].metrics;
    assert_eq!(points.len(), 3);
    let gauge = points
        .iter()
        .find(|metric| metric.name == "possums.admission.capacity")
        .unwrap();
    assert_eq!(gauge.unit, "{permit}");
    let Some(opentelemetry_proto::tonic::metrics::v1::metric::Data::Gauge(gauge)) = &gauge.data
    else {
        panic!("capacity must be a gauge")
    };
    assert_eq!(gauge.data_points.len(), 6);
    for ((lane, capacity), point) in crate::web::admission_capacities()
        .into_iter()
        .zip(&gauge.data_points)
    {
        assert_eq!(point.start_time_unix_nano, 0);
        assert_eq!(point.time_unix_nano, 360 * SECOND);
        assert_eq!(
            point.value,
            Some(
                opentelemetry_proto::tonic::metrics::v1::number_data_point::Value::AsDouble(
                    capacity as f64
                )
            )
        );
        let attrs: Vec<_> = point
            .attributes
            .iter()
            .map(|a| (a.key.as_str(), &a.value.as_ref().unwrap().value))
            .collect();
        use opentelemetry_proto::tonic::common::v1::any_value::Value::StringValue;
        let name = match lane {
            Lane::Connection => "connection",
            Lane::Generation => "generation",
            Lane::Heavy => "heavy",
            Lane::Ingress => "ingress",
            Lane::NewChat => "new_chat",
            Lane::Control => "control",
        };
        assert_eq!(
            attrs,
            vec![
                ("source", &Some(StringValue("configuration".into()))),
                ("scope", &Some(StringValue("gateway".into()))),
                ("lane", &Some(StringValue(name.into()))),
            ]
        );
    }
    for scope in [
        ResourceScope::Process,
        ResourceScope::Cgroup,
        ResourceScope::TinfoilEnclave,
        ResourceScope::TinfoilWorkload,
    ] {
        assert_eq!(table.resource(scope, ResourceMetric::CpuCapacity), None);
        assert_eq!(table.resource(scope, ResourceMetric::MemoryCapacity), None);
    }
    for (lane, capacity) in crate::web::admission_capacities() {
        assert_eq!(table.capacity(lane), Some(capacity as f64));
    }
    drop(view);
    assert!(metrics.infrastructure().is_none());
}

#[test]
fn incomplete_or_failed_minute_is_unavailable_and_next_full_minute_recovers() {
    for defect in 0..4 {
        let metrics = new();
        let base = Instant::now();
        let mut sampler = Sampler::new(crate::web::admission_capacities());
        drive(&metrics, 300 * SECOND);
        sampler.sample_with(&metrics, || reading(base, 300., Some(30.), Some(4096)));
        metrics.poll();
        for i in 1..=12 {
            let end = (300 + i * 10) * SECOND;
            drive(&metrics, end - SECOND);
            let delay = if defect == 0 && i == 6 {
                SECOND + 1
            } else {
                SECOND / 10
            };
            metrics.clock.set(end + delay);
            if !(defect == 1 && i == 3) {
                sampler.sample_with(&metrics, || {
                    if defect == 3 && i == 6 {
                        // Read began on time but finished outside the grace.
                        metrics.clock.set(end + SECOND + 1);
                    }
                    reading(
                        base,
                        (end + delay) as f64 / SECOND as f64,
                        if defect == 2 && i == 3 {
                            None
                        } else {
                            Some(i as f64 + 30.)
                        },
                        Some(4096),
                    )
                });
            }
            metrics.poll();
            if i == 6 {
                let view = metrics.infrastructure();
                if defect == 2 {
                    let view = view.unwrap();
                    assert_eq!(view.tables().unwrap().series_count(), 7);
                    assert_eq!(
                        view.tables()
                            .unwrap()
                            .resource(ResourceScope::Process, ResourceMetric::CpuUsed),
                        None
                    );
                } else {
                    assert!(view.is_none());
                }
            }
        }
        let view = metrics.infrastructure().unwrap();
        let table = view.tables().unwrap();
        assert_eq!(
            table.resource(ResourceScope::Process, ResourceMetric::MemoryUsed),
            Some(4096.)
        );
        // Missing final CPU baseline also makes the following CPU minute unavailable.
        assert_eq!(
            table
                .resource(ResourceScope::Process, ResourceMetric::CpuUsed)
                .is_some(),
            matches!(defect, 1 | 2)
        );
        drop(view);
        for i in 13..=18 {
            let end = (300 + i * 10) * SECOND;
            drive(&metrics, end - SECOND);
            // The inclusive one-second grace is still on time.
            metrics.clock.set(end + SECOND);
            sampler.sample_with(&metrics, || {
                reading(
                    base,
                    (end + SECOND) as f64 / SECOND as f64,
                    Some(i as f64 + 30.),
                    Some(4096),
                )
            });
            metrics.poll();
        }
        assert_eq!(
            metrics
                .infrastructure()
                .unwrap()
                .tables()
                .unwrap()
                .series_count(),
            8
        );
    }
}

#[test]
fn configured_capacity_is_independent_of_idle_traffic_and_failed_resource_readings() {
    for (cpu, rss) in [(None, None), (Some(30.), None), (None, Some(4096))] {
        let metrics = new();
        assert!(metrics.enable(Deployment::Production));
        let base = Instant::now();
        let mut sampler = Sampler::new(crate::web::admission_capacities());
        drive(&metrics, 300 * SECOND);
        sampler.sample_with(&metrics, || reading(base, 300., Some(30.), Some(4096)));
        metrics.poll();
        for i in 1..=6 {
            let end = (300 + i * 10) * SECOND;
            drive(&metrics, end - SECOND);
            metrics.clock.set(end);
            sampler.sample_with(&metrics, || reading(base, (300 + i * 10) as f64, cpu, rss));
            metrics.poll();
        }
        let view = metrics.infrastructure().unwrap();
        let table = view.tables().unwrap();
        assert_eq!(
            table.series_count(),
            6 + usize::from(rss.is_some()) + usize::from(cpu.is_some())
        );
        for (lane, capacity) in crate::web::admission_capacities() {
            assert_eq!(table.capacity(lane), Some(capacity as f64));
        }
        assert!(metrics.request().is_none());
    }
}

#[test]
fn mismatched_source_capacity_is_rejected_without_affecting_other_lanes() {
    let metrics = new();
    assert!(metrics.enable(Deployment::Production));
    let base = Instant::now();
    let mut capacities = crate::web::admission_capacities();
    capacities[0].1 += 1;
    let mut sampler = Sampler::new(capacities);
    drive(&metrics, 300 * SECOND);
    sampler.sample_with(&metrics, || reading(base, 300., None, None));
    metrics.poll();
    for i in 1..=6 {
        let end = (300 + i * 10) * SECOND;
        drive(&metrics, end - SECOND);
        metrics.clock.set(end);
        sampler.sample_with(&metrics, || {
            reading(base, (300 + i * 10) as f64, None, None)
        });
        metrics.poll();
    }
    let view = metrics.infrastructure().unwrap();
    let table = view.tables().unwrap();
    assert_eq!(table.capacity(Lane::Connection), None);
    assert_eq!(table.series_count(), 5);
    for (lane, capacity) in crate::web::admission_capacities().into_iter().skip(1) {
        assert_eq!(table.capacity(lane), Some(capacity as f64));
    }
}

#[test]
fn restart_discards_partial_capacity_minute_and_requires_fresh_full_window() {
    let base = Instant::now();
    {
        let metrics = new();
        assert!(metrics.enable(Deployment::Production));
        let mut sampler = Sampler::new(crate::web::admission_capacities());
        for seconds in [310, 320, 330] {
            drive(&metrics, seconds * SECOND - SECOND);
            metrics.clock.set(seconds * SECOND);
            sampler.sample_with(&metrics, || reading(base, seconds as f64, None, None));
            metrics.poll();
        }
        assert!(metrics.infrastructure().is_none());
    }
    let metrics = new();
    drive(&metrics, 330 * SECOND);
    assert!(metrics.enable(Deployment::Production));
    let mut sampler = Sampler::new(crate::web::admission_capacities());
    for seconds in (340..=420).step_by(10) {
        drive(&metrics, seconds * SECOND - SECOND);
        metrics.clock.set(seconds * SECOND);
        sampler.sample_with(&metrics, || reading(base, seconds as f64, None, None));
        metrics.poll();
        if seconds < 420 {
            assert!(metrics.infrastructure().is_none());
        }
    }
    let view = metrics.infrastructure().unwrap();
    assert_eq!(
        view.window(),
        Window {
            start_ns: 360 * SECOND,
            end_ns: 420 * SECOND
        }
    );
    let table = view.tables().unwrap();
    assert_eq!(table.series_count(), 6);
    for (lane, capacity) in crate::web::admission_capacities() {
        assert_eq!(table.capacity(lane), Some(capacity as f64));
    }
}

#[test]
fn disabled_and_nonisolated_modes_do_not_read_sources() {
    for mode in [Deployment::Off, Deployment::NonIsolated] {
        let metrics = new();
        assert!(metrics.enable(mode));
        Sampler::new(crate::web::admission_capacities())
            .sample_with(&metrics, || panic!("closed release gate"));
        assert!(metrics.infrastructure().is_none());
    }
    assert!(!crate::telemetry::runtime::Config::from_lookup(|_| None).enabled());
}

#[test]
fn local_process_source_smoke_without_raw_output() {
    let first = Reading::now();
    let second = Reading::now();
    assert!(second.at >= first.at);
    assert!(first.cpu.is_some());
    assert!(second.cpu >= first.cpu);
    assert!(first.rss.is_some_and(|rss| rss > 0));
    assert!(second.rss.is_some_and(|rss| rss > 0));
}
