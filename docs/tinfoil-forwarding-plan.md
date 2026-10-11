# Tinfoil forwarding implementation plan

**Author:** Astra (GPT-6), repository-grounded planning task.  
**Status:** Operator-approved plan implemented and accepted at the local source/fixture scope. See [implementation and scoped review evidence](tinfoil-aligned-accounting.md). No deployment or client installation is claimed. The worktree inventory below describes the pre-implementation planning baseline, not the current diff.  
**Baseline:** HEAD `48dcd2a3c9e5ef9ea3bff6600979274d1070be85`, plus unaccepted worktree changes.

## Recommendation

Use the existing verified Tinfoil HTTP client, reqwest's native response frames, and the SDK SSE decoder for both text and tools.

Delete the separate text SSE parser and synthetic role/delta/finish reconstruction. Preserve supported native fields, adding only a small Possums settlement receipt.

An observed downstream disconnect must cancel the active upstream operation—including tokenization—without cancelling the sole settlement owner. Temporary backpressure must wait within existing memory bounds, not detach delivery.

## Evidence established by planning research

- `reqwest 0.13.5::Response::chunk()` and `bytes_stream()` discard trailers.
- Its supported conversion into `http::Response<reqwest::Body>` preserves native body frames. `BodyExt::frame()` can expose DATA and TRAILERS without another HTTP stack.
- The vendored SDK SSE decoder consumes bytes, emits JSON, suppresses `[DONE]`, and continues to EOF. Use it rather than maintaining two decoders.
- SDK decoding errors can contain payloads: map them immediately to closed local error categories.
- Current gateway inference uses direct verified origin-bound TLS, not SDK EHBP proxy mode. The pinned Rust EHBP proxy path discards trailers internally; this does not block the current direct path.
- Provider documentation supports opt-in usage metrics and partial-disconnect billing. It does not prove that every model exposes interim counters to clients.
- `git diff --check` passed during planning. No build or runtime qualification was performed.

Sources:

