# Local request-admission diagnostic — offline verification

## Incident and scope

The operator reported `possums_request_unexpected`, stage `request`, after a large DeepSeek reply and approximately thirty minutes before a follow-up. The exact incident is **not reproduced** and its underlying cause remains unknown. No private transcript, credential store, live inference, public evidence request, installation or deployment was accessed/performed.

A synthetic reply of 24,000 repeated text/emoji lines (over 500 KiB), followed by a fake-clock advance of thirty minutes, preserves the exact admitted request and succeeds through the provider fixture. This rules out that size/delay combination in this fixture only, not a stale production connection or another host/history/tool condition. Auth/trust expiry has its existing separate diagnostics and renewal behavior.

The exact reported generic code **is reproduced offline** when the existing local invocation admission rejects a tool schema containing an external `$ref`. `clients/pi/wire.ts` delegates to `admitInvocation`; its known `ChannelError(rejected)` previously fell through `safeFailure` to the unexpected fallback while the stage was still `request`.

The initial three-line production change (`baa1024`) classifies that existing, pre-dispatch rejection as `possums_request_schema`. It reports local history/tool admission failure, no inference request from **this attempt**, and unchanged/unknown prior billing. It does not accept the rejected schema, remove tools, reduce history/output allowances, change validation, renew sessions, replay work or change retry/accounting policy. Unexpected exceptions remain unexpected; this is a diagnostic fix, **not a confirmed fix for the operator's original incident**.

## Evidence

Node 24.13.0; locked cached verification dependencies; scratch `/tmp/possums-pi-build-followup-aY7FdX`. No dependency download. Baseline synthetic tests confirmed the old `possums_request_unexpected` for an external schema reference and successful large/delayed follow-up. Updated regression tests cover external `$ref`, unsupported schema root and resource `$id`, repeated rejection, zero fixture inference dispatches, no billing diagnostic fabricated, native retry ineligibility and no hostile schema sentinel in assistant events/errors.

Passed:

- Full `clients/pi/check.mjs` pinned Pi 1.0.4 scratch harness, including reference/transport, forwarding, release, cache, provider/native SDK, discovery and shared admission groups.
- The provider/native SDK suite with all three scratch runtime peers switched to Pi 1.1.0, matching the explicitly selected test runtime.
- Fresh production build (`fixed-package`), pinned TypeScript check and `git diff --check`.
- Independent read-only review: no actionable finding; confirmed the request-stage mapping cannot mask post-receipt settlement failures and that the idle incident remains unresolved.
- Active LSP probe of changed files: no compiler errors after temporary dependency links; existing lint warnings/hints outside the change remain. Temporary checkout dependency links were removed after verification.

An initial rebuild into the already-populated scratch output failed on an existing peer symlink; a fresh output directory then built successfully. Nothing was installed. Installed/running T3 activation, actual DeepSeek wire behavior, original history/tool declarations, production delay, exact billing and the incident's cause remain unverified. Only closed code/stage/constraint/status should be shared for further diagnosis, not a transcript or raw exception.

## Privacy and compatibility

