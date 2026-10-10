# Pi trust/auth renewal — blocked before lifecycle changes

This is new **offline source/SDK evidence**, not a revision of the historical
[session-update record](pi-session-update-verification.md). Timeout alignment is
implemented separately; automatic expiry renewal and auth-expiry tracking are
**not implemented** in this packet. No certificate, approval pin, validity check,
credential flow, transcript or lifecycle integration was weakened to work around
the blocker.

## Approved goal, not yet implemented

Trust expiry must not become a conversation lifetime. Keep the transcript, verify
a fresh published context when needed at a fresh explicit submission, then
reauthenticate before sending the new prompt. Never replace trust during an
accepted operation, tool continuation, automatic retry or compaction. Auth-only
renewal may reuse unexpired verified trust. Stop must never refresh or resume
work. Compiled approval expiry remains administrative and terminal.

The current provider still caches public trust (including failures) for its trust
session. Published validity remains `min(12 hours, certificate expiry)`; credentials
have their existing server expiry. Expiry does not erase history, but subsequent
requests fail closed. The timeout change does not implement the approved
indefinite-conversation behavior or claim `/new` is its solution.

## Pinned runtime evidence

Read completely before evaluating integration: the supplied release **1.1.0**
docs `sdk.md`, `extensions.md`, and linked `sessions.md`, `compaction.md` and
`custom-provider.md`. Those docs describe concepts, not authority for the pinned
runtime. Actual source inspected under:

`/Users/mb5/.pi/agent/install/releases/1.0.4/node_modules/@earendil-works/pi-coding-agent/dist/`

- `core/agent-session.js`, `prompt()`: `input` runs before queue routing; it has
  `source` and `streamingBehavior`. Streaming steer/follow-up queues do not pass
  through `before_agent_start`. Idle submission checks/possible compaction occur
  **before** `emitBeforeAgentStart`; the hook also handles extension-originated
  `sendUserMessage` prompts, not exclusively explicit human submission.
- The native tool loop, queued continuation and retry recovery are owned by
  `_runAgentPrompt` and `_prepareRetry`, not by `before_agent_start`. Refresh in
  per-inference auth resolution would therefore risk changing release on a retry
  or tool continuation. That fallback was not used.
- `core/extensions/types.d.ts`: `BeforeAgentStartEvent` contains prompt/images and
  system-prompt options, but no input source, durable submission identity or
  cancellation token. `InputEvent` does distinguish interactive/RPC/extension and
  steering/follow-up, but its idle dispatch is also outside the active run.
- `core/extensions/runner.js`, context `signal`: delegates to the session's
  `getSignal`; `agent-session.js` supplies `this.agent.signal`. During idle
  `before_agent_start`, this is **undefined**, and `ctx.isIdle()` is **true**.
- `AgentSession.abort()` marks `_agentRunAbortRequested` only when the run is
  already active, aborts existing retry/compaction/agent controllers and waits for
  idle. `_runAgentPrompt()` initializes the run after the hook and clears its abort
  flag. Stop during an awaited pre-run hook does not cancel the pending prompt.

### Actual-SDK reproducer

`tests/phase02_pi.mjs`, test **renewal blocker: Pi 1.0.4 Stop does not cancel an
awaited before_agent_start hook**, uses the real pinned `createAgentSession`,
extension runner and shipped extension with a synthetic provider. It holds an
async hook, observes undefined signal/idle=true, calls and awaits `session.abort()`,
then releases the hook. The pending prompt still makes **one** synthetic inference
call. No renewal is installed: this test establishes why simply awaiting network
verification in that hook would violate the requested Stop semantics.

The full [timeout scratch check command](provider-timeout-alignment.md#local-verification)
passed **71 Pi cases**, including this reproducer and existing actual-SDK native
retry, steering, tool, compaction, session/account replacement and unknown-receipt
billing regressions. No real credential, network inference, installation or
transcript upload was used.

## Decision needed / separable follow-up

A reliable **cancellable** explicit-submission boundary was not demonstrated.
Input source alone is insufficient: even correctly identified idle input can
outlive Stop while verification awaits. Do not paper over this with an
unconditional refresh, expiring-certificate extension, `/new`, monkey-patched
abort, or replay of the pending prompt.

Choose and qualify a supported host lifecycle that provides a submission-owned
abort signal from acceptance through preflight, or explicitly scope renewal to a
separate cancellable, deliberate authentication/reconnect operation that sends
**no inference** and requires a later fresh submission. The latter changes the
requested automatic-on-submit UX and needs that decision recorded. No Pi upgrade
or command mechanism was selected here.

After that boundary is agreed, the separable implementation packet is known-auth
expiry tracking and atomic trust/auth/catalog publication with session/account/
logout/abort epochs. Required new tests remain: expired explicit submission
renews without losing history; failed verification sends neither credential nor
prompt; auth-only refresh; no release swap in retry/tool/compaction; and stale
renewal cannot revive stopped work. Existing passing race tests do **not** qualify
these unimplemented renewal cases.

Privacy references: [data handling](../PRIVACY.md#data-handling-boundaries),
[forbidden telemetry](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[synthetic testing](../PRIVACY.md#synthetic-testing-exception). Only local closed
assertions and synthetic fixtures were added; no telemetry or persistent diagnostic
record of user prompts, identities, credentials or request-level billing.
