// Actual Axum router, synthetic SDK facts only: NOT authentic-chain acceptance.
use super::*;
use crate::{
    attestation::{TorEvidenceError, TorGatewayEvidence, TorVerificationFailure},
    tor_endorsement::TorEndorsementSigner,
};

struct TorProbe {
    facts: Mutex<Result<TorGatewayEvidence, TorEvidenceError>>,
    challenges: Mutex<Vec<[u8; 32]>>,
    expire_after_wait: bool,
}
#[async_trait]
impl EvidenceVerifier for TorProbe {
    async fn verify(&self, _: &str, _: u64) -> Result<GatewayEvidence, EvidenceError> {
        unreachable!()
    }
    async fn verify_tor(
        &self,
        _: &str,
        nonce: &[u8; 32],
    ) -> Result<TorGatewayEvidence, TorEvidenceError> {
        self.challenges.lock().unwrap().push(*nonce);
        let mut result = self.facts.lock().unwrap().clone();
        if self.expire_after_wait {
            tokio::task::yield_now().await;
            result.as_mut().unwrap().freshness_expires_at_unix =
                crate::attestation::tor_now().unwrap();
        }
        result
    }
}
fn tor_fixture() -> (AppState, Arc<Probe>, Arc<TorProbe>) {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/fixtures/tor_grant_rfc8032.json")).unwrap();
    let private = rustls::pki_types::PrivatePkcs8KeyDer::from_pem_slice(
        fixture["private_key_pem"].as_str().unwrap().as_bytes(),
    )
    .unwrap();
    let signer =
        Arc::new(TorEndorsementSigner::from_pkcs8(private.secret_pkcs8_der(), [0x42; 32]).unwrap());
    let now = crate::attestation::tor_now().unwrap();
    let tor = Arc::new(TorProbe {
        facts: Mutex::new(Ok(TorGatewayEvidence {
            quote: json!({"synthetic_sdk_facts_not_an_authenticated_quote": true}),
            issued_at_unix: now,
            release_digest: "ab".repeat(32),
            endpoint_key_sha256: "cd".repeat(32),
            freshness_expires_at_unix: now + 60,
            tor_endorsement_key: fixture["application_key"].as_str().unwrap().into(),
        })),
        challenges: Mutex::default(),
        expire_after_wait: false,
    });
    let (mut state, inference, _) = fixture_state();
    state.gateway_evidence_verifier = tor.clone();
    (state.with_tor_endorsement(signer), inference, tor)
}
fn fixture_state() -> (AppState, Arc<Probe>, String) {
    fixture()
}
use rustls::pki_types::pem::PemObject;

