# Phase 0 contract

## Routes

| Route | Method | Purpose |
|---|---|---|
| `/` | GET | Login or chat form and authenticated model selector |
| `/login` | POST | Exchange a provisioned recovery credential for a session |
| `/logout` | POST | End the session |
| `/chat` | POST | Validate, reserve, generate, and render one buffered turn |
| `/confirm` | POST | Authenticated no-JavaScript delivery acknowledgment and settlement |
| `/recovery` | GET | Explicitly display the user's recovery credential |
| `/recovery/download` | GET | Authenticated plain-text recovery download |
| `/claims` | GET | Current scoped privacy and verification claims |
| `/attestation` | GET | Platform quote and release/endpoint-binding evidence |

There is no public signup or inference API.

## Request limits

Limits are defensive transport controls, not a product limit on conversation history. The server admits at most 64 connections without a connection admission queue, caps each connection at ten minutes, accepts at most 64 headers and rejects aggregate header names/values over 32 KiB, and applies a ten-second total header deadline. Before consuming a body, a shared fail-fast gate reserves one conservative 128 MiB request-memory envelope from a 512 MiB application budget and retains it until the response body is consumed or dropped; consequently at most four requests are admitted. Each admitted request buffers at most 8 MiB of request body under one total 30-second deadline. Render input is checked with a conservative JSON/HTML expansion allowance before intermediates are built, and rendered responses are limited to 32 MiB. Inference responses are read incrementally, limited to 16 MiB, and covered by one five-minute send-and-body deadline. At most four generations run concurrently. These envelopes bound application-controlled request, upstream, parsing, escaped-render, and outstanding-response content across admitted requests; allocator/library overhead and the whole-process resident set still require runtime measurement. Request decompression is not enabled. Each authenticated catalog entry supplies the model's context and maximum-output limits; the complete conversation plus requested maximum output must fit that context. The gateway does not lower a model's maximum output.

## Authentication

An operator provisions a random credential with at least 256 bits of entropy and a demo-credit budget through a runtime secret. Credentials never belong in source, images, URLs, telemetry, or errors. Authentication comparisons are constant-time. Sessions are capped, expire after twelve hours, and use random opaque cookies marked `Secure`, `HttpOnly`, and `SameSite=Strict`. Login uses a single-use, ten-minute, same-site cookie-bound CSRF challenge. Other mutating forms require a session-bound CSRF token. The recovery page is no-store and offers explicit copy/download instructions without JavaScript.

## Catalog and quote

The gateway fetches a bounded catalog over the verified inference channel. Authentication, freshness, unique model IDs, context/output limits, integer rate units, and overflow-safe arithmetic are mandatory. All valid supported models are displayed without substitution. At submission, the gateway snapshots the authenticated input/output rates and applies a 30% markup using checked integer arithmetic and round-up division. Because model-specific tokenization is an upstream operation, the gateway first reserves the maximum cost that can fit the model context, then tokenizes and releases the excess at settlement. This prevents insufficient credit from exposing prompt bytes upstream.

## Accounting lifecycle

State is single-process, short-lived, and contains no prompts or responses:

1. A capped, fifteen-minute submission token is issued by the gateway and bound on first use to the account, session, process epoch, and selected model. Reusing it with changed prompt/history never regenerates, but the gateway intentionally retains no content hash and does not claim to detect which content changed.
2. `reserve` atomically checks balance, account concurrency/quota, and duplicate status, then deducts the model-context maximum quoted cost.
3. A duplicate token always returns its stored terminal status and never invokes inference. Terminal tombstones remain until the associated token expires, then are reclaimed; authentication removes expired tokens first, so a reclaimed identifier cannot be accepted or replayed.
4. Verified inference returns authenticated usage. The gateway records the prospective actual charge at the snapshotted rate, but does not settle until the authenticated browser acknowledgment.
5. Missing/invalid usage, upstream failure, timeout, cancellation, or incomplete/uncertain HTTP delivery refunds the full user reservation exactly once. Measured upstream cost is an aggregate operator expense only.
6. No automatic inference retry occurs after generation may have started.

The HTML response is buffered and contains a safe form using a new token; an old token cannot regenerate. It also contains an explicit no-JavaScript delivery-confirmation form bound to the original account, session, token, and model. Confirmation re-renders the client-carried transcript and next-turn form rather than discarding conversation history. Until confirmation, the maximum reservation remains held. The gateway fetches the catalog, issues the next token, and completes bounded rendering before the acknowledgment atomically settles; failures remain retryable, and repeated successful acknowledgments are idempotent. Cancellation, panic, or a socket disconnect before handoff refunds through reservation ownership; unconfirmed handoffs expire after five minutes and refund on the next accounting operation. Local socket, panic, expiry, rendering/catalog/token-capacity failure, retry, and settlement/refund race tests cover these transitions. Only authenticated confirmation settles actual usage; deployed behavior remains a release evidence requirement.

### Restart semantics

Phase 0 has no durable paid accounting. Operator-granted demo balances are loaded afresh on restart. Outstanding reservations disappear and are therefore conservatively treated as refunded; no inference is replayed. A random process epoch invalidates all pre-restart sessions and submission tokens. A pre-restart POST fails authentication and cannot become a fresh generation. These semantics are unsuitable for purchased credit.

## Content and privacy

Conversation history is carried by the form and exists server-side only for the active request. HTML is escaped. Model Markdown is sanitized with raw HTML and automatic remote resources disabled. Responses set a restrictive Content Security Policy, `Cache-Control: no-store`, `Referrer-Policy: no-referrer`, and `X-Content-Type-Options: nosniff`.

No production request logs or request-level traces are exported. Telemetry export is disabled and unwired. The current local-only scaffold contains allowlisted lifetime counters with sparse-value suppression; because it does not implement time buckets or resource/datapoint sanitization, it must not be enabled in production. They never include bodies, prompts, responses, URLs, headers, addresses, agents, credentials, account/payment identifiers, stable pseudonyms, exact timestamps, or per-request token/cost events. Core behavior is unchanged when telemetry is off.

## Deployment constraints

The proposed container configuration declares a read-only root, a bounded memory-only `/tmp`, a process limit, and an inference/verifier-host egress allowlist. The OCI entrypoint disables core dumps before starting the gateway. These fields and an evidence-file delivery mechanism have not yet been validated against the deployed platform schema. In particular, setting `POSSUMS_GATEWAY_EVIDENCE=/tinfoil/attestation.json` does not mount or create that file. Release requires a supported read-only delivery mechanism plus runtime verification of mounts, egress, process/core limits, and platform logging. Configuration intent is not evidence, and open egress, if required by the platform, is a residual risk.
