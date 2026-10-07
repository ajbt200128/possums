# Possums privacy policy and engineering requirements

**Status: engineering policy; not a claim that every control is deployed.** Last researched: 2026-10-07. Telemetry rollout is planned, not enabled by this document. See [`docs/telemetry-plan.md`](docs/telemetry-plan.md) for implementation gates and [`docs/verification.md`](docs/verification.md) for scoped release evidence. This is not yet a complete public legal notice: operator identity, jurisdiction, privacy contact, legal obligations, and support/deletion procedures need review before publication.

## Mandatory reference and change control

Read this file **before touching telemetry**: instrumentation, metrics, logging, tracing, SDK configuration, collectors, resource discovery, dashboards, alerts, exporters, deployment, crash reporting, or diagnostic/support tooling. Link the applicable sections in the change description and verification record. Repository guidance in [`AGENTS.md`](AGENTS.md) requires this review.

This policy applies to real data in **every environment**, including testing. A `test` label is not consent to collect sensitive data. Only an isolated, operator-controlled synthetic workload may use the synthetic-testing exception below. Broader collection requires an explicit policy/threat-model decision first, not a new environment variable or vendor default. If documents disagree, keep the stricter privacy restriction until the conflict is resolved; do not silently weaken `AGENTS.md` or the phase contract.

## What we are protecting

Our goal is to operate a reliable private inference gateway without building a history of people's activity. Collect the minimum operational information, keep it briefly, and explain the limitations. Do not sell request data, use it for advertising/profiling, or repurpose observability data to train models.

Possums is **not Signal-style end-to-end encrypted messaging**: the gateway and Tinfoil inference service process prompt and response plaintext in memory. Attestation authenticates measured code/configuration and endpoint binding; it does not prove that retention is impossible after runtime compromise. It does not make unrelated services attested or prevent traffic correlation.

The following remain explicit risks: network timing and source-address visibility to network intermediaries, TLS termination, open egress, dependencies, upstream inference caches, live catalog changes outside the gateway measurement, account reuse, and platform/operator access. A future external accounting store can associate payments with accounts and observe reservation/settlement events; it is not anonymous or hardware-verifiable. Persistent accounting would also introduce backup/WAL retention concerns. Phase 0 has manually granted demo credit, not payments or durable purchased-credit accounting.

## Data handling boundaries

| Data | Permitted purpose and location | Retention / restriction |
|---|---|---|
| Prompts, outputs, history, attachments, tool names/arguments/results | Request processing in the gateway and verified inference path; local client history where that client supports it | No intentional gateway persistence or telemetry collection. Bounded memory is not guaranteed immediate memory erasure. A disconnected client may leave inference running until its terminal outcome. |
| Credentials, sessions, CSRF and submission tokens | Authentication and short-lived in-memory control state; necessary secure browser cookies | Never telemetry, URLs, images, errors, support bundles, or repositories. Sessions/tokens follow the phase contract's expiry and restart semantics; provisioned credentials are runtime secrets, not all session data. |
| Reservations, idempotency, quota/concurrency state | Short-lived, prompt-free accounting and abuse controls | No content/content hashes. Do not export account-linked events. The gateway is not strictly stateless. Restart behavior is phase-specific, not durable paid accounting. |
| Operational request statistics | Locally sanitized, privacy-gated aggregates only | No event-level records. Honeycomb US, **at most seven days**, after retention verification. No secondary archive or dashboard export that extends retention. |
| CPU, memory, capacity and service-health statistics | Coarse infrastructure health by operator-assigned gateway slot | No process arguments, environment, paths, addresses, user identifiers or content. Correlation with sparse traffic is still a risk. |
| Voluntary support contact | Only what the user deliberately shares | Ask for a content-free error code, not credentials/history. Do not auto-upload diagnostics. Public support retention/deletion terms remain to be defined. |

