use possums::catalog::{actual_cost, Catalog, CatalogError};

fn catalog(now: u64) -> Catalog {
    Catalog::parse_authenticated(
        format!(
            r#"{{"issued_at_unix":{now},"models":[{{"id":"model-a","context_tokens":100,"max_output_tokens":40,"input_microunits_per_token":2,"output_microunits_per_token":3}}]}}"#
        )
        .as_bytes(),
        now,
    )
    .unwrap()
}

#[test]
fn quote_snapshots_rate_and_allows_maximum_output() {
    let quote = catalog(1_000).quote("model-a", 60).unwrap();
    assert_eq!(quote.reserved_microunits, 312);
    assert_eq!(actual_cost(&quote, 40).unwrap(), 312);
}

#[test]
fn rejects_stale_duplicate_and_context_overflow() {
    assert_eq!(
        catalog(1_000).quote("model-a", 61).unwrap_err(),
        CatalogError::ContextExceeded
    );
    let duplicate = br#"{"issued_at_unix":1000,"models":[{"id":"same","context_tokens":2,"max_output_tokens":1,"input_microunits_per_token":1,"output_microunits_per_token":1},{"id":"same","context_tokens":2,"max_output_tokens":1,"input_microunits_per_token":1,"output_microunits_per_token":1}]}"#;
    assert_eq!(
        Catalog::parse_authenticated(duplicate, 1_000).unwrap_err(),
        CatalogError::Invalid
    );
    assert_eq!(
        Catalog::parse_authenticated(
            br#"{"issued_at_unix":1,"models":[{"id":"x","context_tokens":2,"max_output_tokens":1,"input_microunits_per_token":1,"output_microunits_per_token":1}]}"#,
            1_000,
        )
        .unwrap_err(),
        CatalogError::Stale
    );
}

#[test]
fn rejects_overflow() {
    let bytes = format!(
        r#"{{"issued_at_unix":1,"models":[{{"id":"x","context_tokens":{},"max_output_tokens":1,"input_microunits_per_token":{},"output_microunits_per_token":1}}]}}"#,
        u64::MAX,
        u64::MAX
    );
    let catalog = Catalog::parse_authenticated(bytes.as_bytes(), 1).unwrap();
    assert_eq!(catalog.quote("x", 2).unwrap_err(), CatalogError::Overflow);
}
