use super::super::{
    capture_header, clean_child, drain_request, materialize::tests as oracle, phase,
};
use super::*;
use crate::telemetry::{
    self as agg,
    tests::{cohort, drive, new, ready, Metrics, Vector},
    *,
};
use opentelemetry_http::HttpClient;
use std::{sync::atomic::Ordering::SeqCst, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

async fn listener() -> (TcpListener, Client) {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let client = Client::new(listener.local_addr().unwrap()).unwrap();
    (listener, client)
}
fn infrastructure(m: &Metrics) {
    for i in 1..=6 {
        let end = (540 + i * 10) * agg::SECOND;
        drive(m, end);
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
                m.resource(end, scope, metric, 1.);
            }
        }
        for (lane, value) in [
            Lane::Connection,
            Lane::Generation,
            Lane::Heavy,
            Lane::Ingress,
            Lane::NewChat,
            Lane::Control,
        ]
        .into_iter()
        .zip([64, 4, 4, 4, 1, 1])
        {
            m.configured_capacity(end, lane, value);
        }
    }
}
fn released() -> Metrics {
    let m = new();
    ready(&m);
    cohort(&m, 10, Vector::Plain);
    m.poll();
    m
}
async fn captured(m: &Metrics) -> Vec<u8> {
    let (listener, client) = listener().await;
    let permit = m.take_window().unwrap();
    let peer = async {
        let (mut stream, _) = listener.accept().await.unwrap();
        let length = capture_header(&mut stream, true).await;
        assert!(length <= super::super::transport::MAX_OUTBOUND);
        let mut body = vec![0; length];
        stream.read_exact(&mut body).await.unwrap();
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
            .await
            .unwrap();
        body
    };
    let (sent, body) = tokio::join!(send(permit, client, Pauses::default()), peer);
    assert_eq!(sent, Ok(()));
    body
}

#[tokio::test]
async fn real_aggregation_wire_and_suppression() {
    if !clean_child("telemetry::export::handoff::tests::real_aggregation_wire_and_suppression") {
        return;
    }
    for (n, vector) in [
        (10, Vector::Plain),
        (11, Vector::Plain),
        (20, Vector::Mixed),
        (20, Vector::MissingRelease),
        (10, Vector::MissingAll),
    ] {
        let m = new();
        ready(&m);
        cohort(&m, n, vector);
        let bytes = captured(&m).await;
        assert_eq!(
            oracle::points(&bytes),
            oracle::expected(
                n as i64,
                vector == Vector::Mixed,
                if vector == Vector::MissingRelease || vector == Vector::MissingAll {
                    10
                } else {
                    0
                }
            )
        );
        assert!(m.take_window().is_none());
    }
    let (listener, client) = listener().await;
    for (n, vector) in [
        (0, Vector::Plain),
        (1, Vector::Plain),
        (9, Vector::Plain),
        (11, Vector::RareError),
        (11, Vector::RareModel),
        (11, Vector::RareEndpoint),
        (11, Vector::RareBin),
        (11, Vector::MissingRare),
        (10, Vector::Duplicate),
    ] {
        let m = new();
        ready(&m);
        cohort(&m, n, vector);
        assert!(m.take_window().is_none());
    }
    assert_eq!(client.evidence().connecting.load(SeqCst), 0);
    assert!(
        tokio::time::timeout(Duration::from_millis(10), listener.accept())
            .await
            .is_err()
    );
    let m = new();
    ready(&m);
    infrastructure(&m);
    let bytes = captured(&m).await;
    let decoded = oracle::decode(&bytes);
    let mut points = 0;
    for metric in &decoded.resource_metrics[0].scope_metrics[0].metrics {
        let Some(super::super::Data::Gauge(gauge)) = &metric.data else {
            panic!("gauge only")
        };
        for p in &gauge.data_points {
            assert_eq!(p.start_time_unix_nano, 0);
            assert_eq!(p.time_unix_nano, 600 * agg::SECOND);
            let expected = if metric.name == "possums.admission.capacity" {
                let lane = p.attributes.iter().find(|a| a.key == "lane").unwrap();
                let value = lane.value.as_ref().unwrap().value.as_ref().unwrap();
                use opentelemetry_proto::tonic::common::v1::any_value::Value::StringValue;
                match value {
                    StringValue(v) if v == "connection" => 64.,
                    StringValue(v) if v == "new_chat" || v == "control" => 1.,
                    _ => 4.,
                }
            } else {
                1.
            };
            assert_eq!(p.value, Some(super::super::Value::AsDouble(expected)));
            points += 1;
        }
    }
    assert_eq!(points, 20);
}

