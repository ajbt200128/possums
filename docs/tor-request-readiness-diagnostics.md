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
protected deployment and bounded public checks.

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

Exact-head/main CI, reproducible provenance, protected rollout and bounded public
checks remain **PENDING**. Record only content-free reviewed observations, not
raw SDK errors, identifiers, private config or response archives. Neither
successful deployment nor a diagnostic response establishes restored Tor
availability or completed fresh client trust/challenge acceptance.
