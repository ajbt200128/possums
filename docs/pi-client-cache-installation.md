# Operator-authorized cache/diagnostics installation and live smoke

2026-10-10. The operator authorized installation and startup testing, then separately authorized an inference test. This is local client activation evidence, not a gateway deployment or actual T3 UI acceptance. PR #50 source commits `20e1565` and `e07891f` follow prerequisite `be92e0a`.

Policy reviewed: [change control](../PRIVACY.md#mandatory-reference-and-change-control), [data handling boundaries](../PRIVACY.md#data-handling-boundaries), [forbidden telemetry](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data), [synthetic testing limits](../PRIVACY.md#synthetic-testing-exception), and [required evidence](../PRIVACY.md#required-evidence-for-every-telemetry-change). The tiny test message was synthetic, but used the existing saved credential and shared deployed gateway: this was **not an isolated synthetic workload**. No raw credentials, upstream exceptions, hardware reports, request/response transcript, account identifiers, logs, traces or support bundle were retained in this record. Native credential/model-store and public evidence-cache persistence remain explicit; an in-memory session does not establish whole-process transience or upstream retention.

## Installation

The previously qualified production bundle's SHA-256 and every bundled client-source hash matched the checkout before copying. The artifact was copied with peer symlinks preserved, then the existing registered stable `~/.local/share/possums/pi/installed` link was replaced atomically:

- New target: `pi-public-cache-diagnostics-f3ed79084dd2`.
- Extension SHA-256: `f3ed79084dd2c67bb506403721d01644512ff8e4c4d62df19e39b7fdf8660ccf`.
- Rollback target retained unchanged: `pi-runtime-unblocked-30bc9b770a02`.
- Build and linked peers remain **Pi 1.0.4**. The live smoke host was the installed **Pi 1.1.0 SDK**.

No package registration, default provider/model, filters, user retry/compaction settings, credentials, history, Pi runtime, T3 host or gateway deployment was changed. Fresh processes used the stable installed extension, not a fixture/test bundle. Existing processes may still hold the previous module; use a user-controlled full restart/fresh provider process for activation.

## Live evidence

Two independently created Pi 1.1.0 SDK processes loaded only the installed extension, with no tools/context files/skills/other extensions and in-memory sessions/settings. Native credential resolution used the existing auth store without printing/copying credentials. Successful authenticated discovery seeded Pi's existing public model store. Only the isolated test settings disabled automatic retry, compaction, warming and install telemetry; user settings remain unchanged.

1. Startup-only process: six live authenticated models, no current setup failure or stale label. `deepseek-v4-1-flash` selected without substitution and without inference. The manually observed fetch counters showed two GitHub API requests and one live gateway attestation fetch.
2. Independent process: six live authenticated models, the same requested selection, no setup failure or stale label. Zero GitHub API requests and one live gateway attestation fetch demonstrated evidence reuse. Existing verifier orchestration still ran; fresh certificate/key/auth/catalog requests were not replaced by cached authorization.
3. In that second process, one short tool-free synthetic inference completed with the expected reply and an authenticated settled receipt. Observed charge: **69 microunits ($0.000069)**. No automatic retry or manual replay occurred. No receipt/balance reconciliation or upstream invoice check was performed; this verifies the receipt, not broader live billing properties.

Fetch counters were transient manual in-process observations, not request logging/telemetry or whole-process network interception. They count API fetch invocations, not redirected HTTP hops, OS traffic or every SDK transport. No sensitive URL/header/response value was emitted. No forced live failure, cache corruption, credential revocation or concurrent production probe was attempted; negative/concurrency qualification remains the [offline cache](pi-public-evidence-cache-verification.md) and [SDK/RPC diagnostic](possums-only-diagnostics-verification.md) evidence.

Actual installed T3 rendering, activation in pre-existing Pi/T3 processes, native first-use failure without metadata, process-loss/extension-load diagnostics, legacy-v2 freshness and native v3 platform-signer mismatch remain unchanged limits. The isolated SDK success is not full Pi 1.1.0 or end-to-end host qualification. Gateway/upstream retention, production accounting durability and privacy runtime properties are not established by this smoke.
