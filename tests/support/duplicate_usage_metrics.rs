//! Identical HTTP field repetition is one snapshot, never additive usage.
use super::{
    native_usage_contract::{check, duplicate_response},
    InferenceFailure,
};

#[tokio::test]
async fn identical_usage_headers_and_trailers_settle_once() {
    let metrics = "prompt=2,completion=3,total=5,model=m,cached_prompt_tokens=1,uncached_prompt_tokens=1,cost_usd=0.000005,unknown=private-marker";
    for initial_header in [false, true] {
        for repetitions in [1, 2, 3] {
            check(
                duplicate_response(&vec![metrics; repetitions], initial_header),
                None,
                "final_metrics",
                5,
                7,
            )
            .await;
        }
    }
}

#[tokio::test]
async fn conflicting_usage_trailers_cannot_replace_valid_running_evidence() {
    let metrics = "prompt=2,completion=3,total=5,model=m,cost_usd=0.000005";
    for conflicting in [
        "prompt=2,completion=4,total=6,model=m,cost_usd=0.000005",
        "prompt=2,completion=3,total=5,model=wrong,cost_usd=0.000005",
        "prompt=2,completion=3,total=5,model=m,cost_usd=0.000006",
        "prompt=2,completion=3,total=5,model=m,cached_prompt_tokens=1,uncached_prompt_tokens=1,cost_usd=0.000005",
        "completion=3,prompt=2,total=5,model=m,cost_usd=0.000005",
        "prompt=2,completion=3,total=5,model=m,cost_usd=private-marker",
    ] {
        for values in [vec![metrics, conflicting], vec![conflicting, metrics], vec![metrics, metrics, conflicting]] {
            check(duplicate_response(&values, false), Some(InferenceFailure::StreamUsageInvalid), "running_sse", 5, 7).await;
        }
    }
}

#[tokio::test]
async fn identical_malformed_usage_trailers_still_fail_closed() {
    for metrics in [
        "prompt=2,completion=3,total=6,model=m",
        "prompt=2,completion=3,total=5,model=wrong",
        "prompt=2,prompt=2,completion=3,total=5,model=m",
        "prompt=2,completion=3,total=5,model=m,cached_prompt_tokens=1",
        "prompt=2,completion=3,total=5,model=m,cached_prompt_tokens=3,uncached_prompt_tokens=0",
        "prompt=2,completion=3,total=5,model=m,cost_usd=private-marker",
    ] {
        check(
            duplicate_response(&[metrics, metrics], false),
            Some(InferenceFailure::StreamUsageInvalid),
            "running_sse",
            5,
            7,
        )
        .await;
    }
}
