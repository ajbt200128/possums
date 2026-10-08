# First-pass telemetry: four golden signals, privacy first

**Status:** implementation in progress; original plan 2026-10-07, MVP decisions updated 2026-10-08. Local aggregation, encoding, transport and handoff qualification are recorded in [`verification.md`](verification.md); the gateway is not yet emitting telemetry. **Read [`../PRIVACY.md`](../PRIVACY.md) before implementing or changing any part of this plan.**

## Current MVP decisions

- The operator authorizes deployment to the **existing production gateway**; a separate isolated test instance is not required. Normal build, attestation, accounting, runtime and privacy verification still apply.
- Use the supplied Honeycomb **`dev`** configuration in `.env` for the MVP; never display/commit credentials or indiscriminately pass inherited OTEL defaults into the gateway. Read-only MCP on 2026-10-08 lists `dev`, `prod` and `fun`, all empty. A separate `prod` telemetry destination is not a prerequisite and was not selected merely because it exists.
- **Provider-default retention is accepted for the MVP.** Verify/disclose the effective period when possible; custom seven-day retention is deferred, not a blocker. Unknown deletion/backup or transport/audit properties remain unknown, not automatically verified by this acceptance.
- Deployment permission does not enable the synthetic-only schema on real or mixed traffic. Keep those exports off until the reviewed real-traffic release mode is ready; do not flip `IsolatedSynthetic` or retain a misleading `test` workload label on production. Continue isolated local fixture tests without requiring a new live instance.
- These decisions supersede earlier separate-instance, no-production-deployment and seven-day-retention prerequisites in historical packet records, not their measured evidence or remaining content/identifier protections.

## Recommendation and scope

Use **explicit Rust application metrics → local, measured OpenTelemetry Collector → Honeycomb US**, plus Tinfoil's existing resource metrics if they meet our needs. Use the configured Honeycomb **`dev`** destination for the MVP, with local synthetic qualification before the authorized production deployment. No traces, logs, per-request records, GenAI content instrumentation, profiling, billing analytics or OBI in the first pass.

Organize the dashboard around Google's **latency, traffic, errors and saturation** [1]. Prioritize model reliability and speed, endpoint errors, and CPU/memory headroom for each gateway. Use the same safe metric schema in testing that we intend to review for production later; do not prototype with request traces and promise to sanitize them afterward.

Two explicit compromises:

- Granular model/endpoint/instance views are useful but not automatically private. Validate them with synthetic traffic first; real-traffic export remains gated on sparse-data and correlation review.
- CPU/memory belong to gateway/container resources, not individual models or endpoints. The gateway calls remote Tinfoil inference; its CPU utilization is **not the model's GPU utilization**. Model/endpoint saturation means active work and admission pressure, not fabricated CPU attribution.

## What we found

### Initial repository findings (2026-10-07, before implementation)

- `src/telemetry.rs` contains four allowlisted lifetime counters and a minimum-export-count of ten. It has no fixed windows, resource/datapoint sanitization or OTLP exporter. Repeated cumulative snapshots can reveal small increments after the threshold. **Do not just wire this scaffold to Honeycomb.**
- `Cargo.toml` currently has no OpenTelemetry dependency. Prefer supported Rust OTel SDK/exporter types and existing resource-collection libraries when implementing; custom code should cover the privacy release policy and gateway lifecycle, not reimplement OTLP.
- `tinfoil-config.yml` pins CVM `0.14.12`, requests **2 CPUs / 8192 MB for the enclave**, runs one gateway, and declares no per-container CPU/memory limit or Collector. Do not treat that enclave allocation or the application's 512-MiB allocation target as a verified gateway cgroup limit/RSS cap. The existing egress allowlist does not include Honeycomb.
- Read `src/main.rs`, `src/web.rs`, `src/api.rs`, `src/generation.rs`, `src/generation_owner.rs`, `src/stream_owner.rs`, `src/streaming_chat.rs`, `src/api_stream.rs`, `src/inference.rs`, `src/inference/stream.rs` and `src/accounting.rs` before placing observations. Export must not own accounting transitions or detached-generation lifetime.
- `docs/phase0.md` and `docs/verification.md` retain historical `v0.0.5` buffered/delivery-owned charging evidence and later streaming release notes. Do not use the old behavior to define current generation completion. API instrumentation applies only to releases that actually enable the separate Phase 0.1 API; do not expand Phase 0's route surface for telemetry.

