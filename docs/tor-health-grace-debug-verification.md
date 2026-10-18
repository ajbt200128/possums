# Tor-gated health with ten-minute debug grace

## Authorization and scope

Operator requested restoring Tor to health and deploying with the new600s grace
to the existing `possums-debug` only. This continues the v0.0.53-based experimental
branch `debug/tor-no-health-gate-v53`, repository `ajbt200128/possums`. The separate
attestation investigation remains with its existing owner; this packet performs
no attestation or inference probe, production/main operation, SDK fork, new
instance/repository, key provisioning or private-original key read. Existing
approved bindings/custody remain unchanged; exclusive shared-key publication and
physical cleanup of prior publishers remain unproved.

Read AGENTS and PRIVACY. Applicable policy: [closed startup diagnostics and the
debug extension](../PRIVACY.md#startup-diagnostics-exception), [data boundaries](../PRIVACY.md#data-handling-boundaries),
[allowed signals](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[processors and persistence](../PRIVACY.md#processors-retention-access-and-shutdown).
Export remains disabled. No new logger, periodic records, request/peer collection,
export family, retry, spool or detached work. Native platform capture remains
best-effort/nonblocking, not guaranteed.

## Minimal change

A separate mechanical cleanup renames the experimental feature to
`debug-tor-readiness`, accurately describing its retained extended observer.
No observer semantics changed in that commit. Historical records/tags retain
their original names and evidence.

The actual behavior change removes only the experimental exclusion from the
existing Tor health condition. The actual router now requires current SDK
`Running && !fatal` readiness when Tor is configured, plus the unchanged
non-Tor serving lifecycle. Pending, degraded or recovering Tor fails health;
readiness loss reverses a prior healthy result. Quiescence fails health even
with ready Tor. Disabled Tor still uses the ordinary process health condition.

Retain the exact600s Docker grace,10s interval,5s timeout and three retries;
preparation/bootstrap/launch, engine failure/drain and all client trust/accounting
controls are unchanged. Keep the pre-bootstrap observer, waiting record after30s,
explicit590s diagnostic deadline if pending, and finite observation until actual
first-ready/failure/shutdown. A timeout never fakes readiness or changes lifecycle.
The debug image label becomes `tor-health-gated-v53`.

## Offline source evidence

Pinned flake Rust1.92, locked Cargo: debug library432 passed, one pre-existing
operator-only fixture ignored; binary6 passed. Normal Tor readiness filter17
passed (includes the newly renamed health test); the explicit normal Tor router
health test also passed. Formatting, strict all-target/all-feature Clippy and
no-default-feature check passed. Existing vendored SDK dead-code warning remains.

The actual router regression exercises pending503, ready204, readiness loss503,
recovery204 and quiescence503 using one shared router. The actual engine status
owner is also connected to the actual router in an offline synthetic stream:
initial Shutdown, Bootstrapping, DegradedReachable, DegradedUnreachable and
Recovering give503; Running gives204; terminal EOF clears readiness and gives503.
Existing fatal/panic/engine tests, extended deadline/delivery and hostile diagnostic
privacy tests remain in the full debug suite. No SDK bootstrap or publication
occurs in these tests. Positive Linux FIFO behavior is left to the hosted Linux
suite, not claimed from the Darwin run.

[Hosted Linux build39152985349](https://github.com/ajbt200128/possums/actions/runs/39152985349),
attempt1, passed from exact source `e964d1cbfc2218192d62b9b080dfe3cc63dca779`:
normal/debug tests, formatting, strict Clippy, pinned image build and archive/
registry equality. Independently inspected the downloaded archive with supported
Skopeo: digest, Linux/amd64, UID/GID65532, source/debug labels and Nix-store debug
entrypoint matched the recipe. Exact digest:
`sha256:8667405e5469618437259700a06c63bc73d540ac07d5ef5a65e7a325bf715491`.
The config-only image replacement retains the prior600s grace, all remaining
controls and disabled export. No raw image configuration was printed or retained
as diagnostics. Signed release, actual debug rollout and runtime health remain
pending.
