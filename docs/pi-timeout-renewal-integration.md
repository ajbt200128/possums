# Timeout, renewal and diagnostics — local integration checkpoint

## Status and source

**Ready for independent conflict review; not installed or pushed.** The operator
approved merge and installation, with installation conditional on integrated
qualification and review of meaningful conflict resolutions. This checkpoint
pauses at that review gate. It is not gateway deployment or live expiry evidence.

Worktree: `/private/tmp/possums-timeout-renewal-integration`, attached to local
`main`. Clean starting main/origin: `6149e423242db730e37cf56190be084308452759`.
Merged only `fix/provider-timeout-alignment` at
`75744a7027736b3e7385641db41be21ed0ac9cda` via merge commit
`e622af19a0a763e610ebb422eca83e0fb27ab0e3` (the exact archive/build source).
The source branch contributes `edb9979`, `0c8bcd2`, `7c5b0ac`, `0fac62c` and
`75744a7`. No blue/green or free-feature branch was merged. Main's diagnostic
commits `0b5103f`/`d393196`, installation record `eda32f9`, API-only reverts and
image reconciliation `6149e42` remain ancestors.

The root paid image pin is unchanged:
`sha256:d937a95193357ef5e0cb9c44c87dccc4b6c10160f21cb7df1e4c37f293aa7be5`.
`tinfoil-config.yml` remains byte-identical to starting main, SHA-256
`e19157cc7e2f71ae7980a457278aed3ffe658111592ef4a6f53311ae38047acb`.
This does not build or deploy a new image.

The separate `/Users/mb5/projects/possums` worktree was neither edited nor staged.
Its six dirty Rust files and empty index were preserved; before/after
`git diff --binary` SHA-256:
`84077b600f6e8db689efbc0ca7c766a2d3cbbfddc2dda5e203de95cf68256112`.
Builds used `git archive` of the committed merge, not those working files.

## Deliberate conflict resolution

Conflicts were confined to `examples/phase01/{client,limits,transport}.ts`,
`clients/pi/README.md` and append-only sections of `docs/verification.md`.

- `Operation` composes nullable total/idle timers with main's closed
  interrupted/idle/deadline provenance and parent cancellation propagation.
- Reference login keeps diagnostic stage/constraint handling, clears observed
  bearer expiry on every attempt, uses the reviewed two-control-operation
  envelope, and records validated expiry conservatively before session POST.
- Chat/parsing have no absolute stream lifetime. Standalone bodies retain idle
  waits; privately marked encrypted bodies delegate inactivity to HTTP bytes.
  Hooks retain individual finite waits and main's authored failure categories.
- Encrypted transport retains main's request-local frame-error snapshot before
  cleanup, UTF-8/decryption distinctions, observed status and hook diagnostics.
  HTTP-byte waits govern encrypted fragment progress without a competing
  decrypted-frame timer. Non-chat controls retain finite total deadlines.
- Provider/index changes auto-merged. Inspection and the combined suites retain
  synchronous source marking, first-user-message confirmation, one-use matching
  signal permission consumed before awaits, renewal ownership epochs and atomic
  trust/auth/catalog publication. Detailed authentication/startup diagnostics
  remain; no awaited idle pre-run hook, new retry authority or history reset.
- Both historical verification additions were retained without changing past
  results. README distinguishes the installed diagnostic predecessor from the
  integrated renewal candidate.

These composed transport/error semantics require independent read-only review
before installation, despite passing functional tests. The review diff is
`git show --remerge-diff e622af1`; the full integration is
`git diff 6149e42 e622af1`. No new test expectations were weakened to pass.

## Fresh offline qualification

Read complete repository AGENTS/PRIVACY guidance, build scripts and relevant
release/diagnostic/renewal records. Read the pinned installation's complete
`sdk.md`, `extensions.md`, `sessions.md`, `compaction.md` and `custom-provider.md`;
actual Pi 1.0.4 source/SDK fixture behavior remains authoritative, not a runtime
upgrade or a claim about the current main-loop's Pi 1.1.0 activation.

Scratch root: `/private/tmp/possums-pi-build-integrated-t1Dq1i` (`S` below).
Reproduced `build.sh`'s layout from `git archive e622af1`. Reused the dependency
tree from `/private/tmp/possums-pi-build-renewal-eROom8/source/clients/pi` only after
byte-identical package-lock comparison. No `npm ci`, dependency installation or
repository dependency link was used. Build/check ran with `env -i`, isolated
HOME/TMPDIR, Node **24.13.0**, TypeScript **5.9.3**, esbuild **0.25.10** and all
three Pi peers **1.0.4**.

