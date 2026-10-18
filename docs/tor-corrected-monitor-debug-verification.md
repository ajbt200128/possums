# Corrected-monitor Tor-health debug attempt

## Scope and authorization

The operator separately authorized another deployment/check/SSH attempt after the
[previous monitored failure](tor-monitored-debug-verification.md). This record
covers **one new attempt** against the existing `possums-debug` instance on
`debug/tor-no-health-gate-v53`, not an automatic replay. No production/main change,
new instance, inference, device-attestation acquisition or Tor reachability probe
was performed. The separate attestation investigation remains independent.

Applicable policy: [PRIVACY.md](../PRIVACY.md), especially the
[startup-only diagnostic exception](../PRIVACY.md#startup-diagnostics-exception),
and the diagnostic/privacy boundaries in [AGENTS.md](../AGENTS.md). Only closed
projected categories and public source/artifact identity appear here. Runtime
logs, Docker/control metadata, configuration, credentials, identifiers and SSH
subprocess details were handled in memory, not retained as diagnostic files.
Native Pi/T3 history, existing control-plane retention and OpenSSH known-host
storage remain separate boundaries; this is not a whole-process transience claim.

## Offline observer qualification and immutable release

The parent reviewed the actual correction, preserved the original scripts and
consumed marker, reran **34 pinned offline tests** successfully with `nix develop`,
and checked shell syntax. Fixtures exercised the actual shell with synthetic
Docker Duration Stringer rendering, wrong typed durations, each closed guard,
hostile/partial output, missing records, exact identity, preserved primary failures
and no replay. This offline evidence alone was not runtime qualification.

A new scratch entry point differed from the reviewed control script only in its
current verification-commit pin (`3e3df31`) and **distinct consume-once marker**.
The old marker was not removed or reused. A byte-for-byte expected-delta check and
syntax compilation passed before execution. Both markers remain consumed.

The same signed health.3 release was used; no gateway code/configuration was
changed:

- Tag: `v0.0.53-debug-tor-health.3`.
- Source: `e964d1cbfc2218192d62b9b080dfe3cc63dca779`.
- Release: `c5696be5068083b3cbb6a5bd3ddc50c613d91e01`.
- Image: `sha256:8667405e5469618437259700a06c63bc73d540ac07d5ef5a65e7a325bf715491`.
- Manifest: `f1f4f78e8ef5a5467ee4d2c1e4b2b49d757bdc74f96ac3db445e8b9fefb6ca18`.

The signed artifact gate passed again. Installed current-toolbox document hashes
and supported observation-path preflight passed before mutation; candidate access
was explicitly not guaranteed by that preflight. The existing formatting-only
`src/server/tor_tests.rs` working-tree change was not staged or deployed.

## Controlled lifecycle

Fresh exact-target state/settings checks and current public artifact checks
preceded one minimal failed-stage cancellation with rollback-latest false. Its
command returned zero; independent metadata confirmed the original source still
running, no pending candidate, and preserved settings. Metadata cleanup does not
prove physical VM/key cleanup or exclusive onion publication.

A fresh plan preserved all five bindings, opaque references, resources and SSH,
with blue-green true, downtime/hold false and promotion false. **One** update was
submitted and acknowledged. Monitoring began from acknowledgment, bounded to900s,
60 control observations,61 candidate SSH calls and35s in-flight reads. No uncertain
mutation was retried. Neither marker can authorize another attempt.

## Captured candidate trace

Unlike the previous attempt, this run obtained **validated actual health.3 runtime
samples**. Candidate installed README/AGENTS were read and verified before gateway
inspection. Every accepted sample gated exact gateway selection, image/source,
container stability, typed600s start period,10s interval,5s timeout,3 retries,
expected health command, read-only root/tmpfs and existing diagnostic switches.
Early SSH delivery failures and the final SSH failure remained coverage gaps.

| Observed stage | Closed evidence |
| --- | --- |
| Platform start | Pending → started; health.2 remained current/running. |
| Before grace expiry | Container running, exit0, OOM false; Docker health `starting`, failure count zero. Initial local probe `deadline_exceeded`, subsequently `not_ready`. |
| Gateway initialization | Configuration/accounts/signal/Tor preparation/connect/bind/bootstrap/launch stages observed; initialization complete. |
| Early SDK observation | `waiting`, state `bootstrapping`, problem `none`, observer elapsed30–under60s. |
| Observer diagnostic deadline | `readiness_timeout`, state **`degraded_reachable`**, problem **`descriptor_upload`**, observer elapsed590–under600s. This is observer time, not an exact Docker-start timestamp. The corresponding near-boundary Docker clock projection was withheld as `unavailable`. |
| After Docker grace expiry | Still running/exit0/OOM false, Docker health `starting`, failure count **one or two**; no retained successful check observed. |
| Platform terminal state | Candidate failed; health.2 remained current/running. Following candidate SSH sample returned255. |

The first locally observed primary diagnostic was `READINESS_DEADLINE`; subsequent
`CANDIDATE_FAILED` and transport loss did not erase it. That ordering does **not**
make the diagnostic deadline an authoritative platform failure reason: it is a
nonterminal observer report and does not stop Tor or change health state.

Final observer summary: runtime sample observed true, coverage gaps true,
`healthy_not_observed_in_samples`, `never_healthy_proven=false`, no replay true.
We did **not** capture Docker's transition to `unhealthy`, a third counted failure,
process exit or the platform's underlying failure detail. Final independent native
read-only status confirmed failed pending state and unchanged opaque settings.
No later cancellation/update/rollback or SSH retry followed.

## Interpretation and remaining decision

The strict health gate requires current SDK **`Running && !fatal`**. At the captured
deadline, `DegradedReachable` therefore correctly produced non-ready health under
the current implementation. The container did not appear to have crashed or OOMed
in accepted samples. Effective600s grace was validated; the observed zero failures
before grace and counted failures afterward are consistent with its operation.
This attempt supports failure to attain strict readiness, **not** an observed early
healthy→degraded flap. Sampling gaps still prevent a global never-healthy claim.

Pinned `tor-hsservice`0.47.0 source provides important context:
`publish/reactor.rs` around2040–2081 returns `DegradedReachable` when descriptor
uploads succeeded to at least one directory in **each** primary/secondary ring,
but some other uploads failed. It returns publisher `Running` when both rings have
success and none failed. `status.rs::OnionServiceStatus::state` combines publisher
and introduction-point states; `Problem::DescriptorUpload` means one or more
uploads failed. These supported SDK distinctions explain why a service considered
degraded-but-reachable by the SDK can fail our stricter health predicate.

This is not an independent connection to the candidate onion, proof of exclusive
candidate publication, or proof of fresh attestation. We did not collect the inner
upload-error subtype, so timeout, directory rejection, connectivity or another
cause cannot be selected. Shared-key concurrent publication and platform behavior
remain unproved, not established causes. The exact final platform failure cause
also remains unavailable, although failed local readiness and post-grace counted
failures are now directly observed.

The next decision is whether to investigate the descriptor-upload failures further
or deliberately change Tor health to accept the SDK's degraded-but-reachable state.
Neither policy change nor another deployment is performed here. Fresh nonce-bound
attestation, endpoint binding and existing trust/accounting controls remain intact.
