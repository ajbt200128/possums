# no-rollout-needed — source verification

Candidate base: `854aa4a078049242d2750c79d2e098164d562f04`.
The operator explicitly replaced the initial guarded-path proposal with a
**discretionary label**: review policy lives in [AGENTS.md](../AGENTS.md#pr-rollout-labels),
not a code-enforced allow/deny list. No pinned test exceptions, diff inspection,
publication-baseline lookups or automatic labeling remain.

## Offline evidence

`python3 -m unittest discover -s tests -p 'test_release*.py'`

**53 tests passed**, including eight label/source integration tests that invoke
actual `select_source()` with synthetic GitHub evidence. They cover labeled and
unlabeled PRs, exact originating-source binding (not implementation SHA), missing/
ambiguous/unmerged/wrong-source/wrong-repository linkage, manual label bypass,
API failures/malformed hostile evidence, existing event/check rejection before
label lookup, stale main after lookup, and explicit build/publish workflow gates.
The synthetic API rejects unexpected endpoints and Git calls are prohibited:
there is no residual file classifier or publication-history sweep.

`python3 -m unittest discover -s tests -p 'test_image_health.py'`

**8 tests passed**. `git diff --check` passed. The original, more complex candidate
passed hosted PR check run [38298749909](https://github.com/ajbt200128/possums/actions/runs/38298749909);
that is not hosted qualification of this subsequent simplification.
The existing `workflow_run` auxiliary scanner advisory remains: the source job
keeps its trusted `github.sha` checkout and consumes no PR artifacts/code.

## Hosted qualification (2026-10-12)

The simplified feature [PR #63](https://github.com/ajbt200128/possums/pull/63)
merged as `9401de51bf21dd88da6ce5e1b659766ce9ed848f` after successful updated
[PR CI 38299992345](https://github.com/ajbt200128/possums/actions/runs/38299992345).
Independent read-only review of the simplification found no actionable issues.

Labeled [PR #62](https://github.com/ajbt200128/possums/pull/62) then merged as
`c26772f72ff6bc89b456c567a5d4035e33e83f69`. It superseded the feature's main check
[38300418454](https://github.com/ajbt200128/possums/actions/runs/38300418454), which
was canceled by existing check concurrency. Its downstream publication
[38300586467](https://github.com/ajbt200128/possums/actions/runs/38300586467)
rejected source admission; build/publish were skipped. This is rejection of
unsuccessful source CI, not a successful label-skip test.

For current main `c26772f`, required
[push CI 38300512743](https://github.com/ajbt200128/possums/actions/runs/38300512743)
passed all three jobs. The downstream
[publication gate 38301507760](https://github.com/ajbt200128/possums/actions/runs/38301507760)
succeeded with `RELEASE_ROLLOUT_LABELED`, stage `source-rollout`, `rollout=false`:
`source-checks` passed, `build` and `publish` were skipped. This exercises native
GitHub-token PR reads and the actual post-merge skip graph, including a
client-production change with backend/root test additions under the operator's
discretionary policy. No image/tag/release or production candidate is created
by those skipped jobs, and no deployment was approved for this test.

## Trust, privacy and remaining limits

Only a directly queried, unambiguously associated merged main PR whose
`merge_commit_sha` equals the admitted source can supply the explicit label.
Unknown linkage keeps normal publication; operational lookup failure stops
admission with closed `RELEASE_ROLLOUT_LOOKUP` diagnostics and no workflow outputs.
Manual current-main publication ignores labels and retains existing safeguards.
Required CI, exact-main/provenance and protected deployment approval are unchanged.

The operator/user/agent, **not the gate**, decides rollout impact. A mislabeled
PR can skip gateway changes; the gate does not inspect complete diffs, renamed/
deleted paths, test-only semantics or earlier unpublished changes. Labels are
mutable and read at admission; they cannot cancel already admitted work. This
feature is not evidence that deployment/serving matches main.

Policy references: [change control](../PRIVACY.md#mandatory-reference-and-change-control),
[data boundaries](../PRIVACY.md#data-handling-boundaries),
[forbidden raw errors/content](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data)
and [synthetic testing](../PRIVACY.md#synthetic-testing-exception).
Synthetic hostile evidence cannot escape the closed diagnostic. No request data,
credential inspection, telemetry changes, inference or deployment is included.
GitHub retains native workflow history; no whole-process transience is claimed.

The labeled hosted path is qualified above; unlabeled publication and manual
label bypass are covered by existing release behavior/offline tests, not new
live dispatches in this test. No production deployment is approved for the
feature release. Existing native V3 CLI signer and legacy-V2 freshness
limitations are unchanged.
