# Request-only Tor readiness diagnostics

## Authority and limits

The operator selected **Prepare PR and protected rollout**: review and required
CI/provenance/protected approval precede deployment, then bounded public checks.
This supersedes the degraded-investigation deferral only for this scope. No debug
mode, paid inference, client installation/activation, key/reference changes,
automatic inference retries/replay, background diagnostic observer/FIFO/logs or
new telemetry/export is authorized. Preserve the accepted single-address risk
and all existing trust/accounting/release gates. Following the v0.0.63 rollout,
the operator again selected **Prepare PR and protected rollout** for the minimal
timeout-source refinement described below; the same gates precede one new
protected deployment and bounded public checks. The operator subsequently
requested maximum useful already available typed evidence for circuit acquisition,
directory-stream opening, upload requests and related supported causes, **no Arti
patch**, then deployment under the same gates. This authorizes the explicit rich
view below, not raw logs, changed retry/health policy or extra production updates.

Applicable policy: [current authority](../AGENTS.md#current-tor-source-restoration-authority),
[data handling](../PRIVACY.md#data-handling-boundaries),
[forbidden telemetry](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[typed failures](../PRIVACY.md#mandatory-typed-interface-failures),
[request-only exception](../PRIVACY.md#request-only-readiness-diagnostics-exception)
and the separate [startup exception](../PRIVACY.md#startup-diagnostics-exception).

## Question and prior evidence

Production v0.0.62 was current/running with no pending update, but validated
clearnet and onion TLS health observations returned 503. Both curl and the
production native client also experienced SOCKS connection failures on one
isolated proxy. Container inventory showed one running and one stopped logical
repository object; it does not establish physical publisher cleanup/exclusivity.

Independent lifecycle review found no concrete ownership/polling/shutdown bug.
Pinned Arti 0.47 documents that false `is_fully_reachable()` does not establish
unreachability; normal descriptor republishing can report Bootstrapping. The
added Running → Bootstrapping → Running regression preserves serving while
health changes 204 → 503 → 204. **The routing/publication root cause remains
UNCONFIRMED. This patch is diagnostic, not an availability fix.**

## Response contract and non-interference

Only when Tor is enabled, credential-free `GET /healthz/diagnostics` returns
HTTP 200 when the existing local readiness/lifecycle predicates pass, otherwise
503, with `Cache-Control: no-store`. Exactly eight JSON fields are returned:
`code`, `stage`, `constraint`, `action`, `lifecycle_serving`, `ready`, `tor_state`
and `tor_problem`. The two booleans distinguish local lifecycle state from the
combined readiness decision. Other fields are closed locally authored strings.
That default response remains unchanged. Only the exact literal query
`?detail=descriptor` opts into one additional `descriptor_evidence` object; other
queries, including extra/unknown parameters, retain the default contract without
reflecting query values. Both views use the same readiness/status/no-store rules.

Existing `/healthz` statuses and empty bodies, serving/drain ownership, startup
and all shipped authentication/catalog/inference/stream/receipt contracts remain
unchanged. The additive route requires no client upgrade. It sits outside
application auth/admission/evidence/request-observation middleware, like health;
a request performs no appraisal, catalog/inference call, probe, retry or SDK
status poll. No metric name/dimension, logger, exporter or configuration changes.

The existing operational engine status owner replaces its readiness boolean with
one coherent atomic word containing the same predicate and fixed state/problem
projection. Raw SDK errors, formatter/panic payloads, config, identities, keys,
URLs, request data, timestamps, counters and history never enter that snapshot.
SDK descriptor errors select one category: internal, fatal, unknown, timeout,
retry exhaustion, then unspecified upload failure. This is diagnostic priority,
not a changed runtime fatality/retry policy: only SDK `Problem::Runtime` remains
fatal alongside the original terminal-state rules. EOF or caught stream-poll
unwinding clears readiness, retains the last closed problem and reports unknown
underlying cause; it does not erase a known primary problem.

State labels cover the seven pinned SDK states plus `not_observed`,
`status_stream_ended`, `status_stream_interrupted` and `unknown`. Problem labels
are `none_observed`, `runtime`, `descriptor_upload`, `introduction_point`,
`descriptor_fatal`, `descriptor_retry_exhausted`, `descriptor_timeout`,
`descriptor_internal` and `unknown`. No unreported problem is fabricated.

### Descriptor-timeout source refinement candidate

Only the selected outer `descriptor_timeout` classification is refined. Existing
outer priority and all other categories remain unchanged. The candidate adds
seven closed values:

- `descriptor_timeout_circuit`
- `descriptor_timeout_stream`
- `descriptor_timeout_request`
- `descriptor_timeout_internal`
- `descriptor_timeout_mixed`
- `descriptor_timeout_no_recorded_source`
- `descriptor_timeout_unknown`

The existing owner classifies only public typed `DescUploadError` variants in
already retained `RetryError::sources()` values. Equal classes remain one class;
different classes across timeout failures yield `mixed`. No raw source, count,
order, timestamp or history is stored or returned. Fatal/internal outer failures
still outrank timeout; an inner source never changes runtime fatality. Sixteen
fixed problem values fit the existing atomic representation without changing its
readiness bit or adding another snapshot field.

Pinned Arti's timeout is a per-HSDir **five-minute overall upload retry budget**;
reporting can occur later while a retry delay finishes. Individual-attempt
timeouts are not recorded as typed sources. `no_recorded_source` therefore means
only that the SDK retained no completed typed source in those timeout values,
not that no attempt failed. Nonempty sources describe earlier completed failures,
not necessarily the operation interrupted last or the cause of a later SOCKS
failure. `unknown` covers an unsupported future SDK source variant. The legacy
bare `descriptor_timeout` label does not establish that sources were absent;
older diagnostic responses did not classify them.

These are diagnostic-only enum additions, not changes to shipped Pi
authentication, attestation, catalog, inference, stream, accounting or receipt
contracts. Strict manual diagnostic allowlists need the candidate's closed
catalog to interpret its new labels; unknown labels must not authorize a private
request or be treated as success. No compatibility with an old manual diagnostic
allowlist is claimed. Exactly eight response fields and all existing health,
readiness, shutdown, retry and trust semantics remain unchanged.

SDK events can coalesce; `current_problem()` exposes one prioritized, possibly
last problem, not all causes. Snapshot age is unknown. These fields prove neither
freshness, liveness, publisher attribution/exclusivity, end-to-end serving nor
backend authenticity, billing or tool authorization. Private requests retain all
independent verification gates. The public response exposes coarse process
state; repeated callers can correlate transitions. `no-store` cannot prevent
capture or prove erasure. Platform/proxy retention and native Pi/T3 history are
separate unknown boundaries. No automatic support archive is created.

## Typed descriptor evidence candidate

This candidate uses existing public Arti 0.47 typed APIs, not dependency-source
patches, tracing or a new observer. The gateway embeds Cargo crates; changing a
separate Nix `arti`/C Tor package would not change this SDK. Any necessary direct
type/trait dependencies reuse the existing locked versions and Tor-only features.

The opt-in view preserves the default eight fields and adds only
`descriptor_evidence`. Its fixed schema distinguishes an unobserved snapshot,
a different selected problem, and selected descriptor-upload evidence. It reports
sets of outer retry outcomes and retained source categories, with independent
circuit, directory-stream and request-origin buckets. Supported nested typed
causes may distinguish circuit-build versus circuit-request timeout, no path,
channel/protocol/guard/state/internal failures, directory timeout, truncated
headers, parser/encoding/size rejection, I/O kind, HTTP rejection or clock skew.
Valid observed HTTP status values are limited to 100–599; reason phrases and
invalid values never escape. Labels come from reviewed local catalogs, not error
formatting. Related internal/unknown evidence cannot invent phase attribution.

No source object, relay/onion/circuit association, formatted error, content,
operation string, size, OS error number, timestamp, attempt count or ordering is
retained or returned. Unique category presence is not an error/attempt count.
Nested traversal has fixed depth/work bounds and closed coverage-limit flags;
unsupported or incomplete evidence cannot be interpreted as healthy absence.
Sets describe the selected SDK batch, not all components or prior history.

Arti's `publish/backoff.rs` discards individual wrapper-timeout errors and returns
previously retained completed errors when the overall budget expires. The active
upload spans circuit acquisition, directory-stream creation and request/response
processing. Its interrupted phase is unavailable through these APIs. This patch
must not claim to reconstruct it or confuse source origin with the last phase.
`current_problem()` prioritizes components rather than newest chronology; SDK
watch updates can coalesce and snapshot age remains unknown.

Keep the existing atomic readiness word/default reader. A small fixed evidence
snapshot is synchronized with every owner update; the opt-in reader holds that
synchronization while reading readiness and evidence together. No SDK work or
await occurs under the lock. Stream EOF/poll-unwind clears readiness but preserves
last known problem/evidence; subsequent SDK status replaces/clears evidence. No
history, health/fatality/retry/timer/drain change or diagnostic-triggered work.

Compatibility: the existing default strict eight-field decoder remains valid;
only an explicit new reader accepts the nine-field rich view. Private Pi/client
attestation, catalog, inference, stream and accounting contracts are untouched.
Tests must cover actual SDK hostile fixtures, every reachable mapping, mixed and
empty sets, nested limits/unknowns, deterministic replacement and concurrent
coherence, original ordinal/readiness behavior, finite worst-case serialization,
actual Axum opt-in/default/hostile query handling/no-store and no private work.

The rich object has exactly seven fields: `selection`, `outcomes`, `coverage`,
`circuit`, `stream`, `request` and `unphased`. Each bucket has the same four
fixed sets: `variants`, `kinds`, `io_kinds` and `http_statuses`. Selection separates
`no_snapshot`, `no_problem_selected`, `other_problem_selected` and
`descriptor_upload_selected`. Coverage includes retained-only, empty/absent source,
nested absence, unsupported variant/kind/I/O kind, and depth/node-limit labels.
Depth is bounded at 8 and total visited typed nodes at 128, with limit flags rather
than silent truncation. Snapshot storage is at most 512 bytes; the intentionally
unreachable all-flags/all-statuses serialization over-approximation is below
32 KiB. The default eight-field response has not acquired these fields.

**Local source qualification:** the parent inspected the actual implementation
and independently ran the already-installed exact Nix-store Rust/Cargo **1.92.0**
with `--offline --locked`: **615 passed, 5 ignored** for all targets/all features;
**531 passed, 4 ignored** without default features. Package formatting and strict
all-target/all-feature Clippy passed. Pinned Python 3.12 release/image suites:
**100 tests, zero failures, one existing skip**. Active seven-path LSP found no
errors/warnings or inconclusive paths; one unrelated informational refactoring
suggestion was not applied. Independent Luna source review found no concrete
security/privacy/concurrency/correctness blockers; it did not execute the tests.

Actual SDK fixtures exercise all request structures, constructible circuit
structures, nested constituent kinds, all projected kind and I/O labels, every
u16 HTTP-status input, hostile payload exclusion, phase separation, duplicate/
permutation behavior, limits, replacement/clearing, poison recovery, coherent
concurrent snapshots and operational EOF/poll-unwind. Actual Tor-enabled Axum
checks cover both exact response shapes, literal opt-in/hostile query defaults,
no-store/status/quiescing/disabled-route behavior and zero evidence/catalog/
tokenizer/document/inference work. Some circuit/source-identity structures lack
direct fixtures; their ignored payload paths were source-reviewed. Future SDK
variants cannot be constructed: only local unknown fallback paths are exercised.

**Environment limit:** recreating `nix develop --offline --no-write-lock-file`
initially failed before Cargo on an unavailable legacy `bmake` build source. The
exact build-tool outputs were restored from the trusted signed cache and their
content/signatures verified, without pin/profile changes. Shell recreation then
attempted a large dependency bootstrap and exceeded its bounded time budget,
still before Cargo. The successful direct pinned-toolchain tests above are not
a successful recreated-dev-shell check. Exact Linux Nix CI/image qualification
remains mandatory and **PENDING**, along with signed reproducible provenance,
protected deployment and bounded public rich-view verification. No Arti source
patch, dependency version upgrade, runtime configuration change, inference or
candidate deployment occurred. This is diagnostic evidence, not an availability
fix or fresh client trust/inference acceptance.

## Offline evidence

Baseline: main `88b20b7090147f0d913b6ea23da86732c685ab3a`; candidate revision is
identified by the PR head. Pinned `nix develop --offline`, Rust/Cargo 1.92; Cargo
uses `--offline --locked`. No live/funded tests or export probes ran.

- `cargo test --all-targets --all-features`: **597 passed, 5 ignored**.
- `cargo test --all-targets --no-default-features`: **531 passed, 4 ignored**.
- `cargo clippy --all-targets --all-features -- -D warnings`, package-scoped
  `cargo fmt -p possums --check` and whitespace checks passed. Existing vendored
  unused-method warning remains; vendored formatting was not changed.
- Pinned Python 3.12.12 discovery: **100 run, zero failures, one existing skip**.
- Six changed Rust paths actively LSP-checked: no errors/warnings/inconclusive
  results; one unrelated informational refactoring suggestion was not applied.

Focused fixtures exercise every pinned SDK state and unchanged readiness,
typed hostile SDK error projection, fixed/size-bounded JSON keys, unknown
fallback, concurrent coherent snapshots, last-problem preservation, actual engine
EOF/panic and Running → Bootstrapping → Running. Actual Axum tests verify
credential-free/no-store responses, hostile query/header/body exclusion, empty
legacy health bodies, quiescing, disabled-route absence and zero evidence,
catalog, tokenizer, document or inference calls. The exact pinned existing
readiness predicate and legacy route are exercised directly; no installed-client
upgrade/artifact qualification is claimed. Existing telemetry-off/outage tests
run in the full suite; no new metrics/export hooks exist.

## Initial v0.0.63 rollout evidence

PR101 merged at `059629624794c0ab5f541533da730aeae9f9e48c` after exact-head CI
39279271750. Exact-main CI 39286316590 and publisher 39292325558 passed. Public
signed release provenance was independently verified before one protected
approval for deployment run 39295070153, attempt 1. The exact one-update completion
receipt and separate bound-serving receipt passed independent checks. Current
control-plane readback showed v0.0.63 running/no pending update, confidential mode
on, debug off and the approved domain/configuration/reference selections.

One strict-TLS public clearnet diagnostic qualified HTTP 200, no-store and the
closed schema, reporting local readiness and a descriptor-timeout problem in
`degraded_reachable`. A subsequent owned isolated Tor client bootstrapped, but
its single health HEAD and single diagnostic GET both failed during the SOCKS
handshake, before TLS or HTTP. No diagnostic schema/headers over Tor were
qualified; no retry, fallback, private request or inference was performed. Owned
process/controller/temp-state cleanup completed, not proof of erasure. The
underlying cause and fresh client trust/challenge acceptance remain unconfirmed.

## Refinement qualification and rollout

Baseline: exact main `059629624794c0ab5f541533da730aeae9f9e48c`; identify the
candidate by its reviewed PR head. Pinned Rust/Cargo 1.92 qualification used
`nix develop --offline` and Cargo `--offline --locked`:

- All targets/all features: **603 passed, 5 ignored**.
- All targets/no default features: **531 passed, 4 ignored**.
- Strict all-target/all-feature Clippy, package formatting and whitespace passed;
  only the existing vendored unused-method warning remains.
- Pinned Python 3.12 release/image suites: **100 run, zero failures, one existing
  skip**.
- Active LSP checks of the three changed Rust paths: no warnings/errors and no
  unsupported, failed or inconclusive checks.
- Independent read-only review found no concrete blockers and executed the
  focused Tor-enabled actual-route regression. A prior zero-selected-test run
  without Tor features is not test evidence.

Real pinned SDK fixtures cover circuit, stream, request, internal, mixed and
empty retained sources; source/row permutations, outer priority, all sixteen
ordinals/readiness bits, concurrent snapshots and actual engine EOF/poll unwind
are exercised. The actual Axum route excludes hostile source/context payloads,
returns only the existing eight bounded fields/no-store, preserves empty legacy
health responses and initiates no evidence, catalog or inference work. A future
non-exhaustive SDK source variant cannot be constructed with the pinned SDK;
only its local unknown/mixed fallback is tested. Three dev-only SDK dependencies
reuse already locked 0.47.0 packages; lockfile changes are root edges only, not
runtime upgrades or configuration changes.

That timeout-source refinement subsequently shipped as **v0.0.64**, from source
`b4890cb34011d44cfaf3942e677581c308d4f9e6`, release-only commit
`285e14b6e65b19d81487e401bc9251fa62548b34`. Exact-main CI 39312285393 and publisher
39315597438 passed. Existing public signature/build verification passed before one
protected approval for run 39318276378, attempt 1. The exact single-update completion
receipt, separate bound-serving receipt and running/no-pending readback passed
independent parent checks. One bounded strict-TLS clearnet diagnostic returned
HTTP 200/no-store/closed schema, locally ready with `degraded_reachable` and
`descriptor_timeout_mixed`. The constituent categories were not available in that
view. A subsequent explicitly authorized isolated SOCKS-only check reported
`proxy_reported_ttl_expired`; no onion TLS/HTTP/application bytes or inference.
These are separate observations, not a demonstrated causal relationship.

The richer typed-evidence candidate above still requires its own review,
qualification, exact CI/provenance, protected approval and one new deployment.
Record only content-free reviewed conclusions, not raw SDK errors, identifiers,
private config or response archives. Neither deployment nor a diagnostic response
establishes restored Tor availability or completed fresh client trust/challenge.
