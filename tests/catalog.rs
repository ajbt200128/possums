use possums::catalog::{actual_cost, Catalog, CatalogError, Model};

fn catalog() -> Catalog {
    Catalog::parse_authenticated(
        br#"{"object":"list","data":[{"id":"model-a","type":"chat","context_window":100,"endpoints":["/v1/chat/completions","/v1/responses"],"pricing":{"inputTokenPricePer1M":2,"outputTokenPricePer1M":3,"requestPrice":0}}]}"#,
        1_000,
    )
    .unwrap()
}

#[test]
fn quote_snapshots_rate_and_uses_remaining_context_for_output() {
    let quote = catalog().quote("model-a", 60).unwrap();
    assert_eq!(quote.model.max_output_tokens, 40);
    assert_eq!(quote.reserved_microunits, 312);
    assert_eq!(actual_cost(&quote, 40).unwrap(), 312);
}

#[test]
fn reservation_covers_the_most_expensive_context_allocation() {
    let quote = catalog().reservation_quote("model-a").unwrap();
    assert_eq!(quote.input_tokens, 100);
    assert_eq!(quote.model.max_output_tokens, 100);
    assert_eq!(quote.reserved_microunits, 390);
}

#[test]
fn parses_decimal_prices_without_floating_point() {
    let catalog = Catalog::parse_authenticated(
        br#"{"object":"list","data":[{"id":"model-a","type":"chat","context_window":1000000,"endpoints":["/v1/chat/completions"],"pricing":{"inputTokenPricePer1M":0.65,"outputTokenPricePer1M":1.45,"requestPrice":0}}]}"#,
        1,
    )
    .unwrap();
    let quote = catalog.quote("model-a", 500_000).unwrap();
    assert_eq!(quote.model.input_microunits_per_million_tokens, 650_000);
    assert_eq!(quote.model.output_microunits_per_million_tokens, 1_450_000);
    assert_eq!(quote.reserved_microunits, 1_365_000);

    let precise = Catalog::parse_authenticated(
        br#"{"object":"list","data":[{"id":"precise","type":"chat","context_window":10,"endpoints":["/v1/chat/completions"],"pricing":{"inputTokenPricePer1M":0.10000000000000001,"outputTokenPricePer1M":1,"requestPrice":0}}]}"#,
        1,
    )
    .unwrap();
    assert_eq!(
        precise.models[0].input_microunits_per_million_tokens,
        100_001
    );
}

#[test]
fn excludes_non_chat_models_and_rejects_duplicate_chat_models() {
    let filtered = Catalog::parse_authenticated(
        br#"{"object":"list","data":[{"id":"embed","type":"embedding","context_window":10,"endpoints":["/v1/embeddings"],"pricing":{"inputTokenPricePer1M":1,"outputTokenPricePer1M":0,"requestPrice":0}},{"id":"chat","type":"chat","context_window":10,"endpoints":["/v1/chat/completions"],"pricing":{"inputTokenPricePer1M":1,"outputTokenPricePer1M":1,"requestPrice":0}}]}"#,
        1,
    )
    .unwrap();
    assert_eq!(filtered.models.len(), 1);
    assert_eq!(filtered.models[0].id, "chat");

    let duplicate = br#"{"object":"list","data":[{"id":"same","type":"chat","context_window":2,"endpoints":["/v1/chat/completions"],"pricing":{"inputTokenPricePer1M":1,"outputTokenPricePer1M":1,"requestPrice":0}},{"id":"same","type":"chat","context_window":2,"endpoints":["/v1/chat/completions"],"pricing":{"inputTokenPricePer1M":1,"outputTokenPricePer1M":1,"requestPrice":0}}]}"#;
    assert_eq!(
        Catalog::parse_authenticated(duplicate, 1).unwrap_err(),
        CatalogError::Invalid
    );
}

#[test]
fn rejects_unknown_schema_invalid_prices_and_context_overflow() {
    assert_eq!(
        Catalog::parse_authenticated(br#"{"issued_at_unix":1,"models":[]}"#, 1).unwrap_err(),
        CatalogError::Invalid
    );
    assert_eq!(
        Catalog::parse_authenticated(
            br#"{"object":"list","data":[{"id":"x","type":"chat","context_window":2,"endpoints":["/v1/chat/completions"],"pricing":{"inputTokenPricePer1M":0,"outputTokenPricePer1M":1,"requestPrice":0}}]}"#,
            1,
        )
        .unwrap_err(),
        CatalogError::Invalid
    );
    assert_eq!(
        Catalog::parse_authenticated(
            br#"{"object":"list","data":[{"id":"x","type":"chat","context_window":2,"endpoints":["/v1/chat/completions"],"pricing":{"inputTokenPricePer1M":1,"outputTokenPricePer1M":1,"requestPrice":0.01}}]}"#,
            1,
        )
        .unwrap_err(),
        CatalogError::Invalid
    );
    assert_eq!(
        catalog().quote("model-a", 100).unwrap_err(),
        CatalogError::ContextExceeded
    );
}

#[test]
fn rejects_cost_overflow() {
    let catalog = Catalog {
        models: vec![Model {
            id: "x".into(),
            context_tokens: u64::MAX,
            max_output_tokens: u64::MAX,
            input_microunits_per_million_tokens: u64::MAX,
            output_microunits_per_million_tokens: u64::MAX,
        }],
    };
    assert_eq!(
        catalog.quote("x", u64::MAX - 1).unwrap_err(),
        CatalogError::Overflow
    );
}
