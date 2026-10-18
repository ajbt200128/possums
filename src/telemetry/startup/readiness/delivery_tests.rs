use super::*;
use crate::telemetry::startup::tests::{capture, text};
use futures_util::FutureExt;
use tokio::io::AsyncWriteExt;

#[tokio::test]
async fn configuration_failure_warns_once_and_off_does_not_warn() {
    for failed in [false, true] {
        let (observer, receiver) = tests::fixture();
        observer.failure(Terminal::StatusEnded);
        let client = if failed {
            Err(transport::Failure::Configuration)
        } else {
            Ok(None)
        };
        run(observer.clone(), receiver, client).await;
        let local = observer.local.records.lock().unwrap();
        assert_eq!(local.len(), if failed { 2 } else { 1 });
        assert!(local[0].contains("code=tor_startup_status_ended"));
        if failed {
            assert!(local[1].contains(
                "stage=tor_startup_readiness_delivery constraint=transport_configuration"
            ));
        }
    }
}

#[tokio::test(start_paused = true)]
async fn terminal_before_waiting_poll_never_emits_a_late_waiting_record() {
    let (observer, receiver) = tests::fixture();
    tokio::time::advance(WAIT).await;
    observer.failure(Terminal::StatusEnded);
    run(observer.clone(), receiver, Ok(None)).await;
    let local = observer.local.records.lock().unwrap();
    assert_eq!(local.len(), 1);
    assert!(!local[0].contains("outcome=waiting"));
}

#[cfg(not(feature = "debug-tor-no-health"))]
#[tokio::test(start_paused = true)]
async fn late_waiting_upload_cannot_extend_the_180_second_observation_budget() {
    let (mut observer, receiver) = tests::fixture();
    observer.start -= Duration::from_millis(179_900);
    let end = observer.start + END;
    let client = tests::stalled();
    let evidence = client.evidence();
    let worker = run(observer.clone(), receiver, Ok(Some(client)));
    tokio::pin!(worker);
    assert!(worker.as_mut().now_or_never().is_none());
    tokio::time::advance(Duration::from_millis(100)).await;
    assert_eq!(Instant::now(), end);
    assert!(worker.as_mut().now_or_never().is_none());
    assert_eq!(
        evidence
            .connecting
            .load(std::sync::atomic::Ordering::SeqCst),
        2
    );
    tokio::time::advance(Duration::from_secs(1)).await;
    worker.await;
    assert_eq!(Instant::now(), end + Duration::from_secs(1));
    let local = observer.local.records.lock().unwrap();
    assert_eq!(local.len(), 3);
    assert_eq!(
        local
            .iter()
            .filter(|r| r.contains("outcome=waiting"))
            .count(),
        1
    );
    assert_eq!(
        local
            .iter()
            .filter(|r| r.contains("outcome=unconfirmed"))
            .count(),
        1
    );
}

