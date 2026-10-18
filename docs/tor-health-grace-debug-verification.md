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
as diagnostics.

## Signed release

[Publication39154073031](https://github.com/ajbt200128/possums/actions/runs/39154073031),
attempt1, passed for `v0.0.53-debug-tor-health.3`, release commit
`c5696be5068083b3cbb6a5bd3ddc50c613d91e01`. The existing repository manifest
validator checked exact committed config/image and downloaded manifest/hash.
Supported `gh attestation verify` checked the exact tagged diagnostic workflow,
source/signer commit, supported repository predicate and hosted runner. Additional
checks bound the signed subject/predicate and exact run/attempt, successful job,
build parent and config-only image replacement. Manifest SHA256:
`f1f4f78e8ef5a5467ee4d2c1e4b2b49d757bdc74f96ac3db445e8b9fefb6ca18`.
Promotion was disabled. These are public release-signing checks, not fresh device
attestation or production admission. The separate attestation investigation was
not probed or changed.

## Actual rollout — failed, no replay

Fresh exact existing debug selection found `.2/running`, no pending update and
five managed bindings. Two read-only plans (standalone and immediately before
submission) approved `.3`, blue-green/no downtime/no hold, promotion false,
unchanged resources/opaque variables/references/SSH settings and only the exact
existing five secret refresh names. One update POST was acknowledged. Readback
progressed through pending and started, then **update failed** while `.2` remained
current/running. Opaque settings/references and promotion false were preserved.
No replay, acceptance, cancellation, rollback, relaunch, direct toolbox boot or
production/main action occurred.

| Boundary | Observed result |
| --- | --- |
| Source tests and image build | Passed |
| Signed release/config/image binding | Passed |
| Debug update | `DEBUG_UPDATE_FAILED_NO_REPLAY`; platform-reported failed |
| Current release after failure | `.2/running` |
| Pending release after failure | `.3/failed` |
| New candidate runtime readiness/health/grace | Unknown; not qualified |

The fresh root metadata retained a linked candidate debug SSH port. One bounded
read-only SSH attempt used that port under the exact `.2/current/.3/failed` root
selection, accept-new/never-replace-changed host keys and no forwarding. The
required installed toolbox README/AGENTS could not be obtained, so inspection
stopped at `DEBUG_FAILED_CANDIDATE_TOOLBOX_DOCUMENT_MISSING` before any gateway
inspection/probe. Neither candidate image identity nor runtime Tor state, Docker
health, effective grace or startup logs were established. The SSH/candidate
availability cause is unknown; port metadata alone is not a live-host guarantee.
No raw SSH/platform errors, logs, config, identifiers or private data were
printed or retained as diagnostics. Final native read-only update status still
reported failed; its underlying cause was not available through the closed
projection. Thus this does **not** establish that Tor missed600s, that the new
binary started, or that attestation caused the rollout failure. Prior `.2`
readiness/reachability evidence must not be relabeled as `.3` evidence.

The failed pending operation remains in place. Another update or lifecycle action
requires a separately reviewed operator decision and fresh exact state selection;
this experiment does not authorize automatic cleanup/retry. Production unchanged.
