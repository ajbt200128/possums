# Local request-admission diagnostic — offline verification

## Incident and scope

The operator reported `possums_request_unexpected`, stage `request`, after a large DeepSeek reply and approximately thirty minutes before a follow-up. The exact incident is **not reproduced** and its underlying cause remains unknown. No private transcript, credential store, live inference, public evidence request, installation or deployment was accessed/performed.

A synthetic reply of 24,000 repeated text/emoji lines (over 500 KiB), followed by a fake-clock advance of thirty minutes, preserves the exact admitted request and succeeds through the provider fixture. This rules out that size/delay combination in this fixture only, not a stale production connection or another host/history/tool condition. Auth/trust expiry has its existing separate diagnostics and renewal behavior.

The exact reported generic code **is reproduced offline** when the existing local invocation admission rejects a tool schema containing an external `$ref`. `clients/pi/wire.ts` delegates to `admitInvocation`; its known `ChannelError(rejected)` previously fell through `safeFailure` to the unexpected fallback while the stage was still `request`.

The three-line production change classifies that existing, pre-dispatch rejection as `possums_request_schema`. It reports local history/tool admission failure, no inference request from **this attempt**, and unchanged/unknown prior billing. It does not accept the rejected schema, remove tools, reduce history/output allowances, change validation, renew sessions, replay work or change retry/accounting policy. Unexpected exceptions remain unexpected; this is a diagnostic fix, **not a confirmed fix for the operator's original incident**.

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

No gateway/client wire contract changes; old-client gateway compatibility qualification is not needed for this local presentation-only patch. Shipped Pi 1.0.4 and current Pi 1.1.0 supported conversion/runtime paths were exercised offline; this does not qualify an installed or running client.
