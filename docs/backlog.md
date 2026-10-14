# API-only backlog

These proposals are not implemented or authorized telemetry changes. The gateway target is JSON `/attestation` and `/v1` only; no Web UI or `/app` is deferred.

## Shared verified connection / Possums client engine (after the v3 verifier MVP)

Operator-requested backlog, not permission to implement or activate a broader client engine. Start with the shared offline Go v3 verifier, native gateway wrapper and WASM client bridge. Revisit these ideas only when observed bugs or development friction justify the additional scope:

- **Verified connection ownership:** consider sharing acquisition, expiry enforcement, renewal state and TLS/EHBP binding if callers repeatedly miss a binding/deadline or need the same fixes. First assess the supported Go `enclave.Handle` and transport helpers; keep Unix-socket versus HTTPS acquisition and Pi lifecycle authority in thin adapters. Do not grant new renewal eligibility, retries or inference replay.
- **Possums client engine:** consider native/WASM authentication, catalog/submission handling, supported EHBP encoding/stream decoding and receipt interpretation if duplicated protocol fixes or cancellation/backpressure bugs recur. Qualify WASM networking, memory, streaming, cancellation and actual Pi host behavior before replacing the current TS client. No generic transport framework or speculative adapters.
- **Accounting boundary:** client receipt validation/reconciliation may be shared; authoritative reservations, settlement and refunds remain server-owned in Rust. A Go accounting rewrite is a separate decision, not implied by verifier reuse.
- **Application-key/Tor consumer:** later use fresh verified v3 material for exact key A, then a purpose/challenge-bound A-to-onion-B endorsement and connected-address comparison. Arti, persistent onion identity/storage, signing endpoints and private-key grants are not part of the verifier MVP. Application keys rotate independently of the future persistent onion identity.

These proposals preserve the operator-approved no-replay/failure policy, authenticated completion/tool gates, key/prompt-cache scope, public-evidence persistence limits and existing shipped-client contracts. They authorize no global Pi/T3 routing or telemetry changes. Installation/activation, live evidence, secrets, paid inference and deployment require separate authorization. An error is a reason to evaluate scope, not automatic permission to implement it. See [v3 verifier qualification](v3-verifier-mvp.md).

## Optional Tinfoil web search through API clients

- Explicit per-turn opt-in; disabled by default. Disclose that conversation-derived search terms may reach Tinfoil's external provider Exa. Treat results/citations as hostile data, rendered safely by each independent client, not the gateway.
- Verify authenticated usage and fee. Reserve maximum model cost plus any search fee before prompt transmission; settle once from authenticated usage. Do not assume the fee is included in catalog token prices.
- Clients may instead let users search independently and paste excerpts. Never automatically put a private prompt in a search URL.

## Ephemeral document upload through API clients

- Opt-in single-turn attachments via verified document-processing/chat path, without persistent gateway library or index.
- Verify attestation, endpoint binding, formats, limits, conversions and charges. Bound uploads/extracted content and account for model context and maximum billable cost before upstream transmission.
- Treat filenames and content as hostile; no gateway persistence or telemetry export. Review service retention and privacy before release.
