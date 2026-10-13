# Automatic paid releases and approved production deployment

**Release flow guide, not a live activation or deployment-status report.**
The [2026-10-10 hosted qualification record](approved-main-release-qualification.md)
records automatic `v0.0.22` and the then-pending simplified deployment gate;
its disabled/pending statements describe that checkpoint, not current settings.
Both activation switches default off in code; their effective repository values
and exact run outcomes must be checked separately. No supplemental PAT is needed.
Historical [v0.0.20 rollout evidence](timeout-renewal-v20-rollout.md) does not
qualify later workflows or establish the currently served release.

## Release and approval flow

1. Successful exact-main **push** CI can start image preparation when repository
   variable `RELEASE_AUTOMATION_ENABLED` is exactly `true`. Manual publication and
   the legacy release entry obey the same switch/current-main checks.
2. Independent fresh A/B builds agree on digest D; the exact archive is published
   and the registry digest checked. Existing release-cache isolation is retained.
3. Deterministic release-only commit R has sole parent S (the tested main source)
   and changes only the image line in `tinfoil-config.yml`. Exact config/tree and
   gateway-image derivation identity checks bind S/R/D. R is tagged, **never merged
   into main**; main's config is a template, not deployed inventory.
4. Paid semantic version allocation is serialized. Exact tagged-R CI and tagged
   measurement/publication are explicitly dispatched because `GITHUB_TOKEN` pushes
   do not trigger ordinary downstream events. Existing immutable releases may be
   reused only after verification; partial publication stops for reconciliation.
5. Public verification checks exact manifest/hash/config/image, GitHub signature,
   tagged source/signer/predicate, and actual successful invocation and attempt.
6. Only when `PRODUCTION_DEPLOYMENT_ENABLED=true` does publication dispatch an
   exact S/R/V/D/manifest production candidate. Preflight validates an **existing**
   protected environment before a job references it; it cannot implicitly create
   an unprotected environment.
7. GitHub pauses at the production approval. The protected job rechecks policy,
   actual review history, immutable public provenance and current main before
   admitting one update. Only the latest main candidate may deploy at admission.
8. The official checksum-pinned CLI updates once, preserving opaque settings and
   secret references except the explicitly approved hostname transition from
   `possum-phase0.possums.containers.tinfoil.dev` to `gateway.possums.dev`. Other
   source hostnames are rejected. A bounded, redirect-free read-only Admin API
   plan is checked before the CLI invocation using the production credential;
   it must identify this instance/current resources and only the approved
   `custom_domain` setting change on the initial transition, with no setting
   changes thereafter. The update explicitly requests the new domain and repeats
   the plan constraints in post-call review. Bounded readback requires
   the new hostname, target running/no pending, confidential mode on and debug off.
   A separate credential-free public serving check at the new hostname uses the
   already-pinned Go SDK's V3 appraisal and TLS-key-bound client. See
   [domain migration evidence and breaking-cutover approval](gateway-custom-domain.md).

Superseded candidates may leave old approvals visible; approving them cannot pass
postapproval current-main checks. Checks/preparation rejection is not rollback.
Production concurrency is non-canceling. GitHub's head comparison and Tinfoil's
update are not one transaction: a main push **after admission** does not cancel
an already-started update. GitHub's pending-job ordering/replacement does not
promise a release or approval for every intermediate main commit.

## no-rollout-needed

