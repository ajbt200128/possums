# Repository guidance

## Minimum implementation per phase

- Build only what is needed to complete the current phase. Do not pull future-phase infrastructure or speculative hardening into the implementation.
- Check provider SDKs and existing libraries before writing an adapter. Reuse their supported clients, stream decoders, types and helpers instead of reimplementing provider protocols.
- Keep custom code for gateway-specific behavior such as authentication, reservations, settlement, receipts and bounded stream state—not duplicate SDK wire handling. Tool-name/choice and argument validation for execution belong to the caller; a gateway settlement receipt is not tool-execution authorization.
- Check nearby code for reusable functionality before adding another helper. Remove superseded code within the task; propose unrelated cleanup separately.
- Keep existing required security/accounting behavior intact. Defer additional caps, abstractions and optimizations until the phase or production plan actually needs them.

## Product decisions

For Phase 0, follow this file first, then `SPEC.md` and `docs/phase0.md`. `OVERALL_PLAN.md` preserves historical, long-term context and is not authoritative for Phase 0. The deployed `v0.0.5` gateway still buffers complete responses and refunds on incomplete delivery; streaming-only inference and usage-based settlement below are a **planned replacement**, not implemented or verified behavior. Keep the previous regime explicit in the Phase 0 contract and verification record until a new release passes its gates.

- Phase 0 is an attested clearnet gateway with a plain, no-JavaScript interface. Issue demo credentials manually; do not add public signup or payments to this phase.
- Expose all supported Tinfoil models through a live, authenticated Tinfoil catalog and an OpenAI-style model selector. Catalog contents are outside the gateway measurement; disclose that boundary and fail closed when catalog authentication or validation fails.
- Permit each selected model's maximum output. Snapshot the authenticated catalog price when a request is submitted, reserve its maximum possible cost before sending prompt content upstream, then settle the actual cost and refund the remainder at that quoted rate. Keep every supported model visible; if the selected model's maximum reservation exceeds available credit, reject before upstream prompt transmission with a clear insufficient-credit message, not a substituted model or lower output cap. The planned replacement streams inference output without buffering a complete response; bounded input parsing and small transport buffers remain necessary.
- Keep only short-lived, prompt-free reservation and idempotency state. Never claim the replicas are strictly stateless while this state exists.
- Use Mullvad-style abuse controls: transient per-account concurrency and quota state plus aggregate service metrics. Do not retain or export client IPs. Add IP-keyed controls only through a later, explicit threat-model decision.
- Do not impose a product-level byte cap on submitted conversation history, but retain defensive HTTP/parser/memory ceilings. Enforce each model's context window before inference.
- Use high-entropy copy/paste recovery credentials, not 16-digit account numbers. The browser may hold the credential in a secure session cookie and must provide an explicit copy/download recovery action; users re-enter it in new sessions.
- Initial payment policy is a $5 minimum and a 30% markup over upstream inference cost.
- Tor identity, Monero, `/app`, CLI/SDK, replicas, and an external audit are deferred beyond Phase 0.

## Security model

Preserve these trust boundaries:

