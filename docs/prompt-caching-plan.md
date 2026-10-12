# Session-scoped upstream prompt caching — current-main integration

Status: source integration accepted on `966477afa2448e277d95a34833efac86ae6d7730` in `feat/session-prompt-caching`; operator authorized merge/deployment. Parent pinned Rust 1.88 tests and independent Luna review passed. At this source checkpoint no release, deployment, installation, credential/settings mutation or inference probes occurred. Paid/live testing is explicitly skipped by the operator. Hosted CI, protected publication/deployment and serving verification remain separate gates.

## Authority and narrow scope

The reviewed old-source cache plan/patch supplied the session-isolation and reporting intent, not replacement files or a current streaming contract. Current main's [Tinfoil-aligned accounting](tinfoil-aligned-accounting.md), [native forwarding integration](tinfoil-forwarding-integration.md), and `AGENTS.md` remain authoritative. Preserve PR58 native forwarding/validated partial settlement, PR59 shipped-client compatibility, PR60 duplicate-usage handling, PR61/62 request admission and generation-HTTP diagnostics, and PR63/64 release status. No old `ProtocolParser`, drain-to-terminal policy or ad-hoc final-usage authority is restored.

1. Authorize only session-scoped volatile upstream reuse in `PRIVACY.md` and `AGENTS.md`.
2. Add an independently random 32-byte `CacheScope` to the existing API `Session`. Clone it at locked accounting admission, carry it through detached preflight/prepared text or tool input, and supply it explicitly to the existing SDK builder over verified generation transport.
3. Reuse existing cached quote/settlement and wire compatibility. Correct only cache availability and Pi component reporting.
4. Run offline pinned-toolchain/client checks and record evidence. Parent independently reviews, commits small logical changes and runs the separately authorized protected release. No release action is part of this integration packet.

## Session and privacy boundary