- [How usage is recorded](https://docs.tinfoil.sh/admin/admin-api#how-usage-is-recorded)
- [Usage metrics for billing](https://docs.tinfoil.sh/guides/proxy-server#usage-metrics-for-billing)
- `vendor/tinfoil/POSSUMS.md`
- `vendor/tinfoil/src/sse.rs`
- Pinned reqwest response/body source.

## Worktree disposition

| Changes | Decision |
|---|---|
| `AGENTS.md`, `SPEC.md`, `PRIVACY.md` | Retain approved policy. Clarify that receipt functionality is retained, not necessarily historical wire ordering. |
| `clients/pi/README.md` | Retain candidate-status notice; update after implementation. |
| Untracked `docs/tinfoil-aligned-accounting.md` | Retain policy context; mark implementation paused/unverified. |
| `src/inference.rs` | Rework/drop draft observed-method additions. They introduce inconsistent call signatures and duplicate interfaces. |
| `src/inference/stream.rs` | Retain whole-event validation and unsigned-count ideas, not the duplicated observer/parser plumbing. |

The draft implements no cancellation, settlement, route, trailer or client changes. It has no test/build qualification.

## Architecture

```text
Authenticated request
 → verified live catalog snapshot
 → maximum reservation + sole settlement owner
 → cancellation-aware tokenization
 → one verified, non-replayable generation request
 → native HTTP DATA/TRAILERS
 → SDK SSE decoder + bounded metrics parser
 → bounded native-event forwarding
 → exactly-once settlement
 → authenticated Possums receipt, if delivery remains possible
```

Keep existing trust, leased upload bytes, bounded delivery storage, passive
lifetime accounting and transient idempotency state. Current main independently
removed shared chat Heavy/Ingress and generation admission, cumulative client
stream quotas and live tool-count/argument quotas. Preserve those removals and
32-MiB request/retained-state envelopes; do not restore a shared workload gate.
See [current-main integration evidence](tinfoil-forwarding-integration.md).

Send upstream `X-Tinfoil-Request-Usage-Metrics: true`. Keep supported `stream_options.include_usage`. Do not invent undocumented continuous-usage options.

## Cancellation and accounting

Use one disconnect signal with seamless ownership transfer from request/preflight to response body.

Observe it before dispatch and during tokenizer, generation send, body reads and downstream-capacity waits. If cancelled during tokenization, drop tokenization and never start generation.

Replace queue-full detachment with bounded asynchronous waiting. Do not interpret capacity waiting as network inactivity.

The settlement owner survives cancellation and settles once:

| Evidence/outcome | Action |
|---|---|
| Admission failure | No charge |
| Reserved, cancelled before generation response | Refund |
| Model HTTP 4xx/5xx rejection | Refund |
| Valid terminal response metrics at clean EOF | Prefer these counts |
| Otherwise valid terminal SSE usage | Use these counts |
| Interruption with trustworthy running usage | Use last validated cumulative snapshot |
| No trustworthy usage | Refund |
| Accounting failure | Do not fabricate confirmed billing |

Never sum cumulative snapshots, infer tokens from text, or drain after disconnect to obtain a trailer.

**Completion and money are separate.** Missing finish/EOF, malformed streams and cancelled tool calls remain incomplete even when usage is billable.

Define the race explicitly:

- EOF committed first: final settlement survives later disconnect.
- Disconnect observed first: close upstream and settle evidence already validated.
- Receipt delivery failure never reverses settlement.
- Panic/drop fallback uses committed trustworthy evidence or refunds; no second owner.

## Pricing

Preserve the submitted quote and reservation cap:

```text
charge = min(reservation,
             ceil(1.30 × quoted token cost in microunits))
refund = reservation − charge
```

Use checked integer arithmetic and one final upward rounding.

- Retain authenticated cached-input rates currently discarded by `src/catalog.rs`.
- Apply valid reported cache splits; cached tokens are a subset of prompt tokens.
- Reasoning is already included in output; do not add it again.
- Where caching is not reported, retain the disclosed standard-input calculation without claiming invoice equality.
- Do not debit `cost_usd × 1.3`: provider published rates, quote timing, exemptions and fees can differ.
- Preserve native `cost_usd` as provider metadata; label Possums charges separately.
- Do not enable unquoted fee-bearing features. Unexpected fees and unobserved usage remain operator liability.
- Fix client arithmetic to validate capped charges and avoid negative displayed cost components.

The existing “maximum output equals context” catalog assumption remains an assumption, not a proven provider invoice bound.

## Forwarding and client migration

Forward supported native `choices`, deltas, finish reasons and usage fields. Permit native grouping such as role/content or finish/content together.

Do not forward arbitrary headers, raw errors, upstream identifiers or metadata. Bound and drop irrelevant optional fields.

Keep gateway validation for trust, model binding, resource limits, supported choices, numeric usage and terminal state. Tool argument assembly, schema checking and execution authorization belong to the caller.

Emit one small `possums` receipt containing:

- Monetary outcome and complete/incomplete status.
- Reservation, charge and refund.
- Submitted rates and 30% markup.
- Selected usage evidence and native counts used.

Reserve this namespace against upstream injection.

Reference/Pi clients must require a complete receipt and authenticated EOF before successful completion or tool execution. A paid incomplete receipt confirms money, not tools. Missing/truncated receipts remain unknown billing.

Migrate:

- `examples/phase01/client.ts`, `transport.ts`, `limits.ts`
- `clients/pi/provider.ts`, `diagnostics.ts`

Preserve Pi's approved native retries; each retry is a new submission.

Optional downstream native metrics trailers may be exposed when requested and qualified through the existing shim. Body receipts must not depend on browser Fetch trailer support.

## Commit sequence and deletion targets

1. **Baseline and transport fixtures**
   - Remove/rework partial source draft.
   - Prove native trailer reception, actual EOF and body-drop closure with synthetic HTTP.
   - Assert one upstream request, no replay.
2. **Evidence-aware settlement and pricing**
   - `src/catalog.rs`, `accounting.rs`, `generation_owner.rs`.
   - Preserve mutex ledger and absorbing outcomes.
   - Separate completion from settlement; add cached rates and cap-aware tests.
3. **Unified SDK consumption**
   - `src/inference.rs`, `src/inference/stream.rs`.
   - Native frame reader plus bounded metrics parser.
   - Delete `ProtocolParser`, duplicated text/tool consumption and legacy/observed method proliferation.
4. **Cancellation and native forwarding**
   - `src/server.rs`, `api.rs`, `generation.rs`, `stream_owner.rs`, `api_stream.rs`.
   - Delete continue-after-disconnect behavior and full-queue detachment.
   - Delete synthetic role emission and delta/tool/finish reconstruction.
   - Land reference-client parser changes with the new producer.
5. **Pi migration and qualification**
   - Partial-charge diagnostics, reconciliation and cap/cache display.
   - Preserve execution gates and retry policy.
   - Exercise actual pinned shim/EHBP fixture.
   - Update documentation with source-only results.

Do not build new ledgers, transports, admin reconciliation services, persistent request archives or replacement EHBP.

## Required tests

- Reservation before tokenizer; trust/catalog/credit failures send no prompts.
- Early disconnect before/during tokenization prevents generation.
- Midstream disconnect closes upstream without draining.
- Temporary full queue waits and resumes within byte/frame budgets.
- EOF/disconnect/receipt-drop races settle once.
- Final metrics, final SSE, running-only, no-usage and HTTP-rejection cases.
- Malformed/duplicate/oversized metrics, wrong model and inconsistent cache splits.
- Markup, rounding, cap, overflow and quote-versus-published-cost differences.
- Native event grouping and tool fragments.
- Paid partial tools never execute; missing receipts remain unknown.
- Panic/drop cleanup preserves accounting and lease ownership.
- Raw SDK errors, headers and content cannot enter telemetry or exported diagnostics.
- JS cancellation through actual shim to Rust and synthetic upstream.

Run existing Rust suites offline after implementation. Client checks require the repository's pinned scratch environment; do not install missing dependencies under this authorization.

## Real blockers and unresolved evidence

**Acceptance blockers:**

1. Demonstrate trailer reception with the pinned native HTTP path.
2. Demonstrate disconnect observation during preflight; add a minimal existing-server connection notification only if handler cancellation is insufficient.
3. Obtain access to already-provisioned pinned client dependencies for qualification. Reported missing `@earendil-works/pi-ai` and `ehbp` are environment blockers, not proof of code defects.
4. Land the paired receipt migration coherently.

**Unresolved external evidence—not reasons for new infrastructure:**

- Which models expose interim usage.
- Provider trailer availability on the current direct endpoint.
- Deployed shim cancellation latency and trailer propagation.
- Immediate compute termination and exact invoice equality.

Current scope establishes neither installed nor deployed behavior. No paid probes, deployment or installation are authorized.
