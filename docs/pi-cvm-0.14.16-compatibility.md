# Pi CVM 0.14.16 compatibility — source/scratch verification

## Scope and observed blocker

2026-10-24. The operator requested preparation and testing after a fresh T3/Pi
session failed with `possums_trust_manifest`, stage `trust`, constraint `manifest`.
This authorizes source preparation and offline checks only, not installation,
activation, merge, gateway deployment or paid inference.

A bounded credential-free public diagnosis appraised fresh nonce-bound gateway
v3 evidence with the installed offline Worker/WASM verifier. It authenticated
release **v0.0.68**, downloaded its exact public manifest, checked its SHA-256
against the appraised release digest, and independently verified the complete
signed manifest association. That manifest selects **v0.14.16**, with nonempty
config and a matching exact config-command-line hash. These checks sent no
credentials or inference and did not establish complete client/channel readiness,
authentication, catalog availability, earlier billing or whole-service health.
No evidence, nonce, key facts or verdict was written to a diagnostic artifact.

The installed bundle and current client source require **v0.14.12** in both the
v3 and legacy qualification paths. This establishes a deterministic client
compatibility blocker for the reviewed [CVM upgrade](cvm-0.14.16-upgrade.md),
not evidence that the user's credential is invalid. The preceding separate
`possums_evidence_unavailable` / `gateway_attestation` / `request` failure's
transport cause remains unknown; this patch does not fix or reclassify it.

## Minimal source change

- Share one exact, type-safe CVM-version check between v3 and legacy clients.
  Accept only **v0.14.12** and **v0.14.16**; there is no range, prefix, coercion,
  runtime-configured override or remote-controlled policy.
- Retain v0.14.12 for previously supported gateway releases. Historical compiled
  approval bytes, original endpoint and expired administrative deadline are
  unchanged; this does not renew that approval.
- Preserve exact manifest/config hashes, signed predicate equality, supported SDK
  appraisal, publisher/release association, hardware verification, freshness,
  revocation checks, certificate and endpoint-key binding, 41-byte EHBP config,
  retry/recovery policy, accounting and authenticated tool-completion gates.
- No wire-contract, gateway, dependency, cache, telemetry or host changes.

## Offline reproduction and acceptance

Fresh scratch builds used the repository's flake environment with explicitly
selected pinned **Node 24.13.0**, **TypeScript 5.9.3**, **esbuild 0.25.10** and
**Pi 1.0.4** peers. The flake shell's default Node did not match the client build
pin; selecting the already available pinned executable resolved that prerequisite
without changing the flake or installing a runtime. Existing Go/WASM and native
Tor scratch artifacts were reused only after their reports matched current
helper sources; normal packaging verified artifact/source hashes. Locked npm
dependencies were installed only into fresh build scratch, not the checkout or
host client installation.

- Before the fix, both updated `phase02_v3.mjs` and `phase02_release.mjs`
  reproduced **`possums_trust_manifest`** with valid synthetic v0.14.16 fixtures.
- After the fix, the normal scratch package build and complete
  `clients/pi/check.mjs` passed: legacy release **194**, v3 **69**, packaged real
  Worker/WASM/Pi rejection **18**, native Pi **162 groups**, peer-key recovery
  **18**, discovery **62**, public-evidence cache **40**, v3 manifest cache
  **72**, and the remaining client, stream, thinking, Tor and admission suites.
- Both runtime versions qualify through actual client factories under the
  suites' explicitly synthetic crypto boundaries. Unsupported adjacent/future,
  unprefixed, suffix/prerelease/newline, hostile and non-string/missing version
  inputs fail closed. The v3 version rejection occurs before SDK appraisal.
- Existing negative trust/config/freshness/key/terminal tests now run against
  the v0.14.16 fixture. Legacy hardware/DSSE/X509/public acquisition and v3
  appraisal remain mocked in those positive synthetic tests; actual loopback TLS,
  SDK orchestration and packaged real WASM negative tests remain separate.
- Explicit `tsc --noEmit` passed for both `clients/pi/tsconfig.json` and
  `examples/phase01/tsconfig.json` in the scratch tree. SHA-256 comparisons
  confirmed that both changed source files and both test files exactly matched
  the tested scratch copies. The reference project's unchanged `tsconfig.json`
  was copied separately because the package builder copies only its `.ts` files.
