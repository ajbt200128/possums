# Phase 0 contract

## Routes

| Route | Method | Purpose |
|---|---|---|
| `/` | GET | Login or chat form and authenticated model selector |
| `/login` | POST | Exchange a provisioned recovery credential for a session |
| `/logout` | POST | End the session |
| `/chat` | POST | Validate, reserve, generate, settle, and render one buffered turn |
| `/recovery` | GET | Explicitly display or download the user's recovery credential |
| `/claims` | GET | Current scoped privacy and verification claims |
| `/attestation` | GET | Platform quote and release/endpoint-binding evidence |

There is no public signup or inference API.

## Request limits

Limits are defensive transport controls, not a product limit on conversation history. The application accepts at most 8 MiB of decoded form data, 32 concurrent requests, and 4 concurrent generations. Inference responses are limited to 16 MiB and inference calls to five minutes. Request decompression is not enabled. Header size/time, body time, and whole-process memory ceilings depend on the serving shim and remain UNKNOWN until runtime verification; production must configure and verify them. Each authenticated catalog entry supplies the model's context and maximum-output limits; the complete conversation plus requested maximum output must fit that context. The gateway does not lower a model's maximum output.

## Authentication

An operator provisions a random credential with at least 256 bits of entropy and a demo-credit budget through a runtime secret. Credentials never belong in source, images, URLs, telemetry, or errors. Authentication comparisons are constant-time. Sessions use random opaque cookies marked `Secure`, `HttpOnly`, and `SameSite=Strict`; mutating forms require a session-bound CSRF token. The recovery page is no-store and offers explicit copy/download instructions without JavaScript.

## Catalog and quote

The gateway fetches a bounded catalog over the verified inference channel. Authentication, freshness, unique model IDs, context/output limits, integer rate units, and overflow-safe arithmetic are mandatory. All valid supported models are displayed without substitution. At submission, the gateway snapshots the authenticated input/output rates and applies a 30% markup using checked integer arithmetic and round-up division. Because model-specific tokenization is an upstream operation, the gateway first reserves the maximum cost that can fit the model context, then tokenizes and releases the excess at settlement. This prevents insufficient credit from exposing prompt bytes upstream.

## Accounting lifecycle

State is single-process, short-lived, and contains no prompts or responses:

1. A single-use submission token is bound to account, session epoch, and a digest of model plus prompt-free request metadata.
2. `reserve` atomically checks balance, account concurrency/quota, and duplicate status, then deducts the model-context maximum quoted cost.
3. A duplicate token always returns its stored terminal status and never invokes inference. Tombstones remain for the process lifetime; bounded capacity fails closed rather than evicting them.
4. Verified inference returns authenticated usage. `settle` charges actual input/output usage at the snapshotted rate and releases the remainder.
5. Missing/invalid usage, upstream failure, timeout, cancellation, or incomplete/uncertain HTTP delivery refunds the full user reservation exactly once. Measured upstream cost is an aggregate operator expense only.
6. No automatic inference retry occurs after generation may have started.

The HTML response is buffered and contains a safe form using a new token; an old token cannot regenerate. The current application settles before returning the body to the HTTP server. It cannot prove browser receipt or reliably distinguish a completed delivery from a disconnect, so conservative disconnect refund behavior is **not yet verified and blocks production**.

### Restart semantics

Phase 0 has no durable paid accounting. Operator-granted demo balances are loaded afresh on restart. Outstanding reservations disappear and are therefore conservatively treated as refunded; no inference is replayed. A random process epoch invalidates all pre-restart sessions and submission tokens. A pre-restart POST fails authentication and cannot become a fresh generation. These semantics are unsuitable for purchased credit.

## Content and privacy

Conversation history is carried by the form and exists server-side only for the active request. HTML is escaped. Model Markdown is sanitized with raw HTML and automatic remote resources disabled. Responses set a restrictive Content Security Policy, `Cache-Control: no-store`, `Referrer-Policy: no-referrer`, and `X-Content-Type-Options: nosniff`.

No production request logs or request-level traces are exported. Optional metrics are low-cardinality, time-bucketed aggregates produced locally; sparse buckets are suppressed. They never include bodies, prompts, responses, URLs, headers, addresses, agents, credentials, account/payment identifiers, stable pseudonyms, exact timestamps, or per-request token/cost events. Core behavior is unchanged when telemetry is off.

## Deployment constraints

The root filesystem should be read-only, writable storage memory-only, Linux capabilities dropped, core dumps disabled, and egress restricted to authenticated catalog/inference and aggregate telemetry endpoints where the platform supports it. Configuration is not evidence: runtime behavior must be recorded in `docs/verification.md`. Open egress, if required by the platform, is disclosed as a residual risk.
