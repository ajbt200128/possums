# Independent offline Tor preparation and ingress integration

> Historical evidence only in this inactive HTTPS-only backup. Tor code/assets
> are removed; commands and authorizations below do not apply to this branch.
> Results and unresolved failures retain their original scope. See
> [backup verification](without-tor-backup-verification.md).

Source-only independent tester, based on `dbc623391940131229b90c32df6cc6eb4e8af679`.
No rollout, provisioning, approval, inference, public network probe, service
bootstrap/publication or production mutation is authorized by this record.

## Privacy boundary

Applicable policy: [data handling](../PRIVACY.md#data-handling-boundaries),
[forbidden diagnostics/telemetry](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[typed failures](../PRIVACY.md#mandatory-typed-interface-failures), and
[synthetic testing](../PRIVACY.md#synthetic-testing-exception).

The separately authorized selected production **B was used**, from its original
native private-key and hostname paths, inside a local Rust child only. Neither
file was read by an LLM/file-view tool. B was not copied to a fixture, repository,
output file, argument/environment value or support artifact; no B-derived
identifier, size, fingerprint/hash, signature or endorsement was emitted. Private
file contents use bounded reads into `Zeroizing` owners; this does not establish
whole-process memory erasure or upstream library zeroization.

A was the public RFC8032 PKCS8/SPKI fixture, **not a mounted production grant or
attested A**. No real A was acquired. The B check never calls bootstrap,
`launch_onion_service`, signing or an inference API. It is not publication
exclusivity evidence, and failed pending rollout metadata establishes no such
exclusivity either.

`scripts/tor-offline-e2e.py` verifies an exact local test-executable SHA256 before
running it. This is an **artifact pin**, not a key fingerprint. Before private
access, the same wrapper/executable must pass:

- Actual Darwin sandbox TCP/UDP bind and TCP connect denial, requiring the typed
  `PermissionDenied` result, not merely a failed connection.
- Actual denied filesystem write outside ephemeral state, plus permitted
  creation/use/removal of an owner-only synthetic runtime directory.
- A hostile synthetic panic: hook output suppressed, caught payload not formatted,
  exact closed fallback only.
- Whole-output projection rejecting extra/unknown text, nonzero exit and any
  stderr; bounded in-memory output and a child deadline with kill/reap cleanup.
- Public synthetic full preparation and real SDK client/key-store construction.

No operator environment is inherited by the child. Core dumps are disabled.
The final runner sets `HOME` to the empty string. The initial baseline private
run used a newly created empty 0700 HOME directory instead. Production preparation
uses its unchanged hardcoded `/tmp` path: the sandbox permits writes only beneath
its `possums-tor-<alphanumeric>` runtime prefix (canonical Darwin `/private/tmp`),
not arbitrary `/tmp`, HOME or key-directory writes. Exclusive `tempfile` creation
owns the actual scratch; the helper checks its 0700 mode and removal. The sandbox
prefix allowlist is not protection against a malicious helper targeting another
matching same-user runtime directory. No key is written to that runtime tree;
identity stores used here are the supported ephemeral stores.

The child creates the actual Manual `TorClient::create_unbootstrapped`, inserts
held B into its actual ephemeral key manager, checks approved identity, drops its
owners and tears down its Tokio runtime. The subprocess is then joined. This is
not a proof of graceful shutdown of every internal Arti task: the bounded process
boundary, not a new SDK shutdown framework, owns final cleanup.

## Executed baseline evidence

Pinned Nix environment: Rust/Cargo **1.92.0**, Arti **0.47.0**, Darwin arm64.
Every gateway Cargo command selects **`--features tor`**, never `--all-features`.
The normal shipping-binary dependency closure was compiled separately. Tests have
the repository's normal dev dependencies (notably Tokio test utilities/rcgen);
this is not a measured Linux release-artifact execution.

- Unmodified production factory on the baseline: full `prepare_with_grants`
  with public A/synthetic B **passed**; real unbootstrapped SDK construction,
  actual client ephemeral B insertion/identity check and scratch cleanup passed.
- Same baseline path with selected production B: **passed**, once, after the
  synthetic qualification above. Output was only
  `code=tor_offline_e2e_prepare_and_sdk_ok`.
- Thus ordinary Darwin did **not** reproduce the production
  `tor_configuration_invalid` failure. No before/after fix claim follows from
  that passing baseline. Missing Linux NSS/passwd behavior remains separately
  owned by the builder-fix packet.
- New integration `full_preparation_shared_ingress_tls_bridge_stream_and_owned_drain`
  **passed**: full public preparation/configure, real TCP acceptor and combined
  ingress, Futures/Tokio compat onion stream, actual opaque bridge TCP copy,
  real local Rustls client/server handshake, returning TCP HTTP connection,
  authenticated synthetic API request, progressive stream, pre-generation
  reservation, complete settled fixture receipt, quiesce and once-only settlement,
  both shim/server task joins and refusal on both released ports.
- The first new test attempt timed out at its fixture's `generation` checkpoint:
  it waited before flushing the TLS request/reading its deliberately tiny
  transport buffer. Flushing and consuming progressive delivery before waiting
  corrected the **test harness**, not production backpressure behavior. The
  short-name `--exact` attempt selected zero tests and is not counted as a pass.
- Library suite: **407 passed, 1 operator-only helper ignored**.
- API suites: **9 auth + 38 chat + 5 model tests passed**, including maintained SDK
  stream decoding, final/partial settlement, quote/credit/trust refusals,
  provisional tools and interrupted terminal evidence.
- Actual compiled native Pi connector: **87 passed** against local fake SOCKS,
  TLS and HTTP fixtures, including early data, abort/EOF, both copy directions,
  backpressure, pre-HTTP certificate policy, no fallback and final helper ownership.
- Strict selected-feature library/test Clippy passed. Package formatting and
  explicit formatting of included test files are checked separately. Existing
  vendored Tinfoil dead-code warning is unchanged.

## Candidate result: constructor blocker, shipping HOLD

Completed production fix `b123869dc8a058a63ddc57e34919d8d96470c4f6` was inspected and
cherry-picked **only** into this isolated test branch (local `f4170aa`). The tester
made no production fix and did not merge/push/deploy. The fix's completed owner
supplied a tiny public C injector interposing only `getpwuid_r`/`getpwnam_r` to
return `ENOENT` and a null result. Its inspected source is retained in
`tests/fixtures/tor-missing-nss.c`; an independently built local dylib is pinned
separately from the Rust helper. Injection is rejected for private-input mode.

Independent public before/after execution:

1. Baseline helper with actual qualified network/write denial, `HOME=/tmp` and
   injected missing NSS: full preparation returned `PREPARE_CONFIGURATION`;
   wrapper output `code=tor_offline_e2e_baseline_confirmed`. An additional public
   probe requires the default strict Mistrust build to fail, proving the injector
   actually took effect instead of being stripped by Darwin SIP.
2. Baseline combined integration under the same public NSS injector: failed
   immediately at public full preparation, typed `Configuration`; ingress was
   not started. This is a synthetic platform-fault reproduction, not proof of the
   deployed failure's cause.
3. Candidate combined preparation/ingress/TLS/stream/drain test: **passed** under
   injection. It does not itself construct `TorClient`.
4. Candidate full preparation **plus actual SDK constructor** under the qualified
   deny-network/write wrapper: **FAILED**. Stage projection initially enclosed
   the whole constructor chain; splitting the expressions proved the panic is
   in standalone **`TorClient::builder()`**, before `.config(...)`, Manual policy,
   `create_unbootstrapped`, key-store access, bootstrap or publication. Final closed
   output is `code=tor_offline_e2e_sdk_builder_panic`, exit 2. The panic payload was
   neither inspected nor formatted; stage markers are locally authored constants.
5. Candidate ordinary (no NSS injection) full-prepare/SDK helper: **passed**.
   That success must not erase the injected constructor failure. No private B
   was reread for this candidate or injected reproduction; retained real-B
   evidence applies only to the earlier ordinary baseline path.

Final ordinary candidate checks: **409 library tests passed, 1 operator helper
ignored**; selected-feature strict library/test Clippy, actual shipping-feature
binary compilation, package/included-file formatting, whitespace checks and
active changed-file LSP diagnostics passed. The default-feature-off test also
passed with its lookup closure refusing every identity/config lookup beyond the
switch. These successful checks do **not** override the injected constructor
failure. Final build/test executables use this worktree's private `target/build`
namespace; only compiled dependency artifacts were seeded by filesystem clone
from the existing cache. The copied/hash-pinned helper is never rebuilt in place
by another worktree.

The parent and production-fix owner received the new boundary immediately.
**No successful post-builder startup acceptance or shipping approval is claimed.**
The production-fix owner retains investigation and implementation ownership.
Future tests must exercise its actual resulting construction path, not merely
our combined transport fixture. The Linux NSS equivalent reported by that owner
is separate evidence; this tester did not execute a shipping Linux SDK/image.

## Reproduction

Run from this test worktree. The shared target directory is an optional compilation
cache; keep the hash-pinned private helper as a separate copy in this worktree.
Never invoke the private entry through Cargo or a general test runner.

`nix develop --offline --command cargo build --offline --locked --features tor --bin possums`

`nix develop --offline --command cargo test --offline --locked --features tor --lib server::lifecycle_tests::tor_offline_e2e_tests::full_preparation_shared_ingress_tls_bridge_stream_and_owned_drain -- --exact`

`nix develop --offline --command cargo test --offline --locked --features tor --lib -- --test-threads=4`

`nix develop --offline --command cargo test --offline --locked --features tor --test api_auth --test api_chat --test api_models`

`nix develop --offline --command cargo clippy --offline --locked --features tor --lib --tests -- -D warnings`

`nix develop --offline --command cargo fmt --package possums -- --check`

`nix develop --offline --command rustfmt --edition 2021 --check src/tor_offline_e2e_tests.rs src/server/tor_offline_e2e_tests.rs`

The operator helper is intentionally not portable-by-default: its executable pin
must match the reviewed local build copied to `target/tor-offline-e2e-helper`.
After any rebuild, review/re-pin that **public artifact** and rerun qualification
before authorizing private input. A changed or absent artifact fails closed.

`/usr/bin/python3 scripts/tor-offline-e2e.py qualify`

Build/review and pin the separate **public-only** injector before using this
optional synthetic mode; never put `DYLD_INSERT_LIBRARIES` on Cargo/Nix or a
private-input execution. The wrapper sets it immediately before the Rust child
inside the sandbox and checks an independent public NSS-effect probe.

`clang -dynamiclib tests/fixtures/tor-missing-nss.c -o target/missing_nss.dylib`

`/usr/bin/python3 scripts/tor-offline-e2e.py qualify --synthetic-missing-nss`

On the tested candidate the latter command intentionally exits 2 with
`code=tor_offline_e2e_sdk_builder_panic`. Do not reinterpret libtest's inner
one-test pass (the containment entry) as success; the wrapper's closed code and
exit status are authoritative. The successful combined local-network fixture is
a **separate public-only** direct test-executable invocation with the injector,
not a private sandbox invocation or proof of SDK constructor success.

Native connector reproduction uses the locked supported SDK, not a replacement
SOCKS parser. Build with the pinned compiler and pass the resulting helper path
to the maintained suite using `POSSUMS_TOR_NATIVE_TEST_HELPER`. The executed helper
was copied into this worktree, and the Node test child had an empty environment
except pinned PATH and that public executable path.

`nix develop --offline --command cargo build --offline --locked --manifest-path clients/pi/tor-native/Cargo.toml`

## Exact remaining gates

- **Real production B:** yes, preparation/client-keystore only. No signing,
  rendezvous, descriptor publication or Tor transport with that identity.
- **A/appraisal:** public synthetic A only; no fresh production nonce/hardware/
  publisher/application-key chain or mounted-grant provenance/rotation proof.
- **Bootstrap/onion acceptance:** not called. Combined ingress substitutes a
  duplex/compat stream for SDK rendezvous/accept; existing SDK BEGIN/port/32-pair/
  two-permit and shared-64-limit tests remain synthetic source evidence.
- **Public shim/TLS/EHBP:** local real Rustls, fixture certificate/root and local
  shim. Not platform TLS, production endpoint binding, public DNS or EHBP proof.
- **Client:** compiled native connector passes its maintained local fixtures;
  not installed Pi/T3 behavior or a live selected-B connection. No client trust
  checker is disabled to make this gateway test pass. Synthetic fixture roots
  cannot authorize real credentials or prompts.
- **Upstream/accounting:** synthetic `Inference`/`EvidenceVerifier`, public test
  account/session created by the existing fixture. Observed reservation, receipt
  and once-only accounting are local fixture outcomes, not upstream inference,
  billing, refund or invoice-parity evidence. No new wire decoder was written.
- **Binary/platform:** the actual gateway binary is compiled, not run through
  successful startup: production verified upstream and real A-provider boundaries
  prevent that offline path. Tests use actual production functions in process;
  no image test mode or trust bypass is added. Darwin's real UID/NSS/permissions
  are not Linux's empty passwd/group database, measured image, read-only root or
  container UID. Deployment and Linux source/image qualification remain separate.
- **Live paired acceptance:** only an exclusively owned properly running gateway
  can qualify Tor bootstrap/rendezvous/publication, selected-B transport,
  independently fresh A/endorsement/channel verification, installed Pi streaming,
  receipts, disconnect/backpressure and shutdown. Legacy-v2 freshness and native
  v3 platform-signer limitations are not resolved by these local checks.
