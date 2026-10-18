# Candidate Linux image: offline NSS/SDK acceptance

> Current source-restoration scope: [Tor main restoration](tor-main-restoration.md).
> The key-free image probe and CI path are restored. The historical evidence
> below does not qualify this new head; new Linux execution remains required.
>
> Historical Tor image qualification record only. This inactive HTTPS-only
> backup removes the Tor probe/CI path; UID/GID 65532 NSS accounts and ordinary
> image startup/health gates remain. Commands and requirements below are not
> this branch's acceptance plan. Earlier failures remain unresolved at their
> recorded scope. See [backup verification](without-tor-backup-verification.md).

This is development/PR CI only, not deployment or live Tor acceptance. The draft
PR's verification record must pin the tested head, check workflow run/attempt,
Docker image ID and Linux architecture after CI finishes. Until that record has
all required checks green, shipping-image qualification remains pending.

## Boundary and composition

Policy: [data handling](../PRIVACY.md#data-handling-boundaries),
[forbidden telemetry](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[synthetic testing](../PRIVACY.md#synthetic-testing-exception), and
[startup diagnostics](../PRIVACY.md#startup-diagnostics-exception).
No telemetry/export changes, secrets, private identity inputs, inference or
production actions. Docker/Actions retain public build/container metadata and
closed test output; this is not a claim of whole-host transience.

- The integrated config fix selects `trust_no_group_id()`: group permissions are
  **not trusted**. Ownership checks, ignored permission-disable environment,
  private scratch and ephemeral keys remain enforced. The image adds only public
  locked account records from pinned `dockerTools.fakeNss`, not a login program.
- Existing full-preparation and composed TLS/stream/reservation/settlement/drain
  tests use public fixtures. The shared-ingress test injects the ingress boundary;
  it does **not** construct the SDK. See [r41 evidence](tor-offline-e2e-verification.md).
- `tests/support/tor_image_probe.rs` is a separate ordinary binary built by Nix
  from root normal dependencies and exactly `torCommon`'s `--locked --features tor`
  plus binary selection. Nix appends only a binary target to the probe's build
  manifest. No examples/tests, dev-dependency features, SDK versions or production
  build flags are substituted. Shared build artifacts do not enable extra Cargo
  features. The Rust source filter includes the public helper; the production
  image's contents/entrypoint do **not** include it.
- The existing `image-startup` job mounts that single ELF read-only in the actual
  `gateway-image`. It does not mount a host Nix store, libc or NSS. Successful
  execution therefore checks interpreter/shared-library resolution inside the
  candidate rootfs, not just a host SDK build. Image user metadata must equal
  `65532:65532`; libc real/effective UID/GID and by-ID/by-name user/group lookups
  must match those fixed records. CI asserts `linux/amd64`.
- Both runs use the same immutable image ID, UID/GID 65532, `--network none`,
  read-only root, all capabilities dropped, no-new-privileges, zero core limit,
  and a private owner-only 64 MiB `/tmp` tmpfs. Only the probe ELF is mounted in
  the positive case. The negative case additionally mounts a mode-000 **public**
  file read-only over `/etc/passwd` and `/etc/group`; UID 65532 has no DAC-override
  capability. `nsswitch.conf` stays the image's own. Neither run permanently
  modifies the image or host NSS. Validated public image ID/architecture print
  before either probe, and both runs use that exact ID even if the tag changes.
- Positive output must prove all stages in order: real NSS, actual
  `TorClient::builder()` default constructor, explicit strict configuration,
  Manual `create_unbootstrapped()`, and empty ephemeral key-manager access.
  This is not the full preparation factory; its existing direct regression and
  r41 composed tests remain separate, complementary coverage. No identity key
  is needed or inserted by this probe.
- Negative output must show the existing `fs-mistrust` dependency's typed
  `PasswdGroupIoError` with `io::ErrorKind::PermissionDenied`, then a caught
  failure at the **actual SDK default builder**, before `.config()`. Only that
  ordered transcript, empty stderr and exit 2 qualify. The preliminary typed
  lookup does not cache failures; the SDK still performs its own default lookup.
  No new dependency, POSIX ABI, interposer, permission-disable environment or
  panic-payload inspection is added. An empty **readable** account database is
  not this fixture: a successful no-match is valid no-group-trust behavior, not
  a security failure. Closed diagnostics distinguish that wrong fixture, an
  unexpectedly returning SDK builder, and unknown output; none qualify.
- The SDK is never bootstrapped; no onion launch, publication, attestation,
  endorsement, authentication or inference is attempted. Runtime shutdown is
  bounded to one second; container wait to twenty seconds with owned cleanup.
  Panic hooks never print SDK payloads; the parser rejects extra stdout/stderr,
  missing stages and wrong exits. Public hostile-output unit tests cover this
  projection and unexpected-exception fallback.

## Local evidence before PR CI

Pinned offline Nix development shell: Rust/Cargo 1.92.0, aarch64-darwin.

- `cargo test --offline --locked --all-targets --features tor`: pass; library
  409 passed / 1 operator-only ignored, plus ordinary binary/integration/example
  suites. Four funded tests remain ignored. Actual production preparation,
  strict 0700 vs 0750/0770/0777 checks and combined offline ingress test pass.
- Root all-target/all-feature strict Clippy and formatting: pass. Standalone
  ordinary-bin probe build/strict Clippy: pass (Darwin compilation only, **not**
  Linux NSS execution). Existing vendored tinfoil dead-code warning is unchanged.
- Seven offline Tor-image projection tests and nine existing image-health tests:
  pass after the NSS I/O-fixture correction. Coverage includes every required
  stage/exit, wrong-fixture transcripts, hostile stdout/stderr, mode-000 fixture,
  identical image IDs across runs and public identity output before failure.
  Root and ordinary-bin strict Clippy plus Rust formatting pass in the pinned
  Darwin shell; this does not execute Linux NSS. No Docker/inference is initiated
  by these unit tests.
- Pinned `checks.aarch64-darwin.gateway-accounts`: pass. Exact x86_64-linux
  candidate image and probe derivations evaluate. Local Linux image build is
  not substituted for cached original-repository PR CI.

## Required CI record and remaining limits

The draft PR's verification section is the public artifact record: exact head,
`check.yml` run ID/attempt 1, all required job conclusions, the positive/negative
closed stage evidence, and `stage=image_identity` / `stage=image_architecture`
from `image-startup`. Do not infer success from a queued job, image evaluation,
Darwin compilation, injected test, or an older workflow run. A corrective head
requires its own normal attempt-1 checks; no dispatch/rerun/release is authorized.

First candidate `aa2142d3b4f56d96c38596a0c75adae6fa00d12f`, original-repository
check run `39053432234`, attempt 1: `flake` failed after 21 minutes at the new
permission test's unconditional whole-path 0700 acceptance assertion (408 other
library tests passed); image jobs were skipped. That assertion assumed trusted
Nix sandbox ancestors. The sole corrective commit changes only this test to
inspect the production verifier's complete typed error set: 0700 may have only
ancestor ownership/permission refusals, while 0750/0770/0777 must specifically
include a leaf permission refusal. No ignored prefix, permission bypass or
production code change is introduced. The independent existing strict-ancestor
test and actual-image positive check remain required. No image result is inferred
from the first run.

Head `0ca0bb059d079438cfa618906e9e228a1a1aa964`, check run `39055238199`,
attempt 1: `flake` and `image` passed. The actual linux/amd64 rootfs positive
passed all NSS/default-builder/config/Manual-client/empty-keymgr stages.
`image-startup` failed the negative's exact projection after exit 2; packaged
health and startup smoke were skipped. The mismatching transcript was not
projected, so neither an actual SDK panic nor its absence was observed there.
The Docker ID was not printed before failure; prior Nix paths are not substituted
for it.

The separately authorized one-commit follow-up corrects that fixture, not
production. Pinned `fs-mistrust` 0.16.0 `src/user.rs`
`get_own_username`/`get_self_named_gid_impl` accept `Ok(None)` and
`TrustedGroup::SelfNamed` propagates only lookup I/O failures through
`PasswdGroupIoError`; failed results are not inserted into `TrustedUsersCache`.
Pinned `pwd-grp` 1.0.2 uses libc's reentrant lookups and preserves nonzero return
codes as I/O errors. Thus a readable empty database did not faithfully test the
intended I/O-error constraint. The corrected mode-000 hypothesis requires its own
new-head Linux attempt-1 CI observation; Darwin compilation is not EACCES evidence.
No source NSS, permission policy, SDK features, production binary/configuration,
workflow or publication change is made by this follow-up. Post-CI observations
belong in the PR record without creating another unqualified documentation head.

The operator additionally requires a **fresh independent Linux E2E after the
final candidate fixes and these image/SDK checks pass**, then final review.
The parent owns that delegation; this branch does not spawn it. Passing this CI
alone does not establish shipping acceptance. Repeat only public fixtures at the
exact final head/rootfs; keep full preparation, real SDK/keymgr construction and
composed ingress/TLS/API stream/accounting/drain/cleanup coverage distinguished
from a full daemon or live Tor test.

The earlier [NSS investigation](tor-config-builder-verification.md) and r41
record preserve historical pre-image-acceptance gaps. A passing new CI record
qualifies only this public, key-free Linux image path. Attribution of the
reported failed production update to NSS remains unconfirmed. Production serving,
real B custody under these OS records, Tor bootstrap/reachability/client auth,
fresh per-request attestation, live billing, native v3 platform signer and
legacy-v2 freshness limits remain outside this test. Production/client/
attestation/accounting/drain changes beyond the integrated config fix are absent;
all production actions remain with the parent/operator.