### Honeycomb inspection (read-only MCP)

On 2026-10-07, `get_workspace_context` reported team `possums`, an existing **`test`** environment with **zero datasets**, and no telemetry sent yet. The system Activity Log environment is not an application telemetry environment. There was no data to query, and no board/query was created.

This historical MCP result did **not** establish data-region placement, key permissions, native metrics support, effective retention or backup deletion behavior. The 2026-10-08 MVP decision accepts default retention; seven days is a future goal. Honeycomb's public documentation says 60 days for most customers [8], not necessarily this account. Verify remaining capabilities and distinguish provider-default acceptance from observed retention/deletion evidence.

### What Tinfoil provides

Official documentation [2–5] exposes:

| Capability | Evidence | How we should use it |
|---|---|---|
| Resource utilization time series | `GET /api/containers/:id/metrics`; CLI `tinfoil container metrics <instance> --time 24h` | First candidate for per-gateway CPU/memory. API returns `data_points` and `interval`, average/max CPU/GPU utilization, average/max CPU/GPU memory utilization and total CPU/GPU memory. Percentages are documented; validate memory units and capacity scope. |
| Scoped container administration | `containers.metrics.read` action; other discovery reads may be separately necessary | Request a least-privilege metrics reader. Keep it outside the prompt-serving gateway; never use an unrestricted billing/admin key merely to read health. |
| Lifecycle/readiness | Instance lifecycle API/CLI; runtime healthchecks and `/tinfoil/container-status.json` | Distinguish deployment/process health from successful inference. Healthcheck output can appear in platform errors: keep checks content-free. |
| Billing/usage summaries | `/api/billing/usage`, `/api/billing/time-series`, including key/model groupings | Useful provider capability, **excluded from pass one**. These are not latency/error metrics, and provider key/account labels are not safe telemetry dimensions. |
| Public service status | `status.tinfoil.sh` | Context for provider incidents, not evidence that a particular model/request succeeded. |

**Not verified:** live access in this account; reporting cadence/lag; retention/deletion; whether “container” means the whole deployed enclave/instance or each named workload; memory units/denominator; whether CPU percentages are normalized across all CPUs; cgroup throttling/OOM/restart coverage; monitoring agent placement and attestation coverage. The gateway's pinned CVM may not support every feature in today's docs. No Tinfoil admin API was called and no secrets were inspected during this research.

Before choosing a resource collector, inspect the authorized existing instance's metrics read-only and ask Tinfoil to resolve those questions. Use local fixtures for load/normalization tests first; do not run disruptive CPU/memory stress or add a second workload to the serving gateway merely to infer scope. Documentation is evidence of an API, **not evidence of its behavior on our deployment**.

## Questions, metrics and denominators

Names below are proposed custom instruments, not existing Honeycomb fields. Metric schema review must pin units, types, labels and limits. Use fixed-bucket distributions; do not export precise per-request samples, histogram exemplars, individual min/max durations or raw duration sums.

| Golden signal | First-pass measurements | Answers / interpretation |
|---|---|---|
| **Traffic** | `possums.http.requests` and `possums.http.completed` window counts; `possums.generation.started` and `possums.generation.completed` window counts | Requests/s and generations/s by endpoint, gateway slot and validated model. Duplicate submissions count as HTTP traffic, never as a new generation. Started/completed are different cohorts; do not use same-window completions/starts as a success rate. |
| **Latency** | `possums.http.duration` fixed-bucket histogram; `possums.generation.duration`; `possums.generation.first_output` | HTTP service/delivery lifetime, admitted-generation-to-terminal-outcome, and time from upstream dispatch to first non-empty displayable text/reasoning or tool-call delta. Keep success/failure separate. This last metric is **not a true tokenizer TTFT** and excludes pre-dispatch verification. Exclude missing first output rather than record zero; show its sample count. |
| **Errors** | `possums.http.completed` by coarse status class; `possums.generation.completed` by closed terminal outcome and failure stage; a separate `possums.delivery.completed` family | Overall/instance/endpoint/model terminal-failure counts and rates. HTTP 200 can precede an invalid stream or settlement failure. Separate downstream interruption from generation failure, pre-generation rejection and confirmed accounting outcome. |
| **Saturation** | Windowed active-generation occupancy and configured lane capacity; admission rejection counts; connection/request-memory admission occupancy where supported; CPU use/capacity and memory used/capacity per gateway | What is full and where? Report fail-fast rejection rather than inventing a queue. Per-model active work shares a global limit: there is no model-specific capacity denominator unless one actually exists. |