- The scope is generated independently of credentials, account identity, bearer, admission binding, conversation IDs and submission hashing. No HMAC, derived identifier, environment namespace, client-supplied scope, endpoint or persistent state is added.
- Concurrent accepted submissions and conversation resets within one API login share the scope. Separate logins (including the same account), accounts and process restarts get independent scopes. There is no replica sharing.
- Logout/expiry removes the stored session scope and prevents new admission; it does not veto already accepted work's owned copy. Existing downstream disconnect/preflight cancellation behavior is unchanged. Session removal does **not** prove upstream cache erasure or immediate memory erasure.
- Scope has redacted Debug and no serialization implementation. Encoding is confined to the SDK request builder (and isolated synthetic tests). Both text and tool production paths use the same native forwarding adapter; non-session diagnostic/legacy calls retain fresh random volatile scopes.
- No local SDK prompt-cache persistence, disk prompt cache, cache warming, inference replay, extra inference/catalog call for reporting, telemetry family/dimension, logs/traces or support artifact is introduced. Native Pi cache warming remains stopped. Native bounded retries/steering remain operator-authorized new submissions with possible additional charges.
- The opaque scope is still a sensitive session-linked pseudonym. Shared cache timing can link conversations within a login; compromised gateway/inference code can expose active scope mappings and plaintext. Independent logins avoid intentional namespace linkage, not network/cache-timing correlation. Tinfoil documents volatile GPU prefix state, no disk/export and eviction (minutes, potentially hours), not a fixed TTL; that is provider documentation, not independently verified runtime behavior. Derived KV state is not harmless merely because it is not literal text.
- Policy references: [data handling](../PRIVACY.md#data-handling-boundaries), [forbidden telemetry/diagnostic exports](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data), [synthetic testing](../PRIVACY.md#synthetic-testing-exception). Pi/T3's existing host history can persist usage/diagnostics; no whole-host transience claim is made.

## Accounting reused without changing terminal authority

`src/catalog.rs` already preserves/validates optional cached-input pricing (including zero), snapshots it in the submitted quote and reserves the full uncached context/output maximum. `src/accounting.rs::finish_cached` and `src/generation_owner.rs` already discount validated cached tokens at that quote, apply the disclosed 30% markup with one checked upward rounding and cap liability at the original reservation. Missing cached price/count uses standard input pricing. Those files are unchanged by this packet.

`src/inference/forward.rs` remains the source of validated cumulative SSE/native metrics evidence. Current native number/schema validation, final-versus-running usage selection, clean-EOF metrics handling, identical duplicate metrics acceptance, conflicting duplicate rejection and previously validated evidence retention remain unchanged. Cached counts are a subset of input; never sum cumulative snapshots or infer counts from output. An observed disconnect promptly closes upstream; settle trustworthy accepted final/running evidence once, release unused reservation, refund fully without trustworthy usage. Model rejection before response is not billable. Partial settlement is neither completion nor tool authorization. Close does not prove immediate compute termination or invoice parity; the operator absorbs unobserved upstream usage.

The only evidence change is a boolean indicating whether the authoritative selected snapshot actually reported cached usage. Existing null/absent optional upstream metadata semantics are retained. Unknown counts no longer become a fabricated `cached_tokens: 0` in the receipt; confirmed zero stays zero. This flag does not authorize evidence or change the charged amount.

The sibling intake is untouched. Its money-only reservation/final-charge contract needs no cache secret, rate or token schema. Future authoritative intake settlement/expiry/conflicting duplicate/lost-response qualification remains outside this packet; the old plan's drain-policy mismatch is resolved here by retaining current gateway partial-settlement authority, not changing intake.

## Reporting and compatibility

Current main already exposes optional cached catalog/quoted receipt rates and native usage/partial-receipt fields, and its shipped decoder already handles them. Reuse that actual PR59-compatible wire contract rather than porting the old patch's `X-Possums-Cache-Reporting: true` opt-in/default legacy-shape shim. No new wire fields, version framework or mandatory breakdown are added. The only receipt shape change is omitting the already-optional cached count when unavailable. Current/new decoders accept old payloads without cached prices/counts, unknown price with reported reads, and optional quoted rates missing altogether; total charge/refund stays authoritative. Supported shipped-artifact checks are recorded below; this is not indefinite compatibility with historical pre-forwarding strict parsers. Protocol compatibility does not bypass attestation/release pairing or authorize installation.

Pi maps total API input into `input = prompt_tokens - cached_tokens` and `cacheRead = cached_tokens`, without changing `totalTokens` or inventing cache writes. Missing counts retain the numeric fallback required by Pi plus explicit `cacheUsageAvailable: false`; that zero is unavailable, not a confirmed miss. Catalog cache-read pricing uses the quoted cached rate or standard input fallback and remains indicative.

Pi native component costs are populated only when each quoted component is integral in microunits and their sum equals the authenticated charge. Rounded, capped, unquoted or otherwise unsupported breakdowns use required numeric zero fallbacks plus `quotedCostComponents: false`, never assigning rounding residue or an absorbed cap to output. The actual total/refund remains recorded. Both successful and validated partial receipt diagnostics retain these availability flags. Existing interrupted-delivery, confirmed-money, completion/tool gates, safe diagnostics and native retry policy are preserved. No savings or upstream invoice agreement is invented.

## Integrated files

- Policy/record: `AGENTS.md`, `PRIVACY.md`, `docs/prompt-caching-plan.md`, `docs/verification.md`, `clients/pi/README.md`.
- Gateway: `src/auth.rs`, `src/generation.rs`, `src/api_stream.rs`, `src/inference.rs`, `src/inference/stream.rs`, `src/inference/forward.rs`.
- Client: `examples/phase01/client.ts`, `clients/pi/provider.ts`.
- Offline fixtures/tests: `examples/phase01_fixture.rs`, `src/inference/tools.rs`, `tests/api_chat.rs`, `tests/live_tinfoil.rs` (signature update only; funded tests remain ignored), `tests/support/native_usage_contract.rs`, `tests/phase02_pi.mjs`, `tests/shipped_usage_decoder.mjs`, `tests/tinfoil_forwarding_client.mjs`.

Parent-owned logical split: policy/plan first; session scope and gateway fixture/interface tests; client availability/component reporting and compatibility tests; final verification record. No cleanup or future-phase infrastructure is included.

## Scoped acceptance and verification

Offline tests cover random 32-byte scope, redacted Debug, same-session/reset/concurrent admissions, distinct login/account/restart scope, accepted logout/expiry ownership, text/tool builder scope override resistance, and defensive request envelopes. Gateway tests exercise full/partial/zero cached reads with quoted zero cached pricing, unchanged full reservation, native successful/partial settlement and duplicates, unknown versus confirmed-zero cache availability, and hostile client scope rejection before prompt work. Existing forwarding, nullable usage, hostile metadata, duplicate metrics, interrupted receipt, diagnostics, disconnect/backpressure, reservation/refund and tool suites remain intact.

Client fixtures cover missing/zero/partial/full cache counts, missing cached prices/quotes, combined rounding/capping, authoritative totals, distinct Pi buckets, availability flags and no warming/reporting work. Tests use exact declarations extracted statically from the actual installed older bundle and the candidate production bundle; they never initialize those providers or perform authentication/network calls. Prior source-only evidence is not retrospectively upgraded. See [integration verification](verification.md#session-scoped-prompt-caching--current-main-integration).

No live cache hit, provider dashboard/invoice comparison, upstream retention/erasure, deployed source identity, installed client activation or whole-runtime privacy is established. Historical billing/privacy unknowns, native-v3 CLI platform-signer mismatch and legacy-v2 freshness limitations remain unresolved. The old run's missing pinned Cargo was a historical local blocker; this integration uses the already-existing Nix Rust 1.88 toolchain, without installation.

Provider references from the reviewed plan (not newly fetched/verified): [prompt caching](https://docs.tinfoil.sh/sdk/prompt-caching), [secure caching design](https://tinfoil.sh/blog/2026-07-14-secure-prompt-caching), [security/privacy FAQ](https://tinfoil.sh/security-and-privacy-faq).
