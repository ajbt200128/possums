# Inactive HTTPS-only backup — local verification

**BACKUP ONLY: not merged, installed, activated, released or deployed.**
Worktree: `/Users/mb5/projects/possums-backup-without-tor`, branch
`backup/remove-tor`. The removal was safely rebased onto exact PR90 main
`ca7243b88a6b5b11a930ff743206589633a497d6`; no base gap remains against that
requested commit. Implementation commit:
`30d2c902668ce0675d3b7ae4cddb3830a3dd5bf5`.
The parent's independent r62 release qualification neither qualifies nor blocks
this local backup. Later main changes are not implicitly included.

## Removed and preserved

- Removed Arti construction/bootstrap/publication, opaque ingress/bridge,
  endorsement/application-key binding, `/attestation/tor`, runtime settings,
  Tor mounts/key grants/secret references in measured source configuration,
  direct Tor dependencies, native client helper, SOCKS/onion selection and
  transport, packaging, spikes, probes, fixtures and Tor-only CI targets.
- Removed PR90's five readiness files, observer/delivery hooks and policy
  exception. No non-Tor Rust runtime `libc::` use remains; removed the direct
  Unix libc dependency/root lock edge. Necessary transitive libc remains.
- Retained verified credentialed HTTPS, official v3 appraisal, nonce,
  publisher/release/manifest/freshness checks, actual endpoint TLS/HPKE binding,
  supported SDK decoding and closed diagnostics. No fallback or inference replay.
- Retained HTTP's single 64-permit connection owner, JoinSet and lifecycle
  fence/drain; backpressure is not disconnect. Ordinary disconnect/upstream
  closure, maximum-cost pre-reservation, cumulative/final usage, settle-once,
  refunds and authenticated completion/tool gates are unchanged.
- Retained general startup diagnostics (five stages plus terminal, six records),
  primary-failure preservation, two-second shared delivery budget and off switch.
  No startup self-appraisal was reintroduced. No new telemetry or logging.
- Retained UID/GID 65532 NSS records, read-only root, tmpfs and general health/
  startup checks. Exact-tested/current-main, source/image-only child, A/B image,
  strict cache partition, provenance, tagged CI, protected approval and separate
  serving gates remain. The cache recipe only drops removed Tor graph members;
  `scripts/check-release-cache.py` is unchanged.
- Removed first-Tor activation/binding/exclusivity rollout rules. Existing opaque
  configuration must remain identical: no runtime secret selection/deletion.
  A plan requiring configuration drift must stop for separately approved
  migration, not weaken this guard. The image digest remains a historical
  template; it was not replaced with a purported backup image.

## Executed checks after PR90 composition

Darwin arm64; pinned Rust/Cargo **1.92.0**, Go **1.27.2**, Node **24.13.0**,
Pi **1.0.4**, TypeScript **5.9.3**, esbuild **0.25.10**. Rust used isolated
`/tmp/possums-without-tor-target`, `--offline --locked`; Go used
`GOTOOLCHAIN=local GOPROXY=off GOSUMDB=off` and `-mod=readonly`.

- Default and all-target/all-feature Rust suites: **525 passed, four funded/live
  tests ignored in each**. Formatting and strict all-target/all-feature Clippy
  passed. Existing vendored Tinfoil unused-method warning remains.
- Go helper/shared verifier and standalone serving-verifier **race tests passed**;
  gofmt found no changed formatting.
- Flake-pinned Python **3.12.12**: **89 tests passed** (80 release/cache/rollout/
  HTTPS-backup guards plus nine image-health tests). The 13 cache tests include
  the actual offline Linux derivation-graph partition check. New guards cover
  opaque references, configuration drift, one invocation/no replay, timeout/
  failed candidate, protected provenance/approval and PR90 readiness removal.
- `checks.aarch64-darwin.gateway-accounts` passed. This is account-file recipe
  evidence, not Linux NSS execution or an image build.
- Fresh post-rebase scratch client typecheck/build and full offline runner
  passed: 38 reference, 691 client, 179 retained legacy-v2 bootstrap, 36 v3
  synthetic/loopback-TLS, 18 emitted-package host, 40 cross-process cache,
  164 native Pi, 31 SDK/RPC and 70 admission checks, plus thinking/forwarding/
  cumulative streaming fixtures. Bridge/WASM suite: **40 passed**. Packaged real
  WASM rejects invalid synthetic evidence; no positive hardware claim follows.
- Candidate bundle's statically extracted usage/error decoders passed synthetic
  catalog/cache/partial/interrupted-receipt and closed error-vocabulary tests.
  These are candidate checks, **not a newly tested older shipped artifact**.
- Cargo.lock adds **zero** package versions and removes **291**. Remaining Nix,
  Go and Pi lockfiles/pins are unchanged. Active-code search found no Tor runtime,
  connector, readiness or libc call; documentation and negative canaries remain.
  `git diff --check` passed.

Final scratch package: `/tmp/possums-pi-build-daiP5D/package`, extension SHA-256
`b9fd453ebf5cc7028397fd9ee145e7d34efc066383b3f8fd8404470cc8924174`.
Reused the prior locked scratch dependency install without another npm fetch;
rebuilt current source and checked Go source hashes against rebuilt WASM.
No installation or persistent client settings changed.

## Compatibility and activation limits

**Tor is unsupported, not default-disabled.** Previously shipped Tor-selected
clients/packages cannot use this candidate's removed route/transport. No silent
HTTPS downgrade is provided. Operators must explicitly approve this breaking
cutover and qualify/install a matching HTTPS client with user-controlled
activation before any deployment. No uncertain generation may be replayed.

The existing HTTPS wire contract is retained. Retained exact legacy-v2 bootstrap
fixtures and current synthetic authentication/catalog/submission/stream/receipt/
error/tool suites pass, but older shipped artifact decoders were **not newly
requalified end-to-end** against this backup. Pi 1.0.4 is the tested host; other
installed versions/T3 UI, elapsed compiled-manifest approvals and old-host
migration remain their own compatibility boundaries. Source success does not
update installed packages or authorize bypassing an expired approval.

No Linux executable/image run, fresh A/B build, hosted CI, measured release,
public/live probe, Tor bootstrap, paid inference, Honeycomb query/export or
production operation occurred. Linux cache graph evaluation and Darwin account
checks do not qualify Linux runtime behavior. Native CLI v3 platform-signer
mismatch, legacy-v2 freshness, actual provider invoice parity, upstream closure/
erasure, production privacy/resource headroom and earlier v48/v49 startup/update
failures remain unresolved at their historical scope. General startup stderr can
still block or be lost; best-effort delivery is not guaranteed ingestion.

## Privacy scope

Applied [data boundaries](../PRIVACY.md#data-handling-boundaries),
[forbidden data](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[typed failures](../PRIVACY.md#mandatory-typed-interface-failures),
[startup exception](../PRIVACY.md#startup-diagnostics-exception),
[processors/retention](../PRIVACY.md#processors-retention-access-and-shutdown),
[synthetic testing](../PRIVACY.md#synthetic-testing-exception) and
[required evidence](../PRIVACY.md#required-evidence-for-every-telemetry-change).
Offline suites retain hostile-data, actual local OTLP wire, off/outage,
first-failure/cleanup, no-replay and accounting tests. No content-bearing runtime
logs, support bundles, new destinations/dimensions or retention were introduced.
No native configuration, credentials or private Tor keys were accessed/deleted.
Historical records, including PR90's Linux/readiness gaps, are preserved with
archive notices. Native Pi/T3/platform history remains a separate persistence
boundary; no whole-host transience or anonymity claim follows.