#[tokio::test]
async fn public_challenge_only_endpoint_and_canonical_node_fixture() {
    let (state, inference, tor) = tor_fixture();
    let response = router(state)
        .oneshot(
            Request::get(format!("/attestation/tor?nonce={}", "23".repeat(32)))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let value: serde_json::Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/fixtures/tor_grant_rfc8032.json")).unwrap();
    assert_eq!(value.as_object().unwrap().len(), 3);
    let mut payload = b"possums/tor-onion-endorsement\0".to_vec();
    payload.push(1);
    payload.extend([0x23; 32]);
    payload.extend([0x42; 32]);
    assert_eq!(value["payload"], crate::server::tor_hex(&payload));
    assert_eq!(value["signature"], fixture["signature"]);
    assert_eq!(*tor.challenges.lock().unwrap(), vec![[0x23; 32]]);
    for counter in [
        &inference.catalog_calls,
        &inference.tokenize_calls,
        &inference.calls,
        &inference.document_calls,
    ] {
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn rejects_arbitrary_target_unknown_query_body_and_bad_challenges_without_appraisal() {
    let (state, _, tor) = tor_fixture();
    for query in [
        "".into(),
        "nonce=ab".into(),
        format!("nonce={}&b={}", "23".repeat(32), "42".repeat(32)),
        format!("nonce={}&nonce={}", "23".repeat(32), "23".repeat(32)),
        format!("nonce={}", "xx".repeat(32)),
    ] {
        let response = router(state.clone())
            .oneshot(
                Request::get(format!("/attestation/tor?{query}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let value: serde_json::Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(value["error"]["code"], "tor_nonce_refused");
        assert_eq!(value["error"]["stage"], "input");
        assert_eq!(value["error"]["constraint"], "challenge_get_empty_body");
        assert!(!value.to_string().contains("nonce="));
        node_decode_failures(
            &[value],
            &[
                json!({"code":"tor_nonce_refused", "stage":"input", "constraint":"challenge_get_empty_body"}),
            ],
        );
    }
    let path = format!("/attestation/tor?nonce={}", "23".repeat(32));
    for method in [Method::GET, Method::HEAD] {
        let response = router(state.clone())
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(&path)
                    .body(Body::from("HOSTILE_CALLER_B"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    assert!(tor.challenges.lock().unwrap().is_empty());
    let (disabled, _, _) = fixture();
    let response = router(disabled)
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn current_boot_key_and_async_expiry_fail_closed_with_closed_diagnostics() {
    for mode in 0..3 {
        let (mut state, inference, tor) = tor_fixture();
        match mode {
            0 => {
                tor.facts
                    .lock()
                    .unwrap()
                    .as_mut()
                    .unwrap()
                    .tor_endorsement_key = "ab".repeat(32)
            }
            1 => {
                state.gateway_evidence_verifier = Arc::new(TorProbe {
                    facts: Mutex::new(tor.facts.lock().unwrap().clone()),
                    challenges: Mutex::default(),
                    expire_after_wait: true,
                });
            }
            _ => {
                *tor.facts.lock().unwrap() = Err(TorEvidenceError::Verification(
                    TorVerificationFailure::TorSpki,
                ))
            }
        }
        let response = router(state)
            .oneshot(
                Request::get(format!("/attestation/tor?nonce={}", "23".repeat(32)))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let text = String::from_utf8(
            response
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .to_vec(),
        )
        .unwrap();
        assert!(text.contains(
            [
                "tor_grant_mismatch",
                "tor_appraisal_expired",
                "possums_v3_keys_tor_endorsement_spki"
            ][mode]
        ));
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 1);
        assert!(value["error"]["action"].as_str().unwrap().len() > 10);
        assert!(value.get("payload").is_none());
        let (code, stage, constraint) = [
            (
                "tor_grant_mismatch",
                "grant",
                "derived_mounted_appraised_A_equality",
            ),
            ("tor_appraisal_expired", "freshness", "current_evidence"),
            (
                "possums_v3_keys_tor_endorsement_spki",
                "keys",
                "tor_endorsement_spki",
            ),
        ][mode];
        assert_eq!(value["error"]["code"], code);
        assert_eq!(value["error"]["stage"], stage);
        assert_eq!(value["error"]["constraint"], constraint);
        if mode == 0 {
            assert!(!text.contains("no service launched"));
            assert!(text.contains("no endorsement or inference"));
            assert!(crate::tor_runtime::TorStartupFailure::GrantMismatch
                .to_string()
                .contains("no service launched"));
        }
        node_decode_failures(
            &[value],
            &[json!({"code":code,"stage":stage,"constraint":constraint})],
        );
        assert!(!text.contains("synthetic_sdk_facts"));
        assert!(!text.contains("ab".repeat(32).as_str()));
        assert_eq!(inference.calls.load(Ordering::SeqCst), 0);
    }
}

// Feed actual router JSON directly to the production pure Node adapter. No SDK
// callback, hardware-chain verdict, worker, Tor client or network is fabricated.
fn node_decode_failures(values: &[serde_json::Value], expected: &[serde_json::Value]) {
    use std::io::Write;
    let mut child = std::process::Command::new("node")
        .args(["--input-type=module", "-e", r#"
            import { readFileSync } from 'node:fs';
            import { pathToFileURL } from 'node:url';
            const { decodeTorSetupResponse, TorSetupBackendFailure } = await import(pathToFileURL(process.argv[1]));
            globalThis.fetch = () => { throw new Error('no acquisition allowed'); };
            const values = JSON.parse(readFileSync(0, 'utf8'));
            const results = values.map(value => {
                try { decodeTorSetupResponse(Buffer.from(JSON.stringify(value))); }
                catch (error) {
                    if (!(error instanceof TorSetupBackendFailure) || error.metadata_authenticated !== false || error.transport_authorized !== false) process.exit(2);
                    const { code, stage, constraint, status } = error;
                    return { code, stage, constraint, ...(status === undefined ? {} : { status }) };
                }
                process.exit(3);
            });
            process.stdout.write(JSON.stringify(results));
        "#])
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/clients/pi/tor-setup.mjs"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("pinned Node must be available for the offline paired fixture");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(values).unwrap())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "offline paired decoder rejected the generated route failure"
    );
    let decoded: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(decoded, expected);
}

#[tokio::test]
async fn closed_appraisal_and_acquisition_facts_survive_route_to_node() {
    let mut values = Vec::new();
    let mut expected = Vec::new();
    for category in [
        TorVerificationFailure::InputSize,
        TorVerificationFailure::InputNonce,
        TorVerificationFailure::InputWorkload,
        TorVerificationFailure::InputUnknown,
        TorVerificationFailure::AcquisitionUnknown,
        TorVerificationFailure::AcquisitionTransport,
        TorVerificationFailure::AcquisitionStatus,
        TorVerificationFailure::AcquisitionRead,
        TorVerificationFailure::AcquisitionSize,
        TorVerificationFailure::EnvelopeRejected,
        TorVerificationFailure::EnvelopeUnknown,
        TorVerificationFailure::VerificationRejected,
        TorVerificationFailure::VerificationUnknown,
        TorVerificationFailure::VerificationResult,
        TorVerificationFailure::PublisherSchema,
        TorVerificationFailure::PublisherCertificate,
        TorVerificationFailure::PublisherIdentity,
        TorVerificationFailure::PublisherArtifact,
        TorVerificationFailure::PublisherManifest,
        TorVerificationFailure::PublisherUnknown,
        TorVerificationFailure::KeysTls,
        TorVerificationFailure::KeysHpke,
        TorVerificationFailure::KeysUnknown,
        TorVerificationFailure::TorEntry,
        TorVerificationFailure::TorFormat,
        TorVerificationFailure::TorSpki,
        TorVerificationFailure::TorAlgorithm,
        TorVerificationFailure::FreshnessExpired,
    ] {
        let (code, stage, constraint) = category.parts();
        expected.push(json!({"code":code, "stage":stage, "constraint":constraint}));
        let (state, inference, tor) = tor_fixture();
        *tor.facts.lock().unwrap() = Err(TorEvidenceError::Verification(category));
        let response = router(state)
            .oneshot(
                Request::get(format!("/attestation/tor?nonce={}", "23".repeat(32)))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let expected_cause = if stage == "acquisition" {
            InterfaceError::EvidenceUnavailable
        } else {
            InterfaceError::EvidenceInvalid
        };
        assert_eq!(
            response.extensions().get::<InterfaceError>(),
            Some(&expected_cause)
        );
        values.push(
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap(),
        );
        assert_eq!(inference.calls.load(Ordering::SeqCst), 0);
    }
    for status in [100, 503, 999] {
        let (state, inference, tor) = tor_fixture();
        *tor.facts.lock().unwrap() = Err(TorEvidenceError::AcquisitionStatus(status));
        let response = router(state)
            .oneshot(
                Request::get(format!("/attestation/tor?nonce={}", "23".repeat(32)))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            response.extensions().get::<InterfaceError>(),
            Some(&InterfaceError::EvidenceUnavailable)
        );
        values.push(
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap(),
        );
        expected.push(json!({"code":"possums_v3_acquisition_http_status", "stage":"acquisition", "constraint":"http_status", "status":status}));
        assert_eq!(inference.calls.load(Ordering::SeqCst), 0);
    }
    let (state, inference, tor) = tor_fixture();
    tor.facts.lock().unwrap().as_mut().unwrap().quote = json!({"synthetic_oversized_quote_not_authenticated": "x".repeat(MAX_ATTESTATION_RESPONSE_BYTES)});
    let response = router(state)
        .oneshot(
            Request::get(format!("/attestation/tor?nonce={}", "23".repeat(32)))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        response.extensions().get::<InterfaceError>(),
        Some(&InterfaceError::ResponseEncodingLimit)
    );
    values.push(
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap(),
    );
    expected.push(json!({"code":"tor_response_encoding_limit", "stage":"response", "constraint":"bounded_json"}));
    assert_eq!(inference.calls.load(Ordering::SeqCst), 0);
    node_decode_failures(&values, &expected);
}

#[tokio::test]
async fn enabled_health_requires_sdk_reachability_and_serving_lifecycle() {
    let (state, _, _) = tor_fixture();
    let check = || Request::get("/healthz").body(Body::empty()).unwrap();
    assert_eq!(
        router(state.clone())
            .oneshot(check())
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    state.tor_ready.as_ref().unwrap().mark_running();
    assert_eq!(
        router(state.clone())
            .oneshot(check())
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    state.lifecycle.quiesce();
    assert_eq!(
        router(state).oneshot(check()).await.unwrap().status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
}

#[tokio::test]
async fn typed_timeout_sources_reach_actual_diagnostic_response_without_payloads_or_work() {
    use retry_error::RetryError;
    use tor_hsservice::{
        status::{DescUploadRetryError, Problem, State},
        DescUploadError,
    };

    let (state, inference, tor) = tor_fixture();
    let health = state.tor_ready.as_ref().unwrap();
    let app = router(state.clone());
    let check = || {
        Request::get("/healthz/diagnostics")
            .body(Body::empty())
            .unwrap()
    };
    health.observe_sdk(State::DegradedUnreachable, None);
    let response = app.clone().oneshot(check()).await.unwrap();
    let mut baseline: serde_json::Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(baseline.as_object().unwrap().len(), 8);

    let request = DescUploadError::Request(tor_dirclient::RequestFailedError {
        source: None,
        error: tor_dirclient::RequestError::HttpStatus(
            503,
            "HOSTILE_REQUEST_SECRET https://private.invalid prompt=PRIVATE".into(),
        ),
    });
    let internal = DescUploadError::Bug(tor_error::Bug::new(
        tor_error::ErrorKind::Internal,
        "HOSTILE_INTERNAL_SECRET account=PRIVATE",
    ));
    for (sources, expected) in [
        (vec![], "descriptor_timeout_no_recorded_source"),
        (
            vec![DescUploadError::Circuit(tor_circmgr::Error::RequestTimeout)],
            "descriptor_timeout_circuit",
        ),
        (
            vec![DescUploadError::Stream(tor_circmgr::Error::RequestTimeout)],
            "descriptor_timeout_stream",
        ),
        (vec![request.clone()], "descriptor_timeout_request"),
        (vec![internal.clone()], "descriptor_timeout_internal"),
        (vec![request, internal], "descriptor_timeout_mixed"),
    ] {
        let mut retries =
            RetryError::in_attempt_to("HOSTILE_CONTEXT_SECRET https://private.invalid");
        for source in sources {
            retries.push(source);
        }
        let problem = Problem::DescriptorUpload(vec![DescUploadRetryError::Timeout(retries)]);
        health.observe_sdk(State::DegradedUnreachable, Some(&problem));
        drop(problem);
        // No credential or attestation is needed to read this closed projection.
        let response = app.clone().oneshot(check()).await.unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
        assert!(response.extensions().get::<InterfaceError>().is_none());
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert!(bytes.len() < 1024);
        let text = String::from_utf8_lossy(&bytes);
        for sentinel in [
            "HOSTILE",
            "PRIVATE",
            "https://",
            "private.invalid",
            "prompt=",
            "account=",
        ] {
            assert!(!text.contains(sentinel));
        }
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        baseline["tor_problem"] = json!(expected);
        assert_eq!(value, baseline);
        assert_eq!(value["code"], "tor_reachability_unconfirmed");
        assert_eq!(value["ready"], false);
        assert_eq!(value["lifecycle_serving"], true);
        assert!(!health.is_ready());
        let old_health = app
            .clone()
            .oneshot(Request::get("/healthz").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(old_health.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(old_health
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .is_empty());
    }
    assert!(tor.challenges.lock().unwrap().is_empty());
    for counter in [
        &inference.catalog_calls,
        &inference.tokenize_calls,
        &inference.calls,
        &inference.document_calls,
    ] {
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn descriptor_detail_is_exact_opt_in_coherent_and_initiates_no_work() {
    use retry_error::RetryError;
    use tor_hsservice::{
        status::{DescUploadRetryError, Problem, State},
        DescUploadError,
    };
    let (state, inference, tor) = tor_fixture();
    let health = state.tor_ready.as_ref().unwrap();
    let app = router(state.clone());
    let mut sources = RetryError::in_attempt_to("HOSTILE_CONTEXT_SECRET");
    sources.push(DescUploadError::Circuit(tor_circmgr::Error::CircTimeout(
        None,
    )));
    sources.push(DescUploadError::Stream(tor_circmgr::Error::RequestTimeout));
    sources.push(DescUploadError::Request(
        tor_dirclient::RequestFailedError {
            source: None,
            error: tor_dirclient::RequestError::HttpStatus(503, "HOSTILE_REASON_SECRET".into()),
        },
    ));
    let problem = Problem::DescriptorUpload(vec![DescUploadRetryError::Timeout(sources)]);
    for (phase, expected_status, expected_code) in [
        (
            0,
            StatusCode::SERVICE_UNAVAILABLE,
            "tor_status_not_observed",
        ),
        (
            1,
            StatusCode::SERVICE_UNAVAILABLE,
            "tor_reachability_unconfirmed",
        ),
        (2, StatusCode::OK, "gateway_ready"),
        (3, StatusCode::SERVICE_UNAVAILABLE, "gateway_not_serving"),
    ] {
        match phase {
            1 => health.observe_sdk(State::Bootstrapping, Some(&problem)),
            2 => health.observe_sdk(State::Running, None),
            3 => {
                state.lifecycle.quiesce();
            }
            _ => {}
        }
        let mut baseline = None;
        for query in [
            "",
            "?",
            "?unknown=HOSTILE_PRIVATE_QUERY",
            "?detail=descriptor&extra=HOSTILE_PRIVATE_QUERY",
            "?detail=descriptor&detail=descriptor",
            "?detail=Descriptor",
            "?detail=%64escriptor",
            "?%64etail=descriptor",
            "?detail=descriptor&",
            "?detail=descriptor%00",
            "?detail=descriptor",
        ] {
            let rich = query == "?detail=descriptor";
            let response = app
                .clone()
                .oneshot(
                    Request::get(format!("/healthz/diagnostics{query}"))
                        .header("authorization", "HOSTILE_PRIVATE_HEADER")
                        .body(Body::from("HOSTILE_PRIVATE_BODY"))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), expected_status);
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
            assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
            assert!(response.extensions().get::<InterfaceError>().is_none());
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            assert!(bytes.len() < 32 * 1024 + 1024);
            assert!(!String::from_utf8_lossy(&bytes).contains("HOSTILE"));
            let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(value["code"], expected_code);
            assert_eq!(value.as_object().unwrap().len(), if rich { 9 } else { 8 });
            if rich {
                let evidence = value
                    .as_object_mut()
                    .unwrap()
                    .remove("descriptor_evidence")
                    .unwrap();
                assert_eq!(evidence.as_object().unwrap().len(), 7);
                assert_eq!(
                    evidence["selection"],
                    match phase {
                        0 => "no_snapshot",
                        1 => "descriptor_upload_selected",
                        _ => "no_problem_selected",
                    }
                );
                if phase == 1 {
                    assert_eq!(evidence["outcomes"], json!(["timeout"]));
                    assert_eq!(evidence["circuit"]["variants"], json!(["circuit_timeout"]));
                    assert_eq!(evidence["stream"]["variants"], json!(["request_timeout"]));
                    assert_eq!(evidence["request"]["http_statuses"], json!([503]));
                } else {
                    assert_eq!(evidence["outcomes"], json!([]));
                    assert_eq!(evidence["request"]["http_statuses"], json!([]));
                }
            }
            if let Some(baseline) = &baseline {
                assert_eq!(&value, baseline);
            } else {
                baseline = Some(value);
            }
        }
        let response = app
            .clone()
            .oneshot(
                Request::get("/healthz?detail=descriptor")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            if phase == 2 {
                StatusCode::NO_CONTENT
            } else {
                StatusCode::SERVICE_UNAVAILABLE
            }
        );
        assert!(response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .is_empty());
    }
    assert!(tor.challenges.lock().unwrap().is_empty());
    for counter in [
        &inference.catalog_calls,
        &inference.tokenize_calls,
        &inference.calls,
        &inference.document_calls,
    ] {
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }
    let (plain, _, _) = fixture_state();
    assert_eq!(
        router(plain)
            .oneshot(
                Request::get("/healthz/diagnostics?detail=descriptor")
                    .body(Body::empty())
                    .unwrap()
            )
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn request_only_readiness_diagnostic_is_closed_uncached_and_initiates_no_work() {
    let (state, inference, tor) = tor_fixture();
    let app = router(state.clone());
    let check = || Request::get("/healthz").body(Body::empty()).unwrap();
    for (running, expected_code) in [(false, "tor_status_not_observed"), (true, "gateway_ready")] {
        if running {
            state.tor_ready.as_ref().unwrap().mark_running();
        }
        let old_health = app.clone().oneshot(check()).await.unwrap();
        assert_eq!(
            old_health.status(),
            if running {
                StatusCode::NO_CONTENT
            } else {
                StatusCode::SERVICE_UNAVAILABLE
            }
        );
        assert!(old_health
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .is_empty());
        let response = app
            .clone()
            .oneshot(
                Request::get("/healthz/diagnostics?credential=HOSTILE_PRIVATE_QUERY")
                    .header("authorization", "HOSTILE_PRIVATE_HEADER")
                    .body(Body::from("HOSTILE_PRIVATE_BODY"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            if running {
                StatusCode::OK
            } else {
                StatusCode::SERVICE_UNAVAILABLE
            }
        );
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
        assert!(response.extensions().get::<InterfaceError>().is_none());
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert!(bytes.len() < 1024);
        assert!(!String::from_utf8_lossy(&bytes).contains("HOSTILE"));
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let keys: std::collections::BTreeSet<_> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            [
                "code",
                "stage",
                "constraint",
                "action",
                "lifecycle_serving",
                "ready",
                "tor_state",
                "tor_problem"
            ]
            .into_iter()
            .collect()
        );
        assert_eq!(value["code"], expected_code);
        assert_eq!(value["ready"], running);
        assert_eq!(value["lifecycle_serving"], true);
        assert_eq!(state.tor_ready.as_ref().unwrap().is_ready(), running);
    }
    state.lifecycle.quiesce();
    let response = app
        .oneshot(
            Request::get("/healthz/diagnostics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let value: serde_json::Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(value["code"], "gateway_not_serving");
    assert_eq!(value["ready"], false);
    assert_eq!(value["lifecycle_serving"], false);
    assert!(tor.challenges.lock().unwrap().is_empty());
    for counter in [
        &inference.catalog_calls,
        &inference.tokenize_calls,
        &inference.calls,
        &inference.document_calls,
    ] {
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }
    let (plain, _, _) = fixture_state();
    assert_eq!(
        router(plain)
            .oneshot(
                Request::get("/healthz/diagnostics")
                    .body(Body::empty())
                    .unwrap()
            )
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
}
