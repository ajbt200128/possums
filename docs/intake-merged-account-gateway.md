# Gateway adaptation to merged intake accounts

## Scope and source authority

This is a gateway source/offline qualification packet, not release or deployment
acceptance. It starts from fresh `origin/main`
`f000e81adae1c4a9a895135e0b8659fbafcf47f0` (including client-only PR #111).
That PR did not fix intake or publish a gateway release.

The reported connection failure was `INTAKE_PROTOCOL_UNCONFIRMED`, operation
`intake_auth_methods`, stage `remote`, constraint `remote_cause_unknown`, observed
intake HTTP 404. It is distinct from the earlier CVM manifest rejection. That
connection attempt sent no inference; previous billing remains unknown. No live
request was made here to reproduce it or test the operator's credential.

Read-only intake worktree/branch metadata identified committed main
`1946aa43f21f3ccd6ceaee7819aa53a5f8221f6f` in `t3-a8787a0b`, newer than
`b96debd` and `d5b9c16`; the original checkout's
`cb64b63` main is stale. Qualification archives the exact committed revision, not
sibling dirty files. Intake's `docs/merged-account-schema.md`,
`docs/merged-account-deployment.md`, `internal/ledger/storage.go` and
`internal/siteauth/{customer_key,auth}.go` define the current contract:

- One native PocketBase auth collection, `accounts`, ID `ledgeraccounts2`, fresh
  schema v3. The authenticated SDK `record.id` is the sole ledger account identity.
- No native `record.account_id` or separate credential-to-ledger binding exists.
  Accounting wire fields named `account_id` carry that same SDK record ID.
- The raw customer key's domain-separated lookup/password derivation is unchanged.
  This does not prove that any particular credential has a current record.
- PocketBase remains pinned to v0.40.5. The historical
  `gateway-integration-handoff.md` describes the retired split schema and is not
  current schema authority. Earlier gateway qualification is recorded in
  [the historical integration review](intake-integration-review.md).

## Minimal behavior change

Login, refresh and auth-method discovery use `/api/collections/accounts/…`.
The adapter requires the exact collection name/ID and token-to-record binding,
then uses only `record.id` for accounting. It removes the now-redundant second
principal identifier, not a security check. No old route/schema fallback,
production alias, provisioning or local-account authority is introduced.

Supported native HTTPS authentication remains authoritative; JWT inspection is
metadata validation, not local signature verification. Every protected operation
still checks current native authority. MFA/password-method checks fail closed;
refresh never adopts a new token or extends captured original expiry. The gateway
session remains bounded by original intake expiry and its existing twelve-hour
maximum, with the shipped `expires_in: 43200` outer wire scalar unchanged.

Reservation, renewal, accepted-work continuation, terminal settlement/refund,
receipt/tool gates, pre-prompt revalidation, transport bounds and no-replay policy
are unchanged. No new diagnostics, metrics, logs/traces, persistence, dependencies,
retry or availability policy are added.

## Qualification and compatibility

Parent-executed local checks:

- Existing Nix/devenv Rust **1.92.0**: all-target/all-feature tests pass (**500
  library**, four ignored, plus binary/integration groups); Clippy `-D warnings`,
  application formatting and whitespace pass. The two ignored paired intake
  tests were separately executed below; operator/public Tor and funded tests
  remain unexecuted. Existing dependency dead-code warnings remain unchanged.
- `tests/intake_review/run.py`: all **34 explicitly named Go tests** pass under
  `-race`, using Go **1.27.2**, PocketBase **0.40.5**, exact archived intake source
  above, offline modules and read-only sibling/module-cache sandbox. Includes
  native account creation/confirmation, unchanged key derivation, same-ID account
  ownership, AuthRule/password revocation, native MFA/refresh distinctions,
  original/token-duration behavior, ledger and HTTP renewal/terminal/privacy
  cases. The delegated fixture author also passed overlay vet and read-only
  denial probes; those are separately reported checks, not parent execution.
- `tests/intake_review/transport.py`: both actual **Go/PocketBase/Rust HTTPS**
  cases pass. The real native response has `id`, `collectionName`, `collectionId`,
  balance/enabled and **no `account_id`**. Login/refresh/session/reserve use that
  same generated SDK ID. Covers credential separation, duplicate/conflicting
  reservations, competing last-credit holds, settlement/refund/renewal/lost
  committed-response behavior, and full gateway-router receipt paths. Actual
  password revocation at post-reserve and post-tokenization refresh prevents
  inference, refunds the captured same-account hold once and drains. Logout/reset
  preflight races remain covered. These replace obsolete second-binding mutation
  seams; unit tests still reject a different token-bound record/collection.
- Six current client suites and both source typechecks pass using **Node
  24.13.0 / TypeScript 5.9.3 / esbuild 0.25.10 / Pi 1.3.0** scratch dependencies:
  intake diagnostics, reference client, current client, stream limits, forwarding
  and Pi host. These exercise hostile diagnostics, terminal/tool gates and no
  replay; they do not activate an installed provider.
- Exact statically extracted shipped error/usage decoders pass for forwarding
  `4a2a41884907`, graceful warnings `5e7c451e7259` and current CVM-compatible
  `d16b1eaf3af5` extension bytes. Their full SHA-256 values match the existing
  [integration](intake-integration-review.md) and
  [CVM installation](pi-cvm-0.14.16-compatibility.md#subsequent-operator-authorized-local-installation)
  records. Unknown billing, partial/interrupted receipts and hostile remote text
  remain nonauthorizing. The existing hash-pinned older login/balance obstruction
  fixture also passes: legacy login requires 43200; retired balances are not
  compatible. Public bundle code was read without provider/network initialization;
  no installed bytes, links or reports were changed.
- Full Python suite: **150 tests, three optional graph skips**, using existing
  Python **3.13.13**. Attestation-helper Go tests pass. Active LSP checked all
  eight changed Rust/Python paths without findings. Go compilation/race tests,
  rather than host LSP availability, provide the Go fixture evidence.

All tests use disposable synthetic state, offline dependencies and telemetry off;
no production credentials, configuration, customer records, history or private
runtime caches are read. Source archives and build caches are not deployment
provenance. The fresh Darwin flake-shell attempt did **not** qualify: a pinned
Darwin build dependency was unavailable (observed public fetch HTTP 404).
No toolchain pin or check was relaxed; existing Nix/devenv executables supplied
local checks instead. Required exact-head Linux flake/image CI remains a separate
PR gate, not substituted by the host tests. Fresh independent review and exact
CI results belong in the PR record, not inferred from this local evidence.

The gateway-to-client default wire contract is unchanged. The existing retired
balance route remains a deliberate 404; older balance commands are not restored.
Decoder qualification is not full installed-client end-to-end acceptance. Positive
live authentication/catalog/inference, actual expiry during the post-reserve auth
await, physical crash/durability and native host UI remain separately unqualified.
The known same-signal/native-origin renewal ambiguity is not addressed.

Privacy references: [data boundaries](../PRIVACY.md#data-handling-boundaries),
[typed failures](../PRIVACY.md#mandatory-typed-interface-failures),
[forbidden exports](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
and [synthetic isolation](../PRIVACY.md#synthetic-testing-exception).
Safe diagnostics retain no-replay and unknown-billing semantics. Pi/T3 native
history and platform retention remain independent boundaries; no whole-process
transience or upstream erasure is claimed.

## Protected rollout plan — fresh confirmation required

1. Review the exact candidate and require successful exact-head Linux CI:
   `flake`, `image`, `image-startup`, including real Linux/amd64 archives, NSS/SDK
   rootfs, packaged health and Tor-image qualification. Do not apply
   `no-rollout-needed` to this runtime gateway fix.
2. Obtain fresh explicit operator confirmation for this exact reviewed candidate
   **before merge or release publication**. Opening a PR is not that approval.
   Preserve the normal exact-main checks, independent A/B image agreement,
   signed public release provenance and release-only source relationship in
   [the release flow](approved-main-releases.md).
3. The operator reports public intake origin `https://possums.dev`; this belongs
   to gateway `POSSUMS_INTAKE_ORIGIN`, not Pi. Neither the actual running origin nor
   its secret-value delivery has been inspected/established. Source changes do
   not update runtime configuration. Do not read or rotate the user's credential.
4. Before admitting any production update, privately reconcile current reference
   selections and correct origin/key delivery through approved tools/custody.
   Ordinary updates preserve **every** selection/settings value, especially Tor
   A/B identities. If first cutover is still needed, use only the reviewed exact
   [protected intake transition](intake-protected-cutover.md), with new signed
   image, private provisioning, captured-ID refresh, fresh per-run exclusive
   custody assertion and protected approval. It refuses an already-cut-over state.
   If the origin needs changing outside that exact transition, stop and obtain a
   separately reviewed exact protected transition and operator approval/private
   provisioning; do not repurpose first-cutover mode, overwrite opaque values or
   update configuration on an old image. No origin transition is presumed needed.
5. Require exact source/release/provenance/current-main/no-drift admission and
   protected production approval. Keep all other configuration writers idle where
   exclusive custody is required through outcome reconciliation; an assertion or
   snapshot is not a server lock/CAS or proof of secret-value delivery.
6. Observe control-plane completion, signed provenance and serving appraisal as
   distinct evidence. Unknown outcomes stop for private passive reconciliation,
   not replay, rollback or a fresh automatic run. No intake disk/customer mutation,
   release dispatch, runtime update, client installation/activation, T3 restart or
   paid inference is authorized by this source packet.
