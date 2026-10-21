# Intake gateway PR preparation and gated rollout

## Subsequent operator rollout request

After the local preparation below, the operator explicitly requested deployment.
This authorizes publishing the branch/PR and proceeding through normal reviewed
merge, exact-source Linux CI, release provenance and protected production approval.
It does not waive those gates, authorize client installation/activation or paid
inference, or establish correct live reference delivery. The initial prepare-only
restrictions below describe that earlier checkpoint, not the new rollout request.
The operator subsequently selected preparation of an explicit protected first
cutover because references are not provisioned. The image and exact reference
replacement must occur together through the [one-time cutover path](intake-protected-cutover.md),
not an old-image configuration update. The ordinary no-drift guard is unchanged.

Main advanced to `b2cc2a90b23271969b5989b39b70898547682f57` before publication.
Integration commit `f375d9e` preserves its public-evidence cache/availability warning
and native charge-display fixes plus local Tor fixtures. The only textual conflict
combined both required client test suites; the local Tor test auth constructor also
explicitly initializes its new optional intake field to `None`.

The combined source passed all-target/all-feature Rust tests (499 library passed,
four ignored, plus integrations), Clippy and application formatting, and 119 Python
tests plus eight planner tests using existing Python 3.13.13. An incidental run via
Apple Python 3.9 failed two pre-existing interpreter-sensitive fixtures; no source
or release gate was weakened for it. The earlier Pi artifact below predates this
main integration. A fresh actual package at
`/private/tmp/possums-pi-build-NFeOWAUU/package` passed unmodified `check.mjs`: all
19 invoked suites plus 70 admission checks, including 72 manifest-cache checks,
162 Pi groups, three intake diagnostic groups and 18 emitted-package host checks.
Parent rechecked all 75 recorded source inputs after resolving/committing the merge
and matched the extension/report hashes. The snapshot's Git metadata predates the
merge commit; the per-file hashes bind its actual inputs, not tagged provenance.
The current extension SHA-256 is
`38536f556cbe27503df8b6072d62de9e51338e491bbca5157f1d50b84fc9e1c0`;
build-report SHA-256 is
`1c495c153a4c89901482ffefcc6ee15a076a4758fa51d03cc507cc5e77339b01`.
Sandbox/helper coverage remains scratch-only, loopback-only and darwin/arm64.
Fresh independent integration review of `c979809` found no concrete new source
blocker, retaining the Linux/reference/provenance/compatibility gates. Its client
README checklist inconsistency is corrected: gateway rollout does not authorize
client installation/activation or claim full old-client compatibility.
Required Linux CI and live deployment/provenance/reference verification remain
outstanding.

## Protected cutover review and operator confirmations

Published head `9a2a898` passed all Linux flake/image/image-startup jobs in run
`39672832320`. This includes actual archives/rootfs/packaged-health/Tor/synthetic
startup; the additional protected-cutover source needs fresh exact-head CI.
Cutover review R1 identified a stale full-list replacement race and name
re-resolution exposure. The revised path requires explicit per-run exclusive
configuration custody, refreshes the captured-ID snapshot before final admission,
and targets that ID for execution and all later reads. A refresh/assertion is not
a lock/CAS; actual operator single-writer custody is required for the remaining
race window. Fresh read-only R2 found no blocking source finding under that
explicit single-writer condition. Its nonblocking test-count documentation note
is resolved by labeling the earlier 141/22 checkpoint and recording parent-observed
149 total tests (three optional skips), 30 cutover tests and eight planner tests.
The reviewer did not run those checks or approve deployment.

The operator confirms control of all gateway console/API/project writers and will
keep them idle during the protected cutover until outcome reconciliation. The
operator also reports both private secret objects created, with running selections
unchanged. No values were accessed, and this is not independent delivery or
credential-correctness verification. All 30 revised offline cutover tests pass;
the full Python suite ran 149 tests with no failures and three optional skips.
No production mutation, client activation or paid inference has occurred.

## Draft PR

**Title:** Use native intake authentication and renewable durable accounting

### Summary

- Use verified HTTPS native intake authentication/current binding and the original
  non-sliding authority deadline; the gateway never retains the raw customer key
  for silent reauthentication. Intake remains the sole durable financial authority.
- Root reserve, conditional renewal and terminal ownership before dispatch. Accepted
  work continues through renewal uncertainty/expiry; no accounting-driven cutoff,
  automatic retry/reconciliation, replacement hold, post-expiry debit or replay.
  Initial admission and independent trust/preflight/disconnect gates remain intact.
- Preserve actual terminal receipts, cumulative usage, completion/tool gates and
  closed opt-in diagnostics. Remove gateway/client balance/reconciliation authority.
