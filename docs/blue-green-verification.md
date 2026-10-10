# Paid blue/green preparation — scoped verification

## Initial preparation scope and authorization (historical)

Branch `feat/blue-green-deployments`, based on local main including free-endpoint reverts `23792a1` / `1358856`. Paid-only offline preparation. No production deployment/update/promote/cancel/stop, secrets/settings/DNS mutation, inference, merge, installation or release publication occurred in this packet. No live platform status was acquired; live strategy, readiness, endpoint retention and secret binding behavior remain unknown. Independent timeout/client-lifecycle and diagnostics efforts were not edited.

Policy references: [data boundaries](../PRIVACY.md#data-handling-boundaries), [forbidden exports](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data), [processor/shutdown boundary](../PRIVACY.md#processors-retention-access-and-shutdown), [required evidence](../PRIVACY.md#required-evidence-for-every-telemetry-change). This packet adds no telemetry. Local synthetic sentinel tests below qualify only the new offline tool's output privacy; previous telemetry source/wire evidence remains in [the MVP record](telemetry-mvp-five-minute.md), not a new whole-runtime privacy pass.

## Preparation-time source observations (historical)

The following observations describe the initial preparation packet, before the separately authorized local runtime implementation recorded below; they are not current shutdown-code claims.

- `tinfoil-config.yml`: CPU 2, memory 8192, CVM 0.14.12, read-only gateway, tmpfs, three secret references, no persistent volume declaration. No checked-in update strategy and no readback of effective platform strategy.
- `src/main.rs::run` / `shutdown_signal`: SIGTERM selects out of serving, disposes telemetry, returns. No generation-owner join or accounting-drain rendezvous. `src/generation.rs` detaches charged preflight and transfers its sole owner independently of delivery; zero sockets is not terminal accounting.
- `src/server.rs::AppState::new` initializes a fresh `Accounting` from `Auth::account_budgets`; memory-only state cannot safely be described as stateless or credit-preserving across overlap/restart. No safe overlap accounting was implemented.
- `clients/pi/bootstrap.ts::connectPublished` obtains latest at fixed production endpoints. `examples/phase01/approval.ts::qualifyApproved` binds bundle domain to the fixed policy origin; `transport.ts::Channel.published` constructs the production channel. Current code is not an explicit-tag, review-port public-only verifier. No verifier/client trust change was made.
- `.github/workflows/tinfoil-release-publish.yml` pins measurement action `f2ec2fdf4510459730f0a5158c5817f5361e9461` but supplies no explicit latest control. No publication was run. A public raw `action.yml` lookup at that revision returned not found; actual publication/latest semantics are unresolved, not a proven safe staging path.

Existing native CLI v3 platform-signer mismatch and approved JS legacy-v2 freshness limitations remain unresolved. The gateway Go v3 helper's historical public-evidence pass is not a candidate review pass. Historical downtime/demo-reset acceptance and public same-release Pi `/new` evidence are not blue/green session preservation evidence.

## Public platform contract research (not runtime evidence)

Bounded parallel research inspected public docs and CLI/Admin API source. Parent independently fetched the [CLI source at v0.19.2 commit `d227ddb6f398e5cd2071cb7e87815e156d84e157`](https://github.com/tinfoilsh/tinfoil-cli/blob/d227ddb6f398e5cd2071cb7e87815e156d84e157/container.go), [Admin API](https://docs.tinfoil.sh/admin/admin-api.md) and [updates contract](https://docs.tinfoil.sh/containers/updates.md). CLI flags confirm explicit tag/hold and `mark-latest=false`; promote posts no body/candidate precondition. API documentation establishes the non-deploying update-plan POST, target-strategy override, unavailable hold on replacement, readiness states, review URL/repository/tag and latest default true. These are supported contract names, not tested control-plane behavior. The [runbook](blue-green-deployments.md#supported-public-contracts-not-live-qualification) records exact shapes and fail-closed checks without executing them.

Research also inspected public CVM shim/PID1 revision `be74c3395921a9b1fe2347dd2af47cc496fa354c`, which contains graceful-drain code. No mapping of that source to the deployed CVM was qualified. Published contracts do not establish drain duration, old endpoint retention, forced-stop deadline or effective `stop_timeout`; at preparation time, gateway SIGTERM lacked owner waiting. No local CLI version was installed/upgraded or adopted for native v3 verification. No live admin credential or platform metadata was read.

## Local synthetic qualification

Tool: `scripts/plan-held-update.py`, commit `431ba31`. Standard-library-only stdin checklist lint; no network, CLI execution, crypto, platform schema adapter or approval. **Even consistent input always emits `promote_allowed: false`.** Expected/observed fingerprints are manually supplied comparisons, not authenticated evidence. Readiness/strategy assertions are untrusted local inputs, never permission to perform a mutation.

`python3 scripts/test-plan-held-update.py` — seven tests passed. Includes subcases for wrong explicit tag/manifest/measurement/key; replacement/unknown strategy; absent/malformed review readiness; latest advanced before promotion; duplicate/missing secret references and rejected values; unknown fields; duplicate JSON keys; malformed/oversized/hostile input. Subprocess cases assert exact content-free rejection output and empty stderr, including a synthetic secret sentinel. A consistent plan preserves reference names and still blocks promotion. These are lint/privacy tests, **not cryptographic wrong-measurement/key tests against real hardware**.

Independent bounded read-only review found no concrete defect in the planner, tests or initial runbook and independently ran all seven tests successfully. The later public-contract section was parent-checked against the fetched CLI/API excerpts, not separately agent-reviewed. This review did not qualify platform behavior or cryptography.

Active LSP checks on both changed Python paths: two files clean, zero error diagnostics. `git diff --check` passed. No gateway/Pi source changed, no full runtime test suite run, and no real artifact/candidate/hardware key qualification performed.

## Authorized local runtime implementation — accepted local scope

Local implementation was subsequently authorized on base `ca24b869`, without merge, deployment, installation, platform mutation, release publication or live inference authorization. The original preparation-only test and source statements above remain historical.

- `db48720`: process-local lifecycle fence/count and cleanup-drain primitive.
- `04cdf0a`: admitted handler, whole-preflight and original accounting-owner tracking; actual terminal-accounting failures latch before owner retirement.
- `6f6ce2e`: retained voluntary-shutdown connections and independent owner drain; credential-free readiness route and fixed-loopback binary probe. Root health configuration and image definition remain unchanged.
- Sol reported 187/187 library tests, offline integration tests, strict Clippy and package formatting passing. Parent confirmed the commit/clean worktree and read main/server/health implementation.
- T3 Astra actual-diff review R1 independently passed 187/187 library tests and 46/46 focused API auth/chat, lifecycle, privacy and transport integration tests, including the early-413 upload-retention regression. Whitespace checks passed. The previously intermittent telemetry handoff test passed these runs; historical intermittency is not resolved.

**R1 decision: revise for missing required local evidence**, not a demonstrated incorrect-drain implementation path. Parent checked the cited tests and confirmed the gaps:

1. Fatal listener failure with accepted ownership held is untested. Dropping a client socket in the detached-worker test does not prove server connection completion; its held preflight also prevents isolating the owner-vs-connection distinction. A deterministic accepted-connection/sticky-shutdown race is missing.
2. The health test has telemetry disabled and counts only generation calls, so it does not establish absence of application observations or all forbidden dependencies. Private probe tests do not exercise the actual binary's early configuration-bypass/output contract.

Sol corrected these gaps in `b57bbb9` (deterministic accept failure/connection checkpoints), `85f839c` (actual built binary mode), and `fbb779c` (enabled, eligible synthetic telemetry with a routed API positive control). Parent inspected the actual correction diffs. **T3 Astra actual-diff R2 approved the local server packet through `fbb779c`**, resolving both R1 objections without a new concrete server defect.

Astra independently ran `cargo test --offline --locked --lib` (189 passed), `--test health_binary` (1 passed), and `--test api_auth --test api_chat --test lifecycle --test privacy --test transport` (46 passed). Strict package Clippy, `cargo fmt -p possums -- --check`, and base-to-head whitespace checks passed. Sol's full offline integration run also passed with three funded live tests ignored; no paid inference was performed. Workspace-wide formatting has an unrelated untouched-vendor baseline failure; package formatting passed and vendor files were not changed.

The health telemetry fixture bypasses warmup only under `#[cfg(test)]`, proves a normal API request records observations, and then checks health leaves application aggregates, dependency calls and checked auth/accounting state unchanged. It does not qualify network connection occupancy, complete-window release or whole-runtime telemetry privacy. Test checkpoints are not production instrumentation or a new orchestration framework.

Client packet `32ae099` added exact closed GET catalog/challenge diagnostics. Parent inspection and T3 Astra client R1 reproduced category loss for **authenticated encrypted** quiescing responses; EHBP supports encrypted non-200 responses, so this was source-client integration rather than SDK incompatibility. Sol corrected status plumbing and EOF-gated classification in `754432d`. Astra R2 confirmed those fixes but reproduced loss of known HTTP status/body constraints for malformed or interrupted balance responses. Sol's `fea263e` preserves those closed observations; parent inspected each actual source/test diff.

**T3 Astra client actual-diff R3 approved the local packet through `fea263e`**, resolving all R1/R2 objections with no new concrete defect. Independently rebuilt current-source cached-dependency bundles passed 638 SDK-backed reference-client checks and 117 native Pi groups, including actual reconciliation presentation of encrypted HTTP 503 malformed-JSON and interrupted-EOF failures. Scratch TypeScript and scoped whitespace checks passed; the scratch source snapshot matched repository source. Checkout LSP lacks the dependency installation, so this is not a clean checkout-LSP claim. No dependency installation occurred.

Only the exact bounded envelope at the qualified response boundary acquires the quiescing category. Encrypted routes retain SDK nonce/context/framing/decryption checks and complete EOF; plaintext errors fail closed, incomplete/malformed bodies do not establish quiescing or refunds, and known safe status/constraints survive. The existing native retry policy and authenticated refund/receipt rules are unchanged. Diagnostics initiate no additional requests, inference or recovery. Server `billing=not_submitted` does not establish an earlier request's outcome or authorize replay. Deployed shim behavior remains unknown.

Parent reran the seven offline planner tests and base-to-head whitespace checks. Final local acceptance covers implemented A–D and these closed client diagnostics, **not conditional recovery E or deployment**. No merge, publication, installation, platform mutation, image healthcheck activation or live inference occurred. The final runbook distinguishes accepted local source/binary evidence from remaining generated-image, public-candidate-verifier, platform retention, overlapping accounting and discovery-ordering gates.

[Pi permission qualification](blue-green-pi-qualification.md) passed 111/111 offline groups and independently confirmed native/nested ambiguity at the tested boundary. Recovery E remains disabled; the existing expiry-permission limitation was documented, not silently repaired. These tests are not live billing, runtime attestation, generated-image or platform qualification. Successful local drain covers tracked envelopes and accounting, not all Hyper-owned upload buffers, delivery, preserved sessions or balances.

## Release decision

Safe now: offline checklist lint and [gated runbook](blue-green-deployments.md). Unsafe/unqualified: paid overlapping update or promotion, zero-session-loss claims, blind native CLI verification, latest advancement during review, authenticated candidate probes, inferred refunds or restored balances.

Required next decisions: platform retention/drain contract and isolated qualification; public-only review-origin/tag verifier; full-session endpoint/key affinity; explicit accounting/admission treatment or newly authorized downtime/reset fallback; non-latest artifact publication capability and controlled discovery transition. No approval is presumed. A timeout fix alone resolves none of these deployment gates.
