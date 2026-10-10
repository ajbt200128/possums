# Explicit-submit expiry renewal — local source candidate

Implemented on `fix/provider-timeout-alignment` after the operator selected
synchronous explicit-input/run marking followed by the **first native API-key
`auth.resolve`**. No network work was added to `input` or `before_agent_start`.
No installation, deployment, runtime upgrade, host edit, monkey patch, push or live
request occurred. This supersedes the *placement decision* in the
[earlier blocker record](pi-renewal-boundary-blocker.md), not its reproduced finding.

## Placement and authority

Pinned runtime: Pi **1.0.4** (`pi-ai`, `pi-agent-core`, `pi-coding-agent`), Node
**24.13.0**. The supplied 1.1.0 `sdk.md`, `extensions.md` and linked sessions,
compaction and provider docs were read; actual 1.0.4 source and SDK fixtures decide
behavior. No 1.1.0 upgrade is needed for this implementation.

1. `input` marks only idle, non-queued `interactive`/`rpc` input with Possums
   selected. It records booleans, not input text. Extension input is not permission.
2. `before_agent_start` synchronously arms that marker. Pre-prompt compaction has
   already run by this point; it cannot borrow the marker from `input`.
3. `agent_start` binds the candidate to Pi's active `AbortSignal`. The **first
   non-system `message_start` must be a user message** before permission becomes
   usable. Native initial user delivery precedes auth resolution.
4. The matching signal's first `auth.resolve` consumes permission **before any
   await or expiry decision**, even when everything remains valid. A request on
   that signal before the confirming user event consumes the unconfirmed candidate
   without renewing. Direct provider streams also discard any remaining permission.
5. Background model refresh has a different signal and cannot consume permission.
   End/settled/session/account boundaries clear markers. Retries, tool continuation,
   queued steering/follow-up, manual/pre-prompt compaction and extension requests do
   not acquire fresh permission.

Actual source chain: `Agent.runWithLifecycle` creates the controller before
`agent_start`; `runAgentLoop` emits initial messages before requesting a response;
`ModelRuntime.prepareRequest` awaits `getAuth` before calling provider streaming;
`operationSignal` preserves a supplied signal; API-key auth forwards that same
signal to `resolve`. Native model refresh instead creates a combined refresh
signal. Existing SDK auth cancellation stops the wait; renewal additionally
passes cancellation into verification/login/catalog operations and guards late
publication.

### Fail-closed correlation

Pi does not expose a durable submission ID across these events. Overlapping or
previously handled idle inputs are therefore considered ambiguous: they do not
renew. A later fresh, unambiguous submission can renew after the run settles.
Existing extension or concurrent-input behavior is not treated as human authority.

A new actual-SDK test found that an extension can call `sendCustomMessage` with
`triggerTurn` inside `before_agent_start`: that nested run bypasses both input
marking and another before-start event. Merely binding the next `agent_start`
would lend it the user's permission. The first-message check rejects this case.
Another test covers an earlier nested auth request using the active signal: it
cannot leave renewal permission for a later request. These cases fail closed;
they are not automatically resumed or replayed.

The original Pi bug remains: Stop while an *arbitrary other extension* awaits an
idle pre-run hook may not cancel that pending prompt. This implementation adds no
such wait and does not claim to fix the host. Once renewal starts in native auth,
the real run signal exists and Stop cancels it.

## Expiry, publication and accounting

- A valid cached context and bearer cause **no renewal verification/login** per
  turn. The existing authenticated per-invocation catalog behavior is unchanged.
- A fresh eligible submission with expired public trust verifies a new context
  before transmitting a recovery credential. Published trust still expires at
  `min(12 hours, serving certificate expiry)`; no expiry, verification or certificate
  check is removed. A returned expired context is rejected before login.
- Compiled-manifest establishment retains its administrative expiry and exact pins.
  Expired compiled approvals fail locally; discovery is never substituted for them.
- `ReferenceClient` records validated `expires_in` from successful login, measured
  conservatively from just before the session POST. The getter exposes only the
  expiry timestamp, never the bearer. Fresh clients and failed logins have no known
  auth expiry. Known auth-only expiry reauthenticates using still-valid public trust.
