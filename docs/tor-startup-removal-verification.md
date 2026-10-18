# Tor startup self-appraisal removal — source-only candidate

> Current source-restoration scope: [Tor main restoration](tor-main-restoration.md).
> The startup-removal trust decision below is retained; historical tests are not
> new verification or activation permission.
>
> Historical evidence only in this inactive HTTPS-only backup. All Tor support,
> not just startup self-appraisal, is removed. Commands and authorizations below
> do not apply here; unresolved evidence remains historical. See
> [backup verification](without-tor-backup-verification.md).

## Explicit operator decision and changed trust boundary

The operator chose to remove Tor **startup** self-appraisal completely after
local attestation acquisition returned HTTP 503. This narrowly supersedes the
former fresh-appraised-A-before-B startup requirement in
[identity binding](tor-identity-binding.md) and the
[dual-endpoint plan](tor-dual-endpoint-plan.md). The historical implementations,
reviews and acceptance records are not rewritten as if that requirement never
existed. [Repository policy](../AGENTS.md#security-model) records the amendment.

Preparation, bootstrap and launch no longer acquire a nonce-bound appraisal,
hold `TorGatewayEvidence`, or check cached appraisal freshness. `PreparedTor`
takes only runtime configuration. The mounted PKCS8 private grant and SPKI public
grant must still decode as Ed25519 and derive the same public A. This is **local
consistency, not appraised authenticity**. The maintained PEM parser also rejects
non-whitespace trailing material after the selected grant block, covering the
requested trailing-PEM negatives; no custom ASN.1 or crypto parser is introduced.
Approved fixed B, native private/public identity consistency, strict production
fs-mistrust ancestry, no-overwrite import into Arti's memory-only primary and
protected ephemeral local state remain unchanged. No replacement A/B generation,
identity fallback, persistent state, debug override or feature guard is added.

**Publication may now precede any successful self-appraisal**, including while
`/attestation/tor` cannot acquire evidence. SDK Running/health readiness does not
establish attestation availability or an authenticated client connection. This
can remove a startup dependency; it does **not** repair the underlying acquisition
503, identify its provider-side cause, or qualify Tor-client E2E.

The per-client-request `src/server.rs::tor_attestation` path is byte-for-byte
unchanged: fresh client challenge passed to `verify_tor`, current appraised-A
comparison with the held signer, endorsement of only fixed B, bounded canonical
response and expiry recheck immediately before bytes. Unavailable/malformed,
stale, mismatched-A or failed-trust evidence still returns 503 without an
endorsement. Clients independently retain nonce/publisher/release/A/B/TLS/HPKE/
freshness and selected-endpoint checks. Native Go `VerifyTorClient`, manifests,
Pi clients and helper acquisition/readiness deadlines are unchanged.

Ordinary gateway attestation, verified upstream connection, authentication,
catalog/context validation, pre-prompt reservation, settlement/receipts and tool
authorization are unchanged. Biased owned startup cancellation, the shared 64
permits/task set/original accepted-work drain, two-permit bridge/max-32 pairing,
backpressure and whole-upstream-close behavior are unchanged. No inference,
automatic retry/replay, new telemetry, deployment or production operation follows.

## Completed offline verification

Based on current fetched `origin/main`
`8f0450419c2b9c8e1cd371c630736beae86acea4`, in a separate local worktree; original
`feat/tor-v0` checkout and sibling worktrees were not modified. No nested agents.
Darwin/aarch64, pinned root `nix develop --offline`: Rust/Cargo **1.92.0**, Go
**1.27.2**, root Node **24.20.0**. Client scripts explicitly require **24.13.0**;
those checks used the existing Nix-store Node 24.13.0, without changing flake,
lockfiles or configuration. Python **3.12.12** was resolved from this flake's
`nixpkgs` `python312`, not the host's Python.

- Full default Rust: **351 library + 6 binary + 174 integration = 531 passed**.
  Full all-feature Rust: **406 + 6 + 174 = 586 passed**. Four existing live/funded
  probes remain ignored in each; zero doc tests. Both use `--offline --locked`.
- Focused runtime **16 passed**, including successful production preparation
  after synthetic mounted-input substitution with unavailable/panic verifiers in
  HTTP state, held approved B and bootstrap inputs, and ready cancellation of
  actual prepare/bootstrap futures before polling. No TorClient/network/bootstrap
  is constructed by these tests. Source inspection confirms no verifier,
  appraisal or freshness dependency remains through actual bootstrap/launch;
  offline cancellation is not a completed Tor bootstrap test.
- Local grant mismatch, Ed25519/OID/DER/PEM/trailing/size/read negatives, wrong B,
  no-generation/no-overwrite custody and strict production ancestry fixtures pass.
  Disabled mode still reads only the explicit switch, with no secret/grant/SDK
  reads. Existing actual Axum Tor route **5 passed**, covering challenge/query
  rejection, helper/acquisition 503 and all closed trust categories, stale or
  mismatched A, bounded encoding and no-endorsement failures through Node decoding.
- Strict all-target default/no-default-feature and all-feature Clippy, package
  formatting, explicit included-test Rustfmt and whitespace checks pass. The
  existing vendored Tinfoil dead-code warning is unchanged. Active path-scoped
  LSP probes report zero diagnostics across the five changed Rust files.
- Unchanged helper: Go tests, race tests, vet, module verification and shared-core
  js/wasm tests pass with `GOPROXY=off GOSUMDB=off GOTOOLCHAIN=local`.
- A new scratch Pi **1.0.4** package was rebuilt offline using byte-matching client
  sources/dependencies from an existing scratch build and source/hash-checked
  verifier/native artifacts. Complete `check.mjs` passes, including **69** Tor
  channel, **36** direct-v3, **691** reference-client, **179** retained legacy-v2,
  **18** packaged-host, **164** Pi SDK and **31** discovery checks. Existing built
  WASM **40 + 31**, endorsement **55**, Tor setup **193**, and native connector
  **87/87** checks pass. Synthetic local TLS/HTTP/appraisal is not hardware trust.
- Release automation **85**, image-health guards **9**, developer-helper stubs
  **7** pass with flake-pinned Python. Full Rust includes startup diagnostics
  hostile-data/protobuf, disabled/outage independence, owned cancellation,
  local-TLS delivery and actual-binary privacy negatives.

Intermediate fixture/setup runs were corrected before these completed passes:
an empty synthetic account was invalid; a Node-pin mismatch and stale helper
selection did not qualify client tests. One stale-helper run ended with exit 137
(the exit cause is unknown). The completed 87-check run used the source/hash-
checked helper actually packaged in the new scratch build; tests were not
weakened. No dependency installation, runtime activation or live probe was used.

## Privacy, compatibility and remaining limits

Applicable policy: [data handling](../PRIVACY.md#data-handling-boundaries),
[forbidden telemetry](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[typed failures](../PRIVACY.md#mandatory-typed-interface-failures),
[startup diagnostics](../PRIVACY.md#startup-diagnostics-exception) and
[synthetic testing](../PRIVACY.md#synthetic-testing-exception). Superseded startup
nonce/appraisal diagnostics and their fixtures are removed, while closed local
failures retain useful content-free actions. Per-request diagnostics, schema,
metric/export policies and no-replay/unknown-billing behavior are untouched.
Tests use public synthetic material only; no raw exceptions, evidence, credentials,
onion identities or private data are added to repository/support artifacts.
Arti failfast/native panic hooks and Pi/T3 host history remain separate boundaries.

No wire field, optional extension, serializer or client decoder changed. Current
client and retained legacy fixtures passed; no old shipped full-client artifact
was newly qualified against a live/candidate gateway. The previously recorded
[old-stream decoder incompatibilities](verification.md#typed-interface-errors-and-token-aggregates--source-candidate)
are not resolved by startup removal. Native-v3 platform-signer and legacy-v2
freshness limits, mounted-grant authenticity/rotation, exclusive B publication,
correct-replica shim routing, measured Linux artifacts/resources and live
billing/privacy/Tor-client E2E remain unqualified. Existing 30-second Tor helper
503 readiness/45-second owner bounds and the provider's reported cooldown are
not changed. Parent security review is required before merge or any new release;
no rollout-exemption label, merge, tag, release or production mutation is authorized
by this source record.
