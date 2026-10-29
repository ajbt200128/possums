# Pi native connector — build preparation and qualification limits

**Source preparation, not a distributable client release.** This packet implements
explicit native targets and an executable-inspection gate. Only Darwin arm64 has
been built and executed here. The other three targets remain unqualified and
block the four-platform release in [the release plan](pi-client-release-plan.md).
No installation, activation, npm/org/trust mutation, publication, gateway update,
live Tor/public-gateway probe or inference was performed.

## Build contract

Run `scripts/build-tor-native.sh` inside the pinned `devenv` shell for an
already-provisioned host build, or the opt-in `pi-native` flake shell for native
qualification. It uses the active Rust/Cargo **1.92.0** toolchain rather than
recursively opening a shell. Python 3.10+ and platform inspection tools must be
available. The opt-in shell declaratively provisions its own runner's explicit
distribution target; it does not change the gateway/default development shell.
No arguments retains a local host build, including supported GNU Linux hosts;
that mode does not designate a release artifact. Explicit distribution targets:

| Target | Required output and inspection | Qualification here |
| --- | --- | --- |
| `aarch64-apple-darwin` | Thin arm64 Mach-O; platform libraries only; minimum version 14.0 | Built and executed on macOS 26.5.2 arm64 only; macOS 14 execution unqualified |
| `x86_64-apple-darwin` | Thin x86_64 Mach-O; same linkage/deployment checks | Target library unavailable; no build or execution |
| `aarch64-unknown-linux-musl` | Static ELF64 AArch64; no interpreter, shared dependencies or runtime search paths | Target library unavailable; no build or execution |
| `x86_64-unknown-linux-musl` | Static ELF64 x86-64; same linkage checks | Target library unavailable; no build or execution |

Example build on an already-provisioned matching toolchain:

`devenv shell -- sh scripts/build-tor-native.sh --target aarch64-apple-darwin`

The builder creates new scratch source/target/home directories, uses locked
Cargo dependencies offline, and reuses only registry cache bytes—not personal
Cargo configuration or credentials. It clears ambient compiler/linker settings,
resolves the installed compiler before clearing HOME, and disables rustup
implicit toolchain installation. It never installs a missing target.

Apple builds select the platform developer tools in a clean environment, set
`MACOSX_DEPLOYMENT_TARGET=14.0`, and report the observed SDK/compiler versions.
Musl builds select Rust's bundled linker and self-contained static runtime;
the selected target-library contents are bound by a digest. This is an explicit
build recipe, **not Linux portability evidence** until actual builds and clean
runtime cases pass. GNU local-host builds are explicitly reported as unchecked
for distribution linkage.

The inspection gate uses `readelf` or Apple `lipo`/`otool`; it does not execute
the artifact. It rejects wrong formats/architectures, symlinks/non-executables,
Linux dynamic loading and Mach-O non-platform libraries/search paths. Returned
inspection evidence always says `execution: not_tested`. Platform-only library
paths do not prove availability or successful execution on an older OS.
Native reports contain public source/toolchain/target/artifact facts, not raw
command output, environment values or absolute builder/home/cache paths. The
private JS packager still assembles **one host helper**, now under the same
`native/<platform>-<arch>/` layout and closed manifest/resolver intended for the
universal package. Four-target assembly and strict public-report/linkage
acceptance remain separate pending work.

**Packaging integration gap (independent review, medium):** the earlier private
`build.mjs` accepted matching-host GNU helpers and trusted supplied helper hashes
without independent portability inspection. The new private layout rejects a
GNU target label and checks actual executable headers, but those checks do not
prove static musl linkage. The public assembly phase must still require all four
approved target records and independently inspect their bytes/linkage; a
self-reported boolean, target label or hash match alone is insufficient.
Standalone GNU host builds remain available, not public client artifacts.

## Observed local evidence

- Fresh pinned JS/package baseline and complete `check.mjs` passed before changes.
  Go/WASM and the original helper were reused through existing source/hash checks;
  this was not fresh native compilation.
- After implementation, explicit-target and default-host clean builds produced
  identical Darwin arm64 helper bytes in separate scratch source/target trees,
  using the same installed toolchain and public dependency cache. SHA-256:
  `73c54053da3644b9d97ee24abce0051a2df0de2b9e2c2044db410aa6fa3d9dba`.
  This is local repeatability, not cross-runner reproducibility.
- Initial clean builds differed because compiler source locations included random
  scratch paths. Compiler path remapping removed those differences; final binaries
  had no scratch-build or user-home prefixes in the checked byte scans.
