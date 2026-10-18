# Pi session failure after gateway replacement — offline investigation

## Initial investigation checkpoint

This section records the pre-fix checkpoint. The subsequently authorized source
correction and its qualification are recorded below.

Investigation and synthetic regression tests only. No runtime/client lifecycle code,
recovery policy, installed package, activation, gateway, release or deployment was
changed. No production endpoint, inference, replay, credential store or affected
session was accessed. The operator's new-session workaround is sufficient for now.

**Proven mechanism, not a proven production root cause:** a gateway TLS key change
can strand a previously verified v3 client session before its existing deployment
recovery entry points. A fresh trust session can select the replacement identity.
An identical image/configuration/manifest does not imply identical runtime keys.

The reported certificate diagnostic alone cannot distinguish a peer-SPKI mismatch
from certificate parsing, hostname or validity failure. The preceding interrupted
HTTP-200 delivery does not establish the cause of a later certificate failure,
successful generation, settlement or refund. Earlier billing remains unknown.

## Pre-fix source boundary

- [`certificate()` and `nativeRequest()`](../clients/pi/v3.ts): each request uses a
  fresh TLS socket. Exact endorsed SPKI, hostname and date checks precede HTTP
  headers/body. All certificate constraints currently become `trust/certificate`.
  An old pin therefore cannot obtain the replacement gateway's HTTP 401/403 or
  submission 422 response; it fails earlier without sending HTTP credentials.
- [`eligibleSetup()` and `perform()`](../clients/pi/provider.ts): recovery admits
  only closed catalog/submission unauthorized 401/403 and narrow submission
  endpoint-binding 422 observations, with the prepared request not dispatched.
  `trust/certificate` is ineligible. `trustFailure()` makes it fatal and
  `recoveryBlocked` prevents subsequent dispatch in that resident context.
- [`ReferenceClient.chatPrepared()`](../examples/phase01/client.ts): refresh the
  catalog, obtain a submission token, then set the monotonic dispatch latch before
  `Channel.chat`. A failure in that chat call is conservatively post-dispatch,
  even if TLS subsequently withholds HTTP. Never lower or reinterpret this latch.
- [`newSession()`](../clients/pi/provider.ts) discards the prior trust context and
  starts full establishment; ordinary auth resolution/model calls do not
  rediscover a new deployment on every healthy turn. Re-entering a credential is
  not a substitute for new public verification.
- [`recoverPublished()`](../clients/pi/bootstrap.ts) already performs full
  nonce-bound qualification and compares verified identity/freshness.
  `Channel.requireRenewal()` also requires advancing witness freshness. These
  checks, cancellation/account guards, shared budget, fresh authentication and
  atomic catalog publication must remain; recovery success is not guaranteed.

## Source, build, installed and running are distinct

Read-only inspection confirmed the installed extension's SHA-256 matches its
build report: `83624d2899fb58ea4488c41cebff1609926f13275344a47704fc8355d88238cc`.
The report pins Pi 1.0.4, Node 24.13.0 and Go 1.27.2. Its provider and reference
client module hashes match this source. Its transport/bootstrap/v3 module hashes
do **not** all match this checkout; the installed TLS certificate check and
pre-HTTP pinning were separately inspected and exhibit the relevant mechanism.
The installed package's current three Pi peer links resolve to **1.2.0**, not
its build-time 1.0.4 peers. That is an additional runtime-qualification gap, not
evidence that the host upgrade caused the failure. This does not establish which
artifact/runtime the affected process actually loaded, nor make this source
identical to the installed package. No auth, settings, credentials, raw reports
or session history were inspected.

## Offline reproduction and verification

Added two scoped regressions, with no new production fixture capability:

1. [`phase02_v3.mjs`](../tests/phase02_v3.mjs): real loopback HTTPS changes to a
   second self-signed certificate/key at the same address. An old pin produces
   `trust/certificate` and **zero additional HTTP requests**. A synthetic new pin
   reaches the server; restoring the original key restores the old-pin path.
   This tests native TLS, not hardware/provenance acceptance of the new key.
2. [`phase02_pi.mjs`](../tests/phase02_pi.mjs): the actual reference preparation
   and dispatch latch plus provider, with synthetic channel operations, reproduce
   transport/fetch with HTTP 200 -> warning/billing unknown -> later pre-dispatch
   trust/certificate -> fatal sticky failure. No recovery probe, submission token
   or generation is repeated after that certificate failure. Subsequent auth
   resolution is blocked. A new trust session independently establishes the
   replacement and makes one new synthetic invocation. A certificate error after
   the dispatch latch also stays fatal with zero recovery probes. Synthetic
   credentials are absent from terminal messages.

