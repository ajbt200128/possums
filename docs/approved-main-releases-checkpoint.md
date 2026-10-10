# Approved-main releases — historical restart checkpoint

> Resumption continued the implementation and resolved the unconditional serving
> gate and unsupported bypass-field check below. See the current
> [release/deployment guide](approved-main-releases.md) and
> [scoped verification](verification.md#approved-main-release-automation--local-branch-qualification).
> Activation and hosted/live qualification remain separate.

**Historical incomplete, uncommitted implementation for parent review; not accepted or activated.**
Working tree: `/private/tmp/possums-timeout-renewal-integration`.
Branch remains `feat/approved-main-releases`; HEAD remains
`ca24b8691ce4c04be5d440717011914274b66898`.
Stopped at the operator's restart request. No commits are required to resume.

## Approved design (do not redesign on resume)

- Automatic verified paid releases, gated by absent-by-default
  `RELEASE_AUTOMATION_ENABLED == 'true'`.
- Successful exact-main **push** `check.yml` completion can start preparation;
  manual entry must obey the same current-main/exact-source checks.
- S is the eligible main source. R is a deterministic release-only commit with
  sole parent S and only the gateway digest substitution in `tinfoil-config.yml`.
  Paid semantic tag V points to R; never merge R into main. Main config is a
  template, not deployed inventory. Preserve independent fresh A/B Nix builds,
  the existing cache boundary, digest agreement and exact archive publication.
- Explicit exact-R tag checks, then the existing pinned official tagged measure
  action. Public verifier binds manifest/config/signature and actual successful
  signed invocation, including its actual attempt, not hardcoded attempt 1.
- Separate absent-by-default `PRODUCTION_DEPLOYMENT_ENABLED`. Existing protected
  production Environment must be read-only validated before any job references
  it. Required reviewer is `ajbt200128`, self-review permitted, admin bypass
  disabled, branch-only main restriction. Secret must be environment-scoped.
- Only latest main is eligible **at update admission after approval**. Old
  approval requests may remain visible. Shared production concurrency is
  noncanceling; an update already admitted is not canceled when main advances.
  No CAS/durable supersession state, auto-approval, auto rollback or mutation retry.
- No mutating GitHub reruns. Uncertain/partial outcomes require human reconciliation.
- Independent serving endpoint-key binding must be qualified; control-plane
  Running is not serving verification. No changes to Pi approval authority,
  Free, runtime, credentials/settings, or deferred `tests/attestation.rs`.

## Files preserved in checkout

Modified:

- `.github/workflows/check.yml`: correlated exact-tag dispatch title/input and
  scoped `test_release*.py` tests; existing Nix/startup checks retained.
- `.github/workflows/publish-image.yml`: gated workflow_run/manual source admission,
  builds depend on admission, explicit S checkout, run/attempt/S artifact names,
  release-only publication path instead of pin PR, optional deployment dispatch.
- `.github/workflows/tinfoil-release.yml`: legacy entry delegates to the gated
  reproducible image workflow; no separate tag/deploy bypass.
- `.github/workflows/tinfoil-release-publish.yml`: tagged guard, originating build
  run/attempt correlation, existing pinned measure action, noncanceling publication.

New, untracked (must be retained/staged explicitly when parent eventually commits):

- `.github/workflows/deploy-production.yml`
- `scripts/release.py`
- `scripts/deploy.py`
- `tests/test_release_automation.py`
- This checkpoint.

`release.py` implements source admission, deterministic R, semantic paid version
allocation/reconciliation, S/R tree and image derivation checks, explicit check
and publisher dispatch/wait, cryptographic public verification, and deployment
dispatch. Publisher admission checks originating successful source/A/B jobs and
exact run/attempt/S digest artifacts. Existing releases are verified, never
replaced; prior/partial publishers are not automatically replayed.

`deploy.py` includes policy/provenance checks and a **dormant** single-update
wrapper adapted from the approved local v0.0.20 helper. The wrapper uses official
CLI 0.19.0 Linux amd64 checksum pin and documented `TINFOIL_ADMIN_KEY`, closed
stdin, bounded in-memory pipes, one update command, post-call plan categories,
opaque configuration comparison and bounded completion. There is no local
credential discovery/login, confirmation bypass, replacement flag, retry or
rollback. Its supported CLI plans execute before our post-call review; do not
claim this is a separate pre-mutation reviewed plan.

## Exact verification at checkpoint

- **PASS:** `python3 -m unittest discover -s tests -p 'test_release*.py'` —
  **37 tests**, 0.490s (existing cache tests plus new offline synthetic tests).
  Covers event/ref confusion, transform/tree relation, deterministic commit,
  versions/collisions, failed invocation no-redispatch, build attempt/digest,
  signatures/config substitution, policy/credentials, stale gates, one mutation,
  no rerun replay, bounded pipes/completion and opaque configuration preservation.
  Default tests make no remote calls.
- **PASS:** real Nix evaluation of
  `git+file://<checkout>?rev=S#packages.x86_64-linux.gateway-image.drvPath`
  versus a locally generated R with a synthetic changed digest: identical
  derivation path. S was the current HEAD above. This created an unreferenced
  local Git commit object only, not a tag/branch/ref or working-tree change.
  It evaluated identity; it did not build/reproduce a new image.
- **PASS:** official public 0.19.0 checksums asset independently matched
  `ef014c67a976b4b47941d084fb0a3837ba6eb59c9f4e662424c6826fb7a70f8c`
  for `tinfoil-cli_0.19.0_linux_amd64.tar.gz`. No CLI installation or execution
  against production occurred.
- **PASS:** `git diff --check` at checkpoint.
- **LSP/auxiliary diagnostics:** changed Python/YAML paths checked. Python type
  errors were resolved. The only remaining unsuppressed batch finding was
  zizmor's generic `dangerous-triggers` warning for explicitly approved
  workflow_run, dispositioned false-positive with the source-validation trust
  boundary rationale. Recheck on resume; this disposition is session-local.
  Earlier boolean singleton lint false positives were removed by a typed JSON
  boolean helper, preserving rejection of numeric zero/one.
- **NOT CLEAN:** `nix shell --inputs-from . nixpkgs#actionlint --command actionlint -oneline`
  exited 1 for `.github/workflows/check.yml:76:9`, existing startup smoke shell
  statement `! docker logs ... | grep ...`: ShellCheck **SC2251** (negation skips
  errexit). No other actionlint diagnostic printed. This statement was not
  modified; do not quietly broaden scope to fix it.
- No full Rust suite/Nix image builds, hosted workflow invocation, cryptographic
  live release verification, environment acceptance, or production update was run.

## Remaining work / blockers (do not claim completion)

1. **Hard serving gate:** `serving_prerequisite()` unconditionally stops preflight
   **before** the production Environment job, and again in the protected entry.
   No input/variable bypass exists. The existing Go helper verifies its local
   Unix attestation socket, not an independent public TLS connection. Pi retains
   its existing legacy-v2 path. Native-v3 signer mismatch and legacy-v2 freshness
   are unresolved. A separately reviewed supported endpoint-key verification path
   must bind the exact release/manifest and real endpoint without credentials or
   prompts before activation/acceptance. Do not invent a v3 protocol or mistake
   public release provenance/control-plane Running for verified serving.
2. **Policy API qualification:** the code fails closed unless production policy
   exposes `can_admins_bypass: false`, exact reviewer identity, branch-only main,
   and readable environment-secret metadata. Official environment REST docs do
   not clearly promise the admin-bypass field. Hosted GITHUB_TOKEN read access to
   environment secret metadata/repository variables also remains unqualified.
   Missing/unreadable evidence blocks; never assume safe defaults or weaken policy.
   No environment existed in the prior supplied inspection; no setup was done.
3. **Independent review and integration tests:** inspect actual diff and scripts,
   especially workflow_run trust, GitHub API/run-name/job-name shapes, tagged
   invocation/build binding, retry/partial publication and concurrency behavior.
   Current synthetic tests are not proof of hosted orchestration. Add focused
   gaps found by review; test full verifier/invocation failure paths as needed.
4. **Documentation unfinished:** `docs/ci-builds.md` still describes the old pin-PR
   flow and checks/builds in parallel. Update it. Write the requested concise
   deployment guide, including separately authorized setup, exact candidate/run/
   attempt/environment/current-head validation for explicitly requested operator
   approval, inline-only shell commands, manual reconciliation and separate
   plan/CLI/public-provenance/serving results. Update `docs/verification.md` with
   scoped local evidence and unresolved hosted/runtime gates. This checkpoint is
   not a substitute for those final documents.
5. Parent must decide whether retaining the dormant production wrapper is the
   smallest acceptable packet while serving/policy acceptance is blocked. No
   blanket end-to-end completion/acceptance claim is justified.

## Privacy and authorization record

Read `AGENTS.md` and `PRIVACY.md` fully before implementation, and read both supplied
local v0.0.20 scripts and existing helper/client boundary material. Relevant policy:
[data boundaries](../PRIVACY.md#data-handling-boundaries),
[permitted/forbidden telemetry](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[synthetic testing](../PRIVACY.md#synthetic-testing-exception), and
[required evidence](../PRIVACY.md#required-evidence-for-every-telemetry-change).
New diagnostics are locally authored stage/constraint/action codes; no raw parser,
CLI, control-plane or exception output is emitted. Synthetic tests include hostile
sentinels. No real configuration/secrets/prompts were read for tests.

**No live settings changes, remote mutation, push, merge, approval, deployment,
workflow dispatch, branch switch, global install or commit occurred.** No production
activation variable, environment or secret was provisioned. No changes were made
to `tests/attestation.rs`, Pi, Free, or gateway runtime.
