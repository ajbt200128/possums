use possums::{render, telemetry::AggregateMetrics};

#[test]
fn telemetry_is_off_by_default_and_suppresses_sparse_buckets() {
    let off = AggregateMetrics::default();
    for _ in 0..20 {
        off.increment("requests_total");
    }
    assert!(off.exportable_snapshot().is_empty());

    let enabled = AggregateMetrics::new(true);
    enabled.increment("requests_total");
    enabled.increment("credential-canary");
    assert!(enabled.exportable_snapshot().is_empty());
    for _ in 1..10 {
        enabled.increment("requests_total");
    }
    assert_eq!(enabled.exportable_snapshot(), vec![("requests_total", 10)]);
}

#[test]
fn hostile_content_is_neither_active_nor_remotely_loaded() {
    let canary = "sensitive-canary.invalid";
    let rendered = render::markdown(&format!(
        "<script>fetch('https://{canary}')</script> ![x](https://{canary}/pixel)"
    ));
    assert!(!rendered.contains("<script"));
    assert!(!rendered.contains("<img"));
    assert!(!rendered.contains("src="));
}
