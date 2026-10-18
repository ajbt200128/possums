# Arti background diagnostics and fully-reachable debug release

## Authorization and policy

The operator requested Arti's `is_fully_reachable()` health predicate and as much
non-request-specific Tor/Arti error information as can be safely exposed. They
explicitly selected **ongoing diagnostics** and a new signed deployment with
Honeycomb enablement to the **existing debug instance only**. No production/main
operation, inference, additional instance/repository, raw SDK logging, device
attestation probe or private-original key access is authorized by this packet.
The separate attestation investigation remains independent.

Applicable policy: [the debug-only Tor exception](../PRIVACY.md#debug-only-tor-background-diagnostics-exception),
[startup exception](../PRIVACY.md#startup-diagnostics-exception),
[data boundaries](../PRIVACY.md#data-handling-boundaries), and
[processors/retention](../PRIVACY.md#processors-retention-access-and-shutdown).
Honeycomb uses the already-approved destination and provider-default MVP retention;
effective retention remains unverified. Non-request-specific Tor lifecycle/error
information can still reveal availability and permit external timing correlation.
Native Honeycomb/platform/Pi/T3 retention remains separate from in-memory owners.

## Health predicate — tested source

[The health-only record](tor-fully-reachable-health-verification.md) documents
commit `dcdcb77`: SDK `is_fully_reachable() && !fatal`, with unchanged non-Tor
process/quiescence, fatal/EOF/panic/drain and client trust/accounting controls.
`Running` and `DegradedReachable` pass the Tor component; an SDK assessment is not
an independently verified connection or fresh attestation.600s debug grace remains.
The finite startup observer continues to record **first SDK Running**, not a
perpetual health result; its diagnostic deadline does not stop Tor or alter health.

## Background diagnostics — qualified source, not live evidence

The source candidate is debug-feature-only. It uses supported SDK bootstrap/onion
status, typed problem variants and status-source lifecycle, without request/peer
callbacks. A fixed closed projection includes onion state and SDK reachability,
coarse bootstrap progress/ready/blockage, selected problem and typed upload
retry/stage, introduction-point and runtime error categories. Inner SDK strings,
identifiers, paths, configuration, credentials and exact counts are excluded.
The public SDK exposes the selected most-severe problem, not every simultaneous
subsystem problem or per-directory success statistics.

The design reuses the existing sealed OTLP transport/resource helpers and
nonblocking platform emitter. One latest-state cell deduplicates/coalesces changes
with at most one attempt per60s, one-second owned delivery and no retries/spool/
detached upload. Disabled export constructs no new background encoder/TLS/network
and emits no new background records; existing finite startup-local behavior stays
unchanged. Primary failures and original bootstrap/drain behavior must be preserved.

### Offline qualification

Parent inspected the actual implementation and integrated SDK bootstrap/onion
watchers independently of request callbacks. The bootstrap observer polls the
**same single** original bootstrap future even if its status stream ends; ready
shutdown does not poll diagnostic/bootstrap work. Normal builds keep the existing
finite startup behavior. The debug worker remains owned through serving/drain,
without detached work or a guaranteed fatal-upload flush.

Pinned flake Rust1.92, Cargo offline/locked checks:

| Check | Result |
| --- | --- |
| Debug library |449 passed, one pre-existing operator-only fixture ignored |
| Debug binary |6 passed |
| All-feature full unit/binary/integration suites |629 passed, five pre-existing opt-in tests ignored |
| Normal Tor library |430 passed, one pre-existing operator-only fixture ignored |
| New background projection/synthetic-TLS tests |15 passed |
| Bootstrap stream-EOF/no-replay and ready-cancellation tests |2 passed |
| Formatting, strict all-target/all-feature Clippy, no-default check |Passed |
| Active LSP probe of eight changed Rust paths |No errors |

The15 tests exercise actual typed IPT/runtime/descriptor retry errors, mixed/empty
error vectors, hostile SDK message/path/bug strings, SDK reachability states,
unknown evidence before observation/after source loss, exact decoded OTLP
attributes/resources and empty trace/timestamp metadata. They also cover global
minute pacing, duplicate suppression,10000 coalesced updates in one fixed cell,
unchanged failed attempts not retried, OFF/configuration failure, hostile HTTP/
OTLP responses and actual owned local TLS/HTTP disposal without late writes.
Normal manual-bootstrap `Disabled` and initial Shutdown are informational, not
fabricated errors; Broken/unknown SDK state and unexpected source loss are errors.
A narrow final strict-lint fixture expression was corrected and strict Clippy,
15 background tests and2 bootstrap regressions rerun successfully. Existing vendor
dead-code warning remains.

`retry-error =0.14.0` is a dev-only direct dependency for constructing the SDK's
already-locked nested error type; the entire lock delta is one root dependency
entry, with no new package/version. Public SDK status components cannot be
constructed independently; projection tests use the actual public State/Problem
variants through the same production projector. RequestFailedError's inner
fixture and non-exhaustive directory-shutdown construction remain source-reviewed
gaps. We do not claim every hidden SDK internal branch was exercised or collect
raw SDK formatting to fill those gaps.

Independent Luna read-only review round1 found no blocking acceptance/privacy/
concurrency issue; it performed no tests or live operations. Parent retained
admission, checked actual diffs and ran the checks above. Source evidence does not
qualify Linux image/runtime behavior, actual Honeycomb ingestion or retention.
The pre-existing assertion-format change initially blocked clean-source formatting;
operator explicitly approved committing it separately as `be49a66`, with normal
formatting gates retained. No unrelated cleanup was folded into the feature.

No `/v1` authentication/catalog/request/stream/receipt or attestation wire fields
are changed by this diagnostic patch. Health semantics are the deliberate Arti
reachability change above; no client upgrade or decoder extension is required by
these new internal OTLP fields. The full existing offline integration suites pass;
no separately installed old-client artifact was exercised in this packet.

## Pre-signing preparation — historical read-only snapshot

Fresh exact debug readback still found health.2 running and health.3 failed pending,
with existing resources, promotion false, five bindings and opaque settings
unchanged. The first disable-override projection withheld a conclusion because
variables used an unqualified representation. The reviewed official CLI decoder
supports object or base64-encoded JSONB; applying that same decoding only in memory
confirmed the known `OTEL_SDK_DISABLED` override is **absent**. No variable/secret
values, private identifiers or raw metadata were printed or retained. Thus release
configuration can enable the existing switch without editing opaque overrides.
This is a preparation snapshot, not authority to skip fresh selection before the
one permitted later lifecycle attempt. Both previous consumed markers remain.

## Qualified debug image build, signing candidate

Implementation commit `609c26eccbb49f2df5d247a1ba2b53a99819d3be` was followed
by the workflow-only build source `aba880c786ac4b3cf7939b2167f706cd52ddf8fa`.
Build tag `v0.0.53-debug-tor-health.4-build`, hosted run39168974498/attempt1,
completed successfully. Parent independently checked exact repository, workflow,
source, event, tag, attempt and successful job plus the sole unexpired source-bound
artifact. Downloaded public archive inspection verified Linux/amd64, UID/GID65532,
source/diagnostic labels and the expected debug entrypoint.

Qualified image:
`sha256:e51654c571fffd9a43aa480185d2b40f148777d0147ce9025f8584f405381df5`.
This is artifact/configuration evidence, not initialized runtime or attestation.

The signing candidate is release-only: measured config changes exactly the image
digest and `OTEL_SDK_DISABLED` from true to false. All other bytes, including600s
grace, probe/interval/timeout/retries, resources, five secret bindings, read-only
root/tmpfs and attested-key controls, remain unchanged. The exact-build and
release-only-delta guard requires the pinned successful build and artifact before
measurement, with promotion/latest false and attempt1 only.

Additional offline release preparation used explicit locked nixpkgs Python3.12:
automation64 passed, cache12 passed/one skipped, rollout8 passed. The Rust dev shell
does not pin Python: an initial ambient3.15 alpha ZIP-fixture failure did not
qualify that tool. The two existing normal measured-health fixtures still expect
300s, so do not pass against this deliberately approved600s debug configuration;
they were not weakened. No claim that all Python tests pass. The debug path checks
the exact600s source/config separately.

Fresh read-only pre-signing selection again confirmed health.2 running/health.3
failed pending, existing resources/bindings/SSH/promotion and absent export-disable
override, with no instance mutation. Original attempt markers remain consumed;
the new build-dispatch marker was consumed once and no dispatch was replayed.

## Public signature and one completed debug deployment

Signed tag `v0.0.53-debug-tor-health.4` binds release
`b2b23eff83e8e4b1b74be05a83b4d5db887fe8f2`, hosted signing run39171162238/1,
and manifest `7c0ebcb8c8175295789caa531b153d6f171032055a1839ff0a2d6d46b5fd0ac7`.
Parent verified the public signature's exact workflow/tag/source/signer/hosted-run
identity, successful build/signing jobs, manifest subject/hash and decoded config.
This verifies public release provenance, **not device/runtime attestation**.

New scratch monitor controls preserved both earlier attempts and consumed a new
exclusive-create deployment marker. Parent inspected real diffs and ran39 pinned
offline tests plus shell syntax. Independent guard reviews caught two stale-evidence
admission paths: a failed final sample and a control read crossing the deadline
before its runtime sample. Both were fixed/tested. Final admission requires a valid
sample paired with the latest control observation, not cached earlier health.
Round3 found no remaining pairing blocker and independently passed the three
relevant regressions; its broader unpinned shell lacked timeout, so parent retained
the complete pinned39-test evidence rather than claiming that host suite passed.

Fresh exact target/settings/override, signature/archive, installed-document and
monitor-path gates passed before mutation. One minimal failed health.3 cancellation
was independently confirmed: health.2 still running, no pending update, settings
preserved. One fresh blue-green/no-hold/no-downtime plan then one health.4 update
were acknowledged, promotion false, resources/opaque references/five bindings/SSH
key references preserved. **Health.4 became current/running with no pending operation.** No uncertain
mutation was replayed and no subsequent cancellation/rollback was performed.
Physical cleanup and exclusive Tor publication remain unproved.

## Runtime checks — deployment succeeded, health is unstable

The bounded900s monitor verified the actual candidate's exact image/source/debug
labels, installed documents, typed600s grace/10s interval/5s timeout/three retries,
health command, read-only root/tmpfs and effective Tor-on/export-enabled switches.
Process samples remained running, exit0, no OOM. Local initialization completed.

**Healthy-to-unhealthy flapping was observed both before and after600s.** Docker
failure counts rose and recovered; local probes alternated `ready`/`not_ready`.
Unlike earlier unqualified attempts, this is direct evidence of a healthy/unhealthy
sequence for health.4, not a claim that historical health.3 flapped. A first healthy
check can end Docker's startup exemption;600s is not an unconditional immunity
period after early health success. The exact historical platform failure remains
unknown.

The finite590s diagnostic reported `degraded_reachable`/`descriptor_upload`, not
first SDK Running. It remained diagnostic-only under the new fully-reachable health
semantics. Final paired control/runtime sample was current health.4/running,
Docker healthy, local probe ready, exit0/no OOM. A separate fresh read-only postcheck
again observed those results with no pending operation/settings drift and effective
export enabled. Neither sample establishes sustained availability.

A bounded, source/identity/document-guarded platform projection (13 offline hostile-
input cases passed) returned the **last locally retained** closed background record:
SDK `degraded_reachable`, SDK-reachable true, bootstrap complete/ready, blockage
none, selected `descriptor_upload`, flags `upload_timeout` and `desc_circuit`,
selected runtime kind none. This identifies an upload deadline with a circuit error
among recorded causes, **not** the underlying circuit failure or a full subsystem
inventory. It is not current connection, fresh attestation or exclusive-publication
proof; coalescing can miss the states that caused individual health failures.

## Honeycomb evidence and remaining limits

Before rollout, the existing prod events dataset lacked the new background fields.
After rollout, metadata showed the new typed `diagnostic_profile`, reachability,
bootstrap/source/runtime and upload/IPT flag columns in the existing
`prod/possums-gateway` dataset. This is scoped **new-schema ingestion evidence**, not
an end-to-end query of event values, completeness or every transition. No raw event,
body or request-level records were downloaded. Available Honeycomb tools exposed
metadata/query-history/result reads, not new query execution; no matching saved
Tor query was found. Event-value/count querying therefore remains unqualified.
No query capability was bypassed and no new credential was requested or exported.

The shared switch also enables the existing approved aggregate/startup signals;
the shared destination/resource is not per-VM isolation or exclusive attribution.
Existing Honeycomb US/provider-default retention is accepted; actual retention/
access/platform capture properties remain unverified. Native host history is a
separate persistence boundary. No gateway inference, device-attestation probe,
production/main rollout, private-original key read or further lifecycle mutation
was performed. Current deployment checks succeeded at their sampled endpoints,
but **health flapping and the underlying circuit/upload cause remain unresolved**.
Further rollout/recovery or deeper diagnostic changes require a separate decision;
the one authorized deployment marker remains consumed.
