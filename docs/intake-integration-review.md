# Astra review: intake authentication and durable accounting

**Status: serving source has scoped local evidence; release remains gated.** This is the requested Astra
lead's own source review, not delegated architectural approval. The operator
subsequently clarified: **defer to intake's existing implementation**. Its current
APIs and semantics are authoritative; adapt the gateway, do not make intake changes
or a new endpoint a prerequisite. Remaining incompatibilities require gateway-side
choices, not unilateral intake redesign. Work remains source/disposable-synthetic
only under [gateway privacy policy](../PRIVACY.md) and intake's reviewed policy.
No runtime, secret, account, image, installation, push, merge or deployment changed
from this gateway workstream. The separately authorized intake rollout is recorded
below as an operator/intake-thread handoff, not gateway deployment evidence.

## Renewal consumer: source-only local qualification

The operator authorized steps 1–3: implement owned renewals, qualify against the
paired intake source locally, and rerun tests/clients plus independent review.
These steps are complete within the coverage below. Gateway/client release,
configuration/secret changes, installation, live integration/inference probes and
activation remain separately gated. This section supersedes the earlier
in-progress renewal statements and historical fixed-expiry/cutoff checkpoints.

### Implemented ownership and evidence

- `src/intake/protocol.rs` requires immutable `initial_expires_at`; key admission
  time derives from that original expiry, while current expiry can advance.
  Original account/reservation/key/maximum/timeout/initial-expiry identity cannot
  change, current expiry cannot regress, and a known terminal row remains exact.
  Renewed active rows can outlive the original 24-hour key window. Terminal
  retention and amount/state/null/timestamp validation remain authoritative.
- `POST /reservations/renew` uses the separate accounting bearer and canonical
  captured expiry, no idempotency header. Existing 4 KiB request/16 KiB response
  bounds and five-second attempt apply; no automatic retries or lookup recovery.
  Complete success and known 409 decision rows are validated separately. Server
  prose is discarded. A valid closed renewal row retains its actual operation,
  status and financial outcome; HTTP 200 alone is not confirmation.
- The existing rooted actor in `src/generation_owner.rs` serializes reservation,
  renewal and terminal ownership. Renewal cadence is 600 seconds. Confirmation is
  mapped conservatively from dispatch-time monotonic time, never receipt time:
  renewed expiry minus transaction `observed_at`; initial confirmation uses
  remaining wall-time evidence capped by the original timeout. An overdue poll
  cannot renew after its confirmed horizon. No catch-up burst or overlapping RPC.
- One unconfirmed attempt, active refusal or elapsed confirmation ends all later
  renewals for that operation, not accepted preflight/inference/backpressure.
  Genuine disconnect and independent preflight/trust/transport constraints remain.
  Completion during an in-flight renewal waits for that bounded attempt's evidence
  before one normal terminal operation. An already observed absorbing terminal row
  is compared at normal end instead of issuing another mutation. There is no early
  settlement, work cutoff, replacement hold, post-expiry debit or inference replay.
- Renewal failures retain a closed, content-free diagnostic transiently at the sole
  owner, with a renewal-stage cause-unknown fallback. Authoritative terminal
  evidence or a typed terminal failure takes precedence. Successful final
  settlement is not changed to failure because a renewal failed. Only the existing
  opt-in error projection may present this diagnostic; no new wire fields, logs,
  traces, metrics or automatic diagnostic export were added. The current clients
  accept the new closed renewal labels and render only local messages.

