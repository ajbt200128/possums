# Phase 0.1 pinned reference client

Pinned reference client for the independently verified **`v0.0.9` API release**.
`API_APPROVALS` in `approval.ts` pins its source, measured config, reproduced image
and publication identity, with administrative expiry on 2026-10-11 (not quote
freshness). The historical `v0.0.8` pin authorizes web observation only.
No credentials or conversation content leave `ReferenceClient.verified(...)`
when verification or release approval fails. Pi integration has not started.

## Client flow

Import `ReferenceClient` from a scratch-built `channel-node.mjs` or
`channel-browser.mjs`. Supply untrusted v2 bundle, provenance manifest and public
key-configuration bytes to `ReferenceClient.verified(bundle, manifest, config)`.
The installed approval, not those supplied documents, selects the identity and
origin. Every replacement API approval must be independently reviewed before
installation. The transport snapshots verification inputs and rechecks approval
before sending.

Then call `login(credential)`, `models()`, and
`chat(model, messages, onDelta, newConversation)`. Tokens stay in memory; the
client does not persist credentials/history or automatically retry generations.
Every chat refreshes model discovery and obtains a new model-bound submission.
All supported chat models remain visible, even when unaffordable. The server
revalidates the catalog and reserves maximum cost before upstream prompt traffic.

`onDelta` receives progressive plain text; render with `createTextNode`/
`textContent`, never `innerHTML`. The client does not accumulate a complete
answer. Caller/UI retention is outside this transport guarantee. A receipt is
returned only after ordered finish/settled-usage/`[DONE]` events and authenticated
stream EOF. Interruption is uncertain, not a refund guarantee or an invitation to
replay. Streaming waits use a five-minute absolute deadline, not a 15-second
idle cutoff that could interrupt legitimate reasoning pauses. A fixed `reason`
of `insufficient_credit` reports an authenticated refusal;
the selected model is not substituted or its output cap reduced.

## Deliberately narrow protocol

The server exposes challenge/session creation, session revocation, authenticated
`GET /v1/models`, submission issuance and `POST /v1/chat/completions`.
Chat accepts text `system`/`user`/`assistant` messages, `stream: true`, and the
server-issued `submission`; optional `n: 1` and
`stream_options: { include_usage: true }` are supported. Tools, tool results,
multimodal input, buffered requests and output-cap overrides are rejected.
Tool-call compatibility is **not validated or advertised for any model**.
This is OpenAI-style SSE, not a claim of general SDK/Pi drop-in compatibility.

Ordinary HTTPS for outer authorization headers and bodyless discovery is the
accepted contract: TLS intermediaries can see these. This reference client uses
attested-key EHBP protection for prompt-bearing requests and streamed bodies.
Arbitrary API callers are not proof of reference-client verification or encryption.
External v2 verification does not provide v3 nonce/witness freshness, quote-age,
AMD CRL/OCSP, or whole-process RSS guarantees.

## Verification artifacts

`build.mjs` requires a scratch install and emits reproducible Node/browser bundles
and a source/dependency/output hash report. Locked installs disable package scripts.
The fixture-capable bundles are separate test artifacts: never promote them to
production or add synthetic trust to production clients.

Tests: `tests/phase01_transport.mjs` (published verifier/actual pinned shim),
`tests/phase01_client.mjs` (catalog and ordered streaming parser, Node/Chromium),
and `tests/phase01_api.mjs` (actual shim → Rust API → synthetic inference,
Node/Chromium). `examples/phase01_fixture.rs` is the non-production backend for
the last test. Funded `v0.0.9` checks exercised authenticated catalog discovery,
progressive encrypted completion, final settled receipts, terminal duplicates,
and settlement after disconnect. The actual reference flow also passed in Node
and Chromium with supplied verified collateral, safe text DOM and no browser
session storage. Autonomous browser collateral acquisition remains unqualified;
physical Android is excluded by the user's scope decision. See `docs/phase01.md`
for the scoped acceptance record and remaining unknowns.
