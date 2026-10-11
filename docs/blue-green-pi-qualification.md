# Pi deployment recovery — native-permission qualification

**Historical native-only qualification (test-only commit `58f1b82`): blocked at the observed provider boundary.** This finding remains true. The operator later approved otherwise authorized trusted nested/extension and other caller-wide model-request setup within a shared run budget. Initial E2/E3 evidence was local/offline only; the subsequent paired live qualification below is separately scoped and does not imply an installed-client update. This historical record does not retrospectively verify a deployment or grant native-only provenance.

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

At that source packet, remaining unknowns included installed-client activation, live release compatibility, production health/retained owners, overlap credit/account continuity, catalog/profile changes, paid receipts/reconciliation and platform retention. H1/H2 were still separate. Those historical checks do not imply the later live qualification below.

## Subsequent paired real-Pi / v0.0.26 qualification

The operator required E completion before one combined, bounded paid real-Pi pass and one production health rotation, without further Astra rounds for small changes. Parent inspected the actual scope fixes (`bb595e4`, `16b0c76`) and qualified the source. Callback types were made explicit (`9fc69e9`). Live credential-free provenance preflight then reproduced two GitHub attestation candidates for the reissued v24/v25 manifest, before inference. `18d37c8` uses the supported Sigstore SDK identity policy to select the unique expected-tag **hint**; full signature/Rekor/publisher/subject/hardware/endpoint verification remains mandatory. Wrong-tag, ambiguous, forged-signature and bounded-candidate negatives pass.

Fresh production bundle and strict pinned Pi 1.0.4 compiler checks passed, together with the full offline SDK/client/admission/cache/discovery/Pi suites: **177 provenance assertions and 147 Pi groups**. A separate cached Pi 1.1.0 strict peer-interface check passed; it was not Pi 1.1 execution or live qualification. Active LSP probes were inconclusive, not clean-build evidence. Removed dependency bindings subsequently produced stale missing-module diagnostics; restoring cached bindings and running strict TypeScript directly on the checkout passed. The cached server findings were marked stale, without source suppressions or dependency downloads.

The tested ephemeral extension SHA256 was `37217444ddeb50fbc62bcbbae53d67db9dc7ca65a8a721ffa338a208969b57a2`, using Node 24.13.0 / pinned Pi 1.0.4. The owned harness used a memory session, fixed synthetic prompts, no ambient tools/extensions/history, a conservative four-stream-start limit and a caller-attested current authenticated maximum-reservation snapshot. This is not an enforced future-price ceiling or proof of dispatch count. The supported stored recovery credential remained private; the credential store and permitted public-evidence cache are separate persistence boundaries.

The same resident real-Pi process completed paid calls before and after the paired production rotation, with authenticated terminal settlement receipts and no observed unknown-billing diagnostic. The harness closed after its successful finish event. Billing values and transcripts are not published here. This verifies the scoped same-session before/after path; it does not live-test every synthetic caller/compaction/race case, prove a guaranteed receipt, account/credit durability or retained owners, or change the native generation-retry policy. The installed artifact/link was not changed.

PR #54 paired head `a42b83e208cce4117243d8ee8229a1a155fefc2b` passed CI `38101186287` and merged as source `56c9b4cec3fb791ac21834507e62ee158f57f5ae`. Exact release and production receipts are recorded in [health activation](tinfoil-health-activation.md#paired-v0026-production-evidence). Actual runtime deployment is not retrospective verification of the preceding mocked crypto fixtures. Preserve legacy-v2 freshness, native-v3 CLI signer mismatch, unknown platform health-flag readback and retention/credit/startup-duration limits.

## Subsequent operator-authorized E installation

The operator separately authorized installation and a new DeepSeek thread across another protected deployment. Before that overlap test, parent revalidated the exact live-tested extension hash above, all 11 bundled own-source module hashes, third-party hashes, lock and three pinned Pi 1.0.4 peer manifests. Only the production extension/package/build report/peer links were copied. The existing registered stable link was atomically changed to `pi-deployment-recovery-37217444ddeb`; rollback `pi-public-cache-integration-988efd995ef2` remains. Relative user-package registration and credentials/defaults/retry/history settings were unchanged. No existing process was restarted.

A fresh isolated **installed Pi 1.1.0 SDK** process loaded that stable extension, restored fully verified **v0.0.26**, authenticated the current catalog and selected `deepseek-v4-1-flash` without inference. This is installed startup/selection evidence, not the upcoming overlap test, guaranteed owner retention or full Pi 1.1 qualification. Native credential/model metadata/public-evidence persistence and ordinary T3 history remain separate boundaries.