Applicable privacy rules: [typed interface failures](../PRIVACY.md#mandatory-typed-interface-failures),
[data boundaries](../PRIVACY.md#data-handling-boundaries), and
[synthetic testing](../PRIVACY.md#synthetic-testing-exception).

### Executed local evidence

Pinned Rust 1.92.0, Go 1.27.2, Node 24.13.0, TypeScript 5.9.3 and esbuild 0.25.10;
no dependency installation or downloading. The obsolete temporary client dependency
path was unavailable, so checks reused the existing pinned dependencies at
`/private/tmp/possums-pi-build-6OBJvV/source/clients/pi/node_modules`, without linking
anything into source or changing installed clients.

- Full `cargo test --locked --offline --all-targets --all-features`: **499 library
  tests passed, three ignored**, and all integration groups passed. The two ignored
  local HTTPS cases were executed separately by the paired runner; live/funded
  acceptance remained ignored. Clippy with the same feature/target scope and
  `-D warnings` passed. Existing vendored Tinfoil unused `encode` warning remains.
- Nine new owner tests cover serial renewed evidence, permanent stop after unknown,
  completion during an in-flight attempt, delayed-receipt monotonic mapping,
  short/elapsed confirmation, disabled active refusal, all observed terminal
  outcomes, cumulative usage, retained leases and drain. Known/unknown failure ×
  final-success/failure cases verify diagnostic survival and terminal precedence
  without changing successful completion. Synthetic actor panic and local runtime
  loss during renewal cannot fabricate a receipt or clean drain.
- The three continued-work composition tests still pass their 14 scenarios:
  ordinary/eight-frame backpressure, complete/decode-failure natural outcomes,
  genuine disconnect and before-expiry complete/partial accounting. Tokio paused
  time is scheduling evidence, not a claim to advance real wall time.
- `python3 -B tests/intake_review/run.py`: all four Go/PocketBase groups passed
  under `-race`, including the selected deployed-source renewal/clock/auth tests.
  `transport.py` passed both original full-router HTTPS/auth/accounting/binding-race
  acceptance and actual Go/PocketBase/Rust renewal acceptance. Fixtures demonstrate
  duplicate/reordered no-op current rows, no progress, future expectations,
  below-initial validation precedence, disabled holds, settlement after original
  expiry, closed settled/refunded/expired rows and an extension that actually
  committed before the response was lost. No recovery call was made; a single
  natural-end finish accepts a later authoritative expiry despite lost renewal.
- The runner archives **exact deployed private source
  `9a8b6bac3b2407419a8637bb90b0db4f3528b3e2`**, not dirty intake HEAD, into disposable
  `target/intake-review/source`. It verifies every archived file; mismatched scratch
  is refused without deletion. Sandbox denies writes to both intake checkouts,
  original Possums, the source snapshot and module cache. No existing test state
  is initialized, reset, upgraded or deleted. The fixture agent also qualified
  snapshot mismatch/refusal and sandbox-denial cases and overlay Go vet.
- Both client source typechecks and all six offline suites passed:
  `intake_diagnostics`, `phase01_client`, `phase02_client`, `phase01_stream_limits`,
  `tinfoil_forwarding_client`, `phase02_pi`. Hostile/invalid renewal metadata cannot
  authorize billing/completion/tools, leak server prose or initiate work.
- Hash-pinned forwarding `4a2a41884907` and graceful-warning `5e7c451e7259` installed
  artifact error and usage decoders passed by static extraction, without loading
  their provider/network stack. Their login/balance contract fixture also passed:
  older strict login requires wire 43200, and the intentionally removed balance
  route remains a known incompatibility, not a restored accounting authority.
  This is not installed-client end-to-end or package qualification.
- Application `cargo fmt --package possums -- --check` and `git diff --check`
  passed. Workspace-wide formatting additionally reports pre-existing unmodified
  vendor formatting; vendor code was not reformatted. Active LSP checks of nine
  changed Rust/TypeScript paths reported clean, not merely an empty session cache.

Independent renewal review R1 identified one blocker: discarded typed renewal
failures. The parent preserved the closed projection and added the four-case
regression described above. A fresh read-only R2 review found the blocker resolved
and no additional concrete blockers in owner, receipt or paired-fixture scope.
Reviewers did not run Cargo; executed evidence above belongs to the parent.

### Separate intake deployment handoff and remaining limits

The intake thread/operator reports signed v0.1.17 state-preserving v1→v2 migration
exit zero and signed v0.1.18 ordinary serving healthy, with original storage/key
selection/instance/configuration retained. Deployed private source is the exact pin
above; image is
`sha256:70c01cfb228fb9edfe5523d83d8d260689951061e84d15a1a018769eadb0484d`;
public signed v0.1.18 source is `c03c5a36f2c308a653367198f8af4db5df6de4cc`.
Both approved boot allowances are consumed. This gateway workstream made no live
checks or mutations and does not independently attest that rollout.

The operator confirmed saving and reopening native `customer_keys` Auth duration
**43200** in the normal browser, with only that setting changed. This is
operator-confirmed UI readback, not agent API/JWT readback or a live 12-hour token
expiry test. Old tokens retain original expiry; website one-hour absolute/ten-minute
idle limits remain. No further intake mutation is implied by local acceptance.

Unverified limits remain: actual expiry during the post-reserve auth await;
provider-decoder/tool-gating end-to-end in the composed continued-work fixture
(which bypasses the provider decoder and emits no provisional tools); full
package/image/native-Linux/runtime qualification, crash/power-loss recovery and
live paired behavior. Existing provider/client negative tool-gating suites pass
separately; synthetic actor/runtime loss tests are not whole-process crash proof.
The subsequent prepare-only packet updates source rollout references and CI/image
fixtures; see [PR preparation and artifact limits](intake-pr-preparation.md).
Required live intake reference delivery/removal of legacy authority is still a
separately authorized configuration transition, not performed by these checks.
Local Pi package qualification is recorded there; required Linux gateway image
qualification remains outstanding. No push, remote gateway PR, release,
installation, secret update or activation follows from these checks.

## Revised accepted-work policy (2026-10-21; scoped local qualification)

In the intake thread, the operator explicitly rejected cutting off a user's
accepted stream and approved the uninterrupted-work/operator-loss alternative.
This supersedes the former 29-minute accounting cutoff and proposed renewal-failure
stopping margin. New work still requires confirmed active, unexpired reservation
and current authority before prompt-bearing preflight; an uncertain reserve grants
no dispatch authority. After accepted handoff, lease age/expiry or renewal failure
must not cancel preflight, inference or connected backpressure. Existing independent
trust, preflight/transport and genuine disconnect/cancellation rules remain.

Do not terminalize while deliberately continuing work. At normal completion or
genuine cancellation, make one bounded terminal attempt from trustworthy final or
last cumulative usage at the original quote/cap; refund only without trustworthy
usage. Preserve actual terminal or unknown outcome. The operator absorbs usage
that an expired hold cannot collect. There is no replacement hold, inference
replay, compensating debit/refund or new retry/recovery loop.

The approved planning contract is intake branch `feat/gateway-session-renewal`,
`docs/gateway-hold-renewal.md`, starting at the reviewed intake HEAD. It specifies
conditional `POST /reservations/renew` with reservation ID and expected expiry,
immutable `initial_expires_at`, current expiry and transaction `observed_at`.
Use a 30-minute lease/10-minute cadence with one bounded attempt in flight. After
one unconfirmed renewal, permanently stop renewals for that operation while work
continues; also skip renewal once the confirmed horizon has passed. A confirmed
lease horizon is evidence, not a work deadline. No consumer integration is claimed:
intake's implementation/DTO qualification is still underway, and exact candidate
source must be checked before adapting the decoder and owner. Its separate fresh
12-hour authentication change does not retrospectively extend old tokens or
activate an existing installation's settings.

Gateway accounting cutoffs are removed. Initial active/unexpired hold evidence is
rechecked after the post-reserve auth await, immediately before tokenizer dispatch;
there is no minimum remaining work-budget requirement. The parent reran the full
pinned offline all-target/all-feature suite (479 library tests plus integrations),
Clippy `-D warnings`, and all six pinned client suites successfully. Three composed
Rust tests cover 14 continued-work cases, including backpressure beyond the old
29/30-minute horizons, genuine disconnect, authoritative expired/unknown terminal
outcomes and pre-expiry complete/partial receipts. Paused Tokio time is not wall-clock
expiry evidence; the expired receipt separately exercises the actual validator.
A test-consumer deadlock was fixed by releasing frame credits rather than retaining
eight frames in `BodyExt::collect`; production transport bounds were not relaxed.

The real unchanged Go/PocketBase HTTPS fixture now also passes binding mutation at
post-reserve/post-tokenization refresh, plus logout/reset while tokenization is held.
No inference is dispatched; holds are refunded against their original accounts,
leases retire and lifecycle drains. Fixture fixes release retained response bytes
and distinguish native binding 401 from local post-await admission-invalid 400.
No production error mapping was changed to satisfy the tests. Round 4 independent
read-only review found no new blocking findings in this policy/race scope.

Remaining gaps include actual expiry during the post-reserve auth await, terminal
actor panic/runtime shutdown, and full package/image/runtime qualification. The new
composition fixture emits no provisional tools and bypasses the provider decoder;
its tool-absence assertion alone is not client tool-suppression qualification.
Existing client/provider regressions remain separately tested. The earlier section
below records the old checkpoint, not current cutoff policy. Original intake test
state and dirty files remain untouched.

The intake workstream subsequently reported its completed local source candidate,
with unchanged approved failure policy and clarified input-validation precedence.
The parent independently matched SHA-256 for its four wire files against the handoff;
this is not image provenance or runtime acceptance. Candidate decoder/renewal-owner
integration is the next source packet; no renewal dispatch is implemented yet.

## Earlier v0 source checkpoint (not current policy or release acceptance)

The source now contains typed native-auth/ledger receipt decoding, intake-backed
authentication with an original-authority expiry cap, removal of the public
balance route and client balance/reconciliation commands, and the approved v0
work-deadline plumbing. The production entry point requires
`POSSUMS_INTAKE_ORIGIN` and `POSSUMS_INTAKE_LEDGER_KEY`; it no longer uses
`POSSUMS_ACCOUNTS_JSON` for credentials or startup credits. These are source
configuration names, not created secrets, approved deployment bindings or runtime
changes. The existing account-configuration startup code is preserved with an
intake-specific constraint/action; unexpected intake-client construction has a
separate closed cause-unknown fallback in the same finite startup owner. No new
stage, signal, destination or request observation is added. Intake's actual one-hour token still bounds a gateway session despite the paired 12-hour
target; no intake token-duration setting has changed.

Async reservation/terminal ownership and production backend selection are now
implemented. Local balances/completion counters are isolated to the explicitly
synthetic fixture backend, never a production fallback. After the integration
fixes, pinned Rust 1.92.0 `cargo test --locked --offline --all-targets --all-features`
passed (479 library tests plus integrations; operator/funded tests stayed ignored).
Clippy with `-D warnings` passed; the existing vendored Tinfoil dead-code warning
remains. The actual unchanged Go/PocketBase HTTPS fixture passed typed native
login/refresh, original-expiry preservation, gateway Auth login/logout, ledger
receipts/replay/mismatch and credit refusal. Its full gateway-router exercise also
passed: maximum hold 52, complete charge 7, interrupted cumulative charge 4,
no-usage refund, duplicate suppression, body loss/logout/reset after work capture,
retired balance 404 and lifecycle drain. The four Go review groups were rerun and
passed under the read-only confinement.

Independent serving review round 1 found that clients discard server messages,
losing intake-specific diagnostics. The implemented exact-singleton
`x-possums-intake-diagnostics: v1` capability exposes only closed nonauthorizing
`error.intake` metadata. Default envelopes remain unchanged. Round 2 found that
transport constraints still collapsed and cutoff metadata masked observed terminal
accounting evidence. Both were fixed: projection preserves the original typed
transport constraint; terminal evidence precedes deadline fallback without
changing primary `stream_deadline_exceeded`. Round 3 accepted those fixes with no
new concrete findings within its diagnostic scope.

Four composed Rust tests cover six cutoff scenarios: commit uncertainty, validated
expiry and successful partial settlement, each with and without connected
backpressure. They verify 29-minute cancellation, last cumulative usage charged
once (29, not summed snapshots), preserved terminal diagnostics, no complete/tool
release, and failed drain for accounting failures. Backpressure fixtures release
queue credits before the terminal reply; they do not promise delivery to a reader
that stays stalled.

Parent and reviewer independently passed the pinned source typechecks and offline
reference/Pi diagnostic fixtures using `tests/intake_review/clients.mjs` with an
existing dependency directory (Node 24.13.0, TypeScript 5.9.3, esbuild 0.25.10).
The runner creates only scratch test artifacts, installs nothing and suppresses
raw child failures. Every transport constraint and cutoff commit/expiry metadata
traverses decoder/host fixtures with hostile-input, unknown-billing, no-tool and
no-replay checks. The actual Channel seam exercises a binding diagnostic, not each
transport fault. Parent reran both hash-pinned shipped error decoders and the
login/balance obstruction fixture. This is decoder evidence, not full installed-
client E2E: old balance callers still receive the deliberately retired route's
404. No installed client artifact has been changed.

The operator subsequently includes Pi/reference-client changes in this same PR
and confirms there are no existing customers or real customer data, only personal
tests. A customer-data migration is therefore not a cutover prerequisite; this
neither authorizes deleting test state nor replaces client qualification/activation.
The parent extended the source-only runner and independently passed current
`intake_diagnostics`, `phase01_client`, `phase02_client`, `phase01_stream_limits`,
`tinfoil_forwarding_client` and `phase02_pi` suites. An initial Pi runner-layout
failure after 120 groups was corrected to match its existing pinned dependency
probe paths; the complete rerun passed. The normal client scratch build/check
scripts now include the intake diagnostic fixture via the existing test entry.
No client release package was built or installed by these checks.

A separate intake worktree `feat/gateway-session-renewal` started from the reviewed
intake HEAD for the 12-hour token change and renewable holds. Its plan-first review
and subsequent revised approval are recorded above. Implementation approval is
not runtime activation authority; the original dirty intake checkout is preserved.

Remaining qualification gaps include current-binding mutation during reservation/
tokenization, logout/reset during those preflight windows, actor panic/runtime
shutdown, and full client-package/image/runtime acceptance. These are unverified
cases, not demonstrated bugs.

**Configuration/release gate:** `tinfoil-config.yml`, protected deployment secret
references and live configuration remain unchanged. They still describe the old
account input and cannot qualify the new production entry point. Creating or
binding intake credentials, changing measured deployment configuration, intake's
12-hour token migration, image/runtime qualification and any activation require
separate scoped preparation/authority; none follows from these local tests.

The scope preserves [data boundaries](../PRIVACY.md#data-handling-boundaries),
[forbidden exports](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[typed failures](../PRIVACY.md#typed-interface-failures-and-token-aggregate-extension)
and the existing closed startup diagnostics. No request logs, telemetry, support
exports, automatic retries or diagnostic-triggered inference are added.

## Reviewed baseline and limits

- Gateway worktree: `feat/intake-auth-ledger`, exact base
  `9bdd33e642db805c4baa924509d275fcd13ec4b3`; initially clean. The original
  `feat/tor-v0` checkout and stale local main were not used or changed.
- Intake: read-only `cb64b630e86b1a894697c988ecaa203e0bed5224`, including the full
  handoff, AGENTS/README/privacy, **committed** PLAN, customer contract/evidence,
  website/browser/image/storage records. Uncommitted PLAN and unrelated files
  remain untouched. Gateway AGENTS/SPEC/roadmap/full privacy and historical
  `7ee4604:OVERALL_PLAN.md` were reviewed under the current authority.
- `git diff 5fea2c3 HEAD -- internal/httpapi internal/siteauth internal/ledger
  internal/operator/handler.go` is empty in intake. This verifies the handoff's
  source-equality claim, not live behavior. v0.1.16 readiness/authenticated-use
  distinctions and consumed setup/serving permissions remain unchanged.
- Inspected actual PocketBase **v0.40.5** cached source, not just intake wrappers.
  Review tests use pinned existing Go **1.27.2**, offline modules, telemetry off,
  new disposable state and gateway-owned build/temp directories. Initial free
  disk was approximately 12 GiB; no installation or user/cache deletion occurred.

## Facts, corrections and objections

### 1. Native authentication is a candidate, not the private validator

`internal/siteauth/customer_key.go` matches the exact strict base64url contract
and both SHA-256 domains in the handoff. `customer.go` initializes fixed
`customer_keys/customerkeys001`, hidden lookup/verifier/token material, ordinary
`account_id`, nil CRUD/manage rules and a **3600-second** SDK token. Provisioning
uses the SDK pre-form path; neither login nor serving creates a ledger account.

`operator/handler.go` exposes stock native auth routes. Its accounting Bearer key
is not a native superuser/customer credential. `CustomerKeyAdapter.Validate` in
`siteauth/auth.go` additionally checks current schema/MFA/config, SDK token and
record, current AuthRule and exact binding **without minting a token**.

The pinned SDK establishes these differences:

- `apis/record_auth_refresh.go` reissues a refreshable token and calls
  `recordAuthResponse` with an empty auth method. `record_helpers.go::checkMFA`
  explicitly skips MFA for that method. An already-issued token still refreshes
  after enabling MFA, whereas the private customer validator refuses it. This was
  reproduced through the actual native handler over verified loopback TLS.
- Native password login with MFA enabled creates an `_mfas` challenge row and
  returns `401 {mfaId:...}`. The private customer adapter refuses before creating
  challenges. A gateway must not forward the identifier as a diagnostic or
  mistake this response for completed authentication.
- **Review correction:** a supported AuthRule change rotates the collection's
  auth signing secret (`core/collection_model.go::onCollectionSaveExecute`). Old
  tokens receive **401**, not necessarily a rule-specific 403. The first review
  fixture expected 403; corrected after source inspection, then rerun. Password
  updates also invalidate existing tokens. Native refresh returns a changed
  account binding; it does not enforce the gateway's captured-login binding.
- `apis/middlewares.go::loadAuthToken` discards token-lookup errors into an absent
  auth context. `operator/handler.go` presents customer native failures as
  `OPERATOR_NATIVE_REQUEST_UNAVAILABLE`, stage `native_request`, cause unknown;
  distinctions internally classified in `operator/failures.go` are not a customer
  wire contract. A gateway can author a useful stage/status fallback, but cannot
  reconstruct swallowed storage-vs-token causes from that response.

**Gateway adaptation gate:** use the existing native API as authority, not the
website-private validator as an implied remote contract. The differences above
are observed semantics, not a request to change intake. Public `auth-methods`
exposes MFA configuration; a bounded gateway check is possible but is a separate
observation/race and cannot recover swallowed failure categories. Preserve native
MFA behavior, do not claim refresh is equivalent to private validation, and report
any unmet original requirement for an explicit gateway-side scope decision. Derive
the approved lookup/password exactly; do not add a verifier, binding registry,
raw-key bridge or unimplemented route. No intake API amendment is proposed.

### 2. Session lifetime is a real shipped-wire conflict

Gateway `auth.rs` and `api.rs` currently grant/advertise 43200 seconds. Both
hash-pinned shipped bundles tested below, and current
`examples/phase01/client.ts::login`, require **exactly 43200**, not merely a
positive lifetime. Returning 3600 breaks login before discovery/inference.
Retaining renewed tokens forever would violate original-expiry authority.

**Operator selected legacy wire with earlier expiry:** retain the 12-hour outer
bearer maximum while current intake authority expires it early (no refresh
extension; older clients may need deliberate re-login). The operator subsequently selected a paired 12-hour gateway/intake target and
authorized `docs/gateway-session-lifetime.md` in the intake checkout. That note
records the supported customer-token-duration follow-up and acceptance gates;
no intake code or settings were changed here. Do not advertise 12-hour guaranteed
intake authorization today. Internal bound: `min(original SDK expiry, login +
12 hours)`—currently one hour under intake's configuration. No sliding refresh/idle
extension; memory-only token and record/collection/account consistency.
Every protected operation revalidates current authority; logout can always remove
local state without depending on intake availability. Captured in-flight authority
is not an instantaneous global revocation guarantee.

### 3. Accounting handoff is substantially correct; snapshot compatibility is not solved

Read `httpapi/{handler,decode,bind}.go`, `ledger/{types,operations,transaction,storage}.go`
and their contract/race tests. Confirmed: seven shared-key POST routes; 4096-byte
requests; exact u64 decimal-string money; globally scoped reserve keys with exact
account/maximum/timeout equality; 1–1800-second immutable holds; atomic complete
account/pending/available snapshots; disabled-account reads; serialized transaction
app and durable clock watermark; bounded terminal/key retention. Reserve does not
debit total balance. No endpoint renews a hold. Trusted native admins still bypass
Service protections and can invalidate accounting/schema assumptions.

200 terminal replies return the **first state**, including conflicts. Late settle
409 supplies an expired row; missing evidence410 is not nonexecution/refund proof.
The transaction owner preserves an observed commit through later faults. HTTP
loss does not erase it. Lookup/balance perform expiry and are not passive reads.
The 2-second writer/1-second work/250-ms busy budgets do not bound OS commit/fsync.

Corrections to documentation breadth: the HTTP README omits the implemented
`LEDGER_HTTP_READ_UNAVAILABLE` 503 path and says native mutation input remains
strict; unrestricted native administration supersedes that old statement. The
4-KiB bound is **request-only**: `responseBuffer` buffers the complete snapshot
without a configured output cap. A gateway must bound its read and reject overflow
as unavailable, never return a truncated snapshot. Do not modify intake here.

**Operator decision: remove gateway balance functionality.** The former
`/v1/balance` wire requires `available_microunits`, `in_flight` and
`completed_requests`; intake has no matching completion counter. Rather than add
one or introduce an opt-in gateway balance view, remove gateway-owned balances,
completion counters, user-facing balance API and balance-difference reconciliation.
The gateway checks and updates the authoritative intake ledger for admission,
ongoing holds and terminal settlement/refund. Do not maintain a shadow spendable
balance, reconstruct financial state, or implement a second ledger.

Trust intake's valid account-scoped balances, pending state and committed debit
receipts. Keep transport/schema/account/reservation binding checks and distinguish
terminal outcomes; trusting the ledger does not make an unknown HTTP result a
confirmed debit. Gateway still calculates submitted-quote charges from validated
usage; intake owns applying them atomically. A snapshot check is not a reservation:
confirm the actual reserve before sending prompt content, avoiding a read/spend
race. Shipped balance/reconciliation functionality is deliberately retired by this
selection; qualify useful old-client failure and preserve inference/receipt paths.
No replacement balance endpoint or intake schema change is requested.

### 4. Ownership must change, not just the accounting method body

`auth.rs::admit_submission` holds sessions → tokens → accounting locks and reserves
synchronously. `generation.rs` immediately constructs the sole reservation owner
and transfers it to detached preflight **before tokenizer prompt transmission**.
`generation_owner.rs::Terminal::Drop` currently performs synchronous terminal work.
`api_stream.rs` considers settled money plus completed inference before emitting a
complete receipt; `stream_owner.rs` waits for bounded delivery capacity and wakes
on disconnect. `server.rs`/`lifecycle.rs` retain tickets until owners AND connections
drain. These are current-main behaviors, not the old drain-upstream proposal.

Remote I/O inside a mutex or async work launched from Drop without tracked ownership
is unacceptable. Proposed owner begins **before reserve dispatch**: claim a token
attempt under short locks, retain its exact independent random v1 key/payload and
lifecycle ticket, release locks, await auth/reserve, then recheck session/conversation/
model/token validity before authorizing prompt work. A competing duplicate can
observe only that attempt, never launch another generation. If invalidated before
handoff, an observed active late reserve gets one owned refund attempt; uncertain
reserve gets no inference or invented refund. After accepted handoff, logout/reset
must not transfer ownership or suppress settlement of captured usage. Every panic,
pre-poll cancellation and receipt-loss path must join the same terminal owner;
retain known receipt through later delivery faults. No generic background queue.

**Open hold/drain gate:** the operator asks whether holds can be renewed or
whether gateway-selected duration plus intake GC is preferable. Current intake
already lets the gateway choose 1–1800 seconds at reserve time, but makes that
expiry immutable and exposes no renewal operation. Repeating reserve with the
same key does not extend it; changing timeout conflicts. A new key creates a
second hold and is not renewal.

Recommended future design, not implemented/approved here: the gateway decides
whether work still needs a hold; intake owns an expiring renewable lease and its
atomic monetary effects. Renewal extends only an active, unexpired reservation,
never revives a terminal row, and serializes with settlement/refund/expiry. Use
one reservation and unchanged maximum/quote, with bounded extension and no new
owner registry/fencing framework. Intake's existing expiry/cleanup releases
orphaned holds after gateway loss. A renewal failure/unknown response requires
an explicit policy for stopping work before last-confirmed expiry versus accepting
possible unbillable work; do not silently add cancellation or retry behavior.
This needs a separately agreed intake API/transaction amendment, not a gateway
workaround pretending renewal exists. The operator subsequently requires an attempt
to bill trustworthy usage on renewal loss and authorized the second intake note
`docs/gateway-hold-renewal.md`. It records the proposed lease semantics and a v0
policy: an existing 30-minute hold with a temporary 29-minute work deadline to
leave a settlement margin. The operator subsequently **approved this v0 cutoff**;
implementation and qualification remain separate. Late/uncertain accounting can
still leave usage unbillable under existing intake semantics.

Current main deliberately has no whole-generation, connected-backpressure or
application drain deadline. Renewable holds alone do not make shutdown finite.
A hard shutdown timer would change accepted-work semantics and Tor connection
draining; do not add one without a separate explicit policy decision.

## Smallest proposed implementation (after decisions)

1. Resolve gateway native-auth adaptation and hold/drain choices against intake's
   existing contracts. Implement the paired 12-hour session target without
   extending observed intake authority, and remove gateway balance functionality.
   Intake remains read-only except the expressly requested session-lifetime and
   hold-renewal notes; no intake bridge, schema or runtime change.
2. Add one narrow intake HTTP client using existing reqwest/serde/SHA-256/base64
   dependencies; reuse bounded encoding and quote arithmetic. No Rust PocketBase
   verifier, provider parser, superuser token, generic retries or new telemetry.
   Proposed runtime inputs: `POSSUMS_INTAKE_ORIGIN` and `LEDGER_ADMIN_KEY`; require
   both, exact HTTPS origin, normal certificate/hostname verification, no URL auth,
   query/fragment, ambient proxies, redirects, compression or automatic retries.
   Any loopback HTTP seam must be test-only. No hardcoded live origin or startup probe.
3. Replace production demo auth/credit construction in `main.rs`/`AppState::new`;
   retain local fixtures only as explicitly synthetic test constructors. No local
   fallback, startup funding, provision/credit route or public signup/top-up.
   Use asynchronous session/admission and the single owned reserve/terminal path
   above, with no std mutex across I/O. Existing verified catalog/inference/Tor,
   release, freshness, backpressure and completion/tool checks stay intact.
4. Validate every receipt's account, reservation/key, maximum/timeout, canonical
   amounts, state/null/time invariants. Compute charge using existing `cached_cost`
   at the submitted quote, cap to hold, never sum cumulative usage. Treat unknown,
   expired and conflicting terminals as distinct internal evidence. Do not turn an
   expired hold or mismatched charge into the requested successful receipt. Preserve
   old SSE/default schemas only where their semantics are established; otherwise
   closed unavailable/incomplete, no tools. No raw upstream diagnostics.
5. Qualify actual Go/PocketBase + Rust transport/API with two gateway callers,
   current/prior decoders, race/fault/privacy fixtures. Packet-sized local commits
   only after passing checks; no publication/activation implied.

### Proposed budgets and unresolved policy

- Connect **2 s**, whole auth/accounting attempt **5 s**, including response read.
  Response caps: auth **64 KiB**, terminal/error **16 KiB**, complete balance
  **1 MiB**; encoded accounting request **4096 bytes**. Overflow is unavailable.
- Hold **1800 s**, fixed at initial reserve; no renewal/fencing. This maximizes
  the existing allowed interval but still cannot cover arbitrarily slow work.
  Expiry may make later usage unbillable; preserve the accepted operator-loss rule,
  never stop/shorten inference merely to improve collection without authorization.
- **No automatic retries**, including same-key reserve retries or implicit HTTP
  retry policy. An unknown reserve without its ID remains unknown until expiry;
  never mint a replacement key or send prompt. With a known ID, propose at most
  one same-owner lookup after an uncertain terminal attempt (another **5 s**), not
  a second mutation. That recovery policy is a proposal, not silently authorized.
  Simpler default is zero lookup recovery and explicit unknown outcome.
- Added accounting cleanup can be bounded to one **5 s** terminal RPC (or **10 s**
  only if the lookup proposal is approved). **Whole-process drain remains unbounded
  under current policy.** Resolve whether to retain this or separately design a
  finite shutdown/cancellation budget; do not call a timed-out owner drained.
- No durable gateway recovery queue. Process loss discards sessions/ownership;
  intake expiry remains authoritative, not proof that this process observed a
  refund. Avoid simultaneous ledger/gateway replacements. No 120-second retry
  loop from historical intake planning is authorized.

## Executed local review evidence and remaining acceptance

`python3 tests/intake_review/run.py`: **four groups passed**, 15 selected top-level
Go tests under `-race -tags elpscheck -mod=readonly -p 1`, with `GOPROXY=off`,
`GOSUMDB=off`, `GOTOOLCHAIN=local`, telemetry off. Go overlays add gateway-owned
review/transport tests without changing the intake package; Go compiler cache,
temp and state are under this worktree's ignored `target/intake-review`.
No real credentials are read.

- New native TLS test: actual login/current binding, reassignment, AuthRule
  invalidation, MFA-enabled refresh vs private denial, renewed expiry, native MFA
  challenge persistence, password revocation, empty logs/auth-origin tables.
- Existing private authority test plus ledger money/full-u64, duplicate/conflict,
  last-credit concurrency, terminal/expiry, complete/scope/atomic snapshot,
  prune/clock/restart tests passed. HTTP schema/timeout/header/scoped balance,
  expired receipt and modeled post-commit delivery-loss cases passed.
- `node tests/intake_review/shipped_contract.mjs` passed on Node **24.13.0**.
  It executes unchanged login/balance methods and scalar decoder helpers extracted
  from the exact public bundle bytes, never imports/activates the provider:
  `pi-v0.0.30-forwarding` SHA256
  `4a2a4188490785efaac408dd7d67e9735464ff81b67cdc9b059a8f15551fab7c`;
  `pi-graceful-warnings` SHA256
  `5e7c451e7259b3778e9d35b2fafda59dd35ea9596f9dfb8a3a4404c6f04e8db3`.
  Both accept legacy vectors and reject 3600-second login/missing-null counts.
  Transport/deadline/error carriers are synthetic. This is obstruction evidence,
  **not full old-client/candidate integration acceptance**.

### First implementation packet: transport only

`src/intake/` now implements the exact native key derivation and fixed native-auth /
accounting request routes, without provision/credit/admin APIs. It uses existing
reqwest/rustls/serde dependencies, verified HTTPS, explicit no-proxy/no-redirect/
no-retry settings and the proposed transport budgets above. It separates native
credentials from the accounting shared key, sends no raw user key, and preserves
non-success response bodies (including a potential expired receipt) without
asserting their financial meaning. Only closed transport failures and observed
status survive error handling; no private request/response/client Debug output.
This private module is **not wired into serving** and has no financial receipt or
session-authority validator yet. Transport success is not an authorization verdict.

- Eight Rust transport tests pass: strict derivation, configuration/input refusal,
  fixed route/body/header separation, certificate/hostname rejection, declared and
  streamed limits, compressed-response refusal, JSON failure, body deadline and
  observed status, no redirects/retries on the tested status cases.
- `python3 -B tests/intake_review/transport.py` passes: pinned Rust test binary
  talks through actual loopback TLS to the existing Go/PocketBase native/accounting
  HTTP handlers under `-race`, with disposable state. Covers native methods/login/
  refresh, cross-use of native/accounting credentials rejected, reserve duplicate/
  conflict, complete balance, lookup, settlement, refund, first terminal winning
  a conflicting request, and competing last-credit reservations. Provisioning and
  synthetic funding occur only in the Go test fixture. No provider is called.
- The fixture uses a generated non-CA server leaf; the stock Go httptest CA-as-leaf
  certificate was rejected by Rustls. No certificate verification bypass was added.
  Rust child output is not forwarded: failures expose only a source line and a
  closed transport constraint. No process output is persisted as a diagnostic.
- Independent source review caught a fixture isolation gap: `-mod=readonly` alone
  does not stop writes into intake's module cache. The runners now wrap Go and
  Cargo in macOS `sandbox-exec` denying filesystem writes beneath both protected
  sibling trees; children inherit the restriction, with no unsandboxed fallback.
  A non-creating/non-truncating write-open check was denied. All four review groups
  and the cross-language fixture passed under that restriction. Follow-up review
  found no new concrete defects; it did not independently rerun tests. This is a
  macOS-local harness, not portable CI qualification. Earlier runs did not enforce
  this filesystem restriction; no module-cache immutability claim is made for them.
- Pinned Rust **1.92.0**: formatting, `cargo clippy --locked --offline --all-targets
  --all-features -- -D warnings`, and `cargo test --locked --offline --all-targets
  --all-features` passed. The explicit Go-owned transport test is ignored by ordinary
  Cargo runs and executed by the wrapper above; funded and operator-only tests
  remain ignored. Active LSP checks were clean for changed Rust/Python paths;
  compiled Go acceptance, not the host's advisory LSP availability, is the Go evidence.

Privacy references: [data handling](../PRIVACY.md#data-handling-boundaries),
[forbidden collection](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[typed failures](../PRIVACY.md#mandatory-typed-interface-failures), plus intake's
accounting/customer-auth and forbidden-collection sections. No new logs, traces,
metrics family or automatic support artifact is added. Native host/tool history is
an independent persistence boundary; this is not a whole-process transience claim.

Serving integration, typed terminal/balance validation, current/old full
stream/error/receipt E2E against the remote adapter, concurrent gateway admission,
late-reserve/cancellation/drain/restart and generated-image qualification remain
**not implemented/qualified**. No real-data readiness, physical durability, intake
enclave identity or live result is established. Gateway compatibility/policy
choices and separate runtime authority remain gates.