Final checks passed in `nix develop --offline`, Node 24.13.0, with a fresh
Go 1.27.2 native/WASM verifier build (`GOPROXY=off`, `GOSUMDB=off`), strict
TypeScript 5.9.3/esbuild 0.25.10 build and the complete scratch `check.mjs` suite:
165 Pi groups, 39 v3 checks, 18 packaged-host checks, 31 discovery checks, plus
reference-client, legacy-release, cache, thinking, diagnostics, streaming and
admission suites. All public acquisition/inference responses were synthetic;
only loopback TLS used real sockets. No live hardware, T3 UI or production billing
qualification is claimed.

Old scratch dependency trees were incomplete. Dependencies were restored from
local npm cache with `--offline --ignore-scripts --no-audit --no-fund`, isolated
HOME/config and scratch-only package trees. An initial runtime restoration mixed
Pi 1.0.4 with newer caret-resolved Pi companions and failed during module import,
not in the regressions. Recreating all seven Pi companion packages at 1.0.4
resolved it; the three build-checked runtime manifest hashes match the installed
report. The historical full transitive lock was unavailable, so byte identity of
that entire old runtime is not claimed. Installed peers were not modified.
Final scratch: `/tmp/possums-pi-build-session-recovery-geg8avx9`.
The active LSP probe returned no findings but could not confirm either changed
JavaScript file clean; executed suites and the scratch compiler provide the
verification evidence, not an empty diagnostics cache. `git diff --check` passed.

