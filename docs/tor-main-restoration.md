# Tor restoration on current main — source/PR only

## Authority and baseline

The operator explicitly requests an independent PR against `main`, not a stack
on the diagnostic branch. This source authorization supersedes the historical
Tor-removal restriction only for implementation, offline testing, small commits,
push and PR creation. It authorizes **no merge, release dispatch/publication,
deployment, client installation/activation, runtime/key-binding mutation,
inference, attestation or live Tor probe**. The operator subsequently requests
that this PR turn Tor on in measured source configuration; that change does not
authorize live mutation. Degraded-state investigation is deferred.

Base: `04abc9efab5557705284779719c4701527631dae` (PR97 recreated gateway target),
confirmed against origin/main before implementation. The original diagnostic
worktree is untouched. Donors are inspected through Git objects:

- `ca7243b88a6b5b11a930ff743206589633a497d6`: historical integrated gateway,
  helper/Pi trust and native transport, before PR91 removal.
- `a248115`: ingress and bounded general startup code before PR90's post-launch
  diagnostic observer. The observer/FIFO/background machinery is not restored.
- `dcdcb77`: only the supported `is_fully_reachable() && !fatal` health predicate
  and corresponding regression intent. No debug branch merge.

Later main's verified upstream renewal, tokenizer diagnostics, NSS accounts,
startup ownership, API/accounting behavior, release/provenance/protected-approval
controls and `scripts/deploy.py`'s recreated `gateway` target are preserved.
The release workflow is byte-unchanged from the base. `tinfoil-config.yml` now
enables Tor and declares its attested A grant/mount and B secret references;
open egress, 300-second grace, 512 MiB memory-only `/tmp`, existing image pin and
export settings remain. No signed debug image, 600-second grace, debug-only
logging, export activation or one-shot signing workflow override is imported.

## Restored and deliberately excluded

Restored: optional Arti 0.47 gateway runtime/opaque onion bridge, fixed-B custody,
nonce-bound `/attestation/tor`, official helper/WASM Tor appraisal, Pi explicit
selection and route-preserving renewal, native `tor-socksproto` transport,
packaging and paired offline tests. Existing pinned Rust 1.92 supports Arti;
SDK/dependency versions are not upgraded. The Nix shipping binary is Tor-capable;
Cargo's default features remain empty. The measured configuration explicitly
sets `POSSUMS_TOR_ENABLED=true`, declares the `possums-tor-endorsement-v1`
Ed25519 A key owned by UID/GID 65532, grants it to the gateway, and references
`POSSUMS_TOR_IDENTITY_NATIVE_KEY` and `POSSUMS_TOR_EXPECTED_ONION` for approved B.
Missing required inputs fail closed; this source change is not proof that the
platform has provisioned them.

Approved B custody/reference delivery remains required. The operator now accepts
single-address blue/green overlap, deferring exclusive publication and seamless
handoff. The deploy wrapper preserves existing secret/reference selections and
normal no-drift/provenance/approval gates; it does not rotate identities or
provision missing bindings. It provides neither first-binding nor exclusive-host
handover automation. Neither this PR nor its tests mutate live bindings or the
release image pin. Do not use historical/debug commands as activation authority.
An already-running shared identity is not evidence of a seamless handover.

Alternative curl/Undici/SOCKS transport candidates, historical spikes, private-key
probe scripts, PR90 post-launch readiness export and debug-only ongoing diagnostics
are excluded. Existing developer tools do not auto-launch a Tor client. The
native Pi client requires an independently owned SOCKS listener; selection/status
never launch one. No new retry/recovery policy or diagnostic framework is added.

## Trust, ownership and privacy

Preparation checks bounded mounted PKCS8/SPKI A consistency and approved B
private/public consistency, strict ancestry and no-overwrite ephemeral import.
It performs **no startup self-appraisal/freshness gate**. Publication can precede
successful appraisal; local consistency is not authenticity. Each endorsement
request independently acquires fresh caller-nonce-bound official evidence,
compares current appraised A to the held signer and rechecks expiry before bytes.
The client independently authenticates publisher/release/manifest/A/B, nonce,
freshness, TLS peer possession/hostname/date before private HTTP, and HPKE binding.
Selected routes remain locked during renewal, without direct fallback or replay.

Shared connection admission, two physical permits per paired onion operation,
original accepted-work drain, backpressure, disconnect cancellation, quoted
reservation, cumulative/final usage and once-only settlement are retained.
Partial settlement never authorizes tools; interrupted delivery without a receipt
leaves billing unknown and a deliberate new request may incur another charge.

