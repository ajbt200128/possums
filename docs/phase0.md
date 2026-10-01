# Phase 0 contract

## Routes

| Route | Method | Purpose |
|---|---|---|
| `/` | GET | Login or chat form and authenticated model selector |
| `/login` | POST | Exchange a provisioned recovery credential for a session |
| `/logout` | POST | End the session |
| `/chat` | POST | Validate, reserve, generate, render, and settle one buffered turn |
| `/recovery` | GET | Explicitly display the user's recovery credential |
| `/recovery/download` | GET | Authenticated plain-text recovery download |
| `/claims` | GET | Current scoped privacy and verification claims |
| `/attestation` | GET | Platform quote and release/endpoint-binding evidence |

There is no public signup or inference API.

## Request limits

Limits are defensive transport controls, not a product limit on conversation history. The server admits at most 64 connections without a connection admission queue, caps each connection at ten minutes, accepts at most 64 headers and rejects aggregate header names/values over 32 KiB, and applies a ten-second total header deadline. Before consuming a body, a shared fail-fast gate reserves one conservative 128 MiB request-memory envelope from a 512 MiB application budget and retains it until the response body is consumed or dropped; consequently at most four requests are admitted. Each admitted request buffers at most 8 MiB of request body under one total 30-second deadline. Render input is checked with a conservative JSON/HTML expansion allowance before intermediates are built, and rendered responses are limited to 32 MiB. Inference responses are read incrementally, limited to 16 MiB, and covered by one five-minute send-and-body deadline. At most four generations run concurrently. These envelopes bound application-controlled request, upstream, parsing, escaped-render, and outstanding-response content across admitted requests; allocator/library overhead and the whole-process resident set still require runtime measurement. Request decompression is not enabled. Each authenticated catalog entry supplies a chat model's context window. Because Tinfoil's live catalog does not publish a separate maximum-output limit, the gateway requests the entire context remaining after model-specific tokenization rather than imposing a lower product limit.

## Authentication

An operator provisions a random credential with at least 256 bits of entropy and a demo-credit budget through a runtime secret. Credentials never belong in source, images, URLs, telemetry, or errors. Authentication comparisons are constant-time. Sessions are capped, expire after twelve hours, and use random opaque cookies marked `Secure`, `HttpOnly`, and `SameSite=Strict`. Login uses a single-use, ten-minute, same-site cookie-bound CSRF challenge. Other mutating forms require a session-bound CSRF token. The recovery page is no-store and offers explicit copy/download instructions without JavaScript.

## Catalog and quote

The gateway fetches the bounded live `/v1/models` catalog over the attested, endpoint-key-bound inference channel. It accepts only unique `type: chat` entries advertising `/v1/chat/completions`, with valid context windows, positive input/output prices, and no unaccounted per-request fee. Decimal USD-per-million-token prices are converted exactly to fixed-point microunits without floating-point arithmetic; malformed, unsupported, duplicate, zero-price, or overflowing entries fail closed. All valid supported chat models are displayed without substitution. At submission, the gateway snapshots those rates and applies a 30% markup with checked integer arithmetic and round-up division. Because model-specific tokenization is an upstream operation, the gateway first reserves the greatest possible cost across the model's full context at either the input or output rate, then tokenizes and refunds the excess at settlement. This prevents insufficient credit from exposing prompt bytes upstream.

## Accounting lifecycle

State is single-process, short-lived, and contains no prompts or responses:

1. A capped, fifteen-minute submission token is issued by the gateway and bound on first use to the account, session, process epoch, and selected model. Reusing it with changed prompt/history never regenerates, but the gateway intentionally retains no content hash and does not claim to detect which content changed.
2. `reserve` atomically checks balance, account concurrency/quota, and duplicate status, then deducts the model-context maximum quoted cost.
3. A duplicate token always returns its stored terminal status and never invokes inference. Terminal tombstones remain until the associated token expires, then are reclaimed; authentication removes expired tokens first, so a reclaimed identifier cannot be accepted or replayed.
4. Verified inference returns authenticated usage. The gateway records the prospective actual charge at the snapshotted rate and settles when the complete response body is consumed by the HTTP transport.
5. Missing/invalid usage, upstream failure, timeout, cancellation, body error, or abandonment before transport completion refunds the full user reservation exactly once. Measured upstream cost is an aggregate operator expense only.
6. No automatic inference retry occurs after generation may have started.

