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
fn reservation_covers_operational_input_and_output_bounds_together() {
    let quote = catalog().reservation_quote("model-a").unwrap();
    assert_eq!(quote.input_tokens, 100);
    assert_eq!(quote.model.max_output_tokens, 100);
    assert_eq!(quote.reserved_microunits, 650);
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
    assert_eq!(catalog.reservation_quote("x"), Err(CatalogError::Overflow));
}

#[test]
fn distinct_normalized_output_bound_and_context_allowance_preserve_snapshot() {
    let mut catalog = catalog();
    // Synthetic normalized bound, not an observed Tinfoil catalog field.
    catalog.models[0].max_output_tokens = 40;
    let reservation = catalog.reservation_quote("model-a").unwrap();
    assert_eq!(reservation.reserved_microunits, 416);
    for (input, output) in [(0, 40), (59, 40), (60, 40), (61, 39), (99, 1)] {
        let request = catalog.quote("model-a", input).unwrap();
        assert_eq!(request.model.max_output_tokens, output);
        assert_eq!(reservation.input_tokens, 100);
        assert_eq!(reservation.model.max_output_tokens, 40);
        assert!(request.reserved_microunits <= reservation.reserved_microunits);
    }
    for input in [100, 101, u64::MAX] {
        assert_eq!(
            catalog.quote("model-a", input),
            Err(CatalogError::ContextExceeded)
        );
    }
    catalog.models[0].input_microunits_per_million_tokens *= 2;
    catalog.models[0].output_microunits_per_million_tokens *= 2;
    assert_eq!(
        catalog
            .reservation_quote("model-a")
            .unwrap()
            .reserved_microunits,
        832
    );
    assert_eq!(reservation.reserved_microunits, 416);
    assert_eq!(
        reservation.model.input_microunits_per_million_tokens,
        2_000_000
    );
    assert_eq!(
        reservation.model.output_microunits_per_million_tokens,
        3_000_000
    );
}

#[test]
fn reservation_rounds_the_combined_numerator_once_and_rejects_unrepresentable_cost() {
    let mut catalog = catalog();
    catalog.models[0].context_tokens = 1;
    catalog.models[0].max_output_tokens = 1;
    catalog.models[0].input_microunits_per_million_tokens = 1;
    catalog.models[0].output_microunits_per_million_tokens = 1;
    assert_eq!(
        catalog
            .reservation_quote("model-a")
            .unwrap()
            .reserved_microunits,
        1
    );
    catalog.models[0].context_tokens = u64::MAX;
    catalog.models[0].max_output_tokens = u64::MAX;
    catalog.models[0].input_microunits_per_million_tokens = 1_000_000;
    catalog.models[0].output_microunits_per_million_tokens = 1_000_000;
    // Fits the u128 numerator, but not the u64 monetary representation.
    assert_eq!(
        catalog.reservation_quote("model-a"),
        Err(CatalogError::Overflow)
    );
}

#[test]
fn invalid_normalized_bounds_and_prices_cannot_be_quoted() {
    for (context, output, input_price, output_price) in [
        (0, 1, 1, 1),
        (100, 0, 1, 1),
        (100, 101, 1, 1),
        (100, 100, 0, 1),
        (100, 100, 1, 0),
    ] {
        let mut catalog = catalog();
        let model = &mut catalog.models[0];
        model.context_tokens = context;
        model.max_output_tokens = output;
        model.input_microunits_per_million_tokens = input_price;
        model.output_microunits_per_million_tokens = output_price;
        assert_eq!(
            catalog.reservation_quote("model-a"),
            Err(CatalogError::Invalid)
        );
        assert_eq!(catalog.quote("model-a", 1), Err(CatalogError::Invalid));
    }
}

#[test]
fn unusable_catalog_fields_fail_closed() {
    let base = serde_json::json!({"object":"list","data":[{
        "id":"m", "type":"chat", "context_window":100,
        "endpoints":["/v1/chat/completions"],
        "pricing":{"inputTokenPricePer1M":2,"outputTokenPricePer1M":3,"requestPrice":0}
    }]});
    for context in [
        serde_json::Value::Null,
        serde_json::json!(0),
        serde_json::json!(-1),
        serde_json::json!(1.5),
    ] {
        let mut value = base.clone();
        value["data"][0]["context_window"] = context;
        assert!(Catalog::parse_authenticated(&serde_json::to_vec(&value).unwrap(), 0).is_err());
    }
    let mut missing = base.clone();
    missing["data"][0]
        .as_object_mut()
        .unwrap()
        .remove("context_window");
    assert!(Catalog::parse_authenticated(&serde_json::to_vec(&missing).unwrap(), 0).is_err());
    for field in [
        "inputTokenPricePer1M",
        "outputTokenPricePer1M",
        "requestPrice",
    ] {
        let mut value = base.clone();
        value["data"][0]["pricing"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(Catalog::parse_authenticated(&serde_json::to_vec(&value).unwrap(), 0).is_err());
        value["data"][0]["pricing"][field] = serde_json::json!(-1);
        assert!(Catalog::parse_authenticated(&serde_json::to_vec(&value).unwrap(), 0).is_err());
    }
    for extra in ["imagePrice", "unknownFee"] {
        let mut value = base.clone();
        value["data"][0]["pricing"][extra] = serde_json::json!(0);
        assert_eq!(
            Catalog::parse_authenticated(&serde_json::to_vec(&value).unwrap(), 0),
            Err(CatalogError::Invalid)
        );
    }
}

#[test]
fn cached_input_discount_never_lowers_the_reservation() {
    let mut value = serde_json::json!({"object":"list","data":[{
        "id":"m", "type":"chat", "context_window":100,
        "endpoints":["/v1/chat/completions"],
        "pricing":{"inputTokenPricePer1M":2,"outputTokenPricePer1M":3,"requestPrice":0}
    }]});
    let quote = Catalog::parse_authenticated(&serde_json::to_vec(&value).unwrap(), 0)
        .unwrap()
        .reservation_quote("m")
        .unwrap();
    for discount in [
        serde_json::json!(0),
        serde_json::json!(0.5),
        serde_json::json!(2),
    ] {
        value["data"][0]["pricing"]["cachedInputTokenPricePer1M"] = discount;
        let catalog =
            Catalog::parse_authenticated(&serde_json::to_vec(&value).unwrap(), 0).unwrap();
        assert_eq!(catalog.reservation_quote("m"), Ok(quote.clone()));
    }
    for invalid in [
        serde_json::json!(2.000001),
        serde_json::json!(-1),
        serde_json::json!("0.5"),
    ] {
        value["data"][0]["pricing"]["cachedInputTokenPricePer1M"] = invalid;
        assert_eq!(
            Catalog::parse_authenticated(&serde_json::to_vec(&value).unwrap(), 0),
            Err(CatalogError::Invalid)
        );
    }
}
