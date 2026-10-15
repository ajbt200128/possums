use super::*;
use std::{
    pin::Pin,
    task::{Context, Poll, Waker},
    time::Duration,
};
use tokio::sync::oneshot;

async fn no_verification(_: Arc<u8>) -> Result<u8, InferenceError> {
    panic!("verification must not run for this acquisition")
}

fn pending<F: Future>(future: Pin<&mut F>) {
    assert!(matches!(
        future.poll(&mut Context::from_waker(Waker::noop())),
        Poll::Pending
    ));
}

#[tokio::test]
async fn healthy_acquisition_and_status_do_no_verification() {
    let clients = VerifiedClient::new(1);
    let client = clients.acquire(no_verification).await.unwrap();
    assert!(Arc::ptr_eq(&client, &clients.last_verified()));
    clients.invalidate(&client);
    // Evidence remains the actual previously verified snapshot, not dispatch permission.
    assert!(Arc::ptr_eq(&client, &clients.last_verified()));
}

#[tokio::test]
async fn replacement_preserves_admitted_snapshot_and_ignores_old_errors() {
    let clients = VerifiedClient::new(1);
    let admitted = clients.acquire(no_verification).await.unwrap();
    clients.invalidate(&admitted);
    let renewed = clients
        .acquire(|previous| async move {
            assert_eq!(*previous, 1);
            Ok(2)
        })
        .await
        .unwrap();
    assert_eq!(*admitted, 1);
    assert_eq!(*renewed, 2);
    assert!(!Arc::ptr_eq(&admitted, &renewed));
    clients.invalidate(&admitted);
    assert!(Arc::ptr_eq(
        &renewed,
        &clients.acquire(no_verification).await.unwrap()
    ));
}

#[tokio::test]
async fn successful_attempt_is_shared_without_serial_verification() {
    let clients = VerifiedClient::new(1);
    clients.invalidate(&clients.last_verified());
    let (release, gate) = oneshot::channel();
    let mut owner = Box::pin(clients.acquire(|_| async move {
        gate.await.unwrap();
        Ok(2)
    }));
    pending(owner.as_mut());
    let mut waiters = (0..32)
        .map(|_| Box::pin(clients.acquire(no_verification)))
        .collect::<Vec<_>>();
    for waiter in &mut waiters {
        pending(waiter.as_mut());
    }
    release.send(()).unwrap();
    let current = owner.await.unwrap();
    for waiter in waiters {
        assert!(Arc::ptr_eq(&current, &waiter.await.unwrap()));
    }
}

#[tokio::test]
async fn failed_attempt_is_shared_even_if_a_later_operation_succeeds() {
    let clients = VerifiedClient::new(1);
    let old = clients.last_verified();
    clients.invalidate(&old);
    let (release, gate) = oneshot::channel();
    let mut owner = Box::pin(clients.acquire(|_| async move {
        gate.await.unwrap();
        Err(InferenceFailure::VerificationFailed.into())
    }));
    pending(owner.as_mut());
    let mut waiters = (0..32)
        .map(|_| Box::pin(clients.acquire(no_verification)))
        .collect::<Vec<_>>();
    for waiter in &mut waiters {
        pending(waiter.as_mut());
    }
    release.send(()).unwrap();
    assert_eq!(
        owner.await.unwrap_err().failure(),
        InferenceFailure::VerificationFailed
    );
    assert!(Arc::ptr_eq(&old, &clients.last_verified()));
    let recovered = clients.acquire(|_| async { Ok(2) }).await.unwrap();
    for waiter in waiters {
        assert_eq!(
            waiter.await.unwrap_err().failure(),
            InferenceFailure::VerificationFailed
        );
    }
    assert!(Arc::ptr_eq(
        &recovered,
        &clients.acquire(no_verification).await.unwrap()
    ));
}

#[tokio::test]
async fn cancelled_owner_closes_only_its_attempt_and_never_falls_back() {
    let clients = VerifiedClient::new(1);
    let old = clients.last_verified();
    clients.invalidate(&old);
    let (release, gate) = oneshot::channel();
    let mut owner = Box::pin(clients.acquire(|_| async move {
        gate.await.unwrap();
        Ok(2)
    }));
    pending(owner.as_mut());
    let mut waiter = Box::pin(clients.acquire(no_verification));
    pending(waiter.as_mut());
    drop(owner);
    assert!(release.send(()).is_err());
    assert!(Arc::ptr_eq(&old, &clients.last_verified()));
    let recovered = clients.acquire(|_| async { Ok(3) }).await.unwrap();
    assert_eq!(
        waiter.await.unwrap_err().failure(),
        InferenceFailure::VerificationFailed
    );
    assert_eq!(*recovered, 3);
}

#[tokio::test]
async fn cancelling_a_waiter_does_not_cancel_verification() {
    let clients = VerifiedClient::new(1);
    clients.invalidate(&clients.last_verified());
    let (release, gate) = oneshot::channel();
    let mut owner = Box::pin(clients.acquire(|_| async move {
        gate.await.unwrap();
        Ok(2)
    }));
    pending(owner.as_mut());
    let mut waiter = Box::pin(clients.acquire(no_verification));
    pending(waiter.as_mut());
    drop(waiter);
    release.send(()).unwrap();
    assert_eq!(*owner.await.unwrap(), 2);
    assert_eq!(*clients.acquire(no_verification).await.unwrap(), 2);
}

#[tokio::test]
async fn a_new_connect_failure_after_publication_does_not_start_waiter_replay() {
    let clients = VerifiedClient::new(1);
    clients.invalidate(&clients.last_verified());
    let (release, gate) = oneshot::channel();
    let mut owner = Box::pin(clients.acquire(|_| async move {
        gate.await.unwrap();
        Ok(2)
    }));
    pending(owner.as_mut());
    let mut waiter = Box::pin(clients.acquire(no_verification));
    pending(waiter.as_mut());
    release.send(()).unwrap();
    let current = owner.await.unwrap();
    clients.invalidate(&current);
    assert_eq!(
        waiter.await.unwrap_err().failure(),
        InferenceFailure::VerificationFailed
    );
    assert_eq!(*clients.acquire(|_| async { Ok(3) }).await.unwrap(), 3);
}

#[tokio::test]
async fn sdk_certificate_rejection_is_a_supported_connect_failure() {
    use rustls::pki_types::PrivateKeyDer;
    let _ = rustls::crypto::ring::default_provider().install_default();
    let certified = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![certified.cert.der().clone()],
            PrivateKeyDer::Pkcs8(certified.key_pair.serialize_der().into()),
        )
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let peer = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        // The peer cannot receive an HTTP request after the client rejects TLS.
        assert!(tokio_rustls::TlsAcceptor::from(Arc::new(config))
            .accept(socket)
            .await
            .is_err());
    });
    let verifier = tinfoil::verifier::tls::PinnedCertVerifier::new("0".repeat(64)).unwrap();
    let tls = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    let client = reqwest::Client::builder()
        .no_proxy()
        .use_preconfigured_tls(tls)
        .build()
        .unwrap();
    let error = tokio::time::timeout(
        Duration::from_secs(5),
        client.get(format!("https://{address}/synthetic")).send(),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert!(error.is_connect());
    assert!(!error.is_timeout());
    peer.await.unwrap();
}
