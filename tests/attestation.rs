use possums::attestation::{load_evidence, EvidenceError};
use std::io::Write;

const NOW: u64 = 1_000;

fn envelope(quote: &str) -> String {
    envelope_at(quote, NOW, NOW + 1)
}

fn envelope_at(quote: &str, issued: u64, expires: u64) -> String {
    format!(
        r#"{{"quote":{quote},"issued_at_unix":{issued},"release_digest":"{}","endpoint_key_sha256":"{}","freshness_expires_at_unix":{expires}}}"#,
        "a".repeat(64),
        "b".repeat(64),
    )
}

struct TempFile(std::path::PathBuf);

impl TempFile {
    fn new(contents: &[u8]) -> Self {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "possums-attestation-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(contents).unwrap();
        Self(path)
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn file_evidence_rejects_wide_and_deep_quotes_before_deserialization() {
    // A small envelope with many tiny Values would otherwise allocate far more than its byte size.
    let wide = TempFile::new(envelope(&format!("[{}]", "[],".repeat(4_096) + "[]")).as_bytes());
    assert!(matches!(
        load_evidence(&wide.0),
        Err(EvidenceError::Invalid)
    ));

    let deep =
        TempFile::new(envelope(&format!("{}0{}", "[".repeat(65), "]".repeat(65))).as_bytes());
    assert!(matches!(
        load_evidence(&deep.0),
        Err(EvidenceError::Invalid)
    ));
}

#[test]
fn file_evidence_enforces_4096_lexical_node_boundary() {
    // Envelope: 1 root object + 5 keys + 4 scalar metadata values + 1 quote array.
    // Each array element adds one lexical node.
    let accepted = TempFile::new(envelope(&format!("[{}]", vec!["0"; 4_085].join(","))).as_bytes());
    assert!(load_evidence(&accepted.0).is_ok());

    let rejected = TempFile::new(envelope(&format!("[{}]", vec!["0"; 4_086].join(","))).as_bytes());
    assert!(matches!(
        load_evidence(&rejected.0),
        Err(EvidenceError::Invalid)
    ));
}

#[test]
fn file_evidence_accepts_normal_and_near_limit_quotes() {
    let normal = TempFile::new(envelope(r#"{"format":"v3"}"#).as_bytes());
    assert!(load_evidence(&normal.0).is_ok());

    // A full 1 MiB envelope with a low-cost scalar remains valid.
    let empty = envelope("\"\"");
    let large = TempFile::new(
        envelope(&format!("\"{}\"", "x".repeat(1024 * 1024 - empty.len()))).as_bytes(),
    );
    assert!(load_evidence(&large.0).is_ok());
}

#[test]
fn evidence_rejects_raw_value_reparse_before_building_quote_tree() {
    let nested = format!("[{}]", vec![r#"{"":0}"#; 35_000].join(","));
    let encoded = serde_json::to_string(&nested).unwrap();
    for key in [
        "$serde_json::private::RawValue",
        r"$serde_json::private::Ra\u0077Value",
    ] {
        let quote = format!(r#"{{"{key}":{encoded}}}"#);
        let document = envelope(&quote);
        assert!(document.len() < 1024 * 1024);
        let file = TempFile::new(document.as_bytes());
        assert!(matches!(
            load_evidence(&file.0),
            Err(EvidenceError::Invalid)
        ));
    }
    let normal = TempFile::new(envelope(r#"{"RawValue":"[]"}"#).as_bytes());
    assert!(load_evidence(&normal.0).is_ok());
}

#[cfg(unix)]
mod helper {
    use super::*;
    use possums::attestation::{EvidenceVerifier, TinfoilEvidenceVerifier};
    use std::os::unix::fs::PermissionsExt;

    fn script(body: &str) -> TempFile {
        let file = TempFile::new(format!("#!/bin/sh\n{body}\n").as_bytes());
        let mut permissions = std::fs::metadata(&file.0).unwrap().permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&file.0, permissions).unwrap();
        file
    }

    async fn verify(
        file: &TempFile,
    ) -> Result<possums::attestation::GatewayEvidence, EvidenceError> {
        TinfoilEvidenceVerifier::new(file.0.to_str().unwrap(), "fixture")
            .verify("fixture", NOW)
            .await
    }

    #[tokio::test]
    async fn helper_accepts_evidence_issued_after_request_started() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let file = script(&format!(
            "sleep 1; printf '%s' '{}'",
            envelope_at(r#"{"format":"v3"}"#, now + 1, now + 301)
        ));
        // The supplied request-start sample predates issuance; only a
        // validation-time sample after the helper exits can accept it.
        assert!(verify(&file).await.is_ok());
    }

    #[tokio::test]
    async fn helper_still_rejects_genuinely_future_issuance() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let file = script(&format!(
            "printf '%s' '{}'",
            envelope_at(r#"{"format":"v3"}"#, now + 60, now + 360)
        ));
        assert!(matches!(verify(&file).await, Err(EvidenceError::Invalid)));
    }

    #[tokio::test]
    async fn helper_rejects_failed_status_without_exposing_stderr() {
        let file = script(&format!(
            "printf '%s' '{}'; echo private >&2; exit 1",
            envelope(r#"{"format":"v3"}"#)
        ));
        assert!(matches!(verify(&file).await, Err(EvidenceError::Invalid)));
    }

    #[tokio::test]
    async fn helper_rejects_oversized_stdout() {
        let file = script("printf '%1048577s' x");
        assert!(matches!(verify(&file).await, Err(EvidenceError::Invalid)));
    }

    #[tokio::test]
    async fn helper_rejects_wide_quote() {
        let file = script(&format!(
            "printf '%s' '{}'",
            envelope(&format!("[{}]", "[],".repeat(4_096) + "[]"))
        ));
        assert!(matches!(verify(&file).await, Err(EvidenceError::Invalid)));
    }

    #[tokio::test(start_paused = true)]
    async fn helper_times_out() {
        let file = script("exec sleep 60");
        let task = tokio::spawn(async move { verify(&file).await });
        tokio::task::yield_now().await;
        tokio::time::advance(std::time::Duration::from_secs(46)).await;
        assert!(matches!(
            task.await.unwrap(),
            Err(EvidenceError::Unavailable)
        ));
    }
}
