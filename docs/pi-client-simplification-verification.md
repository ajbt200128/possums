# Pi client simplification — scoped offline verification

Source candidate in `fix-deepseek-generation-http-retries`, based on `8123b3323da6a12dfe568f98f994b5ce7c110ae0`. **Both unfiltered offline suites pass.** This is source/offline acceptance evidence, not installed, deployed or live qualification. Parent source review of the ownership fix and an independent unfiltered Pi 1.0.4 rerun passed (158 groups). The operator explicitly accepted the narrow native intentional-cancellation presentation exception below; source/offline acceptance is complete. No installation, deployment, commit, live credentials, live inference or gateway/Rust changes. Policy: [client failure policy](pi-client-failure-policy.md).

## Final results

Node **24.13.0**, cached dependencies, scratch `/private/tmp/possums-pi-build-client-simplification-HrkWFN`. No skips or filters: the Pi suite's optional focus filter was removed.

Both actual cached Pi **1.0.4 and 1.1.0** completed full `check.mjs`, exit **0**, with:

- Shared client **38** checks; Phase 02 client **682**.
- Shared cumulative stream regression **65,541 events/frames and 76,809,934 encrypted bytes**; forwarding/accounting passed.
- Release policy **177**; public acquisition cache **39**.
- Pi/diagnostic suite **158 groups** (including **9 diagnostic groups**).
- Actual native SDK/subprocess RPC discovery **28 checks**, zero synthetic inference dispatch and zero public-network calls in those discovery fixtures.
- Final shared admission **70 checks**; rejected requests caused zero encryption, network or getter calls.

Strict TypeScript `--noEmit` passed separately with each SDK's actual peer links, exit **0**. A fresh **production-macro package build passed against the package's pinned Pi 1.0.4**, output `package-acceptance-final-104/extension.mjs`; it was neither installed nor activated. `git diff --check` passed.

A separate packaging attempt against 1.1.0 correctly refused the unqualified package runtime: `build.mjs` still pins 1.0.4. This is not a claim of a 1.1.0 release package; its actual SDK suite and strict compilation passed. Reusing an earlier output directory also encountered `EEXIST`; the final successful package build used the fresh directory above.

## Ownership repair and practical coverage

The prior account-replacement blocker is fixed by the small guard in `clients/pi/provider.ts:291–374`: capture existing account/run epoch and run scope before auth awaits; validate live ownership/abort state before publishing a request marker or converting a failure into a current-turn warning. Cancellation/replacement dominates warning fallback and cannot poison the replacement's failure state. Existing auth-epoch checks and same-account renewal remain. No new token/capability framework, replay or recovery policy was added; `perform()` needed no change in this follow-up.

Actual native regression `tests/phase02_pi.mjs`, “logout/session/account replacement invalidates pending renewal and rejects stale publication,” passes on both SDKs: **zero dispatches for all three obsolete pending-renewal variants**, replacement failure state unchanged, and a deliberate new-account turn then succeeds once. A separate actual current-turn auth-expiry/login HTTP failure yields a safe warning, **zero dispatch/retry**, and no resident account/client/trust reset.

Other completed coverage includes successful text/tool continuation; Pi-owned undeclared-tool/schema errors and argument parsing; immutable dispatch; extra response fields and authoritative charge despite inconsistent/unreadable optional metadata; missing receipt/finish/DONE/EOF; incomplete/length/interrupted tools withheld; interrupted receipt evidence; same-session next-user-turn; no inference retry/replay; pre-dispatch setup recovery and abort; fatal backend binding/decryption failures; and an earlier operational error followed by typed integrity failure retaining fatal priority and blocking later dispatch.

Manual/split/threshold compaction, cancellation, summary failures, and no checkpoint/fallback/replay passed. Native threshold compaction was exercised after both completed and warning-length high-usage turns: **three distinct submissions** (two user turns plus one new summary), not a replay of the original generation. This existing host behavior can incur summary charges; warning-only control flow is not a guarantee of zero autonomous host work. Native same-signal extension callbacks can participate in existing setup renewal, and Stop does not cancel an already awaited `before_agent_start` hook in the tested SDKs; these retained qualification cases are explicit host boundaries, not new restrictions or fixes.

Catalog regression: upstream input/output unit rates `1000000` become indicative retail estimates **1.30**. Received total charge **7 microunits** remains authoritative; component zeros are unavailable placeholders. No reservation/charge arithmetic or equality gate was reintroduced. Cache-price metadata was not redesigned.