- Renewal clears usable old bearer/catalog state, invalidates the reconciliation
  window and advances ownership epochs. New public trust, bearer and catalog are
  published together only after current verification, login and catalog success.
  Session/account/logout/abort changes reject late results. The originating auth
  resolver retains the renewal epoch across its completion handoff.
- Failed/unfinished renewal cannot fall through to ordinary automatic restoration.
  Another fresh explicit submission may retry renewal; no prior prompt is replayed.
  Existing closed verification/session/catalog codes are retained. No raw errors,
  credentials, prompt data or upstream error text are added to diagnostics.
- Stored recovery credentials, transcript and conversation progression are retained;
  renewal does not call `/new` or reset `newConversation`. An invalidated balance
  window cannot fabricate a reconciliation match.
- Server restart/revocation can invalidate a bearer **before known expiry**. This
  change does not infer safe refresh/replay from an unauthorized response. That
  failure remains terminal; it is not automatic expiry renewal.
- Native bounded transient retries and unknown-charge disclosure are unchanged.
  Missing final receipts remain unknown. Abort cannot undo earlier accepted remote
  work or establish a refund.

## Local verification

Fresh scratch source: `/private/tmp/possums-pi-build-renewal-eROom8`. Existing
locked dependencies were linked from the earlier matching scratch tree; no
`npm ci`, package installation or host modification. Build output used for the
last check: `package-epoch`.

```sh
cd "$S/source/clients/pi"
PHASE02_OUT="$S/package-epoch" node build.mjs
POSSUMS_PI_ROOT=/Users/mb5/.pi/agent/install/releases/1.0.4 node check.mjs
```

Results: TypeScript **5.9.3** / esbuild **0.25.10** build passed; reference **35**,
encrypted/reference client **523**, release-policy **97**, actual-SDK/provider
**87**, shared admission **71** checks passed. LSP probes reported no errors but
could not confirm clean TypeScript rechecks; the compiler supplied type validation.

New checks cover:

- Real native source/active-signal ordering, auth before inference, distinct
  background refresh signal, and Stop during auth preflight.
- Interactive and RPC expiry renewal, unchanged prior transcript entries,
  preserved conversation progression, and no extra verification for valid turns.
- Known auth-only expiry and observed ReferenceClient login-expiry bookkeeping.
- Stop during public verification/login: no inference; no credential before
  verification; late results cannot publish; a later explicit turn can succeed.
- Failed verification/evidence/approval renewal: no credential/prompt and no
  renewal loop from background/extension requests; expired returned contexts fail.
- Expiry during native retry/tool continuation and queued steering/follow-up:
  no release swap; later idle explicit submission can renew.
- Manual/pre-prompt compaction and background refresh during input cannot consume
  or grant renewal permission.
- Nested custom runs, earlier nested auth and overlapping input origins fail closed.
- Logout/session/account replacement, late public verification and auth completion
  handoff reject obsolete work without inference or stale publication.

These are synthetic fixtures: genuine native Pi lifecycle/auth and genuine
ReferenceClient session parsing, with injected public verification and synthetic
session/catalog/inference endpoints. Auth-only expiry and trust passage are
injected fixture state, not a twelve-hour live wait. The separate 97 release tests
retain SDK verification orchestration and expiry checks with documented crypto/
hardware/network mocks. No new live certificate, provider, billing or platform
privacy qualification is claimed. Existing native-v3 CLI signer and legacy-v2
freshness limitations remain unchanged.

The six pre-existing dirty Rust/transport files were left byte-for-byte untouched
and were never staged. No Rust suite was rerun for this client-only packet. No
startup/auth race fix from another worktree was merged. Integration conflict points
are `auth.resolve`/`authenticate`/`logout` in `clients/pi/provider.ts` and lifecycle
handlers in `clients/pi/index.ts`; later integration must preserve these epochs,
signal-bound one-use permission and atomic publication.

Privacy references: [data handling](../PRIVACY.md#data-handling-boundaries),
[forbidden telemetry](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[synthetic testing](../PRIVACY.md#synthetic-testing-exception). No new telemetry,
request traces, logging, persistent diagnostic ledger or transcript exports.
