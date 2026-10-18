# Tor restoration on current main — source/PR only

## Authority and baseline

The operator explicitly requests an independent PR against `main`, not a stack
on the diagnostic branch. This source authorization supersedes the historical
Tor-removal restriction only for implementation, offline testing, small commits,
push and PR creation. It authorizes **no merge, release dispatch/publication,
deployment, client installation/activation, runtime/key-binding mutation,
inference, attestation or live Tor probe**. Degraded-state investigation is deferred.

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
`tinfoil-config.yml` and release workflow are byte-unchanged from the base:
open egress, 300-second grace, 512 MiB memory-only `/tmp`, existing image pin and
export settings remain. No signed debug image, 600-second grace, debug-only
logging, export activation or one-shot signing workflow override is imported.

## Restored and deliberately excluded

Restored: optional Arti 0.47 gateway runtime/opaque onion bridge, fixed-B custody,
nonce-bound `/attestation/tor`, official helper/WASM Tor appraisal, Pi explicit
selection and route-preserving renewal, native `tor-socksproto` transport,
packaging and paired offline tests. Existing pinned Rust 1.92 supports Arti;
SDK/dependency versions are not upgraded. The Nix shipping binary is Tor-capable;
Cargo's default features remain empty and runtime opt-in remains absent/false.

The measured deployment configuration remains HTTPS-only, deliberately not a
first-Tor activation recipe. A separately reviewed activation would need the
platform attested Ed25519 A grant/mount, approved B custody/reference delivery,
explicit runtime opt-in and exclusive publication/old-candidate cleanup evidence.
Neither this PR nor its unchanged deploy script adds or removes secret bindings.
Do not use historical/debug commands as activation authority. An already-running
shared identity is not evidence that another publisher is safe.

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
- Explicit Python 3.12 offline discovery: 97 tests, zero failures, one existing
  opt-in skip. Includes release/approval/target preservation, no activation,
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
