use possums::inference::{
    authenticated_catalog, Inference, Message, ProbeFinish, TinfoilInference,
};
use std::{env, fs, time::SystemTime};

const HOST: &str = "inference.tinfoil.sh";
const REPOSITORY: &str = "tinfoilsh/confidential-model-router";
const CANARY_MAX_OUTPUT_TOKENS: u64 = 64;

#[tokio::test]
#[ignore = "requires a funded TINFOIL_API_KEY and makes one live streaming canary request"]
async fn production_client_authenticates_catalog_tokenization_and_streaming() {
    let api_key = load_api_key();
    let inference = TinfoilInference::connect(HOST, REPOSITORY, api_key)
        .await
        .expect("live attested Tinfoil connection failed");
    let now = SystemTime::UNIX_EPOCH.elapsed().unwrap().as_secs();
    let catalog = authenticated_catalog(&inference, now)
        .await
        .expect("live Tinfoil catalog authentication or validation failed");
    let requested_model = env::var("TINFOIL_LIVE_MODEL").ok();
    let model = requested_model
        .as_deref()
        .map(|id| {
            catalog
                .models
                .iter()
                .find(|model| model.id == id)
                .expect("requested live model was not in the authenticated catalog")
        })
        .unwrap_or_else(|| {
            catalog
                .models
                .first()
                .expect("live Tinfoil catalog contained no supported chat model")
        });
    let messages = [Message {
        role: "user".into(),
        content: "Reply with exactly: possums-live-canary".into(),
    }];
    // Synthetic admission for this ignored production-adapter canary only;
    // the separate fixed-input diagnostic has no route lease.
    let heavy: std::sync::Arc<possums::telemetry::hooks::Lease> = std::sync::Arc::new(
        std::sync::Arc::new(tokio::sync::Semaphore::new(1))
            .try_acquire_owned()
            .unwrap()
            .into(),
    );
    let input_tokens = inference
        .count_tokens(&model.id, &messages, heavy.clone())
        .await
        .expect("authenticated live tokenization failed");
    let mut quote = catalog
        .quote(&model.id, input_tokens)
        .expect("live canary exceeded the selected model context");
    quote.model.max_output_tokens = quote.model.max_output_tokens.min(CANARY_MAX_OUTPUT_TOKENS);
    // A 64-token canary may spend every output token on reasoning and emit no
    // visible content. Terminal authenticated usage, not delta count, decides
    // success; this does not establish provider invoice semantics.
    let usage = inference
        .generate_stream(&quote.model, &messages, heavy, |_| {})
        .await
        .expect("authenticated live streaming failed");
    // Success is terminal: the adapter has validated finish, DONE and EOF.
    assert!(usage.total_tokens > 0, "live stream returned empty usage");
    assert_eq!(
        usage.input_tokens.checked_add(usage.output_tokens),
        Some(usage.total_tokens),
        "live stream returned inconsistent terminal usage"
    );
}

/// Check every current model with the production adapter, not only the tolerant
/// diagnostic probe. A failure stops before any next model; no generation retries.
#[tokio::test]
#[ignore = "funded opt-in: up to eight independent 64-output-token live generations"]
async fn production_adapter_streams_all_catalog_models() {
    assert_eq!(
        env::var("TINFOIL_ALL_MODELS_FUNDED").as_deref(),
        Ok("yes"),
        "set TINFOIL_ALL_MODELS_FUNDED=yes to authorize the funded canaries"
    );
    let inference = TinfoilInference::connect(HOST, REPOSITORY, load_api_key())
        .await
        .expect("live attested Tinfoil connection failed");
    let now = SystemTime::UNIX_EPOCH.elapsed().unwrap().as_secs();
    let catalog = authenticated_catalog(&inference, now)
        .await
        .expect("live Tinfoil catalog authentication or validation failed");
    assert!(
        catalog.models.len() <= 8,
        "catalog exceeds funded request budget"
    );
    for model in &catalog.models {
        let messages = [Message {
            role: "user".into(),
            content: "Reply with exactly: possums-production-adapter-canary".into(),
        }];
        let heavy: std::sync::Arc<possums::telemetry::hooks::Lease> = std::sync::Arc::new(
            std::sync::Arc::new(tokio::sync::Semaphore::new(1))
                .try_acquire_owned()
                .unwrap()
                .into(),
        );
        let input_tokens = inference
            .count_tokens(&model.id, &messages, heavy.clone())
            .await
            .expect("authenticated tokenization failed");
        let mut quote = catalog
            .quote(&model.id, input_tokens)
            .expect("canary exceeded model context");
        quote.model.max_output_tokens = quote.model.max_output_tokens.min(CANARY_MAX_OUTPUT_TOKENS);
        let usage = inference
            .generate_stream(&quote.model, &messages, heavy, |_| {})
            .await
            .expect("authenticated live streaming failed; do not replay");
        assert!(usage.total_tokens > 0);
        assert_eq!(
            usage.input_tokens.checked_add(usage.output_tokens),
            Some(usage.total_tokens),
            "live stream returned inconsistent terminal usage"
        );
        // Public model IDs only; never retain or log content, keys or raw usage.
        println!("validated model={}", model.id);
    }
}

