# Request body and upload alignment — source candidate

The operator requested alignment with normal provider request sizes and upload timeouts. The selected local baseline is **32 MiB** for the complete chat request and **600 seconds** for total chat-body upload. This is not a universal provider or Tinfoil guarantee.

[Anthropic documents 32 MB](https://platform.claude.com/docs/en/api/errors#request-size-limits) for Messages and Token Counting requests. The inspected installed OpenAI 7.19.0 and Anthropic 0.129.0 SDKs default to 600,000 ms request timeouts; this is a comparison to SDK request behavior, not a provider promise about server-side upload handling. The local byte ceiling uses binary MiB explicitly.

Gateway HTTP bodies, reference/Pi client chat serialization, and the full upstream generation/tokenizer envelopes each have a 32-MiB ceiling. An admitted HTTP body does not guarantee that its independently encoded upstream envelope fits. Controls remain 4 KiB with a 30-second total body deadline; shorter synthetic overrides still apply. Header bounds/deadlines, structural JSON limits, model context enforcement, authentication, credit reservation, settlement/refund, cancellation and retry policy are unchanged. No history truncation, model substitution or output reduction is added.

## Local evidence

- Parent reran `cargo +stable test --all-features`: **327 passed, 3 live tests ignored**.
- Exact generation/tokenizer serializer boundaries at 32 MiB passed; oversized inputs fail before upstream sending.
- An authenticated near-32-MiB history exceeds the former 8-MiB ceiling and reaches the synthetic upstream; existing over-body/parser/header negatives remain.
- Paused-clock tests prove default chat/control upload deadlines of 600/30 seconds, plus shorter overridden total trickle deadlines.
- Parent reran the lock-matched scratch reference/Pi check runner and phase01 TypeScript checks after copying the updated source/tests. Client size checks and shared admission regressions passed; no live inference or public network was used.
- Formatting and whitespace checks passed. Rust checks used installed stable 1.99.0 because pinned 1.88 lacks Cargo locally; pinned-toolchain/deployed-artifact qualification is not claimed.

The larger body invalidates historical per-lane and 512-MiB memory arithmetic. The allocation fixture's 1-GiB threshold is only a finite synthetic regression guard, not an application admission ceiling, whole-process bound, measured RSS or SDK/TLS qualification. The implementer observed a 291,912,745-byte peak delta for that specific run; it is not a concurrency capacity guarantee. The separately requested admission/backpressure/tool changes require their own integrated checks.

Privacy references: [data handling](../PRIVACY.md#data-handling-boundaries), [forbidden telemetry](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data), [synthetic testing](../PRIVACY.md#synthetic-testing-exception) and [required evidence](../PRIVACY.md#required-evidence-for-every-telemetry-change). Tests use synthetic content and throwaway fixtures; no request logs/traces, real history, credential values or support bundles were collected/exported. Existing native Pi/T3 history boundaries remain. No installation, release, deployment, paid inference or replay occurred.
