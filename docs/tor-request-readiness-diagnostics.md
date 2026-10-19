# Request-only Tor readiness diagnostics

## Authority and limits

The operator selected **Prepare PR and protected rollout**: review and required
CI/provenance/protected approval precede deployment, then bounded public checks.
This supersedes the degraded-investigation deferral only for this scope. No debug
mode, paid inference, client installation/activation, key/reference changes,
automatic inference retries/replay, background diagnostic observer/FIFO/logs or
new telemetry/export is authorized. Preserve the accepted single-address risk
and all existing trust/accounting/release gates.

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

## Remaining rollout evidence

Independent final read-only source/SDK review found no concrete blockers after
panic-constraint wording and exact wire/evidence documentation corrections.
Required exact-head/main CI, reproducible release provenance, protected deployment
and bounded public diagnostics remain **PENDING**. Record only content-free reviewed observations, not raw SDK errors,
identifiers, private config or response archives. Neither successful deployment
nor a diagnostic response establishes restored Tor availability or completed
fresh client trust/challenge acceptance.
