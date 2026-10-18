# Recreated production instance target — v60

After the [v59 exact-v54 re-release](v54-rerelease-v59.md), the operator requested
deployment and verification. The pre-approval selector could not find exactly
one match for the configured `possum-phase0` name/repository. It stopped before
approval; **no v59 update was attempted**. The operator confirmed that the
recreated production instance is named **`gateway`**, then explicitly approved
correcting the target, publishing v60 and deploying/checking that release.

## Minimal correction and preserved gates

Change only `scripts/deploy.py`'s fixed `NAME` from `possum-phase0` to `gateway`.
The CLI lookup/update and read-only plan identity checks all use that constant.
Retain the historical hostname-transition allowlist; the observed target already
uses `gateway.possums.dev`. Do not add an arbitrary target parameter, weaken name,
repository, domain, provenance or current-main checks, or run an old release
through a patched ad-hoc deployment path.

A new source commit requires a new normally qualified release: v60 is expected
to retain v54/v59's exact image and measured config/manifest. V59 remains an
unchanged published release, not retagged or replaced. Its old pending production
run must not be approved; the current-main gate makes it stale after this merge.

Three read-only GETs with the corrected selector confirmed v54 running/no pending,
approved domain, confidential mode enabled, debug/hold disabled and required
reference presence. The recreated instance's observed latest-release flag is
**true**; preserve it rather than copying the old instance's false value. Existing
opaque selections include historical references; do not add, select, remove or
publish their values. Their presence does not restore Tor runtime support.
Normal before/after configuration comparison and no-change plan guards remain.

## Verification and limits

A focused offline regression pins the configured target and actual CLI lookup to
`gateway`; the existing suite checks update arguments, both latest-flag values,
opaque-reference preservation, plan drift rejection, approval/current-main gates,
one mutation and separate serving verification. No gateway binary, SDK, Nix input,
measured config, client or telemetry implementation changes.

Applied [data boundaries](../PRIVACY.md#data-handling-boundaries),
[forbidden diagnostics](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[typed failures](../PRIVACY.md#mandatory-typed-interface-failures) and
[processor limits](../PRIVACY.md#processors-retention-access-and-shutdown).
Only closed projections/public release identifiers are recorded. No inference,
raw platform errors, configuration dump, new telemetry or automatic retry.

Local pinned Python 3.12 release suite passed: **81 tests, one opt-in graph skip**;
enabled Linux graph suite **13 passed**. Both changed Python paths passed active
LSP checks; diff checks passed. The Linux image derivation remains
`/nix/store/hpbx5ydy4insn6d720c660ps8ss906gd-possums-gateway.tar.gz.drv`.

At this source checkpoint hosted qualification, v60 publication, protected
approval, one update and exact-manifest fresh serving verification remain pending. A later successful update/attestation must be reported separately;
the original collateral failure's precise root cause remains unknown.
