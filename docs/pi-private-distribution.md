# Private Pi candidate assembly and packing

**Source preparation only.** The authored candidate is `@possums/pi@0.3.0-rc.1`,
`private:true`, with `license:null` and `redistributionApproved:false`. No public
client package, universal real candidate, installation, activation, publication or
live verification is established by this work. Licensing/notices, current-source
four-target artifact handoff and operator-owned release prerequisites remain gates.
See the [release plan](pi-client-release-plan.md), [source CI](pi-source-ci.md) and
[native evidence](pi-native-portability.md).

## Implemented boundary

- `clients/pi/release.json` is the name/version/pin/rights source.
  `release-metadata.mjs` generates the private runtime manifest and literal sorted
  **21-file** inventory. It rejects fabricated license/redistribution approval.
  The private source workspace's `possums-pi@0.2.0` label is not a public version.
- `build.mjs` records the production `index.ts` recipe, `fixture:false`, exact
  recipe/source/lock/module hashes and existing verifier/native receipts. The
  opt-in `pi-client` shell supplies compiler/inspection tools; select **Node
  24.13.0 / npm 11.6.2** separately. Its default environment is not evidence of
  another Node/npm version's compatibility.
- `distribution-reports.mjs` validates exact schemas, pins, artifacts and source
  correspondence against the repository and scratch dependency workspace.
  Node builtins and declared Pi peers are the only external imports. Public
  projections retain module names/hashes, not the detailed import graph or
  absolute workspace paths. The native Go CLI can appear in the upstream
  verifier receipt but is not a shipped file.
- `assemble.mjs` requires a fresh output and all four target directories beneath
  `NATIVE_ROOT`: `darwin-arm64`, `darwin-x64`, `linux-arm64`, `linux-x64`.
  Each contains `possums-tor-native` and `tor-native-build-report.json`.
  Linux must be musl; GNU and missing targets fail closed. The host artifact
  must match the source client's receipt. Reports/hashes/header labels never
  substitute for the independent Python ELF/Mach-O inspection of **each original
  and each copy**. No helper is executed by assembly.
- The final Git snapshot follows source reads, writing, post-copy inspection and
  inventory validation. A dirty candidate records `sourceCommit:null` and
  `sourceState:"uncommitted"`; it never claims exact-commit provenance. A late
  failure yields no accepted receipt, even if an unaccepted partial directory
  remains. These checks are not a filesystem lock or protection against a writer
  changing files after acceptance.
- The output contains only the reviewed inventory, with regular data files
  **0644** and four helpers **0755**. It excludes Pi, peer links, fixtures,
  caches, install scripts and the Go CLI. There is no project `LICENSE`: the
  package README and `THIRD-PARTY-NOTICES` explicitly say rights review and
  complete dependency notices/source-offer decisions are unfinished.
- `pack.mjs` verifies bytes/modes/inventory/manifest, stages verified copies, and
  runs pinned offline `npm pack --ignore-scripts` without personal npm configs,
  authentication or workspace/lifecycle behavior. Pinned build-only
  `tar@7.5.22` lists the actual archive without extraction. Exact entries,
  content hashes, sizes/modes, expanded canonical TAR geometry and nonidentifying
  ownership are required; links, duplicates, PAX/unlisted/trailing expanded bytes
  are rejected. Every expanded byte must also match canonical headers from the
  pinned library's supported `Header` encoder, exact payloads, zero file padding
  and two zero EOF blocks; ignored header tails cannot carry extra data. The
  single gzip member has zero timestamp, closed encoder/platform flags, exact
  raw-deflate consumption and validated CRC/ISIZE. Bounded regular-file reads
  use nonblocking no-follow descriptors.
- Archive validation, compressed size, SHA-256 and SHA-512 integrity use **one
  checked descriptor/stream**, not a later reopened pathname. Descriptor/path
  identity and stability are checked before acceptance; primary failures survive
  cleanup faults. CLI receipts contain only a basename, digests, sizes and
  relative inventory. Errors use closed locally authored `possums_pi_pack_*`,
  `possums_pi_distribution_*`, metadata or typed inspector categories, never raw
  exception/stderr classification, paths or stacks.

The aggregate build report cannot hash itself. The returned independent inventory
and tarball receipt cover `build-report.json` too. Correspondence and local
integrity are **not authentication**, linkage labels are not execution proof, and
assembly explicitly reports `qualification:"not_established_by_assembly"`.

## Build-only commands

These procedures require already prepared, matching inputs and selected pinned
tools. They are not permission to create missing targets, publish or install.
`SOURCE_PACKAGE` is the one-host scratch package printed by `build.sh`;
`NATIVE_ROOT`, `DIST` and `ARTIFACTS` are absolute scratch paths. Both output
paths must be new. Do not feed a previous unaccepted partial output onward.

`node clients/pi/assemble.mjs "$SOURCE_PACKAGE" "$NATIVE_ROOT" "$DIST"`

`node clients/pi/pack.mjs "$DIST" "$ARTIFACTS"`

The packer alone checks inventory/bytes, not native execution, linkage, source
provenance, rights or publication eligibility. A raw `npm pack` listing is not a
replacement for either gate. No complete real four-target assembly/packing was
performed locally: three current matching native inputs are absent.