#[tokio::test]
async fn hostile_wire_failures_warn_once_locally_and_preserve_first_terminal() {
    use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsPartialSuccess;
    const HOSTILE: &str = "HOSTILE_SECRET_URL_ACCOUNT_ONION";
    let partial = ExportLogsServiceResponse {
        partial_success: Some(ExportLogsPartialSuccess {
            rejected_log_records: 0,
            error_message: HOSTILE.into(),
        }),
    }
    .encode_to_vec();
    for (status, body, constraint) in [
        (503, HOSTILE.as_bytes().to_vec(), "http_response_rejected"),
        (200, HOSTILE.as_bytes().to_vec(), "otlp_response_invalid"),
        (200, partial, "otlp_partial_success_or_warning"),
    ] {
        let (listener, client, acceptor) =
            crate::telemetry::runtime::tests::fixture(true, "api.honeycomb.io").await;
        let evidence = client.evidence();
        let (mut observer, receiver) = tests::fixture();
        observer.start -= WAIT;
        let peer = async {
            for index in 0..2 {
                let (io, _) = listener.accept().await.unwrap();
                let mut io = acceptor.accept(io).await.unwrap();
                let bytes = capture(&mut io, "/v1/logs").await;
                let message = ExportLogsServiceRequest::decode(bytes.as_slice()).unwrap();
                let records = &message.resource_logs[0].scope_logs[0].log_records;
                assert_eq!(records.len(), 1);
                let record = &records[0];
                assert_eq!(record.event_name, EVENT);
                assert_eq!(record.attributes.len(), 7);
                assert!(!text(record.body.as_ref().unwrap()).contains("delivery_unconfirmed"));
                assert!(!String::from_utf8_lossy(&bytes).contains(HOSTILE));
                assert!(!String::from_utf8_lossy(&bytes).contains("synthetic-key-canary"));
                if index == 0 {
                    observer.observe(
                        OnionState::Running,
                        ProblemKind::None,
                        Some(Terminal::FirstLocalReady),
                    );
                    observer.failure(Terminal::StatusPanicked);
                }
                let (status, body) = if index == 0 {
                    (status, body.as_slice())
                } else {
                    (401, HOSTILE.as_bytes())
                };
                let header = format!("HTTP/1.1 {status} Synthetic\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                io.write_all(header.as_bytes()).await.unwrap();
                let _ = io.write_all(body).await;
            }
        };
        tokio::join!(run(observer.clone(), receiver, Ok(Some(client))), peer);
        let local = observer.local.records.lock().unwrap();
        assert_eq!(local.len(), 3); // two readiness records, one warning
        assert!(local[0].contains("outcome=waiting"));
        assert!(local[1].contains("outcome=first_local_ready"));
        assert!(local[2].contains(&format!("constraint={constraint} ")));
        assert!(local[2].contains("stage=tor_startup_readiness_delivery"));
        assert!(!local[2].contains("status=")); // sender does not retain HTTP status
        assert!(local.iter().all(|r| !r.contains(HOSTILE)));
        assert_eq!(
            evidence
                .connections
                .load(std::sync::atomic::Ordering::SeqCst),
            2
        );
        assert_eq!(
            evidence.live_io.load(std::sync::atomic::Ordering::SeqCst),
            0
        );
    }
}

#[tokio::test]
async fn actual_transport_close_keeps_closed_cause_unknown_warning() {
    let (listener, client, _acceptor) =
        crate::telemetry::runtime::tests::fixture(true, "api.honeycomb.io").await;
    let (observer, receiver) = tests::fixture();
    observer.failure(Terminal::StatusEnded);
    let peer = async {
        let (io, _) = listener.accept().await.unwrap();
        drop(io); // synthetic transport outage, no upstream text to classify
    };
    tokio::join!(run(observer.clone(), receiver, Ok(Some(client))), peer);
    let local = observer.local.records.lock().unwrap();
    assert_eq!(local.len(), 2);
    assert!(local[0].contains("code=tor_startup_status_ended"));
    assert!(local[1].contains("constraint=transport_cause_unknown"));
    assert!(!local[1].contains("status="));
}

#[tokio::test(start_paused = true)]
async fn stalled_outage_warns_once_without_erasing_primary_terminal() {
    let (observer, receiver) = tests::fixture();
    observer.failure(Terminal::StatusEnded);
    let client = tests::stalled();
    let evidence = client.evidence();
    let worker = run(observer.clone(), receiver, Ok(Some(client)));
    tokio::pin!(worker);
    assert!(worker.as_mut().now_or_never().is_none());
    tokio::time::advance(Duration::from_secs(1)).await;
    worker.await;
    let local = observer.local.records.lock().unwrap();
    assert_eq!(local.len(), 2);
    assert!(local[0].contains("code=tor_startup_status_ended"));
    assert!(local[1].contains("delivery_deadline"));
    assert_eq!(
        evidence
            .connecting
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
}
