use possums::attestation::{load_evidence, EvidenceError};
use std::io::Write;

#[test]
fn missing_gateway_evidence_fails_closed() {
    assert!(matches!(
        load_evidence("/definitely/not/present"),
        Err(EvidenceError::Unavailable)
    ));
}

#[test]
fn malformed_gateway_evidence_fails_closed() {
    let path = std::env::temp_dir().join(format!("possums-evidence-{}", std::process::id()));
    let mut file = std::fs::File::create(&path).unwrap();
    file.write_all(b"not evidence").unwrap();
    assert!(matches!(load_evidence(&path), Err(EvidenceError::Invalid)));
    std::fs::remove_file(path).unwrap();
}