## Scoped verification

A fresh one-host source package was built on macOS **26.5.2 arm64** with exact
Node **24.13.0**, npm **11.6.2** and Pi **1.3.0**, reusing matching Go/WASM
inputs and a freshly rebuilt Darwin arm64 helper. A final fresh source build and
complete scratch `check.mjs` passed after the reviewed corrections, including:

- **100 synthetic distribution tests**: all four CPU-header fixtures, eight mocked
  inspection boundaries, report/source/recipe/import/artifact mismatches,
  GNU/missing targets, projections/privacy, typed errors, output ownership,
  late source changes and unaccepted partial output.
- **108 offline packing checks**, including real isolated npm packing of synthetic
  inventory, gzip/ignored-header/file-padding/owner/trailing-byte negatives,
  timeout-bounded real FIFO swaps,
  single-descriptor hashes, replacement/in-place mutation, cleanup precedence and
  bounded real CLI misuse diagnostics.
- **35 synthetic native resolver tests**, including real FIFO/regular-file swaps,
  descriptor checks, no import I/O and unchanged lazy selection failures.
- **84 actual-helper/local-TLS checks**, **14 selection/diagnostic groups**,
  **160 pinned Pi host groups**, and the existing trust, stream, receipt,
  cache, discovery and renewal/recovery suites. This remains synthetic appraisal,
  not positive hardware/app-key-chain, paid inference or public Tor evidence.

The fresh raw client report passed the actual validator against **166 modules**,
its production recipe and current source/workspace bytes. **67 Python native
policy/control-flow tests** passed in the pinned shell. Real Darwin arm64 static
inspection returned platform-only Mach-O with deployment target **14.0** and
`execution:"not_tested"`; deployment metadata does not prove macOS 14 execution.
Active LSP probes found no reported errors: five paths were confirmed clean and
five were inconclusive (push-only servers), not affirmative clean evidence. The
actual compiler and full scratch suites supplied the positive execution checks.

Independent review first found a regular-file-to-FIFO race. The runtime/report
readers now use nonblocking no-follow opens and descriptor identity checks; the
archive uses one validated/hash stream and owned cleanup. The expanded regression
checks above passed. Subsequent independent review identified unbound payload
rereads, Python blocking/reopened inspection, extra gzip members/padding, and
ignored TAR header/file-padding bytes. Copies are now rebound to validated hashes;
Python inspects the same inherited descriptor with expected-SHA binding and bounded
tools, and the parent has a bounded typed deadline. Canonical gzip/TAR checks close
the ignored-byte cases. Review rounds 3/4 accepted these fixes; round 4 was scoped
to the final archive correction, not a new broad audit. No unqualified security,
authentication or platform acceptance is inferred.

PR #115's native workflow run **40925858145** succeeded for all four targets at
**`9ac39772724b95e741ec53885f0ef4e469f1c9b8`**. The earlier CA preparation fix
also had a successful run, **40922458945**. This records queried GitHub workflow
status, not a current-source artifact receipt/handoff or universal package
acceptance. The newer inspector source invalidates those older report hashes.

Subsequently, source CI run **40975281859** and all four native jobs in run
**40975281886** reported **SUCCESS** for PR head **`e9458030`**. A fresh actual
source-driver run also passed locally on Darwin arm64 after the offline-cache
recipe change and the selected-Node-wrapper correction. It acquired public SDK/
compiler/Go/Cargo inputs, compiled the actual verifier/helper/client and passed
report checks plus the full offline suite. **47** source-policy tests passed
with pinned Python **3.12.12**. See [source CI evidence and limits](pi-source-ci.md).
PR statuses may describe synthetic merge checkouts; neither status nor local
source success supplies approved matching four-target artifact/report bytes.
The real universal candidate and managed-loader acceptance remain unestablished.

`publish-pi.yml` only reserves a hard-disabled workflow identity: its sole dormant
body exits with a closed blocker, even if its false job guard were removed.
**4** offline static/shell tests and bounded independent review accepted this
boundary. No functional publisher/mirror, exact artifact handoff or publication
admission/reconciliation is implemented. Referencing the dormant environment does
not verify protection; changing the guard would cross its privilege boundary.
Do not enable it, configure a token/placeholder path, or treat it as ready to publish.

## Privacy and remaining limits

Apply [data boundaries](../PRIVACY.md#data-handling-boundaries),
[forbidden exports](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[typed failures](../PRIVACY.md#mandatory-typed-interface-failures) and
[synthetic testing](../PRIVACY.md#synthetic-testing-exception).
Only synthetic local fixtures/public build inputs were used. No runtime telemetry,
request log, support bundle, credential, profile/history or live inference was
added. Public dependency acquisition differs from offline runtime testing.
Nix/npm/GitHub and host Pi/T3 history/capture have separate persistence boundaries;
this is not a whole-process transience claim.

Still unestablished: current exact-source all-target receipt/handoff and universal
archive; older macOS floors and download/quarantine behavior; independently
reproduced complete JS/WASM payload; actual npm-managed profile loading;
installation/activation/live behavior; rights and complete notices; organization,
package record, protected reviewers/tags and trusted publishing. No binary upload,
package bootstrap, placeholder, long-lived npm token or publisher enablement is
authorized by these checks.