- The gateway and Tinfoil inference service see prompt plaintext in memory. The store and telemetry pipeline must never receive prompts or responses.
- The store is outside the attested boundary. It can associate payments with accounts and observes reservation/settlement events. Do not describe it as anonymous, invisible, or hardware-verifiable.
- Attestation proves a measured code/configuration identity; it does not prove that retention is impossible under runtime compromise, that the operator cannot change an unattested service, or that traffic cannot be correlated.
- Open egress, payment metadata, timing, account reuse, backups/WAL, inference caches, live catalog changes, and compromised dependencies are explicit residual risks.
- Production must fail closed before sending a prompt when attestation, release provenance, endpoint-key binding, model-catalog authentication/validation, or cost reservation fails.
- Never expose a usable credential in an image, repository, URL, metric, log, trace, panic, error body, or support artifact. Set session cookies with `Secure`, `HttpOnly`, and an appropriate `SameSite` policy. Compare authentication material in constant time where applicable.
- Treat all prompt text, submitted history, Markdown, model output, headers, payment callbacks, catalog responses, and upstream errors as hostile input. Escape HTML, sanitize rendered Markdown, block remote-resource loading, bound parsing and memory at the transport layer, and verify webhook signatures and replay resistance. The planned no-JavaScript web stream displays escaped plain text; safe client-side Markdown rendering is optional later work, not a streaming dependency.
- Idempotency, reservations, settlement, refunds, cancellation, retries, concurrency, and crash recovery are security/accounting behavior. Test their failure modes. For the planned streaming regime, a downstream disconnect does not cancel inference: keep consuming the authenticated upstream stream until it completes or errors, then settle once from authenticated final usage at the submitted price even if the client did not receive the answer. If upstream errors or final usage is absent/invalid, refund the user reservation exactly once and absorb any upstream cost. Never infer billable usage from emitted text or automatically replay an uncertain generation. A user who submits a new request after a disconnect may pay for two generations; do not claim receipt is guaranteed. Upstream transport/resource failures remain errors, not a special post-disconnect grace-period cutoff.
- Disable SDK prompt caches and unintended persistence explicitly. Use read-only roots and memory-only writable storage where the platform permits it.

## User-facing error policy

- Because production does not export request logs or request-level traces, user-facing errors must be useful diagnostics: short, specific and actionable, not just “failed” or “invalid stream.”
- Preserve a stable, content-free error code and the failing stage/constraint across SDK, gateway and client boundaries. Distinguish request encoding, verified transport, SDK decoding, gateway validation, missing terminal usage/finish and settlement failures. Do not collapse known causes into one generic error.
- State billing/reservation status only from observed accounting or an authenticated terminal receipt. Otherwise say it is unknown. Never imply cancellation, refund or safe retry merely because delivery failed; say when the request was not replayed and warn that a new request may incur another charge.
- Use closed error categories and locally authored messages. Never display or retain raw SDK/upstream/parser errors, offending values, prompts, responses, history, credentials, headers, URLs, account identifiers or panic payloads. Rich diagnostics are not permission to echo hostile input.
- Show detailed errors transiently to the affected user; do not automatically log, trace, export or package them into support artifacts. Users may manually share the content-free error code for support, never credentials or request data. Aggregate observability remains governed by the telemetry policy below.
- Test both usefulness and privacy: each known failure keeps its category through the user interface; hostile error text cannot escape; interrupted error delivery cannot fabricate a confirmed billing outcome. No diagnostic path may automatically replay inference.

## Telemetry policy

Production observability uses aggregate Honeycomb metrics only:

- Sanitize and aggregate locally before export through an OpenTelemetry Collector.
- Send sanitized aggregates to Honeycomb US with seven-day retention.
- Do not export production logs or request-level traces.
- Never export prompts, responses, conversation history, request bodies, URLs/query strings, headers, client IPs, user agents, credentials, account/payment identifiers, onion identifiers, stable pseudonyms, exact request timestamps, or per-request token/cost events.
- Allowed signals are low-cardinality aggregates such as coarse request counts, status classes, latency buckets, queue depth, process health, and time-bucketed model/cost totals that cannot isolate a user or session.
- Sampling is not sanitization. Do not rely on vendor-side deletion as the primary control.
- Restrict access, document Honeycomb as a third-party processor, and provide a kill switch. Core service behavior must work with telemetry disabled.
- Do not claim “no telemetry” or “no logs whatsoever.” Claim only that request content and identifying/request-level telemetry are not collected, within the tested configuration.

## Verification discipline

- Convert privacy claims into scoped, testable statements with assumptions and live-phase labels.
- Add negative tests for sensitive-field export, hostile rendering, oversized transport input, duplicate submissions, reservation races, retries, disconnects, upstream failures, and crash recovery.
- Verify actual runtime behavior and generated artifacts; configuration intent alone is not evidence.
- Record unresolved platform properties as unknowns rather than guarantees.