Lifecycle rules:

- Exactly one HTTP terminal observation per accepted request lifetime, and exactly one generation terminal observation per actual generation. Count transport/header rejection separately when the router has no endpoint/model; do not force it into a matched handler. Dropped connection admission may not produce an HTTP status.
- Measure generation completion in the worker that observes verified upstream termination and the actual ledger result, **not** when response headers are sent or the browser disconnects. Continuing upstream after disconnect still occupies a lane. Refund is not automatically an error; do not classify it without its cause. Unknown settlement stays unknown.
- Keep `delivery` (`completed`, `interrupted`, `unknown`) separate from generation outcome; server transport completion is not proof of browser receipt. Do not add delivery failures, HTTP failures and generation failures together as if they were disjoint requests.
- Service error percentage: failed HTTP completions / all HTTP completions for that family/window. Show 4xx/admission rejection separately from server failures. Generation failure percentage: failed generation terminals / all generation terminals, with unknown outcomes shown separately. At zero or suppressed denominator, the rate is unavailable.
- Suggested closed failure stages: `admission`, `verification`, `catalog`, `encoding`, `transport`, `decoding`, `validation`, `terminal_usage`, `settlement`, `internal`. Map existing content-free errors deliberately; never use error text or the full diagnostic-code space as labels. Rejections before model validation use `unknown`, not an attacker-supplied model string.
- Do not instrument every function. Add observations at admission, upstream dispatch/first output, worker terminal accounting, and downstream completion/drop. Metric failures must never unwind or hold an accounting lock across I/O.

### Dimensions: useful, bounded, not arbitrary

Resource labels: fixed `service.name=possums-gateway`, an accurately declared source deployment environment, and operator-supplied `possums.gateway.slot` (initially one slot). The frozen local fixture schema uses `deployment.environment.name=test`; a reviewed production schema must identify production honestly even when the Honeycomb destination is `dev`. Keep raw instance UUIDs, hostnames, IPs, repository paths and provider resource IDs out of OTLP; local operator inventory maps slots to instances. Do not add a fresh UUID on every restart. Release evidence can map the test run to a version without putting arbitrary resource metadata into every point.

Datapoint labels, only where relevant:

- `possums.endpoint`: a compiled handler enum such as `chat_web`, `chat_api`, `models`, `login`, `logout`, `recovery`, `claims`, `attestation`, `other`. Not a raw path or route extracted from hostile input. API values only exist when that API is enabled.
- `possums.model`: canonical names from the authenticated catalog **intersected with a reviewed telemetry label map**; use `other`, `unknown`, `not_applicable` as needed. New model IDs do not automatically become external labels. An unmapped model remains fully available for inference; only its telemetry groups into `other`.
- Closed `status_class`, `outcome` and `failure_stage` sets on the specific families that need them. Do not attach every dimension to every metric.

Test instance × endpoint × model combinations on request/generation metrics, with only relevant dimensions on resource metrics. Bound the Cartesian product and bucket count up front; reject/drop unknown attributes. Do not substitute hashed IDs for forbidden dimensions. Metric/resource names and instrumentation-scope metadata are also subject to an allowlist.

### Windowing and sparse traffic

Proposed starting cadence: **five-minute, non-overlapping request windows**, **one-minute infrastructure windows** sampled locally more frequently as needed. All externally visible timestamps represent fixed bucket boundaries. High-frequency resource correlation and all requested cross-breakdowns must be reviewed before real-traffic use.

