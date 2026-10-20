# CI/release performance: bounded work reuse

## Scope

Source-only optimization based on main `0f57ba87849af8709e48f4e0902c93250bfa0db8`.
No workflow dispatch, rerun, cancellation, merge, deployment, live application/Tor
probe, client activation or inference is part of this work. The parent thread
owns partial v0.0.65 publication and reconciliation; this work neither observes
its remaining jobs nor attempts recovery.

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
of the relevant paths. No cache settings or toolchain/dependency pins changed.

## Input identity evidence

Offline, locked `x86_64-linux` evaluation at exact source S and release R
`0c52e596aa9e2f64dceb7f25891a0b177d9a3c99` found all **eight packages** and all
**seven checks** have identical derivation paths, including their selected Cargo
artifacts. Derivation identity binds the recursive declared input graph, not
merely matching source filenames. The existing `relation` verifier also passed:
R has sole parent S and only the intended image-line transformation is permitted,
with config mode unchanged; the actual diff contains only `tinfoil-config.yml`.

For this optimization versus S, only the Linux Clippy check derivation changes.
Gateway image, smoke image, Tor probe, release-build-deps and all other checks
retain identical derivation identities. Neither finding authorizes skipping
exact-tag checks. No cross-ref CI result-reuse policy is introduced.

## Changes and expected gains

1. All-features Clippy consumes the existing Tor Cargo artifact derivation,
   rather than the non-Tor one. `--locked --all-targets --all-features -- -D warnings`
   remains unchanged. Cargo still validates feature/profile fingerprints; the
   compilation saving is not yet benchmarked.
2. Build production/smoke images and the key-free Tor probe on the flake runner,
   where checks already realized their dependencies. The smoke image realizes
   its API fixture dependency; requesting that fixture separately is unnecessary.
3. Transfer only two archives and the public probe ELF, not a Nix store, source
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
steps**. Most should disappear, replaced by artifact upload/download, a small Tor
probe build moved to the producer, and Docker loads/qualification. This is a
**projection**, not an achieved nineteen-minute reduction or SLA. Upload time,
artifact size/storage cost, runner disk headroom, cache effects, Clippy savings
and hosted wall-clock gains need actual PR/main measurements. A cold flake build
can still exceed the publisher wait budget.

## Preserved release gates and deferred orchestration

`publish-image.yml`, `release.py` and the strict release-cache checker are
unchanged. CI artifacts never enter release builders. Two fresh independent
application/helper/NSS/image builds, no substitution/remote builders for fresh
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

## Verification and privacy limits

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
  the ordinary PR CI gate. Hosted artifact transfer/timing is not yet qualified.

Policy: [data boundaries](../PRIVACY.md#data-handling-boundaries),
[forbidden raw errors/identifiers](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[typed failures](../PRIVACY.md#mandatory-typed-interface-failures), and
[synthetic testing](../PRIVACY.md#synthetic-testing-exception).
Only public workflow timings and derivation identities are recorded. GitHub's
native workflow/cache/artifact retention remains a separate persistence boundary;
this is not a whole-process transience claim. No telemetry, production logs,
private custody, runtime/key/identity/reference changes or exports were added.
