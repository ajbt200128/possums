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
not an independently reproduced production release.

[Signed release run 39146916679](https://github.com/ajbt200128/possums/actions/runs/39146916679),
attempt one, succeeded for tag `v0.0.53-debug-tor-health.1`, release commit
`82b8ff523c10a1c3c1727a9122e2855a45b4ebec`. The exact-source/image/config gate
passed before the official pinned measurement/signing action. Downloaded public
manifest/hash were checked with the existing repository manifest validator;
`gh attestation verify` cryptographically verified the exact tagged
`tinfoil-release.yml` signer, source/signer commit, predicate type and hosted
runner. The verified statement's predicate and subject equal the checked
manifest and its SHA256, and its invocation is that exact run/attempt.
Manifest SHA256:
`32f8a1c1190559c43feb120fa2c71e13d8affd02ec768b510d14007e5714a12b`.
No production dispatcher or promotion was requested.

Initial fresh selection: exact debug target/repository, five existing bindings,
current `v0.0.49-debug-keys.1`, stopped, no pending update. The operator authorized
restart after this was reported. Subsequent fresh checks still selected that
same stopped target, no pending update and five existing bindings. The known
`mark_latest_release` setting is true; a debug deployment must explicitly decline
promotion rather than promote this experiment. No successful setting change is
established.

The pinned native CLI start attempt exited1 without a safe exposed cause/status.
It was not replayed. After signed release verification, one distinct new-tag
start request to the CLI's documented `/start` boundary, using the observed
legacy `mark_latest_release: false` field, returned HTTP404, with no successful
start acknowledgement. One read-only `/update/plan` for the candidate returned
HTTP409. Final read-only metadata still shows the old tag, stopped, no pending
update. Neither response establishes physical cleanup or publication exclusivity.
No other lifecycle mutation, cancellation, rollback, vault change or production
operation was attempted.

**External blocker at that attempt:** the existing debug instance could not be started
through the pinned client's documented lifecycle endpoint, while the existing
update-plan endpoint refuses the stopped state. The underlying control-plane
cause is unknown; HTTP404 alone does not distinguish unavailable route from
resource/admission concealment. No guessed mutation route, new instance,
credential-scope upgrade or protected-admission bypass was attempted. A supported
platform restart of this exact existing instance (or correction of that API
boundary) was needed before deployment/SSH could continue. No toolbox docs could
be reread remotely at that point because no SSH session was established.

At that stopping point no successful new debug deployment, runtime Tor startup
or SSH inspection was established. The operator deployment and subsequent
observations below supersede that operational blocker, not the historical
HTTP responses. Before further mutation reselect exact state and preserve opaque
settings, references and toolbox wiring. Metadata is not physical-cleanup proof.

## Operator deployment and SSH observation

The operator subsequently reported deploying the candidate. Fresh pinned
native/Admin selection confirmed the exact existing debug target/repository,
`debug=true`, current `v0.0.53-debug-tor-health.1`, running, no pending update,
five managed bindings. The agent did not submit another deployment/start request.

SSH succeeded using freshly selected metadata, the existing operator key,
`StrictHostKeyChecking=accept-new` (never replace changed keys), no forwarding,
and bounded noninteractive commands. Installed toolbox README and AGENTS were
read fully before workload inspection. The toolbox remained untouched. Each SSH
operation compared opaque settings and references before/after in memory; no
unexpected pending update or setting/reference change was observed.

Actual runtime checks, projected on the VM before SSH output:

- Gateway image exactly matches the signed debug digest above, source label
  `ebaf98473836c87dcd2bf3d498a216ca4de88687`, diagnostic label
  `tor-no-health-v53`.
- Gateway running, Docker healthy, exit0, no observed OOM. Fixed credential-free
  `/bin/possums --healthcheck` succeeded (HTTP204).
- Tor enabled; `OTEL_SDK_DISABLED=true`. Only exact boolean matches were returned,
  not environment/configuration contents.
- Existing healthcheck retained: 300s grace, 10s interval, 5s timeout, three
  retries. Root read-only; /tmp512MiB. No timeout lengthening or healthcheck removal.
- Nine closed startup milestones included preparation, bootstrap, launch and
  local initialization completion. By the unchanged owner ordering, mounted
  identity checks and the official `client.bootstrap().await` returned before
  service launch. This does not prove descriptor publication or attested serving.
- Three bounded observation commands read only the existing locally authored
  startup/readiness records. One deliberately waited150s before its single
  observation; there was no retry/reboot, new logger or diagnostic exporter.

The complete finite Tor readiness handoff observed:

| Coarse elapsed bucket | Outcome | SDK onion state | Problem kind |
| --- | --- | --- | --- |
| 30s to under60s | waiting | bootstrapping | none |
| 180s or more | unconfirmed | bootstrapping | none |

At the terminal snapshot the gateway remained Docker healthy and the direct
healthcheck still returned204. This is actual-image/runtime evidence that Tor
unreadiness no longer gates debug container health. No additional process-exit
relaxation was needed for this observed run. The two-record observer ends after
180s; it cannot establish the service's current status later, or establish that
Running was never reached after that window.

### What Tor is waiting on, and the remaining SDK boundary

The observed wait is **after successful directory bootstrap and local onion
service launch, before the SDK reports fully established onion service state**.
It is not the earlier preparation/self-appraisal gate. No attestation probe was
made and no collateral failure is inferred from these observations.

Pinned official `tor-hsservice0.47.0/src/status.rs` keeps IPT-manager and publisher
component states private. Its supported status API exposes only the combined
`state()` and selected `current_problem()` (plus an unrelated unimplemented key
expiry accessor). In `publish/reactor.rs:1165–1182`, AwaitingIpts, UploadScheduled
and RateLimited all project to Bootstrapping with no problem. Therefore these
actual closed snapshots cannot distinguish introduction-point establishment,
descriptor upload scheduling/retry/rate limiting, or an unreported/stale internal
failure. `problem_kind=none` is not proof that the network or publisher is healthy.
Effective egress restrictions and the precise blocking cause remain unknown.

The smallest useful next diagnostic boundary is an upstream-supported closed
projection of IPT component state, publisher phase and terminal failure category
at their existing typed owners. No addresses, peer/onion IDs, raw errors or
formatted Debug values should enter that projection. This run did not fork the
SDK, scrape its Debug formatting, enable raw Arti/global logs, inspect private
key bytes, test onion publication by contacting the identity, or weaken any
client/per-request verification. A further app-only formatting change cannot
recover component fields the SDK does not expose.

No production operation, configuration edit, boot/restart, new container, key
provisioning, paid/live inference or new attestation probe was performed in this
SSH follow-up. Publication exclusivity, successful onion reachability, native-v3
signer compatibility and platform retention/erasure remain unproved.