Do not retain or export client IPs for abuse controls. Transient account quota/concurrency controls are permitted. An IP-keyed control needs a later explicit threat-model decision. SDK prompt caches and unintended persistence must be explicitly disabled. Runtime roots should be read-only and writable storage memory-only where supported; deployed enforcement remains a verification question.

## Telemetry: permitted signals and forbidden data

**Metrics only.** No production request logs or request-level traces. Apply the same collection policy to real traffic in testing. Do not use access logs, trace-to-metrics processing, log bridges, automatic HTTP/GenAI instrumentation, profiling or session replay as a back door to collecting request data. Aggregate directly from closed, typed observations in application memory. Never put raw requests or raw errors into the telemetry pipeline to redact later.

Permitted after privacy gating:

- Coarse request/terminal-outcome counts and status classes.
- Fixed-bucket latency distributions, separated by success/failure.
- Coarse concurrency, admission pressure, resource use and process health.
- Bounded model, endpoint-class and infrastructure-slot dimensions.
- Future time-bucketed model token/cost totals only after additional sparse-data review; they are **not part of the first pass**.

**Never collect into telemetry or export:**

- Prompts, responses, history, documents, tool payloads, content hashes or derived text/features.
- Request bodies, raw URLs/paths/query strings, headers, cookies, user agents, client/server IPs, DNS/network flow records or identifying hostnames.
- Credentials, account/payment identifiers, session/conversation/request/trace/span/submission/reservation IDs, onion identifiers, or stable user pseudonyms. Hashing an identifier does not make it acceptable.
- Exact request timestamps, per-request token/cost events, request/response byte sizes, or exact individual duration samples.
- Raw SDK/upstream/parser errors, panic payloads, stack traces, environment variables, process command lines or memory/core dumps.

The no-stable-pseudonym rule concerns people/accounts/sessions. An operator-assigned **infrastructure slot** such as `gateway-01` is allowed solely to diagnose gateway resources. It must not identify a customer, encode a hostname/IP, or describe a dedicated user deployment. Source request values cannot set telemetry labels. Error categories must be a reviewed closed enum, not exception strings; detailed content-free error codes remain transient user diagnostics, not automatically exported labels.

## Aggregation is necessary, not sufficient

1. Sanitize and aggregate **before data leaves the gateway trust boundary**. An OpenTelemetry Collector provides a second allowlist, not the first privacy boundary. Drop unknown metric names, attributes, resources, exemplars and free-form scope metadata; fail closed on sanitizer errors.
2. Export fixed, non-overlapping time windows, not cumulative request counters whose successive values expose new events. Export only bucket boundary timestamps, never request times, start times or last-seen times. Do not flush partial request windows on shutdown.
3. Review the **complete family of released values**. Model × instance × endpoint × outcome can isolate one request even when each individual label is low cardinality. Rare histogram bins, sums/min/max, successful-vs-total counts, and overlapping totals can reveal suppressed values by subtraction.
4. Suppress or coarsen sparse families locally. A minimum of ten requests is a useful test threshold, **not an anonymity guarantee**: ten requests can come from one person. Never claim k-anonymity, differential privacy or resistance to correlation without the corresponding design and evidence. Do not introduce a user-identifier collection system just to claim a privacy threshold.
5. Missing or suppressed values mean **unavailable**, not zero or healthy. Dashboards and alerts must preserve this distinction. Do not export fine-grained suppression counts that reconstruct the withheld activity.
6. Before exporting real-traffic request/model aggregates, approve and test a concrete release algorithm covering sparse traffic, all simultaneous breakdowns, repeated users and external timing correlation. Until then, keep those exports off; validate first with synthetic workloads. High-resolution infrastructure usage also needs this correlation review before a real-traffic rollout.

## Processors, retention, access and shutdown

