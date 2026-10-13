use super::*;
use crate::telemetry::tests::extensions::{observe, qualify};

fn extension_released() -> Metrics {
    let m = new();
    ready(&m);
    qualify(&m);
    observe(&m);
    drive(&m, 600 * agg::SECOND);
    m.poll();
    m
}

#[tokio::test]
async fn extension_owned_attempt_is_once_only_even_during_outage() {
    if !clean_child("telemetry::export::handoff::tests::extensions::extension_owned_attempt_is_once_only_even_during_outage") { return; }
    for status in ["200 OK", "503 Unavailable", "307 Temporary Redirect"] {
        let m = extension_released();
        let (listener, client) = listener().await;
        let evidence = client.evidence();
        let permit = m.take_window().unwrap();
        let peer = async {
            let (mut stream, _) = listener.accept().await.unwrap();
            let length = capture_header(&mut stream, true).await;
            let mut bytes = vec![0; length];
            stream.read_exact(&mut bytes).await.unwrap();
            stream
                .write_all(
                    format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                        .as_bytes(),
                )
                .await
                .unwrap();
            bytes
        };
        let (sent, bytes) = tokio::time::timeout(Duration::from_secs(2), async {
            tokio::join!(send(permit, client, Pauses::default()), peer)
        })
        .await
        .unwrap();
        assert_eq!(sent.is_ok(), status == "200 OK");
        let p = oracle::points(&bytes);
        assert_eq!(p.len(), 4);
        assert!(p.keys().all(|(name, labels)| labels.len() == 1
            && (name == "possums.interface.errors" || name.starts_with("possums.tokens."))));
        assert_eq!(evidence.connections.load(SeqCst), 1);
        assert_eq!(evidence.live_io.load(SeqCst), 0);
        assert!(m.take_window().is_none());
        assert!(
            tokio::time::timeout(Duration::from_millis(20), listener.accept())
                .await
                .is_err()
        );
        assert!(m.off());
        assert!(m.take_window().is_none());
    }
}

#[tokio::test]
async fn extension_permit_invalidates_without_partial_flush_or_connection() {
    if !clean_child("telemetry::export::handoff::tests::extensions::extension_permit_invalidates_without_partial_flush_or_connection") { return; }
    for defect in 0..5 {
        let m = extension_released();
        let (_listener, client) = listener().await;
        let evidence = client.evidence();
        let (resume, paused) = tokio::sync::oneshot::channel();
        let mut sender = Box::pin(send(
            m.take_window().unwrap(),
            client,
            Pauses {
                before: Some(paused),
                ..Default::default()
            },
        ));
        assert!(futures_util::poll!(&mut sender).is_pending());
        match defect {
            0 => {
                m.invalidate();
            }
            1 => {
                m.clock.pair(601 * agg::SECOND, 603 * agg::SECOND);
                observe(&m);
            }
            2 | 3 => {
                let guard = m.state.lock().unwrap();
                if defect == 2 {
                    m.interface_error(crate::failures::InterfaceError::BodyLimit);
                } else {
                    m.observed_usage(QualifiedModel(0), 0, 0, 0, Some(0));
                }
                drop(guard);
            }
            _ => drive(&m, 602 * agg::SECOND),
        }
        resume.send(()).unwrap();
        assert!(sender.await.is_err());
        assert_eq!(evidence.connecting.load(SeqCst), 0);
        assert!(m.take_window().is_none());
        assert!(m.off());
    }
    let m = new();
    ready(&m);
    qualify(&m);
    observe(&m);
    assert!(m.off()); // shutdown discards the incomplete extension window
    assert!(m.take_window().is_none());
}