- Checkout LSP reported unresolved Node/verification dependencies because this
  checkout intentionally has no installed client dependencies. It is not a clean
  LSP result; the exact-source pinned scratch typechecks above passed instead.
- Independent read-only review found no actionable code/test findings. The
  reviewer did not independently rerun the reported build or suites.

Prepared scratch extension SHA-256:
`d16b1eaf3af596bfc126088a75d832ce824b338f9f4202b67d48a2d2bb2c48ab`.
Its bytes match its build report. The installed extension remains unchanged at
`38536f556cbe27503df8b6072d62de9e51338e491bbca5157f1d50b84fc9e1c0`;
the stable installation link was not changed.

## Compatibility, privacy and remaining gates

The unchanged installed artifact's exact v0.14.12 check was inspected, not
replaced. The pre-fix reproduction used a fresh build of the current source,
not a positive full execution of the previously shipped artifact against a new
live gateway. **The installed client still rejects v0.14.16.** There is no claim
that a gateway update can make that old client compatible, that the prepared
package is installed/activated, or that a scratch build repairs a running T3
process. Positive end-to-end candidate startup, authentication/catalog and host
rendering remain unverified. No automatic install, process restart, inference
retry/replay, live Tor probe or gateway update follows from these checks.

Privacy scope: [data boundaries](../PRIVACY.md#data-handling-boundaries),
[forbidden request-level exports](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[typed failures](../PRIVACY.md#typed-interface-failures-and-token-aggregate-extension)
and [synthetic testing](../PRIVACY.md#synthetic-testing-exception). Tests use
synthetic fixtures and isolated temporary homes/caches; no diagnostic logger,
raw exception export, telemetry or new persistence was added. The manual public
check retained only the public release/runtime and content-free results recorded
above. Pi/T3 may retain ordinary messages and safe diagnostics in native history;
no whole-process transience or upstream erasure claim is made. The exact upstream
CVM [observability risk exception](../PRIVACY.md#exact-cvm-01416-upstream-observability-exception)
is unchanged, not proof of deployed privacy properties.

Any later installation/activation needs separate operator approval and matching
artifact/source qualification. This record is not deployment or release evidence.

## Subsequent operator-authorized local installation

The operator then explicitly requested installation for their own testing. The
source/scratch-only and unchanged-installation statements above describe the
preceding qualification phase, not the installation state after this section.

Before changing the stable link, all **8 artifacts**, **165 bundled input hashes**
and **22 bundled project-source hashes** matched the qualified report/current
source at commit `088ab8c7`. The candidate was copied into a new local package;
only its three external Pi peer links were aligned to the already-installed
**Pi 1.3.0** runtime. No Pi runtime or dependency version was installed/updated.
The package's original build report still accurately describes its **Pi 1.0.4
build qualification**; it does not claim the installation peer links are 1.0.4.

The staged actual package then passed all **18** `phase02_v3_package.mjs`
checks on **Pi 1.3.0**, using an isolated temporary home, in-memory native session
and synthetic public acquisition. This exercises extension registration,
session-start failure handling, offline status and packaged real Worker/WASM
rejection; it is **not** positive live startup/authentication or full Pi 1.3.0
qualification. No credentials or inference were used by these checks.

The existing `~/.local/share/possums/pi/installed` symlink was atomically replaced:

- New target: `pi-cvm01416-d16b1eaf3af5-pi130`.
- Extension SHA-256:
  `d16b1eaf3af596bfc126088a75d832ce824b338f9f4202b67d48a2d2bb2c48ab`.
- Previous target retained unchanged: `pi-intake-v66-38536f556cbe-pi130`.
- All 8 installed artifact hashes and all three unchanged Pi 1.3.0 peer targets
  were verified through the stable link after replacement; the previous
  extension's hash also remained unchanged.

No registration, provider/model selection, settings, credentials, caches,
history, T3 host, gateway or running process was changed/restarted. The verified
artifact is installed, but **activation in existing Pi/T3 processes and positive
live behavior remain unverified**. The user controls a fresh provider process
or full application restart and their deliberate test; this installation sends
no inference, resumes no failed conversation and establishes no billing outcome.
