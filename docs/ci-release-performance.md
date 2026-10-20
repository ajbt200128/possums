# CI/release performance: bounded work reuse

## Scope

Source-only optimization based on main `0f57ba87849af8709e48f4e0902c93250bfa0db8`.
The operator subsequently authorized PR104 check-workflow reruns and a draft
stacked PR105 with sequential first/repeat checks. Publication/deployment reruns,
manual workflow dispatch, cancellation, merge, deployment, live application/Tor
probes, client activation and inference remain outside this work. The parent
thread owns partial v0.0.65 publication and reconciliation; this work neither
observes its remaining jobs nor attempts recovery.

## Measured critical path, not a root-cause claim

Completed public Actions job/step metadata gives this baseline:

| Invocation, attempt 1 | Work | Elapsed |
| --- | --- | --- |
| PR CI `39338220343` | flake check / subsequent image seed | 18m32s / 1m39s |
| Main CI `39342287569` | complete workflow's job span | 35m50s |
| Main CI | flake job / flake check / subsequent image seed | 16m56s / 14m03s / 1m14s |
| Main CI | image job, after flake | 9m43s |
| Main CI | image-startup job, after flake | 18m52s |
| Main CI image-startup | smoke image / production image / Tor probe builds | 6m12s / 11m34s / 35s |
| Exact-tag CI `39347417578`, completed flake job `118383268699` only | flake check / subsequent image seed | approximately 84m / 7m |

The graph is `flake(checks -> seed) -> {image, image-startup}`. Its longest
post-flake branch is almost nineteen minutes, overwhelmingly builds, not Docker
qualification. The publisher then requires two fresh agreeing builds and fresh-A
health qualification, publishes the archive, and serially waits for exact-tag CI
before the official signed publisher and protected-candidate dispatch. Its
`dispatch_wait` budget is 5,400 seconds **per invocation**. Timeout is an observer
failure, not cancellation or permission to replay publication.

Bounded, transient inspection of four **completed synthetic CI job** logs
retained only exact Nix build/copy grammar projections. No raw log was saved or
reproduced. The same Tor dependency derivation
`kfic6qkdqy09bafmg990m71hdi3p64pf-possums-deps-0.1.0.drv` was built in main's
flake, image and image-startup jobs. The non-Tor dependency derivation was also
rebuilt in image-startup. Thus seeding did not prevent repeated compilation.
Main flake had 953 recognized substitution lines; exact-tag flake had 962.
Substitution was occurring: this is **not** evidence of a wholly unused cache,
a cache hit ratio, or the reason a particular output was unavailable.

Main and exact-tag logs showed identical dependency/check derivation identities.
The interval from dependency-build starts to the first dependent checks' starts
was approximately 6m39s on main versus 39m54s on the tag. These are scheduling
intervals, not isolated compiler benchmarks. They locate expensive repeated work
but do not explain the slowdown. One cache-usage metadata snapshot was roughly
10.8 GB / 10.5k entries; capacity pressure is a hypothesis, not proven eviction
of the relevant paths. That initial investigation changed neither cache settings
nor toolchain/dependency pins; the subsequent lookup-order experiment is below.

## Input identity evidence

Offline, locked `x86_64-linux` evaluation at exact source S and release R
`0c52e596aa9e2f64dceb7f25891a0b177d9a3c99` found all **eight packages** and all
**seven checks** have identical derivation paths, including their selected Cargo
artifacts. Derivation identity binds the recursive declared input graph, not
merely matching source filenames. The existing `relation` verifier also passed:
R has sole parent S and only the intended image-line transformation is permitted,
with config mode unchanged; the actual diff contains only `tinfoil-config.yml`.

For the initial optimization versus S, only the Linux Clippy check derivation
changes. Gateway image, smoke image, Tor probe, release-build-deps and all other
checks retain identical derivation identities. The subsequent terminal-archive
cut changes three check identities, but all eight Linux package identities and
both dependency artifact identities remain unchanged. Neither finding authorizes
skipping exact-tag checks. No cross-ref CI result-reuse policy is introduced.

## Changes and expected gains

1. All-features Clippy consumes the existing Tor Cargo artifact derivation,
   rather than the non-Tor one. `--locked --all-targets --all-features -- -D warnings`
   remains unchanged. Cargo still validates feature/profile fingerprints; the
   compilation saving is not yet benchmarked.
2. Build production/smoke images and the key-free Tor probe on the flake runner,
   where checks already realized their dependencies. The smoke image realizes
   its API fixture dependency; requesting that fixture separately is unnecessary.
3. Transfer only two archives and public verifier executables (the Tor probe and,
   in the follow-up, the standalone serving verifier), not a Nix store, source
   tree, credentials or diagnostics. The existing pinned v4 artifact actions use
   immutable upload-result IDs, current-run downloads, no cross-run token, and
   names containing producer run/attempt/SHA. Retain for one day with compression
   disabled for already-compressed archives. Missing files fail the producer;
   build failure stops staging; missing downloads/files fail consumers.