Reviewed [mandatory change control](../PRIVACY.md#mandatory-reference-and-change-control), [data handling](../PRIVACY.md#data-handling-boundaries), [forbidden telemetry/errors](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data) and [synthetic testing](../PRIVACY.md#synthetic-testing-exception). No logger, trace, support artifact, raw exception retention or telemetry pipeline is added. Normal Pi/T3 assistant-error history persistence remains a host boundary.

The initial diagnostic-only patch changes no gateway/client wire contract. Shipped Pi 1.0.4 and current Pi 1.1.0 supported conversion/runtime paths were exercised offline; this does not qualify an installed or running client.

## Requested request-preparation follow-up (steps 1–4)

The operator subsequently requested specific validation, SDK normalization, actionable errors, a distinct unknown conversion boundary, and a short fuzz pass, with simple code and sensible relaxation of unnecessary client restrictions.

Implementation reuses existing descriptor-aware admission/parsing and `DiagnosticFailure`, not a new validator framework. Existing assertions now attach closed request constraints for tool names/IDs, declaration shape, schema references, tool choice, history structure/order and historical object arguments. Boundary catches preserve those categories and the existing JSON-depth error through payload-hook re-admission. No exception text or offending value enters a diagnostic. SDK conversion exceptions become `possums_request_conversion`, explicitly with an unknown underlying cause. A provider-owned pre-submission flag permits the no-inference statement for these failures only before submission setup begins; the same typed error after that boundary keeps billing unknown. Earlier settled outcomes are not rewritten.

Pi integration guidance and the pinned 1.0.4/1.1.0 OpenAI converter were inspected. The OpenAI `convertTools` and Gemini `google-shared.convertTools` paths both preserve JSON schemas absent an explicitly supported strict/legacy transformation; this client follows that pattern rather than copying the Gemini OpenAPI sanitizer or adding a strict-schema rewrite. `convertMessages` already resolves system/tool transcript updates, so `3b8affc` removes the redundant explicit `resolveTranscript` pass. `getCurrentTools` and the exported OpenAI converter remain authoritative; no SDK parser, reasoning handler, tool-ID repair or strict-schema rewriter was copied into this client. Missing/undefined optional descriptions are omitted as absent metadata, matching normal JSON request serialization. Parameter schemas and completed history are not silently rewritten or removed.

Two client-only checks are relaxed against the existing gateway `src/inference/tools.rs` contract:

- JSON schema object roots can omit `type`, permitting local `$ref`, combinator and unconstrained object schemas. Explicit non-object root types, external/rebased/dynamic references, descriptor and parser limits remain rejected. No reference fetching/resolver or new schema-validation dependency is added.
- Completed historical calls need not name a tool still in the current loadout. Their names/IDs, complete result batches and object arguments remain validated; **live** calls still require current declarations, supported choice, valid object arguments and authenticated successful completion. A negative stream test confirms a retired tool cannot become executable merely because its past call is accepted.

This extends local request acceptance without adding wire fields, changing gateway behavior or weakening live execution/accounting gates. Old and current Pi runtimes are tested; upstream model acceptance of every schema shape, deployed gateway behavior and original incident causation remain unqualified.

### Follow-up evidence and fuzz scope

Passed the full pinned Pi 1.0.4 scratch check runner (including **646** tool/client/transport checks), both project TypeScript configurations, `git diff --check`, and **156 provider/native SDK groups** on each of Pi 1.0.4 and 1.1.0 with matching scratch peers. The actual native `context_with_system` hook fixture preserves the typed schema-reference diagnostic without inference or native retry. Actual `ReferenceClient.prepareChat` hook replacements retain precise categories and make zero catalog/control/chat calls. Unexpected SDK `toJSON` failures lose their hostile exception text, while post-submission typed failure tests retain unknown billing. Independent read-only review reported no actionable finding and did not repeat the full harness.

After steps 1–4 passed, a deterministic **256-case** fuzz pass (seed `0x51a7`) varied schema roots/references, optional descriptions, names, text blocks, Unicode including unpaired surrogates, orphan results, retired calls, argument types and SDK conversion exceptions. It exercised conversion, admission and real preparation including payload hooks: **64 accepted, 192 rejected, zero network/inference dispatches** on each runtime. Expected closed codes, no cause/sentinel leakage, exact schema preservation and preparation cleanup were asserted. This is a short bounded mutation pass, not exhaustive fuzzing, network fault injection or a whole-process privacy proof; it found no additional defect.

Fresh production bundle SHA-256: `693e580b216c9a023ec3b6e45abea4ca020642e2e25041a20bec5fa8ecba1056` in the scratch `request-preparation-final`. No client installation, PR publication, gateway deployment or live inference occurred. Active LSP probes reported no errors but six push-only results were inconclusive; the pinned compiler checks are the TypeScript evidence. Temporary checkout dependency links were removed after verification. The privacy references and host-history persistence limits above apply unchanged.