- Prepare measured deployment references and synthetic startup fixtures for intake.
  Preserve Tor enablement, attested A grant/mount and approved B references, open
  egress, 600-second startup grace, memory-only 512 MiB `/tmp`, release provenance,
  protected approval and no-drift constraints.

### Compatibility and privacy

Default error/receipt envelopes and the shipped 43200 login scalar remain. New
intake diagnostics require the existing exact-singleton opt-in header and are
nonauthorizing; remote prose and private identifiers never enter that projection.
Hash-pinned old error/usage decoders pass their synthetic fixtures. **The retired
balance route remains a deliberate 404**, and older clients' balance commands are
not supported. This is not full installed-client compatibility. The operator
reports manual tests only, no existing customers; test state is still preserved.

See [data boundaries](../PRIVACY.md#data-handling-boundaries),
[typed failures](../PRIVACY.md#mandatory-typed-interface-failures),
[synthetic isolation](../PRIVACY.md#synthetic-testing-exception) and the complete
[implementation/qualification record](intake-integration-review.md). No request
logs/traces, new telemetry signals, automatic support export or diagnostic-triggered
work were added. Local tools/Pi retain their native history boundaries; this is not
whole-process transience or an anonymity claim.

### Verification

- Pinned full offline Rust all-target/all-feature suite: 499 library tests passed,
  three ignored, plus all integration groups; Clippy `-D warnings`, application
  formatting and whitespace passed. The existing vendor `encode` warning remains.
- Exact deployed intake source `9a8b6bac3b2407419a8637bb90b0db4f3528b3e2` archived
  and content-verified in disposable local fixtures: four Go race groups and both
  actual Go/PocketBase/Rust HTTPS auth/accounting/renewal cases pass. No live probes.
- Six source client suites/typechecks and exact pinned shipped decoder checks pass.
  Renewal review R1's diagnostic-loss finding was fixed; fresh R2 found no blockers
  in the scoped ownership/receipt/paired-fixture review.
- Rollout/health source guards: 119 Python tests passed with three optional skips,
  plus eight held-plan tests. All three opt-in pinned Linux release-cache graph
  checks were also executed successfully with an offline Nix wrapper; they verify
  derivation partition/check semantics, not realized image outputs.
  A compiled native smoke fixture configured only with
  synthetic intake origin/key returned 204 under a loopback-only sandbox and
  emitted no configuration values. Health mode still bypasses account/provider
  setup; hostile current intake configuration does not escape startup diagnostics.
- Actual current production Pi package built and passed its full normal check
  harness as described below. **Linux/amd64 image qualification is outstanding**;
  successful flake evaluation and Python runner tests are not image runtime proof.

### Release gates / do not merge as rollout-ready yet

Required current-source Linux CI must still build the actual Linux/amd64 gateway
and fixture archives and run packaged health, real NSS/SDK rootfs and Tor-image
checks. Keep every existing required job and exact artifact/provenance gate.
No `no-rollout-needed` label is appropriate for this gateway behavior change.

Also retain the documented actual-expiry-during-auth and composed provider-decoder/
tool-gating coverage limits, and positive hardware/native-Linux/live paired unknowns.
The local Pi artifact does not prove client installation/activation or production
serving. User-controlled live integration/inference checks need separate approval.

## Required reference transition — no runtime values in source

The source configuration and deployment guards now require:

- `POSSUMS_INTAKE_ORIGIN`
- `POSSUMS_INTAKE_LEDGER_KEY`
- Existing `TINFOIL_API_KEY` and `OTEL_EXPORTER_OTLP_HEADERS`
- Existing approved Tor identity references remain unchanged in measured source.

The HTTPS origin is not a credential, but is delivered through a runtime reference
alongside the separately protected accounting key so this patch does not invent a
live endpoint or put private configuration in source. Neither a source reference
nor local synthetic construction proves that the reference exists, is correct or
has been delivered to production. HTTPS certificate verification remains required;
intake enclave-attestation integration remains deferred.

`POSSUMS_ACCOUNTS_JSON` no longer grants production authority. The deployment guard
intentionally rejects an instance missing either required intake reference and also
rejects a leftover legacy account reference even when both new references exist.
It does **not** create secrets, change
settings or allow a reference/configuration refresh in the normal update plan.
Provisioning the exact new origin/key secret objects is private operator setup,
not a running selection change. The subsequently approved
[one-time path](intake-protected-cutover.md) combines the signed image with the two
intake additions and legacy-selection removal; unrelated selections/settings and
all normal provenance/protected-approval gates remain. Future ordinary updates
remain unchanged-configuration only. No mutation or secret entry happened in this
preparation packet.

The measured image digest is intentionally unchanged by PR preparation. The normal
release algorithm alone binds a newly built candidate image to tested source;
never substitute the old image's passing health result for this source candidate.

## Actual local Pi artifact

Scratch package: `/private/tmp/possums-pi-build-lyvm3vkd/package` (not installed).
Current `build.mjs` ran with the same isolated source layout as `build.sh`, but
reused existing pinned dependencies rather than installing/downloading them. The
actual production build has `__PHASE01_FIXTURE__=false`. Reused verifier/native
helpers were accepted only after matching all report source inputs and artifact
hashes. Parent rechecked the extension/report hashes and all 60 source inputs.

- Node 24.13.0, TypeScript 5.9.3, esbuild 0.25.10, Pi peers 1.0.4.
- Go verifier 1.27.2 with the pinned SDK; Tor helper Rust 1.92.0 / Arti 0.47.0,
  **darwin/arm64 only**.
- Full current `check.mjs`: 18 invoked suites plus 70 admission checks passed;
  1501 numerically reported checks/groups, plus unnumbered checks. Includes actual
  emitted-package/Pi SDK host (18 checks), Pi suite (160 groups), native-helper/
  loopback TLS (84 checks), and three intake diagnostic groups.
- Extension SHA-256:
  `f4632488aac75b965538dde1c1a2950ed08b87e9dd6de016892d3dc4899e00aa`.
- Build-report SHA-256:
  `ad8030a560680441ffe420c52543db9337333ec92657daf5091c93da274bc2c6`.
- Sorted 12-file package manifest SHA-256:
  `d77d441c689cb5b04222bca3d298df6e945c48f39c59ad7a40c933df22073edc`.

Sandbox writes stayed in scratch plus `/dev/null`, with outbound sockets restricted
to loopback. An initial synthetic OpenSSL `/dev/null` denial was fixed in the local
sandbox policy, not source. No installed package, provider initialization against a
live service, positive hardware test, inference, dependency installation or network
download was performed. Scratch paths may disappear and are not release provenance.

## Local Linux image limit

A unique source snapshot including all untracked implementation files was made at
`target/release-prep/gateway-vhugvzel/source`, with a per-file hash manifest beside
it. An offline Linux/amd64 Nix image build with remote builders explicitly disabled
returned nonzero; no image is qualified. Both the Linux image and native gateway
flake derivation evaluations passed, so this is not an observed evaluation failure.
The build's underlying cause was not inferred from raw error text.

This host is aarch64-Darwin; its existing local Unix-socket Docker engine is Linux
arm64 and no configured x86_64-Linux builder was advertised. No remote builder,
image pull, new VM or dependency installation was enabled to bridge the gap. Keep
the actual Linux image/rootfs/health checks as required CI, not waived tests. Local
Docker retains its native image/metadata boundary; no production configuration was
loaded or container deployed for this unsuccessful image attempt.

## Preparation review

Fresh independent preparation review R1 identified two blockers: the deploy guard
accepted new intake references coexisting with the retired account reference, and
the earlier review record still described source rollout preparation as pending.
The guard now refuses coexistence without modifying the input or runtime state;
a focused negative test covers it. The review record distinguishes prepared source
from the still-unperformed live reference transition and outstanding Linux image
qualification. The 119 Python tests and eight held-plan tests passed after both
fixes. Fresh read-only R2 confirmed both blockers resolved and found no new concrete blocker
for local preparation; it did not run tests/builds or clear Linux/runtime gates.

## Local commit series

1. `e606532` — validated native auth and renewable ledger transport/receipts, with
   bounded negative unit tests and no serving switch.
2. `d1d3742` — atomic serving authority/accounting/ownership/API switch. These
   mutually dependent types and failure-mode tests must land together.
3. `7e827de` — paired Pi/reference clients use receipts, not balance reconciliation.
4. `1af88bf` — required intake rollout references and synthetic CI/health fixtures.
5. `2e548e2` — exact deployed-source disposable Go/Rust HTTPS and shipped contracts.
6. Evidence/PR-preparation documentation follows as its own commit.

The first adapter-only index snapshot passed its 38 focused tests, all-target/
all-feature Clippy and formatting. The second serving-only index snapshot passed
499 library tests plus all integrations and Clippy before commit. This checks that
splitting off later paired fixtures did not leave a broken intermediate Rust
commit; actual HTTPS cases were added in the dedicated qualification commit.
Complete final-source checks and artifact source hashes remain the release evidence
above, not a claim that each intermediate commit is deployable.

This initial preparation packet did not authorize pushing, opening a remote PR,
merging, publishing, deployment, client installation, runtime-reference changes or
live integration/inference. The subsequent rollout request above expands only the
stated gateway rollout scope; historical qualification is not deployment evidence.