The user or agent explicitly applies `no-rollout-needed` after reviewing rollout
impact under [repository policy](../AGENTS.md#pr-rollout-labels). No workflow adds
labels and no code classifies changed files. Required PR/main CI is unchanged.

After existing exact-main checks, the trusted source gate reads the PR associated
with the **originating check source**, not the latest PR or implementation SHA.
If exactly one associated PR is merged into this repository's main, its
`merge_commit_sha` equals the source and it has the label, automatic A/B image
publication and downstream deployment are skipped (`RELEASE_ROLLOUT_LABELED`).
Absent labels or missing/ambiguous/mismatched linkage keep normal publication
(`RELEASE_ROLLOUT_NORMAL`). An unreadable or malformed API response stops
admission with `RELEASE_ROLLOUT_LOOKUP`; check GitHub read permissions/availability
and rerun source admission or explicitly dispatch current-main publication.
Raw API/exception text is never printed.

The gate runs from trusted `github.sha`, never PR code/artifacts. Manual
`workflow_dispatch` on current main ignores the label (`RELEASE_ROLLOUT_MANUAL`)
and retains all existing activation, qualification, provenance and deployment
safeguards. Labels are read at admission; changing them cannot cancel work
already admitted. There is no diff, path, test-only or publication-backlog check:
the user/agent owns whether skipping this main arrival leaves necessary gateway
changes unpublished. A label does not prove that the deployed gateway matches
main. Offline and hosted evidence belongs in
[label verification](no-rollout-needed-verification.md).

## Separately authorized setup and qualification

Do not activate these workflows merely because local tests passed. Before setup:

- Review/merge the implementation only under a separate instruction; no automatic
  merge or deployment is authorized by this document.
- Create the `production` environment deliberately. Required reviewer: only user
  `ajbt200128`, self-review allowed so that account can initiate and approve. Use
  custom branch restrictions permitting only branch `main`, no tags/wildcards.
- Disable administrator bypass in GitHub settings where supported. The documented
  REST environment schema does **not** expose that setting; the script does not
  manufacture a readback claim. Instead, admission requires an authenticated
  `approved` review by the required user for this exact run and environment.
  Entering a job by bypass without that review cannot authorize its update.
- Provision `TINFOIL_PRODUCTION_ADMIN_KEY` only as an environment secret using
  private secret entry. Never copy credentials from local CLI config or chat.
  Correct environment placement is an operator setup responsibility; the gate
  does not inspect secret-name metadata or prove the absence of a same-named
  repository/organization fallback. Missing actual credentials still block update.
- Qualify native `GITHUB_TOKEN` read access to environment policy, branch
  restrictions, public provenance and deployment review history. No supplemental
  reader/PAT is required. Unreadable required evidence still fails closed.
- Activation uses GitHub-provided variable snapshots, not live variable API reads.
  Disabling a switch is **not guaranteed to revoke a queued/approved run**; do not
  treat it as cancellation. Actual operator review and current main are still
  checked after approval and immediately before the single update.
- Exercise the actual hosted graph with controlled public/synthetic candidates:
  successful CI, reversed completion of two main arrivals, stale queued/approved
  candidates, tag/run/artifact binding, version/partial-publication rejection,
  actual Environment approval and non-canceling concurrency behavior.
- Independently qualify the new SDK serving command on the production platform
  without login, catalog or inference. Synthetic tests qualify local control
  flow, not hardware appraisal or a deployed connection. A serving failure after
  update means **deployment occurred; serving acceptance failed**.
- Only after qualification and separate authorization set the activation variables
  to `true`. Release publication and deployment have independent switches. A
  switch change is not an atomic revocation of an already-admitted operation.

Native CLI 0.19.0's historical V3 signer mismatch and Pi's legacy-V2 freshness
limitations remain unresolved. The SDK command is a different, supported path;
no native CLI V3 pass, Pi approval refresh or Pi migration is claimed.

## Approving with gh on the operator's behalf

The assistant may approve **only after the user explicitly asks to deploy**. No
workflow contains an approval request, impersonation token or automatic approval.
Before using the operator's authenticated `gh` identity:

1. Independently verify S/R/tag/image/manifest from the public release and require
   S equals current main. Verify the release-only parent/tree relationship.
2. Select the exact successful production preflight/run, not the newest run by
   ordering alone. Require repository/workflow/ref identity, run attempt **1**,
   and title `Production <tag> from <source>`. Preflight validates all dispatched
   pins; protected admission repeats their verification.
3. Require the pending environment ID matches the deliberately configured
   production environment and the account is eligible to approve. Inspect API
   responses transiently; do not print/store arbitrary comments or metadata.
4. Recheck current main immediately before approval. If stale, do not approve;
   identify the newest eligible candidate instead.
5. Submit approval for that exact run/environment with a content-free comment.

Read-only identity/head commands:

`gh api user --jq .login`

`gh api repos/ajbt200128/possums/git/ref/heads/main --jq .object.sha`

Public pin verification from this checkout:

`python3 scripts/release.py verify "$SOURCE" "$RELEASE" "$TAG" "$IMAGE"`

After the checks above and explicit user authorization, approve only the verified
integer run/environment IDs:

`gh api --method POST "repos/ajbt200128/possums/actions/runs/$RUN_ID/pending_deployments" -f state=approved -f comment='Operator-authorized deployment of verified current-main candidate' -F "environment_ids[]=$ENVIRONMENT_ID"`

The protected workflow still checks main and actual review evidence after this
approval. Approval alone does not establish that deployment or serving succeeded.

## Read-only planning client headers

The Python planning request sends fixed `Accept: application/json` and accurately identifies itself as `possums-release-verifier/1`, instead of urllib's default user agent. The [pinned CLI client](https://github.com/tinfoilsh/tinfoil-cli/blob/v0.19.0/controlplane.go) also sets JSON acceptance and an explicit client identity; the [documented Admin API](https://docs.tinfoil.sh/admin/admin-api) confirms the same read-only plan route and update action permission.

Credential-free, read-only investigation reproduced HTTP 403 with the default Python user agent and expected unauthenticated HTTP 401 with explicit release-client or pinned CLI headers, including a synthetic-instance planning request. Only status/content-type categories were inspected; response bodies and credentials were not collected. This establishes a header-dependent rejection, not a guaranteed authenticated deployment fix or its server-side root cause. Do not rotate credentials, broaden access or weaken planning/provenance checks based on that observation.

Offline release tests assert the fixed method, headers, body, timeout and redirect/proxy restrictions, and that an observed planning HTTP refusal never invokes the update or final admission. The first v0.0.39 attempt stopped at `RELEASE_PLAN_HTTP` with HTTP 403 before the update invocation. A fresh attempt requires separate authorization; the operator authorized this header correction and one fresh protected attempt. A new approval still requires verified current-main release pins; no mutating workflow rerun or uncertain update replay is permitted.

## Interrupted or uncertain operations

- No automatic inference, mutation retry, rollback or downtime confirmation.
  Closed stdin and no `--yes` leave the official CLI's pre-mutation plan checks
  intact. The Admin API plan is checked before mutation; the CLI's plan is also
  inspected **post-call**. Neither is a transactional reservation, independent
  pre-mutation human plan review, or proof of uninterrupted availability.
- Mutating workflow reruns are refused (`run_attempt` must be 1). A new run is not
  a safe retry merely because it has a new ID. On an uncertain update, stop
  approvals, inspect production read-only and reconcile privately before any
  separately authorized new candidate. A pending update blocks new admission.
- A partial/failed tagged publisher is not dispatched again automatically. Never
  delete/repoint its tag or silently replace assets. If an existing publication
  completed, verify its exact immutable identity before reuse.
- Running state, public signed-release provenance and serving endpoint binding
  are distinct results. No request was sent to manufacture a billing or runtime
  privacy result; no paid canary is included.

## Local checks and privacy

`python3 -m unittest discover -s tests -p 'test_release*.py'`

Use the existing pinned `attestation-go` package to test the standalone command
from the helper module; it does not add a binary or dependencies to the gateway:

`nix build .#attestation-go --out-link /tmp/possums-serving-go`

`GOTOOLCHAIN=local /tmp/possums-serving-go/bin/go -C attestation-helper test -mod=readonly ../scripts/verify-serving/main.go ../scripts/verify-serving/main_test.go`

Policy: [data boundaries](../PRIVACY.md#data-handling-boundaries),
[forbidden content/identifiers/raw errors](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[processors](../PRIVACY.md#processors-retention-access-and-shutdown),
[synthetic testing](../PRIVACY.md#synthetic-testing-exception) and
[required evidence](../PRIVACY.md#required-evidence-for-every-telemetry-change).

New subprocess output is bounded while collected, held in memory and never
printed verbatim. Closed diagnostics preserve known stage/constraint/status and
unknown outcomes. Only validated public release pins go to workflow outputs.
Deployment configuration stays opaque and memory-only; serving verification has
no admin/GitHub credentials, prompt body or cookies. GitHub retains native workflow
history and public release artifacts; this is **not** whole-process transience or
an absence-of-platform-logs claim. No telemetry family, destination, retention or
runtime privacy policy changed. See scoped [verification](verification.md).