- Observed Apple compiler **21.0.0**, SDK **26.4.1**; Mach-O reports minimum OS
  **14.0**. Actual execution was **macOS 26.5.2 arm64**, not the minimum OS.
- **22** Python build/inspection tests passed, including hostile-output suppression,
  target/format/linkage/deployment rejection, missing tools, isolation and
  no-execution checks. **87** existing actual-helper synthetic loopback transport
  tests passed with the final helper bytes; no Tor service or external destination.
- A fresh JS/package build incorporating that helper passed the complete pinned
  Pi **1.3.0** / Node **24.13.0** `check.mjs`: v3 **69**, packaged Worker/WASM/Pi
  **18**, Tor channel **84**, native Pi **160**, peer recovery **18**, discovery
  **62**, admission **70**, and the remaining client/cache/stream/diagnostic suites.
  Reused source-matching Go/WASM bytes; no npm-managed installation claim.
- Python LSP error checks, shell syntax and `git diff --check` passed.

The flake dev-shell attempt stopped at a dependency fetch with observed HTTP 404
before client compilation. The pinned `devenv` shell worked; its default Node is
not the client build pin, so the already-available exact Node 24.13.0 executable
was selected explicitly. No dependency pins or lockfiles were relaxed.

## Native runner matrix — source checks in progress