4. Keep the `flake`, `image` and `image-startup` jobs. `image` loads the producer's
   production archive and checks Linux/amd64. `image-startup` runs the same real
   rootfs/NSS/Tor SDK positive/negative, packaged health and network-isolated
   read-only startup checks against those exact producer artifacts. It restores
   the probe execute permission lost by artifact packaging.

This explicitly changes **where CI builds run**, not their required coverage:
three CI production-image realizations become one; two smoke-image realizations
become one. It eliminates downstream Nix installation, substitution and repeated
compilation, rather than assuming best-effort cache publication works. Both
consumers use the same producer image. Partial job reruns can retain a successful
producer from an earlier attempt of the **same run/ref**; the immutable IDs keep
that provenance rather than claiming new production in the consumer attempt.

The historical downstream branch contains about **18m21s of image/probe build
steps**. Those downstream builds are replaced by artifact upload/download, a small
Tor probe build moved to the producer, and Docker loads/qualification.

At initial head `b734ed1335a556488b21e7856bc24c31ae6a0631`, PR104 CI
`39359478642` passed both attempts. Attempt 1's job span was **1,108s (18m28s)**,
with image/startup jobs **8s/16s**; attempt 2 was **1,384s (23m04s)**, with
image/startup **9s/14s**. Relative to historical main's 2,150s, the observed spans
were 48.5% and 35.6% shorter. Producer flake-check times were 886s and 1,116s;
image/probe preparation was 107s and 140s. Artifact uploads were 2s/1s in both.
These are successful hosted handoff/qualification results, not a controlled
causal benchmark, SLA, measured Clippy saving or proof of cache health. Runner
and cache conditions varied. A cold flake build can still exceed the publisher
wait budget.

Draft PR105 adds only a test-only Markdown file on top of PR104. All seven Linux
check and eight package derivation identities were verified identical across
those exact heads before its sequential first/repeat CI measurements. It is a
probe, not a candidate to merge; its results do not qualify the later cuts below.

## Preserved release gates and deferred orchestration

`release.py` and the strict release-cache checker are unchanged. The publisher's
only follow-up change is cache lookup ordering before dependency prewarming;
its fresh-build commands remain unchanged. CI artifacts never enter release
builders. Two fresh independent application/helper/NSS/image builds, no substitution/remote builders for fresh
outputs, digest agreement, fresh-A qualification, exact release-only identity,
exact-tag CI, signed public provenance, actual invocation/attempt/artifact
binding, current-main admission, protected operator approval, immutable tags and
assets, configuration/reference integrity and one-update/no-replay remain.
No automatic label or timeout increase was added.

Completion-driven release continuation is worth separate design review, but is
not implemented here. It must bind an immutable public origin run/attempt and
S/R/tag/image evidence, accept only the exact successful CI/publisher completion,
recheck eligibility, serialize version/publication ownership, reject duplicate
or failed/uncertain publication, and preserve protected one-update admission.
A broad `workflow_run` listener or rerunning `prepare` is not safe continuation.
This packet leaves the fixed wait limitation explicit instead of silently
changing consequential release/recovery policy or adding a speculative state
machine. Any later cross-ref result reuse requires graph-equivalence proof and
explicit gate-policy review; the identity evidence above alone is insufficient.

## Follow-up cuts and cache evidence

The user subsequently requested parallel cache/artifact investigations and
explicitly authorized the supported cuts. Completed PR104 flake-log projections
showed one reported resource-exhaustion event in each attempt, followed by proxy
refusals/circuit-breaker markers; both dependency builds began about 13 seconds
later. This does not identify the upstream service's underlying resource limit.
Pinned cache source confirms that reads and writes share one permanently tripped
breaker per daemon invocation, and the upload worker skips queued paths once it
trips. Thus later successful builds need not warm the cache.

A bounded metadata snapshot found no matching narinfo entries for either
expensive dependency output, although PR104's merge scope had 45 NAR/narinfo
pairs totaling about 158 MB. Absence at that snapshot is not proof that an output
was never uploaded or was evicted. A largest-100-entry sample showed substantial
repeated content keys; the effective repository quota and retention were not
verified. No cache was purged, quota increased or service changed.

