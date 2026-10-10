# Paid blue/green preparation — scoped verification

## Scope and authorization

Branch `feat/blue-green-deployments`, based on local main including free-endpoint reverts `23792a1` / `1358856`. Paid-only offline preparation. No production deployment/update/promote/cancel/stop, secrets/settings/DNS mutation, inference, merge, installation or release publication occurred in this packet. No live platform status was acquired; live strategy, readiness, endpoint retention and secret binding behavior remain unknown. Independent timeout/client-lifecycle and diagnostics efforts were not edited.

Policy references: [data boundaries](../PRIVACY.md#data-handling-boundaries), [forbidden exports](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data), [processor/shutdown boundary](../PRIVACY.md#processors-retention-access-and-shutdown), [required evidence](../PRIVACY.md#required-evidence-for-every-telemetry-change). This packet adds no telemetry. Local synthetic sentinel tests below qualify only the new offline tool's output privacy; previous telemetry source/wire evidence remains in [the MVP record](telemetry-mvp-five-minute.md), not a new whole-runtime privacy pass.

## Actual source observations

- `tinfoil-config.yml`: CPU 2, memory 8192, CVM 0.14.12, read-only gateway, tmpfs, three secret references, no persistent volume declaration. No checked-in update strategy and no readback of effective platform strategy.
- `src/main.rs::run` / `shutdown_signal`: SIGTERM selects out of serving, disposes telemetry, returns. No generation-owner join or accounting-drain rendezvous. `src/generation.rs` detaches charged preflight and transfers its sole owner independently of delivery; zero sockets is not terminal accounting.
- `src/server.rs::AppState::new` initializes a fresh `Accounting` from `Auth::account_budgets`; memory-only state cannot safely be described as stateless or credit-preserving across overlap/restart. No safe overlap accounting was implemented.
- `clients/pi/bootstrap.ts::connectPublished` obtains latest at fixed production endpoints. `examples/phase01/approval.ts::qualifyApproved` binds bundle domain to the fixed policy origin; `transport.ts::Channel.published` constructs the production channel. Current code is not an explicit-tag, review-port public-only verifier. No verifier/client trust change was made.
- `.github/workflows/tinfoil-release-publish.yml` pins measurement action `f2ec2fdf4510459730f0a5158c5817f5361e9461` but supplies no explicit latest control. No publication was run. A public raw `action.yml` lookup at that revision returned not found; actual publication/latest semantics are unresolved, not a proven safe staging path.

Existing native CLI v3 platform-signer mismatch and approved JS legacy-v2 freshness limitations remain unresolved. The gateway Go v3 helper's historical public-evidence pass is not a candidate review pass. Historical downtime/demo-reset acceptance and public same-release Pi `/new` evidence are not blue/green session preservation evidence.

## Public platform contract research (not runtime evidence)

Bounded parallel research inspected public docs and CLI/Admin API source. Parent independently fetched the [CLI source at v0.19.2 commit `d227ddb6f398e5cd2071cb7e87815e156d84e157`](https://github.com/tinfoilsh/tinfoil-cli/blob/d227ddb6f398e5cd2071cb7e87815e156d84e157/container.go), [Admin API](https://docs.tinfoil.sh/admin/admin-api.md) and [updates contract](https://docs.tinfoil.sh/containers/updates.md). CLI flags confirm explicit tag/hold and `mark-latest=false`; promote posts no body/candidate precondition. API documentation establishes the non-deploying update-plan POST, target-strategy override, unavailable hold on replacement, readiness states, review URL/repository/tag and latest default true. These are supported contract names, not tested control-plane behavior. The [runbook](blue-green-deployments.md#supported-public-contracts-not-live-qualification) records exact shapes and fail-closed checks without executing them.

Research also inspected public CVM shim/PID1 revision `be74c3395921a9b1fe2347dd2af47cc496fa354c`, which contains graceful-drain code. No mapping of that source to the deployed CVM was qualified. Published contracts do not establish drain duration, old endpoint retention, forced-stop deadline or effective `stop_timeout`; gateway SIGTERM still lacks owner waiting. No local CLI version was installed/upgraded or adopted for native v3 verification. No live admin credential or platform metadata was read.

## Local synthetic qualification

Tool: `scripts/plan-held-update.py`, commit `431ba31`. Standard-library-only stdin checklist lint; no network, CLI execution, crypto, platform schema adapter or approval. **Even consistent input always emits `promote_allowed: false`.** Expected/observed fingerprints are manually supplied comparisons, not authenticated evidence. Readiness/strategy assertions are untrusted local inputs, never permission to perform a mutation.

`python3 scripts/test-plan-held-update.py` — seven tests passed. Includes subcases for wrong explicit tag/manifest/measurement/key; replacement/unknown strategy; absent/malformed review readiness; latest advanced before promotion; duplicate/missing secret references and rejected values; unknown fields; duplicate JSON keys; malformed/oversized/hostile input. Subprocess cases assert exact content-free rejection output and empty stderr, including a synthetic secret sentinel. A consistent plan preserves reference names and still blocks promotion. These are lint/privacy tests, **not cryptographic wrong-measurement/key tests against real hardware**.

Independent bounded read-only review found no concrete defect in the planner, tests or initial runbook and independently ran all seven tests successfully. The later public-contract section was parent-checked against the fetched CLI/API excerpts, not separately agent-reviewed. This review did not qualify platform behavior or cryptography.

Active LSP checks on both changed Python paths: two files clean, zero error diagnostics. `git diff --check` passed. No gateway/Pi source changed, no full runtime test suite run, and no real artifact/candidate/hardware key qualification performed.

## Release decision

Safe now: offline checklist lint and [gated runbook](blue-green-deployments.md). Unsafe/unqualified: paid overlapping update or promotion, zero-session-loss claims, blind native CLI verification, latest advancement during review, authenticated candidate probes, inferred refunds or restored balances.

Required next decisions: platform retention/drain contract and isolated qualification; public-only review-origin/tag verifier; full-session endpoint/key affinity; explicit accounting/admission treatment or newly authorized downtime/reset fallback; non-latest artifact publication capability and controlled discovery transition. No approval is presumed. A timeout fix alone resolves none of these deployment gates.
