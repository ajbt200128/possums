use super::*;

#[test]
fn tokenizer_send_predicates_prefer_deadline_without_guessing_connection_cause() {
    for (timeout, connect, expected) in [
        (true, false, InferenceFailure::TokenizerSendDeadline),
        (true, true, InferenceFailure::TokenizerSendDeadline),
        (false, true, InferenceFailure::TokenizerConnectFailed),
        (false, false, InferenceFailure::TokenizerSendFailed),
    ] {
        assert_eq!(tokenizer_send_failure(timeout, connect), expected);
        assert_eq!(
            serde_json::to_string(&expected).unwrap(),
            "\"tokenizer_send_failed\""
        );
    }
}

#[tokio::test]
async fn supported_tokenizer_transport_predicates_discard_hostile_url_and_error_text() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    // A listening loopback socket with no application response yields a real SDK
    // timeout; no external service, inference, credentials or verified channel.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://{}/HOSTILE_PRIVATE_MARKER",
        listener.local_addr().unwrap()
    );
    let error = client
        .get(&url)
        .timeout(Duration::from_millis(25))
        .send()
        .await
        .unwrap_err();
    assert!(error.is_timeout());
    let failure = tokenizer_send_failure(error.is_timeout(), error.is_connect());
    assert_eq!(failure, InferenceFailure::TokenizerSendDeadline);
    assert!(!failure.to_string().contains("HOSTILE_PRIVATE_MARKER"));
    drop(error);
    drop(listener);

    let error = client
        .get("http://127.0.0.1:0/HOSTILE_PRIVATE_MARKER")
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .unwrap_err();
    assert!(error.is_connect());
    assert!(!error.is_timeout());
    let failure = tokenizer_send_failure(error.is_timeout(), error.is_connect());
    assert_eq!(failure, InferenceFailure::TokenizerConnectFailed);
    assert!(!failure.to_string().contains("HOSTILE_PRIVATE_MARKER"));
    drop(error);

    // A malformed synthetic header exercises an SDK request-build error rather
    // than classifying exception text as a connection or deadline.
    let error = client
        .get(&url)
        .header("x-synthetic", "HOSTILE_PRIVATE_MARKER\n")
        .send()
        .await
        .unwrap_err();
    assert!(error.is_builder());
    let failure = tokenizer_send_failure(error.is_timeout(), error.is_connect());
    assert_eq!(failure, InferenceFailure::TokenizerSendFailed);
    assert!(failure.to_string().contains("cause is unknown"));
    assert!(!failure.to_string().contains("HOSTILE_PRIVATE_MARKER"));
}