The pinned daemon explicitly sets its proxy priority to 1, before public
cache.nixos.org's usual 40. `scripts/ci-cache-order.py` changes only that daemon's
exact declared proxy entry to priority 50, preserving its trust/compression
parameters and every other configuration line. It runs after cache setup in
check CI and each independent release builder. Missing/ambiguous/changed entries
are left untouched with an explicit unqualified-order diagnostic; normal checks
continue, because this optimization must not make optional cache availability a
new admission gate. Other configuration I/O failures stop with an unknown-outcome
diagnostic. Private configuration and exceptions are never printed. This is an
evidence-backed **upstream-first experiment**, not proof that
all rate pressure or upload-side failures are resolved. Validation must establish
actual dependency publication and subsequent substitution, not just fast timing.
Source evidence: pinned [proxy setup](https://github.com/DeterminateSystems/magic-nix-cache/blob/5a7f052c8f0c6511bbef3570eeed4d39242171ef/magic-nix-cache/src/main.rs),
[shared breaker](https://github.com/DeterminateSystems/magic-nix-cache/blob/5a7f052c8f0c6511bbef3570eeed4d39242171ef/gha-cache/src/api.rs),
and [upload worker](https://github.com/DeterminateSystems/magic-nix-cache/blob/5a7f052c8f0c6511bbef3570eeed4d39242171ef/magic-nix-cache/src/gha.rs).

GitHub PR-created caches are scoped to their merge ref: same-PR reruns can reuse
them, but another PR, main or a release tag cannot simply consume that PR's warm
cache. A stacked PR does not inherit its parent's merge-ref cache. Main warming
and release visibility still need separate qualification after normal review;
no main push/merge/release is authorized by this experiment.

The second cut uses Crane's supported `doInstallCargoArtifacts = false` only for
final Clippy, non-Tor tests and Tor tests. A combined package/check/dev-shell graph
review found no downstream consumers of those three final archives. Their lint
and test commands, profiles, features and Node input remain unchanged; shared
dependency producers still archive and compile tests. Linux archives already
chain deltas, so the removed payload is not assumed to be three whole duplicate
dependency trees. All eight package identities, including image/probe/prewarm,
remain unchanged. Sizes and additional hosted seconds saved remain unmeasured.

The third cut compiles the standalone serving verifier once in deployment
preflight and transfers that exact executable to the protected job through an
immutable producer artifact ID. Its numeric ID is checked before downloading;
current-run selection and explicit flattening avoid name/cross-run lookup and
incorrect extraction paths. The one-day artifact contains only the verifier,
not credentials, configuration or an earlier serving verdict. Approval and all
postapproval/final admission/provenance/plan/readback checks remain unchanged.

Both producer and consumer qualify executable launch without arguments, under
the existing credential-scrubbed environment and a ten-second/64-KiB bound.
The existing Go code rejects those arguments before SDK construction; only the
expected exit/empty stdout/closed `SERVING_ARGUMENTS` response is accepted.
Launch failure blocks admission before any update. This proves local execution,
not complete ABI equivalence or successful serving. The post-update fresh
attestation/endpoint-bound serving check remains mandatory. Ordinary PR CI also
builds this exact verifier with its existing pinned compiler, transfers it with
the public fixture payload and repeats the local-only launch qualification on a
second Ubuntu runner. No application request is made by this test. Go/SDK source,
compiler and flags are unchanged; actual protected-workflow transfer/deployment
is not authorized or qualified here.

Offline follow-up verification passed **102 release tests**, including all three
opt-in Linux graph checks, plus **16 image tests**. This includes the twenty
cache tests, synthetic configuration/privacy failures, exact artifact wiring,
missing/hostile artifact IDs, real synthetic subprocess launch, credential
scrubbing, and launch-failure refusal before mutation. Nix calls were
offline/no-lock-write. Independent follow-up review found no blocker and repeated
both suites successfully. Active diagnostics were clean on eight changed paths;
`git diff --check` passed. Hosted follow-up performance/transfer remains pending.
The following initial-head checks are historical evidence for the first patch,
not blanket qualification of every follow-up.

## Initial-head verification and privacy limits

- Flake-pinned Python 3.12.12: **90 release tests passed**, including both opt-in
  Linux graph tests. Nix was wrapped to add `--offline --no-write-lock-file`;
  full suite took 10.673s locally (not a hosted build benchmark).
- **16 image projection/negative tests passed**; no Docker/application was run
  locally. The new CI tests execute the actual staging shell with synthetic Nix,
  verify exact copied files, and verify build failure prevents staging. Static
  guards retain all qualification commands and independent release builders.
- Active LSP checks: four changed Nix/YAML/Python files clean. `git diff --check`
  passed. The pinned download-action metadata supports `artifact-ids` and default
  current-run selection. No dependency/action/toolchain upgrade.
- Independent read-only Sol review found no blocker. Its low-severity concern
  about claiming same-consumer-attempt production was addressed by documenting
  the actual immutable producer/run binding and partial-rerun behavior.
- A direct pinned offline Darwin formatting build exceeded a bounded 90-second
  wait. No pass or code-failure diagnosis is claimed; no dev-shell bootstrap was
  retried and no pins changed. Full Rust/Clippy/Linux/Docker execution remains
  the ordinary PR CI gate. Subsequent hosted image artifact transfer/timing is
  recorded above; follow-up cuts still require their own hosted qualification.

Policy: [data boundaries](../PRIVACY.md#data-handling-boundaries),
[forbidden raw errors/identifiers](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[typed failures](../PRIVACY.md#mandatory-typed-interface-failures), and
[synthetic testing](../PRIVACY.md#synthetic-testing-exception).
Only public workflow/cache measurements and derivation identities are recorded. GitHub's
native workflow/cache/artifact retention remains a separate persistence boundary;
this is not a whole-process transience claim. No telemetry, production-log or
private-custody access, runtime/key/identity/reference changes, or private
diagnostic exports were added.
