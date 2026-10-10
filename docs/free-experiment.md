# Free waiting-room experiment

**Status: implemented and independently reviewed; `free-v0.0.1` artifacts published and provenance verified; live deployment/E2E verification pending.** This is a separate executable/image/instance targeting `free.possums.dev`, not a change to paid accounts or a claim that the endpoint is live.

## Operator-approved contract

- API only, except `/` and `/index.html` serve the same canonical static asset with a runtime info appendix. Authenticated `/v1/info` returns the current connection count, source Git link, version and configured budget limit (plus shared spend/period), computed from bounded local state per request. Pretty-print the same typed JSON and append it after the full HTML source on a new line for both public serving and AI context. Thus these info fields are intentionally public through the static route; authentication on `/v1/info` does not make them private. No chat UI, cookies, signup, payment, recovery, balance endpoint or upstream balance lookup.
- `index.html` currently contains exactly `speak like a possum girl` followed by a newline. Preserve the complete source as the first system message on every tokenizer and generation request; append caller-provided history/messages in order, including tool history. Clients maintain continuity. Count the prefix against model context/transport bounds. Empty asset content is supported.
- Shared high-entropy Bearer admission keys, not individual accounts. Bots and multiple simultaneous connections are deliberately allowed. Operator publication of keys happens externally; never put usable keys in source, images, URLs, diagnostics or verification artifacts.
- A live streaming request holds a FIFO position after authenticated bounded upload completes. No tickets, polling, retained reconnect position or stored conversations. One completion ends that request; reconnect at the back for another turn.
- One worker handles prompt preflight, inference and terminal accounting at a time. Queue raw bounded bodies; decode only at the head. Resource limits protect capacity, not fairness per person. More than four waiting requests must be supported without inheriting paid heavy-lane admission.
- Maximum downstream connection lifetime: twelve hours, including upload, waiting and delivery. Existing shorter upload/header limits still apply. A waiting disconnect/expiry loses its position. After dispatch, downstream loss does not cancel upstream: consume the authenticated terminal outcome under existing upstream resource deadlines, without automatic replay.
- A $10 **soft stopping threshold** applies to each explicitly launched experiment run. If known settled upstream-priced usage cost is below $10, allow the next full context-legal turn even if its maximum exceeds the remainder. At $9.99, a $6.50 turn is allowed, ending at $16.49. At or above $10 admit no further turns. Never terminate an admitted turn merely for crossing $10.
- Use checked fixed-point catalog-priced authenticated final usage, without user reservations or markup. This is not independently verified invoice spending. Unknown dispatched charges, invalid/missing terminal usage, accounting overflow or worker panic stop subsequent inference. Never derive usage from response text or count unknown outcomes as zero.
- The operator approved a memory-only, deployment-scoped budget with automatic restart disabled and explicit budget disposition before a fresh process/run. No daily reset or speculative durable ledger. Verify whole-platform recovery behavior, not just `restart: "no"`, before rollout.
- Emergency reset means redeployment, booting all connections and rotating shared keys. Active inference/charges may outlive disconnect; no guarantee of cancellation or receipt. A reset is an explicit operator spending decision, not an implicit effect of key rotation, asset editing or rollback.

## Info response

`GET /v1/info` requires the shared Bearer admission key and returns pretty JSON. `live_connections` counts current downstream sockets, not detached upstream owners; the querying connection is included while open. `source` is the exact built Git commit link, `version` the free release version, `limit_usd` the configured soft threshold, `spent_usd` known settled generation cost, and `limit_period` is `run`. USD values are fixed six-decimal strings to preserve exact microUSD representation. This is not an upstream account balance; an uncertain outcome does not turn known spend into a verified total.

The public static response appends the same fields after a newline. During inference, one head-of-queue snapshot is reused for both tokenizer and generation, so spending/counts can change afterwards without changing that turn's prefix. Metadata computation is local and bounded; it does not initiate upstream inference or provider balance calls.

## Isolation and trust

Reuse the existing verified SDK transport, catalog authentication, decoder/terminal validation and bounded upload/delivery ownership. Preserve fail-closed release provenance, gateway attestation and endpoint-key binding before prompt transmission. Keep paid authentication, reservations, receipts, settlement and tests unchanged.

The free release uses a separate image/configuration and explicit free tag in the existing repository, without replacing paid latest-release discovery. Verification clients must pin the free release identity; existing paid Pi session and receipt assumptions do not automatically support this API.

The static source is part of the measured artifact. The appended info snapshot is generated by measured code from live state; its changing values are not themselves a measured artifact. Capture one snapshot at execution admission and reuse the exact augmented prefix for tokenizer and generation. Later source content changes require rebuild/redeploy and renewed verification, not a mutable unattested file. Its content may guide the model but never enforces queue, authentication or spending policy. The approved budget remains per-run, not daily; calling the limit daily does not silently authorize an automatic reset.

## Deployment and end-to-end gates

1. Preserve the separately committed API-only base and build the isolated free image from exact source. Run focused tests, paid regressions and Linux image/startup checks. Independently reproduce the image and verify signed measured configuration.
2. Confirm access to a second confidential Tinfoil instance and organization custom-domain support. Configure and verify `free.possums.dev` DNS/TLS.
3. Verify restart-stop semantics, public disconnect propagation, edge buffering, long-lived streaming and twelve-hour expiry. Backend HTTP/1 single-request closure alone does not prove physical public socket closure through a multiplexing proxy. Report incompatibilities rather than silently substituting queue tickets.
4. Deliver keys privately. Verify release provenance and endpoint-key binding before transmitting credentials/prompts. Run limited live synthetic canaries using the actual free protocol, counting costs in the run budget. Check SSE-comment decoder compatibility.
5. Read back and privately retain the original deployment's exact release/settings/secret bindings and supported restoration procedure. Do not dump secrets, request data or memory. Deploy and verify the second instance before stopping—not deleting—the original. Confirm stopped state independently.
6. Rollback stops free and redeploys the retained exact original release/settings with renewed verification. Original balances are memory-only demo credit: restart recreates configured balances, not exact pre-stop remaining credit. Do not promise memory suspension.

The custom domain is registered but DNS verification is pending. Published artifact qualification is recorded in [verification](verification.md#free-v001--native-and-published-artifact-qualification-2026-10-10); it is not proof of live deployment, public E2E or pause. Record subsequent runtime evidence separately without usable keys or request content.

## Privacy scope

Apply [PRIVACY.md: Data handling boundaries](../PRIVACY.md#data-handling-boundaries), [the experiment boundary](../PRIVACY.md#free-instance-experiment-boundary), [forbidden telemetry data](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data), [reviewed MVP scope](../PRIVACY.md#reviewed-mvp-release-scope), [processors/access/shutdown](../PRIVACY.md#processors-retention-access-and-shutdown) and [required evidence](../PRIVACY.md#required-evidence-for-every-telemetry-change).

No free-instance telemetry startup/export; set `OTEL_SDK_DISABLED=true` and omit Honeycomb credentials/egress. This does not turn off platform observability or prove anonymity/retention impossibility. Keep native-v3 signer mismatch, legacy-v2 freshness, provider billing/cache/logging and whole-platform runtime properties unresolved at their recorded scope.