## Exact commands and artifacts

From scratch `source/clients/pi`, cached dependencies only; **do not run `build.sh`**. Select peer links for each SDK:

`for NAME in pi-ai pi-coding-agent pi-agent-core; do ln -snf "/Users/mb5/.pi/agent/install/releases/1.0.4/node_modules/@earendil-works/$NAME" "node_modules/@earendil-works/$NAME"; done`

`POSSUMS_PI_ROOT=/Users/mb5/.pi/agent/install/releases/1.0.4 node check.mjs`

`node node_modules/typescript/bin/tsc --noEmit`

`for NAME in pi-ai pi-coding-agent pi-agent-core; do ln -snf "/Users/mb5/.pi/agent/install/releases/1.1.0/node_modules/@earendil-works/$NAME" "node_modules/@earendil-works/$NAME"; done`

`POSSUMS_PI_ROOT=/Users/mb5/.pi/agent/install/releases/1.1.0 node check.mjs`

`node node_modules/typescript/bin/tsc --noEmit`

For the fresh production bundle, restore the scratch peer links to **1.0.4**, then:

`PHASE02_OUT=/private/tmp/possums-pi-build-client-simplification-HrkWFN/package-acceptance-final-104 node build.mjs`

Synthetic outputs in scratch: `acceptance-104.log`, `acceptance-110.log`, `tsc-104.log`, `tsc-110.log`, `build-final-104.log`. The check harness switches stale scratch bundle peer symlinks between SDKs. Installed SDK files remain unchanged. Scratch source links were restored to 1.0.4 after final verification.

## Lean-suite changes

Only Pi tests and their related discovery/check harness were pruned/migrated; shared gateway/trust/privacy/accounting regressions remain intact.

Removed/collapsed categories:

- Gigantic numeric presentation-boundary fixtures and unused modes.
- Giant tool/history payloads, 66-generation continuation stress and oversized idle-history fixtures.
- The 256-case preparation matrix and duplicate schema stress. One modest nine-level schema snapshot and submission demonstrates no artificial inference-depth quota.
- Obsolete exact-field, reservation-arithmetic, null-history and duplicate tool-schema rejection assertions.
- Broad obsolete retry/component matrices and retry-driven expiry/reconciliation branches, replaced by representative actual native stage, warning, continuation and privacy checks. Duplicate repeated-warning turns were reduced.

Meaningful stale-run, trust, abort, accounting, tool-withholding, compaction and privacy regressions were retained. Received complete money before interrupted delivery now remains known rather than being erased; incomplete tools still cannot execute. Discovery/RPC assertions use warnings for current operational setup failures and fatal host errors for unconfirmed backend trust.

## Native cancellation-presentation exception and external limits

The supported `ApiKeyAuth.resolve()` returns `AuthResult | undefined`; it has no warning/canceled-turn result. SDK `pi-ai/dist/auth/resolve.js:124–130` wraps a rejection in `ModelsError("auth", ...)`, and native stream setup produces an error before invoking the adapter. On both SDKs, intentionally obsolete account/session/logout auth therefore presents **`API key auth failed for provider possums: possums_login_cancelled`**, not an adapter warning. Returning a success marker is unsafe. The regression verifies this fixed local code, no backend-trust classification/latch, zero obsolete dispatch and usable replacement account. **The operator explicitly accepted this narrow intentional-cancellation exception after reviewing these results.** Ordinary current-turn operational failures remain warning-only.

Actual installed T3 notification/history rendering was not exercised. Native Pi/T3 may retain ordinary output and safe diagnostics; notifications are not a whole-process transience guarantee. Gateway charge-component fields remain unavailable pending an agreed wire contract. Exact monetary strings remain authoritative despite approximate native numeric presentation. Preserve legacy-v2 freshness limits, unresolved native-v3 CLI platform-signer mismatch, live billing/privacy unknowns and upstream-erasure limits.

## Privacy scope