Applicable policy: [data handling](../PRIVACY.md#data-handling-boundaries),
[forbidden telemetry](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[typed failures](../PRIVACY.md#mandatory-typed-interface-failures),
[startup exception](../PRIVACY.md#startup-diagnostics-exception), and
[synthetic tests](../PRIVACY.md#synthetic-testing-exception).
Only closed preparation/bootstrap/local-launch initialization stages rejoin the
bounded startup diagnostic owner (at most nine records; six without Tor). No
post-launch **diagnostic-export** observer, SDK logger, peer/request diagnostics,
new destination or background export is restored. The existing owned Arti
status stream remains necessary for engine-failure handling and the separately
tested health readiness bit; it emits no post-launch diagnostic records. Its
fully-reachable health assessment can still flap, and does not trigger inference,
appraisal, retries, export or recovery. Existing generic startup/off/outage/hostile-data tests remain.
Native Pi/T3 history and platform capture are separate persistence boundaries.
No private originals, credentials or runtime configuration were read by this work.

## New offline verification

Pinned `nix develop --offline`: Rust/Cargo 1.92.0, Go 1.27.2, root Node 24.20.0.
Client acceptance uses its separate required Node 24.13.0/Pi 1.0.4 pins.
Python is explicitly locked nixpkgs `python312` 3.12.12, not ambient shell Python.

- Cargo `--offline --locked --all-targets --all-features`: **592 total passed**; library 412 passed,
  one existing operator-only test ignored; binary 6 passed; integration/example
  results passed, four existing live/funded probes ignored. No live tests run.
- Cargo `--offline --locked --all-targets --no-default-features`: **531 total passed**; library 351
  and binary 6 passed; all integration/example results passed, four live ignored.
- Package formatting and strict all-target/all-feature Clippy passed; existing
  vendored Tinfoil unused-method warning remains. Nine changed Rust paths passed
  active LSP with no errors/inconclusive outcomes. Whitespace check passed.
- Initial capability-only candidate: explicit Python 3.12 offline discovery
  passed 97 tests, zero failures, one existing opt-in skip. Includes
  release/approval/target preservation, no live activation,
  image-health 300-second controls and key-free Tor-image projections. Separate
  opt-in actual release-cache graph checks passed all 13 tests. Pinned Nix
  `checks.aarch64-darwin.gateway-accounts` also passed.
- Parent independently reran the complete pinned scratch client runner after
  comparing 28 source/dependency-manifest files with the checkout: passed,
  including 84 actual-native/local-TLS Tor channel checks, real packaged-WASM
  identity 31/setup 193, direct-v3 36, retained legacy-v2 179, actual Pi SDK/RPC
  164 and discovery 31. These use synthetic trust, not hardware-positive evidence.
- Parent reran Go race tests, vet and module verification with `GOPROXY=off`,
  `GOSUMDB=off`, `GOTOOLCHAIN=local`: passed. Parent reran the actual native-helper
  local SOCKS/TLS suite: 87 passed. Worker additionally rebuilt verifier/native
  artifacts offline, verified packaging hashes, passed general WASM 40,
  Rust-sign/Node-verify endorsement 55 and maintained-Arti onion vectors 19.
- Scratch dependencies were integrity/byte-checked against cached lock tarballs,
  not downloaded. The worker reconstructed a scratch Pi 1.0.4 runtime and pinned
  four internal peers explicitly to cached 1.0.4 tarballs after an older scratch
  lock resolved newer peers. This qualifies that disclosed synthetic host, not
  a shipped dependency tree; original installed/runtime directories are unchanged.
- `nix flake check --offline --no-build` passed. Full offline flake construction
  was interrupted by the tool's 200-second deadline during dependency compilation;
  it is **not a passing flake-build claim**, nor evidence the pinned compiler
  failed. Executed Cargo suites/Clippy above are separate completed evidence.
- Independent review found two client gaps: non-2xx setup bodies bypassed the
  closed diagnostic decoder, and the normal scratch runner omitted the real Tor
  WASM ABI suite. Both were fixed and regression-tested; second review confirmed
  those fixes. A later health-observer objection arose from ambiguous wording:
  excluding post-launch diagnostic export does not remove the owned engine/health
  status stream. Final strong independent review refuted that objection using
  pre-PR90 source and historical observer documentation, and found no remaining
  blocker for source/PR scope. It independently reran 67 Tor-focused Rust tests
  (one operator-only ignored), Go race, native/local-TLS channel 84, packaged-WASM
  identity 31/setup 193 and image projection 7. Parent then repeated complete
  final-source Cargo suites/format/Clippy and Python discovery successfully.

New health tests carry actual SDK states into the real health router:
Running/DegradedReachable are reachable; Bootstrapping/DegradedUnreachable/
Recovering are not. Fatal, EOF and panic clear readiness; lifecycle quiescence
still fails health. This is offline SDK-assessment evidence, not Tor connectivity.
Local TLS/HTTP/appraisal fixtures do not prove hardware trust or live settlement.
New-head Linux rootfs/NSS/probe execution and hosted CI remain unqualified locally;
restored CI performs only key-free network-denied fixture construction.

## Subsequent Tor-on PR amendment

At the operator's request, the candidate now enables Tor in measured source
configuration instead of merely shipping dormant capability. The exact A grant,
gateway mount, B references and unchanged 300-second/512-MiB controls have offline
regressions. The deploy guard rejects either existing B reference before planning
or mutation, preserving the separate exclusive-publication requirement.

Fresh pinned Python 3.12.12 discovery: **98 tests, zero failures, one existing
opt-in skip**. All four changed Python/YAML paths passed active LSP diagnostics;
`git diff --check` passed. Independent read-only Sol review found no source-only
blocker and independently reproduced the 98-test result. Rust/client source is
unchanged by this amendment; their earlier completed evidence remains scoped
above. No live action or new runtime qualification occurred.

## Main integration before requested merge/deployment

The operator subsequently requested merge and deployment. That authorizes the
normal gated flow, not bypassing secret-delivery, custody or exclusive-publication
qualification. No merge into main or deployment has occurred at this checkpoint.

Integrated main `16ce000` (PR98): retain its verified TLS peer-key pre-dispatch
recovery and obsolete-context guards together with Tor route isolation. The
pinned peer branch keeps the new mismatch category; public Tor acquisition still
requires native WebPKI. Build/check wiring retains both recovery and Tor suites.

Fresh Node 24.13.0/Pi 1.0.4 scratch TypeScript/build and full normal runner passed,
including v3 47, Tor channel 84, packaged-WASM identity 31/setup 193, actual Pi
SDK/RPC 165, peer-key recovery 18 and discovery 31. The existing integrity-checked
scratch dependencies/runtime described above were reused; no installation or
public-network test occurred. Pinned Python discovery again passed 98 tests
with one existing opt-in skip. Shell syntax and whitespace checks passed.
Independent read-only Sol review compared both parents and cleared integration;
it did not rerun tests. Checkout LSP reports missing Node/Pi dependency types;
the actual pinned scratch compiler passed, rather than changing source to silence
an unprovisioned editor workspace.

Deployment remains blocked on demonstrated required secret delivery and exclusive
host publication, including old-candidate cleanup, and a qualified activation
procedure. The existing wrapper neither provisions the new references nor
permits automatic updates of already Tor-bound instances. No gate was relaxed.

## Accepted single-address rollout — supersedes the overlap blocker

After discussing directory descriptor replacement, the operator explicitly asks
to proceed with one stable address and fix handoff later. Same-key publication
is not intrinsically impossible: higher descriptor revision counters can replace
older descriptors, but concurrent publishers, propagation and client caching do
not implement exclusive ownership or a deterministic release cutover. Existing
connections do not migrate. Stopping an old publisher can disrupt clients still
using its descriptor. Shim routing and TLS/HPKE context mismatch can fail trust
checks; no client may weaken them or replay uncertain inference to hide this.

The accepted rollout permits temporary same-B publication during normal
blue/green replacement. Alternating onion identities, exclusive-host proof and
seamless handoff are deferred, not prerequisites for this expressly accepted
mode. This supersedes earlier blocking statements in the two amendment records
above. It does not authorize key rotation, arbitrary binding changes, debug
export, client installation or paid inference. Health flapping remains unresolved.

Remove only the overlap-specific deploy refusal. Preserve exact-main CI,
reproducible image/provenance, protected approval, one update without replay,
read-only plan validation and unchanged opaque secret/reference readback.
Existing startup checks still require mounted A private/public consistency and
approved B identity consistency; requests/clients independently verify fresh
nonce-bound release/A/B/TLS/HPKE evidence. The normal protected serving check is
not a Tor-positive connectivity or exclusive-publication test.

Read-only control-plane inventory returned running/no pending update, debug off,
confidential mode on, and both required Tor secret references already selected.
Only closed booleans were projected; no key values, identifiers or raw output
were displayed or retained. This demonstrates reference selection, not secret
contents, runtime consistency, current onion availability or custody provenance.
No new binding delivery procedure is needed if the unchanged-reference plan and
runtime checks accept the existing operator-approved B selections.

Fresh pinned Python 3.12.12 discovery passed **99 tests, zero failures, one
existing opt-in skip**. Regressions permit unchanged existing Tor references,
retain one update without secret-selection/downtime overrides, and reject changed
reference readback. Active LSP on both changed Python paths and whitespace checks
passed. Independent read-only Sol review found no blocker and independently
reproduced the 99-test result. Runtime/client code and measured configuration
are unchanged by this policy amendment; their earlier evidence remains separately
scoped.

## Compatibility and remaining limits

Default HTTPS/API/accounting and default native/WASM helper schemas are retained;
Tor is an explicit separate route/action. Retained legacy fixtures are not a full
shipped-client artifact qualification. Historical v0.0.11–v0.0.15 strict stream
decoders already reject the current stream grouping/receipt fixture; restoration
does not repair that pre-existing gap. No older installed Tor package, installed
T3 presentation, new client activation or matching deployed gateway is qualified.
Native-v3 platform-signer and historical legacy-v2 freshness limitations remain.

The diagnostic branch's last reported debug instance was current/running with no
pending update, but healthy/unhealthy flapping occurred before and after its
600-second grace even with the fully-reachable predicate. A first healthy check
ends Docker's startup exemption. Descriptor-upload timeout/circuit categories
were observed, not a proven root cause or full per-directory progress inventory.
These are supplied historical observations, not new probes or claims about this PR.

The parent also reported one authorized isolated Tor-client SOCKS remote-onion
HTTPS HEAD `/healthz` test: TLS/hostname validation enabled, no redirects, HTTP204,
curl0. This demonstrates onion availability **then**, not sustained availability,
fresh device attestation or exclusive physical-instance attribution under shared
identity. This thread did not repeat it. Exclusive publication, old physical
candidate cleanup, current fresh attestation, Linux runtime/resources, upstream
billing/invoice parity and broader runtime/privacy properties remain unproved.
Tor is not claimed solved or production-ready by this PR.

## Failed v0.0.61 rollout and ten-minute grace amendment

PR #99 merged normally; exact-main CI and independent reproducible publication
passed for v0.0.61. One protected approval admitted one update. Deployment run
39196698284 ended with `RELEASE_COMPLETION_TIMEOUT`; subsequent control-plane
readback independently reported v0.0.60 current/running and v0.0.61's candidate
failed. No completion receipt or serving verification was observed. The release
configured `start_period: 300s`, not the historical diagnostic build's `600s`;
the wrapper also had a separate 300-second completion observation window.
Neither deadline establishes why the platform candidate failed.

Read-only Honeycomb investigation followed
[the startup-only exception](../PRIVACY.md#startup-diagnostics-exception), using
bounded closed-field aggregate queries, not raw rows or bodies. In the rollout
window 2026-10-19 03:30–04:11 UTC, `possums.startup` had 18 records: two entered
records for each of eight stages and two `initialization_complete` records at
`tor_launch`, constrained to `local_initialization_only_not_verified_serving`.
There were no failed/interrupted records in that classified result. These events
are not release/instance-attributed and do not establish verified serving,
later health, or the candidate failure cause. No debug export was activated;
no raw logs, private configuration or key material were retained.

The operator requests a fresh normal-gated attempt with 600-second measured
startup grace. The wrapper's fixed observation budget becomes 900 seconds so
it can observe completion after that grace; the budget is not renewed on each
readback and adds no update replay. Existing failed-candidate, drift, provenance
and protected-approval gates remain. A first healthy check still ends Docker's
startup exemption; this amendment is not evidence that longer grace fixes Tor.

Separately authorized cancellation sent exactly one supported `container cancel`
without `--rollback-latest`. Readback confirmed no pending candidate, unchanged
current v0.0.60/running, debug off/confidential mode on, and equality of all
existing opaque configuration fields. This proves control-plane reconciliation,
not physical cleanup or serving/onion health. There was no rollback, binding
change, client activation or inference probe.

Pinned Python 3.12.12 discovery passed **100 tests, zero failures, one existing
opt-in skip**. Fixtures assert 600-second measured grace and release-transform
preservation; synthetic monotonic time observes completion after 601 seconds
with only one update, while the fixed 900-second timeout and failed-candidate
paths still stop without replay. Active LSP found no diagnostics on the four
changed Python paths; whitespace checks passed. Independent read-only Sol review
found no blocker and reproduced the test run. Hosted CI/release/deployment evidence
for this new amendment is pending; v0.0.61 remains a failed rollout, not a deployed
fix.
