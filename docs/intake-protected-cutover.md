# Protected one-time intake reference cutover

## Authority and limits

The operator requested gateway deployment after local source/package preparation,
then confirmed that the intake references had not been provisioned. The operator
explicitly selected **Prepare protected cutover path**: a reviewed one-time update
combining the new signed image with the exact intake reference replacement. This
is not an instruction to remove the old reference from a still-running old
release, perform an old-image configuration rollout, waive normal gates, rotate
keys, change Tor identity, install/activate clients or send paid inference.
The operator subsequently confirmed that all other gateway configuration writers
will remain idle throughout the cutover, and reported both private secret objects
created with running selections unchanged. Those are operator confirmations, not
agent verification of values, delivery or a control-plane lock.

The [documented Tinfoil update API](https://docs.tinfoil.sh/admin/admin-api#update-container)
replaces saved managed selections as part of an update; omission retains them and
merges target-declared existing secrets. No separate harmless selection-only save
was established. The previous preparation record's assumption of completing a
selection transition before the image update is superseded by this scoped path.
The official pinned CLI v0.19.0 supports repeated `--secret` name selections and a
read-only plan. Stable blue/green plans do not require `--yes`; changed plans or
downtime still require fresh consent and closed stdin refuses it. A read-only
preplan is not a reservation, nor an atomic compare-and-swap with execution.

## Exact opt-in, not a relaxed default

`deploy-production.yml` adds the explicit `reference_transition` choice:

- `none` (default): existing automatic dispatch and ordinary no-drift behavior.
  Both new intake references must already be selected and legacy authority absent.
- `intake`: manually selected one-time cutover; the run title ends with
  `[intake cutover]`. Preflight and protected entry require workflow `GITHUB_SHA`
  equal to tested source S, in addition to existing current-main checks.

Both entries verify the same exact S/R/tag/image/manifest provenance, existing
production policy, invocation attempt 1 and serving-tool qualification. Protected
entry and immediate final admission retain authenticated operator approval and
current-main checks. Source/release relationship, A/B builds, immutable signature,
protected environment, noncanceling production concurrency and no replay remain.
The automatic publisher does not select this exception.

The signed target must declare exactly the reviewed six service/intake/Tor names.
Current production must be stable, confidential, nondebug and nonheld at the
canonical gateway domain, with no pending update. It must retain both approved Tor
identity selections and existing inference/telemetry references, select
`POSSUMS_ACCOUNTS_JSON`, and select **neither** new intake reference. Mixed,
partially transitioned or already-cut-over states are refused before planning.

## Required exclusive configuration custody

Review R1 found that full-list replacement can remove an unrelated selection added
after the wrapper preplan but before the CLI's first plan. Two matching CLI plans
compare only to each other; post-call rejection is too late to prevent that removal.
It also found that name re-resolution can target a different instance. The revised
path binds execution and every subsequent read to the captured, validated instance
ID. A fresh read of that ID after wrapper planning must match the captured source,
reference set and all unrelated stable fields before final policy/main admission;
already-stale snapshots are refused without invoking the update.

The remaining read-to-execution race is **not** closed by that fresh read. The
supported API has no documented state-bound mutation, so the path requires actual
exclusive operator mutation custody. `exclusive_configuration_custody` is a
separate per-run boolean, default false. Opt-in preflight, protected entry and the
mutation primitive refuse without its exact `true` assertion. Do not assert it
unless all other console/API/project-update writers are controlled and idle from
protected approval until completion or private reconciliation of an unknown result.
GitHub concurrency serializes these workflow updates only; it does not lock the
console, other API keys or project automation. This is an operational single-writer
condition, not a server-enforced lock, CAS or permission to overwrite concurrent
changes. If exclusive custody cannot be maintained, do not use this path. An
uncoordinated writer after the last read remains a residual risk, not a property
proved by synthetic tests or the assertion. Normal updates do not require or
inherit this exceptional full-list replacement.

## Exact reference replacement and observation

Only this reference delta is accepted:

- add `POSSUMS_INTAKE_ORIGIN` and `POSSUMS_INTAKE_LEDGER_KEY`;
- remove the `POSSUMS_ACCOUNTS_JSON` **selection**, not its underlying secret object;
- change no other names, values, settings, variables, SSH selections or resources.

Under the exclusive-custody condition, every other existing selected name is
retained. The full sorted target name list
is sent identically to the read-only API plan and repeated CLI `--secret` flags.
Each API/CLI plan must identify the same instance/name, source/target tags and
unchanged resource shape, and report exactly the allowlisted additions/removal.
Unknown/duplicate/missing changes, hold, replacement/downtime and unrelated drift
are rejected in each reviewed plan. A preplan refusal sends no update; a post-call
refusal can occur after mutation and does not establish rollback or authorize a
retry. Optional refresh metadata may name only unique target selections;
this does not establish the secret values' correctness or preservation.

During bounded readback, either the complete old or complete target name set is
accepted, never a mixed/extra/missing set. Every unrelated opaque stable field is
compared to its captured value. Success requires target running/no pending,
canonical domain and the exact target selection. The result explicitly does not
claim all opaque configuration was unchanged; it records the approved reference
transition and preservation of unrelated captured configuration. Public serving
appraisal remains a separate later result. No raw control-plane/CLI output or
secret values are printed or retained. Uncertain calls/post-call failures never
trigger mutation retry, rollback, secret cleanup or inference.

## Private provisioning and operational sequence

1. Pass reviewed PR CI, including the real Linux/amd64 gateway/fixture archives,
   NSS/SDK rootfs, packaged health and Tor-image checks. Do not use a
   `no-rollout-needed` label. Merge only the reviewed candidate.
2. Privately create the two correctly scoped gateway-repository secret objects in
   Tinfoil. The origin is the intended HTTPS intake origin; the ledger key must
   match intake's existing accounting bearer, not a customer key/native auth
   token. Never paste values into chat, PRs, commands, logs or support artifacts.
   Do not rotate intake keys, touch its disk/customer state or delete the old
   secret. **Provision objects only: do not deploy, save changed instance
   selections or remove the running legacy selection here.** If the existing
   bearer cannot be obtained through approved private custody, stop.
3. Require current-main CI, independent A/B image agreement and exact signed public
   release evidence. Verify S/R/tag/image/manifest and release-only parent/tree.
   No scratch package/source hashes substitute for release provenance.
4. An automatic ordinary production request may be pending. Do not approve it for
   first cutover. Before creating the explicit request, cancel only an identified
   unapproved/unstarted protected job after verifying no update was admitted; do
   not interrupt or replay a possibly admitted operation.
5. Dispatch `deploy-production.yml` on current main with the verified pins and
   `reference_transition=intake` and `exclusive_configuration_custody=true` only
   under the exclusive-writer commitment above. Check exact run/workflow/ref/attempt/title
   and successful preflight. Verify private provisioning independently; a source name,
   planning response or public health result does not prove correct credentials.
6. Recheck current main and approve only that exact eligible protected run as the
   operator. Begin and maintain the exclusive configuration window; use no other
   console/API/project writer until completion or reconciliation. It admits at most
   one signed-image/reference update. On refusal or
   uncertainty, report known stage/outcome and reconcile privately; a new run is
   not automatically a safe retry. The cutover mode refuses an already-transitioned
   state. Future ordinary updates use `reference_transition=none`.
7. Record control-plane completion, public signed provenance and credential-free
   bound serving appraisal separately. No client activation, authenticated live
   intake acceptance or paid inference follows from those observations.

## Offline verification and privacy

`tests/test_intake_cutover.py` exercises exact named selection, before/after plan
identity, ordinary-path refusal, permanent second-cutover refusal, malformed and
mixed states, config/source binding, hostile extra changes, bounded preplan HTTP
failures and cleanup, plan-before-admission/mutation, all opaque-field drift and
uncertain no-replay behavior using synthetic mocks only. R1 regressions require
custody before I/O, reject a selection added before the final captured-ID refresh
without calling the mutation, and prohibit name re-resolution after initial lookup.
These tests do not prove exclusivity against an external writer after that refresh. Required CI runs it
explicitly. No negative test contacts production or initializes a real provider.

Initial local verification ran 141 Python tests without failures (three optional
graph skips), including 22 cutover tests. After the R1 custody/ID corrections, all
30 cutover tests passed and the full Python suite ran 149 tests without failures,
with the same three optional skips. Eight held-plan tests also passed; active LSP
checked the changed Python and workflow paths without findings. A fresh optional local
Linux graph probe timed out at 300 seconds, so no new local graph pass is claimed.
Published pre-cutover source `9a2a898` passed all actual Linux flake/image/startup
jobs in run `39672832320`, but fresh exact-head CI for this cutover addition remains
required. Neither that earlier CI result nor the mocks establish live provisioning
or deployment. Fresh read-only R2 accepted the source conditionally under actual
operator single-writer custody, with no blocking finding. Its nonblocking test-count
record note is addressed by distinguishing the initial and revised checkpoints
above; the reviewer did not independently run checks or clear deployment gates.

Policy: [data boundaries](../PRIVACY.md#data-handling-boundaries),
[mandatory typed failures](../PRIVACY.md#mandatory-typed-interface-failures),
[forbidden telemetry](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
and [synthetic isolation](../PRIVACY.md#synthetic-testing-exception). This adds no
telemetry, logs/traces, support export or persistence of detailed control-plane
responses. Native GitHub workflow history and the Tinfoil control plane retain
their existing boundaries. Secret-name equality cannot prove secret-value
correctness; hardware/live-paired/crash properties remain unqualified by mocks.
