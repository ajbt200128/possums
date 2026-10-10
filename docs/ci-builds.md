# CI and release build acceleration

## Cache scope

`check.yml` runs on pull requests, main pushes and explicit dispatches. Branch
pushes no longer duplicate PR checks; superseded checks on the same PR/ref are
cancelled. Release/image publication is not cancelled this way.

Magic Nix Cache uses GitHub Actions cache storage, with FlakeHub explicitly
disabled. Both its action and daemon source revision are pinned. Installer and
cache diagnostic endpoints are explicitly disabled. No Cachix account, external
cache-write secret or new OIDC permission is added. Check jobs do not persist
checkout credentials. Cache availability is an optimization, not a release gate:
a miss must build from the pinned source. GitHub cache scope/eviction and upload
cost still apply. The `flake` job runs reproducible checks and seeds the
`api-smoke-fixture`, `gateway-image` and `gateway-smoke-image` outputs before the
`image` and `image-startup` jobs. Consumers can restore those outputs without
querying/building an entire cold dependency graph. Hosted execution still needs
fresh CI evidence. Large retained toolchain/vendor
closures in `release-build-deps` are prewarmed only for release builds, not for every
ordinary CI run. This
avoids concurrent downstream daemons querying an empty cache; it is not a guarantee
against GitHub throttling or eviction.

The flake already separates Cargo dependencies with Crane `buildDepsOnly`.
Ordinary checks may reuse these artifacts and unchanged application/check/image
outputs. `checks.tests` retains the existing release-profile Cargo suite;
`gateway` no longer compiles/runs that suite a second time during packaging.
Go helper checks remain intact. Image builds now start only after successful
`check.yml` on the exact admitted main source and originating-event validation.
Missing, failed or cancelled checks block both builders. The standalone serving
verifier has synthetic/offline tests using the existing pinned Go compiler; it
is not added to the gateway image.

## Independent release builds

Each fresh hosted matrix runner first builds `release-build-deps`. This uses
nixpkgs' supported `inputDerivation` to retain gateway/helper build inputs,
including Cargo artifacts, the custom Go compiler and Go modules. An
application-free dependency image supplies input roots for each image assembly
stage, including build-only tools that an already-substituted wrapper could
otherwise omit. The pinned nixpkgs 25.05 input wrapper emits null structured
output checks that Nix 2.34 rejects. We recreate that input-only derivation from
its original `drvAttrs` with `outputChecks` removed. Actual application/image
output constraints are unchanged; this is not a relaxation of release checks.

`scripts/check-release-cache.py` compares the image and prewarm derivation graphs
against the flake's explicit `rebuildDerivations` list. It rejects:

- An application/image derivation accidentally included in prewarming, or an
  unclassified assembly stage.
- Any already-realized application/image output in the runner's local store.
- A missing selected output of an external direct build dependency.
- Unsupported dynamic derivations or unreadable/malformed Nix graph data.

The current graph has nine fresh stages: gateway, helper, entrypoint, image base
JSON, customisation layer, layer metadata, configuration, stream wrapper and
archive. Revalidate this boundary when changing pins or image construction.
The following image build disables substitution and remote builders. Merely
setting `substitute=false` would not suffice without the preceding local-store
contamination check. Digest agreement, publishing the exact archive and registry
digest verification are unchanged. Inspection/publication tools use the flake's
pinned nixpkgs input.

The claim is **independent application/image builds with shared cached build
dependencies**, not independent toolchain/dependency reproduction. A cached final
image is permissible in ordinary checks but cannot satisfy the two release builds.

## Automatic release candidates (disabled until setup)

`RELEASE_AUTOMATION_ENABLED=true` enables the paid release pipeline after a
successful main-push check. Missing variables leave it off, including manual
entry points. Older source runs are rejected when main has advanced; not every
intermediate merge is guaranteed a release. Independent builds and publication
are serialized without canceling an active publication. Artifact names bind the
run, attempt, source and builder. A `workflow_run`'s workflow implementation SHA
can differ from the admitted source SHA; successful source admission plus the
immutable source-bearing run title establish the binding.

For source S and agreeing image digest D, the pipeline creates deterministic
release-only commit R with sole parent S. Only the gateway image line changes.
R is never merged into main: root `tinfoil-config.yml` becomes a release template,
not deployed inventory. The pipeline compares complete trees/config bytes and
exact S/R gateway-image derivation paths before allocating an immutable paid
semantic tag V. It explicitly dispatches exact-R checks and the tagged official
measurement/publisher workflow, then verifies the signed manifest, embedded
configuration, source, predicate and actual successful invocation/attempt.
Tag/release collisions and partial publishers require manual reconciliation;
existing tags/assets are never rewritten or blindly replayed. Free tags are
excluded from paid version allocation.

`PRODUCTION_DEPLOYMENT_ENABLED` is a separate, absent-by-default switch.
Publication can operate without production credentials or deployment. When
separately configured, an existing protected GitHub Environment gates production;
only latest main can pass the postapproval admission check. Older approval
requests may remain visible. See [approved deployment](approved-main-releases.md)
for setup, exact candidate approval and verification limits. Hosted orchestration,
policy API access and live serving verification remain activation qualifications,
not consequences of local tests passing.

## API-only and local builds

The browser-fixture Nix package and Playwright job are removed. The replacement
`api-smoke-fixture` builds `examples/api_smoke_fixture.rs`; the Linux
`gateway-smoke-image` uses that synthetic API server. The `image-startup` job
checks read-only startup with a runtime-injected synthetic account and exactly
one content-free listener line. It does not qualify production attestation or
live inference. API, transport, accounting and telemetry behavior is covered by
the Rust suite in `flake`; Go helper checks remain. Historical browser/Playwright
results remain in [verification](verification.md), not active release gates.

See [development](development.md#incremental-builds-and-worktrees) for the local
compiler/host-separated Cargo cache. It is not used by reproducible Nix builds.
Linux outputs cannot substitute native macOS builds, so no Cachix or platform
compiler/profile tuning is introduced.

## Privacy and qualification

Applicable policy: [data handling boundaries](../PRIVACY.md#data-handling-boundaries),
[permitted/forbidden telemetry](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[synthetic testing](../PRIVACY.md#synthetic-testing-exception) and
[required evidence](../PRIVACY.md#required-evidence-for-every-telemetry-change).
Only build inputs and synthetic fixtures belong in these caches. Never add runtime
credentials, live conversation state, environment dumps or support artifacts.
No production exporter change or inference request is part of this change.
Production automation remains disabled until separately authorized setup and
qualification. The dormant-by-default update path does not replay uncertain
mutations or export raw control-plane values/diagnostics.
Disabling action diagnostics is configuration intent, not on-wire qualification
or a claim of no platform telemetry. Scoped tests and remaining CI/Linux evidence
are recorded in [verification](verification.md).

Measure the first cold run, an identical-commit warm run and a source-only change,
including cache upload/teardown and the slowest job. Confirm dependency hits,
fresh release-stage logs, matching release digests and unchanged test coverage.
Do not use local Mac timings to claim a measured Linux CI speedup. The historical
October 9 sample showed a 12m27s flake-check step, a 10m25s image-build step, and a
separate 10m56s apt-heavy browser-install outlier. These are baseline observations,
not promised post-change timings.
