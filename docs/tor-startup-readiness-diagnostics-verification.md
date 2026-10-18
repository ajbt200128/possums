# Finite Tor startup-readiness diagnostics — source only

Policy: [startup exception](../PRIVACY.md#startup-diagnostics-exception), [forbidden data](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data), [processor/retention/shutdown boundaries](../PRIVACY.md#processors-retention-access-and-shutdown), [required evidence](../PRIVACY.md#required-evidence-for-every-telemetry-change). Earlier local-initialization evidence remains in [startup diagnostics](startup-diagnostics-verification.md).

## Narrow contract

This fills only the gap after successful local launch/initialization and before first SDK Running. It reuses the **same** `status_events()` consumed by `engine_status`; no second client, stream decoder, SDK logger, peer/request hook or probe is added. Original startup ownership, nine-record limit and transition metrics are unchanged. The observer is created only on the opt-in Tor serving path. Disabled telemetry short-circuits before credential lookup and constructs no transport/TLS/HTTP work; platform stderr is separate.

One bounded watch cell holds closed onion state/problem categories and a latched first terminal with coarse elapsed bucket. At most two readiness records: waiting at 30 seconds if pending, then first-local-ready, observed engine failure or unconfirmed at 180 seconds. Earlier readiness/failure yields only a terminal. SDK Runtime/DescriptorUpload/Ipt map by variant, never by formatting their contents. Pinned features omit Pow; non-exhaustive/feature additions map to unknown. Status-stream EOF/panic retain distinct closed codes. No raw SDK value enters the cell or encoder.

Elapsed time uses Tokio's monotonic Instant (saturating elapsed): under 30s, 30s to under 60s, 60s to under 180s, or 180s and above. No exact duration/time fields, duration metrics, new metric families/dimensions, dataset or identifiers are added. Existing milestone timestamps remain available separately. An observed Running is historical local SDK evidence, **not current health, verified serving, remote reachability or an attestation/publication guarantee**.

Delivery uses existing official OTLP types, fixed resource/scope, normal Honeycomb US routing/configuration/off switch and the existing bounded transport/response classification. One owned one-second best-effort log attempt per record; no retries, spool, detached task or additional delivery-warning records. Thus the entire new stderr/OTLP record budget remains two. Typed delivery failures are not recursively exported; absent ingestion means unavailable, not successful submission or health. No new metrics are sent by this observer.

The diagnostic future is selected alongside the existing serving/drain future, not awaited inside engine/source polling or readiness updates. Finishing serving drops diagnostics and owned I/O. The 180-second deadline ends observation only; a final delivery attempt may take one further second. It does not stop Tor, change readiness, restart, cancel, infer billing or replay. First Running cannot be overwritten by Broken during stalled waiting delivery. Subsequent engine faults retain the original serving/drain policy without further readiness logs. Connection permits, bridge pairs, JoinSet ownership, upstream closure, wire contracts and health predicates are unchanged.

## Offline evidence

Pinned devenv Rust/Cargo **1.92.0**, Darwin arm64; dependencies offline/locked, isolated copy-on-write target directory (not the shared worktree cache):

- `cargo test --offline --locked --all-targets --all-features`: **602 passed**, zero failed; four funded/live fixtures and the operator-only sandbox Tor fixture ignored.
- `cargo test --offline --locked --all-targets`: **531 passed**, zero failed; four funded/live fixtures ignored.
- Package-only formatting, strict all-target/all-feature Clippy and `git diff --check` passed. Unchanged vendored Tinfoil unused-method warning remains.

New synthetic tests cover ready before waiting; waiting then ready/failure under stalled export; complete first-terminal preservation; duplicate terminal suppression; never-ready deadline and inert later observation; all monotonic bucket boundaries; off-path no credential reads/transport; typed SDK projection with hostile nickname canary and explicit unknown output; fatal/EOF/panic engine policy; original readiness becoming true **after** observation expiry and clearing on failure; actual pending-connect export not delaying SDK Running/readiness or engine failure, plus stalled diagnostics not delaying source-failure/shared drain; actual TLS HTTP/OTLP captures of waiting then latched ready; actual pending-I/O drop with no late writes/retry. Generated protobuf comparisons pin resource/scope, seven closed attributes and all other LogRecord defaults (no IDs, trace context or enrichment). Captured HTTP headers retain the existing sealed transport allowlist and normal log route; credential canaries are absent from payloads. Existing accounting, streaming, readiness, admission, bridge, privacy and startup-owner suites passed.

Synthetic example bodies (not production observations):

- `event=possums.startup_readiness outcome=waiting code=tor_startup_readiness_waiting onion_state=bootstrapping problem_kind=descriptor_upload elapsed_bucket=30s_to_under_60s constraint=first_sdk_running_not_observed_within_diagnostic_window action=review_startup_evidence_offline; diagnostic_only; no retry or service_outcome_change`
- `event=possums.startup_readiness outcome=first_local_ready code=tor_startup_first_local_ready onion_state=running problem_kind=none elapsed_bucket=30s_to_under_60s constraint=observed_sdk_running_once_not_current_health_or_verified_serving action=review_startup_evidence_offline; diagnostic_only; no retry or service_outcome_change`

## Limits and handoff

No Linux image/CI, installed/deployed binary, real Tor bootstrap/publication, actual configuration, Honeycomb ingestion/key capability/retention, platform stderr capture or live inference was qualified. No remote mutation, release, cancellation, retry or production reconciliation was performed. Parent owns independent review and all release/approval/reconciliation operations.

Timer coverage depends on executor progress, not a hard wall-clock delivery guarantee. The SDK can coalesce states before our single existing watcher observes them. Process loss, signal, cancellation, unavailable configuration/network or blocked stderr can lose observation/delivery. Serving completion can drop even latched but not yet delivered terminal evidence, including an immediate engine failure with an empty drain; no detached final upload or artificial drain delay is introduced. No fabricated interrupted/success record or healthy zero replaces missing evidence. Standard fixed-host DNS may outlive attempt cancellation without credentials/payloads; whole-process transience/exit latency is not guaranteed. Honeycomb/provider-default and platform/host history/access/retention boundaries remain separately unverified; no anonymity claim follows from coarse startup timing.
