// Diagnostic feature only; synthetic evidence is never accepted by the real verifier.
use super::*;

fn diagnostic_config() -> TorRuntimeConfig {
    TorRuntimeConfig::from_lookup(|key| match key {
        "POSSUMS_TOR_ENABLED" => Ok("true".into()),
        "POSSUMS_TOR_EXPECTED_ONION" => Ok(
            "5jfgyy7ctrjavpxvkb5rglwf7gkuo5vox27hxescd3vgsfcg2iwfmxqd.onion".into(),
        ),
        _ => panic!("diagnostic must not request private identity or directory"),
    })
    .unwrap()
    .unwrap()
}

struct SyntheticVerifier {
    expired: bool,
}
#[async_trait::async_trait]
impl EvidenceVerifier for SyntheticVerifier {
    async fn verify(
        &self,
        _: &str,
        _: u64,
    ) -> Result<crate::attestation::GatewayEvidence, crate::attestation::EvidenceError> {
        panic!("diagnostic must use fresh Tor appraisal")
    }
    async fn verify_tor(
        &self,
        _: &str,
        _: &[u8; 32],
    ) -> Result<TorGatewayEvidence, crate::attestation::TorEvidenceError> {
        let now = tor_now().unwrap();
        Ok(TorGatewayEvidence {
            quote: serde_json::json!({"synthetic_not_authentic": true}),
            issued_at_unix: now,
            release_digest: "ab".repeat(32),
            endpoint_key_sha256: "cd".repeat(32),
            freshness_expires_at_unix: if self.expired { now } else { now + 30 },
            tor_endorsement_key: "ef".repeat(32),
        })
    }
}

#[tokio::test]
async fn diagnostic_never_reads_private_identity_and_stops_before_grants_even_on_success() {
    let failure = PreparedTor::prepare(
        diagnostic_config(),
        &SyntheticVerifier { expired: false },
        "unused",
    )
    .await
    .err()
    .unwrap();
    // Actual grant paths are not fixtures and cannot be read by this test.
    // Reaching either read would produce GrantRead, not DiagnosticStop.
    assert_eq!(failure, TorStartupFailure::DiagnosticStop);
}

#[tokio::test]
async fn diagnostic_preserves_failed_appraisal_and_freshness_gates() {
    assert_eq!(
        PreparedTor::prepare(
            diagnostic_config(),
            &crate::attestation::UnavailableEvidenceVerifier,
            "unused",
        )
        .await
        .err(),
        Some(TorStartupFailure::Appraisal(
            crate::attestation::TorEvidenceError::Unsupported
        ))
    );
    assert_eq!(
        PreparedTor::prepare(
            diagnostic_config(),
            &SyntheticVerifier { expired: true },
            "unused",
        )
        .await
        .err(),
        Some(TorStartupFailure::Appraisal(
            crate::attestation::TorEvidenceError::Expired
        ))
    );
}
