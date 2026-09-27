# Repository guidance

## Product decisions

For Phase 0, follow this file first, then `SPEC.md` and `docs/phase0.md`. `OVERALL_PLAN.md` preserves historical, long-term context and is not authoritative for Phase 0.

- Phase 0 is an attested clearnet gateway with a plain, no-JavaScript interface. Issue demo credentials manually; do not add public signup or payments to this phase.
- Expose all supported Tinfoil models through a live, authenticated Tinfoil catalog and an OpenAI-style model selector. Catalog contents are outside the gateway measurement; disclose that boundary and fail closed when catalog authentication or validation fails.
- Permit each selected model's maximum output. Snapshot the authenticated catalog price when a request is submitted, reserve its maximum possible cost, then settle the actual cost and refund the remainder at that quoted rate.
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
- Treat all prompt text, submitted history, Markdown, model output, headers, payment callbacks, catalog responses, and upstream errors as hostile input. Escape HTML, sanitize rendered Markdown, block remote-resource loading, bound parsing and memory at the transport layer, and verify webhook signatures and replay resistance.
- Idempotency, reservations, settlement, refunds, cancellation, retries, concurrency, and crash recovery are security/accounting behavior. Test their failure modes. Settle when the complete response body is consumed by the HTTP transport; refund if generation or body delivery fails or is abandoned before completion. This is a server-side completion boundary, not proof of browser receipt. Do not promise detection of disconnects after that boundary or that cancellation eliminates measured upstream cost.
- Disable SDK prompt caches and unintended persistence explicitly. Use read-only roots and memory-only writable storage where the platform permits it.

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