```sh
cd "$S/source/clients/pi"
PHASE02_OUT="$S/package" node build.mjs
POSSUMS_PI_ROOT=/Users/mb5/.pi/agent/install/releases/1.0.4 node check.mjs
```

Passed: **35** reference, **537** encrypted/tool/client, **128** synthetic release,
**100** Pi/provider/diagnostic groups and **71** shared-admission checks. The
537-check encrypted suite passed two additional isolated repetitions. Coverage
includes actual encrypted fragments/stalls and closed error preservation,
hostile diagnostic inputs, native startup/auth races, native transient retry
budget/Stop, source/signal correlation, nested/ambiguous inputs, renewal failure
and late-publication races, compaction and authenticated billing/receipt gates.
The historical Pi pre-run Stop blocker and retry-backoff diagnostic-loss
reproducer remain passing evidence of external limitations, not fixes to Pi.

The scratch production package separately passed import, runtime guard, provider
and all renewal hook registrations, and offline `/possums-status` with fetch
forbidden and **zero fetch attempts**. No session authentication or live startup
was invoked. All **152** bundled input hashes, extension hash, lock hash and
three linked runtime manifest hashes matched the build report. Six active LSP
probes reported no errors but were inconclusive; the full pinned compiler is
the type-check evidence, not a clean-LSP claim.

Rust used a fresh scratch target, existing offline Cargo cache and the existing
Nix Rust **1.88.0** toolchain:

```sh
export PATH=/nix/store/89rqzskr6m71aqpxrglhyifrszxf3a54-rust-minimal-1.88.0/bin:/usr/bin:/bin:/usr/sbin:/sbin
export CARGO_HOME=/Users/mb5/.cargo CARGO_TARGET_DIR="$S/target"
cd "$S/source"
cargo test --offline --locked --lib --tests
cargo test --offline --locked --manifest-path vendor/tinfoil/Cargo.toml verifier::tls::tests
cargo clippy --offline --locked --all-targets -- -D warnings
```

Passed: **302 Rust tests**, **three live tests ignored**, **10 vendor TLS tests**,
and strict root Clippy. The vendored `CheckpointSignature::encode` dead-code
warning remains. Synthetic/loopback accounting, disconnect, telemetry/privacy,
HTTP connection lifetime and paused-time streaming checks are not external
network or deployed behavior qualification. Logs are scratch-only `build.log`,
`check.log`, `rust-tests.log`, `vendor-tests.log`, `clippy.log` and
`package-check.log`.

**Formatting limitation:** `cargo fmt --all -- --check` failed on 28 files
(six first-party Rust files and 22 vendor files). `cargo fmt -p possums -- --check`
confirmed the six first-party discrepancies. All merged Rust and vendor bytes
are identical to reviewed branch `75744a7`; this integration did not import the
root worktree's uncommitted formatting or silently reformat reviewed source.
Formatting cleanup is a separable follow-up, not a reported passing check.
`git diff --check` passed. Functional suites passed; no blanket all-checks-green
claim is made.

## Exact candidate and installation state

| Artifact | SHA-256 |
|---|---|
| `package/extension.mjs` | `569e4cd5ce0a53613a9f99980fa677184bdf830ec940cd3fc39fce45ccb88f6c` |
| `package/build-report.json` | `00b1c219dcb0d3dabf4174eff4895f94561e1a156bffb04e4092e72087b058f9` |
| source `clients/pi/package-lock.json` | `74ab26441996f6c729fde920c06b522abd7a3aead66a89fae3e87acff650d8b0` |
| Pi 1.0.4 `pi-ai/package.json` | `c4e921309a8d3a91ffce9bef0bd151463ad31f0f151a8534e39ae3528cc27834` |
| Pi 1.0.4 `pi-coding-agent/package.json` | `248decde30021109984def7ddc8f7bf4caf21ef04910a1303ab19604206c1bd5` |
| Pi 1.0.4 `pi-agent-core/package.json` | `3aecf0b11ec6d69846d104c96fa0b529bafb72ee8ab6d4c289a5d86ae309ace8` |

