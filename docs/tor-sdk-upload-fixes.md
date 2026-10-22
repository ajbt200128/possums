# Narrow Arti descriptor-upload fixes

## Scope

This series uses Arti 0.47.0 without dependency upgrades or feature changes:

1. Descriptor upload opts out of nested HTTP header/body response clocks beneath
   its existing finite caller-owned attempt/overall deadlines and cancellation.
   Generic directory requests retain their existing 10s response clocks. Parsing,
   decompression, response-size limits and typed error handling remain shared.
2. `upload_all()` continues past a time-period context with no dirty destinations
   instead of returning before later dirty contexts can be scheduled.

HTTP 400 remains retryable under the existing policy. No reason/body classifier,
retry/backoff/fatality/circuit-budget/quorum change, health-policy change or new
availability guarantee is included. Stricter global `Running` semantics are
separate work. Existing deadline ownership is not a new strict wall-clock
guarantee for the retry loop.

Separate test-only corrections start accounting fixture token lifetimes after
expensive setup and resolve the paired Node decoder from Cargo's test CWD. The
accounting fixture lives in `src/accounting/synthetic.rs` on current main; its
function preimage and corrected body match the previously exercised fixture.
Production ledger and expiry rules are unchanged. No validation is relaxed, no
fallback/skip is added, and production JS/authentication/trust is unchanged.

## Review the local changes, not an unexplained upstream rewrite

The prerequisite imports only `tor-dirclient` and `tor-hsservice`; no local
`tor-circmgr` fork is introduced. Review the pristine import separately from the
small response-clock patch, scheduler/tests patch and root fixture corrections.

| Crate 0.47.0 | Pristine crates.io archive SHA-256 |
|---|---|
| tor-dirclient | `f942f4ebaf2ac5275e48d02d77abe1616ce2e8854f032a1b0698ede0566dddfc` |
| tor-hsservice | `55809560d9295dcc2ee77002c5a4275d1f5f5a54203de7cd6acb52215f5344bd` |

Both archives record upstream commit
`ce8bc6e0998bd5a4efdf06dd62dce53c98ea1087` in `.cargo_vcs_info.json`, with
`crates/tor-dirclient` and `crates/tor-hsservice` respectively. The observed tag
`arti-v2.7.0` resolves there; no tag-signature verification is claimed. Each
vendored `POSSUMS.md` records the canonical license URLs, checksums and local scope.
The exact upstream MIT/Apache licenses accompany both imports. Preserve them in
unfiltered source redistribution; a filtered compiler source is not a complete
redistribution bundle or an audit of all release-image notice obligations.

Root Cargo metadata changes only these two registry sources to local paths. The
lockfile retains all 619 packages, versions and dependency lists. Existing
current-main dependency features, including intake/ledger work, remain intact.
Nix includes exactly the two SDK READMEs needed by their compiler inputs and uses
`dummySrc = src` for Tor dependency artifacts, retaining the vendored definitions.
There is no broad license/docs allowlist or new CI framework.

## Evidence and remaining gates

Prior offline evidence on the identical final vendored Rust source:
- 21 publisher SDK tests passed, including response-clock defaults/opt-out/finite
  caller cancellation, retryable rejection recovery, selective-period scheduling,
  bounded timer/consensus selection and status composition.
- Native all-target/all-feature Clippy passed in the pinned Linux environment.
- After artifact restoration and package cleaning, Cargo JSON selected a fresh
  root library-test executable; pre-run binary checks required the relative Node
  module literal and rejected the stale absolute literal. All four selected
  accounting/paired-decoder targets passed. The subsequent normal-parallel root
  library run passed 435 tests, with 0 failures and 2 normally ignored tests.

These are pre-integration checks based on gateway commit
`cb96c6a7af393c28ed76ca20e7867c0a533c9769`, not current-main CI or release evidence.
Root Cargo tests do not execute dependency unit tests; the isolated SDK21-test
result is separate. Its temporary verification manifest/lock/expression is not
part of this source series. No retained compiled-artifact hash or signed release
provenance is claimed for the local root checks.

The broader Cargo invocation stopped in five existing `tests/attestation.rs`
helper integration tests. The private qualification source had lost the helper
fixture's tracked executable bit. Current-main integration preserves that bit;
this is not proof that its full suite passes. The legacy helper boundary did not
retain native spawn/exit details, and a stale compiler-directory binding there
was not ruled out. Do not reinterpret `Unavailable` as a successful helper check
or change verifier categories/freshness/limits to satisfy tests.

Run normal required CI on the integrated source and fix any remaining failures
narrowly. No ignored/public Tor tests, inference or client activation follow from
these local results. Do not merge with failing required checks. Source review,
exact required CI, public signed provenance and protected approval precede any
protected rollout; this runtime series is not `no-rollout-needed`.

## Privacy and exclusions

This patch adds no observer, tracing subscriber, diagnostic capture/export,
background logging, raw error projection or telemetry activation. Temporary
investigation code, raw records, private paths/configuration and identity/path
metadata are excluded. Existing bounded numeric-status diagnostics are unchanged.
See [permitted signals and forbidden data](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data).
This scope does not expand that policy or claim that all upstream logging is
impossible. Measured configuration, key/reference delivery, release/provenance
and protected-approval gates remain unchanged.
