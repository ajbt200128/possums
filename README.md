# Possums

> **Draft skeleton.** Section structure and factual anchors only; the prose is
> yours to write. See [`docs/readme-rewrite-notes.md`](docs/readme-rewrite-notes.md)
> for what each section must contain, plus the setup commands.

A gateway that proxies inference requests to attested [Tinfoil](https://tinfoil.sh)
enclaves and bills them per request, plus a Pi client that verifies the gateway
before it sends anything.

**Status:** source, not audited, no public signup or payment path. Scope authority
is [`SPEC.md`](SPEC.md); long-term plans are historical
([`OVERALL_PLAN.md`](OVERALL_PLAN.md)). Release status is genuinely tangled —
several dated records report different outcomes — so state it carefully rather
than picking one version: see [`docs/verification.md`](docs/verification.md),
[`docs/tor-main-restoration.md`](docs/tor-main-restoration.md) and
[`docs/readme-rewrite-notes.md`](docs/readme-rewrite-notes.md).

## What this is

<!-- Two or three sentences. API-only: JSON /attestation and the authenticated /v1
API. No gateway Web UI, no /app, no browser cookies. Pi is the supported client;
an Obsidian plugin is an independent proposed client, not a gateway route. -->

## Security guarantees

<!-- One guarantee per bullet, each with the mechanism named in the same sentence.
Anchor statements:
  - measured backend identity: signed release manifest + enclave measurement
  - endpoint-key binding: peer must prove the endorsed SPKI before headers/body
  - EHBP-encrypted request bodies over that bound channel
  - nonce-bound freshness on /attestation/tor
  - max-cost reservation before prompt bytes leave the gateway
  - prompts are never persisted or exported; aggregate metrics only -->

## Trust boundaries

<!-- This list is a feature. Do not soften it. Anchor statements:
  - attestation proves measured identity, not impossibility of retention
  - the intake/ledger store is outside the attested boundary
  - gateway and Tinfoil see plaintext in memory
  - Pi, its extensions and the host OS are outside the measurement
  - Honeycomb is a third party outside the attested boundary
  - residual: open egress, payment metadata, timing correlation, backups/WAL,
    upstream caches, dependency compromise -->

## Quickstart: the verified Pi client

<!-- Link clients/pi/README.md once it is rewritten. Anchor commands:

     POSSUMS_PI_ROOT="$HOME/.pi/agent/install/releases/1.3.0" sh clients/pi/build.sh
     pi install /absolute/path/to/<scratch>/package

   Then /login, choose Possums, paste the recovery credential. Note that the
   credential is requested only after public evidence and key binding verify. -->

## Using the API directly (degraded)

<!-- Give this its own heading so it is never mistaken for the recommended path. -->

You get catalog discovery, reservations, receipts, streaming and billing. You do
**not** get enclave measurement, release provenance, endpoint-key binding or EHBP
body encryption. Use this only if you accept trusting the operator's TLS alone.

<!-- Six calls: /v1/auth/challenge, /v1/sessions, /v1/models, /v1/submissions,
   /v1/chat/completions, plus DELETE /v1/sessions/current. The request schema is
   deny_unknown_fields and `submission` is required, so generic OpenAI SDKs will
   not work unmodified. Billing is unknown without an authenticated receipt. -->

## Tor mode

<!-- User guide: provide an existing SOCKS listener (the client never starts Tor),
   then --possums-tor-onion and --possums-tor-proxy (default 127.0.0.1:19050).
   Proxy auth via POSSUMS_TOR_PROXY_USERNAME/PASSWORD, both or neither. Not
   combinable with --possums-manifest. Detail: docs/tor-client-setup.md. -->

## Development

<!-- Link docs/development.md. Anchor commands:

     devenv shell
     devenv shell possums-check
     devenv shell possums-test
     nix flake check --print-build-logs

   Note: the gateway needs a paired intake service for authentication and
   billing, so it does not run standalone from a clean clone. -->

## Privacy

<!-- Link PRIVACY.md. Aggregate-only observability: no prompts, responses,
   history, credentials, account identifiers, IPs, user agents or request-level
   events. Do not claim "no telemetry" or "no logs whatsoever". -->

## License

<!-- Not established in the repository yet. Add only if a licence decision is
   made; several vendored trees carry their own licences (see vendor/). -->
