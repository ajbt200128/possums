# Pi v3 public-manifest cache restoration and warning behavior

## Scope and authority

The operator requested restoration of caching lost in the v3 migration, a check
for other caching/nearby regressions, warning-only presentation of GitHub/AMD
availability failures, and removal of routine received-charge notifications.
This is client source/scratch qualification, not installation, activation,
gateway deployment, a live availability check or paid inference authorization.

Applicable policy: [public-evidence data handling](../PRIVACY.md#data-handling-boundaries),
[typed failures](../PRIVACY.md#mandatory-typed-interface-failures),
[forbidden exports](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[synthetic testing](../PRIVACY.md#synthetic-testing-exception) and the
[updated client failure policy](pi-client-failure-policy.md).
No logs, traces, telemetry, diagnostic uploads or raw-error persistence are added.
Native Pi/T3 may retain ordinary messages/safe diagnostics under their own settings.

## Confirmed regression and restoration

The [original v3 migration](pi-v3-client-verification.md) deliberately left out
legacy disk-cache reuse. Its test even required the cache to be absent from the
bundle. That was documented, but regressed cross-session public-acquisition
availability: every new trust setup called GitHub's rate-limited
`/releases/latest` API before acquiring the manifest.

The new bootstrap:

1. Obtains fresh evidence using a new random 32-byte nonce, through the already
   selected HTTPS or owned Tor route.
2. Uses the existing supported offline `verifyV3` core with the fixed repository
   to authenticate the **serving** release tag/digest. These facts do not create
   a channel or authorize credentials/inference.
3. Selects the exact public manifest from a separate per-user disk entry or
   downloads the specific tagged release asset through the existing bounded
   GitHub-to-asset-CDN redirect policy. There is no GitHub API discovery request.
4. Passes the same fresh evidence and exact manifest through the existing full
   `ReferenceClient.v3`/`Channel.v3` verifier, including publisher, signed
   predicate/subject, config/measurement, freshness, certificate, HPKE and actual
   endpoint transport bindings. Tor additionally verifies its endorsement.
5. Publishes the cache entry atomically only after complete channel qualification.

This intentionally performs two offline appraisals per setup, not a new verifier
or a cached verdict. Ordinary valid resident requests retain their existing
in-memory connection/auth/catalog reuse; no per-turn public acquisition is added.
The first appraisal selects the serving release rather than asserting GitHub
newestness. There is no new anti-rollback guarantee or approval-pin change.

### Cache boundary

- Location: `$XDG_CACHE_HOME/possums/pi-public-manifest-v3` when XDG is absolute,
  otherwise `~/.cache/possums/pi-public-manifest-v3`. Scope is one local OS user's
  cache directory across Pi sessions/processes, not all devices or all users.
- One replaceable strict-schema record: version, fixed origin/repository, tag,
  SHA-256 digest and public manifest bytes only. No reports, nonces, nonce/hash
  hints, v3 collateral, credentials, accounts, model/chat data or trust verdicts.
- The independent legacy `pi-public-evidence-v1` directory/schema is neither
  migrated nor overwritten. Its existing manifest/provenance/AMD reuse and
  per-process verification are unchanged; v3 has no separate AMD acquisition.
- Tag/digest/origin/repository mismatch, malformed/empty/oversized records,
  unknown fields and symlink entries are misses. Every hit still verifies.
  Verification rejection never falls back to an older entry or another release.
- Existing bounded owner-lock/reclamation mechanics coordinate cold processes;
  all processes independently qualify their own fresh evidence and connection.
  Acquired bytes and verifier input are separate snapshots, so mutation cannot
  change the public bytes published under an already checked digest.
- No evidence TTL, expired-trust extension, retry/replay, background refresh,
  GitHub credential or inference authority is introduced. Cache storage failures
  remain closed and content-free. A cold/changed release still needs its asset;
  a warm setup needs zero GitHub or separate AMD downloads.

## Warning behavior and native accounting

Classified external public-acquisition transport/timeout/HTTP/rate-limit failures
at release discovery, manifest redirect/download, release provenance and AMD
certificate stages now produce a warning-only native host stop. Direct native
API-key resolution can return only the existing nonsecret request marker so the
adapter can present that warning; `perform()` stops at the recorded failure
before request preparation, credential delivery or inference. The marker is
never gateway authority. Canceled/stale-account resolution cannot return it or
mutate replacement state.

This does **not** allow inference to proceed without verified evidence. Schema,
binding, appraisal/integrity failures and other unavailable gateway trust remain
fatal/closed. Failed startup verification remains cached within its trust session;
status/model listing do not initiate recovery, and no automatic inference retry
or new setup-recovery budget is added. Wait/check connectivity and deliberately
start a new trust session when needed. A host `done/stop` is not backend completion,
refund, settlement or tool authorization.

Ordinary authenticated charges remain in Pi's native `usage.cost.total` and exact
receipt/charge diagnostics. The extra `gateway-reported charge ... component zeros
...` UI notification is removed. No charge is recomputed or split into invented
components; actionable turn warnings and explicitly unknown-billing warnings
remain. Notification failures still initiate no work and never log raw data.

## Regression review scope

An independent read-only review of provider/index/thinking, migration history and
related tests found no additional confirmed regression in resident trust reuse,
authentication/catalog reuse, renewal/setup coalescing and budgets, lifecycle
invalidation, presentation-only model discovery, session-prefix-cache ownership
or inference replay protection. This is a bounded client/cache review, not an
exhaustive repository audit.

The existing Rust auth tests establish prefix-scope reset/login/account ownership;
Pi fixtures establish client reset/logout/recovery behavior. A combined actual
Pi `/new`/logout-to-gateway-prefix-scope integration test remains absent. This is
a coverage gap, not evidence that the scope changed. No server source is changed.

## Offline evidence

All data used here is synthetic; crypto-positive hardware/public evidence is not
manufactured as a fixture. Qualification uses Node **24.13.0**, Go **1.27.2**,
Rust **1.92.0**, TypeScript **5.9.3**, esbuild **0.25.10**, the locked client
libraries and matching scratch Pi **1.0.4** peers. No host package was installed
or activated. Pi peers and matching chord/codemode/MCP/TUI/telemetry packages were
recreated only in scratch: allowing their published caret ranges initially mixed
versions, so all were pinned to exact 1.0.4 before acceptance.

The normal Nix/devenv shell reconstruction encountered an unavailable upstream
build input (HTTP 404) before compilation. Matching already-present Nix-store
Node/Go/Rust binaries were used without changing repository pins; the native
helper was built offline from its unchanged lockfile and source and packaged
with matching source/artifact hashes. The scratch build verifies pinned tool
versions, runs strict TypeScript checking, verifies verifier/helper assets and
emits the extension/report. Source-checkout LSP lacks intentionally uninstalled
Node/Pi types; scratch LSP was unavailable. Those editor observations are not
clean-typecheck evidence: the pinned compiler is the validation boundary.

Focused suites passed:

- `phase02_manifest_cache.mjs`: **72** cases, including six synchronized child
  processes, one cold download/every-process verification, warm zero downloads,
  selector/schema/size/privacy negatives, symlinks, typed-primary preservation,
  cancellation, lock crash/reclamation/supersession, publication/storage failure
  and verifier/caller byte mutation.
- `phase02_v3.mjs`: **53** synthetic appraisal/bootstrap/cache and real loopback
  TLS checks. GitHub API responses are rate-limited in the fixture yet never
  requested; warm acquisition succeeds when asset downloads would return 429,
  and cold asset 429 remains classified with no channel/fallback. Every setup
  owns a distinct nonce. Wrong endpoint keys withhold HTTP/private bytes.
- `phase02_discovery.mjs`: **62** actual native SDK/RPC checks, including external
  acquisition stage/constraint coverage, repeated warning stops with one cached
  setup failure and no fatal trust latch, real RPC GitHub/AMD/asset-limit warnings,
  no native retry/inference, and preserved fatal schema/binding/trust failures.
- `phase02_pi.mjs`: **167** native SDK/provider cases, including routine-charge
  silence with unchanged native usage, transient UI failure, actual renewal,
  coalescing, Stop/account/session replacement, tools, partial money, compaction
  and no inference replay.
- `phase02_v3_package.mjs`: **18** checks against the actual emitted package and
  real packaged Worker/WASM on Pi **1.0.4**, and separately installed runtimes
  **1.2.0** and **1.3.0**. Invalid fresh evidence rejects after one public fixture
  request, before cache/GitHub/credentials/inference; status remains offline and
  packaged hashes match. Later-runtime checks are rejection-path compatibility,
  not full positive/authenticated runtime or T3 rendering qualification.
- Unchanged Go helper/core tests passed with `go test -mod=readonly ./...`.

The complete printed `check.mjs` command passed from the final scratch build,
including the unchanged reference/decoder/streaming/forwarding, **179** legacy
release cases, **40** legacy-cache cases, thinking, Tor setup/selection/failure,
**84** Tor channel cases, **18** peer-recovery cases and **70** shared admission
cases, alongside the focused suites above. The separate bridge/WASM suite passed
**40** offline cases. These counts describe synthetic groups/cases, not live
requests or a claim that every client behavior is covered.

Final scratch package: `/tmp/possums-pi-build-dUXgpb/package`. Extension SHA-256:
`d6832697096392b60887c2c1c530fa0af05d0c0d3c471acd2fc8efa59777ef9c`.
This is generated-artifact identity evidence, not signed release provenance or
installation evidence. The packaged-host fixture reports the actual selected
runtime version rather than a hardcoded build-version label.

Independent patch review round one found that the manifest redirect acquisition
mapped a GitHub 503 to `redirect`, incorrectly keeping that operational failure
fatal. The fix preserves observed rate limits as `rate_limited`, HTTP errors as
`http`, and malformed/unexpected redirects as `redirect`; actual bootstrap
429/503/200/307 regressions and native RPC 503 warning/no-login/no-catalog/no-
inference assertions passed. Review round two found no remaining code blocker;
its only documentation objection (stale focused-suite counts) is corrected here.
`git diff --check` passed. No claim of positive live verification follows from
those reviews or offline fixture successes.

## Remaining limits

No new live evidence, rate-limit reset timing, cache warm-up on the installed
client, gateway availability, paid inference, billing reconciliation or T3 visual
rendering was tested. Source/scratch behavior does not update an installed or
already-running client. Positive production v3 evidence and publisher/AMD crypto
acceptance remain at the scope of their earlier verification records; no earlier
live unknown is retrospectively resolved. Required trust, release/provenance,
protected-approval and separately authorized installation/activation gates remain.
