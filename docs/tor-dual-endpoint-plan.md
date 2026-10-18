# Tor v0: two stable endpoints, shared source prerequisite

> Historical plan/evidence only in this inactive HTTPS-only backup. All Tor
> runtime, client and release hooks are removed, not merely disabled. Commands,
> continuation instructions and authorizations below do not apply here; prior
> results and unresolved failures remain historical. See
> [backup verification](without-tor-backup-verification.md).

**Current continuation:** backend source integration and the opaque shim bridge
below supersede the historical unwired/raw-HTTP plan. The operator now permits
Monero-style optional SOCKS credentials/no-auth, backend deployment and live E2E;
historical strict-auth and first-packet authorization restrictions below are
superseded. The subsequent operator decision **removes Tor startup self-appraisal**
and all cached-appraisal freshness checks through bootstrap/launch. It explicitly
supersedes the historical fresh-A-before-B startup invariant below, not fresh
per-request or independent client verification. See [publication boundary and
source-only evidence](tor-startup-removal-verification.md). This is authorization,
not deployment or transport evidence.

## Current opaque shim bridge — offline acceptance only

Raw onion HTTP bypassed Tinfoil's TLS/EHBP shim and was not deployable. Production
candidate onion streams enter a blind bridge to the **fixed** public shim destination;
TLS terminates at that shim, not in a new Rust adapter. The shim returns existing
HTTP to the unchanged router. No arbitrary destination, HTTP parser, custom TLS or
private shim protocol was added.

HTTP and opaque streams share the original 64 physical permits, leases, task set
and lifecycle/accounting drain. A paired Tor operation consumes two permits
(bridge plus returning HTTP), so at most 32 pairs without other connections;
both admissions fail fast rather than waiting on nested quotas. Accepted bridges
survive shutdown/backpressure. First-byte/dial/initial-write time is bounded;
observed downstream EOF/error drops the full upstream socket. Reporter faults
retain first-failure evidence and use the original drain.

Latest parent **501 default / 552 Tor-feature tests passed**, with four live
tests ignored in each; strict all-target/all-feature Clippy, package formatting
and whitespace passed (the earlier Tor run had 545 checks). Eleven new bridge regressions passed independently;
read-only review found no scoped issue. One combined parent run timed out after
successful early Tor binaries; the separate full rerun passed. Cause remains
unknown, and no test was changed to hide it. The reviewer also reported its
broader filtered command timeout; its eleven focused tests passed.

The revised optional [developer helper](tor-development.md) passed seven offline
stub checks. Nix now compiles the shipping gateway with Tor support, while runtime
activation remains explicit/default-off. Nix parsing/evaluation and local checks
are not measured Linux artifact qualification or deployment.

**Current client continuation:** the official, unmodified `tor-socksproto`
0.47.0 native connector passed **87/87 offline checks**, independently reproduced
by the parent for both debug and optimized Rust 1.92.0 Darwin arm64 artifacts.
Independent round-two review reproduced 87/87 and closed the initial readiness
ordering finding: pre-READY bytes are an explicit IPC ordering failure, not
cancellation; only observed EOF means cancellation. Generated native targets are
ignored. The review confirmed the official SDK source/archive/lock checksum.
Its Node wrapper owns private configuration/readiness pipes and one native child
per connection, retaining ownership through confirmed exit. Destination DNS is
proxy-side; raw socket success is not channel authorization. The operator
explicitly accepts the official SDK's unused CONNECT reserved-byte tolerance;
authentication version, offered methods, address type and status remain checked.
Optional credentials permit no-auth selection and do not establish isolation or
Tor traversal. Cancellation/EOF, backpressure, native-copy failure, pre-HTTP TLS
identity rejection and cleanup races were exercised with synthetic local peers.
Same-fresh-context channel integration passed 69 checks using the actual optimized
helper and local TLS/HTTP with synthetic appraisal. Final read-only client review
found no scoped issue, but did not independently run the matching package tests.
The parent rebuilt the matching Pi 1.0.4 package and its complete offline checks
passed, including 36 direct-v3, 691 reference-client, native SDK/RPC, selection,
closed-diagnostic and 13 typed-failure groups. Received monetary evidence survives
later delivery failure; incomplete delivery does not authorize tools. These
checks do not qualify installed T3 presentation or the live Tor service.

