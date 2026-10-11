# Pi deployment recovery — native-permission qualification

**Historical native-only qualification (test-only commit `58f1b82`): blocked at the observed provider boundary.** This finding remains true. The operator later approved otherwise authorized trusted nested/extension and other caller-wide model-request setup within a shared run budget; E2/E3 are locally implemented and tested offline, not installed or live-qualified. This historical record does not retrospectively verify a deployment or grant native-only provenance.

Policy: [data boundaries](../PRIVACY.md#data-handling-boundaries), [forbidden exports](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data), [synthetic qualification](../PRIVACY.md#synthetic-testing-exception). Tests use real pinned Pi 1.0.4 lifecycle/auth integration with synthetic provider/channel responses and throwaway synthetic data. They do not perform live inference, production evidence acquisition or account/billing verification. Synthetic case/home directories are cleaned up; result artifacts contain fixed test names/outcomes only. No new Pi/global monkey-patch or core client behavior change was introduced.

## Evidence and scope

A T3 Astra qualification task added ten native/nested-call probes and a two-case existing-expiry probe; the task completed before the user requested switching implementation to Sol. A separate T3 Sol read-only verifier inspected the actual tests and reran the cached scratch harness: **111/111 groups passed**. Syntax and committed-diff whitespace checks passed. The first task's LSP check was inconclusive, not evidence of a clean typed build. Parent inspected the added test assertions directly.

The tests demonstrate:

- At the observed provider auth boundary, nested extension calls can receive the same active signal and full auth inputs as native requests.
- Nine probes also match the native model and transcript, including preceding tool results where applicable. The initial `turn_start` probe is explicitly signal-only because it precedes user-message delivery.
- Probes cover post-confirmation `message_start`, initial/continuation `turn_start`, context/payload hooks and tool execution through supported `ctx.modelRegistry.streamSimple`.
- Native tool rounds remain in one run; the synthetic tool executes once. Matching tool results or run signal cannot serve as a trustworthy native request capability.
- Two post-confirmation probes reproduce nested calls consuming **existing expiry-renewal permission**. This is a newly demonstrated limitation of the current client, not an implemented deployment-recovery feature or server regression. The existing client behavior was not changed.

See the `permission qualification BLOCKED` tests in `tests/phase02_pi.mjs` (at qualification, around lines 1057–1148). The reproducible cached runner used was `node /private/tmp/possums-pi-permission-check.mjs`; it is a transient local harness, not an installed production client or new repository entry point. Reproduction requires the same preexisting cached dependencies/pinned Pi build; no `npm ci`, dependency installation or provider request was performed.

## What is not proved

These probes do **not** establish that every supported Pi integration lacks a request-scoped provenance mechanism. They establish indistinguishability at the tested auth/provider boundary. They do not authorize stack inspection, transcript-text matching, a global host monkey-patch, generic key-error recovery, or automatic replay of uncertain inference.

The proposed three-attempt budget still requires locally proven pre-inference dispatch state, full verification of changed evidence, frozen prepared input and cancellation/account/session guards. Those safeguards do not themselves distinguish native continuations from extension-originated calls.

## Historical decision, now resolved for local source only

At the time recovery activation remained closed until one of these was explicitly approved and qualified:

1. A supported pinned-runtime integration supplies trustworthy request-scoped provenance, preserving the original native-only policy; or
2. The operator changes the policy to permit otherwise eligible trusted extension requests in the same authorized active user run, acknowledging that they cannot be distinguished by the observed boundary.

The operator explicitly chose option 2 for local E implementation. Option 1 was **not** demonstrated: same-signal native/nested calls remain indistinguishable. The caller-wide source candidate preserves the independent native generation-retry policy, uses a shared three-attempt run/idle-compaction budget only for pre-dispatch setup, and never automatically replays uncertain generation or completed tools. Local offline fixtures do not establish installed-client activation, paid billing, production cutover or platform retention.

## Subsequent local E2/E3 source qualification (not installation)

Scoped synthetic checks on Node 24.13.0 and cached Pi 1.0.4 passed strict TypeScript and production-bundle build, **171 release-fixture assertions and 138 Pi check groups**. The in-worktree LSP cannot resolve the intentionally uninstalled Pi/Node peer dependencies (two files report derived module/type errors; three scans are inconclusive); the scratch build with cached qualified peers is the compiler evidence, not an LSP clean claim. Release-fixture public GETs were all credential-free/no-store with fixed authorities. Unchanged report **and** EHBP configuration stopped before GitHub/AMD; a changed report or key required complete existing SDK verification, including same-tag/same-measurement key rotation. A mismatched key was rejected. This fixture mocks AMD hardware cryptography, DSSE/Rekor cryptography, X509 parsing and network responses, so it does not prove live evidence compatibility or a new platform identity.

Pi checks include an actual EHBP plaintext HTTP 422/missing-nonce boundary (only a local submission/endpoint-binding observation); frozen prepared hooks and last synchronous no-dispatch guard; three shared attempts, 0/250/1000 ms delay, coalescing and one caller abort; unchanged evidence/exhaustion latch; credential-free failed probe; logout and payload-hook races; removed model/tool profile; unknown submission-control billing retained beside a later settled receipt; actual Pi native tool continuation; native retry behavior unchanged; idle split-compaction two-setup recovery and known-expiry renewal; and existing privacy/hostile-error/receipt regressions. The isolated local candidate was not installed, did not call production, and sent no paid inference. `PRIVACY.md` [data handling](../PRIVACY.md#data-handling-boundaries), [forbidden exports](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data), and [synthetic boundary](../PRIVACY.md#synthetic-testing-exception) apply: the fixture may use synthetic content and no new logging or exported diagnostics was added. Normal Pi/T3 history persistence remains outside this scope.

Remaining unknowns: actual installed-client activation, release/publisher endpoint compatibility after promotion, production health and retained-owner behavior, overlap credit/account continuity, live catalog/model/profile changes, paid receipt/reconciliation and platform-side retention. The separate H1/H2 local health branch and any runtime/deployment changes were not combined with this source packet; a coordinated paid E2E and health rollout is separately authorized later, not implied by these checks.
