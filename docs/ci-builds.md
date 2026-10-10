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
Go helper checks remain intact. Image publication now additionally waits for a
successful `check.yml` run on the exact commit and branch, in parallel with its
two image builds. Missing, failed or cancelled checks block publication.

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
No production exporter, inference request or deployment is part of this change.
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
