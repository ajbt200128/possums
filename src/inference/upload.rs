//! Request DATA owns admission, not just the request future or outer Body.
use axum::body::Bytes;
use std::sync::Arc;
use tokio::sync::oneshot;

// Field drop order matters: signal only AFTER the serialized allocation and its
// lease are gone. Bytes clones/slices (including Hyper's write queue) share this
// owner. Never derive Debug: bytes contain prompt plaintext.
struct Upload {
    bytes: Vec<u8>,
    _heavy: Option<Arc<crate::telemetry::hooks::Lease>>,
    _released: oneshot::Sender<()>,
}

impl AsRef<[u8]> for Upload {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

pub(super) struct UploadReleased(oneshot::Receiver<()>);

impl UploadReleased {
    pub(super) async fn wait(self) {
        // Sender is deliberately never sent: its destruction is the receipt.
        let _ = self.0.await;
    }
}

// None is reserved for the private, fixed-input funded diagnostic, never routes.
pub(super) fn body(
    bytes: Vec<u8>,
    heavy: Option<Arc<crate::telemetry::hooks::Lease>>,
) -> (reqwest::Body, UploadReleased) {
    let (released, receipt) = oneshot::channel();
    let bytes = Bytes::from_owner(Upload {
        bytes,
        _heavy: heavy,
        _released: released,
    });
    // Keep streaming/non-cloneable request semantics: owning Bytes alone would
    // allow reqwest to replay a prompt on redirects or retries.
    (
        reqwest::Body::wrap(reqwest::Body::from(bytes)),
        UploadReleased(receipt),
    )
}