Follow [PRIVACY.md — data handling boundaries](../PRIVACY.md#data-handling-boundaries), [telemetry restrictions](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data) and [synthetic testing](../PRIVACY.md#synthetic-testing-exception). Negative tests use locally authored `PRIVATE_*`/`HOSTILE` sentinels; raw exceptions, prompts/history, credentials and hostile fields cannot escape diagnostics. Interrupted bare refund claims do not fabricate confirmed refund. No diagnostic logger, telemetry, support bundle, reporting-triggered inference or automatic inference replay was added. Scratch outputs contain synthetic assertions only.

Release checks use real SDK orchestration, hashing and gzip/expiry/configuration checks but mock AMD verification, DSSE/Rekor crypto, X509 and public responses. Cache checks use real separate processes with synthetic acquisitions. Neither proves deployed gateway/hardware or live privacy/accounting behavior.

## Main integration — offline acceptance

Parent independently inspected the five resolutions and final PR diff against current main, then reran both SDKs' full unfiltered checks and strict TypeScript successfully. A separate fresh pinned production artifact at `package-parent-integration-104-ZEr0d3/extension.mjs` has the same SHA-256 as the artifact below. Parent log: `parent-integration-validation-final.log`. The scratch validation helper's peer-restoration path was corrected to be absolute after its first completed check/build run reached cleanup from the worktree directory; the final full rerun exited 0. This is still offline/source evidence, not installation or deployed qualification.

Resolved the conflicted merge of exact main **`8e84e9c37130475433b3009bfe9f8b1c4ebd2631`** (PR64 rollout policy and PR65 session caching) into accepted client source **`bdc62f9bd16accad085153c3dd741ac1e3e08e0c`**, branch `fix/pi-client-graceful-warnings`, worktree `/Users/mb5/.t3/worktrees/possums/fix-deepseek-generation-http-retries`. The older parent main checkout was untouched. This packet resolves/stages source only; the parent owns the merge commit, PR/push/merge, explicitly authorized `no-rollout-needed` label and local installation. No commit, publication, installation, activation, deployment, live credentials or inference occurred here.

The five conflict resolutions preserve the approved warning-only/no-generation-replay policy and auth ownership guard, PR64 label guidance, and PR65 independently random memory-only API-session cache scope/verified-path/privacy boundaries. `examples/phase01/client.ts` retains optional cache counts unchanged when absent; no cache-subset, price, reservation or bill-component arithmetic was restored. Pi input remains the inclusive reported count; cache count and total are independent. Successful and interrupted authenticated receipts expose `cacheUsageAvailable` independently of unavailable charge components. Catalog cache-read metadata uses the optional upstream cached price or input fallback, with the indicative **1.3** retail multiplier. Incoming PR65's quote-derived Pi components and native transient retry descriptions are superseded by [the approved client policy](pi-client-failure-policy.md), not restored by this merge. Its gateway settlement arithmetic remains unchanged.

The automatic README merge was corrected narrowly to describe inclusive input, independent cache availability and unavailable components. The reference transport is byte-identical to `bdc62f9`: verified EHBP dispatch, native decoding, authentication-tag minimum, HTTP byte-idle handling and closed request authority remain; no client-owned cache secret/header or warming was added. All incoming gateway/cache/release files, `PRIVACY.md`, PR65 Rust tests and static shipped-decoder fixtures are byte-identical to main. Policy references remain [data handling](../PRIVACY.md#data-handling-boundaries), [forbidden exports](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data) and [synthetic testing](../PRIVACY.md#synthetic-testing-exception).

Exact worktree TS/MJS/JSON sources and all MJS fixtures were refreshed into the existing cached scratch and compared byte-for-byte, with no network installs. Both real cached SDKs **1.0.4 and 1.1.0** passed unfiltered full `check.mjs` and strict TypeScript `--noEmit`, exit **0**: shared **38**, Phase 02 **682**, release **177**, public acquisition **39**, Pi/diagnostics **159 groups** (including **9 diagnostic groups**), discovery/RPC **28**, admission **70**. The shared stream regression still processes **65,541 events/frames, 76,809,934 encrypted bytes**. Discovery makes zero inference/public-network calls; admission rejection makes zero encryption/network/getter calls. New merged cache regressions cover absent/zero/partial/full counts, missing/zero/discounted catalog rates, unknown quoted cache price and capped authenticated totals, including received money after interrupted delivery, without reconstructed components or warming.

The initial 1.0.4 attempt stopped on a new synthetic catalog fixture supplying an explicit JavaScript `undefined`; the fixture now correctly omits the absent optional JSON property. Both subsequent full runs passed without skips. Worktree LSP lacked cached peer dependency links and reported unresolved SDK imports/derived implicit-any findings plus an existing guarded-JSON rule finding; authoritative scratch strict compilation against both actual SDKs passed. No unresolved implementation/test error remains in the requested checks.

`check.mjs` does not run Rust or the standalone historical `shipped_usage_decoder.mjs` extraction script. PR65's session ownership/expiry/logout/concurrency, builder-scope and cache-availability/discounted-settlement Rust regressions were preserved unchanged, not rerun here; hosted pinned Rust CI remains a required parent gate. The unchanged static extraction script contains the older strict decoder's malformed-cache/interrupted-money expectations; those are not this candidate's acceptance contract and must not be reported as a new-artifact pass. Historical PR65 evidence is not retrospectively upgraded.

Fresh production-macro build against pinned **Pi 1.0.4** passed. Artifact: `/private/tmp/possums-pi-build-client-simplification-HrkWFN/package-integration-104/extension.mjs`; SHA-256 **`5e7c451e7259b3778e9d35b2fafda59dd35ea9596f9dfb8a3a4404c6f04e8db3`**. `build-report.json` hash matches the file; cached source peers were restored to 1.0.4. Logs: `integration-104.log`, `integration-110.log`, `integration-tsc-104.log`, `integration-tsc-110.log`, `integration-build-104.log`; the initial fixture-only failure is retained in `integration-104-initial-fixture-error.log`. Conflict-marker/stale removed-symbol scans and `git diff --check` passed. This is source/build evidence only, not installed notification rendering, live billing/cache-hit/invoice/retention/erasure or deployment evidence. Legacy-v2 freshness, native-v3 platform-signer and live privacy/accounting unknowns remain unchanged.

Merged source SHA-256:

- `clients/pi/provider.ts`: `3cf68976251cacd4c5db2e8d3d8b7efed3387d08488bd5ab0c05a3eefef49822`
- `examples/phase01/client.ts`: `fda17a4b66be57d0445432c0d684db80bdf1b07b852cd5f38bdf4b03155a5485`
- `examples/phase01/transport.ts`: `222dc52ed8edf9def426b15031cf6ee43f297b3d5260921770a12c0a9a894e3d`
- `tests/phase02_pi.mjs`: `6e41d67825635526ec2cff881dfe3127b69ecf2d00ce631d992d89ec1b84c102`
- `tests/tinfoil_forwarding_client.mjs`: `a1624a4282ae9b20398419062c7a38239516fc25109bf048013b6ff47bf34caf`

Preserved main cache source SHA-256 (byte-identical to `8e84e9c`):

- `src/auth.rs`: `84cce501289ec546c20565fa433cd549d0db778063760168d8a6c451e5102c6f`
- `src/api_stream.rs`: `118565a1e38b30b2bb56f905b55ec020f36807a48c8cd5b66b760032f41227e2`
- `src/generation.rs`: `ef2eb8b260ee26674a5b55f093c5e30b348085d6792a6351b65fc4f0a8e529d0`
- `src/inference.rs`: `b8575724b0a59f7f2d0c942943b6bcefc420d269a170b0510c0f14768745f8ef`
- `src/inference/forward.rs`: `4c66e81167455aef712b86fe145562e2322fd077dd1191e3a5566fa587d99dd3`
- `src/inference/stream.rs`: `dd4d41e9324d360785f46ff18b344890f8d529da25d4607509ce81fe09e4942f`
- `tests/support/native_usage_contract.rs`: `db06b5feea32125aa7dbc30a59df5f53dab8c1c9cb69569a4218640233aaf7f7`

Parent revalidation uses the exact SDK-link/check/strict commands above in scratch `source/clients/pi`, substituting logs as desired. Refresh first:

`W=/Users/mb5/.t3/worktrees/possums/fix-deepseek-generation-http-retries; S=/private/tmp/possums-pi-build-client-simplification-HrkWFN; cp "$W/clients/pi/"*.ts "$W/clients/pi/"*.mjs "$W/clients/pi/"*.json "$S/source/clients/pi/"; cp "$W/examples/phase01/"*.ts "$S/source/examples/phase01/"; cp "$W/tests/"*.mjs "$S/checks/source/"; cd "$S/source/clients/pi"`

After both SDK runs, restore source peer links to 1.0.4 and use a fresh package directory (never reuse one with peer links):

`PHASE02_OUT=$(mktemp -d /private/tmp/possums-pi-build-client-simplification-HrkWFN/package-parent-integration-104-XXXXXX) node build.mjs`
