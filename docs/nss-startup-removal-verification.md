# NSS and startup-diagnostics isolation candidate

> **Historical v56 removal:** the [exact-v54/v59 re-release](v54-rerelease-v59.md)
> restores NSS and bounded general startup diagnostics after operator-reported
> instance-recreation recovery. This removal was not a proven collateral fix.

**Subsequent scoped control:** the [runtime-path regression control](runtime-path-regression-control.md)
retains this NSS/startup removal and all three restored measured settings, but
returns Rust to 1.88.0 and removes later upstream-client renewal/generalized
ingress. The Rust 1.92.0 statements and test counts below remain evidence for
this earlier checkpoint, not the later control.

## Decision and scope

The operator requested removing two possible contributors together for the next
attempt: NSS image contents and **all** application startup diagnostics. This is
an isolation experiment, not an established fix for native collateral acquisition.
A success would not distinguish these two changes from each other or exclude
changed VM/provisioning conditions. Rust **1.92.0** remains pinned.

Base: exact main `df921d81ce4d3251665c00af32b71a11af7cb202` (v55 source).
The unchanged-v55 retest, production run `39149565543`, completed with one
successful update receipt and then `SERVING_SDK_FETCH`. Subsequent three-GET,
zero-POST reconciliation observed v55 running, no pending update, preserved
strict flags/domain and legacy secret-reference presence. This is not a fresh
503-envelope observation, proof of physical cleanup or a host-placement comparison.
The interrupted local watcher did not retain the before/after opaque comparison.

## Changes

- Remove `gatewayAccounts`/`fakeNss`, the image contents entry, associated
  freshness declarations and account-file check. Keep numerical UID/GID
  `65532:65532`, entrypoint, read-only root and strict release-cache checker.
- Restore `src/main.rs` exactly to `v0.0.43`. This restores its startup ordering,
  including signal registration after verified connection/TCP binding and before
  acceptance. Retain typed failure classification, nonzero exit codes,
  healthcheck diagnostics, ordinary aggregate telemetry and primary-failure
  precedence over cleanup failure.
- Remove the startup owner, stderr milestones/terminal records, Honeycomb logs,
  lifecycle snapshots, delivery warnings/budget and startup-only tests/helpers.
  Remove the OTLP logs feature and its now-unused lock dependency edge; no package
  versions change. The shared transport returns to fixed `/v1/metrics` only.
- Preserve the egress allowlist, `start_period: 60s`, memory-only
  `/tmp: "size=64m,mode=1777"`, SDK pins, attestation/TLS verification, accounting,
  API/client wire contracts and all release/protected-deployment gates.

Startup silence is an explicit scoped operator decision, not permission to
silence health/request failures or claim that the platform collects no logs.
The former startup-collection exception and NSS verification are marked historical.

## Local evidence

Existing pinned devenv Rust/Cargo **1.92.0**, Darwin arm64; synthetic/local peers
only, no funded inference or Honeycomb export:

- `devenv shell -- cargo fmt --package possums -- --check`: passed.
- `devenv shell -- cargo test --locked --all-targets --all-features`: **511 passed,
  zero failed, four funded/live tests ignored**. Includes four actual binary
  startup fixtures requiring empty stdout/stderr and original exits 2/3/8,
  hostile configuration canaries, healthcheck, trust, accounting/drain and
  ordinary telemetry off/outage/owned-I/O/actual local OTLP-wire tests.
- `devenv shell -- cargo clippy --locked --all-targets --all-features -- -D warnings`:
  passed; vendored Tinfoil's pre-existing unused-method warning remains.
- Flake-pinned Python **3.12.12** release suite: **80 tests, one opt-in graph test
  skipped**, no failures. With `RELEASE_CACHE_GRAPH_CHECK=1`, all **12 cache tests
  passed**, including actual pinned Linux graph partition equality and selected
  external-output reachability. Graph evaluation does not realize an image.
- `git diff --check` passed. Active LSP check confirmed three files clean; five
  timed out. The pinned compiler/tests above, not the partial LSP result, are the
  Rust acceptance evidence.
- Independent read-only review found no source-code blocker. Its documentation
  finding (old current-tense NSS/startup claims) was addressed by policy updates
  and explicit historical notices, without rewriting earlier evidence.

## Privacy and remaining gates

Applied [data boundaries](../PRIVACY.md#data-handling-boundaries),
[forbidden telemetry](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[typed failures](../PRIVACY.md#mandatory-typed-interface-failures),
[startup-removal decision](../PRIVACY.md#startup-diagnostics-exception),
[processor/shutdown limits](../PRIVACY.md#processors-retention-access-and-shutdown)
and [required evidence](../PRIVACY.md#required-evidence-for-every-telemetry-change).
No new logs, trace capture, support artifacts, destinations, retries or inference
are introduced. Native Pi/T3/GitHub/platform history remains a separate boundary.

At this source checkpoint, hosted Linux image execution, fresh independent A/B
builds, publication and serving acceptance of the new candidate remain pending.
No installed Pi package is changed; older shipped clients were not newly
requalified end-to-end. Native CLI v3 signer mismatch, Pi legacy-v2 freshness,
native collateral-fetch cause, provider retention/invoice parity and physical
runtime cleanup remain unresolved. Normal release/provenance, current-main,
protected approval, one-update and separate fresh-serving checks still apply.
