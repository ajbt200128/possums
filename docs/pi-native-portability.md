# Pi native connector — build preparation and qualification limits

**Source preparation, not a distributable client release.** This packet implements
explicit native targets and an executable-inspection gate. Only Darwin arm64 has
been built and executed here. The other three targets remain unqualified and
block the four-platform release in [the release plan](pi-client-release-plan.md).
No installation, activation, npm/org/trust mutation, publication, gateway update,
live Tor/public-gateway probe or inference was performed.

## Build contract

Run `scripts/build-tor-native.sh` inside the pinned `devenv` shell. It uses the
active Rust/Cargo **1.92.0** toolchain rather than recursively opening the flake
shell. Python 3.10+ and platform inspection tools must already be available.
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
existing JS packager still accepts **one host helper**; four-target assembly,
report-schema enforcement and the runtime resolver are separate pending work.

**Known packaging integration gap (independent review, medium):** `build.mjs`
currently accepts a matching-host GNU helper even when the new report says
`distributionTarget: false` / `unchecked_local_host`, and supplied helper bytes
can reach the private scratch package without the new inspector. This is not an
accepted public distribution path. The packaging phase must require the four
approved target records and independently inspect their bytes at assembly; a
self-reported boolean or hash match alone is insufficient. Keep local developer
builds available, but never publish them as the universal package.

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
local fixtures and public build inputs only. No telemetry, logs, crash reporting,
support export or private runtime configuration was added. Pi/T3's own history
is a separate persistence boundary; no whole-process transience claim is made.