Use bounded in-memory window state and delta exports, not lifetime counters. On restart, discard partial request windows; do not emit an immediate partial bucket or persist a spool. Request counts, latency distributions and outcome families must be released consistently, not independently filtered so that a published total exposes a suppressed failure count.

For synthetic validation, exercise the existing threshold of ten at 0/1/9/10/11 observations and test rare histogram bins and complementary totals. **Ten is not sufficient protection for real traffic**, especially a one-user test gateway. A production-ready release algorithm must specify which related tables are withheld together or coarsened and how it prevents differencing across all released families. Until approved, request-derived exports are **synthetic-only**; no claim of anonymity and no silent relaxation to populate a dashboard. Postpone token/cost totals and throughput-per-token calculations to reduce the initial privacy surface.

## Resource instrumentation choice

Prefer the least-privileged source that answers the question:

1. **Tinfoil's native resource metrics first.** Inspect them without deploying another observer. If cadence/scope/retention fit, use a small external metrics reader (existing supported client/receiver if available) with a scoped Tinfoil key, mapping only approved numeric fields and configured instance slots to the Collector. No raw API payload export. Keep polling state bounded, deduplicate fixed buckets, and distinguish stale/missing data from zero. Account for collection lag; do not interpolate absent points into healthy values.
2. **In-process process metrics if native data lacks gateway detail.** A supported Rust process-metrics library can read the gateway's own CPU time and RSS without broad host privileges. Separately test access to its own cgroup CPU/memory accounting. Process RSS, cgroup charged memory/working set and enclave memory are different metrics, not aliases. Include attestation-helper and Collector overhead in the appropriate container/enclave view.
3. **A narrowly configured Collector resource receiver only if needed.** The `hostmetrics` process scraper and `docker_stats` receiver already exist [7], but a sidecar may only see itself unless namespaces/mounts are explicitly changed. A Docker socket is administrative access even when mounted read-only; do not expose it or mount the host root merely to get memory numbers. Prefer own-process/cgroup reads over expanding privileges.
4. **Defer OBI.** OTel eBPF Instrumentation is primarily automatic application/network observability. Its documented capabilities include BPF/performance access and, depending on mode, ptrace/network privileges [6]. It can create request traces/network metadata we forbid and cannot infer Possums settlement semantics. Current runtime examples emphasize Go/JVM/Node, not a complete Rust gateway/cgroup solution. Reconsider only for a demonstrated gap, with an explicit privilege and collection review; do not enable it just for CPU/RSS.

CPU utilization: derive cores used from delta CPU-seconds / wall-seconds, then divide by **verified allocated cores/quota** for a percentage. Tinfoil percentages need their own denominator validation; do not apply that formula twice. Memory: show bytes used and the correct container/enclave limit separately. Unlimited or unavailable limits produce `unavailable`, never a fabricated percentage. OOM, throttling and restart metrics are conditional on a verified source; process-local counters cannot observe the process's own death reliably.

## Data flow and controls

1. Gateway code observes typed counts/timers/occupancy only. It has no telemetry API accepting a request, error string, account ID or arbitrary attributes.
2. Local bounded aggregation performs the privacy release decision before OTLP crosses the enclave boundary. Keep genuine OTel histogram bucket populations without individual samples/exemplars; select a supported SDK/export representation that can omit sensitive sum/min/max fields, and verify the bytes on the wire. If the pinned SDK cannot represent this, use bounded bucket-count instruments rather than exporting those fields by default.
3. A pinned **Collector inside the measured deployment** accepts only the local metrics stream. Use a minimal metrics-only pipeline and a final metric/resource/datapoint allowlist; no log or trace receivers/exporters, no debug/file exporter, no automatic resource enrichment, no persistent queues. Do not rely on stock batching/filtering processors to implement privacy thresholds. Protect/private-bind ingestion; do not expose a public `/metrics` or OTLP endpoint through the shim.
4. The Collector exports over verified TLS to Honeycomb US. Use the supplied `dev` environment's ingestion-only key after verifying its scope; do not copy unrelated OTEL headers or defaults. Add only the necessary Collector egress destination; do not grant the gateway unrestricted egress. Bound queues, retries and memory; drop telemetry under pressure instead of blocking inference. SDK/Collector self-diagnostics must not print payloads, headers, keys or upstream errors to platform logs.
5. If using an external Tinfoil reader, sanitize resource metrics at that reader and apply the same final Collector allowlist. An external Collector for those already-sanitized platform values is outside attestation and must be documented separately; never send raw gateway observations to it.
6. Maintain a deployment-level off switch and a tested export kill switch. No shutdown request-window flush, persistent backlog, or replay of inference. Collector/reader health can use content-free infrastructure signals, not rejected payload samples.

