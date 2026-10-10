# MVP request metrics: complete five-minute aggregates

## Operator decision — 2026-10-09

The operator explicitly selected **Five-minute aggregates only** after disclosure that this permits sparse aggregate counts and histogram bins. This supersedes the ten-observation, touched-lane contributor minimum and linked sparse/complement suppression requirements in the historical telemetry contract. Those controls were not differential privacy. Historical tests, wire fixtures and deployment evidence are not retroactively changed.

Policy: [reviewed MVP release scope](../PRIVACY.md#reviewed-mvp-release-scope), [aggregation and residual risks](../PRIVACY.md#aggregation-is-necessary-not-sufficient), [forbidden data](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data), [processors/retention](../PRIVACY.md#processors-retention-access-and-shutdown), [synthetic testing](../PRIVACY.md#synthetic-testing-exception), [required evidence](../PRIVACY.md#required-evidence-for-every-telemetry-change).

## Current contract

- Keep all existing request metric names, units, closed labels and exact duration/occupancy bucket boundaries from [the schema](telemetry-schema.md#exported-tables-and-encoding). Keep the deployed `.bucket` integer-delta encoding, not native histogram percentile claims.
- Close fixed, non-overlapping **five-minute** UTC-grid request windows. Discard the startup/restart partial window; never publish partial windows or cumulative request snapshots.
- Release active scalar cells and complete populated distribution vectors even when their counts are **one**. Empty families remain absent; absent metrics are unavailable, not invented zero/healthy states.
- Preserve structural integrity: valid bounded enums, checked arithmetic, complete 300 actual occupancy polls, HTTP duration/disposition counts matching HTTP terminals, generation duration counts matching generation terminals, first-output count not exceeding generation terminals, and complete touched-lane occupancy distributions. Lost/invalid observations, clock faults, missed sampling, late closure and materialization failures remain fail closed.
- Keep the existing bounded lifecycle pool/state, real permit-lifetime ownership, final wire allowlist, one owned send attempt, expiry, cancellation and no replay/spool/shutdown partial flush. Metrics cannot alter accounting, inference, delivery or admission behavior.
- Keep CPU/RSS and configured capacities on their independently complete one-minute infrastructure windows. No new capacities, resource scopes, token/cost metrics, collection cadence, resource discovery or log/trace pipeline is approved.
- Continue excluding content, identifiers, URLs/headers, exact request timestamps/durations, exemplars and per-request events. Sparse aggregate release **does not provide anonymity or differential privacy** and may reveal coarse individual activity or support external timing correlation. Keep Honeycomb access restricted and retain the accepted provider-default retention/unknowns.

## Correctness follow-up

Complete the separately documented API control-route rejection hooks using the existing typed request context, recording a sole actual fail-fast boundary rather than inferring rejection from response status. Preserve `input` versus `auth` for API header policy, typed API input, model/catalog/trust and auth-state failures. Ordinary 404s are not rejections; anonymous home-page rendering was historical. Response, authentication and accounting behavior must remain unchanged.

This follow-up improves classification; the deployed absence of request columns was not independently attributed to it. Removing sparsity gates should permit low-count *complete valid* windows, but cannot guarantee delivery when sampling, source integrity or export fails.

## Verification and rollout status

Local implementation is qualified; rollout and live request-query verification remain pending. The deployed gateway remains v0.0.15 with its previous release behavior until a measured update is verified. No paid/filler production traffic is authorized.

`RequestTables::releasable` no longer rejects small counts, bins, contributor populations or complements; it retains validity, 300 samples, checked/matched lifecycle/distribution sums, first-output count ≤ generation terminals, touched-lane completeness and nonempty-family requirements. Exact source and decoded-wire oracles verify 0 absent and valid 1/9/10/11, rare model/endpoint/error/bin, missing-output and duplicate partitions released under the same production and synthetic contract. Invalid pairs, overflow, incomplete sampling, loss, clock/off/warmup/restart and ownership/no-replay negatives remain.

Real router tests cover sole typed API/web control rejections (historical local scope), no generation starts or balance changes, and ordinary 404/anonymous home non-rejections (historical browser scope). A production-mode router-to-loopback TLS test now drives normal warmup/all window polls and verifies sparse 1/9/10 HTTP/control counts on the wire with boundary timestamps, exact integer totals/names and absent synthetic sentinels; 0 remains absent. This is local qualification, not paid inference or live backend evidence. Existing real generation/delivery ownership and accounting tests retain their separately scoped coverage.

Parent `devenv test` passed formatting, strict all-target/all-feature Clippy, the complete locked Rust suite with funded tests ignored, and Go helper tests. Active LSP probes of nine changed Rust paths found no errors/warnings, only one pre-existing informational let-chain suggestion. Independent read-only review found no supported defects and successfully reran both control-rejection tests after correcting an initially empty test filter. `git diff --check` passed. Historical native CLI v3, legacy-v2 freshness, platform privacy and live billing unknowns remain unresolved. Client fixes from the concurrent Pi thread are retained independently and are not telemetry evidence.

Fresh read-only Honeycomb checks found no request-family columns in `prod/metrics`. Metric-scoped attributes remain allowlisted for process CPU/RSS and configured capacity. Verified two-hour, sixty-second aggregate query candidates for the eventual **full** dashboard:

- [Process CPU used in millicores](https://ui.honeycomb.io/possums/environments/prod/datasets/metrics/result/Dihd7TSgjT6): `AVG(possums.resource.cpu.used) × 1000`; not a utilization percentage or model GPU signal.
- [Approximate process RSS in MiB](https://ui.honeycomb.io/possums/environments/prod/datasets/metrics/result/7pLSzQcAuyb): `AVG(possums.resource.memory.used) / 1048576`; not a memory-capacity percentage.
- [Configured capacity by lane](https://ui.honeycomb.io/possums/environments/prod/datasets/metrics/result/qtoeD9aZtUo): verified matching MIN/MAX, six historical lanes: connection 64; generation/heavy/ingress 4; new_chat/control 1. The API-only five-lane target removes new_chat; new wire/deployment verification is pending. Honeycomb OTHER/TOTAL rows are not extra lanes or health signals. These are configured permits, not current occupancy.

No metric-value archive or signed result-download link is retained here. No board exists in the inspected production environment. The operator chose **Full dashboard only**; resource-only creation remains unapproved. Request queries must be run and checked against the deployed metric types/attributes after eligible data arrives. Then preview the exact full board and obtain approval before creation. Missing request populations remain unavailable, never fabricated success, error rates, latency or saturation.
