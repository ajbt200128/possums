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
    let model = catalog
        .models
        .first()
        .expect("live Tinfoil catalog contained no supported chat model");
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