#[tokio::test]
async fn sole_permit_expiry_watermark_and_stale_return() {
    if !clean_child(
        "telemetry::export::handoff::tests::sole_permit_expiry_watermark_and_stale_return",
    ) {
        return;
    }
    let m = released();
    let permit = m.take_window().unwrap();
    assert!(permit.valid());
    assert!(m.take_window().is_none());
    assert!(m.request().is_none());
    let ptr = match permit.table.as_ref().unwrap() {
        agg::handoff::Table::Request(t) => &**t as *const RequestTables,
        _ => unreachable!(),
    };
    // No state lock is held; eligible observations still change the active table.
    for _ in 0..10 {
        let mut h = m.http(Endpoint::Home);
        h.finish_http(
            Status::Success,
            HttpTerminal::Eof,
            Disposition::ControlOrOther,
        );
    }
    assert_eq!(m.state.lock().unwrap().requests.active.http_starts[0], 10);
    assert!(!m.off());
    assert!(!permit.valid());
    assert!(!m.enable(Deployment::IsolatedSynthetic)); // no false kill acknowledgement
    drop(permit);
    assert!(m.stop_export().await);
    assert!(m.enable(Deployment::IsolatedSynthetic));
    {
        let state = m.state.lock().unwrap();
        assert_eq!(
            &**state.requests.frozen.as_ref().unwrap() as *const RequestTables,
            ptr
        );
        assert!(state.requests.pending.is_none());
        assert_eq!(state.requests.attempted, 600 * agg::SECOND);
    }
    assert!(m.take_window().is_none());
    for delay in [2 * agg::SECOND - 1, 2 * agg::SECOND] {
        let m = released();
        let p = m.take_window().unwrap();
        drive(&m, 600 * agg::SECOND + delay);
        assert_eq!(p.valid(), delay < 2 * agg::SECOND);
        drop(p);
        assert!(m.take_window().is_none());
    }
    let m = released();
    let p = m.take_window().unwrap();
    m.state.lock().unwrap().requests.attempted += agg::REQUEST_WINDOW;
    assert!(!p.valid());
    drop(p);
    let m = released();
    let p = m.take_window().unwrap();
    drop(p);
    assert!(m.take_window().is_none()); // discard, not retry
    assert!(m.off());
    let restarted = new();
    assert!(restarted.take_window().is_none());
    let m = released();
    m.state.lock().unwrap().requests.attempted += agg::REQUEST_WINDOW;
    assert!(m.take_window().is_none()); // watermark checked at claim as well
    let m = released();
    drive(&m, 602 * agg::SECOND);
    assert!(m.take_window().is_none()); // unsent expiry at claim
}

