# Agent/history quota simplification — candidate, not deployed

The operator requested removal of arbitrary agent, conversation and tool quotas, then confirmed that the existing defensive HTTP/parser/memory safeguards should remain. This change introduces no replacement request-count quota or new resource-admission framework. It is separate from the new-Pi-session release discovery/verification work and requires a reviewed paired gateway/client rollout. Do not install the candidate against the old gateway and call the history failure fixed.

## Removed

- Three simultaneous credit-backed reservations per account. Prompt-free in-flight bookkeeping, the bounded reservation/idempotency store, credit conservation and absorbing terminal outcomes remain.
- The independent four-generation admission semaphore. Passive, owned lifetime tracking replaces its occupancy bookkeeping; observations cannot reject inference or alter accounting.
- Request-history tool-call counts, both cumulative and per assistant message.
- The 64-entry advertised-tool catalog quota and 4,096-message request-history quota.
- Historical 64-KiB per-call and 256-KiB cumulative argument quotas. History remains inside the existing bounded request envelope. The client still validates object arguments and history integrity; the gateway treats arguments as opaque transcript data, not execution authorization.

## Retained defensive and protocol boundaries

The original quota-only candidate retained four Heavy/Ingress lanes; that historical phase did not permit a fifth simultaneous heavy request. The subsequent operator-approved [control-admission removal](verification.md#control-request-admission-removal-candidate) removes the single control semaphore, and the later [chat admission removal without replacement](verification.md#chat-admission-removal-without-replacement--source-candidate) deletes both four-slot Heavy/Ingress gates. No replacement count, queue or byte-based admission is introduced. Existing connection and per-request defenses remain; connection capacity does not bound disconnected accepted work. There is no remaining shared workload-memory envelope or whole-process memory guarantee. The former New chat guard was removed with the browser route.

Existing HTTP/body, JSON byte/node/depth, transport, stream-buffer and timeout defenses remain. In particular, live completion state still has its existing 64-call, 64-KiB-per-argument and 256-KiB-total-argument bounds. Those live-state bounds no longer reject historical calls or constrain the advertised tool catalog. Protocol name/ID checks, unique identities, call/result completeness and ordering, schema validation, model context, full maximum-cost reservation, endpoint attestation/key binding, authenticated final usage/DONE/EOF, exact-once settlement/refund, and no automatic replay remain.

The published-provider review did **not** establish that absence of a documented limit means unlimited allocation is safe:

- [Anthropic HTTP errors](https://platform.claude.com/docs/en/api/errors) documents **32 MB** for Messages and Token Counting, not file/image limits applicable to every chat endpoint.
- [OpenAI TypeScript client](https://github.com/openai/openai-node/blob/main/src/client.ts) and [Anthropic TypeScript client](https://github.com/anthropics/anthropic-sdk-typescript/blob/main/src/client.ts) document a default **600,000-ms request timeout**. This is not evidence of a universal ten-minute generation/stream lifetime. The installed Anthropic SDK arms the timer around `fetch` and clears it when `fetch` returns; body consumption is separate.
- [OpenRouter limits](https://openrouter.ai/docs/api/reference/limits) documents credit/rate controls, not a numeric general chat JSON-body or stream-lifetime ceiling in the reviewed page.
- Pi **1.0.4** and **1.1.0** built-in OpenAI/Anthropic integrations pass optional `timeoutMs` to their SDK and do not add these Possums history/account quotas. They set SDK `maxRetries: 0` for individual attempts and use separate harness retry machinery. This candidate does not enable that machinery for Possums or adopt automatic retry/fallback guidance.

Those provider facts are not Tinfoil compatibility, deployment memory-headroom or live-model qualification evidence. Existing defensive size/time bounds are unchanged in this quota-only candidate; any later alignment needs to distinguish request setup from whole-stream behavior rather than copying a number into unrelated stages.

## Telemetry and verification scope

Policy: [data handling](../PRIVACY.md#data-handling-boundaries), [forbidden exports](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data), [reviewed MVP release scope](../PRIVACY.md#reviewed-mvp-release-scope), and [required evidence](../PRIVACY.md#required-evidence-for-every-telemetry-change).

Generation occupancy still measures the same owned lifetime. Its configured capacity remains the effective bound inherited from the existing heavy-memory semaphore, not a separately enforced generation quota. This quota change leaves metric names, closed labels, configured capacity values and five-minute lifecycle/arithmetic/complete-window gates unchanged; no infinity/zero sentinel or fabricated capacity is exported. The subsequent control-admission removal separately omits the removed `control` capacity point. Terminal/prompt-bearing work is destroyed first, then generation observation/activity retire before heavy admission can be reused. A queued semaphore-waiter regression checks this order synchronously for both owner envelopes. Telemetry-disabled inference uses the same passive bookkeeping and accounting behavior. This is not an expansion of metric scope or a privacy/anonymity claim.

Synthetic regressions cover longer histories, larger historical arguments, more advertised tools/messages and more credit-backed reservations, while retaining malformed/null/duplicate/orphan/pending/schema/parser/transport and live-stream negatives. Exact executed checks and remaining rollout gates are recorded in [verification](verification.md#agenthistory-quota-simplification-candidate).

The previously observed pre-output `request interrupted` failure still has no established cause, and its billing remains unknown. No original conversation or tool data was replayed, no paid inference was automatically retried, and no installed package, gateway deployment, release approval or credential was changed by this source work.
