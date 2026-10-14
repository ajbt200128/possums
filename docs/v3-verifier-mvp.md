# Shared v3 verifier MVP — scoped source evidence

## Scope and status

Operator-approved first packet: one official-Go-backed offline verification core, reused by the native gateway helper and a standalone Node/WASM bridge. Broader connection/client-engine work is [deferred](backlog.md#shared-verified-connection--possums-client-engine-after-the-v3-verifier-mvp) until concrete bugs or development friction justify it; that backlog is not implementation authority.

**This is a verifier-core MVP, not a completed Pi attestation migration or a production qualification.** The existing Pi bootstrap still uses its accepted pinned JavaScript legacy-v2 verifier. No client was installed/activated, provider loaded, gateway/application endpoint probed, inference submitted, secret accessed, gateway deployed or PR opened for this packet. Rust's separate upstream inference verifier is unchanged.

## Implementation

- `attestation-helper/verifier`: real default-root/default-clock SDK `VerifyV3`, caller-owned 32-byte nonce, fixed Possums workload, 16 MiB evidence ceiling, and closed failures. No custom hardware/signature/transparency verifier or test-root/clock override in production artifacts.
- Preserve the narrower Possums publisher authority: exact release workflow, semver tag, GitHub OIDC issuer/hosted runner, matching signed source/config/signer commits and refs, and deployment-artifact digest/name. Supported Sigstore/X509 helpers inspect authenticated publisher metadata; repository membership alone is insufficient.
- Project endorsed TLS fingerprint, optional native HPKE key, release tag/digest and SDK-authenticated freshness expiry. Native helper response fields and Rust appraisal/transport checks remain unchanged; its source now additionally enforces the shared narrow publisher policy. Real release acceptance under that added policy remains unqualified.
- `cmd/verifier-wasm`: bounded JSON ABI; `clients/pi/verifier.mjs` and `verifier-worker.mjs`: one disposable worker per appraisal, bounded verdicts, 30-second deadline, abort/cleanup handling, mandatory client HPKE and expiry rechecks. A primary failure survives subsequent cleanup failure; otherwise cleanup failure has its own closed cause-unknown code.
- The bridge returns facts, **not an authorized connection**. It performs no acquisition, credentials/catalog/inference, authorization caching, renewal or retries. Actual TLS/EHBP binding, exact manifest/approval checks and lifetime enforcement remain mandatory consumer responsibilities.
- Standard Go `js/wasm`, CGO disabled, matching `wasm_exec.js`, no native client-binary matrix. Inherit the pinned SDK's own in-toto/logger portability replacements explicitly because dependency `replace` directives are not inherited. SDK pin remains `v0.17.0-rc.1.0.20261009012920-31c57af7d7b4`.
- No Tor signer, application-key API/private-key grant, onion identity, connection engine or accounting rewrite is included.

## Offline evidence

Pinned environment: `nix develop`, Go 1.27.2, Node 24.13.0; native host Darwin/arm64. Scratch native/WASM builds succeed; the WASM artifact is approximately 37 MiB. The generated build report records source/artifact SHA-256, compiler/runtime and SDK pins, not independent binary provenance or runtime authenticity. Go runtime license accompanies the artifact; complete dependency-license/distribution review remains a release gate.

Verified in this packet:

- Native Go tests, race tests, `go vet ./...` and `go mod verify` pass.
- Nix native-helper derivation builds with the updated fixed vendor hash.
- Generated-artifact Node suite passes **34 checks**: artifact hashes, real SDK envelope rejection, input bounds/workload/nonce, abort before/during work and during cleanup, missing HPKE, expiry, malformed/hostile worker output, unavailable artifact, silent exit, deadline and primary-error preservation. Successful projections use an explicitly synthetic worker, not forged trusted evidence. Deadline acceleration and cleanup faults exist only in tests.
- Native Go exercises real SDK rejection of a structurally built v3 document with untrusted quote/provenance. Publisher/key/deadline positive unit projections are synthetic and do not establish cryptographic acceptance.
- Full Rust library/integration tests pass: **478 passed, four explicitly funded live tests ignored**. This includes attestation, trust, default API contracts, accounting and privacy regressions. No new client-visible gateway wire fields or settlement/retry behavior are introduced.
- Active changed-file LSP checks and `git diff --check HEAD` pass. Existing vendored dead-code compiler warning is unrelated.

Reproduce scratch builds only:

`nix develop --command sh scripts/build-v3-verifier.sh`

Use the printed scratch directory:

`nix develop --command node tests/v3_verifier_wasm.mjs /absolute/scratch/directory`

## Privacy and diagnostic boundaries

Applicable policy: [data handling](../PRIVACY.md#data-handling-boundaries), [typed interface failures](../PRIVACY.md#mandatory-typed-interface-failures), [synthetic testing](../PRIVACY.md#synthetic-testing-exception), [Pi failure policy](pi-client-failure-policy.md), and [public-evidence cache limits](pi-public-evidence-cache-plan.md).

The bridge accepts public evidence and caller-owned expectations only. Worker Go environment is empty; unexpected fetch attempts fail; Go writes and worker stdout/stderr are discarded rather than exported or retained. Closed errors never attach raw SDK errors, panic payloads or stack traces; their locally authored messages give code/stage/constraint/action and say when the cause is unknown. Tests exercise hostile diagnostic text and cleanup races. Diagnostics initiate no inference and assert no billing/refund outcome. No logs, traces, metrics/exporters, support bundle, verdict persistence or public-evidence cache is added.

Native subprocess diagnostic propagation is still limited: the shared Go core preserves closed SDK-stage failures, but `main.go` exits nonzero and the unchanged Rust wrapper observes its existing process-exit category rather than that finer cause. Carrying the safe closed projection across that project-controlled IPC boundary remains unimplemented; this packet does not claim end-to-end diagnostic coverage.

WASM/workers are **not a general security/privacy sandbox**: Node still grants host capabilities and caller memory contains supplied bytes. SDK offline behavior, bounded input/output and explicit host controls are scoped evidence, not a proof of whole-process nonretention. Existing Pi/T3 history/persistence and the authorized public-certificate cache remain separate boundaries.

## Required remaining migration gates

These are necessary migration work, not optional broader-engine backlog:

1. Positive authenticated hardware/Sigstore/publisher/freshness acceptance and negative conformance fixtures through both native and WASM paths, using reproducible authorized evidence. Compilation/rejection tests do not establish all crypto branches, performance, resource bounds or current live release acceptance. No positive WASM fixture, live application key or Linux/Windows runtime qualification was performed.
2. Wire the current Pi/reference bootstrap to nonce-bound v3 acquisition and the packaged bridge; retain exact approved manifest/source/publisher checks, actual TLS/EHBP binding and the minimum of current 12-hour, certificate and authenticated freshness deadlines. Unchanged witnesses must not renew authorization; nonce-dependent report hashes must not become deployment/cache identity.
3. Preserve the closed SDK/native-helper failure through the bounded Go/Rust IPC boundary without changing shipped-client wire semantics; add synthetic known/unexpected/cleanup cases. Qualify the candidate built artifact in the pinned Pi host, preserving native renewal eligibility, trust-versus-operational-stop policy, no inference replay, provisional-tool suppression, receipt/accounting evidence and memory-only session secret scope. No global Pi/T3 changes.
4. Exercise the candidate gateway with exact retained shipped client artifacts and current client, including legacy bootstrap/default wire behavior. Current Rust regressions are not a substitute for that paired qualification. No breaking contract without explicit approval; do not weaken trust/accounting/privacy for compatibility.
5. Separately authorize installation/user activation, any live/public gateway verification or funded inference, and deployment. Preserve existing historical verification limits; this packet does not retrospectively qualify older releases or establish newestness, antirollback or immediate revocation.
