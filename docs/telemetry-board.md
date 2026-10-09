# Possums production aggregate board proposal

**Status: verified queries; awaiting operator approval, not created.** Scope: Honeycomb `prod/metrics`, service `possums-gateway`, production slot `gateway-01`. No existing production board or trigger was returned during discovery. No SLO is included or created.

Policy: [permitted/forbidden signals](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data), [complete-window and missing-data semantics](../PRIVACY.md#aggregation-is-necessary-not-sufficient), [access/retention](../PRIVACY.md#processors-retention-access-and-shutdown), [required evidence](../PRIVACY.md#required-evidence-for-every-telemetry-change). Source/wire negative tests, deployed artifact identity, live checks and remaining limitations are linked in [rollout verification](combined-rollout-verification.md#v0017-sampler-rollout-and-dashboard-qualification--2026-10-09).

## Creation settings

- Name: **Possums gateway — aggregate operations**.
- Description: Approved aggregate request, generation, delivery, admission and process metrics; unavailable data is not zero traffic or health.
- Private: **true**, no public sharing.
- Tags: `service:possums`, `scope:production`.
- Intended board range: relative two-hour lookback. Queries were submitted with `from=-2h`; request-family resolution is 300 seconds and infrastructure 60 seconds, both verified without server granularity adjustment. Persisted execution results resolve those inputs to absolute timestamps; verify the created board's range/query readback before accepting live rerun behavior.
- No preset filters: endpoint/model/lane filters applied globally would misleadingly remove unrelated infrastructure/request series.
- No alerts, SLOs, native histogram percentiles, utilization percentages, per-user fields, token/cost events or derived completions/starts success ratio.

## Exact introductory Markdown

The board's full-width text panel will contain exactly:

> ## Possums gateway: aggregate operations
>
> [Source](https://github.com/ajbt200128/possums) · [Privacy policy](https://github.com/ajbt200128/possums/blob/main/PRIVACY.md)
>
> Request, generation, delivery, duration and occupancy data are complete five-minute aggregates. Counts cover released windows only. Gaps/no points are **unavailable**, not zero traffic or health; exported zero bins are genuine zeros. Starts and terminals describe different cohorts. HTTP 2xx is not generation success; completed delivery is not success or confirmed billing. Model `other` means an authenticated, unmapped catalog model.
>
> CPU/RSS and configured capacities use complete one-minute windows. CPU/RSS cover the gateway process, not the enclave, cgroup or model GPU. Generation capacity is the effective heavy-admission bound, not a separate quota. These are configured limits, not free permits or measured memory headroom.
>
> Duration bins b00–b09 have inclusive upper bounds 0.1, 0.5, 1, 5, 15, 30, 60, 120, 300, 600 seconds; each lower bound is the previous upper bound, exclusive. b00 includes zero; b10 is >600 seconds. First output means dispatch to first qualifying text/tool delta, not tokenizer time. Occupancy bins b00–b09 mean 0, 1, 2, 3, 4, 5–8, 9–16, 17–32, 33–64, >64 occupied permits; counts are sampling populations, not users or requests.
>
> Approved aggregates only: no request logs/traces, content, identifiers or token/cost events. Sparse aggregates can reveal individual activity and timing correlation. Honeycomb US uses provider-default MVP retention; this account's effective period is unverified.

## Exact query panels

Each panel has one calculation (RSS adds only a unit-conversion formula). Every query filters to the service, production environment and fixed slot above, plus existence of its metric. CPU/RSS additionally filter `source=process`, `scope=process`; capacity filters `source=configuration`, `scope=gateway`.

1. **Released HTTP starts by endpoint** — SUM(`possums.http.requests`), breakdown `possums.endpoint`. Chart **bar**, display **combo**. Description: “Application HTTP lifetimes started in released five-minute windows; not unique users or complete traffic across missing windows.” [Verified query](https://ui.honeycomb.io/possums/environments/prod/datasets/metrics/result/qGCAVotsoPz). Returned nonempty control and chat-API endpoint populations.
2. **Released HTTP terminals by status/body** — SUM(`possums.http.completed`), breakdown `status_class`, `http_terminal`. Chart **bar**, display **combo**. Description: “Selected status class and observed body terminal disposition; HTTP success is not generation or billing success.” [Verified query](https://ui.honeycomb.io/possums/environments/prod/datasets/metrics/result/f8vyF69DQjb). Returned 2xx, 4xx and 5xx EOF categories.
3. **Released generation terminals** — SUM(`possums.generation.completed`), breakdown `possums.model`, `outcome`, `failure_stage`. Chart **bar**, display **combo**. Description: “Observed reservation-to-terminal outcomes by closed model/stage categories; not completions divided by same-window starts.” [Verified query](https://ui.honeycomb.io/possums/environments/prod/datasets/metrics/result/BkxndcAucyo). Returned success and transport-failure categories.
4. **Released downstream delivery terminals** — SUM(`possums.delivery.completed`), breakdown `possums.model`, `outcome`. Chart **bar**, display **combo**. Description: “Streaming body delivery disposition; completed delivery can include an error response and does not prove settlement.” [Verified query](https://ui.honeycomb.io/possums/environments/prod/datasets/metrics/result/HrFEpWARuCz). Returned completed delivery populations.
5. **HTTP lifetime duration bins** — SUM(`possums.http.duration.bucket`), breakdown `bucket`. Chart **categorical_bar**, display **combo**. Description: “Released HTTP terminal-duration populations in fixed b00–b10 bins, not exact duration samples or native percentiles.” [Verified query](https://ui.honeycomb.io/possums/environments/prod/datasets/metrics/result/A3wR6HQyw6V). All eleven bins returned; populations matched the HTTP terminal total at qualification.
6. **Generation first-output bins** — SUM(`possums.generation.first_output.bucket`), breakdown `bucket`. Chart **categorical_bar**, display **combo**. Description: “Dispatch-to-first qualifying output among terminal generations with observed output; missing output is not a zero-duration observation.” [Verified query](https://ui.honeycomb.io/possums/environments/prod/datasets/metrics/result/BeuKSe1dUNB). Returned b03/b04 populations with true zeros in the other emitted bins.
7. **Gateway-process CPU cores used** — AVG(`possums.resource.cpu.used`). Chart **line**, display **chart**. Description: “Complete one-minute mean process CPU cores used, not CPU percentage or model/enclave utilization; gaps remain unavailable.” [Verified query](https://ui.honeycomb.io/possums/environments/prod/datasets/metrics/result/hcQKbvgQhbK). Returned numeric process values and gaps.
8. **Gateway-process approximate RSS (MiB)** — AVG(`possums.resource.memory.used`) named `rss_bytes`; formula `rss_mib = $rss_bytes / 1048576`. Chart **line**, display **chart**. Description: “Complete one-minute mean approximate process RSS, converted from bytes to MiB; not a capacity, peak or worst-case memory bound.” [Verified query](https://ui.honeycomb.io/possums/environments/prod/datasets/metrics/result/35BBLsJwU58). Returned numeric process memory values.
9. **Released occupancy sampling populations** — SUM(`possums.admission.occupancy.bucket`), breakdown `lane`, `bucket`. Chart **none**, display **table**. Description: “Full emitted occupancy-bin populations for touched lanes; samples are not distinct permit lifetimes, requests or users. Untouched/missing lanes are unavailable.” [Verified query](https://ui.honeycomb.io/possums/environments/prod/datasets/metrics/result/CsGZhpGpvju). Returned connection/control/generation/heavy/ingress distributions, including emitted zero bins.
10. **Configured application admission capacities** — MAX(`possums.admission.capacity`), breakdown `lane`. Chart **categorical_bar**, display **combo**. Description: “Configured shared limits: connection 64; generation/heavy/ingress 4; new-chat/control 1. Not current free permits, CPU/memory capacity or a separate generation quota.” [Verified query](https://ui.honeycomb.io/possums/environments/prod/datasets/metrics/result/9WWgEX8jSyk). Returned all six configured lanes.
11. **Released admission rejections** — SUM(`possums.admission.rejected`), breakdown `reason`. Chart **bar**, display **combo**. Description: “Observed closed pre-reservation admission causes; duplicate submissions and post-reservation generation failures are not added here.” [Verified query](https://ui.honeycomb.io/possums/environments/prod/datasets/metrics/result/jMezULSuxrR). Returned a request-capacity rejection population.

## Layout

12-column grid: full-width introductory text (height 7); rows of panels 1/2, 3/4, 5/6 and 7/8 (each width 6, height 5); full-width occupancy table 9 (height 7); panels 10/11 side by side (width 6, height 5). Read health/outcomes first, latency next, process resources next, admission detail last.

The exact creation arguments are also preserved in [`telemetry-board.json`](telemetry-board.json), without metric row/series archives. The query/group bounds cover the closed schema without silently discarding categories: endpoints 16; HTTP terminal categories 18; generation model/outcome-stage pairs 36; delivery model/outcome pairs 9; duration bins 11; occupancy cells 60; capacity lanes 6; rejection reasons 14. Additional generation-start/duration queries qualified lifecycle populations but are not added as redundant panels. No board or secondary metric archive exists at this checkpoint.