`.github/workflows/check-pi-native.yml` initially supported manual runs only.
It now also runs on source pull requests under the operator's instruction to
continue qualification without per-commit pauses, with read-only repository permission, immutable action references, no persisted checkout
credential, no npm cache/auth setup, no binary upload and no publisher. Draft
[PR #115](https://github.com/ajbt200128/possums/pull/115) started the first source
checks. All four jobs in run `40920924978` stopped at preparation with
`possums_pi_native_ci_unknown`, before native compilation. A local probe of the
pinned shell confirmed `SSL_CERT_FILE` is absent and `NIX_SSL_CERT_FILE` is
exported. Commit `ea9d6327` accepts that exported public CA bundle, passes it to
Cargo, and classifies a missing bundle without disabling TLS; **42** Python
native tests passed, including both regressions. No successful four-target
result is established by this setup fix. All four `pi-native` shell definitions evaluated offline
with the locked Rust **1.92.0**, Python **3.12.12** and Linux binutils **2.44**;
evaluation does not provision targets or establish successful compilation.
The existing gateway checks/default shell and lockfiles remain unchanged.

The preparation driver `scripts/ci_pi_native.py` requires a clean exact-commit
checkout and Node **24.13.0** already on PATH. It acquires locked public Cargo
dependencies in an isolated scratch home, builds twice offline in fresh trees,
requires identical helper hashes, then runs the existing actual-helper transport
suite. Acquisition of public build dependencies is not runtime no-network proof.
Only public metadata and passed-stage evidence enter its qualification report;
raw compiler/test output and environments are withheld. macOS evidence records
only the observed numeric OS version, never an inferred 14.0 support claim.

Runner choices follow [GitHub's current runner table](https://docs.github.com/en/actions/reference/runners/github-hosted-runners):

| Native target | Prepared runner | Still unproved |
| --- | --- | --- |
| Darwin arm64 | `macos-14` (arm64) | Actual run and exact observed patch; a current 14.x runner is not proof of 14.0 |
| Darwin x64 | `macos-15-intel` | Actual run; macOS 14 x64 remains outside this prepared hosted matrix |
| Linux arm64 musl | `ubuntu-24.04-arm` | Native build and clean glibc/musl execution |
| Linux x64 musl | `ubuntu-24.04` | Native build and clean glibc/musl execution |

For Linux, `scripts/tor_native_qualification.py` independently checks ELF
linkage, source/report/artifact correspondence and matching native host/engine/
image architectures. Its clean userspace smoke test needs no compiler or
OpenSSL: the actual helper performs a synthetic SOCKS round trip. It mounts only
the helper and two public fixture source files, read-only, into unprivileged
network-none containers with no Nix tree, private home or credentials. Require
both official Node 24.13.0 Debian bookworm-slim and Alpine 3.23 image indices,
pinned by digest from [Debian tag metadata](https://hub.docker.com/v2/repositories/library/node/tags/24.13.0-bookworm-slim)
and [Alpine tag metadata](https://hub.docker.com/v2/repositories/library/node/tags/24.13.0-alpine3.23).
The qualifier itself never pulls; the explicitly invoked CI preparation driver
provisions those public images separately. Docker context/auth discovery and
emulation fallback are excluded. Both authoritative exit status and owned
cleanup must pass; a cleanup fault cannot erase an earlier primary failure.
Process/runner loss can still prevent cleanup or reporting.

Earlier local evidence for the initial preparation: **40** Python native policy/
control-flow tests passed (including **18** new mocked runner/CI tests),
**101** release tests passed with **3** skips, YAML parsed with the expected
manual-only four-runner structure, and the new compiler-free smoke fixture
passed with the previously compiled macOS arm64 helper. These are not Docker,
Linux, Intel macOS, new SDK/toolchain or remote-CI execution evidence. Independent
read-only review found no concrete defect in this scoped preparation; the
existing public-assembly integration gap above remains unresolved.

Future local procedure on a matching already-prepared macOS arm64 runner, with
Node 24.13.0 selected and a new scratch output directory (not executed here):

`nix develop --no-write-lock-file .#pi-native --command python3 scripts/ci_pi_native.py aarch64-apple-darwin /tmp/possums-pi-native-candidate`

This can acquire public build dependencies. It does not install/activate a Pi
client or authorize npm publication, redistribution or any live inference.
Actual native-CI success is still needed; a source PR run or manual native check
is not a release dispatch. Licensing continues to block public client binary
uploads.

## Lazy native selection and private source layout

`native-artifacts.mjs` is import-safe and is called only after valid explicit Tor
selection. It maps only Darwin/Linux arm64/x64 to fixed package-relative paths;
Linux labels are musl only. It bounds manifest/helper reads, rejects symlinked
assets, checks executable mode, size, SHA-256 and actual architecture/format,
and preserves a primary read failure across a later descriptor-close failure.
These are local integrity checks, not package authentication or linkage proof.
The existing transport warning owner carries closed `helper_start` diagnostics
for unsupported platform, missing/unreadable assets, integrity mismatch and an
unknown inspection cause. No raw OS exception, path or proxy secret is displayed.

The private source builder creates one host entry in `native-manifest.json` and
puts its helper/report under `native/<key>/`; it remains a private source package,
not the complete universal distribution. It now requests the explicit approved
native ABI. Linux source-client builds therefore require the separately
provisioned musl target (or an existing matching helper); the standalone native
builder's no-argument GNU host mode remains available for direct development.
There is no second weaker runtime resolver or automatic target installation.

Scoped local evidence on macOS arm64, Node **24.13.0**, Pi **1.3.0**:

- **33** synthetic resolver checks passed: all four header/path fixtures,
  corrupt/missing/oversized/redirected/unknown metadata, wrong architecture,
  symlinks/modes/hashes, content-free typed/unknown filesystem faults, forged
  diagnostics, no import I/O and primary-failure cleanup precedence.
- A fresh private scratch build and complete `check.mjs` passed, including **14**
  selection/diagnostic groups, **84** actual-helper/local-TLS channel checks,
  **160** pinned Pi host groups and all existing trust/stream/accounting/cache/
  discovery/recovery suites. A corrupt native manifest did not affect default
  HTTPS start/status/shutdown; missing selected assets warned without proxy
  secret reads, helper execution, direct fallback or traffic. Header-only
  selection fixtures never execute and are not hardware/trust evidence.
- Reused source/hash-matching Go/WASM and previously built Darwin arm64 helper
  bytes; this is not fresh native compilation, npm-managed loading, installation,
  activation, live Tor/inference, or absent-platform qualification.
- Client `tsc --noEmit`, proactive LSP probes and `git diff --check` reported no
  errors. Several push-only LSP checks could not affirmatively confirm clean;
  the exact compiler and complete scratch suites supplied the positive checks.
  Independent read-only review found no actionable defect in this scoped packet,
  while explicitly retaining the separate full-linkage/public-assembly gate.

## Remaining gates and privacy scope

All three absent target libraries require separately provisioned build runners.
Both Linux architectures still need execution in clean glibc and musl userspaces
without Nix/build closures. Intel macOS and the proposed minimum macOS systems
still need real execution; signing/quarantine behavior remains unqualified.
No target is silently dropped, and inspection alone cannot satisfy those gates.
Licensing, npm ownership/bootstrap and protected release approvers remain open.

Privacy references: [data handling](../PRIVACY.md#data-handling-boundaries),
[forbidden exports](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[typed failures](../PRIVACY.md#mandatory-typed-interface-failures) and
[synthetic testing](../PRIVACY.md#synthetic-testing-exception). Tests use synthetic
local fixtures and public build inputs only. No runtime telemetry, request logs,
crash reporting, support export or private runtime configuration was added.
GitHub, Nix caches, public package/image registries and CI may retain public
acquisition/build metadata;
external setup tools and host capture are separate boundaries, not a claim that
all platform output is controlled here. Pi/T3's own history is also a separate
persistence boundary; no whole-process transience claim is made.
