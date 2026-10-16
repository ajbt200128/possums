# Startup diagnostics — source candidate

## Scope and status

Policy: [startup-only exception](../PRIVACY.md#startup-diagnostics-exception), [processor boundaries](../PRIVACY.md#processors-retention-access-and-shutdown), [telemetry plan](telemetry-plan.md#startup-diagnostics-extension).

Operator choice: platform and Honeycomb startup logs plus aggregate startup metrics; **normal dataset**, no special startup dataset. Startup traces were acceptable, but no tracing framework is needed for this packet. Reuse normal service routing and provider-default MVP retention. Actual log-key permissions, ingestion, effective retention, platform capture/access and deletion are unverified. No new credentials, dashboard or archive.

The source candidate and offline evidence below are implemented/qualified locally. This is **not deployment, live ingestion or platform capture evidence**. The v0.0.48 update crossed the mutation boundary and failed completion validation; candidate failure/cleanup/exclusivity and serving remain unresolved. Reported platform `containers`/`shim` failures do not establish the initiating cause. The authorized five-GET investigation budget is exhausted. No live calls, inference, update, replay, cancel, accept or rollback are part of this packet.

## Minimal implementation contract

- One startup owner after the healthcheck branch, before application configuration parsing; finite stage milestones and one terminal outcome.
- Closed typed causes with locally authored constraint/action messages, no arbitrary error/config strings. Preserve supported inference/Tor cause distinctions and unknown fallbacks.
- Stages cover configuration, accounts, signals, optional Tor preparation, verified upstream connection, TCP bind and optional Tor initialization. End before connection acceptance; do not instrument peer/request/runtime callbacks.
- Local initialization completion is not verified serving/onion reachability/settlement. Signal interruption observed before initialization completes is not failure or successful initialization. The terminal snapshot is committed before best-effort export; a later signal during export does not retroactively undo completed local initialization or establish that any request was accepted. Shutdown is checked before the terminal handoff; finite delivery can delay subsequent shutdown polling by up to its shared budget.
- Emit bounded platform stderr records and terminal Honeycomb log delivery, then consume the owner. Keep existing request metrics runtime and shutdown semantics.
- One startup lifecycle count family, closed stage/outcome partitions, observed transitions once, no overlapping total/healthy zeros/durations/request dimensions. Startup snapshot is separate from complete five-minute request windows.
- Reuse fixed Honeycomb US direct TLS transport, generated official OTLP messages, explicit resource and scope, no default discovery/context capture/log bridge. No new dataset header.
- Logs then metrics, one attempt per signal within one finite shared deadline; no spool/retry/detached HTTP driver. Dispose owned work before returning and preserve primary startup exit/failure. Export failure gets at most a closed local warning, never recursive export.
- `OTEL_SDK_DISABLED=true` disables Honeycomb signals; stderr remains subject to platform capture. Missing/invalid telemetry config does not change service startup outcome.

## SDK source findings — not runtime verification

Pinned Rust OTel 0.29 sources expose supported generated OTLP log messages. `SdkLoggerProvider::build` eagerly evaluates `Resource::builder().build()` through `unwrap_or`, even when an explicit resource exists. Normal SDK log emission can capture current trace context. Avoid that provider/emission path; construct official generated OTLP messages from the sealed startup projection instead. Reuse supported transport/protobuf types, not a custom wire protocol.

## Local source and offline evidence

Pinned Rust **1.92.0**, local Darwin host, no external service traffic:

- `cargo test --offline --locked --all-targets`: **527 passed**, zero failed; four existing live tests ignored.
- `cargo test --offline --locked --all-targets --all-features`: **582 passed**, zero failed; the same four live tests ignored.
- `cargo fmt --package possums -- --check`, `cargo clippy --offline --locked --all-targets --all-features -- -D warnings` and `git diff --check` passed. The vendored Tinfoil crate retains its pre-existing unused-method warning; project strict Clippy passed.
- Active LSP probe of eight changed Rust implementation/test files: eight clean, zero error diagnostics, no unsupported/unavailable/inconclusive files.

Focused evidence:

- `src/telemetry/startup/tests.rs`: generated protobuf decoded and compared field-for-field, including absent IDs/enrichment/exemplars; only fixed normal service/resource/scope. Real local TLS/HTTP captures verify `/v1/logs` then `/v1/metrics`, the sole sensitive ingestion header and **no dataset or tracing header**. Existing metric transport regressions remain covered by the full suites.
- At most eight distinct stage entries and one terminal observation; repeated/post-terminal calls inert. Delta transition-count cells are disjoint, value one, only stage/outcome, with no request families or overlapping totals. No generic diagnostic/error/string API.
- Actual local TLS shutdown, paused-time stalled-connect and cancelled-finish tests verify the shared one-second deadline, owned I/O disposal, no replay, first delivery-failure preservation and consumed terminal state. Late shutdown cannot reopen a completed initialization snapshot. Outage cannot replace the original startup result.
- Official OTLP empty-success, partial rejection/warning, malformed/oversized responses and non-200 HTTP paths are exercised with hostile synthetic response text. Only closed local delivery diagnostics escape; no response text is echoed.
- `tests/startup_binary.rs`: four actual binary process fixtures pass in both configurations: missing config (exit 2), invalid accounts with hostile credential/config canaries (exit 3), invalid Tor opt-in (exit 10), non-Unicode config (exit 8). Export disabled/malformed paths remain local; canaries are absent from stdout/stderr, no initialization-complete claim appears. No provider/Tor construction or live inference is needed for these early failure fixtures.
- `src/main.rs` tests preserve typed nested failures and exclude post-startup serving/cleanup failures. Pending initialization is finalized **before** independent telemetry cleanup, preventing cleanup failure from erasing an observed launch failure/interruption. Existing nonzero exit codes remain unchanged.
- `src/tor_ingress/tests.rs`: successful local launch invokes the initialization handoff once after identity/launch, while identity failure, launch failure and observed shutdown never invoke it. Existing first-fault, reporter panic, admission/drain and bridge tests pass unchanged apart from the new no-op initialization parameter.

Independent read-only Luna review round 1 raised late-shutdown wording. The misleading interruption constraint was corrected to `shutdown_before_initialization_complete`, the snapshot boundary documented, and cancelled-completion coverage added. Round 2 accepted that local initialization is committed before export, not evidence of request acceptance/serving, and reported no blocking findings. Parent separately fixed/tested cleanup masking and inspected the real source diff. Reviews did not perform live probes.

Coverage is scoped: no end-to-end boot fault injection for every external SDK stage, actual panic/OOM/pre-main process, Linux image or mounted platform configuration was performed. Closed typed mapping/owner tests and existing SDK/Tor fixtures are not a claim that every real platform failure is captured. No shipped client wire contract was changed.

## External reporting limits

Pre-main, Tokio runtime construction, panic/abort/OOM/SIGKILL, blocked/lost stderr and process shutdown can prevent observation/delivery. A platform failure before the executable runs cannot be explained by its logs. Standard fixed-host DNS may outlive a dropped attempt; whole-process transience/exit timing is not guaranteed. No public callback, readiness probe or inference is added. HTTP acceptance is not observed Honeycomb ingestion, effective retention or serving acceptance. Native host history and platform persistence remain separate boundaries.