Adding a Collector changes the measured image/configuration set and resource budget. Pin it and reproduce/reverify the release. Current Tinfoil networking support, CVM compatibility and enforced egress are deployment gates—not promises derived from YAML.

## First dashboard: “Possums MVP — golden signals”

Use native Honeycomb metrics; confirm `dataset_type=metrics` and each metric's actual attributes after first ingest. Do not guess a dataset slug or build queries before data exists.

- **Overview:** terminal traffic, HTTP server-error rate, generation failure rate, p50/p95/p99 success and failure duration, active-lane occupancy/capacity, freshness and explicit missing-data state.
- **Models:** terminal count, generation failure %, first-output distribution, terminal-duration distribution, failure-stage breakdown and active work by model. No token/cost/user table. Insufficient samples must remain visible as unavailable.
- **Gateways:** CPU/capacity, memory bytes/limit, admission pressure, availability and Collector/reader freshness by slot. Identify the scope/source of each resource panel.
- **Endpoints/errors:** terminal counts and coarse status classes by endpoint; generation failures by stage/model/slot where applicable; downstream interruption shown separately.

Metric queries use `SUM(counter)` for window counts, `RATE` calculated fields for supported rate semantics, and histogram percentiles on the parent metric—not bare `COUNT`, unsupported `RATE_SUM`, or an average of per-instance p99s. A fixed-window count divided by window seconds is also a valid displayed rate. Error-rate numerator and denominator must use the same population/window and be privacy-released together. Five-minute suppressed series cannot support reliable second-by-second alerts.

No paging/SLOs in pass one. Start with dashboard verification and a non-paging telemetry-freshness check; choose thresholds after synthetic load baselines. Later alerts must handle “no data,” avoid sparse activity disclosure in notifications, and receive their own policy review. “How is each model doing?” means reliability/latency/capacity symptoms, **not answer quality**, which would require separate consented evaluation data.

## Small implementation packets and acceptance gates

| Order | Packet / likely files | Done when |
|---|---|---|
| 0 | Policy anchor: `PRIVACY.md`, `AGENTS.md`, this plan | Every telemetry change is required to reference the policy; deployed vs proposed claims remain explicit. **This documentation change only.** |
| 1 | Provider/Honeycomb capability spike; record in `docs/verification.md` | Confirm Honeycomb US routing, native metrics and scoped key permissions; document default retention/deletion information without requiring a custom seven-day setting for MVP. Inspect authorized Tinfoil instance resource values read-only and document scope/units/lag/access/retention. Preserve unresolved properties; stop external export if remaining required controls fail. Reuse configured `dev` destination. |
| 2 | Replace `src/telemetry.rs` scaffold; extend existing `tests/privacy.rs` coverage (split a telemetry suite only if needed); select/pin OTel deps in `Cargo.toml`/lockfile | Typed bounded dimensions, fixed windows, disabled-default behavior, synthetic-mode isolation and family-level privacy gate exist. No user/account/request data enters metrics. Test actual serialization and the chosen histogram representation. |
| 3 | Lifecycle observations in server/router, generation and stream ownership code; related integration tests | Synthetic success, rejection, malformed/absent terminal usage, invalid stream, upstream error, disconnect, duplicate and settlement failure produce correct distinct counts. Worker terminal/delivery outcomes cannot double count or change billing. No inference replay. |
| 4 | Resource source + pinned Collector config under `deploy/`; measured `tinfoil-config.yml` and build changes only as needed | CPU and memory scope is demonstrated by controlled loads; bounded metric pipeline, least privileges, private ingestion, sanitized self-diagnostics and correct egress work. No OBI/host-root/Docker socket added by default. Export outage/off switch leaves inference and accounting unaffected. |
| 5 | Authorized production deployment to existing gateway; Honeycomb `dev` board; `docs/verification.md` | Complete runtime/Collector verification and real-traffic sparse-family/correlation review before enabling applicable exports. Local synthetic payload inspection and scoped deployed verification pass. Read back actual metric/resource schemas; confirm forbidden fields absent. Record effective default retention/access and measured image/config versions without secrets. Never label mixed traffic synthetic. |
| Later | Retention/environment/alerting follow-up | Request shorter retention (target seven days), consider a dedicated production Honeycomb destination, and choose operational alerts. These follow-ups do not replace current privacy/release gates. |

