# Tokenizer send failures — 2026-10-15

## Scope and privacy

Operator requested read-only aggregate-metric investigation, then more specific tokenizer failures. This is a gateway source candidate, not a deployment, client installation or new live probe. Apply [typed error aggregates](../PRIVACY.md#typed-interface-failures-and-token-aggregate-extension), [forbidden signals](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data), [complete aggregation](../PRIVACY.md#aggregation-is-necessary-not-sufficient), [processor/retention limits](../PRIVACY.md#processors-retention-access-and-shutdown) and [required evidence](../PRIVACY.md#required-evidence-for-every-telemetry-change).

## Read-only ingestion evidence

Honeycomb confirmed `prod/metrics` is a metrics dataset. Attribute discovery for `possums.interface.errors` returned only `error_type` plus the reviewed fixed production service/environment/slot resources. A [300-second aggregate query](https://ui.honeycomb.io/possums/environments/prod/datasets/metrics/result/76F1EEJY4Jc), 2026-10-14 23:00 through 2026-10-15 01:30 UTC, returned `route_refused=12` and `catalog_acquisition_unknown=4`. No tokenizer series was returned. These observations establish error-family ingestion, not which release is serving, complete coverage, request attribution, root cause, or zero tokenizer errors. No schema/board/export configuration was changed. An initial shorter-range query was automatically interpolated to 120-second buckets; its fractional sums were not treated as exact failure counts. No request-level records were queried or retained.

This later ingestion evidence does not retroactively qualify the source-only activation entries in [verification](verification.md#typed-errortoken-release-and-activation-follow-up). Token-family ingestion, processor retention and broad runtime coverage were not requalified here.

## Closed source refinement

`send_tokenizer` uses supported SDK transport predicates, never formatted errors or exception-chain text:

- Outer send deadline expiry or SDK `is_timeout()` → `TokenizerSendDeadline`, aggregate `tokenizer_send_deadline`.
- Otherwise SDK `is_connect()` → `TokenizerConnectFailed`, aggregate `tokenizer_connect_failed`. This does not identify DNS, TLS or another underlying connection cause.
- Otherwise → existing `TokenizerSendFailed`, aggregate `tokenizer_transport_failed`, explicitly cause unknown.

The existing sole preflight failure owner counts once before returning the rejection; propagation and API presentation do not recount. Request encoding, HTTP rejection, response validation and upload-release categories remain separate and unchanged. No tokenizer input, request URL, credential, headers, response/error body or raw error is retained in the new enum/projection.

Both new source variants serialize to the existing **`tokenizer_send_failed`** wire detail. Default HTTP 503, `unavailable`, the existing four error fields and billing `unknown` remain unchanged. The bounded gateway `message` now identifies the finer closed cause and action. For example: `[tokenizer_send_deadline] Stage: tokenizer_send; constraint: tokenizer_send_deadline`, with a send-deadline explanation and explicit unknown outcome/no replay/possible new-charge warning. Connection and unknown-send messages do not invent a root cause.

Pi's shipped decoders intentionally ignore gateway messages, so **Pi still displays its legacy coarse warning**. No new optional metadata, raw-message display or closed wire detail is introduced. Finer native client display would require a separately qualified compatible protocol path, not parsing server message strings.

### Aggregate release review

The existing `possums.interface.errors` integer monotonic DELTA sum, unit `{error}`, keeps only `error_type` and complete non-overlapping five-minute windows. Two locally authored values refine one existing send category: **56** error labels, plus nine token points = **65** extension points. Baseline ceilings remain 1,051 request series / 3,256 expanded cells; extension-inclusive ceilings become **1,116 / 3,321**. No new dimension, overlapping total, finer timestamp, identifier, raw exception, exemplar, persistence or client upload is added. Retention remains the existing Honeycomb US provider-default MVP boundary; sparse activity/correlation risks are unchanged. Existing off/outage, once-only, complete-window and arithmetic gates remain.

## Offline verification

Pinned `devenv shell` Rust 1.88 / Node 24.13.0, isolated Cargo target:

- `cargo test --locked --all-targets --all-features`: **491 passed**, four funded/live probes ignored. Includes supported reqwest timeout/connect/request-build predicates using isolated loopback/synthetic values; hostile URL/header marker exclusion; alias serialization; actual API envelope; and both new causes through preflight, once-only metrics and existing refund checks. An initial test lacked the fixture's rustls provider initialization; corrected before the final passing run. Initial stale cardinality expectations were corrected, without raising allocation or transport ceilings.
- Scoped formatting, diff checks and all-target/all-feature strict Clippy passed; active LSP probes for the five production/classifier/API Rust paths found no errors. The unchanged vendored unused-method warning remains.
- **33** serialized export/allocation tests passed with `POSSUMS_TELEMETRY_ALLOCATION_TEST=1`: supported SDK/TLS wire includes the full closed enum, **2,815 maximum reachable points / 377,403 bytes**; materialization peak **850,300 bytes**, serialization/capture **1,507,420 bytes**, co-closing incremental **2,443,885 bytes**, composed TLS incremental **4,483,412 bytes**. These are requested allocations, not RSS. Unchanged 8,388,608-byte library and 6,576,128-byte outbound ceilings passed. `ErrorTotals` is 456 bytes and `RequestTables` 32,392 bytes on this tested platform.
- Exact shipped error decoder extraction with TypeScript 5.9.3 passed for v0.0.11–v0.0.15, request-preparation (`693e580b216c`), v0.0.30 forwarding (`4a2a41884907`), graceful warnings (`5e7c451e7259`), and the installed image/v3 bundle (`83624d2899fb58ea4488c41cebff1609926f13275344a47704fc8355d88238cc`). Prior full hashes are retained in [the original matrix](verification.md#typed-interface-errors-and-token-aggregates--source-candidate). Added regressions verify the unchanged tokenizer envelope with each finer message is accepted, ignored for native presentation and cannot confirm billing/settlement. The separately compiled current-source test bundle also passed the decoder matrix (`2b5be94ef3c631ed235db55ceef1f9724a59236ff072003cf1621caf8a1397a6`); client sources were unchanged by this patch. These tests initialize no provider/network and do not claim old streaming compatibility or whole-host E2E.
- Independent read-only review found no blocker and identified the outer-preflight deadline limit below. No real inference, exporter destination, client installation, protected release or deployment was initiated by these checks.

## Remaining limits

The enclosing preflight starts its existing 600-second deadline before tokenizer send starts its own equal-duration deadline. If that outer deadline wins, the correct observed category remains `preflight_deadline`; do not relabel it tokenizer deadline or change deadlines/retry policy for diagnosis. The existing paused-clock preflight-deadline regression remains in the passing suite; new SDK predicate tests do not reproduce authenticated production transport or prove which timer won historically.

No live tokenizer outage/root cause was reproduced, and no generated Linux image or deployed behavior for these two new labels was qualified. Historical missing aggregates remain unavailable; this patch cannot recover lost/incomplete windows or external SDK/host failures. No attestation, accounting, cancellation, retry, wire receipt or tool-authorization gate was changed.

## Subsequent gateway upstream-pin investigation

The operator requested source investigation of repeated catalog HTTP 503 failures. The source and historical `v0.0.43` code establish:

- `src/main.rs` constructs one `TinfoilInference` at startup and shares it through `Arc`. Its SDK direct client verifies once and stores an immutable upstream TLS pin; catalog and tokenizer sends reuse that client. No production gateway call to the SDK's explicit `Client::verify(&mut self)` refresh method was found.
- `vendor/tinfoil/src/verifier/tls.rs` compares the peer DER-SPKI SHA-256 with that startup pin on every TLS connection. Idle pooling is disabled. A changed upstream key therefore fails closed on new connections; this protection is correct, but the gateway has no verified renewal path.
- Catalog send errors other than timeout become `CatalogFailure::SendUnknown` and legacy `catalog_failed` on the wire; tokenizer send errors use `tokenizer_send_failed`. Process readiness does not test upstream availability. This explains how a stale pin could produce the observed symptom while the gateway remains ready. Existing funded tests construct fresh clients rather than exercising a resident client across upstream rotation.

One bounded credential-free public gateway `/attestation` acquisition and one standard-Web-PKI-verified TLS handshake to the fixed configured upstream were compared in memory. **The public document's upstream TLS pin did not match the observed peer's DER-SPKI SHA-256.** No raw keys, fingerprints, reports, certificates, response bodies or exception text were retained in this record; no inference, mutation or retry was initiated. The comparison used the same fingerprint definition as the production verifier. This was a public-document/HTTPS observation, **not fresh enclave attestation**, proof of every upstream replica's key, or cryptographic attribution of an earlier failed request.

Stale upstream pinning is consequently the leading source-backed explanation, not a proven historical root cause. A separately authorized rollout may construct a fresh client and temporarily mask the lifetime defect; check the same closed comparison after verified serving acceptance before interpreting a successful probe as a durable fix. Any renewal design must fully reverify provenance, attestation and endpoint binding, preserve in-flight trust/accounting boundaries and fail closed, with no automatic inference replay or relaxed TLS checks. This investigation does not authorize or implement a changed renewal/recovery policy.