The buffered HTML response contains the transcript and a safe next-turn form using a new token; an old token cannot regenerate. Reservation ownership remains attached to the response body: cancellation, panic, body error, or abandonment before complete body consumption refunds, while complete body consumption atomically settles actual usage and releases the unused reservation. Local response-drop, socket, panic, rendering, catalog, token-capacity, and settlement/refund race tests cover these transitions. This server-side transport boundary does not prove browser receipt, and disconnects after it may still charge the user. Deployed write-failure behavior remains a release evidence requirement.

### Restart semantics

Phase 0 has no durable paid accounting. Operator-granted demo balances are loaded afresh on restart. Outstanding reservations disappear and are therefore conservatively treated as refunded; no inference is replayed. A random process epoch invalidates all pre-restart sessions and submission tokens. A pre-restart POST fails authentication and cannot become a fresh generation. These semantics are unsuitable for purchased credit.

## Content and privacy

Conversation history is carried by the form and exists server-side only for the active request. HTML is escaped. Model Markdown is sanitized with raw HTML and automatic remote resources disabled. Responses set a restrictive Content Security Policy, `Cache-Control: no-store`, `Referrer-Policy: no-referrer`, and `X-Content-Type-Options: nosniff`.

No production request logs or request-level traces are exported. Telemetry export is disabled and unwired. The current local-only scaffold contains allowlisted lifetime counters with sparse-value suppression; because it does not implement time buckets or resource/datapoint sanitization, it must not be enabled in production. They never include bodies, prompts, responses, URLs, headers, addresses, agents, credentials, account/payment identifiers, stable pseudonyms, exact timestamps, or per-request token/cost events. Core behavior is unchanged when telemetry is off.

## Deployment constraints

The proposed container configuration declares a read-only root, a bounded memory-only `/tmp`, a process limit, and an inference/verifier-host egress allowlist. The OCI entrypoint disables core dumps before starting the gateway. Tinfoil CVM v0.14.12 exposes `/tinfoil/attestation.sock` only because the gateway declares `attestation: true`. Before each prompt, a measured helper requests a fresh random nonce-bound v3 quote and uses the pinned Tinfoil Go verifier to authenticate hardware evidence, release provenance, freshness witnesses, and the endorsed TLS-key fingerprint. Missing sockets, malformed or stale evidence, verifier errors, and helper timeouts fail closed. Deployed validation must still compare the endorsed key with the public serving certificate and verify mounts, egress, process/core limits, platform logging, and helper behavior. Configuration intent and local tests are not deployed evidence, and open egress, if required by the platform, is a residual risk.

## Packet 1 — superseding user settlement decision

This explicit user decision supersedes the provider-contract stop condition above and in the historical BLOCKED verification record; it does not erase the failed probe or change the deployed `v0.0.5` evidence. No provider billing contract is required to implement this gateway policy.

- Only the **LAST valid authenticated usage event**, after a valid successful finish, `[DONE]`, EOF, and no upstream error, authorizes settlement. Missing/invalid usage, malformed/incomplete termination, or upstream failure refunds the reservation exactly once. Never derive usage from emitted text. Protocol validation is a later stream-adapter responsibility, not something the ledger can establish from token counts.
- Reserve `ceil(130*(C*p + M*q)/(100*1_000_000))` at snapshotted authenticated prices before prompt-bearing tokenization or generation. `C` is catalog context. Use a validated separate advertised output bound only where actually available; the current Tinfoil adapter has **no observed separate output-limit field** and retains `M=C` as an operational assumption. Reject unsupported prices/fees, invalid bounds, arithmetic overflow, unrepresentable quotes, or insufficient credit before sending prompt content.
- Keep that reservation and price snapshot separate from the context-legal request allowance `min(M,C-tokenizer_input)`. Request the entire allowance without credit-driven reduction, model substitution, or hiding supported models; reject when no output room remains.
- For valid final usage, compute the checked marked-up charge at the reserved prices and charge `min(calculated_charge, original_reservation)`, refund the remainder and release account capacity atomically once. Counts above tokenizer counts or operational bounds are not intrinsically invalid. Invalid totals or charge arithmetic refund once. Delivery cannot own this new terminal operation; repeated/conflicting terminal calls cannot reverse an outcome.

