# Phase 0.2 upstream tool qualification — working record

**No production tool model is qualified.** Accepted gateway v0.0.9 remains text-only. This record concerns test-only upstream protocol probes, not gateway/Pi end-to-end acceptance, provider invoices, a measured release or deployment.

## Authority and safety

User authorized $5 in **quoted** worst-case cost (including conservative markup), at most 40 generation attempts. Only the test process may load TINFOIL_API_KEY from the existing ignored mode-0600 `.env`; no credentials, raw responses, argument strings or provider error bodies are printed or persisted. Fixed synthetic inputs only; no host tools execute. No automatic retry. Unknown transport/protocol/usage stops the run and retains its worst-case quote hold. The quoted cap is not a proven provider invoice ceiling.

The ignored unit-test module `src/inference/tool_qualification_tests.rs` is compiled only under `cfg(test)`. It uses the production verified origin-bound transport, noncloneable structured serializers, upload-release checks and terminal stream parser, with a test-only candidate protocol. It cannot grant a production profile. The production table in `src/inference/tools.rs` and the approval pins in `examples/phase01/approval.ts` remain unchanged.

Offline preparation passed nine new budget/argument/continuation tests within 31 inference tests (funded test ignored), plus five tool-protocol tests. An independent review found the pre-send reservation/no-retry/bounded-report behavior sound. Its concern that this does not exercise the empty production profile gate was independently verified as an already disclosed protocol-only limitation, not permission to enable models.

## First authorized run

Sanitized evidence: `/tmp/possums-tool-qualification-EvDzAq/report.json` and `run.log`.

- Authenticated catalog selected `deepseek-v4-1-flash`; the `auto` case reached **one generation attempt**.
- The harness returned `uncertain_stop` and stopped before any retry or next model.
- **886,580 microunits ($0.886580) remain held as a conservative quote**, not a measured debit or invoice amount.
- No model passed the matrix; no profile, release or approval was changed.
- Original report collapses transport, stream and terminal-usage failure into uncertainty. It does not establish the specific cause, receipt validity, observed call count or actual charge. Do not invent those observations retrospectively.

## Explicit next diagnostic decision

After that stop, user authorized **one new distinct diagnostic canary** on another authenticated-catalog model, after safe failure-stage reporting and offline tests. The previous quote hold remains counted: no more than **4,113,420 additional quoted microunits**, and only **one additional generation** for this decision. Do not rerun the original matrix or replay DeepSeek. Stop again on uncertainty. A single diagnostic pass cannot qualify a full production tool profile.

The test-only diagnostic added fixed last-stage/outcome enums, allowlisted HTTP statuses and bounded observed-call counts without raw content or token/credential data. All 15 offline diagnostic tests passed, including preserved prior holds and a cumulative one-additional-generation cap; five existing tool-protocol tests also passed. A broader socket-free selection exposed an existing rustls-provider initialization dependency in the isolated serialization fixture (26 passed, 1 failed); it was not changed, and a complete fresh inference suite is not claimed.

Second sanitized evidence: `/tmp/possums-tool-qualification-Bscs9J/report.json` and `run.log`.

- Exact authenticated catalog model `kimi-k3`; one distinct `named` instruction; no continuation or local tool execution.
- Tokenizer and generation HTTP statuses were both **200**. SSE content-type validation passed. The shared stream consumer rejected at **`stream_acceptance`**, before accepting any tool-call deltas.
- This stage encompasses invalid stream/protocol/terminal state or stream transport failure. It does **not** identify a specific malformed field, prove upstream tool incompatibility, or establish the actual charge. Raw content was not retained for retrospective inspection.
- Cumulative generation attempts: **2**. Cumulative conservative quoted hold: **2,260,379 microunits ($2.260379)**, including the unchanged first hold. This is not measured spending.
- The single diagnostic failed and stopped. No automatic replay or third attempt was performed. This explicit one-new-attempt decision is exhausted; unspent quoted headroom is not permission to continue after uncertainty.

Protected production-profile/approval file hashes, HEAD and Git index match the pre-canary baseline; `.env` remains ignored and mode 0600. These six checks are scoped preservation evidence, not a complete filesystem or privacy audit. Further useful work is offline parser rejection attribution/fixtures before separately deciding whether another funded diagnostic is warranted.

## Docs/SDK-first offline diagnosis

User requested checking Tinfoil's docs and SDK before further local work; **no new paid requests** were authorized or sent here.

Sources inspected:

- Official [tool-calling guide](https://docs.tinfoil.sh/guides/tool-calling), [Rust SDK](https://docs.tinfoil.sh/sdk/rust-sdk) and [JavaScript SDK](https://docs.tinfoil.sh/sdk/javascript-sdk) docs. These describe OpenAI-compatible calls and recommend Kimi for agentic workflows; recommendations are not qualification evidence.
- Vendored Rust SDK 0.2.1, imported at `34157e497a747c191852d52af39cfbdb8dbd9eb7`, and its locked `async-openai` 0.41.1 types. `ChatChoiceStream.finish_reason` is `Option<FinishReason>`: absence means no finish. The relaxed SDK accessor also returns `None` when the field is absent. Its permissive SSE parser is **not** a replacement for the gateway's receipt/EOF validation or sanitized errors.
- Public router source at tree `9716d140dc70aa6aa2b10ff1a21defbe4d23d442`: [`toolruntime/chat_stream.go`](https://raw.githubusercontent.com/tinfoilsh/confidential-model-router/9716d140dc70aa6aa2b10ff1a21defbe4d23d442/toolruntime/chat_stream.go), particularly `ensureRoleEmitted`, `emitClientToolCallDelta`, `writeChunk` and `finalize`. Nonterminal role/content/tool frames omit `finish_reason`; finalization emits an explicit finish followed by requested usage and DONE. This public snapshot is **not established as the measured source of either live canary**.

A synthetic role frame in that router shape deserialized successfully through the actual pinned SDK response type but failed the old gateway parser with `StreamError::Protocol`. Evidence: `/tmp/possums-tool-offline-gCKACB/repro-before.log`. This establishes a **local interoperability defect**, not the retrospective cause of the two uncertain attempts.

Small fix in `src/inference/stream.rs`: only an explicitly qualified structured parser treats absent nonterminal `finish_reason` like `null`. Default text behavior is unchanged. A real terminal finish, coherent allowed calls, exactly one valid final usage record, DONE and transport EOF remain mandatory; no final token/usage or billable completion is synthesized. Production profiles and approval pins remain unchanged.

Verification in `/tmp/possums-tool-offline-gCKACB/`:

- Two new protocol tests compare actual SDK deserialization with synthetic router-shaped role and UTF-8 argument-fragment frames, exercise every two-fragment byte split, and reject missing terminal finish, early usage/DONE, missing DONE and malformed finish values. They also check unchanged legacy rejection.
- The real reqwest loopback consumer test exercises both explicit-null and omitted nonterminal finish fields, withholding success until EOF and rejecting a post-DONE transport failure. It does not verify a live attested peer.
- Complete offline/locked gateway suite: **284 passed, 0 failed, 5 funded tests ignored**, 23 suite reports (`full-suite.log`). This is the complete suite, unlike the earlier isolated socket-free selection; the standalone rustls initialization dependency has not been separately refactored.
- Formatting, whitespace and offline all-targets Clippy checks pass; nine protected-file hashes match the pre-diagnosis baseline (`preservation.json`). Active LSP checks found no errors; one informational Rust 2024 let-chain suggestion was intentionally not applied to the Rust 1.88/edition-2021 code.
- Independent focused review found no concrete defect in this isolated optional-field change. It did not review the larger existing working-tree diff or verify live calls.

No credential reads, model qualification, paid retries, deployment, commits or approval updates occurred in this offline diagnosis. The previous unknown billing and **2,260,379-microunit quoted hold** remain unchanged. A fresh funded decision and observed full-matrix success are still needed before enabling any production tool model.

## Newly authorized post-fix live canary

User explicitly authorized a live test after the offline fix. One fresh `kimi-k3` named-function request (synthetic specimen **C**, not either earlier prompt) ran through the patched shared parser. Before execution, the test-only ledger was reseeded with the previous **2,260,379-microunit hold and two attempts**, allowing at most one new generation within the original $5 quoted limit. Fifteen budget/diagnostic tests and seven protocol tests passed before this run.

Evidence: `/tmp/possums-tool-qualification-sVr524/report.json` and `run.log`.

- Tokenizer HTTP **200**, generation HTTP **200**, valid SSE content type.
- Result: **`uncertain_stop` at `stream_acceptance`**, **zero accepted tool-call deltas**. No final usage was accepted by the harness. This does not prove that upstream produced no calls or establish the specific failed field, stream condition or actual cost.
- Cumulative attempts: **3**. Cumulative conservative quoted hold: **3,634,178 microunits ($3.634178)**. The two older holds were not released or retrospectively settled; this total is **not measured spending**.
- No automatic retry, further model attempt, local tool execution, production profile, release/approval change or deployment.

The omitted-finish fix resolves its reproduced local defect but was not sufficient to demonstrate live compatibility. The new single-attempt authorization is exhausted. Remaining quoted headroom is **1,365,822 microunits**, not authorization to continue; at this run's rates it is less than the 1,373,799-microunit worst-case reservation used for another identical-cap Kimi probe. Before any separately authorized future run, carry forward this latest hold/attempt count (the executable specimen-C ledger was seeded from the earlier report), use a distinct specimen, and add narrower sanitized rejection diagnostics rather than another blind probe. Actual prior billing remains unknown.

## SDK reuse and minimum-phase implementation

User directed using Tinfoil's SDK instead of maintaining a parallel provider decoder, then requested checking nearby duplication and recording a minimum-per-phase rule. `AGENTS.md` now says to reuse SDKs/libraries first, build only what the phase requires, and keep custom code for gateway-specific policy rather than provider wire handling. Its existing guidance was preserved.

The structured inference path now passes the verified response byte stream to the pinned **`tinfoil::sse::parse_event_stream`** and uses the SDK's choice/delta/tool-call types. The old qualified tool `ProtocolParser` API, manual tool wire-key checks and tool-specific framing branches were removed. SDK nullable/empty metadata placeholders and noncritical extra fields are accepted; fixtures now traverse the real SDK decoder instead of a shadow parser. Existing SDK framing/limits are used without a new custom cap framework.

A small application validator remains for allowed/stable tool identities, tool-choice policy, final finish and checked `u64` usage (the SDK's usage type uses `u32`). Verified origin-bound HTTP and noncloneable upload stay unchanged to prevent automatic request replay. Raw SDK errors, which can contain response payloads, are mapped to static gateway errors. These are gateway security/accounting behavior, not a second provider decoder.

**Termination change:** the SDK suppresses upstream `[DONE]` and drains the HTTP stream to EOF. Structured upstream success therefore requires a real terminal finish, valid final usage and actual successful transport EOF, **not an independently observed upstream DONE marker**. A missing finish/usage or post-DONE transport error still fails. The gateway's downstream settled receipt + DONE + authenticated EOF contract is unchanged. Legacy text decoding is unchanged; the earlier omitted-finish patch is superseded for tools by SDK decoding.

Local integration verification: **288 tests passed, 0 failed, 5 funded tests ignored** across the complete offline/locked gateway suite (23 suite reports). Formatting, whitespace, all-targets Clippy and active LSP error checks pass. A focused independent review found no concrete defect and reran the seven SDK protocol tests; it did not review unrelated dirty work or live calls. Protected-file checks passed, including preservation of the original AGENTS content around the requested added policy; `Cargo.lock` is unchanged. New SDK tests exercise fragmented UTF-8, optional/null/empty fields, metadata permissiveness, EOF withholding/failure, HTTP byte-progress deadlines, invalid final usage and sanitized SDK errors. API accounting/disconnect and offline canary fixtures use this SDK path. No paid request or credential read was performed for this migration; previous unknown billing and the **3,634,178-microunit hold** remain unchanged.

The bounded nearby duplication check found a legacy `StreamProbe` observer in `src/inference.rs`. It is still used by `tests/live_tinfoil.rs` and recorded in historical verification, so it was not silently removed or expanded. Pi conversion/JSON helpers already reuse Pi APIs; replay guards and reference-client HPKE/receipt handling protect a different gateway protocol and are not replaced by a Tinfoil chunk decoder. This was a nearby check, not a whole-repository no-duplication guarantee. Evidence and before snapshots: `/tmp/possums-sdk-stream-kwomPP/`.

## Authorized SDK live compatibility canary

After the SDK migration, user explicitly authorized **one fresh Kimi canary** and selected a **$10 cumulative quoted-reservation budget**, including all **3,634,178 microunits / three prior attempts**. This does not authorize the original multi-case matrix, another generation, automatic retry, tool execution, deployment or profile/approval changes. Actual prior billing remains unknown; the quoted cap is not a verified invoice ceiling.

The test-only ledger is reseeded from the third report, caps cumulative attempts at **four**, and uses distinct synthetic specimen **D** through the SDK consumer. The existing 512-token diagnostic allowance is unchanged; it is not a production-model cap. Offline preflight passed **15** budget/diagnostic tests plus **seven** SDK protocol tests. Raising the budget made the old “too expensive” fixture affordable; its synthetic rate was adjusted to remain above the new ceiling, and the tests were rerun successfully. No inference occurred during preflight.

Evidence directory: `/tmp/possums-tool-qualification-usd1o369/` (`report.json`, `run.log`, `preservation.json`, preflight logs and source snapshots/hashes).

- The single fresh specimen D ran on exact authenticated-catalog model `kimi-k3` through the SDK consumer. Tokenizer and generation HTTP statuses were **200**; SSE header validation passed.
- Outcome: **`uncertain_stop` at `stream_acceptance`**, with **zero accepted tool-call deltas**. This stage still combines SDK decoding, gateway semantic/terminal validation and transport failure; it does not identify the specific rejection or prove that upstream emitted no calls. No raw live response/error was retained, so it cannot be retrospectively attributed from this report.
- Cumulative attempts: **four**. Cumulative conservative quoted hold: **5,007,977 microunits ($5.007977)**, including all three unchanged prior holds and this attempt's **1,373,799-microunit** maximum reservation. This is not a measured debit or provider invoice. No final usage was accepted.
- No retry, continuation, local tool execution, model fallback, profile/approval change, deployment or commit followed. Fourteen scoped preservation checks passed, including the profile/approval files, existing user documents, lockfile, HEAD/index and ignored mode-0600 credential-file metadata.

SDK reuse did not by itself demonstrate live compatibility. The one-attempt authorization is now **exhausted**; remaining $10 budget headroom is not permission for another request. At that stop, the executable diagnostic seed still described the state **before** specimen D; the next separately authorized canary needed to carry forward **5,007,977 / four attempts** and use a distinct specimen. Before another paid test, distinguish sanitized SDK decoding, gateway validation and transport rejection locally, without persisting raw payloads or rebuilding a parallel provider decoder. Production tool qualification remains empty.

## SDK request construction and user-facing error follow-up

User requested SDK reuse for **request construction too**, and a project policy that errors should be useful without request logging. `AGENTS.md` now requires short, specific, content-free failure codes/messages, observed billing status and safe next actions; it does not claim that all observability is disabled.

Both generation and tokenizer bodies now use the pinned **`RelaxedChatRequestBuilder`**. The custom provider request-envelope structs were removed. Gateway-admitted messages/tools are mapped into SDK JSON, while generic HTTP encoding and the verified noncloneable leased upload remain. Full output allowances, tools/choices/history, requested final usage, fresh cache scopes and single-send behavior are preserved. SDK stream decoding remains in use; no dependency or approval change was made.

`src/inference/errors.rs` defines **35 closed, static failure codes**. Request encoding, verified connection/catalog/tokenization, generation HTTP/endpoint checks, SDK decoding, byte transport/idle/deadline, gateway event/tool validation, missing or invalid finish/usage, and settlement now retain distinct categories. Raw SDK/provider/Serde diagnostic text is discarded—not stored, printed or forwarded. The test-only canary report also retains the fixed category instead of only `stream_acceptance`; its stale pre-specimen-D funded seed/authorization was not changed or executed.

Gateway API errors preserve their legacy `code` and add `detail`, a locally authored `message` and `billing`. Refund status follows an actual matching settlement outcome, not transport failure or a presumed successful Drop. Web errors show escaped fixed diagnostics. The reference client ignores server message text, preserves allowlisted categories, and confirms an error's reservation refund only after successful authenticated EOF. Interrupted, malformed or trailing error delivery remains unknown; hook-thrown fresh or reused `GatewayError` objects cannot fabricate a refund. Pi displays concise reason/status/action and uses a closed message list instead of echoing arbitrary `possums_*` exceptions. Paid partial output/settled charges remain retained and tool execution still requires the successful receipt contract.

Local verification in `/tmp/possums-errors-sdk-xxs11hvn/`: complete offline/locked Rust suite **293 passed, 0 failed, 5 funded tests ignored**; scratch strict TypeScript/build, **31 legacy text**, **286 reference-tool/privacy**, **69 existing admission** checks and **15 Pi checks** pass. The actual pinned Pi SDK test confirms one send, no automatic retry/compaction and no tool execution on a detailed decode failure. All-targets Clippy, formatting and whitespace pass. Active LSP checks confirmed ten Rust paths clean; two TypeScript paths were inconclusive on the push-only server, with strict compiler verification passing. Sixteen scoped preservation checks passed, including unchanged production profiles, approval/dependencies/lock, original guidance and user documents, HEAD/index, and credential-file metadata without reading credentials. Rust/reference-client failure-code vocabularies match. Focused independent Rust review found no concrete defect and reran six SDK stream tests. A client review alleged raw error delivery to hooks; an independent verifier refuted it using the early return and an offline run (role hook only, no raw error), with explicit callback payload assertions retained. The new production-form Pi bundle loaded through the actual 0.99.2 SDK with global fetch blocked and zero fetch calls; this is not a whole-process privacy audit.

No new paid request, credential read, deployment, commit or model qualification occurred in this follow-up. The four historical failures still lack retrospective root-cause evidence and hold **5,007,977 microunits** conservatively; actual billing remains unknown. Better local diagnostics are preparation for a separately authorized future live test, not proof that live tools work.

## Historical authorized diagnostic: SDK builder + classified errors (specimen E)

User explicitly authorized **exactly one fresh Kimi test**, under the **$10 cumulative quoted-reservation ceiling including the four prior holds**. The ledger was seeded from specimen D's report with **5,007,977 microunits / four attempts**, limiting this test to one additional generation. Synthetic specimen **E** used both the pinned SDK request builder and decoder. No matrix, continuation, fallback or local tool execution was authorized.

Evidence: `/var/folders/5v/1zw417197236y_k11j994g8r0000gn/T/possums-tool-qualification-2r5jo2sh/`, including `authorization.json`, `prior-report.json`, `report.json`, `run.log`, `outcome.json`, source hashes and preservation evidence. Offline preparation passed **15 diagnostic**, **seven protocol** and **eight SDK** tests plus formatting/whitespace checks. The initial diagnostic preflight exposed a stale hard-coded remaining-headroom assertion; it was updated from 6,365,822 to **4,992,023** and the checks rerun successfully before any funded request.

- One `kimi-k3` named-function diagnostic ran. Tokenizer and generation HTTP statuses were **200**; SSE header validation passed.
- Result: **`uncertain_stop` at `stream_acceptance`**, now specifically **`failure: "stream_usage_unexpected"`**. The gateway `SdkToolValidator::accept_inner` emitted this category when usage was repeated or present before any accepted/current terminal finish. The report does **not** distinguish those two branches. This identifies a gateway usage-ordering rejection, **not an SDK decoding error**; it does not establish the exact live event shape or the cause of the earlier four failures.
- **Zero accepted tool-call deltas**; this does not prove that upstream emitted no calls. No final usage was accepted for settlement. The test exited **101** after 3.58 seconds and did not retry.
- Cumulative generation attempts: **five**. Conservative quoted hold: **6,381,776 microunits ($6.381776)**, adding **1,373,799** to the unchanged prior holds. Actual provider billing remains **unknown**; these are not measured debits or invoice limits.
- **Twenty scoped preservation checks passed**, including unchanged production profiles, approval/pins/dependencies, existing user documents, HEAD/index, credential metadata, and the exact preflighted sources. Only the isolated funded test read the ignored mode-0600 credential file; no credential or raw provider content was printed or retained.

The one-attempt authorization is **exhausted**. Remaining quoted headroom of **3,618,224 microunits** does not authorize another request. The source seed now describes the state **before E** (`5,007,977 / four attempts`, specimen E); do not rerun it or the saved script. Any separately authorized future attempt needs the latest **6,381,776 / five attempts** ledger and a new specimen. Next useful work is offline investigation of the usage-ordering policy, retaining final-usage/EOF/accounting guarantees—not another blind paid probe. Production tools remain unqualified; no profile, approval, release, deployment or commit changed.

## Offline fix: continuous usage is not final billing usage

After specimen E, user requested local investigation and a fix—not another paid request. A new fragmented UTF-8 fixture passed through the **actual pinned SDK decoder** and reproduced `StreamUsageUnexpected` with a valid role/delta carrying running usage. Before-fix evidence: `repro-before.log` in `/var/folders/5v/1zw417197236y_k11j994g8r0000gn/T/possums-usage-local-zdkrtirn/`.

Public vLLM source at [`4a705c7b54ac775e6436eae6e1ee573f8d4c7775`](https://github.com/vllm-project/vllm/blob/4a705c7b54ac775e6436eae6e1ee573f8d4c7775/vllm/entrypoints/openai/chat_completion/serving.py) attaches cumulative usage to role/delta/finish chunks, then emits a separate empty-choices final usage chunk. Its [`should_include_usage`](https://github.com/vllm-project/vllm/blob/4a705c7b54ac775e6436eae6e1ee573f8d4c7775/vllm/entrypoints/serve/utils/api_utils.py) can force both modes server-side. The inspected public Tinfoil router also documents continuous backend statistics, although that router path strips them before forwarding. Neither public source is established as specimen E's measured implementation; the missing live payload still prevents exact retrospective attribution.

Small `SdkToolValidator` fix:

- Validate every non-null usage snapshot as checked unsigned `u64` counts before callbacks, but **never use interim counts for final billing**.
- Keep finish-chunk usage separate from the definitive final event. One post-finish empty-choices usage event takes precedence, even when its counts differ from the finish snapshot.
- Continuous streams **require that separate final usage**; a finish snapshot cannot substitute when it is missing. Preserve previously supported finish-only usage for non-continuous streams.
- Premature usage-only events, duplicate/conflicting final events, invalid arithmetic/types, missing finish/final usage, late output/errors and transport failure remain errors. Success still waits for actual EOF. No usage is inferred from text; settlement/receipt/tool-execution gates remain unchanged.

Main verification: **297 passed, 0 failed, five funded tests ignored**, 23 complete offline/locked suite reports (`full-final.log`); focused **11 protocol + 12 API accounting + nine SDK** tests pass. Fixtures exercise every two-fragment UTF-8 split, candidate/final precedence, malformed interim atomicity, absorbing errors, identical/conflicting duplicate final events, withheld EOF and post-DONE transport/idle failures. Gateway accounting tests hold the maximum reservation during provisional output, charge exactly once at the submitted rate from final—not interim—counts after disconnect/overflow, and refund once on missing/invalid final usage. Formatting, whitespace, all-targets Clippy and three confirmed-clean active LSP checks pass. Twenty-four scoped preservation checks passed, including byte-identical legacy parser/helpers, unchanged SDK request/transport/accounting code, profiles/approval/pins/dependencies, funded ledger seed, original user documents, HEAD/index and credential metadata without reading credentials.

A scoped independent review alleged duplicate final counts could overwrite billing; an independent verifier refuted it using the pre-commit `self.usage.is_some()` guard and both focused duplicate tests. The explicit conflicting-count regression also passes. No production decoder or accounting changes beyond the validator's usage distinction were needed.

**No paid request or credential read occurred in this fix.** The latest live outcome remains failed, with **6,381,776 microunits / five attempts** conservatively held and actual billing unknown. This establishes a reproduced local interoperability fix, not live compatibility or the exact cause of E/the earlier four failures. Existing finish-reason/tool-choice policy is unchanged; all profile/choice/continuation gates still apply before production qualification. The pre-E executable funded seed remains stale and must not be rerun without a new explicit decision and latest ledger.

## Caller-owned tool policy and thinner SDK checks

User requested removing gateway enforcement of returned tool names/choices and keeping custom checks small around the Tinfoil SDK. `SdkToolValidator` is now request-independent: it retains bounded index/identity/argument-byte state, atomic forwarding, checked final usage and successful EOF, but does not check declaration membership, required/named/none compliance or executable-call completeness. Historical argument strings are bounded opaque input rather than parsed executable objects. The SDK still owns structured SSE/UTF-8/JSON and choice/tool field decoding. Working legacy text parsing, verified transport, reservations, settlement, receipts and independent production qualification are unchanged.

The gateway preserves supported finish reasons, including `stop` with complete or incomplete calls; it does not rewrite them to `tool_calls`. Public vLLM source inspected above documents named-function `stop`, but that is not retrospective evidence of a live failure cause. A valid completed generation may be charged even when its proposed calls are unusable. A settlement receipt is **not** tool-execution authorization.

The reference caller still rejects undeclared, named-choice-mismatched and choice-none calls and validates complete argument objects before executable completion. A receipted `stop` with valid calls maps to Pi tool use while diagnostics retain `stop`. A receipted `length` with partial calls retains the settled charge, executes no tools and cannot authorize automatic replay/compaction.

Local verification: **299 Rust tests passed, zero failed, five funded ignored**, 23 complete offline/locked suite reports; strict TypeScript/build, **31 legacy + 336 tool/privacy + 69 admission = 436 client checks**, and **17 Pi checks** passed. Actual SDK fixtures cover fragmented UTF-8, truthful stop/tool/length finishes, missing identities, usage/EOF failure, disconnect and submitted-rate settlement; actual Pi SDK checks cover a stop-tool/result round and paid partial-tool no-execution/no-replay. Format, all-targets Clippy, production-form loader smoke with zero fetch calls, and scoped preservation passed. Five of six active LSP path checks confirmed clean; one and a later standalone harness probe were inconclusive, with compiler/tests passing. An independent scoped review found no concrete defect and reran protocol/API tests. Evidence: `/tmp/possums-thin-tools-3xgw5shl/`; latest local Pi package: `/tmp/possums-pi-build-4JptYN/package/`. This is not deployed or live tool qualification.

The user also authorized **one fresh Kimi diagnostic after local fixes/preflight**, not the matrix: specimen F was seeded with **6,381,776 / five prior attempts**, cumulative attempt cap **six**, and **3,618,224** quoted headroom under the existing $10 ceiling. Its observed outcome follows; that authorization is now exhausted.

## Historical authorized diagnostic: thinner SDK checks (specimen F)

Exactly one fresh `kimi-k3` named-function diagnostic ran after local preflight and scoped independent review. The isolated process used the unchanged verified origin-bound transport, pinned SDK request builder/decoder and the new request-independent validator. Only that funded process read the ignored mode-0600 credential file. An exclusive run marker, fixed preflight source hashes, clean environment, exact ignored-test filter and new report prevented accidental replay.

- **Passed**, exit **0**: **one accepted named call**, valid final usage and successful SDK/transport EOF; report stage **`settled`**, accepted. Tokenizer and generation HTTP statuses were **200**, with no failure category.
- The test-only caller checked the expected function, complete synthetic UTF-8 argument object and permitted terminal finish. The safe report does not distinguish `stop` from `tool_calls`; no raw payload was retained. This does not retrospectively identify any earlier failure's cause.
- Current-case authenticated usage calculated **6,391 microunits ($0.006391)** at the submitted quoted rates including markup. The **five prior uncertain generations / 6,381,776 microunits** remain held unchanged. Cumulative **six attempts / 6,388,167 microunits ($6.388167)** quoted use/holds; actual provider billing remains **unknown**, not a proven invoice ceiling or a Possums gateway account debit.
- No retry, continuation, fallback, host-tool execution, production profile/approval change, release, deployment or commit. The report remains **`qualified: false`**: one named-call pass cannot qualify a model or establish the real Pi-to-gateway tool path.

Evidence: `/tmp/possums-tool-qualification-cj78ok4g/`, especially `report.json`, `outcome.json`, `authorization.json`, `prior-report.json`, `preflight-source-hashes.json`, `harness-preflight.rs`, local gate logs and preservation evidence. **Ninety-two pre-run and ninety-three final scoped preservation checks passed**; executed sources still match preflight hashes. The original user documents, production profile body, approval/pins/dependencies, HEAD/index and credential metadata are unchanged. Legacy parser/helpers remain byte-identical; no full filesystem/runtime-privacy claim follows.

**Authorization exhausted. Do not rerun** the one-shot script or unchanged harness: its seed describes the state **before F** (`6,381,776 / five`). Remaining quoted headroom **3,611,833** is not permission. Any future separately authorized attempt must carry forward the latest **6,388,167 / six** ledger, preserving the distinction between five unknown holds and the current usage-based quote. Matrix/model qualification and release gates remain separate.

## Latest authorized suite: Kimi-only five cases (specimen G)

User explicitly authorized both local preparation/review and spending for **one Kimi-only suite**: fresh `auto`, `none`, required/multiple calls, a fresh named call and one matching-result continuation belonging to that named call. At most **five new generations**, cumulative attempt cap **eleven**, seeded from F's **6,388,167 microunits / six attempts**, under the existing **$10 cumulative quoted ceiling**. Five historical unknown holds remain **6,381,776**; F's usage-based quote remains **6,391**. Starting quoted headroom: **3,611,833**. Actual provider billing remains unknown.

No broad/all-model matrix, uncertain-request replay, retry, fallback, host-tool execution, profile/approval update, release, deployment or commit is authorized. Stop before another case on uncertainty, unaffordable reservation or failed case. The continuation uses only this suite's new receipted named call and fixed synthetic result; it never continues specimen F. The test-only 512-token output allowance is not a production cap or maximum-output qualification.

Preparation/evidence: `/tmp/possums-kimi-suite-local-x7oj8d69/`; private funded report directory: `/tmp/possums-tool-qualification-17yadt8l/`. The dedicated `TINFOIL_KIMI_TOOLS_FUNDED=yes` plus exact `kimi-k3` gate replaced broad/all-model selection and runs before credential/network activity. The historical F entry now explicitly refuses replay. Only the isolated authorized funded process read the named key from the ignored mode-0600 `.env`; offline work and assistant checks used metadata only.

Local preparation passed **306 Rust tests, zero failed, five funded ignored**, 23 complete offline/locked suite reports; **22 focused harness tests**, format/whitespace, all-targets Clippy, one confirmed-clean LSP path and **100 scoped preservation checks**. An actual no-opt-in invocation rejected as expected before evidence/credential/network work. Independent read-only review found no concrete blocker and reran 22 focused tests. New durable append-only checkpoints contain only aggregate quoted use/holds, generation count, active worst hold and stopped status, before prompt-bearing tokenizer work, after counting a generation **before its send**, and after outcome. Journal failure prevents further sends. No new production persistence or provider decoder was introduced.

### Observed result: stopped at tokenizer agreement

The one-shot suite exited **101** after two new generations and stopped immediately:

| Case | Observed result | Accepted finish | Limit |
| --- | --- | --- | --- |
| G1 `auto` | **Passed**, one accepted call, final usage/EOF and quoted settlement | `tool_calls` | Synthetic upstream protocol case only |
| G2 `none` | **Uncertain stop** at `tokenizer_agreement` | `stop` | SDK/transport completion returned, but the test-only final-input/tokenizer equality check rejected |
| Required/multiple, fresh named, continuation | **Not run** | — | No next case, retry or continuation after uncertainty |

Both reached tokenizer/generation HTTP **200**. G2 accepted zero tool-call deltas, not evidence of zero upstream requests. The SDK returned a valid completion and usage before the test-only equality check failed; this is **not an SDK decoding failure** and does not show that the production gateway would reject the same counts. Production settlement does not impose tokenizer equality. Exact count values, mismatch direction, raw output and underlying provider/tokenizer cause were not retained. G2's semantic response check and later diagnostic output/context/cost bounds were not reached, so they are not passing evidence.

- G1's authenticated-usage quote: **6,459 microunits ($0.006459)** including markup.
- G2's conservative uncertain worst hold: **1,373,799 microunits ($1.373799)**, retained in full.
- Latest cumulative state: **eight generation attempts / 7,768,425 microunits ($7.768425)** quoted use/holds. This consists of **six uncertain holds / 7,755,575** plus F and G1's usage-based quotes totaling **12,850**. All five older unknown holds remain unchanged. Actual provider billing remains **unknown**, not a proven invoice ceiling or gateway account debit.
- The report is **`qualified:false`**. No host tool ran; no tool-result continuation, fallback, profile/approval change, release, deployment or commit occurred. Production tool qualification remains empty and accepted v0.0.9 remains text-only.

Evidence: `report.json`, `outcome.json`, `ledger.jsonl` (eight complete checkpoints), `authorization.json`, `prior-report.json`, `prior-outcome.json`, fixed source hashes and gate logs in `/tmp/possums-tool-qualification-17yadt8l/`. Its last durable checkpoint agrees with the terminal report and retains the active G2 hold. Executed source hashes match preflight; **100 final scoped preservation checks** pass, including unchanged production SDK/transport/accounting, profiles/approval/pins/dependencies, client/Pi sources, original user documents, HEAD/index and credential metadata. This is not a whole-filesystem/runtime privacy proof.

**Authorization exhausted on uncertainty. Do not rerun.** The executed G source had a **pre-G** seed (`6,388,167 / six`); unused cases or quoted headroom **2,231,575** are not permission. Any separately authorized future work needs the latest **7,768,425 / eight** ledger and must retain every unknown hold. The subsequent offline alignment is recorded below, not a paid replay. The full five-case qualification remains incomplete.

## Offline alignment with Pi's reported-usage handling

User approved removing the extra harness equality gate after checking pinned **Pi 0.99.2**. Its [OpenAI-compatible adapter](https://github.com/earendil-works/pi/blob/v0.99.2/packages/ai/src/api/openai-completions.ts) normalizes provider-reported usage, including separate cache counts, without a pre-send tokenizer/final-input equality check. Inspected Responses and Anthropic paths likewise use provider usage; context estimates and metadata-derived UI costs are separate. This is prior art, not a Tinfoil tokenizer guarantee or an explanation of G2's mismatch.

Only the **test harness** changed: tokenization remains required for pre-send admission, but no longer supplies an equality requirement or a tighter final-cost ceiling. The complete worst reservation stays held throughout generation; after the unchanged production SDK consumer accepts final usage and successful transport EOF, the harness prices the reported counts at the submitted catalog rates. Output/context limits, checked usage arithmetic, positive output, actual quote within the held reservation, single settlement, durable checkpoints and stop-on-uncertainty remain. No production decoder, transport, accounting, receipt, client/Pi code, dependency, profile or approval changed. The historical G entry now explicitly refuses replay before any evidence/credential/network work, like F; its pre-G source snapshot remains preserved as historical evidence.

Two new synthetic regressions failed at the old equality gate before the fix. **24 focused harness tests** now pass, including lower/higher final input counts, a valid final quote above the tokenizer-based estimate but below the still-held reservation, duplicate settlement rejection, and above-reservation failure retaining the hold/stopping. Fragmented actual-SDK fixtures verify changed counts and reject a post-DONE transport failure without releasing the reservation. Complete offline/locked Rust verification: **308 passed, zero failed, five funded ignored**, 23 suite reports; format/whitespace and all-targets Clippy pass. Active LSP reported no errors/warnings, with three informational suggestions left unchanged (Rust 2024 syntax and intentionally split UTF-8 fixture text). An exact isolated invocation of the exhausted G entry returned its fixed refusal, as expected, without inference. Independent scoped read-only review found no concrete defect and reran **24 harness tests** successfully; **100 final scoped preservation checks** pass. Evidence: `/tmp/possums-final-usage-local-5atdovld/`.

**No paid request, credential read, retry, deployment or commit.** G2 is not retrospectively accepted or re-costed: its exact final counts and later bounds/semantic results were not retained. Live state remains **eight attempts / 7,768,425 quoted microunits**, including all six uncertain holds; actual invoice unknown. Full live tool qualification and future funding authorization remain outstanding.

## Latest authorized fresh suite after usage alignment (specimen H)

User authorized preparation and spending for **one fresh Kimi-only five-case suite** after the offline usage-alignment fix. Current seed: **7,768,425 quoted microunits / eight attempts**, retaining **six unknown holds / 7,755,575** plus **12,850** usage-based quotes. Existing **$10 cumulative quoted ceiling**, at most **five new generations / thirteen cumulative**, starting headroom **2,231,575**. Actual invoice remains unknown. Stop on failed/uncertain/unaffordable case or journal failure, with no retry, model fallback or host-tool execution. No production profile/approval/release/deployment/commit is authorized.

Fresh H1–H4 synthetic instructions cover auto, none, required/two calls and named; H5 uses only H4's validated actual call/result IDs, accepted finish and fixed synthetic result. Tokenizer admission remains before generation; final reported usage need not equal its count and must fit the full held reservation. Historical G's exact entry remains hard-refused. H has a distinct exact test entry and `TINFOIL_KIMI_TOOLS_H_FUNDED=yes` plus exact `kimi-k3`, checked before evidence/credential/network work.

Local preparation: `/tmp/possums-kimi-h-local-1mnl1oar/`; fresh private funded evidence: `/tmp/possums-tool-qualification-r_biyrkn/`. The one-shot launcher checks latest prior report/outcome, bounds, fixed preflight hashes, private evidence/ignored mode-0600 credential metadata and an exclusive durable marker before a clean-environment exact ignored-test invocation. Only the isolated funded process read the named key; assistant/offline work used metadata only. The observed stop follows. Full-matrix upstream evidence still does not grant production or real Pi-to-gateway acceptance.

### H preflight and observed stop

Local preflight passed **308 Rust tests, zero failed, six funded ignored**, 23 complete offline/locked suite reports; **24 focused harness tests**, format/whitespace and all-targets Clippy. LSP emitted three informational suggestions only. The actual no-opt-in H entry rejected before evidence/credentials/network work. Initial review requested frozen source/scope binding before execution; the launcher gained explicit preflight-to-authorization scope/seed/cap/SHA checks. Independent verification then checked **all 104 frozen manifest entries**, matching the current harness, snapshot, launcher, prior evidence and HEAD, and cleared the procedural concern. Preflight acceptance preceded the single launch. These are scoped checks, not invoice or production acceptance.

The suite ran **once**, exited **101**, and stopped on its third case:

| Case | Outcome | Accepted finish/calls | Authenticated-usage quote including markup |
| --- | --- | --- | --- |
| H1 auto | **Passed** | `tool_calls`, one call | **5,398 microunits** |
| H2 none | **Passed** | `stop`, zero calls | **3,625 microunits** |
| H3 required/multiple | **Not qualified**, stopped | `length`, zero accepted calls | **14,612 microunits** |
| H4 named / H5 matching-result continuation | **Not run** | — | — |

All three reached tokenizer/generation HTTP **200**, accepted final usage/SDK transport EOF and passed quoted-cost bounds; report stages are **`settled`**, accepted, with no inference failure. H3 is a valid quote-settled but semantically incomplete generation, **not an SDK/usage/EOF error or an uncertain new reservation**. It did not provide the required two accepted calls under the **512-token diagnostic allowance**. The report does not retain exact counts, partial text or reasoning allocation; do not infer which internal budget consumed the allowance, unsupported required-mode behavior, or absence of upstream work from zero accepted calls. H2's fresh pass does not retrospectively accept or re-cost G2.

- New usage-based quotes: **23,635 microunits ($0.023635)** across three generations. No new unknown hold was added.
- Latest cumulative ledger: **eleven attempts / 7,792,060 microunits ($7.792060)** quoted use/holds, consisting of unchanged **six unknown holds / 7,755,575** and **five usage-based quotes / 36,485**. Actual provider invoice remains **unknown**; this is not a gateway account-debit record.
- Report remains **`qualified:false`**. Eleven durable prompt-free checkpoints reconcile with the report, with no active new hold at the stop. No retry, fallback, continuation or host tool executed. Production/profile/approval/client/Pi/dependency/release state is unchanged; no deployment or commit.

Evidence: `/tmp/possums-tool-qualification-r_biyrkn/` (`report.json`, `outcome.json`, `ledger.jsonl`, `authorization.json`, exclusive `run-started`, `exit-code`, fixed source hashes, preflight snapshot and logs) plus `/tmp/possums-kimi-h-local-1mnl1oar/`. Executed source/launcher hashes match preflight; **100 final scoped preservation checks** pass. The historical sources and reports remain preserved, not rewritten as passes.

**H authorization exhausted on the failed case. Do not rerun.** Its archived source has a **pre-H** seed (`7,768,425 / eight`); the current H entry explicitly refuses replay. The five-case result remains incomplete. The user waived the proposed offline allowance review and subsequently authorized a smaller multiple-call/result proof followed by real Pi acceptance and a reviewed production rollout, not another five-case retry.

## Production-path prerequisite round (specimen I)

The user authorized scoped source/image/tag publication, verified production deployment and at most **four new generations** under the unchanged **$10 cumulative quoted ceiling**: two for a fresh multiple-call/matching-result proof, then two through real Pi. No separate staging environment was requested. I uses the authenticated selected model's **maximum output**, with the full reservation before tokenizer DATA; it never reduces that allowance to fit credit. Only two cases are eligible. Their fixed synthetic results use both actual validated call IDs; no host tool executes in this prerequisite.

Local gates: **315 Rust tests passed, seven ignored**, 24 suite reports; **30 focused harness tests passed**, format and all-target Clippy passed. Active LSP emitted only three informational suggestions. Independent scoped review found no concrete blocker; **100 preservation checks** and **105 frozen source/launcher/evidence hashes** passed. Evidence: `/tmp/possums-prod-round-local-p9dsp8jp/` and `/tmp/possums-tool-qualification-_nh3pn3r/`.

An initial launcher field-name error rejected **before** its start marker, credentials or network work. The exact read-only guard was corrected and rechecked before the single actual launch. I then exited **101** at the **reservation** gate: `skipped_reservation`, no tokenizer/generation HTTP status, no accepted call or finish, `qualified:false`. The report does **not** retain the exact rejection reason or required maximum quote; it cannot distinguish model-limit validation, quote arithmetic or quoted-ceiling rejection. Do not invent that cause.

**Zero new generations and zero new quoted holds.** All three durable checkpoints retain **eleven attempts / 7,792,060 microunits**, no active hold, unchanged **7,755,575 unknown holds** and **36,485 usage quotes**. Remaining quoted headroom is **2,207,940**; actual invoice remains unknown. No prompt was transmitted, retry/fallback/host tool executed, profile enabled, source committed/published, approval changed or deployment performed. Release preparation is an isolated local worktree only.

**I's one-shot authorization is exhausted. Do not rerun its source or launcher.** Production tools remain disabled. The next concrete blocker is to establish a closed reservation-rejection reason and authenticated maximum/quote metadata before proposing any new prompt-bearing attempt; this is not the waived allowance audit or another broad matrix.

## Later gates

Successful observed choice/tokenizer/UTF-8/multiple-call/matching-result/finish/usage/EOF coverage is necessary before proposing exact model profiles. Production-path verification, a new measured release and explicitly reviewed approval, real Pi↔gateway tool/accounting/privacy tests and scoped acceptance remain separate. No source diagnostic or passing fixture alone satisfies them.
