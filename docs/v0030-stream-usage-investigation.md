# v0.0.30 stream-usage rejection: reproduced fix

## Finding and scope

A fresh, operator-authorized synthetic DeepSeek request through the production
verified Tinfoil client and SDK consumer reproduced `stream_usage_invalid`.
Temporary, closed, content-free observations localized it to **usage trailer
multiplicity**. Those diagnostic changes were removed, not shipped.

The deployed-source validator rejected every repeated
`X-Tinfoil-Usage-Metrics` field, including byte-identical values. With only the
acceptance condition changed to tolerate byte-identical repetition, a fresh
synthetic request completed with validated terminal usage. The offline regression
also fails with the original condition and passes with the candidate.

This establishes a current live compatibility bug. Native user reports show output
followed by the same coarse error and an authenticated incomplete settlement, but
no original raw usage was retained. The historical incidents' exact rejected
values cannot be retrospectively established. Where in the serving stack the
repetition originates remains unknown; public router source alone is not a serving
identity or an explanation of the deployed transport behavior.

The patch addresses **only the stream-usage failure**. No catalog, intake-agent,
client installation, diagnostic protocol, dependency, retry, deployment or settings
changes are included.

## Smallest runtime change

`src/inference/forward.rs::metrics` accepts repeated HTTP field values only when
all bytes match the first value. It then validates and consumes that value once.
It does not sum snapshots or multiply the billable counts.

Everything else remains fail-closed:

- Different values, even apparently equivalent reordered fields, are rejected.
- Repeated keys within a value, oversized/malformed values, wrong/missing model,
  unsigned count shape, checked token sums, paired cache split and decimal money
  requirements are unchanged. Identically repeated malformed data is rejected.
- Multiple trailer frames, missing finish/final usage, invalid SSE, transport
  failure and actual EOF requirements are unchanged.
- The sole owner settles at the original quote, with the existing 30% ceil markup,
  cache arithmetic and reservation cap. Earlier wholly validated running evidence
  survives a rejected trailer; incomplete settlement does not authorize tools.
- Receipt schema, error codes/messages, default stream and optional downstream
  metrics contract are unchanged. A duplicate accepted upstream value becomes the
  same single allowlisted downstream summary as a single accepted upstream value.

## Verification

On Rust **1.88.0**, using only cached dependencies:

- **363 root Rust tests passed; four funded live tests remained ignored** in the
  full offline suite. Strict root clippy, package formatting and diff checks passed.
  The vendored SDK's existing dependency dead-code warning remains unchanged.
- `tests/support/native_usage_contract.rs` and
  `tests/support/duplicate_usage_metrics.rs` exercise **72 synthetic submissions**
  through the production native consumer, SDK decoder, API delivery and settlement
  owner. Assertions cover one receipt/settlement, conserved balance, original
  reservation/rates, sanitized trailers/errors and duplicate submission rejection.
- Identical fields repeated one/two/three times in initial headers and trailers
  pass. Conflicting count/model/cost/cache/order values in either order, a conflicting
  third value, and identically repeated malformed values fail. A rejected trailer
  keeps prior validated evidence rather than changing the charge or authorizing
  completion.
- Restoring the old multiplicity condition makes the new identical-repetition API
  regression fail; restoring the patch makes the full suite pass.
- The exact **shipped v0.0.30 decoder**, artifact SHA-256
  `4a2a4188490785efaac408dd7d67e9735464ff81b67cdc9b059a8f15551fab7c`, passed
  synthetic complete/incomplete settlement and interrupted EOF checks via
  `tests/shipped_usage_decoder.mjs`. Declaration bodies are extracted from the
  existing bundle without initializing its provider, authentication or network.
- A scratch-only current-source Pi/reference bundle passed
  `tests/tinfoil_forwarding_client.mjs`: grouping, cache/rounding/cap, provider-cost
  separation, paid partial tools, truncated receipt billing uncertainty and caller
  validation after confirmed settlement. An older cached fixture bundle was found
  stale and rejected the fixture; it is not counted as current-source qualification.

These client checks qualify the unchanged usage/receipt/error boundary, not an
entire old-client authentication/catalog lifecycle. No claim of compatibility for
all historical clients, installed-client activation or T3 rendering is made.

## Funded probe and privacy boundary

The operator authorized a **$2 total inference budget** and separately authorized
consuming an existing runtime credential inside the probe process without showing
or copying it. Two fresh fixed-input, 64-output-token generations were made:
pre-fix rejection and post-fix validated completion. A separate overly conservative
budget preflight stopped before generation. There was no replay or automatic retry,
no gateway account/receipt probe, and no additional agent or paid sweep.

The retained ignored canary
`production_consumer_validates_deepseek_usage_without_raw_reports` uses verified
catalog/tokenization, a fixed selected model without substitution, a 64-token
explicit output allowance and quoted maximum-cost preflight. It fails on a closed
error category or deadline; it never collects output or reports raw usage. Running
it again requires separate authorization and tracking the remaining total budget;
the per-call guard is not a cross-process spending ledger. Actual invoice charges,
provider token-limit enforcement and invoice parity were not independently verified.

Applicable policy: [data handling boundaries](../PRIVACY.md#data-handling-boundaries),
[forbidden telemetry](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[synthetic testing](../PRIVACY.md#synthetic-testing-exception). No credential, real
prompt/output/history, raw header/exception, per-request token/cost figure or native
thread/run/account identifier is recorded here. Detailed usage remained transient
inside the existing consumer/settlement boundary. No logging, telemetry exporter,
support bundle or persistence was added. Native Pi/T3 history is a separate host
boundary; this document does not claim whole-process transience.

This is a **locally tested source candidate, not a deployed gateway fix**. Live
runtime privacy, cancellation latency, invoice parity, legacy-v2 freshness and the
separate native CLI v3 signer mismatch remain unverified or unresolved. Merge,
installation and deployment require separate authorization.
