use possums::{
    catalog::actual_cost,
    inference::{authenticated_catalog, Inference, Message, TinfoilInference},
};
use std::{env, fs, time::SystemTime};

const HOST: &str = "inference.tinfoil.sh";
const REPOSITORY: &str = "tinfoilsh/confidential-model-router";
const CANARY_MAX_OUTPUT_TOKENS: u64 = 64;

#[tokio::test]
#[ignore = "requires a funded TINFOIL_API_KEY and makes a live canary request"]
async fn production_client_authenticates_catalog_tokenization_and_generation() {
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
    let input_tokens = inference
        .count_tokens(&model.id, &messages)
        .await
        .expect("authenticated live tokenization failed");
    let mut quote = catalog
        .quote(&model.id, input_tokens)
        .expect("live canary exceeded the selected model context");
    quote.model.max_output_tokens = quote.model.max_output_tokens.min(CANARY_MAX_OUTPUT_TOKENS);
    let generation = inference
        .generate(&quote.model, &messages)
        .await
        .expect("authenticated live generation failed");
    assert_eq!(generation.input_tokens, input_tokens);
    assert!(!generation.content.is_empty());
    actual_cost(&quote, generation.output_tokens).expect("live usage exceeded the catalog quote");
}

/// Diagnostic only. The buffered canary above is historical coverage, not stream evidence.
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
    let mut matches_candidate_contract = true;
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
        matches_candidate_contract &= probe.failure.is_none()
            && probe.sse_content_type
            && probe.eof
            && probe.finish_events == 1
            && probe.usage_events == 1
            && !probe.unknown_usage_fields
            && probe
                .finish_event
                .zip(probe.usage_event)
                .is_some_and(|(finish, usage)| finish <= usage)
            && probe
                .usage_event
                .zip(probe.done_event)
                .is_some_and(|(usage, done)| usage < done)
            && probe.usage.as_ref().is_some_and(|usage| {
                usage.prompt_tokens == probe.tokenizer_tokens
                    && usage.completion_tokens <= CANARY_MAX_OUTPUT_TOKENS
                    && usage.prompt_tokens.checked_add(usage.completion_tokens)
                        == Some(usage.total_tokens)
            });
        if probe.failure.is_some() {
            break; // Stop spending on transport/provider failures; do not replay.
        }
    }
    assert!(matches_candidate_contract,
        "stream observations do not establish the candidate contract; financial gate remains BLOCKED");
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
