# Private Pi receipt/balance reconciliation candidate

## Approved purpose and protocol

The operator reauthorized receipt-to-balance reconciliation, not a client spending-budget engine, payments or per-user telemetry. This is a source candidate; no new gateway deployment, client installation or paid live reconciliation is claimed.

`POST /v1/balance` accepts an empty JSON object through the existing verified EHBP control channel and exact API-bearer grammar. It returns only `available_microunits` and `completed_requests` as canonical u64 decimal strings, plus integer `in_flight`, from a single accounting lock. No account/payment/session/submission identifier, catalog, prompt, history or per-request usage appears. It must not invoke the catalog, tokenizer or inference. Existing no-store, parser/body ceilings, control admission, safe errors and encrypted-response validation apply. The deployed shim's existing wildcard path configuration already covers the endpoint; no trust pin or attestation bypass is introduced.

`completed_requests` is a memory-only per-account accounting counter, incremented once for the first settlement or full refund. Duplicate/conflicting terminal calls and rejected/duplicate reservations cannot increment it. Checked arithmetic must fail without partially changing the ledger. It resets with the memory-only ledger on restart; this is not durable purchased-credit accounting.

## Reconciliation window

`/possums-reconcile start` reads an authenticated snapshot while Pi is idle and requires zero outstanding reservations. It opens a private in-memory window containing the starting snapshot, exact settled-charge total, count of observed settled/refunded attempts and an unknown-outcome flag. Each provider attempt is observed once as it terminates, including native retries and compaction summaries. This does not rely on rounded cost displays or projected session history, which may omit failed attempts.

`finish` requires another idle, zero-in-flight snapshot. A match requires both:

- `before.available − after.available = sum(authenticated settled receipt charges)`.
- `after.completed − before.completed = observed settled/refunded attempts`, with no unknown outcome.

Do not subtract receipt refunds again: available credit already includes released reservation remainders. Unknown outcomes, unrelated completed activity (including zero-charge refunds), counter reset, account/session replacement, pending reservations or a debit discrepancy cannot establish a match. A successful check concerns the gateway ledger and its authenticated receipts, not provider invoices or guaranteed response delivery. A concurrent request starting after the final snapshot is outside the checked interval.

Start/finish/cancel never initiate inference. Users deliberately choose any paid task between snapshots; automatic native retries may incur additional charges under the approved client policy. Finish/cancel/logout/new session discard the window. No persistent failure-charge ledger, transcript parsing, model breakdown or budget enforcement is added. UI results are transient; do not auto-export or package them into support artifacts. Share only content-free failure codes, not credentials, account identifiers or private balance values.

## Privacy references and telemetry boundary

- [Data handling boundaries](../PRIVACY.md#data-handling-boundaries): this is necessary first-party account accounting/reconciliation, not exported observability.
- [Telemetry permitted/forbidden data](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data): balances, exact charges and account completion counts must never enter metrics, traces, logs, support exports or Honeycomb.
- [Aggregation and reviewed MVP release scope](../PRIVACY.md#aggregation-is-necessary-not-sufficient): no new metric/resource names, dimensions or cadence. The new route remains in the existing `Other` operational endpoint class; no per-account labels or finance observations are added.
- [Required evidence](../PRIVACY.md#required-evidence-for-every-telemetry-change): inspect synthetic hostile-content/error paths and preserve complete-window gates; neither configuration intent nor low cardinality proves anonymity.

The native Pi auth store, sessions, tools and other extensions remain outside gateway attestation. The new snapshot and counter are memory-only; this does not claim that memory is immediately erased or that runtime compromise cannot expose account activity.

## Verification status

Local implementation qualification passed. Independent read-only review found no concrete defects in the balance/accounting, API isolation, protected-client validation or reconciliation/race boundaries; it made no changes. No deployment or live inference is authorized by the tests or this record. Before live reconciliation, build/release independently, verify public provenance and serving channel, deploy only with operator approval, install the paired client, then use a quiet account and explicitly authorized paid task. Existing v0.0.17 receipt evidence remains historical and unreconciled.

### Local evidence

- Cached Rust 1.88.0, locked/offline: 13 accounting integration tests, nine API-auth tests, 16 API-stream/accounting tests, five private accounting tests and one existing auth expiry/capacity test passed (44 total). They cover atomic races, exactly-once terminal counts, overflow rollback, account isolation, API/auth/body/admission/no-store behavior, no upstream calls and actual Rust API receipt/balance agreement. `cargo fmt --check` passed.
- Privacy regression suite: four passed. Existing exporter/wire/materialization regressions: 26 passed, including closed configuration/header boundaries, outbound payloads, sparse complete windows, lifecycle consistency and no partial flush. No new finance telemetry or endpoint dimension was introduced. These are local synthetic tests, not a live Honeycomb or whole-runtime privacy audit.
- Fresh Node 24.13.0/Pi 1.0.4 scratch TypeScript/build and complete local-check command passed: 33 reference-client checks, 492 synthetic client checks, 98 local release-policy checks, 70 actual pinned-Pi/provider checks and 71 shared admission checks. Pi's own count comes from `checks/pi-cases/results.json`, not the separate shared-admission summary. Release-policy tests mock their declared hardware/crypto/public-network boundaries.
- Protected client tests inspect actual EHBP encryption/decryption against isolated fetch fixtures, including u64 precision, strict schema/errors/HTTP rejection, cancellation and no fallback to inference. Pi tests cover native commands without inference/persistent reports; exact settled and confirmed-refund/retry arithmetic; compaction charges; unknown outcomes; unrelated completions; resets; mismatch; pending/busy snapshots; cancel/session invalidation and cancellation during a pending start. Prior native steering/retry and tool-receipt regressions remain passing.
- Active LSP checks of seven production paths reported no errors but confirmed only three clean and left four inconclusive. Strict compiler/build and runnable tests are the verification evidence, not a clean-LSP certificate.

Final scratch package: `/var/folders/5v/1zw417197236y_k11j994g8r0000gn/T/possums-pi-build-bG41X3/package`; extension SHA-256 `fa47e2f7b6c1d59f917119052344858c3504d873a99984c9909e3d889089b149`. Local test output: `/tmp/possums-pi-reconciliation-final-check.log`, `/tmp/possums-reconciliation-rust-check.log`, `/tmp/possums-reconciliation-ledger-check.log`, `/tmp/possums-reconciliation-expiry-check.log`, `/tmp/possums-reconciliation-privacy-check.log`, `/tmp/possums-reconciliation-export-check.log`. Temporary artifacts are local qualification records, not an installed or distributed client.

The extended Go-shim/loopback transport and combined shim→Rust `phase01_api.mjs` tests were added but not executed in this local run; real combined channel qualification remains a release prerequisite. No browser/image/Linux-flake/new-public-release or live billing acceptance is claimed.

During the gateway agent's initial toolchain lookup, `cargo +stable --version` unexpectedly downloaded a toolchain. This was unintended network access, not a live gateway/inference/deployment call; actual qualification used cached Rust 1.88.0 with locked/offline dependencies. The parent independently repeated the Rust suites under the Nix environment.
