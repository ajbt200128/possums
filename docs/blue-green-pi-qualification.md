# Pi deployment recovery — native-permission qualification

**Status: blocked at the observed provider boundary. Recovery is not enabled.** Local test-only commit `58f1b82` extends `tests/phase02_pi.mjs`. This record does not revise previous source/live evidence retrospectively and is not deployment, installation or runtime-policy authorization.

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

## Decision required

Keep recovery activation closed until one of these is explicitly approved and qualified:

1. A supported pinned-runtime integration supplies trustworthy request-scoped provenance, preserving the original native-only policy; or
2. The operator changes the policy to permit otherwise eligible trusted extension requests in the same authorized active user run, acknowledging that they cannot be distinguished by the observed boundary.

Do not silently choose the second policy while claiming the first. Either path still forbids uncertain generation replay and preserves the separate native transient-retry policy. Server shutdown/readiness work can proceed independently. Existing expiry-permission behavior needs a separately scoped decision rather than an unreviewed adjacent fix.