Privacy checks follow [data handling](../PRIVACY.md#data-handling-boundaries),
[forbidden exports](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data)
and [synthetic testing](../PRIVACY.md#synthetic-testing-exception). No logger,
telemetry, support bundle, raw-error classification or diagnostic-triggered work
was added. Pi/T3 native history persistence remains a separate boundary.

## Correction plan proposed at the initial checkpoint

Keep the current fail-closed workaround. If automatic continuity is desired,
request a narrow addition to the approved
[setup-recovery policy](blue-green-runtime-plan.md#packet-e--bounded-pi-deployment-recovery-before-inference)
and [fatal trust policy](pi-client-failure-policy.md):

1. Distinguish a locally observed peer-SPKI mismatch from malformed, wrong-host,
   expired/not-yet-valid and unexpected certificate failures using supported
   X509 operations and a closed content-free type. Check parsing, hostname and
   dates first; only an otherwise valid peer leaf may produce the mismatch type.
   A mismatch is **not proof** of a deployment or approval of the presented key.
   No raw certificate/key, exception or identifier belongs in its diagnostic.
2. Only for that narrow mismatch, a still-authorized run and affirmative
   `prepared.dispatched === false`, permit the existing bounded credential-free
   full recovery path. Never simply add generic `trust/certificate` to
   `eligibleSetup()`, suppress the trust failure or retry the failed socket.
3. Require all existing publisher/manifest/hardware/freshness/endpoint checks,
   fresh login and validated selected-model catalog, then guarded atomic
   replacement. Verify the actual new TLS peer before credentials. Unverified,
   nonadvancing, canceled or replaced contexts must send no credentials/prompt.
   Do not reset budgets, rerun payload hooks/tools or infer old billing from a
   later successful login/receipt.
4. Keep all other certificate/integrity failures and every already-dispatched
   failure fatal. Preserve unknown earlier/control outcomes and never replay an
   interrupted generation. A deliberately submitted new request may charge again.

Before implementation acceptance: add actual TLS-to-provider integration for the
new discriminator; wrong-host/date/parse/unknown negatives; full verifier
rejection and still-mismatched-new-peer cases; scope exhaustion/coalescing,
Stop/account/session races, unchanged/advancing freshness and hook-once tests;
and after-dispatch no-recovery tests. Requalify the emitted client in pinned Pi.
The existing new-session workaround needs no recovery-policy expansion; client
installation and user-controlled activation would require separate approval even
after a source fix passes. No gateway redeployment is proposed.

## Implemented correction and follow-up qualification

The operator subsequently authorized this client fix, PR publication/merge and
`no-rollout-needed`. This is not installation, activation or deployment authority.

- `certificate()` validates parsing, hostname and dates before comparing SPKI.
  Only its **actual TLS peer** call site can emit `trust/peer_key_mismatch`;
  mismatch of the public certificate against attested facts stays
  `trust/certificate`. Node's supported X509/TLS APIs remain the authority.
- `eligibleSetup()` admits just that additional closed observation, still behind
  the existing `!prepared.dispatched` boundary. The existing full verifier,
  advancing freshness, three-attempt shared budget, fresh authentication/catalog,
  coalescing and current account/run/cancellation guards are unchanged.
- Any unsuccessful replacement (including unchanged evidence and budget
  exhaustion) preserves the original peer-mismatch diagnostic and sticky trust
  block, with a separately closed secondary recovery explanation. Cancellation
  and obsolete account/run/session guards prevent late latching of a replacement
  session. A mismatch during replacement authentication or after dispatch stays
  fatal. A misleading/malicious peer can at most nominate bounded public
  verification, never approve itself. The actual replacement peer must match
  newly verified keys before any credentials or prompt cross.
- `possums_trust_peer_key_mismatch` reports that the peer key differed from the
  verified identity and explicitly leaves the underlying cause unknown. No raw
  exception, key, certificate, URL or request data is included. Billing still
  comes only from observed accounting evidence, not recovery success.

Adjacent recovery behavior was reviewed. Known-expiry renewal, authorized caller
eligibility, coalescing and the existing 401/403 and submission-422 paths already
cover their approved cases. No evidence justified making arbitrary transport,
invalid-certificate, crypto/integrity, unchanged/nonadvancing freshness or
already-dispatched failures recoverable. Those were **not** loosened; neither was
latched-trust recovery changed into silent credential-based reset. Status/checks
remain offline, and generation replay/tool reruns remain forbidden.

Added `tests/phase02_peer_recovery.mjs` to the normal scratch build/check runner.
It combines actual loopback TLS with the real reference preparation/dispatch
latch and provider recovery loop (candidate verification alone is synthetic):

- catalog and submission-stage peer mismatches: zero HTTP on the rejected peer,
  one replacement login and one generation dispatch, payload hook once;
- healthy path: no additional public recovery/authentication;
- unchanged evidence, verifier rejection and still-mismatched replacement peer:
  no replacement credential/inference; original diagnostic and trust latch survive
  run settlement and a later explicit run, with safe secondary diagnostics;
- repeated rotations: at most three shared recovery attempts, no inference,
  exhausted recovery retains the primary trust failure/latch;
- post-dispatch mismatch: fatal, no HTTP transmission or recovery/replay;
- cancellation, logout and session replacement: no late credentials/prompt;
  logout/new-session replacement at rejection-propagation microtask depths 0–6
  cannot relatch an obsolete verifier failure, and late nonrecoverable
  certificate failures cannot mutate replacement trust state;
- concurrent waiters: one verifier/login owner; one caller's abort does not kill
  the surviving request.

The v3 TLS suite additionally exercises wrong-host, expired/not-yet-valid and
unexpected date-check failures even when SPKI differs: all stay generic fatal
certificate failures with zero HTTP requests. The existing public-evidence,
unchanged/nonadvancing/advancing freshness, full verifier rejection, native
Pi lifecycle/tools/compaction/account races and hostile-diagnostic suites remain
in the full runner. These fixtures establish no production root cause, live
hardware acceptance, accounting outcome or installed T3 UI/runtime qualification.

Pinned offline build and the full `check.mjs` runner passed at
`/tmp/possums-pi-build-session-recovery-bls8bw_i`: 165 Pi groups, 47 v3 checks,
16 TLS/provider recovery groups, 18 packaged-host and 31 discovery checks, plus
the other unchanged reference/cache/stream/diagnostics/admission suites. Strict
TypeScript compilation passed. The checkout-only LSP lacks installed Pi/Node
declarations; its missing-module/cascading-any findings were dispositioned as
environment-only after the pinned compiler passed, not suppressed in source.

The same 16 TLS/provider recovery groups also passed in a separate scratch
bundle resolving the current read-only Pi 1.2.0 peer packages. This is bounded
adapter coverage, not full native Pi 1.2.0 lifecycle/T3 UI qualification or proof
of the affected process's loaded artifact. No installed files were modified.

The complete PR scope is client TypeScript/JS build-test wiring, synthetic tests
and policy/evidence documentation only. No Rust gateway, image/configuration,
attestation verifier, release workflow, gateway contract or deployed artifact
changes; a gateway rollout is unnecessary. Existing source-versus-installed and
Pi 1.2.0 peer-link qualifications above remain unresolved by this offline packet.
The installed link/bundle must remain unchanged pending separate authorization.

Independent read-only review found and prompted two failure-path corrections:
failed recovery must not downgrade the original peer trust failure, and an
already-rejected trust error propagating after replacement must not poison the
replacement session. The final implementation shares one current-request owner
guard between dispatch and failure handling, converts obsolete recovery exits to
interruption, and independently guards the final trust-latch mutation. Received
accounting extraction is unchanged. Pending-cancellation and rejected-result
propagation are tested separately; a passing pending-cancellation fixture alone
was insufficient evidence for the latter race.
