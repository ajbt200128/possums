# Startup diagnostics — source candidate

## Scope and status

Policy: [startup-only exception](../PRIVACY.md#startup-diagnostics-exception), [processor boundaries](../PRIVACY.md#processors-retention-access-and-shutdown), [telemetry plan](telemetry-plan.md#startup-diagnostics-extension).

Operator choice: platform and Honeycomb startup logs plus aggregate startup metrics; **normal dataset**, no special startup dataset. Startup traces were acceptable, but no tracing framework is needed for this packet. Reuse normal service routing and provider-default MVP retention. Actual log-key permissions, ingestion, effective retention, platform capture/access and deletion are unverified. No new credentials, dashboard or archive.

The original source packet and offline evidence below qualify local behavior, **not live ingestion or platform capture**. Later separately authorized release/recovery observations are recorded below; they do not retrospectively qualify the original packet. Reported platform `containers`/`shim` failures do not establish the initiating cause. The original five-GET investigation budget is exhausted.

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

## Authorized v49 attempt and closed diagnostic limits

- PR #84 merged as tested source `369cf78ae9c131ba6dc36863a8b15634e02220ac`. Main CI `38937870068`, independent image publisher `38941819988` and tagged CI `38943994216` passed. Immutable v0.0.49 release `f0e6af2f8b135a5bbf95b922b39606478a4437d4` has that source as sole parent and changes only the image pin to `sha256:9d34c323414b3f3361814a7f0daa7bff293ebe8fc40edc20301e12ff789f4433`. Independently verified signed public manifest digest: `5b67c58e362d665791df7f68d2d52c7baf3d6f529b67b762d93dcdfc6e20b2fe`.
- Before this new attempt, one separately authorized plain v48 cancellation was acknowledged HTTP 204. Readback reconciled current v43/running with no pending update and unchanged configuration. This was metadata evidence, not physical termination, secret erasure or exclusive publication evidence. The documented first-activation admission checks were retained; no already-Tor-bound update path was enabled.
- Production `38949086608`, attempt 1, passed preflight. Public provenance, current main, existing production policy and exact operator review were checked before one protected approval. The production job failed `RELEASE_TOR_BINDING_COMPLETION` after the mutation boundary. No successful completion or serving receipt was obtained; no replay or rollback followed.
- A separately bounded three-GET packet then reported current v43/running, pending v49/candidate failed, and strict `has_update=true`. Known latest/debug/CC/hold/domain constraints and legacy references were preserved; neither Tor reference appeared in the saved current view. That view is **not proof of the candidate's mounts**. The budget is exhausted. Initiating cause, runtime cleanup and exclusive publication remain unknown.
- Read-only Honeycomb metadata showed no columns in `prod/possums-gateway`, and the complete 18-item `prod/metrics` schema did not include `possums.startup.transitions`. This is scoped schema visibility, not proof that the executable never started, that submission failed, or that a key lacks log capability. No event bodies, inference or new export was requested. Effective log retention/permissions and actual startup ingestion remain unverified.
- Pinned CLI [container commands](https://github.com/tinfoilsh/tinfoil-cli/blob/025a7c879ad806c9d7309940f1467f3db21cce24/container.go), its README and the [Admin API](https://docs.tinfoil.sh/admin/admin-api.md) did not establish a supported container-stdout retrieval interface. MODELWRAP logs and resource metrics are not substitutes. No guessed endpoint, SSH or callback was used. Obtain a documented interface or operator inspection of only the closed startup records; never paste raw platform errors/configuration into support artifacts.

### Asynchronous completion classification correction — source only

The pinned CLI's [`followAndRender`](https://github.com/tinfoilsh/tinfoil-cli/blob/025a7c879ad806c9d7309940f1467f3db21cce24/container_progress.go) explicitly returns the initial response for JSON output without following progress. An offline reproduction against the released helper showed that a valid old-current reference list with an exact-target pending candidate immediately raised the binding-completion error, without using the existing observation period or reaching a later successful fixture. This establishes a wrapper classification bug, **not the cause of v48/v49 candidate startup failure**.

The correction allows only the exact previous reference set while the previous tag remains running with this exact target pending. Final success still requires the exact approved reference set, target running with no pending update, unchanged opaque configuration and approved domain; separate serving verification is unchanged. Unexpected/partial references still fail closed. An observed exact-target `failed` candidate gets closed `RELEASE_CANDIDATE_FAILED`, with initiating cause/cleanup/serving explicitly unknown. One mutation, all admission/plan/provenance guards, the existing 300-second observation deadline and 10-second interval are unchanged; no retries or recovery policy are added. Regression fixtures cover pending-to-completed, failed candidates with either valid reference projection, deadline expiry, configuration drift, wrong target/current/status and partial/unapproved references.

Pinned Python 3.12: **85 release automation/health tests passed**, including the four new regression methods; `git diff --check` passed. Active LSP probes of both changed Python files were clean, with no unavailable/inconclusive files. Independent read-only Luna code/test review reported no blockers. No live calls were used by these regression tests or review.

This correction is not installed or deployed and does not authorize another update or cancellation.