#[tokio::test]
async fn kill_materialized_and_dropped_sender() {
    if !clean_child("telemetry::export::handoff::tests::kill_materialized_and_dropped_sender") {
        return;
    }
    for at in 0..3 {
        let m = released();
        let (_listener, client) = listener().await;
        let stale = client.clone();
        let evidence = client.evidence();
        let (tx, rx) = tokio::sync::oneshot::channel();
        let pauses = if at == 0 {
            Pauses {
                before: Some(rx),
                ..Default::default()
            }
        } else {
            Pauses {
                materialized: Some(rx),
                ..Default::default()
            }
        };
        let mut sender = Box::pin(send(m.take_window().unwrap(), client, pauses));
        if at != 2 {
            assert!(futures_util::poll!(&mut sender).is_pending());
        }
        assert!(!m.off());
        if at == 2 {
            drop(sender);
            assert!(m.stop_export().await);
        } else {
            let (sent, ack) = tokio::join!(&mut sender, m.stop_export());
            assert!(sent.is_err());
            assert!(ack);
            drop(sender);
        }
        drop(tx);
        assert_eq!(evidence.connecting.load(SeqCst), 0);
        assert_eq!(evidence.live_io.load(SeqCst), 0);
        assert!(stale
            .send_bytes(super::super::request(&stale, hyper::body::Bytes::new()))
            .await
            .is_err());
        assert!(m.take_window().is_none());
    }
    // A live, unpolled permit cannot be falsely acknowledged at the 1s bound.
    let m = released();
    let p = m.take_window().unwrap();
    assert!(!m.stop_export().await);
    assert!(!m.enable(Deployment::IsolatedSynthetic));
    drop(p);
    assert!(m.stop_export().await);
}

