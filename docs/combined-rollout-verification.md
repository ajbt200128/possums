# Combined rollout candidate — 2026-10-09

Source PR: [#34](https://github.com/ajbt200128/possums/pull/34). Scope: [five-minute telemetry](telemetry-mvp-five-minute.md), [Pi trust sessions](pi-session-update-verification.md), and [quota simplification](quota-simplification.md). This is source/local evidence, not a deployed release or full E2E pass.

Policy: [data handling](../PRIVACY.md#data-handling-boundaries), [forbidden exports](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data), [aggregation contract](../PRIVACY.md#aggregation-is-necessary-not-sufficient), and [required evidence](../PRIVACY.md#required-evidence-for-every-telemetry-change).

## Integration checks

Session commit `fe6764f` was followed by the four separately scoped quota commits `2684470`, `ba86202`, `d682a6a`, `5f1cf89`. Prior Pi compatibility fixes remain in history. Full `devenv test` passed formatting, strict Clippy, Rust and Go checks. Fresh Node 24.13.0 / Pi 1.0.4 scratch compilation and 59 Pi, 31 reference, 382 tool-client, 98 synthetic release-policy and 71 admission checks passed. Release-policy crypto mocks and native synthetic gateway limits remain as described in the session record.

Combined read-only review found no accounting, lease-retirement, telemetry or session-trust defect. Its objection that removal of arbitrary tool-history quotas left an unbounded pre-tokenizer decoder was independently refuted: heavy admission precedes collection; body collection retains 8 MiB / 30-second bounds; the duplicate-key JSON guard enforces depth 64 and 131,072 nodes before typed decoding. Focused long-history/catalog/parser/lane tests passed. **This does not prove a 104-MiB heavy RSS envelope or platform headroom.** The existing memory target remains an unproved worksheet, not a new allocation guarantee. No arbitrary quota was restored on this unsupported premise.

## First exact-head CI failure and cleanup rendezvous correction

At `5f1cf89`, both image jobs in runs `37914967487` and `37914978249` failed. The library failures were `caller_cancel_body_and_observer_drop_cannot_cancel_handed_off_generation` and/or `settling_observer_and_body_loss_while_held_never_abort_work`: resource permits were still three when the test expected four. Browser and image-startup jobs passed at the inspected checkpoint; flake jobs were still running. This is recorded as failed CI, not a passing rerun.

Both tests waited for generation-marker permits and assumed this proved final memory cleanup. The reviewed runtime deliberately retires generation tracking **before** returning heavy memory admission; a concurrent test can therefore observe that gap. The two-hunk test-only correction waits for full resource admission instead, with the original cancellation, settlement, drop-probe, held-resource and restored-capacity assertions intact. No runtime drop order, accounting, timeout or production guard changed. The complete generation-owner suite passed thirty runs with eight test threads, and full `devenv test` passed again. Independent read-only review confirmed the correction follows actual field-drop order, preserves the accounting/probe assertions and introduces no supported deadlock. Corrected-head CI remains required.

## Live and operational boundaries

Real production-policy public bootstrap accepted serving v0.0.15 with zero credentials/inference. A separately operator-authorized pre-rollout synthetic native Pi tool probe against v0.0.15 failed: one failed assistant turn, zero tool executions and zero settled receipts. Only a billing-unknown result was retained; its detailed cause was not captured. It was not replayed. This is not new-release E2E evidence or a confirmed refund.

The operator separately authorized reconstructing the write-only deployment account list from a locally stored credential, removing other provisioned entries, and setting a fresh $100 demo grant. The secret update was accepted and its metadata timestamp checked; no credential, hash or account identifier is recorded here. It activates at the next gateway restart; live balance is not verified. Phase 0 restart resets the in-memory ledger, not durable purchased-credit accounting.

Publication, installation, stop-confirm-deploy rollout, actual replacement-session behavior, authenticated live generation/settlement and Honeycomb request-query/dashboard acceptance remain pending. Legacy-v2 freshness, native CLI v3 signer incompatibility, platform memory/retention and whole-runtime privacy limitations remain unresolved.
