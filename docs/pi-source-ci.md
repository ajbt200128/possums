# Private Pi source CI

**Source preparation and one-host offline qualification, not publication.** The
read-only `check-pi-source.yml` job is separate from the four-target native matrix
and from universal packaging/installed-profile acceptance. No npm organization,
package ownership, token, OIDC permission or gateway credential is needed for it.

## Inputs and isolation

The GitHub-hosted `ubuntu-24.04` PR/manual job uses immutable checkout/setup-node/
Nix-installer action references, read-only contents permission, no persisted
checkout credentials, no shared package cache and no artifact upload. Node
**24.13.0 / npm 11.6.2** are selected separately from `.#pi-client`, which supplies
Go **1.27.2**, Rust/Cargo **1.92.0**, Python **3.12.12** and inspection tools.
Nothing automatically installs missing native targets or relaxes a pin.

`clients/pi/ci-sdk/package.json` and its committed lock pin the three Pi SDK
packages to **1.3.0**, with **120** locked dependency entries. These are private
compiler/test dependencies installed only in isolated scratch, not a Possums
client installation or Pi profile. All lifecycle scripts are disabled. The lock
must retain public npm registry URLs, canonical SHA-512 integrity, exact direct/
root pins, no local/linked/workspace dependencies and no project scripts.

The driver requires an exact clean requested commit, including untracked files,
both before work and after all checks. This is not a filesystem lock or protection
against a concurrent writer; the CI checkout must remain under the job's custody.
It creates a replacement child environment with fresh HOME/TMP/public caches and
empty npm configs. Personal npm/Git configuration, credentials, proxies,
`NODE_OPTIONS`, compiler flags, authentication helpers and runtime configuration
are not forwarded. TLS verification stays enabled using the public shell CA.

## Acquisition, then offline work

1. Acquire the locked public SDK/compiler dependencies with npm lifecycle scripts,
   audit and funding disabled; acquire Go modules through the public proxy/sumdb
   and Cargo dependencies with the lock and explicit host distribution target.
2. Compile the verifier and native helper with prepared caches and offline flags.
   Build the source client using the SDK scratch root and explicit public npm
   cache with `POSSUMS_PI_NPM_OFFLINE=1`.
3. Validate the actual raw source/report/module/recipe/artifact correspondence,
   then execute the complete scratch `check.mjs`, including actual SDK/host and
   native-helper synthetic tests and exact **synthetic** packing tests.
4. Recheck source identity, clean owned public-dependency scratch, and only then
   emit the bounded public qualification summary. No receipt survives failed
   checks, late source drift or cleanup failure.

Offline flags are **not an OS-level network/filesystem sandbox**, and dependency
pins do not prove dependencies cannot be compromised. The tests' inference and
appraisal boundaries are synthetic. Compiler acquisition still contacts npm,
Go/Cargo mirrors and Nix caches; those services and the CI host have their own
retention boundaries. Builder-created source/helper/WASM scratch can remain for
local review or the runner's lifecycle; temporary SDK links are not a usable
installed client after dependency scratch cleanup. No binary is uploaded.

The source builder accepts optional `POSSUMS_PI_NPM_CACHE`: an existing absolute,
nonsymlink, caller-owned directory containing only public scratch cache. Ownership
cannot establish contents; never pass personal cache/configuration. Offline is
exactly `0` (default) or `1`; invalid input fails before acquisition. The default
maintainer build still uses its own isolated cache. A Node wrapper's selected
absolute path is retained for checks: Nix `execPath` may resolve to nodejs-slim
without the wrapper's paired npm.

## Diagnostics and evidence

Failures expose a closed `possums_pi_source_ci_<stage>` code, locally authored
constraint/action and known subprocess stage, not raw stdout/stderr, SDK errors,
paths, causes or stacks. Known start/read/exit/decode/timeout/cleanup boundaries
are distinct. Existing builder/test CLIs are opaque subprocess boundaries: an
inner assertion/category may be unavailable; the wrapper does not infer it from
exception text. These are preparation diagnostics, not billing/settlement claims.
The driver captures tool output transiently in memory and creates no diagnostic
logs, traces or support bundle. Normal CI/Nix/host history is a separate boundary;
this is not a whole-process transience or no-logs guarantee.

Owned new process groups receive bounded termination/drain on timeout, interruption
and post-start read failure. Remaining pipe/scratch cleanup is attempted without
replacing a known primary error. This proves the exercised cleanup paths, not
termination of every possible descendant or an OS sandbox.

At **`e9458030870bb9f58a880853d3e2eabd37dbedee`**, the actual driver completed on
macOS **26.5.2 arm64**, with the pinned shell/tools and freshly acquired SDK,
compiler, Go and Cargo inputs. Verifier/native/client compilation, report
validation and the complete offline client suites passed. SDK install scripts
stayed disabled; their absence did not prevent this exercised suite. The public
SDK lock digest was
`9a23d4afab3011e9c6323a2aeda3ef488deafe107433c3617c50ad2ee036a417`.
This is **one-host** evidence, not Linux/full native-matrix or universal archive
acceptance, older-macOS execution, installed loading or live service evidence.

**47** Python unit/stub tests passed with pinned Python **3.12.12** and Node
**24.13.0**. They include actual `build.sh` with stub npm, real bounded
child/grandchild faults, hostile lock shapes, environment isolation, cleanup
precedence, source drift and mocked stage ordering. Mocked success is not a
compiler or CI execution claim. Independent source review found read-fault
orphaning and cleanup-interrupt precedence defects; both fixes were independently
accepted. A subsequent actual-run Nix wrapper lookup failure was reproduced at
the synthetic pack suite and fixed by preserving the selected absolute Node
wrapper, with regression coverage and the fresh successful driver run above.

An earlier `nix develop --offline` attempt failed in public Nix dependency setup
(source HTTP 404), before the driver. The documented acquisition-enabled shell
succeeded without toolchain or lock changes. A first actual driver attempt then
correctly withheld qualification on its check failure; no failed/modified scratch
was promoted. Read-only GitHub run **40975281859** then reported the Ubuntu source job
**SUCCESS** for PR head `e9458030`; native run **40975281886** reported all four
jobs **SUCCESS**. These are observed PR-head workflow statuses (PR checkout can
be a synthetic merge), not a delivered signed artifact receipt or universal
candidate acceptance. Exact-source artifact handoff remains separate; see
[private distribution](pi-private-distribution.md).

Apply [data boundaries](../PRIVACY.md#data-handling-boundaries),
[forbidden exports](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[typed failures](../PRIVACY.md#mandatory-typed-interface-failures) and
[synthetic testing](../PRIVACY.md#synthetic-testing-exception). No runtime telemetry,
accounting, trust, retry or gateway release policy changed.

## Maintainer command

Select the exact Node/npm pair on PATH, then run from a clean reviewed checkout:

`nix develop --no-write-lock-file .#pi-client --command python3 scripts/ci_pi_source.py`

This deliberately performs public dependency acquisition followed by offline
source checks; it neither installs/activates Possums nor creates an npm record.