#[tokio::test]
async fn owned_io_kill_clock_contention_and_skip() {
    if !clean_child("telemetry::export::handoff::tests::owned_io_kill_clock_contention_and_skip") {
        return;
    }
    // Real body boundary for off, clock fault, contention and skipped window;
    // connect uses the existing deterministic pending-connect seam, not SYN proof.
    for mode in 0..5 {
        let m = released();
        let (listener, mut client) = listener().await;
        client.stall_connect = mode == 4;
        let stale = client.clone();
        let evidence = client.evidence();
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        let peer = async {
            let mut stream = None;
            if mode == 4 {
                while evidence.connecting.load(SeqCst) == 0 {
                    tokio::task::yield_now().await;
                }
            } else {
                let (mut s, _) = listener.accept().await.unwrap();
                drain_request(&mut s).await;
                s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nx")
                    .await
                    .unwrap();
                while evidence.received.load(SeqCst) == 0 {
                    tokio::task::yield_now().await;
                }
                stream = Some(s);
            }
            ready_tx.send(()).unwrap();
            let _ = done_rx.await;
            drop(stream);
        };
        let control = async {
            ready_rx.await.unwrap();
            // New eligible observations progress while the network is stalled.
            let mut h = m.http(Endpoint::Home);
            h.finish_http(
                Status::Success,
                HttpTerminal::Eof,
                Disposition::ControlOrOther,
            );
            drop(h);
            assert_eq!(m.state.lock().unwrap().requests.active.http_starts[0], 1);
            match mode {
                1 => {
                    m.clock.pair(600 * agg::SECOND, 602 * agg::SECOND);
                    m.poll();
                }
                2 => {
                    let guard = m.state.lock().unwrap();
                    m.poll();
                    drop(guard);
                }
                3 => {
                    m.clock.set(901 * agg::SECOND);
                    m.poll();
                }
                _ => {
                    assert!(!m.off());
                }
            }
            let start = tokio::time::Instant::now();
            assert!(m.stop_export().await);
            assert!(start.elapsed() <= ATTEMPT_TIMEOUT);
            assert_eq!(evidence.live_io.load(SeqCst), 0);
            let writes = evidence.written.load(SeqCst);
            let polls = evidence.write_polls.load(SeqCst);
            let connects = evidence.connecting.load(SeqCst);
            for _ in 0..8 {
                assert!(stale
                    .send_bytes(super::super::request(&stale, hyper::body::Bytes::new()))
                    .await
                    .is_err());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
            assert_eq!(evidence.written.load(SeqCst), writes);
            assert_eq!(evidence.write_polls.load(SeqCst), polls);
            assert_eq!(evidence.connecting.load(SeqCst), connects);
            done_tx.send(()).unwrap();
        };
        let sender = send(m.take_window().unwrap(), client, Pauses::default());
        let (sent, (), ()) = tokio::time::timeout(Duration::from_secs(3), async {
            tokio::join!(sender, peer, control)
        })
        .await
        .unwrap();
        assert!(sent.is_err());
        assert!(m.take_window().is_none());
    }
}

#[tokio::test]
async fn failure_timeout_refusal_and_no_partial_flush() {
    if !clean_child(
        "telemetry::export::handoff::tests::failure_timeout_refusal_and_no_partial_flush",
    ) {
        return;
    }
    for status in [200, 500, 0] {
        let m = released();
        let (listener, client) = listener().await;
        let evidence = client.evidence();
        let peer = async {
            let (mut stream, _) = listener.accept().await.unwrap();
            drain_request(&mut stream).await;
            if status != 0 {
                super::super::reply(&mut stream, super::super::Wire::Length, 0, status)
                    .await
                    .unwrap();
            } else {
                tokio::time::sleep(Duration::from_millis(1100)).await;
            }
        };
        let (sent, ()) = tokio::join!(
            send(m.take_window().unwrap(), client, Pauses::default()),
            peer
        );
        assert_eq!(sent.is_ok(), status == 200);
        assert!(m.take_window().is_none());
        assert_eq!(evidence.connections.load(SeqCst), 1);
        assert_eq!(evidence.live_io.load(SeqCst), 0);
        assert!(m.off());
        assert!(m.enable(Deployment::IsolatedSynthetic));
        assert!(m.take_window().is_none());
    }
    let m = released();
    let (listener, client) = listener().await;
    drop(listener);
    assert!(send(m.take_window().unwrap(), client, Pauses::default())
        .await
        .is_err());
    assert!(m.take_window().is_none());
    let m = new();
    ready(&m);
    for _ in 0..10 {
        let mut h = m.http(Endpoint::Home);
        h.finish_http(
            Status::Success,
            HttpTerminal::Eof,
            Disposition::ControlOrOther,
        );
    }
    assert!(m.off());
    assert!(m.take_window().is_none());
}

#[tokio::test]
async fn composed_allocation_co_closing_pressure_cancel() {
    if !clean_child(
        "telemetry::export::handoff::tests::composed_allocation_co_closing_pressure_cancel",
    ) {
        return;
    }
    let before = phase(); // BEFORE controller, tables, pool, client, SDK or payload
                          // Charge the inline lifecycle pool/controller too, not just their tables.
    let m = Box::new(new());
    ready(&m);
    infrastructure(&m);
    // Typed-table stress only: same accepted fixture, not reachable lane usage.
    {
        let mut state = m.state.lock().unwrap();
        oracle::fill_maximum(&mut state.requests.active);
    }
    m.poll();
    let mut pool = Vec::with_capacity(agg::POOL);
    for _ in 0..agg::POOL {
        pool.push(m.http(Endpoint::Home));
    }
    let (listener, client) = listener().await;
    let evidence = client.evidence();
    let permit = m.take_window().unwrap();
    assert!(m.take_window().is_none());
    {
        let state = m.state.lock().unwrap();
        assert!(state.requests.frozen.is_none());
        assert!(state.infrastructure.pending.is_some());
        assert_eq!(
            state.infrastructure.frozen.as_ref().unwrap().series_count(),
            20
        );
    }
    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    let peer = async {
        let (mut stream, _) = listener.accept().await.unwrap();
        let length = capture_header(&mut stream, true).await;
        assert_eq!(length, 737_926); // honest production source label adds six bytes
        let mut body = vec![0; length];
        stream.read_exact(&mut body).await.unwrap();
        // Keep the captured body, active/frozen tables, full pool, SDK data and
        // client's retained serialized body live at this overlapping boundary.
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nx")
            .await
            .unwrap();
        while evidence.received.load(SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
        let snapshot = crate::process_alloc_tests::ALLOCATOR.snapshot();
        if std::env::var_os("POSSUMS_TELEMETRY_ALLOCATION_TEST").is_some() {
            let peak = snapshot.phase_peak.saturating_sub(before.live);
            eprintln!("handoff allocation baseline={} sampled_peak={} incremental={} live_overlap={} wire={length}",before.live,snapshot.phase_peak,peak,snapshot.live);
            assert!(peak <= 32 * 1024 * 1024);
            // Stronger sampled check: ALL composed requested allocations here
            // fit under even one component ceiling; no per-component subtraction.
            assert!(peak <= super::super::LIBRARY_BUDGET);
        }
        // Existing frozen box unavailable: repeated closes cannot allocate or
        // replace it; co-closing infrastructure remains in its sole slot.
        {
            let mut state = m.state.lock().unwrap();
            let epoch = state.epoch;
            for i in 3..10 {
                state.requests.freeze(epoch, i * agg::REQUEST_WINDOW, true);
                assert!(state.requests.frozen.is_none());
                assert!(state.requests.pending.is_none());
            }
        }
        // Pool overflow is a real nonblocking invalidation, not send completion.
        drop(m.http(Endpoint::Home));
        assert!(m.stop_export().await);
        assert_eq!(evidence.live_io.load(SeqCst), 0);
        let _ = done_tx.send(());
        body
    };
    let sender = async {
        let result = send(permit, client, Pauses::default()).await;
        assert!(result.is_err());
        let _ = done_rx.await;
    };
    let ((), body) =
        tokio::time::timeout(Duration::from_secs(3), async { tokio::join!(sender, peer) })
            .await
            .unwrap();
    assert_eq!(oracle::decode(&body).resource_metrics.len(), 1);
    drop(body);
    drop(pool);
    assert!(m.off());
    assert!(m.take_window().is_none());
    let after = crate::process_alloc_tests::ALLOCATOR.snapshot();
    if std::env::var_os("POSSUMS_TELEMETRY_ALLOCATION_TEST").is_some() {
        eprintln!(
            "handoff allocation final_sampled_peak={} incremental={} controller={}",
            after.phase_peak,
            after.phase_peak.saturating_sub(before.live),
            std::mem::size_of::<Metrics>()
        );
        assert!(after.phase_peak.saturating_sub(before.live) <= 32 * 1024 * 1024);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn kill_inside_sdk_materialization_joins_owner() {
    if !clean_child(
        "telemetry::export::handoff::tests::kill_inside_sdk_materialization_joins_owner",
    ) {
        return;
    }
    let m = std::sync::Arc::new(released());
    let (_listener, client) = listener().await;
    let evidence = client.evidence();
    let (entered, waiting) = tokio::sync::oneshot::channel();
    let (resume, receiver) = std::sync::mpsc::sync_channel(1);
    let task_owner = m.clone();
    let mut tasks = tokio::task::JoinSet::new();
    tasks.spawn(async move {
        send(
            task_owner.take_window().unwrap(),
            client,
            Pauses {
                during: Some(materialize::Checkpoint {
                    entered,
                    resume: receiver,
                }),
                ..Default::default()
            },
        )
        .await
    });
    waiting.await.unwrap(); // first three SDK metrics already live; construction not done
    assert!(m.state.try_lock().is_ok()); // no aggregation lock in the serializer
    assert!(!m.off());
    assert!(m.handoff.busy.load(SeqCst));
    let start = tokio::time::Instant::now();
    resume.send(()).unwrap();
    assert!(m.stop_export().await);
    assert!(start.elapsed() <= ATTEMPT_TIMEOUT);
    assert!(tasks.join_next().await.unwrap().unwrap().is_err());
    assert!(tasks.is_empty());
    assert_eq!(evidence.connecting.load(SeqCst), 0);
    assert_eq!(evidence.live_io.load(SeqCst), 0);
}

#[tokio::test]
async fn co_closing_sequential_handoff_and_upload_cancel() {
    if !clean_child(
        "telemetry::export::handoff::tests::co_closing_sequential_handoff_and_upload_cancel",
    ) {
        return;
    }
    let m = new();
    ready(&m);
    agg::tests::cohort_before_close(&m, 10, Vector::Plain);
    infrastructure(&m);
    assert_eq!(
        oracle::points(&captured(&m).await),
        oracle::expected(10, false, 0)
    );
    assert!(m.state.lock().unwrap().infrastructure.pending.is_some());
    let bytes = captured(&m).await;
    assert_eq!(
        oracle::decode(&bytes).resource_metrics[0].scope_metrics[0]
            .metrics
            .len(),
        5
    );
    assert!(m.take_window().is_none());
    // Actual SDK body, not the old opaque maximum buffer. Peer accepts but
    // does not drain, observing the candidate's real AsyncWrite boundary.
    let m = new();
    ready(&m);
    drive(&m, 600 * agg::SECOND);
    {
        let mut s = m.state.lock().unwrap();
        oracle::fill_maximum(&mut s.requests.active);
    }
    m.poll();
    // Kernel defaults can absorb this entire upload without the peer reading.
    // Bound both real socket buffers so cancellation observes actual backpressure.
    let socket = tokio::net::TcpSocket::new_v4().unwrap();
    socket.set_recv_buffer_size(4096).unwrap();
    socket
        .bind((std::net::Ipv4Addr::LOCALHOST, 0).into())
        .unwrap();
    let listener = socket.listen(1).unwrap();
    let mut client = Client::new(listener.local_addr().unwrap()).unwrap();
    client.send_buffer_size = Some(4096);
    let evidence = client.evidence();
    let (done, wait) = tokio::sync::oneshot::channel();
    let peer = async {
        let (stream, _) = listener.accept().await.unwrap();
        while evidence.pending_writes.load(SeqCst) == 0 || evidence.written.load(SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
        assert!(evidence.written.load(SeqCst) < 737_926);
        assert!(m.stop_export().await);
        assert_eq!(evidence.live_io.load(SeqCst), 0);
        done.send(()).unwrap();
        drop(stream);
    };
    let sender = async {
        assert!(send(m.take_window().unwrap(), client, Pauses::default())
            .await
            .is_err());
        let _ = wait.await;
    };
    tokio::time::timeout(Duration::from_secs(3), async { tokio::join!(sender, peer) })
        .await
        .unwrap();
}

#[tokio::test]
async fn dropped_io_sender_and_old_return_after_skip() {
    if !clean_child(
        "telemetry::export::handoff::tests::dropped_io_sender_and_old_return_after_skip",
    ) {
        return;
    }
    let m = std::sync::Arc::new(released());
    let (listener, client) = listener().await;
    let evidence = client.evidence();
    let stale = client.clone();
    let mut tasks = tokio::task::JoinSet::new();
    let worker = m.clone();
    tasks
        .spawn(async move { send(worker.take_window().unwrap(), client, Pauses::default()).await });
    let (mut stream, _) = listener.accept().await.unwrap();
    drain_request(&mut stream).await;
    assert_eq!(evidence.live_io.load(SeqCst), 1);
    assert!(m.handoff.busy.load(SeqCst));
    tasks.abort_all();
    assert!(tasks.join_next().await.unwrap().unwrap_err().is_cancelled());
    assert!(tasks.is_empty());
    assert!(m.stop_export().await);
    assert_eq!(evidence.live_io.load(SeqCst), 0);
    let writes = evidence.write_polls.load(SeqCst);
    assert!(stale
        .send_bytes(super::super::request(&stale, hyper::body::Bytes::new()))
        .await
        .is_err());
    assert_eq!(evidence.write_polls.load(SeqCst), writes);
    assert!(m.enable(Deployment::IsolatedSynthetic));
    assert!(m.take_window().is_none());
    // Automatic scheduling recovery can advance an epoch before the old owner
    // returns. Its box return must not restore metadata or clear newer infra.
    let m = released();
    let old = m.take_window().unwrap();
    let old_epoch = old.epoch;
    m.clock.set(901 * agg::SECOND);
    m.poll();
    for i in 1..=6 {
        let end = (960 + 10 * i) * agg::SECOND;
        drive(&m, end);
        m.resource(end, ResourceScope::Cgroup, ResourceMetric::CpuUsed, 1.);
    }
    m.poll();
    assert!(!old.valid());
    assert!(m.take_window().is_none());
    drop(old);
    let next = m.take_window().unwrap();
    assert!(next.epoch > old_epoch);
    assert_eq!(
        next.window,
        Window {
            start_ns: 960 * agg::SECOND,
            end_ns: 1020 * agg::SECOND
        }
    );
    assert!(matches!(
        next.table,
        Some(agg::handoff::Table::Infrastructure(_))
    ));
    assert!(next.valid());
    drop(next);
    assert!(m.take_window().is_none());
    // Expiry between SDK construction and transport must result in no connect.
    let m = released();
    let (_listener, client) = self::listener().await;
    let evidence = client.evidence();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut sender = Box::pin(send(
        m.take_window().unwrap(),
        client,
        Pauses {
            materialized: Some(rx),
            ..Default::default()
        },
    ));
    assert!(futures_util::poll!(&mut sender).is_pending());
    drive(&m, 602 * agg::SECOND);
    tx.send(()).unwrap();
    assert!(sender.await.is_err());
    assert_eq!(evidence.connecting.load(SeqCst), 0);
    assert!(m.take_window().is_none());
}

#[tokio::test]
async fn composed_tls_allocation_and_owned_cancel() {
    if !clean_child("telemetry::export::handoff::tests::composed_tls_allocation_and_owned_cancel") {
        return;
    }
    let before = phase();
    let m = Box::new(new());
    ready(&m);
    infrastructure(&m);
    oracle::fill_maximum(&mut m.state.lock().unwrap().requests.active);
    m.poll();
    let mut pool = Vec::with_capacity(agg::POOL);
    for _ in 0..agg::POOL {
        pool.push(m.http(Endpoint::Home));
    }
    let (listener, client, acceptor) = agg::runtime::tests::fixture(true, "api.honeycomb.io").await;
    let evidence = client.evidence();
    let permit = m.take_window().unwrap();
    let peer = async {
        let (stream, _) = listener.accept().await.unwrap();
        let mut stream = acceptor.accept(stream).await.unwrap();
        let body = agg::runtime::tests::capture(&mut stream).await;
        assert_eq!(body.len(), 737_926);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 65536\r\n\r\n")
            .await
            .unwrap();
        stream.write_all(&[b'x'; 65535]).await.unwrap();
        while evidence.received.load(SeqCst) < 65535 {
            tokio::task::yield_now().await;
        }
        // Keep all tables, pool, SDK, serialized body, bounded response and both
        // local TLS endpoints live. Certificate/key generation is also charged.
        let snapshot = crate::process_alloc_tests::ALLOCATOR.snapshot();
        if std::env::var_os("POSSUMS_TELEMETRY_ALLOCATION_TEST").is_some() {
            let peak = snapshot.phase_peak.saturating_sub(before.live);
            eprintln!("handoff allocation TLS baseline={} sampled_peak={} incremental={} overlap={} wire={}", before.live, snapshot.phase_peak, peak, snapshot.live, body.len());
            assert!(peak <= 32 * 1024 * 1024);
            assert!(peak <= super::super::LIBRARY_BUDGET);
        }
        assert!(m.stop_export().await);
        assert_eq!(evidence.live_io.load(SeqCst), 0);
        body
    };
    let sender = send(permit, client, Pauses::default());
    let (result, body) =
        tokio::time::timeout(Duration::from_secs(3), async { tokio::join!(sender, peer) })
            .await
            .unwrap();
    assert!(result.is_err());
    assert_eq!(oracle::decode(&body).resource_metrics.len(), 1);
    assert!(m.off());
    drop(pool);
    if std::env::var_os("POSSUMS_TELEMETRY_ALLOCATION_TEST").is_some() {
        let peak = crate::process_alloc_tests::ALLOCATOR
            .snapshot()
            .phase_peak
            .saturating_sub(before.live);
        assert!(peak <= super::super::LIBRARY_BUDGET);
    }
}
