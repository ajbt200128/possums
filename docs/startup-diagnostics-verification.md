# Startup diagnostics — source candidate

## Scope and status

Policy: [startup-only exception](../PRIVACY.md#startup-diagnostics-exception), [processor boundaries](../PRIVACY.md#processors-retention-access-and-shutdown), [telemetry plan](telemetry-plan.md#startup-diagnostics-extension).

Operator choice: platform and Honeycomb startup logs plus aggregate startup metrics; **normal dataset**, no special startup dataset. Startup traces were acceptable, but no tracing framework is needed for this packet. Reuse normal service routing and provider-default MVP retention. Actual log-key permissions, ingestion, effective retention, platform capture/access and deletion are unverified. No new credentials, dashboard or archive.

This document initially records design, **not implementation/test/deployment evidence**. The v0.0.48 update crossed the mutation boundary and failed completion validation; candidate failure/cleanup/exclusivity and serving remain unresolved. Reported platform `containers`/`shim` failures do not establish the initiating cause. The authorized five-GET investigation budget is exhausted. No live calls, inference, update, replay, cancel, accept or rollback are part of this packet.

## Minimal implementation contract

- One startup owner after the healthcheck branch, before application configuration parsing; finite stage milestones and one terminal outcome.
- Closed typed causes with locally authored constraint/action messages, no arbitrary error/config strings. Preserve supported inference/Tor cause distinctions and unknown fallbacks.
- Stages cover configuration, accounts, signals, optional Tor preparation, verified upstream connection, TCP bind and optional Tor initialization. End before connection acceptance; do not instrument peer/request/runtime callbacks.
- Local initialization completion is not verified serving/onion reachability/settlement. Signal interruption is not failure or successful initialization.
- Emit bounded platform stderr records and terminal Honeycomb log delivery, then consume the owner. Keep existing request metrics runtime and shutdown semantics.
- One startup lifecycle count family, closed stage/outcome partitions, observed transitions once, no overlapping total/healthy zeros/durations/request dimensions. Startup snapshot is separate from complete five-minute request windows.
- Reuse fixed Honeycomb US direct TLS transport, generated official OTLP messages, explicit resource and scope, no default discovery/context capture/log bridge. No new dataset header.
- Logs then metrics, one attempt per signal within one finite shared deadline; no spool/retry/detached HTTP driver. Dispose owned work before returning and preserve primary startup exit/failure. Export failure gets at most a closed local warning, never recursive export.
- `OTEL_SDK_DISABLED=true` disables Honeycomb signals; stderr remains subject to platform capture. Missing/invalid telemetry config does not change service startup outcome.

## SDK source findings — not runtime verification

Pinned Rust OTel 0.29 sources expose supported generated OTLP log messages. `SdkLoggerProvider::build` eagerly evaluates `Resource::builder().build()` through `unwrap_or`, even when an explicit resource exists. Normal SDK log emission can capture current trace context. Avoid that provider/emission path; construct official generated OTLP messages from the sealed startup projection instead. Reuse supported transport/protobuf types, not a custom wire protocol.

## Required offline evidence — pending

- Exhaustive mappings and every startup boundary, interruption and terminal ownership; existing exit codes and trust/accounting/drain behavior unchanged.
- Actual decoded outbound protobuf and HTTP fixtures: exact normal routing, bodies/attributes/resources, absent IDs/automatic metadata and bounded payloads. Existing metrics wire unchanged.
- Hostile synthetic config/error/io/panic/HTTP-response canaries absent from stderr and OTLP. No arbitrary string diagnostic API.
- Disabled/malformed config, outage/deadline/rejection/cancellation, original-failure preservation and no recursive export/replay/inference.
- Pinned default/Tor tests, formatting/lint and focused independent diff review. Record exact checks/results here when run.

## External reporting limits

Pre-main, Tokio runtime construction, panic/abort/OOM/SIGKILL, blocked/lost stderr and process shutdown can prevent observation/delivery. A platform failure before the executable runs cannot be explained by its logs. Standard fixed-host DNS may outlive a dropped attempt; whole-process transience/exit timing is not guaranteed. No public callback, readiness probe or inference is added. HTTP acceptance is not observed Honeycomb ingestion, effective retention or serving acceptance. Native host history and platform persistence remain separate boundaries.
