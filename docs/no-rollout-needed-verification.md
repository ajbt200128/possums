# no-rollout-needed — local source verification

Scope: implementation on fetched `origin/main` source
`854aa4a078049242d2750c79d2e098164d562f04`. This is offline candidate evidence,
not hosted workflow qualification, installed client evidence or deployment.
No label is created/applied automatically; the user or agent explicitly applies
it according to [usage rules](approved-main-releases.md#no-rollout-needed).

## Implementation and trust boundary

- `scripts/release.py source` keeps originating-event/live-run identity, exact
  successful checks, activation and current-main validation before the label
  decision; main is checked again after lookup. Manual dispatch emits rollout
  enabled without PR/publication lookups.
- `.github/workflows/publish-image.yml` keeps the trusted `github.sha` checkout,
  now with full history and read-only PR permission. Only an explicit admitted
  `rollout=true` starts fresh image builders/publisher. No PR workflow/artifact
  supplies executable gate code. Required `check.yml` is unchanged.
- A previous successful publication run and successful source/A/B/publish jobs
  establish the bounded baseline. Skipped-success runs are not baselines. All
  intervening first-parent commits must map exactly to eligible labeled merged
  PRs. Complete PR-file pagination and no-renames Git diffs cover mixed changes,
  old/new/deleted paths, merge resolutions and earlier changes later reverted.
- Allowlist inspection covered `flake.nix`, `Cargo.toml`, gateway/helper include
  references and release/check workflows. Only the documented Pi client suffixes
  and Markdown qualify. Root tests, scripts and all build/runtime/configuration
  inputs remain ineligible. The allowlist needs review when dependencies change.
- Known ineligibility keeps publication enabled; API/Git/malformed-evidence
  failures stop source admission as `RELEASE_ROLLOUT_LOOKUP`, without admitting
  a skip or publication. Diagnostics contain only locally authored code/stage/
  action and a boolean decision, not API text, filenames or PR metadata.

## Offline evidence

`python3 -m unittest discover -s tests -p 'test_release*.py'`

**63 tests passed**, including 18 new synthetic rollout integration cases that
call the actual `select_source()` with temporary Git histories and mocked GitHub
responses. Coverage includes eligible squash/merge and multiple PRs/commits,
unlabeled/manual behavior, mixed gateway inputs, rename/deletion paths, complete
paginated PR diffs, missing/ambiguous/wrong linkage, skipped/no/non-ancestor
baselines, oversized PR fallback, gateway reverts, stale main after lookup,
malformed/truncated evidence, duplicate baseline jobs, existing event/check
rejection before label lookup and lookup failure at each new API boundary. The
workflow wiring test checks the trusted checkout and explicit build/publish
conditions. Existing release/deployment safety tests also remain passing.

`python3 -m unittest discover -s tests -p 'test_image_health.py'`

**8 tests passed**. `git diff --check` passed. Active LSP checks of the changed
Python files were clean; YAML syntax diagnostics were clean. The auxiliary
zizmor scanner flags the **existing unchanged** `workflow_run` trigger as
`dangerous-triggers`. The implementation retains its trusted checkout and does
not consume triggering PR code/artifacts; this scoped review does not certify
all workflow-trigger risks or suppress that advisory. An independent read-only
Luna review of the gate/workflow and initial 13 rollout tests found no concrete
actionable findings; five further negative/fallback tests were added and run
by the implementing agent.

## Privacy and remaining limits

Applicable policy: [mandatory change control](../PRIVACY.md#mandatory-reference-and-change-control),
[data boundaries](../PRIVACY.md#data-handling-boundaries),
[forbidden raw errors/content/identifiers](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data)
and [synthetic testing](../PRIVACY.md#synthetic-testing-exception).
API evidence and bounded subprocess output remain transient in the existing
release helpers. Synthetic hostile-evidence tests confirm the closed diagnostic
does not expose the sentinel or admit workflow outputs on failure. No telemetry,
credential/private-history access, automation-variable/settings changes, inference
or deployment was performed. GitHub retains native workflow history; no claim of
whole-process transience or absence of platform logs is made.

Hosted `GITHUB_TOKEN` PR-read permission, GitHub's actual job-skip graph, API
pagination/merge metadata behavior and end-to-end label admission still need
separately authorized hosted qualification. Local string-level workflow wiring
tests do not execute GitHub Actions. The 100-run/100-mainline-commit lookup bounds
and GitHub's 3000-file PR limit deliberately keep normal publication when scope
cannot be established; they do not silently truncate an eligible decision.
Rebase/direct pushes without exact linkage for each intervening commit publish
normally. Labels are mutable and read at admission, not frozen at merge; this
feature does not cancel earlier admitted work or guarantee a rollout per arrival.
A publication baseline does not prove deployment or serving health. Existing
native V3 CLI signer and legacy-V2 freshness limitations are unchanged.
