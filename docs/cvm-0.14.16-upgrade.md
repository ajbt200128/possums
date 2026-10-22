# CVM 0.14.16 upgrade candidate — source and offline evidence

## Scope and exact upstream identity

The operator selected trying the latest stable released runtime, then explicitly
accepted the identified upstream observability risks after disclosure. The
[exact-version privacy exception](../PRIVACY.md#exact-cvm-01416-upstream-observability-exception)
is risk acceptance, not a claim that sensitive data cannot reach upstream metrics
or boot logs. Application privacy controls are unchanged.

Base: main `fdbc6841b29db4136d56f98586ea5ac9b5afd19f`. The source configuration
moves **0.14.12 → 0.14.16**, with every other config byte unchanged. A configured
pin is not independent readback of a live VM or its control-plane selection.

- Official latest stable release checked: [v0.14.16](https://github.com/tinfoilsh/cvmimage/releases/tag/v0.14.16),
  not a prerelease; published `2026-10-22T01:54:49Z`.
- Exact upstream commit: `4774a5dcfa2e9d4b95d6614c1d2d8125b58562e5`.
- Downloaded `tinfoil-inference-v0.14.16-manifest.json` SHA-256:
  `ae2045f144d422a2d5837556cc74c6109e61083d9c4aafa0c574aeeea8c8e98d`,
  matching GitHub's published asset digest. This is not full signed-provenance
  acceptance of a Possums release or proof of the running runtime.

## Compatibility and limits

Reviewed the [v0.14.12…v0.14.16 source diff](https://github.com/tinfoilsh/cvmimage/compare/v0.14.12...v0.14.16),
including shim attestation/socket handlers, collateral cache, runtime configuration,
container mounts, boot failure handling and metrics. The release includes the
Go 1.27.3/security snapshot rebuild, terminal boot-status serving, attestation
endpoint refinements and the accepted metrics/logging behaviors. Upstream shared
configuration advances from v0.1.14 to v0.1.18; no Possums SDK pin changes.

The unversioned no-nonce legacy and nonce-bound v3 attestation paths remain;
the explicit v3 route is additive, and fresh responses now report the v3 format
header. The local socket still requires GET, one valid nonce and no request body.
No source requirement was found to alter this CPU-only config's attestation/key
mounts, Tor identities, intake references, resources, read-only root, memory-only
`/tmp`, open egress or 600-second startup grace. GPU consent, persistent-volume
and ECR changes do not require enabling those unused features. This is source
compatibility assessment, not old-client artifact execution or boot qualification
against the new CVM; those runtime outcomes remain unverified.

[Upstream PR #430](https://github.com/tinfoilsh/cvmimage/pull/430) was **open and
unmerged**, head `09037cdcbebf7ade7716d4ba685db61c1f7a9b3f`, and is not an ancestor
of this release. Its proposed change concerns refresh scheduling after successful
fetches, not a new failed-fetch retry interval; the existing one-minute failure
cooldown is unchanged. There is no independent once-per-minute refresh timer:
a new request triggers refresh when eligible. **No claim is made that 0.14.16
fixes the observed `attestation_unavailable` failure or improves availability.**
No cherry-pick or custom CVM patch is included.

## Offline checks and preserved gates

- Flake-pinned Python **3.12.12**: release suite **104 tests, OK, 3 existing skips**;
  image suites **16 tests, OK**; intake-cutover suite **30 tests, OK**.
- Added a regression accepting exactly `v0.14.16`, matching the config pin, while
  rejecting the old pin, other versions, unprefixed/near-match/hostile values and
  an incorrect config command-line hash. Existing canonical VM shape, exact
  config/image, signature/source/invocation, protected approval, no-drift and
  no-replay tests remain in force. The version guard is aligned, not broadened.
- Existing image negative tests reject hostile probe output without echoing it;
  release tests retain content-free hostile-error and secret/reference checks.
  These checks protect repository-owned behavior, not the newly accepted upstream
  exposure. No claim of upstream label/log sanitization or absence is made.
- Pinned Rust **1.92.0** `cargo fmt -- --check`: **PASS**. The Nix formatter wrapper
  did not reach formatting: its Darwin prerequisite source download for
  `bmake-20250308.tar.gz` returned HTTP 404. A broader `cargo fmt --all` also checks
  unchanged vendored dependencies and reports existing formatting differences;
  no vendor reformatting is included. Normal hosted CI remains required.
- Config preservation and `git diff --check`: **PASS**. No new CI requirement or
  workflow override, no `no-rollout-needed` label, and no full CVM/custom build.

The policy accepts authenticated cumulative scrape exposure and raw upstream
boot failure logging, including client-controlled label encoding/correlation and
possible private boot details. Actual platform scraping, capture, access and
retention remain unknown; source inspection found no new automatic outbound
exporter. It does not establish five-minute-window compliance or transience.
See [data boundaries](../PRIVACY.md#data-handling-boundaries),
[application telemetry restrictions](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data)
and [processor boundaries](../PRIVACY.md#processors-retention-access-and-shutdown).

This packet prepares a PR only: no merge, release dispatch, deployment, production
probe, inference, client installation/activation or secret mutation. Independent
exact-diff review, normal required CI, fresh measured release and public signed
provenance, then protected approval remain necessary. The operator subsequently reported stopping the instance and explicitly authorized
starting that same existing approved instance when needed. This state is not
independently verified, is not evidence of an outage/regression, and needs no
start for source PR preparation. Before any later rollout, use the supported
start operation only if needed, reconcile the console/intake-origin update and
require stable running/no-pending state; that update's outcome remains unknown.
No recreation, configuration/reference change, blind retry, concurrent update or
merge/approval waiver follows from start authority. Do not treat this source pin
as serving evidence. Follow the unchanged [normal release flow](approved-main-releases.md).