**No new installed target was created.** The stable
`~/.local/share/possums/pi/installed` link still targets
`pi-diagnostics-70cc73c01165`, with unchanged extension SHA-256
`70cc73c01165f2be8112530ad0d885c605057ac52a9957df8f2977bfc066a9dd`.
Its predecessor rollback package remains untouched. After independent acceptance,
a separate installation can copy the qualified scratch package/report into a
new immutable versioned directory, verify hashes/pins, atomically replace only
the stable link and retain this diagnostic package for rollback. Record that
actual installation in a separate small commit, not retrospectively here.

No auth.json, settings, package registration, Pi runtime, current T3/Pi process
or gateway deployment was changed. No restart was requested or performed; any
future activation requires the user's full restart in the compatible runtime.
No running Pi 1.1.0 process is claimed to have loaded the Pi 1.0.4 package.

## Privacy and evidence boundaries

Policy references: [change control](../PRIVACY.md#mandatory-reference-and-change-control),
[data handling](../PRIVACY.md#data-handling-boundaries),
[forbidden errors/content/identifiers](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[processors/shutdown](../PRIVACY.md#processors-retention-access-and-shutdown),
[synthetic tests](../PRIVACY.md#synthetic-testing-exception) and
[required evidence](../PRIVACY.md#required-evidence-for-every-telemetry-change).

Only synthetic local/loopback fixtures were exercised. No real credential access,
public evidence acquisition, live inference, support upload, live telemetry
query/export, runtime upgrade, deployment or push occurred. No telemetry family/retention
policy changed. The release fixtures use the documented hardware/crypto/network
mocks; they do not requalify live attestation, billing, retention or timeout
behavior. Legacy-v2 freshness and the native-v3 CLI signer mismatch remain
unresolved. A failed delivery/Stop still establishes neither remote cancellation
nor a refund; native retries can incur another charge.

## Subsequent scoped formatting and review acceptance

After the initial paused checkpoint, the parent requested first-party formatting
only. Commit `3b30b0105779da97b5fab4b48ffe459ecbb51fd5` formats exactly
`src/inference.rs`, `src/inference/stream.rs`, `src/inference/upload_tests.rs`,
`src/server.rs`, `src/server/resource_streaming_tests.rs` and `tests/transport.rs`.
No client or vendor file changed. Used the existing Rust 1.88 toolchain's
`rustfmt 1.8.0-stable (6b00bc3880 2025-06-23)` with
`--edition 2021 --config skip_children=true` and those six explicit paths.
Each result exactly matches rustfmt applied to its prior committed bytes.
It is formatting-only, not literally whitespace-only: default rustfmt also
normalizes optional trailing commas, removes redundant braces around one
single-expression match arm and orders two `cfg(test)` module declarations.
A normalized comparison verified those are the only non-whitespace differences;
no expression, literal, timeout, assertion or error-policy change was introduced.

A fresh `git archive 3b30b01` in the same scratch root's `formatted-source`
passed `cargo fmt -p possums -- --check`, all **302 Rust tests** (three live tests
ignored) and strict Clippy using the same offline commands/cache/toolchain.
Logs: `formatted-fmt.log`, `formatted-rust-tests.log`, `formatted-clippy.log`.
The first-party formatting gate now passes. `git diff --check` passes too.

The broad `cargo fmt --all -- --check` also traverses the local path dependency
`vendor/tinfoil`; it is not limited to the first-party package. Reproduced that
check on a separate pristine `git archive 6149e42` in `format-baseline`.
Both baseline and post-format integration report the **same 22 vendor files**,
with all 22 diagnostic bodies identical after normalizing absolute paths and
hunk line numbers. This includes `vendor/tinfoil/src/verifier/tls.rs`: the added
connection-timeout comment/call introduces no formatting discrepancy, so the
vendor file was not reformatted. The other 21 vendor files were not changed.
Logs: `fmt-baseline.log`, `fmt-all-after.log`. The broad vendor-inclusive gate
still fails on established baseline formatting, not new timeout-line drift;
no all-checks-green claim is made.

The independent reviewer then completed read-only review of the actual
integration worktree and reported **no semantic blockers**: closed diagnostic
snapshots, byte-idle timers and signal/epoch-bound renewal are preserved. The
reviewer did not rerun tests; the test evidence above belongs to the implementer.
The parent accepted that review and authorized installation after the first-party
formatting gate. Installation evidence is recorded separately below when done.

Client build inputs remain byte-identical to merge source `e622af1`; the qualified
JavaScript artifact/report retains that exact build-source identity and hash.
The formatting-only Rust source is `3b30b01`, not a claim of a new JS rebuild or
new gateway deployment. Root dirty files and the installed link remained
untouched throughout formatting and review.
