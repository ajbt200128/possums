# Bounded Arti embedding spike

> Historical research only. The standalone `spikes/` tree this document
> describes is still absent from the current source; current Tor support lives
> in [`src/tor_ingress`](../src/tor_ingress) and the restored client helper, not
> here. Commands and authorizations below do not apply. Results and limitations
> remain historical. Current scope:
> [Tor main restoration](tor-main-restoration.md).

**Local source/compile and offline synthetic evidence only, 2026-10-13.**
Feasibility exploration, not a shipping Tor implementation, measured image,
installed client, live service, v3 success or privacy-policy change. Scope is only
`spikes/tor-arti/**` and this document. No additional agents, commits, installation,
deployment, Tor bootstrap, onion publication, self-connect/gateway probe,
inference, real credentials/identities, telemetry or diagnostic-log artifacts.
Public source/dependency downloads and isolated scratch builds were used.

Policies read before state/diagnostic work: [repository guidance](../AGENTS.md),
[specification](../SPEC.md), [privacy data boundaries](../PRIVACY.md#data-handling-boundaries),
[forbidden exports](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[typed failures](../PRIVACY.md#typed-interface-failures-and-token-aggregate-extension),
[synthetic testing](../PRIVACY.md#synthetic-testing-exception),
[development environment](development.md), [lifecycle limits](blue-green-runtime-plan.md)
and [public-cache boundary](pi-public-evidence-cache-plan.md). This record is
scoped to the spike, not a change to production verification records or intake.

## Decision

**Rust same-process embedding is technically available. Production qualification
is incomplete.** Released Arti gives application-owned rendezvous and
stream acceptance, and its Tokio `DataStream` compiles through the official
Hyper/Axum adaptation. A small offline implementation demonstrates bounded
application ownership, backpressure, EOF and cancellation. Native Arti key-manager
roundtrip/reopen/offline restore is also feasible without any Tor connection.

This does not depend on v3 working. Separately unresolved: production's Rust
MSRV, Arti's whole-host fatal exit/diagnostic behavior, approved private persistence,
egress configuration, real stream lifecycle and the shim/trust integration route.
Same-address-space OOM, abort and panics remain shared risks; task boundaries are
not process isolation. Do not bypass any authentication, attestation, encryption,
reservation, settlement, receipt or tool gate to make the transport work.

## Prototype and what actually ran

Independent `[workspace]` library under [the spike](../spikes/tor-arti/README.md),
with its own manifest and lockfile; root Cargo/lock/toolchain/configuration are
unchanged. No executable or listener, client creation/bootstrap, network dialer,
trust contract or production API route exists in this package.

- `src/embedding.rs`: **uncalled compile-only** function accepts an existing
  `TorClient<PreferredRuntime>` (Tokio/rustls with these features), checks its
  actual key manager, then type-checks `launch_onion_service`,
  `RendRequest::accept`, `StreamRequest::accept(Connected::new_empty())`, and
  `TokioIo<DataStream> → Hyper HTTP/1 → Axum`. Only BEGIN/port 80 is accepted;
  other requests use the documented END/DONE rejection. No call to this function
  ran. Invoking it would bootstrap/publish and is outside authorization.
- `src/lib.rs`: `run_bounded` uses supported Futures combinators, one owner and a
  `NonZeroUsize` bound, not spawned tasks or an application queue. Each rendezvous
  occupies one slot through handshake and circuit lifetime; streams within that
  circuit run serially. Thus at most N application handshake/session futures and
  N stream handlers exist. This sacrifices multiplexing throughput and lets an
  idle circuit hold a slot; no production fairness/deadline claim. Cancellation
  stops source admission and drops owned futures; first observed failure retains
  its closed category. This is prototype-wide fail-stop, not a proposed change to
  gateway accounting/cancellation policy.
- HTTP is a fixed `synthetic-only` Axum response over local `tokio::io::duplex`,
  not a substitute `/v1` endpoint or trusted HTTP-over-onion service.
- Opaque forwarding uses Tokio's `copy_bidirectional`, fixed 8 KiB buffers per
  direction, no HTTP parsing or TLS termination. The second stream is supplied,
  not connected by the prototype. Binary synthetic bytes pass unchanged; this
  proves byte adaptation, **not a TLS handshake or verified endpoint binding**.
  Backpressure remains pending rather than being interpreted as disconnect; a
  one-way EOF half-closes the opposite writer while preserving reverse delivery.
- `tests/offline.rs`: 11 tests cover owner limits/no extra polling at capacity,
  pre-stop and ready-handler stop admission, finite completion, failure/drop,
  failure versus same-handler stop, actual Hyper/Axum duplex response, cancellation
  during partial headers, binary/half-close forwarding, stalled reader/bounded
  buffering and abrupt peer loss. No sleeps, network sockets or detached tasks.
- `tests/identity.rs`: 3 tests use `ArtiNativeKeystore`, `KeyMgrBuilder`, supported
  `generate/get/insert/remove`, `HsIdKeypairSpecifier`, `HsIdPublicKeySpecifier`
  and Arti's `CautiousRng`. Random **synthetic** private keys exist only in 0700
  RAII temporary directories; restoration copies only between those scratch
  directories after the original manager is dropped. Nothing prints private keys
  or onion addresses, or writes keys to the repository/docs. Ordinary test exits
  remove directories; forced process loss/secure erasure is not qualified.

### Identity gate

`require_identity` is read-only: private pair must exist, its derived public
identity must match the operator-approved `HsId`, and the public identity obtained
through KeyMgr must agree. KeyMgr derives the public key from the pair when there
is no standalone public file; such a file is **not** a prerequisite. A conflicting
standalone public key, wrong approval or a public-only/missing-pair restore fails
closed. The tests prove no pair is generated on rejection.

This matters because ordinary service creation can generate missing identities;
`launch_onion_service_with_hsid` also inserts a key and is experimental, not a
read-only recovery gate. The prototype uses neither as a fallback. The host must
exclusively own trusted state between preflight and launch: this check is not a
cross-process lock, approval-source authenticator or rollback detector. No real
publisher ran, and no signed endorsement of onion identity was implemented.

## Approved pinned compiler continuation

The operator approved the source-only compiler upgrade. Production source now
pins Rust **1.92.0**, supplied by the unchanged locked rust-overlay. Parent checks
passed the full root suite (319 library tests plus integration suites, four
existing ignored tests), strict root Clippy/format, and **14 unpatched Arti
spike tests** plus strict Clippy on that pinned compiler. The real embedding
function compiled but remained uncalled: no Tor bootstrap/publication occurred.
See [toolchain verification](tor-toolchain-verification.md) for exact scope and
mechanical lint compatibility edits. Earlier 1.88/1.99 records below are historical;
Linux measured-image and live-runtime qualification remain outstanding.

## Released source, features and maintenance

Crates.io's latest release at inspection was **0.47.0** for `arti-client`,
`tor-hsservice` and supporting Tor crates. Their manifests declare **Rust 1.92**
and **MIT OR Apache-2.0**. Downloaded `arti-client` archive identifies source commit
`ce8bc6e0998bd5a4efdf06dd62dce53c98ea1087` in `.cargo_vcs_info.json`. The spike
lockfile pins registry versions/checksums; no vendored/custom protocol fork.
Inspected archives for arti-client, tor-hsservice, tor-keymgr, tor-dirmgr,
tor-proto, tor-memquota and tor-guardmgr matched their lockfile SHA-256 checksums.
The standalone lockfile contains 523 packages (including optional/target entries);
small host glue does not imply a small dependency audit surface.

`arti-client` defaults are Tokio/native-TLS/compression. The spike disables those
and selects `tokio`, `rustls`, `onion-service-service`, `onion-service-cli-extra`,
`memquota`, `vanguards` and `static-sqlite`. Native KeyMgr access via `keymgr()` is
gated by **experimental `onion-service-cli-extra`**, not just `keymgr`.
`cargo tree -e features` confirmed these selections, Tokio stream traits,
`tor-memquota/memquota`, and no `tor-hsservice/hs-pow-full`/metrics feature.
Service support pulls some client/experimental internals transitively; this is
not a claim of an entirely stable, server-only API/dependency graph.

Static SQLite uses the supported Arti feature because the scratch/Nix combination
initially lacked a linkable native SQLite library. No library/toolchain was
installed and no production feature choice is implied. Lockfile transport
versions: Tokio 1.53.3, Futures 0.3.34, Axum 0.8.9, Hyper 1.12.0, Hyper-util 0.1.21,
Tokio-util 0.7.20; key test dependencies include fs-mistrust 0.16.0 and
Tempfile 3.27.0. Minimum manifest MSRV remains 1.88 for the default synthetic
subset; `--features arti` needs a newer compiler. **1.92 itself was not tested.**

[Upstream support policy](https://arti.torproject.org/contributing/support-policy/)
supports only the latest release, expects potentially breaking APIs before 1.0,
recommends its production lockfile dependencies and requires library consumers to
check dependency advisories. It describes intent, not a formal guarantee. No
`cargo-audit` executable was available; none was installed. This is **not a clean
security-advisory or whole-transitive-license audit**. A pinned unsupported older
release is not a solution to the production MSRV blocker.

## Whole-host fatal exit: accepted fail-fast policy

The operator subsequently chose unmodified upstream Arti and accepted whole-gateway
termination when required Tor protocols are unsupported. No lifecycle patch is to
be shipped. The probe below is historical research only, not the selected solution.
Process exit can interrupt requests and accounting owners; it establishes neither
settlement nor refunds and must not trigger automatic inference replay. Upstream
raw diagnostics remain an external privacy boundary, not a sanitization guarantee.

**Continuation result: no viable small host-safe lifecycle replacement was
established.** A test-only patch compiles and probes the actual released library;
it is not a production fix, public fatal hook or permission to continue Tor work.
The original client/HTTP embedding remains uncalled and unmodified.

Upstream was checked first by cloning the public Git repository, not trusting a
moving documentation sample. `main` HEAD was **3adfc4b052b6520c12f6193c593c5557fa5f3771**
(commit date 2026-10-08); `crates/arti-client/src/client.rs` and `protostatus.rs`
were byte-identical to released **0.47.0**. No fix is present in these inspected
files. The public Crates.io API also reconfirmed **0.47.0** as the latest
`arti-client` release during continuation. Source still references [issue 1932](https://gitlab.torproject.org/tpo/core/arti/-/issues/1932).
Web issue/raw-page retrieval supplied a challenge page, not usable source;
issue discussion and unmerged work remain unknown. Git source inspection succeeded.
The tested registry archive corresponds to release commit
**ce8bc6e0998bd5a4efdf06dd62dce53c98ea1087**, SHA-256
`43e6531f26fe6f44d71dabdc63b7845bae81547cc738aa942f0e3e482f776388`.

Actual `arti-client/src/client.rs:1300–1323` calls private
`protostatus::enforce_protocol_recommendations` with a hard-coded fatal closure.
A later unsupported required protocol causes raw tracing/stderr diagnostics,
a five-second sleep, then **`std::process::exit(1)`**. Drop, gateway drain,
settlement cleanup and a task supervisor cannot intercept it; `catch_unwind`
cannot fix it. Cached unsupported status instead returns a typed construction
error. Required-protocol enforcement must remain intact.

### Real dataflow and missing shutdown ownership

Released source establishes why replacing that closure alone is insufficient:

- `tor-dirmgr/src/state.rs:510–548`: `try_checking_sigs` checks authority keys and
  consensus signatures; only validated consensus populates protocol status.
  `tor-dirmgr/src/lib.rs:1121–1128` stores it and publishes
  `NewProtocolRecommendation`. `protostatus.rs:55–69,110–143` preserves software
  publication-time filtering, duplicate-status suppression and required-protocol
  rejection. None of these validation/decision paths were changed.
- `arti-client/src/client.rs:195–208,2356–2385`: private `Inner::Poisoned` already
  rejects access through the client, but is used for construction failure, not a
  later global shutdown. In-flight callers retain `Arc<RunningInner>`. Onion
  launch returns a separately owned service with cloned directory/circuit-pool
  references (`client.rs:1989–2037`). Poisoning the client would not revoke these.
- `tor-rtcompat/src/impls/tokio.rs:421–431`: actual `Spawn::spawn_obj` drops the Tokio
  join handle, explicitly detaching the task. `Runtime::spawn` returns no abort or
  joined-shutdown handle. Channel reactors use this path
  (`tor-chanmgr/src/builder.rs:601–607`); the client does not own a task registry.
- `tor-proto/src/channel.rs:267–279,958–962`: an individual channel has
  `terminate()`, and existing circuits retain channels. `ChanMgr` has no public
  terminate-all/terminal-close/enumerate-all-channels API. A caller cannot assume
  dropping `TorClient` closes every active channel or pending builder.
- `tor-circmgr/src/mgr.rs:1652–1684`: retirement removes reusable/pending entries;
  it is not closure of active tunnels and explicitly permits newly built tunnels.
  `HsCircPool::retire_all_circuits` is likewise not a terminal fence.
- `DormantMode::Soft` is reversible and wakes on use (`client.rs:638–645`).
  `TaskHandle::cancel` cancels only a pending scheduled firing
  (`tor-rtcompat/src/scheduler.rs:159–166`), not an in-progress operation; dropping
  the final handle tells the schedule to terminate when observed. Neither is a
  global cancellation acknowledgement.
- Dropping the last service handle closes its private broadcast sender
  (`tor-hsservice/src/lib.rs:144–160`); publisher upload tasks have their own
  reactor-drop cancellation path (`publish/reactor.rs:1416–1447,1639–1649`). This
  useful service-specific ownership is not a client-wide fence or joined
  completion proof for all directory/build/channel/circuit tasks.

Thus public APIs do not supply the needed irreversible **new + ongoing** Tor
operation fence and all-owner shutdown acknowledgement. A callback-only patch,
`Inner::Poisoned` alone, soft dormancy, retirement or dropping application handlers
would fail this constraint. No such unsafe runtime patch was retained. A custom
cancel-aware Runtime/task/I/O framework was intentionally not built. A dedicated
Tor-only runtime could be investigated separately, but clone/blocking ownership,
shutdown acknowledgement and no-post-fatal-network guarantees were not qualified;
this is not a current solution.

### Small compiled probe, not a lifecycle fix

[The patch artifact](../spikes/tor-arti/patches/protocol-lifecycle-probe.patch)
adds only a `cfg(test)` private test module to `protostatus.rs`. Its
[fixture](../spikes/tor-arti/patches/lifecycle_probe.rs) calls the actual
`enforce_protocol_recommendations` and `watch_protocol_statuses`; production
closure, parser, verifier and diagnostics remain unchanged. Three tests pass:

1. Cached missing required protocol rejects synchronously before callback/spawn.
2. Later fatal status returns after one callback/event, maps the typed error to a
   closed payload-free terminal state, stops admission and drops both bounded
   application owners. A synthetic host API future still completes and an
   already-unknown accounting-like value stays unknown: no inferred refund,
   cancellation, receipt or replay. **Negative boundary:** a separately spawned
   task through the real Arti Runtime survives, and the runtime accepts new work.
   Explicit fixture-only stop/ack then drops that task; no test task is left running.
3. Pre-publication status and missing recommendations alone remain nonfatal;
   required-protocol enforcement is not relaxed to make tests pass.

The fixture injects **synthetic output at the authenticated-status boundary**,
not a cryptographically authenticated consensus or a real channel/reactor. Source
inspection establishes the signature-checking dataflow; these tests do not
requalify consensus cryptography, real resource teardown or client-wide poisoning.
The payload-free callback excludes a hostile synthetic protocol name; no subscriber
is installed. **Upstream raw tracing/stderr defaults still fail the proposed host
privacy requirement.** No whole-library diagnostic privacy pass is claimed.

[Scratch runner](../spikes/tor-arti/patches/run_probe.py) verifies the archive
checksum, extracts only `arti-client` to an RAII scratch directory, applies the
zero-fuzz patch, adds fixture-only dependencies and runs the targeted unit tests
plus a non-test library check. Nothing edits the registry source or production
manifest/config/lock/toolchain; no framework is vendored. Ordinary runner exits
remove scratch source/lockfiles; build artifacts and public dependency cache remain.
The runner reports closed stage-specific failures without exception payloads and
never initiates Tor/inference work. Cargo diagnostics concern public source only;
no raw logs/reports/support files are generated. Native harness history remains
outside this transient-diagnostic claim.

The official helper in `tor-hsservice/src/helpers.rs` uses
`flat_map_unordered(None, ...)` and logs accept errors; the official Axum sample
also spawns each stream without a cap and prints its onion identity. This spike
uses the public individual accept methods and does not copy those behaviors.

## Resource and lifecycle boundaries

Released source defaults are not safe whole-gateway capacity proofs:

- 3 introduction points (maximum 20); 65,535 concurrent streams per circuit;
  introduction rate limiting defaults to consensus choice and is enforced by
  introduction points, not locally by this configuration.
- Non-PoW rendezvous channel base depth 32, using Futures MPSC/no-memquota;
  per-sender capacity behavior means this is not a total 32-object RSS guarantee.
  PoW is disabled by default; its configured queue depth is 8,192 (source estimate
  roughly 32 MiB), but the full PoW feature is not selected here.
- With `memquota`, auto maximum is 40% of available memory at >=8 GiB, otherwise
  75%, clamped to 256 MiB–8 GiB on 64-bit; low-water is 75% of that maximum. Memory
  discovery failure falls back to 8 GiB on 64-bit. It accounts selected internal
  queues, **not total RSS or application/prompt/Hyper buffers**. Shared 8 GiB enclave
  headroom is unqualified; a future authorized integration must budget explicitly.
- `vanguards` is selected rather than silently dropping that protection. Source
  parameter defaults specify Lite/general and Full/onion-service levels; actual
  network/configuration behavior was not exercised.

The application N bound does not replace Arti internal circuit/stream queues,
transport/parser defenses or process memory limits. The compile-only function
receives caller configuration without imposing a production per-circuit policy.
No memory/throughput benchmark or adversarial Tor load ran. Dropping the last
`RunningOnionService` stops publication/new introductions by documented semantics;
existing protocol state/network visibility may outlive that action. The spike owns
its handlers, but real Arti background-task cancellation, stream Drop/END timing,
service shutdown and network erasure were **not** measured. Direct cancellation
here is transport exploration, not evidence of refunds/upstream compute stopping.

Arti emits internal tracing events. No subscriber, exporter or diagnostic file was
added; this is **not** a whole-process no-logging guarantee. In particular the
fatal path and SDK panic hooks remain external boundaries. Any future host
subscriber/diagnostic integration must prevent Tor identity/path/raw error capture
under the existing privacy policy rather than importing the official sample's logs.

## Private state, persistence and rollback

Stable service identity requires the private onion key B in Arti's native
keystore, plus protected guard/vanguard selection and service state. Source uses
persistent guard storage and a per-nickname service state-directory lock, with
introduction-point publication/replay-related state (and PoW state when enabled).
Repeated fresh guard selection is not a privacy-preserving restart strategy.
Public directory/consensus cache bytes are revalidated acquisition material and
can be reacquired; **private identity, guards, service/replay state are not public
cache** and are not covered by the Pi public-certificate exception.

No persistent-production-state permission follows from these synthetic tests.
Do not put prompt/response/account records alongside Tor state or export onion
IDs, guard addresses, paths, key material, filenames or raw SDK diagnostics.
A future authorization must define protected storage, ownership, state schema
compatibility, recovery, deletion/backups, and the correlation/rollback limitations.
Retain the original identity and compatible private state for a controlled rollback;
never run concurrent publishers from copied B/state in blue and green. Arti's
local state-directory lock cannot prevent two separately copied directories or
enclaves publishing the same identity. Source key-manager mutation also documents
TOCTOU constraints; synthetic sequential copying is not a distributed lock or safe
live backup protocol.

[Tinfoil encrypted-volume documentation](https://docs.tinfoil.sh/containers/encrypted-volumes.md)
says volumes are **beta / v3-dependent**, owner-key protected and attached to one
enclave at a time. It explicitly provides **no sector/whole-disk rollback detection**
and exposes storage access patterns. Attachment/configuration is not successful
unlock or durability evidence. [Automatic unlock](https://docs.tinfoil.sh/containers/volume-auto-unlock.md)
requires separately operated key release and exact approved workload pins; original
volume key is required on recovery, not regeneration/ordinary secret rotation.
This was documentation research only. No keys, mounts, policies, private secrets,
volumes or v3 verification were provisioned or qualified. These platform limits do
not invalidate the independent embedding/key-manager feasibility result.

## Shim routes and egress (not implemented)

Research baseline: the Tinfoil Go CVM shim owns public TLS/EHBP; Rust listens behind
it on `:8080`. Calling that Rust listener directly does not reproduce shim protection.

1. **Private shim ingress:** potentially preserves existing trust contract, but no
   supported Rust-to-private-shim entry point was established. The reserved
   `shim-net` carries shim → container traffic, not a documented reverse ingress API.
2. **HTTP-over-onion with attested A endorsing persistent B:** needs a separately
   reviewed binding/freshness/authorization contract and clients. [Attested-key docs](https://docs.tinfoil.sh/containers/attested-keys.md)
   mark it v3-dependent beta; key A is regenerated on enclave reboot/deploy/update,
   unlike intended persistent B. No endorsement, verification or plaintext production
   endpoint was implemented; this route must not bypass existing auth/encryption.
3. **Embedded blind bridge to public gateway HTTPS:** smallest byte-only transport
   candidate that can leave TLS/EHBP termination and existing verifier/auth at the
   public shim. Requires the client's correct authority/SNI/key binding, fixed
   approved destination and supported public self-connect reachability. This spike
   has **no dialer, TLS client, self-connect test or proof that Tinfoil permits it**.
   A blind bridge may carry encrypted bytes without learning their content, but
   buffering/network metadata/shared-host risks remain. It is not authorization to
   connect to the unprotected loopback Rust API instead.

Current `tinfoil-config.yml` allows only four named upstream/proxy/telemetry hosts.
Direct Arti reaches changing relay/directory **IP addresses** and ports; this
hostname-only allowlist cannot support that. [Tinfoil networking docs](https://docs.tinfoil.sh/containers/config-networking.md)
expose `open` public egress, still blocking private/internal ranges. Direct Tor
requires separately approved open public egress under that model; fixed bridges or
an external proxy are different, untested architectures, not this same-process
spike. Open egress widens exfiltration/traffic-correlation exposure and changes the
measurement/threat model. No egress or CVM configuration was edited.

## Exact verification and reproduction

Pinned `nix develop` successfully reports **rustc 1.88.0**. Default subset:

`nix develop --command bash -c 'export CARGO_TARGET_DIR=/tmp/possums-tor-arti-target-1.88; cargo test --locked --manifest-path spikes/tor-arti/Cargo.toml --no-default-features; cargo clippy --locked --manifest-path spikes/tor-arti/Cargo.toml --all-targets --no-default-features -- -D warnings'`

Result: **11/11 tests**, strict all-target Clippy pass. Format check with pinned
rustfmt also passed:

`nix develop --command cargo fmt --manifest-path spikes/tor-arti/Cargo.toml -- --check`

Pinned `cargo check --locked --features arti` rejected declared MSRV 1.92 packages
before compilation: **expected dependency incompatibility, not broken Nix tooling**.
No `--ignore-rust-version`, dependency downgrade or root change was used.

Already-installed `stable-aarch64-apple-darwin` was **rustc 1.99.0
(b940084d7 2026-09-28)** / Cargo 1.99.0. Scratch compiler/rustdoc and Clippy must be
explicit: the Nix shell otherwise selects 1.88 (even `rustup run stable cargo
clippy` initially found the Nix Clippy driver). Do not install a new toolchain if
this one is unavailable; report the blocker. The following uses the existing
scratch toolchain with the pinned Nix shell's remaining native environment:

`nix develop --command bash -c 'set -e; export CARGO_TARGET_DIR=/tmp/possums-tor-arti-target-1.99; export RUSTC="$(rustup which --toolchain stable rustc)" RUSTDOC="$(rustup which --toolchain stable rustdoc)"; export PATH="$(dirname "$RUSTC"):$PATH"; rustc --version; cargo test --locked --manifest-path spikes/tor-arti/Cargo.toml --features arti; cargo clippy --locked --manifest-path spikes/tor-arti/Cargo.toml --all-targets --features arti -- -D warnings'`

Result: **14/14 tests**, real released-API compilation and strict all-target
Clippy pass on native macOS arm64. These unchanged baseline checks were rerun
successfully after the lifecycle continuation. The prior active LSP probe of the
four original Rust files found no diagnostics; Cargo is the authoritative
feature/compiler check. This is not
Linux/image/MSRV-1.92 qualification. Initial source fixes and scratch SQLite/temp
permission failures were corrected before final passes; no failure logs were saved.
The build directories contain only scratch binaries/public dependencies, not the
RAII test keystores. No whole-repository suite was run or claimed.

### Lifecycle continuation reproduction

`nix develop --command bash -c 'CARGO_TARGET_DIR=/tmp/possums-tor-arti-target-1.99 python3 spikes/tor-arti/patches/run_probe.py'`

Result: **3/3 targeted actual-library probe tests**, patched non-test library
`cargo check --locked` pass, Rust **1.99.0 (b940084d7 2026-09-28)**, macOS arm64.
This is a passing **negative-boundary probe**, not a passing shutdown fix. Only
those three tests ran (46 unrelated upstream tests filtered out). Zero-fuzz patch
application and fixture Rustfmt checks passed. The unchanged production-Nix default
subset was rerun: **11/11**, strict Clippy and format pass on **Rust 1.88.0**.
Two manual synthetic runner fault injections (mocked toolchain lookup failures)
returned the closed toolchain-stage diagnostic without hostile exception text;
no subprocess or network work ran in those cases. An initial synthetic fixture
was corrected to legal protocol syntax before the final passes; no failure logs
were retained.

Tested patch SHA-256:
`2ad95670b52bf56c36f99d09054ad8f52dd0dfa7892c6968249b942e2aa0fab6`.
Tested fixture SHA-256:
`e8bd6048bbe013ac49c7ed632b95795c809827840a3335d347190ec839f35ddf`.

The runner seeds scratch resolution from the spike lock; upstream crate unit-test
metadata adds 87 optional/dev/target entries without changing seeded dependency
versions. Compiled extra test dependencies include Anyhow 1.0.104, native-tls
0.2.18, async-native-tls 0.6.0, core-foundation 0.10.1 and security-framework 3.7.0
(upstream dev `tor-rtcompat` requests native TLS). This does not change the spike's
production-feature rustls selection. These extra unit-test resolutions are not
fully pinned in a second repository lockfile; future dev dependency resolution
may differ. The archive/patch and existing spike versions are pinned, but this is
not a bit-for-bit or Linux-image reproduction claim. No MSRV-1.92 build or
transitive security audit ran. The production compiler upgrade remains necessary;
an older unsupported Arti is not an approved workaround.

Next source-integration work uses upstream Arti unchanged under the accepted
fail-fast policy. The prior all-owner graceful-shutdown requirement is superseded;
this does not establish graceful accounting cleanup on process exit. Preserve
observed accounting evidence and unknown outcomes, and never infer settlement or
refunds from transport loss. Ordinary application shutdown still needs explicit
resource ownership. Tor signing A→B requires the merged v3 application's endorsed
key interface; neither this probe nor HTTP adaptation qualifies that binding.

Primary source references: [official Axum sample](https://tpo.pages.torproject.net/core/doc/rust/src/axum_hello_world/main.rs.html),
[arti-client 0.47.0](https://docs.rs/arti-client/0.47.0/arti_client/),
[tor-hsservice 0.47.0](https://docs.rs/tor-hsservice/0.47.0/tor_hsservice/),
[tor-keymgr 0.47.0](https://docs.rs/tor-keymgr/0.47.0/tor_keymgr/) and their downloaded
released crate source. Official sample is a moving documentation page; exact
released APIs were checked by compiling the pinned crates, not by assuming the
sample alone established compatibility.

**Delivery blockers/unknowns:** upstream diagnostic privacy boundary;
real DataStream/rendezvous lifecycle; measured
Linux artifact and resource budget; private state authorization/storage/rollback;
open egress; supported shim ingress or public self-connect; client trust contract
and independent client Tor report. Intake backlog note remains deferred to parent
delivery; no intake/backlog file was touched.
