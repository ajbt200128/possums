use super::*;
fn evidence(now: u64) -> TorGatewayEvidence {
    TorGatewayEvidence {
        quote: serde_json::json!({"synthetic_not_authentic":true}),
        issued_at_unix: now,
        release_digest: "ab".repeat(32),
        endpoint_key_sha256: "cd".repeat(32),
        freshness_expires_at_unix: now + 30,
        tor_endorsement_key: "ef".repeat(32),
    }
}
#[test]
fn bounded_opt_in_wire_preserves_only_exact_closed_failure_categories() {
    let now = 1_000;
    let raw = serde_json::to_vec(&serde_json::json!({"evidence":evidence(now)})).unwrap();
    assert_eq!(
        parse_tor_response(&raw, now)
            .unwrap()
            .current_key(now)
            .unwrap(),
        [0xef; 32]
    );
    assert_eq!(
        parse_tor_response(&raw, now + 30).err(),
        Some(TorEvidenceError::Expired)
    );
    let raw = br#"{"failure":{"code":"possums_v3_keys_tor_endorsement_spki","stage":"keys","constraint":"tor_endorsement_spki"}}"#;
    assert_eq!(
        parse_tor_response(raw, now).err(),
        Some(TorEvidenceError::Verification(
            TorVerificationFailure::TorSpki
        ))
    );
    for raw in [
        br#"{"failure":{"code":"HOSTILE_SECRET","stage":"keys","constraint":"tor_endorsement_spki"}}"#.as_slice(),
        br#"{"failure":{"code":"possums_v3_keys_tor_endorsement_spki","stage":"HOSTILE_SECRET","constraint":"tor_endorsement_spki"}}"#,
        br#"{"failure":{"code":"possums_v3_keys_tor_endorsement_spki","stage":"keys","constraint":"tor_endorsement_spki","cause":"HOSTILE_SECRET"}}"#,
        br#"{"evidence":null,"failure":null}"#,
        br#"{"evidence":{"HOSTILE_SECRET":true},"failure":null}"#,
    ] {
        let error = parse_tor_response(raw, now).err().unwrap();
        assert_eq!(error, TorEvidenceError::Invalid);
        assert!(!format!("{error:?} {error}").contains("HOSTILE_SECRET"));
    }
    let both = serde_json::json!({"evidence":evidence(now), "failure":{"code":"possums_v3_input_nonce","stage":"input","constraint":"nonce"}});
    assert_eq!(
        parse_tor_response(&serde_json::to_vec(&both).unwrap(), now).err(),
        Some(TorEvidenceError::Invalid)
    );
    assert_eq!(
        parse_tor_response(&vec![b' '; MAX_EVIDENCE_BYTES + 1], now).err(),
        Some(TorEvidenceError::Invalid)
    );
    let mut malformed = evidence(now);
    malformed.tor_endorsement_key = "HOSTILE_SECRET".into();
    assert_eq!(
        malformed.current_key(now).err(),
        Some(TorEvidenceError::Invalid)
    );
}

#[test]
fn tor_acquisition_status_is_numeric_optional_and_only_for_acquisition() {
    for status in [100, 503, 999] {
        let raw = serde_json::json!({"failure": {
            "code": "possums_v3_acquisition_http_status", "stage": "acquisition",
            "constraint": "http_status", "status": status,
        }});
        assert_eq!(
            parse_tor_response(&serde_json::to_vec(&raw).unwrap(), 1000).err(),
            Some(TorEvidenceError::AcquisitionStatus(status))
        );
    }
    for status in [
        serde_json::Value::Null,
        serde_json::json!("503"),
        serde_json::json!(99),
        serde_json::json!(1000),
        serde_json::json!(503.5),
        serde_json::json!(true),
        serde_json::json!({"HOSTILE_SECRET": 503}),
    ] {
        let raw = serde_json::json!({"failure": {
            "code": "possums_v3_acquisition_http_status", "stage": "acquisition",
            "constraint": "http_status", "status": status,
        }});
        assert_eq!(
            parse_tor_response(&serde_json::to_vec(&raw).unwrap(), 1000).err(),
            Some(TorEvidenceError::Invalid)
        );
    }
    for (code, stage, constraint) in [
        (
            "possums_v3_acquisition_http_status",
            "HOSTILE_SECRET",
            "http_status",
        ),
        ("HOSTILE_SECRET", "acquisition", "http_status"),
        (
            "possums_v3_keys_tor_endorsement_spki",
            "keys",
            "tor_endorsement_spki",
        ),
    ] {
        let raw = serde_json::json!({"failure": {"code":code, "stage":stage, "constraint":constraint, "status":503}});
        let error = parse_tor_response(&serde_json::to_vec(&raw).unwrap(), 1000)
            .err()
            .unwrap();
        assert_eq!(error, TorEvidenceError::Invalid);
        assert!(!format!("{error:?} {error}").contains("HOSTILE_SECRET"));
    }
    let raw = br#"{"failure":{"code":"possums_v3_acquisition_http_status","stage":"acquisition","constraint":"http_status"}}"#;
    assert_eq!(
        parse_tor_response(raw, 1000).err(),
        Some(TorEvidenceError::Verification(
            TorVerificationFailure::AcquisitionStatus
        ))
    );
}

#[cfg(unix)]
#[tokio::test]
async fn actual_helper_owner_uses_raw_stdin_bounds_and_suppresses_hostile_output() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = tempfile::tempdir().unwrap();
    let helper = scratch.path().join("helper");
    // Shell fixture checks actual stdin count. No network or Go-authentication
    // success is fabricated; only closed negative RPC transport is qualified.
    let write_helper = |body: &str| {
        std::fs::write(
            &helper,
            format!("#!/bin/sh\nn=$(wc -c | tr -d ' ')\n[ \"$n\" = 32 ] || exit 1\n{body}\n"),
        )
        .unwrap();
    };
    write_helper("printf 'HOSTILE_SECRET' >&2\nprintf '%s' '{\"failure\":{\"code\":\"possums_v3_envelope_rejected\",\"stage\":\"envelope\",\"constraint\":\"rejected\"}}'");
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
    let verifier = TinfoilEvidenceVerifier::new(helper.to_string_lossy(), "synthetic");
    let failure = verifier
        .verify_tor("unused", &[0x23; 32])
        .await
        .err()
        .unwrap();
    assert_eq!(
        failure,
        TorEvidenceError::Verification(TorVerificationFailure::EnvelopeRejected)
    );
    assert!(!format!("{failure:?} {failure}").contains("HOSTILE"));
    write_helper("printf 'HOSTILE_SECRET' >&2\nprintf '%s' '{\"failure\":{\"code\":\"possums_v3_acquisition_http_status\",\"stage\":\"acquisition\",\"constraint\":\"http_status\",\"status\":503}}'");
    assert_eq!(
        verifier.verify_tor("unused", &[0; 32]).await.err(),
        Some(TorEvidenceError::AcquisitionStatus(503))
    );
    write_helper("printf '%s' 'HOSTILE_SECRET'");
    assert_eq!(
        verifier.verify_tor("unused", &[0; 32]).await.err(),
        Some(TorEvidenceError::Invalid)
    );
    write_helper("head -c 1048577 /dev/zero");
    assert_eq!(
        verifier.verify_tor("unused", &[0; 32]).await.err(),
        Some(TorEvidenceError::Invalid)
    );
    write_helper("printf 'HOSTILE_SECRET' >&2\nexit 1");
    assert_eq!(
        verifier.verify_tor("unused", &[0; 32]).await.err(),
        Some(TorEvidenceError::Unavailable)
    );
}
