# Automatic paid releases and approved production deployment

**Automatic publication qualified; protected deployment pending reader setup.**
The implementation is merged and the operator authorized environment setup and
qualification. Automatic `v0.0.22` passed; production preflight failed before
approval/update and deployment was disabled again. Activation remains off by
code default; current settings and scoped evidence are recorded chronologically
in [hosted qualification](approved-main-release-qualification.md). Historical
[v0.0.20 rollout evidence](timeout-renewal-v20-rollout.md) is not qualification of
these new workflows.

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
   secret references. Bounded readback requires the target running/no pending,
   confidential mode on and debug off. A separate credential-free public serving
   check uses the already-pinned Go SDK's V3 appraisal and TLS-key-bound client.

Superseded candidates may leave old approvals visible; approving them cannot pass
postapproval current-main checks. Checks/preparation rejection is not rollback.
Production concurrency is non-canceling. GitHub's head comparison and Tinfoil's
update are not one transaction: a main push **after admission** does not cancel
an already-started update. GitHub's pending-job ordering/replacement does not
promise a release or approval for every intermediate main commit.

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
  Preflight requires readable environment-secret **name metadata** to reject an
  accidental repository/organization fallback; it never reads a secret value.
- Qualify native `GITHUB_TOKEN` access to environment policy, branch restrictions,
  provenance and deployment review history. Repository-variable and environment
  secret-name metadata reads can use a supplemental `PRODUCTION_GATE_READ_TOKEN`
  **repository secret**: a fine-grained token limited to this repository with only
  **Variables: read** and **Environments: read**. These permissions are documented
  in [GitHub's permission table](https://docs.github.com/en/rest/authentication/permissions-required-for-fine-grained-personal-access-tokens).
  The native token remains authoritative for all other API reads. The reader
  cannot approve/edit deployments and never retrieves a secret value. It must
  be available before environment entry; do not put it only in `production`.
  Use short expiration and renew privately before expiry; no automatic renewal
  is implemented. Never copy the operator's broad OAuth login token. Missing or
  unreadable metadata fails closed; do not weaken the checks.
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

## Interrupted or uncertain operations

- No automatic inference, mutation retry, rollback or downtime confirmation.
  Closed stdin and no `--yes` leave the official CLI's pre-mutation plan checks
  intact. Our detailed plan inspection is **post-call**, not independent
  pre-mutation human plan review or proof of uninterrupted availability.
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
