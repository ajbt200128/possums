# Verified upstream-client renewal — historical source candidate

**Intentionally removed in the current operator-authorized
[runtime-path regression control](runtime-path-regression-control.md).** The
candidate again uses one startup-verified direct SDK client, without later
renewal or request replay. Upstream rotation can leave that client unavailable;
TLS still fails closed. This loss of availability recovery is deliberate and
is not a proven native-503 fix. The mechanism and offline evidence below describe
the prior renewal implementation, not the current runtime.

## Authorization and scope

The operator requested a durable gateway fix after [source inspection and a public pin comparison](tokenizer-failure-verification.md#subsequent-gateway-upstream-pin-investigation). That comparison found different upstream TLS pins, but was not fresh enclave attestation or historical request attribution. The operator then explicitly authorized implementing verified renewal. This record qualifies source/offline behavior, not installation, deployment, live recovery or an upstream outage's root cause.

Privacy boundaries: [data handling](../PRIVACY.md#data-handling-boundaries), [forbidden telemetry](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data), [typed failures](../PRIVACY.md#typed-interface-failures-and-token-aggregate-extension) and [required evidence](../PRIVACY.md#required-evidence-for-every-telemetry-change). No telemetry family/dimension, logs, traces, automatic runtime diagnostic persistence or client upload is added. Existing API-session prefix scopes, accounting, reservations, usage validation, receipts and disconnect ownership are unchanged.

## Mechanism

- Every catalog, tokenizer and generation operation acquires one `Arc<Client>` before constructing its authenticated upstream request. Acquisition admits that snapshot; an already admitted operation may finish using it even if another operation subsequently invalidates it. This is not atomic cancellation of existing work.
- The same acquired client supplies transport, authentication and failure attribution. A supported SDK `is_connect()` send failure marks renewal-needed only when its `Arc` is still the published generation. Delayed failures from an older generation cannot invalidate a newer client. DNS/TCP and TLS connector failures can all set this condition; it does not diagnose a specific cause.
- **The failed operation is not renewed or replayed.** Its original operation-specific failure continues through the existing sole error/accounting owner. A later distinct authorized operation may renew before dispatch.
- Renewal reconstructs a client through the existing supported SDK `Client::new`, including its provenance, hardware/measurement and endpoint/TLS-binding verification, under the existing 30-second connection-verification bound. The same borrowed evidence bounds are checked before publication. No observed/unverified key is accepted as a replacement pin. No SDK protocol, TLS verifier, trust gate or OpenAI retry layer is replaced or relaxed.
- A short-held synchronous state lock publishes the verified candidate atomically. A separate watch channel identifies each exact renewal attempt and shares its closed outcome with concurrent joiners. A failed cohort remains failed even if a later distinct attempt succeeds; joiners do not initiate serial verification retries. Dropping the owning future closes that attempt and leaves new dispatch gated. There is no spawned verifier, background timer, automatic inference replay or fallback to the old client for a new dispatch after renewal failure.
- Existing in-flight operations retain their own snapshot through response consumption, so publication does not change their endpoint/authentication or reinterpret settlement. The funded diagnostic path uses the same snapshot/invalidation rules; it was not invoked during offline checks.

## Status and compatibility

`verification_document()` remains synchronous and network-free. It projects the **actual last completed direct SDK verification**, with its original `verifiedAt`, rather than claiming current reachability or initiating renewal. Reading it does not clear renewal-needed or authorize dispatch. Direct clients retain immutable verified evidence; proxy SDK revocation still yields no document, with no cached/revoked proxy fallback introduced. No proxy transport or existing Pi bootstrap path is changed.

Wire fields, closed error details, quoted prices and receipt semantics are unchanged. Failed SDK verification uses the existing `verification_failed` category; existing bounded-evidence encoding failures also retain their typed category. The original failed send retains its prior catalog/tokenizer/generation category. Existing error observations remain at their operation owners; the shared renewal primitive does not add its own metrics or exception logging. Historical evidence is not a fresh appraisal or a service-health guarantee.

## Offline evidence

Pinned `devenv shell`, Rust 1.88, `OTEL_SDK_DISABLED=true`:

- Eight focused tests cover healthy/status acquisition without verification, snapshot retention and stale-generation failures, 32-way successful and failed cohorts, failure retained after a later successful attempt, owner/waiter cancellation, and invalidation immediately after publication.
- A loopback TLS fixture exercises a real certificate rejection through the SDK's supported pinned certificate verifier and the locked reqwest/rustls connector, establishing `is_connect()` and no timeout. It sends no inference/authentication and reads no HTTP request after TLS rejection. The local certificate is deliberately untrusted: this is **not** a full valid-CA pin-rotation, provenance/hardware or live upstream fixture. Client-generation tests use synthetic values and an injected verifier, not fake cryptographic acceptance.
- Full all-target/all-feature Rust tests: **565 passed, four live probes ignored**. Strict package Clippy and formatting passed. The unchanged vendored unused-method warning remains outside package Clippy.
- The exact shipped error-decoder matrix passed for **14** artifacts: v0.0.11–v0.0.15 (including retained v0.0.14/v0.0.15 variants), request preparation `693e580b216c`, forwarding `4a2a41884907`, graceful warnings `5e7c451e7259`, and installed v3/image bundle `83624d2899fb`. The harness uses TypeScript 5.9.3 and initializes no provider/network. No client sources, error fields or closed details changed; this is decoder regression evidence, not full old-host rotation acceptance.
- Active LSP probes on all three changed Rust files found no errors; `git diff --check` passed.
- An independent Sol design review and separate fresh implementation review found no blocker. The latter's documentation finding was addressed by explicitly labeling the original no-renewal findings as pre-fix and linking this candidate. Reviewers made no live calls or duplicate full-suite runs.

The candidate preserves current SDK attestation behavior and its existing platform/freshness limits; it is not a v3 migration. A first operation can still fail after upstream rotation. If a key changes again between verification and dispatch, TLS still rejects it; another distinct authorized operation is needed. Body/stream errors and generic HTTP refusals do not trigger speculative renewal. A later deployment/restart may independently replace the startup client; that alone would not demonstrate this renewal path. No live rotation, paid probe, installed-host acceptance, production image or deployed renewal behavior is qualified by these offline fixtures.