- **Honeycomb** is a third-party observability processor, not inside the attested boundary. Send only approved aggregates over authenticated TLS to the US endpoint through a Collector. Honeycomb receives gateway/collector network metadata as part of transport; it must not receive client addresses or request identifiers. Verify vendor-side transport/audit retention separately from metric retention.
- Seven-day metric retention is a **required setting/contract, not an observed fact**. Honeycomb's public documentation describes 60 days for most customers. Verify the actual metrics-dataset retention and deletion/backup semantics with the account UI/support before first export; if the requirement cannot be met, do not export or silently accept a longer default.
- **Tinfoil** provides the enclave/inference platform and already offers container resource metrics and billing/usage APIs. Its platform collection, access, retention and backup behavior are a separate boundary, not controlled by a seven-day Honeycomb setting. Verify/disclose these properties; do not claim that locally disabling export turns off platform observability. Do not import API-key billing breakdowns or provider resource identifiers into telemetry.
- Limit Honeycomb access to named operators who need it; no public boards or unrestricted share links. Use environment-scoped ingestion-only keys where supported. Keep vendor/admin keys in runtime secret delivery, never a document or image. Do not place a Tinfoil admin key in the prompt-processing gateway.
- Disable telemetry by default. Provide a tested kill switch that stops export and clears bounded in-memory pending batches; no disk spool. Vendor outages, quota failures, sanitizer failures and disabled telemetry must not block inference or change reservations, settlement, refunds, retries or user responses. Do not automatically replay inference.
- On suspected leakage: stop export, revoke affected credentials if relevant, restrict access, arrange vendor deletion, and document a content-free incident record. Do not copy suspect payloads into tickets or chat as evidence. Deletion is incident response, not the primary sanitization control.

## Synthetic testing exception

The first telemetry environment is Honeycomb `test`. Use isolated, synthetic prompts and throwaway test accounts only, with no real user traffic or reused production credentials. This permits detailed **aggregate** breakdowns and controlled low-count fixtures to validate arithmetic and suppression behavior; it does not permit bodies, identifiers, logs or traces. The exception must be deployment-controlled, not activated by a client header or request field. Do not mix synthetic and real traffic to manufacture enough samples for export. Retention, access control, secret handling and the export allowlist still apply.

## Required evidence for every telemetry change

- Identify the operational question, source, units, dimensions, window, retention, processor and privacy risk for each metric.
- Test hostile secrets/content in every boundary, including resource/datapoint attributes, errors, SDK defaults, exemplars, Collector diagnostics and export failures. Inspect actual outbound OTLP payloads with synthetic sentinels, not just configuration files.
- Test sparse-value and subtraction attacks, duplicate lifecycle observations, restart/reset, disabled export and exporter outage. No metric may alter accounting or inference behavior.
- Inspect generated images/configurations and the deployed runtime. Record unknown platform properties as unknown, with phase/release labels.
- Record the policy reference and pass/fail evidence in [`docs/verification.md`](docs/verification.md). Do not publish “no telemetry,” “no logs whatsoever,” “operator cannot see plaintext,” or “retention is impossible.” Only make scoped claims supported by the tested configuration.

## Inspiration, not equivalence

Sources checked 2026-10-07:

- [Mullvad's no-logging policy](https://mullvad.net/en/help/no-logging-data-policy): the closest operational model—aggregate connection/system health and transient account concurrency. We do **not** adopt its short-lived website access logs, and we disclose Honeycomb rather than claiming no external analytics processor.
- [Signal's privacy policy](https://signal.org/legal/): minimize retained information and avoid monetizing it. Its inability to decrypt message content does **not** apply to inference plaintext in our enclaves.
- [Proton privacy policy](https://proton.me/legal/privacy) and [Mail-specific policy](https://proton.me/mail/privacy-policy): explicit processor, metadata and retention disclosures. Proton documents IP-logging exceptions and mail metadata; neither is permission to introduce those practices here. We do not inherit Proton's jurisdiction, infrastructure ownership or encryption claims.
- [Honeycomb data protection](https://docs.honeycomb.io/security-compliance/security-data-protection/), [Tinfoil container metrics API](https://docs.tinfoil.sh/admin/admin-api): provider documentation is a starting point, not deployed evidence. See the [telemetry plan's source list](docs/telemetry-plan.md#sources) for implementation references.
