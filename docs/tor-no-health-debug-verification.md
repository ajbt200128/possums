# v53 Tor-independent health debug experiment

## Scope and policy

Nonproduction branch `debug/tor-no-health-gate-v53`, based exactly on
`v0.0.53` / `baeb334664df3e5406cad9b27f7db66dcb273b02` (verified clean before editing).
Only the existing `ajbt200128/possums` repository and `possums-debug` are in scope.
No merge/main activation, production operation, new repository/instance, nested
agent or inference is authorized by this experiment.

Read `AGENTS.md` and `PRIVACY.md` first. Applicable sections:
[data handling](../PRIVACY.md#data-handling-boundaries),
[forbidden diagnostics](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[startup diagnostics](../PRIVACY.md#startup-diagnostics-exception), and
[processor/persistence boundaries](../PRIVACY.md#processors-retention-access-and-shutdown).
Only finite closed projections may leave the debug VM. No raw logs, SDK errors,
configuration, environment, arguments, key material, reports or identifiers may
be collected. Existing platform capture and Pi/T3 history are separate boundaries.
No new telemetry/exporter or logging bridge is added.

The operator subsequently explicitly authorized using existing production-key
bindings on this debug instance and restarting the observed stopped instance.
This does not establish physical cleanup of earlier publishers or exclusive
publication. Private originals are not read/copied; no vault provisioning or
production configuration changes are part of this patch.

## Minimal source change

`debug-tor-no-health` implies `tor` and only removes the Tor atomic readiness
condition from `/healthz`. It neither sets SDK Running nor suppresses the finite
readiness diagnostics. Both pending and lost Tor readiness are ignored by debug
health; application quiescence still returns 503. The normal `tor` build retains
its original readiness gate. `debug-tor-image` is a separate explicit flake
package; the ordinary production `gateway-image` recipe is unchanged.

Preparation, mounted identity checks, official SDK bootstrap/launch, engine
status observation, ingress permits/bridge bounds, shared drain/JoinSet ownership,
trust/attestation/channel gates and accounting are untouched. Tor preparation,
bootstrap and launch failures still fail closed and can exit the process; observed
engine failure still invokes shared drain. No fallback, retry or pretend-success
behavior has been added. A need to keep an actually failed launch alive must be
handled separately and explicitly as debug-only, not silently changed here.

## Local evidence

Pinned flake Rust 1.92.0, offline/locked Cargo:

- Actual router health-policy regression passes with `tor` and with
  `debug-tor-no-health`: pending/not-ready, ready, readiness loss, quiescence.
- Debug library: 430 passed, one pre-existing ignored test; binary: six passed.
- All-target/all-feature strict Clippy and formatting pass. The existing vendored
  dependency dead-code warning remains unchanged.

These are synthetic/offline source checks, not publication, runtime readiness,
onion reachability, attested serving or billing evidence. Native-v3 signer and
legacy-v2 freshness limitations remain unresolved; independent per-request
verification remains required. Collateral availability is a separate boundary.

## Release/runtime evidence

[Hosted build 39145467147](https://github.com/ajbt200128/possums/actions/runs/39145467147),
attempt one, succeeded from exact debug source
`ebaf98473836c87dcd2bf3d498a216ca4de88687`. Hosted normal/debug router health,
health-probe tests, formatting, strict Clippy, pinned image build and archive /
registry digest equality all passed. Image digest:
`sha256:33eb3605cde1f84473ab521b8b587230447f64564dddc9fb7019edc77ece07a7`.
The downloaded actual image was loaded locally: Linux/amd64, UID/GID65532,
source label matching that commit and diagnostic label `tor-no-health-v53`.
An actual-image network-disabled/read-only/no-capabilities health probe correctly
failed with closed `connect_unknown` and exit1 when no listener existed.

The existing same-repository diagnostic release path commits that exact digest
and uses the official pinned measurement/signing action with
`mark-as-latest: false`; no current-main production dispatcher is used. This is
not an independently reproduced production release. Signed release, deployed
runtime and onion readiness remain pending.

Initial fresh selection: exact debug target/repository, five existing bindings,
current `v0.0.49-debug-keys.1`, stopped, no pending update. The operator authorized
restart after this was reported. No SSH or successful new debug deployment is
established yet. Before any further mutation, reselect state and preserve opaque
settings, references and toolbox wiring. Metadata is not physical-cleanup proof.
