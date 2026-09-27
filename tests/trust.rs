use possums::attestation::{load_evidence, validate_evidence, EvidenceError, GatewayEvidence};
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

#[test]
fn gateway_evidence_requires_fresh_provenance_and_endpoint_binding() {
    let now = 1_000;
    let valid = GatewayEvidence {
        quote: serde_json::json!({"format": "v3"}),
        issued_at_unix: now,
        release_digest: "a".repeat(64),
        endpoint_key_sha256: "b".repeat(64),
        freshness_expires_at_unix: now + 1,
    };
    assert!(validate_evidence(valid.clone(), now).is_ok());

    for invalid in [
        GatewayEvidence {
            issued_at_unix: now - 301,
            ..valid.clone()
        },
        GatewayEvidence {
            release_digest: "not-a-digest".into(),
            ..valid.clone()
        },
        GatewayEvidence {
            endpoint_key_sha256: "C".repeat(64),
            ..valid.clone()
        },
        GatewayEvidence {
            freshness_expires_at_unix: now,
            ..valid
        },
    ] {
        assert!(matches!(
            validate_evidence(invalid, now),
            Err(EvidenceError::Invalid)
        ));
    }
}
