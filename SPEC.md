# Possums Phase 0 specification

Possums Phase 0 is a single attested clearnet gateway for manually provisioned demo accounts. It serves a plain HTML, no-JavaScript chat UI and sends prompts only to an attested Tinfoil inference endpoint after all security gates pass.

`AGENTS.md` is authoritative where older designs disagree.

## In scope

- HTTPS clearnet gateway deployed as one measured image
- out-of-band verification through `GET /attestation`
- manually provisioned high-entropy recovery credentials and demo balances
- live, authenticated Tinfoil model catalog and model selector
- buffered chat with model-specific context validation
- in-memory reservation, settlement, refund, concurrency, quota, and idempotency state
- aggregate, locally sanitized operational metrics, disabled by default

## Not in Phase 0

Tor/onion services, payments, Stripe, Monero, a store/database service, `/app`, a public OpenAI-compatible API, CLI/SDK clients, replicas, and an external audit are future work. They must not be shipped in the Phase 0 image.

## Security boundary

The gateway and Tinfoil inference service see prompt and response plaintext in memory. Prompt content is not intentionally persisted or exported. Attestation identifies measured code and configuration; it does not prove that retention is impossible after runtime compromise. The operator, cloud host, dependencies, TLS termination, open egress, timing, account reuse, inference caches, and platform logging remain residual risks.

Before transmitting prompt bytes, production must verify gateway release provenance and serving-endpoint key binding, Tinfoil attestation and release provenance, an authenticated fresh catalog, model/context limits, and a successful credit reservation. Any missing or unknown mandatory evidence fails closed.

Detailed behavior and unresolved platform claims are in [`docs/phase0.md`](docs/phase0.md) and [`docs/verification.md`](docs/verification.md).
