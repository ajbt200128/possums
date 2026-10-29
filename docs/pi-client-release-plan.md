# Pi client release plan

**Status: native-build preparation, reviewed lazy resolver/private source layout and scoped macOS arm64 offline checks; no installation, publication, activation or deployment.** Four-platform qualification, universal distribution packaging and publishing remain incomplete. Originally inspected at `f608e8b9a09c66f7f0405443fb5357ff05dde8b2`; factual corrections were checked against post-retirement baseline `0b3551f6` (PR #114). Subsequent native implementation, evidence and unresolved packaging integration are recorded in [native portability](pi-native-portability.md). Commands below remain future procedures except where that record explicitly establishes execution. Paths are repository-relative unless stated otherwise.

**Reconciled after retirement landed.** While this plan was drafted, the operator separately authorised and landed the retirement of the compiled approval path: `refactor(client): remove the compiled approval and --possums-manifest` (commits `e40382e7`, `04c45517`). That removes `API_APPROVALS`, `requireApiApproval`, `qualifyApi`, `Channel.api`, `ReferenceClient.verified`, `connect`, `recoverCompiled`, the `--possums-manifest` flag, and the frozen `pi-v2-bootstrap` fixture together with `tests/phase02_release.mjs`. The post-retirement baseline still exported `provenanceForTag` with zero callers. It was subsequently deleted independently in `8b7f6f4c`, including only its now-unused candidate limit and comments; all shared verification stayed intact. The fresh pinned scratch build, separate core typecheck and full offline client suites passed. Never reintroduce this retirement residue.

§6 below has been rewritten to match. Do not resurrect the tombstone; where the original draft said "keep it for v1", this plan now records why removal was the right call and what it changed for packaging.

## 1. Decision and intended user experience

Publish **`@possums/pi` on npm as the primary distribution**, with the complete prebuilt JavaScript/WASM client and **Tor support in the first public release**. Ship four native SOCKS connectors in one package: Darwin arm64/x64 and Linux arm64/x64. Use **npm OIDC trusted publishing, not a long-lived npm token**. These are operator decisions, not alternatives awaiting selection.

The operator must first establish an npm account and the **`possums` npm organization**; neither exists according to the operator. Package ownership and the repository↔package trusted-publisher relationship must then be established explicitly. The new-package bootstrap limitation in §7 is a prerequisite, not something a workflow can silently bypass.

After qualification and stable publication, the advertised install is:

`pi install npm:@possums/pi`

For reproducible installation of a reviewed release, show the exact version alongside it, for example:

`pi install npm:@possums/pi@0.3.0`

`0.3.0` is a proposed first stable public version, not an existing release. Users need an already-installed compatible Pi and Node runtime, an operator-issued recovery credential, and sufficient credit. They do **not** need this repository, Nix/devenv, Go, Rust, TypeScript, esbuild, or a pinned Pi *build directory*. Installation does not create an account, authenticate, submit a prompt, or prove the gateway is healthy.

End to end:

1. Install the package with the single command above.
2. Fully restart the Pi/provider process. Preserve the repository's restart guidance until actual npm-installed reload behavior is qualified; do not promise `/reload` activates every running Pi/T3 process.
3. `/login`, choose Possums, and enter the recovery credential only in the native credential interaction. Public verification and serving-key binding precede credential entry/transmission (`clients/pi/provider.ts`, `auth.apiKey.login`). Never put the credential in a command or message.
4. Select a model with `/model possums`. `/possums-status` shows local session-pinned release/expiry or the last safe diagnostic; it does not make a network call or prove current health (`clients/pi/index.ts`). Submitting a prompt is a separate deliberate, potentially paid action.
5. For Tor, first provide an existing SOCKS listener and select the onion explicitly. Packaging the connector does not install or start Tor:

`pi --possums-tor-onion <v3-onion> --possums-tor-proxy 127.0.0.1:19050`

Tor Browser/system Tor may use a different port; supply the actual numeric loopback listener. No direct-network fallback is permitted. Existing proxy credential handling and trust checks remain unchanged.

### Secondary channels

- **GitHub Releases:** mirror the exact npm `.tgz`, its public checksums/provenance, and scoped qualification notes under a client-specific tag. This is an archival/manual-download channel, not a second independently built distribution. Users can verify and extract it to a durable directory, then run `pi install /absolute/path/to/extracted/package`. Pi's documented package sources do not establish a GitHub-release-tarball URL installer; do not advertise one without testing it.
- **Git source:** retain the maintainer/auditor build path, not the primary user install. A git checkout currently requires the full toolchain and scratch build. `pi install git:github.com/ajbt200128/possums` is not an equivalent working client installation: this monorepo's client is nested and its source manifest is not the generated runtime package. Do not introduce install-time compilation or a separate generated distribution branch merely to offer a third one-liner.

Reference: Pi 1.3.0's installed `docs/packages.md` and `docs/cli.md`, under `/Users/mb5/.pi/agent/install/releases/1.3.0/node_modules/@earendil-works/pi-coding-agent/`. Versioned npm specifications are pinned; package updates use `pi update --extensions` or a selected source, whereas bare `pi update` updates Pi itself.

## 2. Verified baseline and corrections to the brief

The following observations describe `0b3551f6`, not subsequent native preparation. The new builder accepts explicit targets and the private JS packager uses the closed manifest/resolver with one host helper under `native/<key>/`. Target labels and executable headers are checked, but the public four-target/full-linkage acceptance gate remains pending. See [the scoped implementation record](pi-native-portability.md); no four-platform release acceptance is claimed.

| Observation | Source and implication |
| --- | --- |
| Source package is `possums-pi`, `0.2.0`, `private: true`, with wildcard Pi peers. | `clients/pi/package.json`. This is a private build workspace, not an established public npm release contract. Registry name/version availability was not checked. |
| Generated package already has `pi: { extensions: ['./extension.mjs'] }`. | `clients/pi/build.mjs` also writes `private: true`, `possums-pi`, `0.2.0`, wildcard peers. The Pi manifest half already exists; publication needs more than deleting `private`. |
| Exact build qualification requires all three Pi packages at 1.3.0, Node 24.13.0, TypeScript 5.9.3, esbuild 0.25.10. | `clients/pi/build.sh` checks package presence and Node; `build.mjs` checks package versions and build tools. The shell requires a nonempty `POSSUMS_PI_ROOT` but does not explicitly test absoluteness; `check.mjs` does. The README now distinguishes those checks. Continue requiring a canonical absolute path in the documented build procedure. |
| Go and Tor artifacts are built unless supplied, and reports/source hashes are checked. | `clients/pi/build.sh`, `scripts/build-v3-verifier.sh`, `scripts/build-tor-native.sh`, `clients/pi/build.mjs`. The native builder unsets `CARGO_BUILD_TARGET`; packaging requires its report's platform/arch to match the packaging host. Existing scripts cannot simply assemble four cross-target binaries. |
| WASM is portable; the SOCKS connector is not. | The Go build explicitly uses `GOOS=js GOARCH=wasm`; Rust currently builds for the host. `clients/pi/tor-native/src/main.rs` uses Unix descriptors, `libc::fcntl`, and Tokio `unix::AsyncFd`; Windows is not just a missing `.exe` build. |
| Only explicit Tor selection uses the native helper. | `clients/pi/index.ts` constructs `torScope` only for the Tor route; `tor-native-transport.mjs` spawns on connection. JS transport modules may be bundled/imported in HTTPS mode, but the native executable is not used there. |
| Wildcard peers are Pi's documented package convention, not proof of universal compatibility. | Pi 1.3.0 `docs/packages.md` requires host-provided packages as `peerDependencies: "*"`, not bundled/dependencies. Pi suppresses automatic peer installation for its managed installs. `clients/pi/index.ts` has no Pi-version-number startup rejection. |
| No npm publisher or downloadable **Pi client** release exists in the workflows inspected. | All five files in `.github/workflows/` were read. `check.yml` has image/test artifact uploads, `publish-image.yml` uploads gateway image/digest artifacts, and `tinfoil-release-publish.yml` delegates public gateway manifest publication to the pinned measurement action. Thus “nothing uploads build artifacts to GitHub releases” is too broad: gateway release assets exist in the release design (`scripts/release.py`, `verify_public`); the missing channel is client artifacts. |
| Default npm use is not blocked by any administrative expiry; that mechanism no longer exists. | `index.ts` always chooses `connectPublished` (cleartext) or `connectTorPublished`. `bootstrap.ts` uses fresh v3 evidence and its authenticated serving-release tag/digest against `PUBLISHER`. See §6. |

`.github/workflows/check.yml` plus the default flake checks cover gateway/Rust/Go/image work, not the complete Pi scratch build and `clients/pi/check.mjs`. A green existing check is therefore necessary but insufficient for publishing this client. The subsequently prepared read-only PR/manual `check-pi-native.yml` and opt-in `pi-native` shell cover the native packet only; the first PR run stopped on all targets at certificate-bundle preparation, uploads no client binaries, and does not establish full Pi CI or a releasable package. The setup fix is recorded without claiming native success. See [native runner scope and remaining gates](pi-native-portability.md#native-runner-matrix--source-checks-in-progress).

## 3. Published artifact contract

Build a clean distribution directory, then `npm pack` **that directory**, never the repository or whole scratch tree. Keep the source workspace private if useful; generate a public distribution manifest from one reviewed name/version definition, avoiding today's two independently hardcoded metadata copies.

### Required manifest metadata

- `name: "@possums/pi"`, an explicit client version, `type: "module"`; omit `private` in the public distribution (or set it false).
- Preserve `pi.extensions: ["./extension.mjs"]`; add `keywords: ["pi-package"]`. The keyword enables gallery eligibility, not automatic gallery listing or a security endorsement.
- Add a descriptive summary, supported-runtime instructions, operator-approved `license`, and `repository` pointing to `https://github.com/ajbt200128/possums` with directory `clients/pi`. Use the exact public repository spelling required by npm provenance.
- Add an explicit `files` allowlist covering only the entries below and `publishConfig` for public npm access/provenance. Check the actual packed inventory, since npm automatically includes some metadata/license/readme files.
- Keep host peers as discussed in §5. Do not copy development dependencies or Pi runtime links into the distribution.
- No `preinstall`, `install`, `postinstall`, or `prepare` downloader/compiler. Packing/publishing must not implicitly rebuild reviewed bytes through lifecycle hooks.
- Declare a Node runtime target separately from the build pin. Proposed initial `engines.node` is `>=24.13.0 <25`; qualify the minimum and the chosen current Node 24 patch before advertising that supported range. Only Node 24.13.0 is established by the existing client qualification record. If the matrix cannot establish the proposed range, narrow the initial documented support instead of pretending the build pin proves it.

### Proposed exact allowlist to implement

**The complete universal distribution remains future work.** `build.mjs` now emits a private manifest, one helper/report under `native/<key>/`, a closed `native-manifest.json`, and three Pi peer symlinks under `node_modules`. It does not assemble all four targets or generate approved project license/notices and a strict public-report projection. Assemble a separate clean distribution and inspect its actual tarball; do not publish the scratch package as-is.

| Ship | Reason |
| --- | --- |
| `package.json`, package `README.md`, approved project `LICENSE`, `THIRD-PARTY-NOTICES` | Pi discovery, install/use limitations, and redistribution obligations. The root README says the repository is not yet licensed; license selection/rights review is a release blocker, not permission to invent an SPDX identifier. |
| `extension.mjs` | Production bundle with non-host JS dependencies and `clients/pi/core` incorporated. Require the production fixture define to remain false. |
| `verifier.wasm`, `wasm_exec.cjs`, `verifier-worker.mjs` | Runtime offline Go verifier, its matching Go runtime shim, and Worker entry. Their relative placement is load-bearing (`verifier.mjs`, `verifier-worker.mjs`). |
| `verifier.mjs`, `verifier.d.mts` | Preserve the existing packaged offline bridge/declaration pair and package-check contract. The authorizing bundle includes the bridge; keeping the small standalone pair is not a promise of a new general-purpose public SDK. |
| `GO-LICENSE` | Required Go runtime license already copied by the current builder; it does not license Possums or every bundled dependency. |
| `native/darwin-arm64/possums-tor-native`, `native/darwin-x64/possums-tor-native`, `native/linux-x64/possums-tor-native`, `native/linux-arm64/possums-tor-native` | The four v1 connectors, with executable mode preserved. No top-level host-only helper in the public package. |
| `native-manifest.json` | Small generated closed map of supported platform/arch, Rust target, relative path, byte size and SHA-256. Used only for local helper selection/integrity checks; no URLs or runtime destinations. |
| `build-report.json`, `verifier-build-report.json`, and `native/<target>/tor-native-build-report.json` for the four targets | Reviewable public source/toolchain/artifact correspondence. Update the aggregate report from one host helper to the actual four-target package. Existing `tests/phase02_v3_package.mjs` consumes the build report; extend it for the final inventory. |

**Reports and source hashes may ship**, after strict public-field validation. Hashes of public repository/dependency source and shipped binaries are reproducibility evidence, not prohibited prompt/content hashes. Keep relative source paths, dependency/tool versions, public source commit, target triples, and artifact digests. Reject absolute builder/home/cache paths, environment values, private registry configuration, auth, raw command output/errors, and runtime evidence. Reports are not signatures and cannot independently authenticate a package that was itself replaced. A verifier build report may describe the native Go CLI built alongside WASM; explicitly distinguish such upstream build outputs from the final package's shipped inventory.

**Must not ship:** Pi itself; `node_modules` peer symlinks or physical Pi copies; build homes/caches/`.npmrc`; npm/Cargo/Go caches; Rust `target`; Go native `possums-attestation`; test bundles or fixtures; any test-authorizing bundle or fixture (the legacy `pi-v2-bootstrap` fixture and its scratch copy are deleted; do not reintroduce them); source maps or full source trees not needed at runtime; production config, credentials, installed auth/history, runtime caches, VCEKs, raw attestation reports, nonces, verdicts, or diagnostic/support dumps. Keep lockfiles and full public source at the immutable source commit, rather than confusing source build inputs with runtime installation dependencies.

The frozen legacy-v2 fixture and its hash pin are **deleted** along with the path they regressed (§6); never restore them. Other test-authorizing bundles still exist: `check.mjs` generates `fixture-node.mjs` and `pi-test.mjs` with `__PHASE01_FIXTURE__` set to true. Keep those outputs outside the distribution, and assert the production bundle uses the false define. Deletion of the legacy fixture is not evidence that all test artifacts are absent.

Licensing must cover bundled npm libraries, the Go/WASM dependency closure, and the statically distributed Rust closure, not just `GO-LICENSE`. Derive notices from exact lockfiles and actual bundle/native inputs. If attribution or source-offer obligations require additional files, add a reviewed explicit allowlist entry before publishing; do not silently omit legally required material to keep the package small.

### Open license blocker and operator shortlist

**No project license is selected. Redistribution of the client remains blocked.** Source preparation and synthetic local qualification do not resolve rights or authorize public distribution. The operator must choose a license, establish authority to license the covered code, and identify who approves the exact dependency closure and required notices/source offers. A root license cannot relicense third-party code.

| Candidate, not a selection | Trade-off for the operator |
| --- | --- |
| MIT | Short permissive terms and simple attribution; permits proprietary forks and has no express patent grant. |
| Apache License 2.0 | Permissive with an express contributor patent grant/termination provision; more notice/change-marking obligations and compatibility review than MIT. |
| Mozilla Public License 2.0 | File-level copyleft can keep modifications to covered files available while allowing larger proprietary combinations; requires a more involved source-availability and bundled-output review. |

Recommend MIT if simplicity is the priority, Apache License 2.0 if explicit patent terms matter, or MPL 2.0 if file-level reciprocity is intentional. These are decision aids, not a legal compatibility determination; do not add an SPDX metadata value, license file or redistribution claim until the operator decides and the rights review is complete.

## 4. Tor in v1: concrete build and runtime design

### One universal npm package, four included binaries

Use one `@possums/pi` tarball containing all four binaries rather than four optional dependency packages. This avoids optional-dependency omission, extra package ownership/OIDC bootstraps, and install-time executable downloads. Accept the package-size cost for the first release; measure packed/unpacked sizes and report them before acceptance. Do not defer Tor because this work is inconvenient.

| Package key | Rust target | Build/test strategy | Intended support floor |
| --- | --- | --- | --- |
| `darwin-arm64` | `aarch64-apple-darwin` | Native arm64 macOS runner; exact Rust 1.92.0, locked dependencies, selected SDK | Proposed macOS 14 qualification floor, arm64; unqualified |
| `darwin-x64` | `x86_64-apple-darwin` | Native Intel macOS runner, same pinned compiler/dependency policy | Proposed macOS 14 qualification floor, x64; unqualified |
| `linux-x64` | `x86_64-unknown-linux-musl` | Native x64 Linux runner with pinned musl linker/sysroot and explicit Rust target | Static musl binary; no installed Nix store or glibc version requirement |
| `linux-arm64` | `aarch64-unknown-linux-musl` | Native arm64 Linux runner with pinned musl linker/sysroot and explicit Rust target | Static musl binary; same constraints as x64 |

Propose macOS 14 as a conservative first qualification floor for both Node and the native helper, rather than advertising an untested macOS 13+ range. No macOS distribution floor is qualified yet. If the available systems cannot exercise that floor, report only the exact OS versions tested and leave older-version support unqualified. Linux static ABI and both userspace execution cases are also unqualified at this baseline.

Select exact available hosted runner images/architectures when implementing; do not assume `macos-latest` means Intel or that a cross-build is execution evidence. Prefer native architecture runners; Linux musl is a deliberate target ABI change. Native Linux execution tests must cover both glibc and musl userspaces on each architecture. Qualify macOS deployment targets and actual oldest supported systems separately; runner success alone does not prove older-OS compatibility. Windows, FreeBSD, 32-bit hosts and other unmatched tuples are not Tor-supported in v1.

The baseline Nix-built host executable was **not proven portable outside Nix**. Subsequent local macOS arm64 inspection/execution is recorded separately; Linux and Intel macOS remain unqualified. Merely copying it out of `$S/target/release` can retain a Nix ELF interpreter/library reference or macOS load paths. Distribution acceptance must inspect ELF interpreter/NEEDED entries and Mach-O architectures/load commands, reject `/nix/store` dependencies, and execute in a clean machine/container without the build closure. For Linux require genuinely static output; do not paper over a dynamic binary with a “musl” label. For macOS use platform SDK libraries only, with the chosen deployment floor; qualify download/quarantine behavior without disabling platform security. No Apple signing identity is assumed or invented.

Native source preparation now makes `scripts/build-tor-native.sh` accept a closed explicit target, uses the active pinned toolchain, emits target/ABI/compiler/source/artifact metadata, and locates target-specific Cargo output. The inspector rejects nonportable linkage without executing the binary. This does not complete cross-platform qualification. Cross-assembly must not require `nativeReport.platform/arch === process.platform/arch` as `build.mjs` still does. The packaging phase must independently validate all four expected target records, source hashes, executable formats, linkage and artifact digests; supplied reports or a `distributionTarget` boolean cannot bypass inspection. Keep local host builds supported with the same layout/resolver; do not create a separate weaker source-build loader.

Build the portable JS/WASM payload with exact pinned tools once per candidate (plus its independent reproducibility build), then assemble only matching-source native artifacts by immutable CI artifact ID and hash. Never “take the latest successful” matrix member. Missing one required target blocks the **whole v1 release**, not a silent partial package.

### Selection and validation

The source candidate now replaces the hardcoded top-level helper URL with `native-artifacts.mjs`, called only after valid explicit Tor selection. Private source builds use the same subdirectory/manifest/resolver contract. The implemented local-integrity boundary follows these rules (full public linkage/package acceptance remains separate):

1. Map `process.platform` and `process.arch` to one of the four literal keys; never use user strings as a path or executable override.
2. Resolve only the expected package-relative binary and reviewed manifest entry. Require a regular executable file, matching declared size/hash and target identity. Avoid a PATH search, arbitrary URL, install-time fetch, fallback executable or shell invocation.
3. Select the same authenticated package bytes verified by release acceptance. Self-contained hashes detect corruption/misassembly, not malicious replacement of both code and metadata; npm/GitHub provenance is the independent supply-chain evidence.
4. Preserve lazy behavior. A missing/unmatched Tor binary must not prevent HTTPS startup or make HTTPS inspect proxy secrets. A selected Tor route must fail before network/credential work if the local artifact cannot be used. It must never silently switch to HTTPS.

The existing `possums_tor_native_helper`, stage `helper_start`, constraint `artifact_unavailable`, is a useful safe fallback for spawn failures (`clients/pi/tor-native-transport.mjs`), but **not sufficient as the only multi-platform diagnostic**: it tells npm users to build a helper and does not distinguish unsupported platform from a damaged installation.

The source candidate adds only the needed closed distinctions, preserving the existing spawn fallback:

- Unsupported platform/architecture: proposed `possums_tor_native_platform_unsupported`, `helper_start` / `unsupported_platform`; explain Tor is unavailable on this platform, no direct fallback occurred, and name the supported platform families. Do not suggest a source build on Windows, where the Unix implementation is unsupported.
- Supported target but missing file: proposed `possums_tor_native_artifact_missing`, `helper_start` / `artifact_missing`; instruct the user to reinstall the matching approved package and fully restart, not compile Rust.
- Digest/declared-target/executable-format mismatch detected from local evidence: proposed `possums_tor_native_artifact_mismatch`, `helper_start` / `artifact_mismatch`; refuse execution and request an approved replacement.
- A validated helper still cannot start: retain `possums_tor_native_helper`, `helper_start` / `artifact_unavailable`; cause unknown, inspect local execution permissions/platform requirements. Do not infer an ABI cause from raw exception text.

The unsupported/missing/mismatch distinctions are implemented in the source candidate, with bounded unreadable-file and unknown-inspection fallbacks; they are not claims about a shipped public package. They reuse the current transport diagnostic owner and closed message path, not a diagnostic framework. Include no local paths, onion, proxy credentials, raw OS errors or process output. Keep known primary failure/cleanup precedence and no-replay semantics intact. Offline tests must cover each category, wrong architecture, missing mode, corrupt bytes, arbitrary manifest paths, and no native access in HTTPS mode.

## 5. Build pins versus host compatibility

**Recommendation: retain `"*"` ranges for Pi host peers**, per Pi 1.3.0's package contract, and document a separate exact qualification matrix. Do not replace these with runtime dependencies or bundle Pi to force the build pin onto every installation.

`build.mjs` must continue requiring **Pi 1.3.0** for all three build peers and the exact toolchain. The `pi-agent-core` import in `provider.ts` is type-only today; maintain its build pin. Declare runtime host peers according to actual emitted imports (`pi-ai`, `pi-coding-agent` currently); if the bundle acquires a runtime `pi-agent-core` import, declare that peer as well. The bundle report already records external imports: assert every external package is a documented host peer or an explicitly reviewed runtime dependency.

The wildcard is **host-resolution metadata, not a claim that every Pi version is compatible**. `index.ts` no longer reads a version and rejects non-1.3.0 hosts. Leave that unchanged; qualification belongs in release notes/tests. Do not add a numeric version gate that prevents otherwise compatible hosts solely because they are newer. Real missing host APIs must fail safely before private dispatch, with a bounded local diagnostic where the extension can report one; pre-loader failures are a Pi boundary and must be disclosed.

`docs/pi-1.3.0-qualification.md` establishes exact source/build offline checks on Pi 1.3.0, including Darwin arm64 native helper evidence. It explicitly does not establish Linux behavior, positive live startup/auth/catalog, T3 rendering, activation, or billing. `docs/pi-cvm-0.14.16-compatibility.md` separately records a local installation, not activation in already-running processes. Neither establishes npm installation or the four-platform binary matrix.

The new matrix must exercise the **actual packed package with Pi's managed package loader**, without `$POSSUMS_PI_ROOT` symlinks inside it. Current packaged checks directly import the scratch extension and selected SDK; they do not prove npm loader resolution. Test for one host registry/class identity, no duplicate physical Pi installations, actual Worker relative paths, and no build tools used at install/run time. This is the acceptance proof that separates exact *build* pins from frictionless *installation*.

## 6. Approval, expiry, upgrades and conversation continuity

### The compiled approval is removed, not tombstoned

The original draft recommended keeping the dead approval as an explicit tombstone and packaging around it. That was the safer call **before** retirement was authorised; it is now superseded. It was removed in `e40382e7`, and that is the better outcome for a distributed package:

- Every npm user would otherwise receive a record that can only fail closed. The only flag that reached it (`--possums-manifest`) produced `possums_approval_expired` with no repair path, since re-login and renewal cannot extend an administrative date.
- It pinned `v0.0.15` on the pre-custom-domain origin, so it was not merely stale — it described a release that no serving endpoint uses.
- Keeping it required keeping `connect`, `recoverCompiled`, `Channel.api`, `ReferenceClient.verified` and a frozen test fixture, and the fixture's bytes were hash-pinned so they could not even be repointed at the moved client core.

`PUBLISHER` is now the single trust input, and the qualification path has no `approval` branch at all. Two consequences for this plan:

1. The package README must not document `--possums-manifest` or `possums_approval_expired`; both are gone. `clients/pi/README.md` and `docs/tor-guide.md` were updated in `04c45517`.
2. Legacy-v2 acquisition regression coverage retired with the path it covered (`tests/phase02_release.mjs`). Current v3 acquisition, channel and recovery coverage is `tests/phase02_v3.mjs`, which still passes. Do not treat this as a coverage gap to fill by resurrecting the fixture.

Any user who had `--possums-manifest` in a launch alias will now get an unknown-flag result rather than a clean error. That is a deliberate, documented removal, not a silent behaviour change; note it in release notes.

### Default v3 trust is independent of that deadline

`connectPublished` first verifies fresh nonce-bound evidence, obtains the serving release's tag/digest, and verifies its exact manifest plus endpoint/channel binding. A fixed publisher/origin and supported CVM policy still apply. It is not trust in npm's `latest` tag or GitHub's “latest release.” An npm package is therefore not required merely because the historical administrative date passed.

The real v3 trust expiry is:

**minimum of acquisition time + 12 hours, serving-certificate expiry, and verified witness freshness expiry** (`clients/pi/v3.ts`, `qualify`). The 12-hour ceiling is not a promise of 12 usable hours. Authenticated API sessions have their separate expiry (`clients/pi/core/client.ts`). Cached public manifest bytes never extend either lifetime or substitute for verification.

### Renew in the existing conversation

Current `clients/pi/provider.ts` already implements `joinRenewal` / `renewForSubmission`. When an eligible model invocation encounters known trust/auth expiry, the existing scoped path re-verifies public trust if needed, authenticates a fresh API session, acquires the catalog, then publishes the replacement under account/run/abort guards. Auth-only expiry can reuse still-valid public trust. Internal renewal preserves conversation progression and does not call `/new` or erase the transcript.

User guidance: allow the active run to settle, then submit the **next deliberate turn in the same conversation**. Normal expiry renewal happens before that turn's private dispatch; a package update or conversation reset is not the routine renewal procedure. `/possums-status` remains offline and must not become a renewal trigger. Failed verification sends neither credential nor prompt and must not be bypassed. An interrupted previous generation is never automatically replayed; a deliberate new submission can incur another charge.

Do not promise “only a human can ever trigger renewal”: current code uses run/signal scopes, native compaction may generate new paid work, and `docs/pi-1.3.0-qualification.md` retains blocked same-signal/native-origin permission cases. Preserve those limits and test actual Pi lifecycle behavior rather than reviving stricter historical descriptions from older renewal records.

If a latched integrity failure requires a fresh trust session, explain that separately from routine expiry. For a package upgrade/restart, Pi's existing persisted-session resume can preserve history (`pi --continue` or `pi --resume`); qualify this with synthetic history and no automatic inference. A `--no-session` conversation is memory-only and cannot be promised to survive process exit. Do not introduce new history persistence just to make upgrades easier.

### Keeping installations usable

- No client/package auto-updater, remote approval downloader, background renewal daemon, or security-policy override.
- Users choose package upgrades. For a pinned installation, use an explicit new version with `pi install npm:@possums/pi@<approved-version>`; `pi update --extensions` does not move a version pin. An unversioned source can be updated deliberately with `pi update npm:@possums/pi`. Verify these paths in the qualification harness.
- New CVM versions/publisher changes/revocations can still require a new client. Current supported CVM versions are exactly `v0.14.12` and `v0.14.16` (`core/approval.ts`), not a permissive future range. Local denylist changes require distribution; this plan adds no remote revocation guarantee.
- Preserve `AGENTS.md` shipped-client compatibility policy: before a gateway-visible change, run the previous shipped package or exact pinned decoder alongside current code against offline auth/catalog/reservation/stream/receipt/error fixtures. A signed gateway release can still be incompatible with an old client's strict policy. If breaking, obtain explicit staged-cutover approval and qualify/install the replacement under separate authority before gateway rollout, or retain a safe old-client path. Never weaken trust/accounting for compatibility.

## 7. Publication authority and supply-chain gates

### Distinguish client publication from gateway deployment

Client publication **does not require the gateway's `production` deployment ceremony**, Tinfoil credentials, configuration custody, or a gateway update. Those gates protect mutation of a live gateway. Reusing them would couple client availability to an unrelated deployment and expose unnecessary authority.

**Proposed minimum: protected immutable `pi-v*` tags, successful exact-source checks, npm provenance and a required human publication review.** Use a dedicated `pi-release` GitHub environment for that approval; first verify that it exists with required-review protection, so referencing it cannot silently create an unprotected environment. The operator must designate the reviewer and recovery owner; none is invented or configured by this plan. Revalidate current-main eligibility, source/tag, checks and artifact digests after approval. Keep the publisher disabled until these prerequisites are verified and one publication is separately authorized.

**Does client publishing need the gateway ceremony? No—not the gateway deployment ceremony, but yes to a separate protected publication approval.** Publishing client code is an irreversible supply-chain mutation users may install, so prior PR review alone is insufficient authority for a particular upload. Reuse required human approval, exact-byte/source provenance and no-replay discipline, not the gateway's production environment, Tinfoil/admin credentials, configuration custody, secret-reference snapshots or live deployment checks. Protected tags plus npm provenance do not replace the human release decision. Additional Git-tag signing is not a proposed mandatory gate; use it only if the operator later selects an existing signing identity/policy.

Use a new dedicated workflow, proposed `.github/workflows/publish-pi.yml`, not `scripts/release.py prepare` or `publish-image.yml`. Preserve the principles in `docs/approved-main-releases.md` and `scripts/release.py`: reviewed source; exact successful CI; immutable artifact/source identity; public signed provenance; serialized mutation; no overwriting, blind retry, or automatic rollback. Gateway-specific release-only commit R, config transformation and A/B image digest logic are not reused as npm version allocation.

For each client-only PR, the user/agent may explicitly apply `no-rollout-needed` **after reviewing the complete diff and rollout impact**. Never auto-label by file path. The label skips ordinary gateway publication/deployment, not required CI, client publication review, or npm provenance. If gateway impact is uncertain, retain normal rollout. A docs plan is not merge/release authority.

### Account, organization and first-package bootstrap

1. Create the operator's npm account with 2FA and private recovery custody, then create/secure the `possums` organization and confirm ownership of the scope. If unavailable, stop for operator resolution; do not rename the chosen scope autonomously. Assign least-privilege maintainers and an account-recovery owner. No credential goes into chat, the repo, or build artifacts.
2. Resolve the project license and redistribution rights before making a real client artifact public.
3. Outside CI, the operator must establish control of an existing `@possums/pi` package record before attaching its trusted publisher. Org creation and a named recovery owner come first; an org alone does not establish package-level trust. No existing package record or trust relationship was verified for this plan. **The non-placeholder method of establishing that record is unresolved and blocks publication:** the operator must obtain a registry-supported route with npm. Do not assume OIDC can create its own unconfigured trust relationship.
4. **No placeholder publish is authorized or to be implemented.** For factual context only, npm documents that staging a new package publishes a public `0.0.0-stage` placeholder; staging itself does not require 2FA, while approval of the staged version does. That is not a selected bootstrap route. Do not add a placeholder/staging workflow, manual real-client publish, staged-client approval or token fallback. If npm offers no acceptable non-placeholder route, stop for a new operator decision; CI cannot resolve this prerequisite.
5. Once the org, recovery custody and package record exist, the operator—not CI—must attach the trusted-publisher link to GitHub owner `ajbt200128`, repository `possums`, workflow filename `publish-pi.yml`, and the verified protected environment `pi-release`. Select permission for the intended direct `npm publish` flow; do not assume it is enabled by default. Configure dist-tag permission separately if using OIDC for stable promotion. Organization creation, account recovery and trust-link setup are operator-run steps outside this pipeline, not agent-authorized mutations.
6. Use GitHub-hosted runners with `id-token: write` only in the publishing job; no `NPM_TOKEN` or `NODE_AUTH_TOKEN` long-lived write secret. Restrict token-based publishing after hosted OIDC qualification, keep maintainer 2FA, and document account recovery. npm currently documents expiry of an unused new trusted-publisher configuration after two days, so create it when the approved candidate is ready.

A future operator-authenticated setup command, **only after the package exists and the environment/workflow are approved**, is:

`npm trust github @possums/pi --repo ajbt200128/possums --file publish-pi.yml --env pi-release --allow-publish`

Pin and qualify the npm CLI separately from Node: current npm docs require at least npm 11.5.1 for trusted publishing, 11.15.0 for `npm trust`/staging; OIDC dist-tag support has a later version requirement. Choose one exact compatible CLI and verify its real capabilities. Do not update global npm on the operator's machine as part of a build. Some npm documentation pages differ on newer trust-management features; confirm the chosen registry/CLI interface at setup rather than treating the documentation as account-state evidence.

External references read for this plan: [trusted publishing](https://docs.npmjs.com/trusted-publishers/), [npm trust prerequisites](https://docs.npmjs.com/cli/v11/commands/npm-trust/), [staged publishing](https://docs.npmjs.com/staged-publishing/), [provenance](https://docs.npmjs.com/generating-provenance-statements/). No npm account setup, name reservation or registry mutation was performed.

### Candidate and publish gate

1. Reviewed PRs land on main only with ordinary repository checks plus the new client matrix. Bind the candidate to an exact main source commit, client version and protected immutable client tag. Require current-main eligibility at publication admission; if superseded while queued, stop and requalify deliberately.
2. Use a separate tag namespace, e.g. `pi-v0.3.0-rc.1`, so it cannot enter the gateway `vX.Y.Z` allocator/parser. Do not edit `tinfoil-config.yml` or invent a gateway release for a client package. Client GitHub Releases must be marked **not latest** so they do not disturb human/legacy gateway discovery.
3. Build with locked dependencies, pinned actions/tools and synthetic isolated homes. Run complete client checks, actual package-loader tests, all four native execution suites, negative trust/accounting/privacy tests, and reproducibility comparisons. Prefer two independent clean builds of each shipped payload with matching hashes; normalize only non-runtime archive metadata, not discrepancies in executable bytes. Record any unproven reproducibility separately and withhold that claim/acceptance until resolved.
4. Pack once; inspect the exact tarball inventory and metadata; test its extracted bytes on every target without build-only symlinks, credentials or toolchains. Bind source, versions, runner targets, immutable CI artifact IDs, file digests and tarball integrity in the release record. Do not rebuild after approval.
5. Publishing job starts only after protected approval and revalidation. No PR code receives npm OIDC authority. Grant only required permissions; no Tinfoil/admin credentials. Pin all actions by commit as existing workflows do. Keep mutation concurrency non-canceling and reject automatic mutation reruns.
6. Publish the exact approved tarball with npm provenance and a prerelease dist-tag initially. An illustrative future command, with `PACKAGE_TARBALL` supplied by the validated artifact handoff, is:

`npm publish "$PACKAGE_TARBALL" --access public --provenance --tag next --ignore-scripts`

OIDC authentication and provenance are distinct: trusted publishing authenticates the workflow; provenance binds public package bytes/source/build identity. npm automatically generates provenance for eligible public GitHub OIDC publishes; keeping `--provenance` explicit makes intent clear. Neither proves absence of malicious code, a verified live backend, or successful npm loader behavior.

7. Independently retrieve the exact registry version, verify registry integrity and provenance repository/workflow/source identity against the candidate, and compare the tarball to the approved bytes. `npm audit signatures` is a useful supported check in an isolated registry-installed acceptance directory, but must be supplemented by checking expected identity/digest, not merely counting signatures.
8. Attach the same `.tgz`, public checksums and scoped evidence to the client GitHub Release, with verifiable GitHub artifact attestation or equivalent keyless signed provenance binding those exact bytes. Do not produce a separately rebuilt archive. Record npm success separately from mirror success; a failed mirror is not permission to republish npm.
9. If a publish result is uncertain, stop and inspect the exact version/integrity/provenance read-only. An existing version is not proof it is ours. Never delete/repoint an immutable source tag, replace an existing version, or retry a possibly completed publish blindly. Reconcile before any new authorized version. A rollback is a deliberate dist-tag change/user-selected earlier compatible client, not an automatic downgrade or gateway rollback.

### Signed tags and provenance

The existing `scripts/release.py` creates gateway tags with `git tag tag release`, not a cryptographically signed Git tag; it verifies signed manifest provenance separately. Do not describe existing tags as signed.

For the client, require immutable protected `pi-v*` tags plus **OIDC-signed package and mirror provenance**. A Git-signed annotated tag is additional defense only if the operator identifies an existing controlled signing identity and verification policy. It cannot replace the byte/source attestation, and this plan neither creates a signing key nor assumes one exists. If signed Git tags are made a mandatory release gate, naming that identity is an explicit prerequisite. Never confuse a tag signature, npm provenance, gateway release signature, and runtime attestation.

## 8. Independent client versioning

Keep client versions independent of gateway `v0.0.*`. Current private `possums-pi@0.2.0` is the source/build label in two files; it does not mean “compatible with gateway 0.2.0,” a published package, or a stable public API promise.

Propose first public candidate **`@possums/pi@0.3.0-rc.1`**, tagged `next`, then **`0.3.0`** after acceptance. “Tor in v1” means the first public distribution wave, not a forced SemVer `1.0.0` maturity claim. If the operator wants `1.0.0`, decide its stability/support promise explicitly before tagging.

Use patch versions for compatible fixes, minor versions for additive pre-1.0 capabilities, and explicitly documented migration releases for breaking behavior. Never overwrite an npm version. Stable promotion from an RC requires a distinct version/package metadata and therefore a freshly gated exact tarball, not a claim that RC and stable tarballs are identical. `latest` and `next` are mutable discovery pointers, not trust pins.

Every release note records client source/tag/artifact digest, exact qualified Pi/Node/OS versions, native ABI/deployment floor, supported CVM policy, tested previous-client/gateway fixture coverage, trust-path behavior, and remaining live/host limits. Preserve prior immutable public packages for compatibility testing. Do not schedule a client release for every gateway patch when the existing client contract still works.

## 9. Ordered rollout: small independently reviewable steps

Every step requires its own bounded review and evidence. Reverting source preparation does not unpublish an artifact, undo an installation, or establish a gateway rollback.

| Step | Change | Proof required before advancing | Failure / reversal |
| --- | --- | --- | --- |
| 0. Correct documentation | Remove obsolete baseline repairs, distinguish proposed inventory from current output, and record open decisions without selecting a license or bootstrap workaround. | Documentation diff against `0b3551f6`; no source/runtime changes or new build claims. | Revert documentation independently; no publication effect. |
| 1. Make native target builds portable | Explicit four-target Rust builds, static Linux outputs, macOS deployment floor and truthful target reports. | Fresh pinned build/check baseline, ELF/Mach-O inspection and each binary executed on its actual architecture without Nix/build closure. Locked sources match. | Any missing/unportable target blocks v1; do not quietly omit Tor or label cross-compilation as execution coverage. |
| 2. Delete retirement residue separately | Delete the zero-caller `provenanceForTag` helper and only imports/constants made unused by that deletion; do not remove shared verification code. | Focused source diff, typechecks and existing trust suites; no policy weakening. | Independently revertible cleanup, not a packaging or trust redesign. |
| 3. Introduce distributable layout | Single name/version source; generated public metadata/allowlist; portable WASM assets and public reports; no lifecycle scripts or host symlinks. Leave license metadata unresolved until approved. | Local `npm pack` inventory, report/artifact checks, extracted runtime loading with Pi host mapping only within separately authorized installation scope. | Remains a local candidate; no public redistribution before license/rights approval. Revert metadata/layout independently of trust behavior. |
| 4. Add closed lazy resolver/diagnostics | Package-relative target selection; hash/target checks; unsupported/missing/mismatch distinctions; unchanged byte transport and trust policy. | HTTPS never accesses/spawns helpers; explicit Tor never falls back; wrong-target/corrupt/missing/error/cleanup fixtures pass. | Revert selector separately; no arbitrary helper path/download workaround. |
| 5. Add exact client CI and compatibility gates | Dedicated client matrix and actual packed Pi loader tests; extend package reports/tests; keep ordinary repository CI. | Full suites on four targets, fresh WASM Worker tests, synthetic renewal/history/no-replay/receipt/tool/privacy checks; prior shipped decoders exercised where affected. | Failed or unavailable coverage remains explicit; no client publish on gateway-only green CI. |
| 6. Add disabled protected publisher/mirror | Dedicated workflow, exact artifact-ID handoff, protected environment, tag/version guard, OIDC/provenance, no-replay reconciliation. | Offline negative tests for stale/unreviewed/wrong-source artifacts, absent environment/trust, collisions, uncertain outcomes; independent workflow/security review. | Leave disabled. No credentials or mutation granted to build/PR jobs. |
| 7. Resolve operator prerequisites; release only with new authority | Operator establishes org/recovery custody, license/rights approval, a non-placeholder package-record route and protected reviewers, then attaches the trusted-publisher link outside CI. Separately authorize one protected RC publication and mirror. | Prerequisites reconciled without secrets; registry version/bytes/provenance match; all four installed package cases qualified only with separate installation authority; no automatic live inference. | Blocked until resolved. No placeholder or token fallback. Stop on unknown publication outcome; a mirror failure does not authorize npm replay. |
| 8. Separately qualify user experience | Authorize test-profile installation, activation and bounded public verification; separately authorize auth/catalog or paid canary if wanted. | Distinct installed/activated/public-verified/authenticated/catalog/inference results, including Tor prerequisites and actual host presentation. | Stop at the failing stage with safe diagnostics. No paid probe assumed; offline fixtures are not live Tor/gateway evidence. |
| 9. Release stable and update onboarding | Gate exact stable package, set stable discovery intentionally, replace source-first README with npm-first instructions and maintenance policy. | Verified stable registry/mirror bytes and documented supported runtime matrix; readable failure/renewal guidance; no false guarantee of universal host compatibility. | Deprecate/redirect a bad version with operator approval; retain immutable evidence and let users choose a compatible replacement. No silent update/rollback. |

### Concrete acceptance commands to retain

For fresh baseline qualification before native-target changes, use the repository's pinned developer environment with the exact Node executable selected (the environment's default Node must be checked, not assumed). The PR #114 path repairs are already present; do not restore the deleted fixture or add an `examples/phase01` symlink:

`node --version`

`POSSUMS_PI_ROOT="$HOME/.pi/agent/install/releases/1.3.0" sh clients/pi/build.sh`

Use the exact scratch directory printed by that invocation, not a prior convenient build:

`POSSUMS_PI_ROOT="$HOME/.pi/agent/install/releases/1.3.0" node /absolute/printed/scratch/source/clients/pi/check.mjs`

For the proposed clean assembled distribution, with `DIST` and `ARTIFACTS` set to approved absolute scratch paths:

`npm pack "$DIST" --dry-run --json --ignore-scripts`

`npm pack "$DIST" --pack-destination "$ARTIFACTS" --ignore-scripts`

`tar -tzf "$PACKAGE_TARBALL"`

Also run existing repository gates, not only packaging tests:

`python3 -m unittest discover -s tests -p 'test_release*.py'`

`nix flake check --no-write-lock-file --print-build-logs`

Future registry checks after authorized publication:

`npm view @possums/pi@0.3.0-rc.1 version dist.integrity dist.attestations --json`

`npm audit signatures`

The last command belongs in the isolated installed acceptance directory. Public registry metadata is not proof of local installation or activation. Native-target and package-loader harness commands must be added with their implementation rather than presenting nonexistent scripts here as runnable today.

## 10. Privacy and evidence requirements

Apply [data handling boundaries](../PRIVACY.md#data-handling-boundaries), [forbidden telemetry](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data), [typed failures](../PRIVACY.md#typed-interface-failures-and-token-aggregate-extension), and [synthetic testing](../PRIVACY.md#synthetic-testing-exception) to packaging, CI, installation tests, error handling and public reports.

- Builds use public source and isolated synthetic test data only. No production auth, user profile, history, runtime caches or private config is mounted into build/test jobs. Inspect both the tarball and actual diagnostic outputs using synthetic hostile sentinels.
- Add no telemetry, crash reporter, log bridge, automatic support bundle, raw exception export, host inventory, installation identifier or runtime binary-download reporter. Publication evidence is public source/artifact metadata, not a request diagnostic archive.
- Reuse `tests/phase02_pi.mjs`, `phase02_discovery.mjs`, `phase02_v3_package.mjs`, `phase02_v3.mjs`, the cache/stream/receipt suites and Tor suites driven by `check.mjs`. `phase02_release.mjs` no longer exists. Add packaging/privacy failures at the real npm/Pi/native boundaries. Preserve no-credential-before-verification, hostile-error suppression, known charge retention, unknown-billing disclosure, incomplete-tool suppression, cancellation races and no inference replay.
- Offline tests must intercept public acquisition or block external traffic, disable Pi installation telemetry in their isolated settings as existing package tests do, and use no real credential. Do not assume a `--offline` flag alone constrains arbitrary extension network calls. Test installs and source dependency acquisition are not equivalent to runtime no-network execution.
- Pi stores credentials and normal sessions under its own policy; npm/GitHub/Sigstore retain installation/build/public provenance metadata under theirs. Do not claim whole-process transience or no third-party metadata. Provenance is public and potentially immutable; it must contain only approved public build associations.
- Package attestations authenticate distribution, not the gateway. Runtime v3 verification remains mandatory on every trust establishment. Public cached manifest bytes are acquisition aids, never shipped cached authorization.
- Follow `docs/pi-client-failure-policy.md`: warning-only non-trust stops do not establish successful generation/settlement; fatal identity failures remain closed. Existing host interception and T3 rendering limits remain explicit.
- Future bounded routine implementation/test packets can use the live-catalog Possums Flash model when authorized and suitable under `AGENTS.md`. Keep privacy, trust, concurrency and release-authority decisions with the stronger lead. Dogfood inference is real service usage, not synthetic release evidence or implicit authorization for paid calls. No extra agent/dogfood calls were required for this design.

Keep a per-candidate evidence record distinguishing **source read → built → offline qualified → packed → published → installed → activated → public verified → authenticated/catalog qualified → inference observed**. Do not infer later stages from earlier ones. Fresh scoped native/build checks are claimed only as recorded in [native portability](pi-native-portability.md); the remaining release and live phases are unqualified.

## 11. Non-goals and remaining operator decisions

### Explicit non-goals

Do not bundle or install Pi; run compilers/downloader scripts on user installation; auto-update packages or trust policy; disable verification/expiry/revocation/channel binding; weaken accounting or tool-completion gates; silently fall back from Tor; start a Tor daemon; patch Arti; introduce Windows support by renaming a Unix binary; add an uncontrolled signing key or long-lived npm token; deploy the gateway; mutate secret references; install/activate the operator's client; or send paid inference as part of publishing.

Do not silently repair old client incompatibility by broadening CVM acceptance. Do not reintroduce a compiled or `--possums-manifest` pin; it was deliberately removed and must not return as an npm convenience. Do not replace existing release/provenance gates with “npm accepted the upload.”

### Still requiring a real decision or prerequisite

1. **License and rights — open redistribution blocker:** select from the shortlist in §3 (or provide another license) and name the rights/notices approver. No license or SPDX value is selected.
2. **Account custody and bootstrap — operator-only blocker:** establish the `possums` org and recovery owner, obtain a non-placeholder route to the package record, then attach the protected workflow's trusted-publisher link outside CI. No placeholder publish will be implemented; no token fallback. A registry-supported route remains unresolved.
3. **Publication approval — proposed minimum:** protected immutable client tags, exact successful CI, npm provenance and required human approval in a dedicated `pi-release` environment, not the gateway deployment ceremony (§7). The operator must name reviewers/recovery owners and approve this policy; no additional Git-signing identity is assumed.
4. **Runtime support — proposed qualification targets, not qualified ranges:** retain exact Pi 1.3.0 and Node 24.13.0 build qualification. Propose `>=24.13.0 <25` only after minimum/current-patch testing; otherwise advertise only the exact tested runtime. Target macOS 14 initially and report only exact tested OS versions until the floor is exercised. Linux static musl and glibc/musl execution on both architectures remain unqualified. Confirm/provide runners; all four Tor targets remain required.
5. **Version/support cadence:** confirm the proposed `0.3.0-rc.1` → `0.3.0` sequence, maintainer upgrade/security-notice ownership, and which prior shipped artifacts remain in the compatibility matrix. No auto-update policy is proposed.
6. **Live acceptance authority:** separately authorize any clean-profile installation/activation, public Tor/HTTPS verification, authentication/catalog exercise or paid test. Until then, report offline/package evidence only.

**Already decided and not reopened:** npm scope `@possums/pi`; create the npm org first; Tor in the first public release with at least the four specified targets; OIDC trusted publishing with no long-lived token.
