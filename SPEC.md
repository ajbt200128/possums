# Possums API-only gateway specification

**Tor source-restoration candidate; not activated or deployed.** The operator
authorizes restoring optional onion ingress, fresh nonce-bound
`/attestation/tor` endorsements and matching Pi native SOCKS transport/trust
integration on current main. Default measured configuration and deployment
bindings remain unchanged: source capability is not activation authority.
No direct fallback, uncertain inference replay, startup self-appraisal gate or
post-launch diagnostic observer is introduced. Local A consistency and approved B
custody precede launch; clients independently verify fresh nonce/release/A/B/TLS/
HPKE binding before credentials or inference. Health uses Arti's fully-reachable
assessment, not proof of availability or attestation; health flapping remains
unresolved. See [current scope and evidence](docs/tor-main-restoration.md).
The [HTTPS-only backup record](docs/without-tor-backup-verification.md) remains
historical, including its compatibility limitations. No merge, release, deploy,
client installation/activation, runtime/key-binding mutation or live probe is
authorized by this source restoration.

**API-only source contract; deployment pending.** The existing v0.0.15 deployment and previous Phase 0 browser behavior are historical; see [phase evidence](docs/phase0.md) and [verification](docs/verification.md). This document does not certify an API-only deployment. `AGENTS.md` is authoritative where historical plans disagree.

The measured clearnet gateway serves JSON `GET /attestation` and the authenticated `/v1` API (challenge, sessions, submissions, model discovery, streaming chat completions). No gateway HTML, `/app`, browser session cookies, CSRF forms, recovery/download or claims pages, web streaming, or browser UI is offered or planned. The Pi client remains supported. Obsidian is an independently proposed API client running outside the gateway, not a deferred gateway UI.

Manually provisioned high-entropy credentials and demo balances remain memory-only; no public signup, payments, durable paid accounting or automatic inference replay is introduced by this cutover. Authenticate the live Tinfoil catalog and measured endpoint-bound transport before prompt transmission. Enforce context limits and a checked maximum-cost reservation at the submitted catalog price; keep supported models visible and reject unaffordable requests before upstream prompt bytes, without substitution or an artificial output cap. Stream bounded inference without collecting a complete response. Operator-approved Tinfoil-aligned settlement uses validated authenticated final usage on successful stream termination, or trustworthy cumulative running usage on interruption, at the submitted quote and capped by the reservation. An observed downstream client disconnect promptly closes upstream work; temporary delivery backpressure is not itself a client disconnect. Release the unused reservation and refund fully when no trustworthy usage exists; model rejection and failures before responding are not billable. Never sum cumulative snapshots or estimate usage from text. Partial charges do not establish successful completion or authorize tool execution. Closing upstream does not prove immediate compute termination or exact provider invoice parity; unobserved provider cost remains with the operator. Opt-in response-side usage metrics remain private first-party billing data, not observability export; existing authenticated SSE receipts remain supported. Source implementation and verification status must be recorded separately from this policy approval. A new client retry can incur another charge after an uncertain outcome. Keep short-lived prompt-free idempotency and accounting state, defensive transport/parser limits, and aggregate-only privacy controls under [PRIVACY.md](PRIVACY.md).

The separately authorized local Pi public-evidence cache may retain GitHub manifest/provenance and public AMD VCEK DER including its embedded gateway hardware ID. It never retains bare chip IDs, raw attestation reports, full KDS URLs or verification decisions; every process independently verifies the complete current report, certificate chain, publisher/artifact and live key binding. This is a narrow public-certificate persistence exception under [PRIVACY.md](PRIVACY.md#data-handling-boundaries), not telemetry or a deployed/installed AMD-cache claim. See [cache evidence](docs/pi-public-evidence-cache-verification.md).

Attestation authenticates measured code/configuration and endpoint binding, not impossibility of retention after compromise. Platform signer/native-v3 CLI, legacy-v2 freshness, provider invoice/maximum billable cost and deployed runtime/privacy limitations remain open at their recorded scope. Local API-only source and fixture checks are recorded in [verification](docs/verification.md#api-only-cutover--local-source-verification-2026-10-09). Existing browser/streaming release tests do not demonstrate the API-only image or deployment; historical results remain unchanged.
