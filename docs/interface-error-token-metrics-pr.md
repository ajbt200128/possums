# Draft PR: preserve typed interface failures and qualify bounded error/token aggregates

## Summary

- Preserve closed, prompt-free failure causes through API, auth/accounting, preflight/terminal owners, catalog SDK acquisition, HTTP/body/connection, startup/health and cleanup boundaries. Give bounded code/stage/constraint/action diagnostics without raw SDK/upstream/user text or inference replay.
- Count originating failures once; preserve independent later accounting/encoding/delivery faults without replacing the primary. Ordinary duplicates, voluntary disconnect/cancellation and orderly shutdown are not faults. Keep absorbing settlement, authenticated completion/tool gates, quote/max-reservation and receipt semantics intact.
- Add fixed complete-five-minute integer DELTA `possums.interface.errors` and `possums.tokens.{input,output,cached}` materialization. Closed dimensions only: 54 `error_type` values or the three authenticated qualified model partitions. Unknown cache is unavailable, not zero; final/last cumulative usage is observed once, never summed snapshots or tokenizer estimates.
- **Both production error/token qualification gates remain false.** This PR is a reviewed source candidate, not new production export activation or deployment.

## Evidence

- Fresh isolated pinned Rust 1.88 / aarch64-darwin full suite: **467 passed; four paid/live probes ignored**; nine offline Python image-health runner guard tests passed (no Docker invocation). Scoped formatting, all-target/all-feature Clippy and diff checks pass. Unchanged vendored formatting/dead-code limitations are recorded rather than cleaned up here.
- After resolving conflicts with catalog-thinking main `eaf8f57`, fresh isolated pinned Rust regression passed **475 tests** (four paid/live ignored), plus nine Python guards and strict scoped Clippy. Typed reasoning refusal preserves main's exact wire message; authenticated gating/options forwarding and expected-interruption ownership remain intact. See the separate merged-main verification record.
- Real synthetic loopback EOF, parser/header/body/deadline and ownership tests; hostile error-text exclusions; complete-window/off/epoch/contention/integer/cache-absence, SDK conversion and local TLS tests. Independent reviews found three issues (upload EOF fault, generic non-API body diagnostic, legacy catalog validation category); corrections and regression assertions were reviewed.
- Fresh requested-allocation qualification: 2,813 points / 377,262-byte wire; 4,482,262-byte composed TLS incremental peak below the retained 8,388,608-byte library ceiling. This is not RSS or live runtime evidence. An unexpectedly reused older shared-target executable was discarded; final tests use an isolated target.
- Exact installed decoder AST fixtures: eight error decoders and three newer native-usage decoders. Older strict clients reject new metadata, so no `interface_failure` wire field is added; optional existing `message` carries diagnostics. Older clients ignore messages; no enhanced host display is claimed.

## Blockers and limits — not ready for release

- **Scoped Linux CI qualification passed:** [run 38343281159](https://github.com/ajbt200128/possums/actions/runs/38343281159), source `3f22da4`, passed all three `flake`, `image` and `image-startup` jobs, including packaged production health against an isolated synthetic fixture and read-only startup. This clears the local builder blocker for the Linux CI candidate, not deployed/live metrics qualification or successful local aarch64-linux compilation.
- v0.0.11–v0.0.15 error envelopes accept the optional message, but their pre-native stream decoders reject the current native-usage fixture. The stream/receipt serializer is unchanged by this patch; these artifacts are not qualified for current streaming. Do not interpret error-only tests as whole-client compatibility or weaken tool/receipt gates.
- Production origin-bound SDK send/attestation, external SDK/host-only failures, installed/deployed behavior, live ingestion/retention and invoice parity remain unqualified. Legacy-v2 freshness and native-v3 platform-signer limitations remain unresolved.
- The operator subsequently authorized logical commits and draft PR publication solely to run existing Linux CI. No merge into main, client installation/activation, deployment, dashboard change, external export activation or paid/live inference was performed. The operator assigned post-release dashboard implementation to this agent, dependent on verified qualified release and metric enablement; it is not a backlog suggestion. No `no-rollout-needed` label: backend wire messages and failure behavior change and require normal rollout review.

## Privacy and review record

See [verification](verification.md#typed-interface-errors-and-token-aggregates--source-candidate), [inventory](interface-error-inventory.md), [implementation plan](interface-error-token-metrics-plan.md), and [PRIVACY.md](../PRIVACY.md#typed-interface-failures-and-token-aggregate-extension), especially forbidden signals, complete aggregation, shutdown/retention and required evidence. No request-level telemetry, raw diagnostics, cost export, user/session identifiers, cache scope or support bundle is added. Native host history, local build storage and image-engine metadata are separate persistence boundaries.

## Commit organization

The authorized split separates policy, closed failure/catalog/encoding foundation, complete-window SDK materialization, necessary atomic API/preflight/terminal carrier wiring, HTTP origin ownership, independent startup/health diagnostics, and compatibility/qualification evidence. No unrelated cleanup is included. The carrier/API refactor must land atomically to retain matched source and wire projections.
