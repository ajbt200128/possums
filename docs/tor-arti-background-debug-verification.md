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

## Live preparation — read-only, no rollout yet

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

**Build/signing, deployment, actual SDK health and Honeycomb ingestion are pending.**
The current running instance has not been changed by the source work above.