**UNVERIFIED operator risk:** provider-to-invoice usage semantics and per-model maximum billable-token bounds. Catalog validation and local arithmetic tests do not prove coverage of all upstream costs. The operator absorbs cost above the reservation and costs on refunded failures. If a finite operational quote cannot be computed, identify the unusable catalog fields rather than demanding a provider billing contract.

**Implementation staging:** this packet adds checked reservation and terminal-ledger primitives only. The stream adapter, detached workers, conversation redesign, and production route migration remain later work. Until that migration, production `/chat` still buffers the answer, applies its old usage checks, and settles/refunds via delivery. Additive primitives are not a streaming release candidate. Demo balances/reservations remain memory-only; restart reloads grants, forgets reservations without replay, and invalidates old auth epochs—not durable paid accounting. Telemetry remains disabled.

### Implemented additive packet-1 primitives

`Catalog::reservation_quote` now computes the combined operational quote with checked arithmetic, validates normalized bounds/prices, and rejects unknown pricing components. `Catalog::quote` remains the separate context-legal request allowance; it does not replace the reservation snapshot stored by the ledger. These quote changes also apply to the still-buffered route's pre-prompt admission.

`Accounting::finish(id, Option<FinalUsage>)` atomically caps a valid charge, refunds the remainder and releases one account slot under the existing ledger mutex. `None`, inconsistent/overflowing totals, or unrepresentable charge arithmetic refunds fully. The returned terminal outcome is absorbing even on conflicting repetitions. Final counts are neither compared with operational bounds nor retained in the tombstone. The caller is responsible for verified stream completion and LAST-event selection; **no production stream caller exists yet**. The old `prepare_settlement`/`settle`/`refund` APIs remain solely for buffered compatibility. No new content or content hashes are retained.

## Current checkout versus historical deployed release

The packet-1 staging paragraphs above record that packet's historical state, **not the current local checkout**. The local `/chat` route now streams escaped plain text, owns accepted generation across browser disconnects, settles once after authenticated terminal usage, and has retired the buffered inference and delivery-owned settlement APIs. Local browser and Rust tests are recorded in [`verification.md`](verification.md), with their synthetic scope. **Scoped Phase 0 streaming release accepted by the operator, not fully independently verified:** `v0.0.8` now serves the streaming implementation from a new measured release; pinned TLS-key verification, selected live/browser/settlement canaries and post-fix no-prompt reliability checks passed (see [`verification.md`](verification.md)). The `v0.0.7` clock-boundary 503 source bug was fixed, but its exact historical incidents cannot be retrospectively classified; reported slow turns on an unspecified model and direct runtime/privacy inspection remain open. The preceding `v0.0.6` and independently verified `v0.0.5` buffered records remain historical. The 8-MiB chat body and 4-KiB control-body ceilings, header and connection admission, parser/catalog/evidence/renderer limits, four generation lanes, three submissions per account, and prompt-free reservation ownership remain in force. The user accepts the **unproved 512-MiB scoped requested-allocation target as an availability risk**, not a Phase 0 release gate or an RSS guarantee; the detached upstream-driver lifetime remains unresolved. Keep the existing narrow SDK evidence, idle-pool, and redirect patches, but do not require another SDK patch solely to prove this target. Selected funded live compatibility and fresh release-provenance checks have passed for `v0.0.8`; unresolved negative trust/catalog/reservation injection, runtime/privacy properties, model-dependent startup latency and provider invoice risk remain explicit. The user elects to rely on Tinfoil's runtime/platform enforcement for now, without treating unknown mounts, egress, logs or caches as independently verified privacy guarantees. The operator additionally accepts the absence of live negative attestation/catalog/reservation fault injection and direct upstream zero-prompt-byte observation before this **scoped** release acceptance; local fail-closed tests passed, but deployed behavior under those faults remains unverified. The previous buffered charging contract and its tests remain explicit historical evidence, not the current streaming billing policy.