### Verification checklist

- Inject unique fake canaries into prompt/output/history, tool fields, credentials/cookies, URLs/headers, invalid model IDs, raw errors/panics and hostile resource attributes. Inspect synthetic app export, Collector egress and self-diagnostics. No canary or forbidden field survives. Use no live secrets or real prompt captures in artifacts.
- Test below-threshold counts, rare error/latency buckets, overlapping totals, repeated snapshots, restarts and late terminals. No reconstruction from totals-minus-breakdowns; one request with many chunks is one generation; ten submissions from one account do not prove privacy.
- Compare deterministic synthetic counts to HTTP, generation and delivery populations separately. Include stream failures after HTTP 200, duplicate submissions, disconnect while upstream continues, refunded failures, accounting races and missing terminal observations after crashes.
- Validate latency bucket placement with a fake clock; do not silently combine admission time, upstream first output and delivery duration. Verify percentile semantics against known distributions.
- Controlled CPU/memory load establishes units, denominator and source scope; record unavailable permissions/limits rather than increasing privileges silently. Observe collector overhead and confirm it fits the deployment budget.
- Kill Collector/export access and restart the gateway. Verify bounded memory, no spool, no sensitive failure logs, no latency/accounting regression, safe partial-window loss and no retry of generation. Exercise the kill switch, including pending batch disposal.
- Inspect actual Honeycomb data types/attributes, retention and access. Configuration intent or an empty MCP dataset is not proof. Keep production export off until its separate gates pass.

## Sources

Primary documentation reviewed 2026-10-07; platform documentation may be newer than our pinned deployment.

1. Google SRE: [Monitoring Distributed Systems — Four Golden Signals](https://sre.google/sre-book/monitoring-distributed-systems/). Successful and failed request latency must be separated; saturation includes headroom, not just current utilization.
2. Tinfoil: [Admin API](https://docs.tinfoil.sh/admin/admin-api), including container metrics, scoped actions and billing APIs; [CLI resource metrics](https://docs.tinfoil.sh/containers/cli).
3. Tinfoil: [Configuration](https://docs.tinfoil.sh/containers/configuration), [Runtime & security](https://docs.tinfoil.sh/containers/config-runtime), [Networking](https://docs.tinfoil.sh/containers/config-networking).
4. Tinfoil: [Troubleshooting](https://docs.tinfoil.sh/containers/troubleshooting). Separate debug instances use disposable data and do not provide the normal attestation guarantee; do not debug production prompts there.
5. Tinfoil: [Status documentation](https://docs.tinfoil.sh/resources/status), [service status](https://status.tinfoil.sh).
6. OpenTelemetry OBI: [Security/capabilities](https://opentelemetry.io/docs/zero-code/obi/security/), [export features](https://opentelemetry.io/docs/zero-code/obi/configure/export-data/).
7. OTel Collector contrib: [hostmetrics](https://github.com/open-telemetry/opentelemetry-collector-contrib/tree/main/receiver/hostmetricsreceiver), [docker_stats](https://github.com/open-telemetry/opentelemetry-collector-contrib/tree/main/receiver/dockerstatsreceiver). Pin a tested version; upstream defaults are not our privacy policy.
8. Honeycomb: [Data protection and retention](https://docs.honeycomb.io/security-compliance/security-data-protection/). Workspace discovery used the read-only Honeycomb MCP; no ingest/retention capability was inferred from discovery alone.
9. Privacy comparators and the limits of the analogy: [`PRIVACY.md`](../PRIVACY.md#inspiration-not-equivalence).