The earlier [Node candidates](tor-socks-connector-research.md#executed-monero-style-qualification--both-candidates-unqualified)
and libcurl remain unwired research, not alternate production fallbacks. Curl's
independent review additionally reproduced a lost bodyless-response cleanup
failure; neither successful TLS checks nor reserved-byte tolerance removes that
fault.

The SDK-native environment import of approved B passed **14 focused runtime
checks**. It uses bounded owned secret copies and the official expanded-Ed25519
OpenSSH decoder, without generating a replacement or materializing a private key
file. Zeroization of the owned copy does not erase the environment or establish
host-wide transience. The candidate measured config now requests the Ed25519 A
grant for uid/gid 65532, explicit Tor activation, open egress, a 512 MiB `/tmp`
tmpfs and a 300-second startup health grace. Schema inspection accepts those
fields; resource headroom, secret provisioning and deployment remain unqualified.
Program activation is still absent/false by default outside this explicit config.

Credential-free direct public probes observed native default TLS acceptance and
HTTP 200 from the gateway certificate endpoint. GitHub acquisition returned 403
without User-Agent and 200 with a fixed locally authored value; public-only Tor
acquisition now sends `possums-pi`, with fixture assertions that private request
headers remain unchanged. These probes used no inference or credentials and are
not Tor, serving-hardware or endorsement qualification.

Public-shim self-connect/correct-replica routing, positive live same-context
TLS/HPKE/B appraisal, mounted grants, stable B custody/exclusive publication,
resource headroom, measured Linux release, installed transport and live E2E remain
unqualified. No runtime activation/deployment/live inference was performed in
this continuation. Existing [privacy boundaries](../PRIVACY.md#data-handling-boundaries)
and [typed failures](../PRIVACY.md#typed-interface-failures-and-token-aggregate-extension)
remain intact: offline fixtures use synthetic data, no new telemetry/logs/support
bundles, no raw upstream diagnostics or private material in the record. Native
Arti/panic hooks and Pi/T3 history remain separate disclosed host boundaries.

## Historical first packet

## Decision and authorization boundary

The latest operator decision accepts **two user-chosen stable v3 onion
addresses**, one distinct secret-backed identity **B per future replica**, with
no Onionbalance and no Arti fork. This is a dual-address plan, not a
shared-address balancer; it does not provision another replica. Each selected address remains
stable across replacement only if that replica's approved B is restored safely.
The rotating, v3-attested application key **A** endorses only that replica's
approved locally owned B through the [existing endorsement
primitive](tor-identity-binding.md). No address or private key belongs in this
record.

**This first packet is an isolated serving-loop refactor, not a working Tor
service.** Production remains default clearnet and the existing router remains
the default. It adds no dependency, configuration, route, replica, Tor launch,
endorsement endpoint, signing/file adapter, bootstrap or deployment change.
Dual-address acceptance authorizes neither deployment nor live tests, managed
Tor launch, installation, real keys/credentials, gateway calls or inference.

The production client connector is **explicitly deferred**: unpatched strict
SOCKS authentication negotiation and cancellation during the handshake remain
unqualified blockers. The prior [client spike](tor-client-spike.md) is not
shipping connector evidence, and this decision does not authorize its patch or
silently weaken those constraints. Upstream Arti remains unmodified; its accepted
whole-gateway fail-fast behavior still does not establish graceful accounting
cleanup or sanitized upstream diagnostics.

## Implemented source seam

`src/server.rs` keeps all public TCP serving signatures unchanged. Its listener
wrapper uses the existing supported Futures `unfold` combinator, discards peer
metadata at acceptance and supplies `Stream<io::Result<TcpStream>>` to the
crate-visible `serve_incoming_until`. The core accepts Tokio
`AsyncRead + AsyncWrite + Unpin + Send + 'static` I/O. It is not a public
arbitrary-authorization interface.

There is still **one** router/AppState, 64-permit connection semaphore, task set,
shutdown watch and lifecycle fence/drain owner. Shutdown, then failed connection
tasks, retain their existing biased priority over incoming acceptance. An accept
error retains the closed `ServerFailure::Accept` mapping and stops admission;
raw transport errors and peer identifiers are not retained. A finite synthetic
source's `None` is orderly exhaustion: stop admission, drop the source, publish
graceful connection shutdown, join connections and wait for original lifecycle
owners. It is neither a busy loop nor a fabricated listener failure. TCP's
source normally never exhausts.

Only `DisconnectIo<IO>` is generalized. Its existing request-disconnect marker,
sole physical-I/O failure owner and closed observer mappings remain unchanged:
actual EOF/error cancels the original owner, while Pending/backpressure does not.
HTTP header/body/parser limits, connection reuse policy, authentication,
attestation, catalog validation, reservation, settlement and receipt/tool gates
are unchanged. No transport label, new telemetry, peer/IP/hostname retention,
retry or inference replay is introduced.

TCP and Tor acceptance must be multiplexed into **this one incoming source**,
never two independent `serve_until` loops with separate permit pools or drains.
The first packet kept production I/O generic and boxed only the mixed fixture.
The optional adapter below now boxes mixed accepted I/O at its own boundary;
the core stays generic. Arti internal queues and whole-process resource budgets
remain outside this accepted-stream seam.

## Future setup and replacement behavior (not implemented)

- Before credentials or inference, an explicitly authorized setup attempt may
  fail over between the two user-selected addresses. Each attempt must establish
  Tor's authenticated connection to the selected B and freshly verify the
  application/channel bindings, A's v3 evidence and same-challenge endorsement
  of that exact B. No automatic clearnet fallback or model substitution follows.
- Once a generation may have been accepted, do not replay it on another address.
  Interrupted delivery does not establish settlement, refund or safe retry.
  Reconcile only from authenticated evidence; a deliberate new submission may
  incur another charge. Setup success is not inference/tool authorization.
- Existing drainage is reused per replica. Accepted streams do **not migrate**
  across addresses or replacements; old original connection, preflight,
  generation and accounting owners must retire under the existing policy.
  A connected stalled reader can delay drain. An actual disconnect still cancels
  upstream work promptly; graceful shutdown is not a new cancellation authority.
- Prefer **ephemeral guard/service runtime state** for this v0 plan, rather than
  depending on Tinfoil persistent volumes whose single-enclave attachment can
  force replacement downtime. B must still be secret-backed and separately
  restored; ephemeral state does not mean an ephemeral address. Repeated fresh
  guard selection across restarts increases exposure to new guards and is an
  explicit planning tradeoff, **not an anonymity or unlinkability claim**. Guard/vanguard/replay-state loss and rollback/resource behavior require
  later qualification; this packet changes no storage policy or volume.
- Historical requirement (superseded by the operator's startup-self-appraisal
  removal): compare the private grant's derived public A against the approved
  public-key file and freshly authenticated same-ID v3 endorsed SPKI at startup.
  Current startup requires local derived/mounted public A equality only; fresh
  per-request appraised-A equality is still required before endorsement bytes. Mounted-grant authenticity, measured
  selection of B, local private B custody, live qualification of read-only
  key-manager identity checks and exclusive publication/restore remain future gates. Do not concurrently
  publish copied B in blue and green; a local state lock is not distributed
  ownership proof.

No supported shim/private-ingress or alternate transport trust path is selected
here. In particular, accepting arbitrary I/O is not permission to expose the
unprotected Rust listener as an authenticated onion API. Open egress, Linux
measured artifacts, client TLS/HPKE/app-key/socket binding, real Arti lifecycle,
resource headroom and upstream diagnostics remain unqualified. See
[Arti evidence](tor-arti-spike.md) and [identity gates](tor-identity-binding.md).

## Offline verification scope

`src/server/transport_tests.rs` is nested beside the existing private lifecycle
fixtures via `src/server.rs`, reusing their synthetic Auth, evidence verifier,
inference probe, pauses and accounting. It does not reimplement SDK parsing or
accounting. Its eight tests exercise the actual generic ingress with local Tokio
duplex I/O and one numeric-loopback mixed TCP fixture:

- Existing readiness/attestation/authenticated catalog/auth challenge routes,
  authentication rejection and retired browser-route fallback; header count and
  byte limits, initial header deadline, control-body limit and pipeline closure.
- Actual duplex Pending reads and full-buffer Pending writes do not cancel;
  actual EOF cancels. Actual Hyper write backpressure retains a connected
  generation through source exhaustion and completes/settles once after release.
- EOF during reserved tokenization cancels the original preflight and restores
  credit without generation/replay. After a connection and generation retire,
  finite-source drain still waits for the detached original preflight owner.
- 32 TCP plus 32 duplex connections share the same 64 permits; an extra connection
  from **each** source is refused. One shutdown fences both sources and waits
  for an originally accepted paused connection, rather than bypassing the bound
  with a second semaphore.
- Empty-source orderly drain, hostile accept-error closed mapping, and ready
  shutdown winning before source polling. Existing TCP injection/race tests
  continue to exercise the unchanged hooks.

Policy references: [data boundaries](../PRIVACY.md#data-handling-boundaries),
[forbidden telemetry](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[typed failures/sole ownership](../PRIVACY.md#mandatory-typed-interface-failures),
[aggregation integrity](../PRIVACY.md#aggregation-is-necessary-not-sufficient)
and [synthetic testing](../PRIVACY.md#synthetic-testing-exception). Tests inspect
existing synthetic connection-failure counters only; no exporter, diagnostic
file, request telemetry or identifying fields are added. Native host history is
a separate boundary, not whole-process transience.

Reproduce in the existing pinned root environment without downloads:

`nix develop --offline --command cargo test --offline --locked`

`nix develop --offline --command cargo clippy --offline --locked --all-targets --all-features -- -D warnings`

`nix develop --offline --command cargo fmt --package possums -- --check`

The included focused test file also needs its explicit formatting check:

`nix develop --offline --command rustfmt --edition 2021 --check src/server/transport_tests.rs`

`git diff --check`

Final local result: pinned **rustc 1.92.0 (ded5c06cf 2025-12-08)**,
Darwin/aarch64. Full offline locked Cargo tests pass: **327 library + 4 binary +
169 integration = 500 passed**, with the four existing funded/live tests still
ignored; doc tests contain no tests. All eight new transport tests pass both
focused and in the full suite. Strict all-target/all-feature root Clippy, root
package format, explicit focused-file Rustfmt and tracked/new-file whitespace
checks pass. Active changed-source LSP checks report no warnings/errors. The
existing vendored Tinfoil dead-code warning remains unchanged. The combined
check command's 120-second harness budget expired after the full tests, while
Clippy was running; a separate completed Clippy/format run passed with a larger
wait budget. This is not a project-check failure or new failure artifact.
Parent-reported Rust 1.92 root and 14 Arti spike passes precede this packet;
spike files/checks are not owned or rerun here. No live onion, real
DataStream/rendezvous, client connector, old shipped-client artifact, deployed
gateway/image or real billing/privacy
property is qualified by these fixtures. Default wire behavior is unchanged;
full tests do not substitute for future Tor contract compatibility acceptance.

## Optional shared Arti adapter — source-only qualification

`src/tor_ingress.rs` and `src/tor_ingress/tests.rs` add an **unwired**, default-off
`tor` feature. `Cargo.toml` uses the spike's exact released Arti **0.47.0** direct
versions/features, plus supported Tokio-util `compat`. Native KeyMgr fixtures
use the same optional fixture dependencies as the spike. The lockfile adds the
optional dependency graph without removing/replacing existing locked package
identities. No main/config/client/attestation/signing/server-core/spike file is
changed by this packet; the existing Rust 1.92 and endorsement changes are
preserved, not owned here. No installation, deployment, Tor bootstrap,
publication, real keys/private configuration, inference, commit or delegated
agent was used.

### Supported API and ownership

- The sole public serving function consumes an already host-owned
  `TorClient<PreferredRuntime>`, service configuration and approved `HsId`.
  It reads the existing KeyMgr private pair and public entry (which KeyMgr can
  derive from the pair), requires pair/public/approved equality **before**
  `launch_onion_service`, and never calls `get_or_generate` or provides an
  identity-generation fallback. Launch itself can trigger Arti bootstrap; the
  function has no source caller and is never invoked by checks. Exclusive host
  custody between read and launch is mandatory: Arti internally generates
  missing identities and this gate is not protection against concurrent
  keystore mutation, rollback or a second publisher.
- Real supported `RendRequest.accept` and `StreamRequest.accept` supply
  `DataStream`, converted with `FuturesAsyncReadCompatExt::compat`. Only `BEGIN`
  port 80 is admitted. Unsupported requests use the SDK's `reject` with `DONE`;
  no custom protocol, crypto, Hyper loop, local proxy or listener is introduced.
- A Futures `unfold` plus bounded `SelectAll` owns at most **64 total pending
  handshakes/circuit sources**, derived from the existing `MAX_CONNECTIONS`
  capacity. Overflow rendezvous requests use the SDK's supported rejection;
  Arti 0.47 rendezvous rejection is an immediately completing local operation.
  Circuit stream acceptance is serial per circuit, but never waits for HTTP
  handling. Other circuits proceed while one handshake or circuit stalls.
  Root EOF remains polled even at full capacity. Cooperative yields prevent
  continuously ready rejections/statuses from monopolizing source polling.
  There are no spawned handshake tasks, actors, adapter-owned unbounded queues
  or additional HTTP admission quotas. The only adapter signal is the bounded,
  operation-owned reporting-fault watch slot described below.
- Fair Futures `select` multiplexes TCP and onion I/O, then calls
  `serve_incoming_until` **once**. The existing router, 64 connection permits,
  connection tasks, shutdown priority, quiesce, Hyper drain and original
  generation/accounting owners are shared. The core borrows the incoming
  stream: circuit sources, the service and the TorClient stay owned until the
  entire shared drain returns. Source failure stops admission but does not
  abort accepted generation. The ten-second initial header deadline matches
  the reviewed TCP wrapper; Pending/backpressure remains distinct from EOF.

### Failure and privacy scope

Peer handshake/accept/reject errors and observable circuit polling panics are
consumed locally, drop that peer and call only a host-owned `Reporter` with the
closed `Failure` enum. No SDK exception text, panic payload, identifier, port,
address or other offending argument enters that callback or the returned error.
No log, telemetry hook, new metric, retry, inference or diagnostic work is added.
The host must keep this callback transient, with no automatic persistence/export;
native Pi/T3 history remains a separate persistence boundary. A private operation
owner contains host callback unwinds with `catch_unwind(AssertUnwindSafe(...))`
**only around the callback**, never around serving/drain. It neither inspects nor
retains the panic payload. One `watch<Option<Failure>>` slot latches only the
first failed report's original closed category via `send_if_modified`; subsequent
reports do not call the faulty callback or recursively report the fallback.

A callback fault wakes the **same core's shutdown future** and fences further
incoming acceptance, rather than dropping/aborting serving. Shared quiesce,
graceful Hyper drain and the original generation/settlement owners still finish.
`ServeFailure.reporting` preserves the original diagnostic plus the locally
authored `tor_reporter_unknown`, `stage=reporter`, `constraint=host_callback`,
`cause=unknown` fallback separately from source and shared-drain failures. A
failed peer diagnostic is a host-presentation failure, not a fabricated fatal
root-listener failure. External shutdown keeps biased priority; already observed
source/engine and shared failures are not replaced by reporting or late cleanup.
The public startup path uses this same owner: identity failure cannot launch a
service even if reporting panics, and launch failure and reporting failure are
returned separately. No receipt, refund or settlement is inferred from this
fallback; accepted-work billing remains unknown without authenticated receipts.

Containment is **after the process panic hook**: `main` already installs a
content-suppressing hook, but another host must suppress payload/stack output
itself. Tests use fixed content-free reporter panic payloads and do not mutate
the global hook. This is not whole-process panic-output sanitization or an Arti
shutdown guarantee.

Root rendezvous EOF/polling failure, TCP accept failure, terminal engine status
or status-source death stops shared admission visibly. `ServeFailure` retains
the source primary **and** the shared serving/drain result, rather than replacing
source failure with cleanup success or erasing drain-integrity failure. Reporting
before drain does not assert completed accounting, cancellation or refunds.
Billing remains unknown without authenticated receipts; a deliberate new
submission may incur another charge. A ready shared shutdown wins before
incoming polling and does not invent a source failure.

SDK limitations remain explicit: Arti's request streams expose no typed circuit
close cause, so ordinary peer EOF cannot be distinguished from an internally
swallowed circuit error. No fabricated fault is assigned to normal EOF.
Statuses can coalesce, and initial `Shutdown` precedes background startup;
initial shutdown alone is not declared death or health. Later shutdown, `Broken`,
reported runtime fatal problems and status EOF are observed; bootstrapping,
recovering or degraded reachability is not claimed healthy. An SDK failure that
neither terminates a source nor surfaces in supported status cannot be promised
observable. Upstream logging/panic hooks require host suppression; this adapter
cannot sanitize SDK-originated output emitted before its boundary. The accepted
unsupported-protocol whole-process fail-fast remains unpatched and can bypass
all application drain/diagnostic code. No guaranteed Arti shutdown, network
withdrawal time, memory erasure, resource headroom or invoice parity is claimed.

Applicable policy: [data handling](../PRIVACY.md#data-handling-boundaries),
[forbidden telemetry](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[typed failures/sole ownership](../PRIVACY.md#mandatory-typed-interface-failures)
and [synthetic fixtures](../PRIVACY.md#synthetic-testing-exception).

### Offline evidence and remaining gates

The original thirteen focused tests cover bounded idle circuits and overflow rejection/root
EOF, stalled-handshake fairness, closed hostile peer-error/panic projection and
continued acceptance, supported port checks, fair source mux/exhaustion and
actual adapter source-panic classification,
synthetic engine-terminal states, duplex plus the exact compatibility path,
one shared 32+32 connection pool with both extra streams refused, ready
shutdown, retained source ownership and source-primary plus drain-failure
preservation while an original lifecycle root remains alive. Native KeyMgr
missing/private-public-mismatch/approved-mismatch/reopen checks use publicly
specified deterministic synthetic seeds in permission-checked RAII scratch
directories. Four reporter regressions additionally exercise actual panicking
callbacks at source, peer and startup boundaries and first-fault/closed-fallback
preservation. Source and peer fixtures reuse current module AppState/lifecycle,
actual duplex/router/Hyper delivery and the existing `ReservedGeneration` terminal
owner with synthetic reservation/usage (not an inference call). New admission
stops; serving remains pending through original owner completion and held Hyper
delivery; settlement occurs once; retained source ownership and independent
shared-drain failure survive alongside the original source/reporting evidence.
The existing source-primary test without a reporter panic remains unchanged
apart from asserting absent reporting evidence. Ready-shutdown coverage also
checks an already-latched reporting fault without polling either source. A
private checked-launch seam uses the actual missing-KeyMgr-identity fixture and
asserts its launch closure never runs; no TorClient is constructed. These checks
launch no onion service.

Final pinned offline **Rust 1.92.0**, Darwin/aarch64 checks pass:

- Default full suite: **327 library + 4 binary + 169 integration = 500 passed**.
- `tor` full suite: **344 library + 4 binary + 169 integration = 517 passed**.
- Four funded/live tests remain ignored in each; all eight previously qualified
  transport checks pass in both. Focused adapter run: **17 passed**.
- Strict all-target default and all-feature Clippy, package format, explicit
  focused-file Rustfmt and whitespace checks pass. Active changed-source LSP
  probes report no diagnostics. The existing vendored Tinfoil dead-code warning
  is unchanged. Intermediate fixture compile/setup failures were corrected;
  the completed pinned acceptance commands passed.

Reproduce without downloads or live Tor work:

`nix develop --offline --command cargo test --offline --locked`

`nix develop --offline --command cargo test --offline --locked --features tor`

`nix develop --offline --command cargo test --offline --locked --features tor tor_ingress::tests`

`nix develop --offline --command cargo clippy --offline --locked --all-targets --no-default-features -- -D warnings`

`nix develop --offline --command cargo clippy --offline --locked --all-targets --all-features -- -D warnings`

`nix develop --offline --command cargo fmt --package possums -- --check`

`nix develop --offline --command rustfmt --edition 2021 --check src/tor_ingress.rs src/tor_ingress/tests.rs src/server/transport_tests.rs`

`git diff --check`

Real Arti rendezvous/DataStream/network lifecycle and SDK internal resource
budgets remain untested. Host bootstrap/key loading, protected B custody,
exclusive publication/restore, grant authenticity and A/public-file equality,
fresh same-ID v3 attestation/SPKI endorsement, endorsement routing, client
Tor/TLS/HPKE/channel binding, the complete live trust chain and Linux measured
artifacts remain separate future work. The shipped Pi artifact and strict SOCKS
connector have not been exercised against this adapter; unchanged default
wire fixtures do not qualify new Tor-client compatibility. No Tor activation,
second replica, Onionbalance/fork, deployment or live billing/privacy property
is established by source compilation and offline fixtures.

## Backend runtime continuation — offline source only

`src/tor_runtime.rs`, `src/main.rs` and the optional adapter now wire one runtime
branch, without activating it. The Cargo `tor` feature remains default-off.
`POSSUMS_TOR_ENABLED` accepts only exact `true` / `false`; missing means false.
A true switch without compiled support fails closed (`TorStartupFailure::FeatureUnavailable`,
root exit 10), rather than silently serving clearnet. Disabled code reads only
that switch: no Tor identity configuration, grants, directory creation,
appraisal or Arti construction. No deployment/config/environment files changed.

The enabled configuration selects one operator-protected **Arti-native** existing
keystore directory (`POSSUMS_TOR_IDENTITY_DIRECTORY`) and one expected v3 onion
address (`POSSUMS_TOR_EXPECTED_ONION`), parsed with supported `HsId`. Fixed
nickname: `possums`. This uses the expressly permitted native-keystore option,
not the alternative C Tor store: the earlier native fixtures already qualified
SDK encoding/decoding, so no C Tor wire-format fixture or adapter is necessary.
SDK key specifiers/types locate bounded existing files; supported `KeyMgr`
requires private/public/approved equality. Public lookup can derive the public
key; it does not prove a physical public file exists. No generation fallback or
write to the operator source is called. Protected source ownership between
metadata checks and SDK decoding, authentic secret mounts, exclusive publication
and rollback-safe restoration remain host constraints, not locks implemented here.

Fixed-path A's PKCS8 v1 PEM and SPKI PEM are decoded with maintained Rustls/Spki
and the existing Ring signer. Derived A and mounted public A must agree before
TorClient construction. No startup verifier/socket/nonce, stored appraisal or
freshness check remains in preparation, bootstrap or launch. This explicitly
replaces the former fresh-appraised-A-before-B startup ordering: Tor can publish
while the attestation endpoint is unavailable. Local consistency does not prove
grant authenticity; each `/attestation/tor` request still freshly appraises the
same client challenge, requires current appraised A to equal the held signer and
checks expiry before returning bytes. Independent client publisher/release,
nonce/A/B/TLS/HPKE/freshness checks are unchanged. Removing the startup dependency
may help initialization but does not resolve an underlying acquisition 503 or
establish Tor-client E2E acceptance.
The existing B pair is inserted once, overwrite false, into supported
`ArtiEphemeralKeystore`; missing/mismatched B fails closed. The prepared owner
retains this memory-only pair, so source removal cannot make B disappear before
its supported insertion into Arti's ephemeral primary. No runtime replacement
identity is generated. Unmodified Arti 0.47.0 / Rust 1.92 remain pinned.

A unique mode-0700 `TempDir` under `/tmp` contains cache/state; paths use SDK
literal-path configuration, not inherited cache directories. SDK permission
checks explicitly ignore disabling environment overrides. This owner and the
client Arc survive the entire original shared drain. `/tmp` is not asserted to
be a memory mount on the tested host; no persistent volume, read-only root
provisioning, enforced memory ceiling or 64-MiB headroom is qualified. Cache,
guard and service state reset each boot. The previously accepted repeated-guard
exposure tradeoff and no-anonymity claims remain unchanged.

Termination signals are registered before optional construction/bootstrap.
Biased cancellation prevents polling preflight/bootstrap after a ready signal;
the adapter checks a ready signal before publication. No service starts from a
preflight failure. The same router, 64 permits, task set, accounting/lifecycle
owners and telemetry cleanup remain in use. No generation retry, cancellation,
backpressure, receipt policy or reporter-containment owner was changed. The
production peer reporter intentionally has no logging/export sink; closed fatal
source/shared/reporting evidence remains in the serving failure. This does not
qualify delivery of every peer diagnostic. Native root exit 10/11 communicates
Tor startup/serve failure, not a complete rendered diagnostic or billing outcome.

For enabled processes `/healthz` requires both existing lifecycle readiness and
observed SDK `State::Running`, clearing on recovery/degradation/termination.
Arti describes Running as satisfactory introduction points and an up-to-date
descriptor, **believed** reachable, not independent reachability proof. TCP bind
or bootstrap completion alone is not green. Coalesced status, publisher behavior,
withdrawal time, replacement overlap and old-stream drainage remain live gaps.
Accepted unpatched SDK failfast can still terminate the whole gateway and bypass
receipts/drain or emit external raw diagnostics. No tracing subscriber was added.
The diagnostic/shutdown review follow-up below refines only the observed-ready
launch guard and new opt-in public failure wire.

### Continuation verification and privacy

Pinned offline Darwin/aarch64 Rust 1.92 checks pass: default **501** tests
(328 library + 4 binary + 169 integration); Tor **531** (358 + 4 + 169).
Four existing live/funded tests remain ignored. The original 17 containment/
shared-ingress regressions still pass; one additional SDK-status readiness
fixture passes. New runtime/RPC/router tests use public synthetic material only,
never construct a TorClient, and qualify maintained Go-v1 PEM/SPKI decoding,
key mismatch/non-Ed25519/malformed DER, read/size/disappearing files, missing/wrong
B, no overwrite/generation fallback, disabled no-work behavior, preflight refusal,
ready-signal no-poll behavior, same challenge/fixed-B layout, default 404,
expiry/current-A refusal and hostile RPC data suppression. Axum successes use
explicitly **synthetic SDK facts**, not a positive authenticated chain.

Strict default/all-feature all-target Clippy, package/focused-file formatting,
whitespace checks and active changed-source LSP diagnostics pass. Go tests,
race tests, vet, module verification and shared-core WASM tests pass with
`GOPROXY=off GOSUMDB=off GOTOOLCHAIN=local`. Scratch native/WASM build
`/tmp/possums-v3-build-SsJ3PA` passes the unchanged/new built bridge suites
(**40 + 31** checks) and public Rust/Node endorsement suite (**55**).
Intermediate fixture-permission/payload assertions and an Arc API mismatch were
corrected before these complete passes. Lockfile delta relative to the packet's
starting dirty tree is only the existing locked Spki dependency reference; no
package identity/version/checksum changed. Prior and concurrent unrelated diffs
were preserved. Final checkout inspection observed an externally initiated
`feat/tor-v0` → `main` switch; both refs and HEAD still resolve to `3086abb`.
The implementation packet did not switch branches or commit. The parent subsequently
restored `feat/tor-v0` with explicit operator approval; all edits remain uncommitted. No commits, installs, external network, real keys, inference,
Tor construction/bootstrap/publication or deployment were performed.

Policy: [data boundaries](../PRIVACY.md#data-handling-boundaries),
[forbidden telemetry](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[typed failures](../PRIVACY.md#mandatory-typed-interface-failures) and
[synthetic tests](../PRIVACY.md#synthetic-testing-exception).
The new endpoint reuses **only** the existing Attestation endpoint class and
existing closed evidence error projection; no metric/dimension/log, onion
identifier, nonce, grant, address, prompt or raw exception enters telemetry.
Helper stderr is suppressed and stdout/JSON are bounded. RPC negative tests
exercise the actual child owner, not just a formatter. Diagnostics do not launch
inference, retry, Tor bootstrap or other work. Host history remains separate.

Still **unqualified**: measured Linux artifact/image and resource budget,
actual mounted grant/boot rotation/provisioning, full positive native v3
hardware/publisher/A chain, real Tor lifecycle/publication/reachability, strict
client connector/application channel binding, replacement ownership/overlap,
upstream failure output and live accounting/privacy. Native v3 platform-signer
mismatch and shipped legacy-v2 freshness limitations are not resolved by these
synthetic source checks. Two stable B slots remain an operator-managed design,
not provisioned replicas; accepted streams cannot migrate or replay.

### Confirmed-review follow-up — scoped offline evidence

The checked-launch seam now checks an already-ready shutdown before identity
reads, then performs a second **biased** ready-shutdown poll immediately before
invoking the launch closure. A oneshot made ready inside the synchronous identity
callback prevents that closure from running. Invalid identity still never launches;
its primary category and any reporter failure survive. This does not guarantee
process exit or prevent an unavoidable signal arriving after the final guard.
Original accepted-work drainage/accounting and reporter containment are unchanged.
No test constructs a TorClient or launches/bootstrap/publishes a service.

Public-route grant mismatch now reports only the key-equality constraint and
**no endorsement or inference**, not the startup-only claim “no service launched.”
Startup `TorStartupFailure::GrantMismatch` retains that correct startup diagnostic.
The new opt-in route alone projects closed code/stage/constraint plus locally
authored message/action; malformed challenge and bounded encoding refusal also
use this projection. Exact helper codes, including
`possums_v3_keys_tor_endorsement_spki`, survive actual Axum JSON into the
[pure Node adapter](tor-client-setup.md#paired-closed-backend-diagnostics--source-only-follow-up).
These are unverified public diagnostics, never authentication, transport or
accounting evidence. Success stays exactly `{evidence, payload, signature}`;
shipped/default routes and helper/WASM contracts are unchanged.

The existing typed Go acquisition boundary retains only numeric observed HTTP
status in the **Tor-native** RPC wrapper. Rust accepts optional status only for
the exact acquisition/http_status tuple, with integer range 100..999; null,
strings, fractions, unknown fields/categories and status on other tuples fail
closed. The new Tor API/client preserve that status as acquisition metadata,
not necessarily the gateway HTTP status. They infer no root cause or retry from
it. Frontend messages/actions are local, discarding even recognized backend
message/action strings. Existing coarse InterfaceError observations are retained;
no new metric category, dimension, diagnostic-code export, log or telemetry hook
is added. Policy remains [data boundaries](../PRIVACY.md#data-handling-boundaries),
[forbidden telemetry](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[typed failures/sole ownership](../PRIVACY.md#mandatory-typed-interface-failures)
and [synthetic testing](../PRIVACY.md#synthetic-testing-exception).

Focused pinned offline checks pass: **19 ingress + 3 attestation + 5 router
Rust tests**, including the original containment/drain regressions; **193 Node
checks** using the existing production WASM scratch artifact; helper main-package
Go tests, race tests and vet with proxy/sumdb off and local toolchain. Router
fixtures exercise every existing closed helper category and numeric status via
actual generated JSON into Node, changed/expired A, useful challenge refusal and
response encoding bounds. RPC negatives include actual child stdout/status with
hostile stderr suppression. No positive hardware chain is fabricated. Package and
owned-file formatting, Node syntax and tracked/new-file whitespace checks pass.
Active path-scoped LSP probes report no errors/warnings across the 13 changed
paths; five auxiliary style/typo hints are not correctness findings and were not
used to expand this patch. One initial closure borrow compile error was corrected
by copying the nickname before moving the service configuration; completed
focused checks above pass.

Focused reproduction (existing pinned environment/artifact, no downloads):

`nix develop --offline --command cargo test --offline --locked --features tor --lib tor_ingress::tests`

`nix develop --offline --command cargo test --offline --locked --features tor --lib attestation::tor_tests`

`nix develop --offline --command cargo test --offline --locked --features tor --lib server::lifecycle_tests::tor_tests`

`nix develop --offline --command node tests/tor_setup.mjs /tmp/possums-v3-build-SsJ3PA`

Parent follow-up on the corrected source passed **501 default / 534 Tor-feature
tests**, with four existing live tests ignored in each, strict default/all-feature
all-target Clippy, package formatting, **193 Node checks**, helper Go tests/vet
and whitespace checks. The earlier combined default run exhausted its 240-second
harness budget without a failed assertion or established cause. Its transport
suite subsequently passed both serially and in parallel, and the completed full
default rerun passed; no integration-test change was made to hide that timeout.

The final coarse mapping correction classifies the closed acquisition failures
(including observed acquisition HTTP status) as `EvidenceUnavailable`, not
`EvidenceInvalid`. The paired router tests assert this distinction for every
acquisition category and statuses 100/503/999 while checking unchanged detailed
codes through the Node decoder. No metric family, dimension, cadence or export
policy was added. Independent final review confirmed the mapping and paired
coverage, with no remaining issue in that scoped correction.

These are source/offline results, not a reachable or authenticated onion-service
qualification. No commits, installation, runtime configuration, live Tor,
gateway inference probes or deployment occurred. Authorized development-agent
calls are separate from synthetic acceptance fixtures and native host history
remains a separate persistence boundary. Platform/positive-chain, connector,
application-channel binding, measured artifact and live billing/privacy gaps
above remain open.

### Final source integration and first-rollout admission

The completed final gateway review found no blocking source-code issue in the
shared 64-permit admission/drain owner, opaque public-shim bridge, then-required
fresh A-before-B startup ordering or measured shipping configuration. That
startup ordering was subsequently removed by explicit operator decision; this
review remains historical evidence for the earlier source. This supersedes an
interrupted review, not its missing acceptance. Source changes are now organized
into logical commits; no deployment or installed-host qualification is implied.

The v0.0.15 lockfile's exact `@tinfoilsh/verifier` 1.2.1 decoder was exercised
offline with synthetic TLS/HPKE report fields. Official CVM 0.14.12's legacy
`BodyV2.Marshal()` omits workload keys; the no-nonce endpoint therefore does not
add A to that decoder's input. Candidate and old decoder bytes matched. This is
only decoder/source evidence, not cryptographic acceptance or full old-Pi
startup, authentication, catalog or streaming qualification on candidate hardware.
Native CLI signer mismatch and legacy-v2 freshness limits remain unresolved.

The protected first-rollout path preserves all existing secret references and
permits only the two explicitly approved Tor additions. Either reference already
present blocks another update pending separately qualified exclusive publication
and host drain. Read-only planning and independent CLI-plan/readback checks do not
establish actual mounting or publication. Parent and independent review each
passed 75 offline release tests with flake-pinned Python 3.12.12; pinned Go tests
and vet also passed. See [release admission](approved-main-releases.md).

Measured Linux build/resources, mounted A/B qualification, stable B custody,
correct-replica public-shim routing, installed-host behavior and live Tor E2E are
still open. The operator reaffirmed deployment authorization; the exact-main,
independent-image, provenance and protected-approval gates remain mandatory.