/// Diagnostic only; separate from the production streaming adapter canary above.
#[tokio::test]
#[ignore = "funded opt-in: up to eight fixed-input streaming requests, 64 output tokens each"]
async fn streaming_contract_probe_all_catalog_models() {
    assert!(
        env::var("TINFOIL_STREAM_PROBE_FUNDED").as_deref() == Ok("yes"),
        "set TINFOIL_STREAM_PROBE_FUNDED=yes to authorize the small operator-funded probe"
    );
    let inference = TinfoilInference::connect(HOST, REPOSITORY, load_api_key())
        .await
        .expect("live attested Tinfoil connection failed");
    let catalog = authenticated_catalog(
        &inference,
        SystemTime::UNIX_EPOCH.elapsed().unwrap().as_secs(),
    )
    .await
    .expect("live catalog authentication or validation failed");
    // Never silently spend against an unexpectedly enlarged catalog.
    assert!(
        catalog.models.len() <= 8,
        "catalog exceeds this probe's funded request budget"
    );
    let mut compatible_observations = true;
    for model in &catalog.models {
        let probe = inference
            .probe_streaming_contract(&model.id)
            .await
            .expect("stream probe connection or tokenization unavailable; no automatic retry");
        // Model IDs have passed the catalog's length/character validation. The
        // report exposes only allowlisted numeric/enum observations, never text.
        println!(
            "model={} context={} probe={probe:?}",
            model.id, model.context_tokens
        );
        // Preliminary observations only: the diagnostic is deliberately not the
        // production protocol validator. LAST usage may precede finish, decrease,
        // differ from tokenization, or exceed the operational output allowance.
        compatible_observations &= probe.failure.is_none()
            && probe.sse_content_type
            && probe.eof
            && probe.finish_events == 1
            && matches!(probe.finish, Some(ProbeFinish::Stop | ProbeFinish::Length))
            && probe.usage_events >= 1
            && probe
                .finish_event
                .zip(probe.done_event)
                .is_some_and(|(finish, done)| finish < done)
            && probe
                .usage_event
                .zip(probe.done_event)
                .is_some_and(|(usage, done)| usage < done)
            && probe.usage.as_ref().is_some_and(|usage| {
                usage.prompt_tokens.checked_add(usage.completion_tokens) == Some(usage.total_tokens)
            });
        if probe.failure.is_some() {
            break; // Stop spending on transport/provider failures; do not replay.
        }
    }
    assert!(
        compatible_observations,
        "diagnostic observations differ from gateway acceptance rules; no automatic replay"
    );
    // This is not a provider billing contract gate. Provider-to-invoice semantics
    // and maximum billable-token bounds remain UNVERIFIED operator risk.
}

fn load_api_key() -> String {
    env::var("TINFOIL_API_KEY")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| dotenv_value("TINFOIL_API_KEY"))
        .expect("set TINFOIL_API_KEY or add it to the ignored local .env file")
}

fn dotenv_value(wanted: &str) -> Option<String> {
    let contents = fs::read_to_string(".env").ok()?;
    contents.lines().find_map(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        let (name, value) = line.split_once('=')?;
        if name.trim() != wanted {
            return None;
        }
        let value = value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .or_else(|| {
                value
                    .strip_prefix('\'')
                    .and_then(|value| value.strip_suffix('\''))
            })
            .unwrap_or(value);
        (!value.is_empty()).then(|| value.to_owned())
    })
}
