# Tor v0: endorsed signing-key and onion signature primitives

**Current continuation:** [backend source integration](#backend-source-integration--default-off) implements the earlier runtime seams. The operator subsequently **removed startup self-appraisal completely**, including cached freshness checks through bootstrap/launch; see [the new publication boundary](tor-startup-removal-verification.md). Local grant consistency is not appraisal. Fresh per-request and independent client verification remain required. Prior packet descriptions and acceptance remain historical; no deployed or positive live trust-chain claim follows.

## Scope and status

Source-only, offline packet on `feat/tor-v0`, based on merged `3086abb` (PR78).
The opt-in projection exposes an attested application public signing key. The
next source packet adds the offline canonical onion endorsement signer and
client signature/appraisal wrapper described below. It implements **no Tor
server, transport, endpoint, wire response schema or private-key
grant/configuration**. No installation, deployment, live evidence acquisition,
Tor, gateway or inference call was performed. The separately owned Tor control spike
is not production qualification.

The existing `Verify`, `VerifyClient`, default WASM response and native helper
response remain unchanged. Default clients do not receive additional result
fields or require this application key. The Pi bootstrap/v3 transport is not
changed or switched to this action.

## Opt-in projection

- Go: `verifier.VerifyTorClient(evidence, nonce, repository, manifest)`.
- WASM JSON: `action: "tor_endorsement_key_v1"`, with the existing evidence,
  nonce, repository and **required** manifest. Only this action returns
  `tor_result` rather than `result`.
- Node bridge: `verifyTorV3(evidence, nonce, manifest, repository?, signal?)`.
  The bridge still requires HPKE, uses a disposable offline worker and rechecks
  expiry after worker cleanup.
- Opt-in facts retain all existing projected facts plus
  `tor_endorsement_key`: exactly 32 raw Ed25519 public-key bytes as 64 lowercase
  hexadecimal characters. This is key **A**, not onion identity **B** and not
  connection/inference/tool authorization.

The single official production `NewVerifier().VerifyV3` path still authenticates
hardware, nonce-bound endorsed material, workload, publisher and freshness.
The existing Possums `qualifyPublisher` gates still require exact workflow,
release/commit authority, artifact subject and complete manifest/predicate
matching before projection. No caller-selected key ID, verified flag,
conformance trust root, replacement clock or second hardware verifier exists.

The locally fixed ID is `possums-tor-endorsement-v1`; the format is the pinned
SDK's actual `document.KeySPKIV1Format` (`https://tinfoil.sh/key/spki/v1`). The
projector requires exactly one matching ID, then calls the supported
`Verification.CryptoMaterialData(id, format)` and Go's
`x509.ParsePKIXPublicKey`, requiring `ed25519.PublicKey`. The SDK parser already
rejects duplicate IDs; an explicit projection check is necessary because the
public accessor itself selects the first match when handed a synthetic record.
The existing result/expiry/TLS/HPKE gates run first and return no partial facts.

Closed additional constraints are `keys/tor_endorsement_entry` (missing, wrong
ID or duplicate), `keys/tor_endorsement_format`, `keys/tor_endorsement_spki`
(including invalid/noncanonical hex or malformed DER), and
`keys/tor_endorsement_algorithm`. Their codes use the existing `possums_v3_`
prefix. Bridge messages remain locally authored, actionable and content-free;
raw SDK/X509 causes never escape. A primary failure survives cleanup failure.

## Canonical endorsement primitive, pending runtime integration

Use a fixed binary payload, not signed JSON, address strings or arbitrary
caller-selected signing targets:

1. The exact ASCII domain `possums/tor-onion-endorsement` followed by
   one NUL byte: 30 bytes total, including NUL.
2. One version byte `0x01`.
3. Exactly 32 bytes of independently random client challenge.
4. Exactly 32 bytes of the approved onion public identity **B**.

Total payload: **95 bytes**. Version 1 fixes A's signing-key ID to
`possums-tor-endorsement-v1`; no variable key-ID field is necessary. Use one
raw Ed25519 signature of exactly **64 bytes**, with no signature list, extensions
or trailing bytes. A future response encoding must have exact decoded lengths
and bounded parsing. The source primitive signs/verifies these raw bytes only;
no endpoint or response encoding is introduced.

The **same challenge bytes** must bind the SDK attestation and endorsement in
one setup attempt. The source client wrapper verifies the signature using the
freshly appraised A, checks domain/version/exact lengths and challenge equality,
and refuses to return identity facts at or after authenticated expiry, including
expiry during asynchronous crypto. Future transport must also enforce any earlier
existing client deadline and recheck freshness before new authorization. No
challenge substitution, cached verdict authorization or inference retry is implied.

Compare the decoded B to the identity of the **user-selected onion address**.
Use a supported Tor library's v3 onion address parsing/encoding/checksum and
identity accessors; do not duplicate Arti's address algorithms or SDK hardware
parsing. A valid signature for a different onion is a rejection. Possession of
A alone is not a connection to B; Tor's authenticated connection to that
selected identity and existing application-channel bindings remain necessary.

### Source interfaces and closed failures

- Rust `TorEndorsementSigner::from_pkcs8(grant, local_onion_identity: [u8; 32])`
  takes **DER, not PEM** and uses pinned supported `ring` **0.17.14**
  `Ed25519KeyPair::from_pkcs8_maybe_unchecked`. It accepts PKCS8 **v1 and v2**:
  v1 derives the public key from the seed; v2 still requires consistent
  public/private components. This is not authenticity verification of a grant.
  Malformed/truncated/trailing DER, wrong algorithms and raw PEM fail closed.
  A future file adapter must use a maintained PEM parser, not custom ASN.1 or
  DER conversion; the documented mounted file is not this core's byte input.
  The immutable owner stores B by value; `endorse(&[u8; 32])` accepts **only the
  challenge** and returns fixed `[u8; 95]` payload and `[u8; 64]` signature.
  There is no arbitrary message/target signer, runtime key generation, private
  key export, Debug/serde on the owner, persistence or key-bearing diagnostics.
  Future startup must supply A from the attested Tinfoil grant and B from Arti's
  approved locally owned identity. Neither provisioning nor custody is proven.
- `clients/pi/tor-endorsement.mjs` exports
  `authorizeTorEndorsement(evidence, challenge, manifest, selectedOnionIdentity,
  payload, signature, repository?, signal?)`. It obtains A **only** through
  existing `verifyTorV3(evidence, challenge, manifest, repository, signal)`.
  Request byte snapshots prevent caller mutation from substituting a different
  challenge/target during appraisal. Supported `node:crypto` JWK public-key
  import and asynchronous Ed25519 verification are used, not handwritten ASN.1
  or curve operations. Abort and authenticated expiry are rechecked after
  crypto before returning frozen scalar identity/freshness/channel facts.
  These include `onion_identity` (hex) and **`transport_authorized: false`**.
  No caller clock, trusted facts, verified flag or fabricated-A authorization
  parameter exists. No request is sent and no generation/settlement is implied.
- `checkTorEndorsementSignature(A, challenge, B, payload, signature)` is plainly
  a low-level **compatibility check only**, including for synthetic/fabricated
  keys. It returns frozen `{ signature_valid: true, transport_authorized:
  false }`, not appraised identity or freshness facts. It cannot authorize a
  transport. Its positive RFC-vector tests are not hardware acceptance.

**B must already be raw32 from a future supported onion address parser.** This
packet does **not** validate hostname syntax, onion checksum, local private-key
custody or connection ownership. Before sending secrets, future transport must
establish Tor's authenticated connection to the **same user-selected address**,
check these identity/freshness facts and preserve existing channel bindings.
Neither exported function installs/switches any Pi transport or clearnet API.

Rust's sole expected failure is typed `TorEndorsementError::Pkcs8`, with code
`possums_tor_endorsement_initialization_pkcs8`; its stage/constraint and action
instruct startup to supply approved Ed25519 PKCS8 v1/v2 DER, not PEM, with
consistent components when present. No endorsement
was produced. Ring signing itself is infallible; process loss/panic is not a
classified result or proof of key erasure.

Client `TorEndorsementFailure` codes use `possums_tor_endorsement_` plus these
closed stage/constraint pairs: `input/{challenge,onion_identity,payload,signature,
application_key,cause_unknown}`, `payload/{domain,version,challenge,onion_identity}`,
`signature/{rejected,cause_unknown}`, `verification/cause_unknown`,
`freshness/{expired,cause_unknown}` and `authorization/{interrupted,cause_unknown}`.
Diagnostics are locally authored, content-free and actionable: correct exact raw lengths,
obtain the same challenge/selected-B endorsement, do not send secrets on failed
signature/appraisal, and acquire fresh evidence after expiry. Unexpected faults
state that the underlying cause is unknown; raw crypto exceptions/stacks never
escape. Existing `VerifierFailure` codes and a known primary failure are retained
(including verification failure plus worker cleanup fault, or signature rejection
plus abort). No diagnostics log, persist, retry or initiate inference.

### Key custody: measured A endorses only approved local B

A rotates per gateway boot; B is the stable, persistent onion identity.
Measured code must sign only its **locally held, configured, approved B**, never
a caller-supplied onion key/address/target. The client supplies a challenge,
not signing authority over arbitrary B. Qualification must establish that B's
private identity key is actually held locally by the intended Tor service and
that approved measured configuration binds its selection; signing public B
alone does not prove custody or exclude relay/substitution. The source primitive
does not implement provisioning, storage, approval or these custody gates.

V3's endorsed SPKI mechanism supports A's projection, and the pinned generator's
encoding is source-qualified below. An actual mounted application signing key,
its per-boot rotation and signing capability remain **runtime-unqualified**.
The historical startup requirement was to compare the private grant's derived
raw Ed25519 public key against the approved public SPKI and the same-ID freshly
authenticated endorsed SPKI before serving endorsements. The later operator
startup change supersedes that self-appraisal requirement: startup checks only
the mounted private/public pair; each endorsement request still requires fresh
appraised-A equality before returning endorsement bytes. This is especially
necessary for v1, which contains no public component to check. Such equality
alone does not authenticate the mount, approved measured configuration, container
grant ownership or local B custody; those integration gates remain unimplemented.
Do not advertise a usable attested onion service on this basis.

### Pinned application-grant encoding: source qualification

Public source only, retrieved into `/tmp/possums-tor-grant-source-sQvYye`;
no mounted files, actual private keys or live evidence were read. Official
`tinfoilsh/cvmimage` **v0.14.12** resolves from annotated tag object
`6899aaf07631b7e50a28337f4276efcc1a1ab919` to commit
`35d94c85ad20e9f98cd4ddcf35dfe4574b5215fb` (the repository's `cvm-version`).
References below are pinned to that commit, not current main:

- [`tinfoil/internal/attestedkeys/store.go` lines 94–124](https://github.com/tinfoilsh/cvmimage/blob/35d94c85ad20e9f98cd4ddcf35dfe4574b5215fb/tinfoil/internal/attestedkeys/store.go#L94-L124):
  `ed25519.GenerateKey(rand.Reader)`, then `x509.MarshalPKCS8PrivateKey(private)`
  and `x509.MarshalPKIXPublicKey(public)`. Lines 46–57 write **unencrypted PEM**
  blocks `PRIVATE KEY` / `PUBLIC KEY` to `private_key.pem` / `public_key.pem`
  with modes `0600` / `0644`; lines 70–91 read the public PEM into endorsed SPKI.
  SHA-256: `b1732eea07381cb99d2270f0be07f9f0107cb01c9bef151d85596646a98de5e3`.
- [`nix/go.nix` lines 11–19](https://github.com/tinfoilsh/cvmimage/blob/35d94c85ad20e9f98cd4ddcf35dfe4574b5215fb/nix/go.nix#L11-L19)
  selects Nixpkgs `go_1_26`. SHA-256:
  `ef0c1ace7175029a0fa3f8a7ad792c3ed112914d480badfcd7cb3cce414a0dcd`.
  [`nixpkgs.lock.json`](https://github.com/tinfoilsh/cvmimage/blob/35d94c85ad20e9f98cd4ddcf35dfe4574b5215fb/nixpkgs.lock.json)
  pins Nixpkgs `597283ad8aa0b331c788e97c4c262d58877074ef`; file SHA-256:
  `295876a791a8519738e5abc3b03340de831134a4b95be34d22720d2796d62ad4`.
  Its [`pkgs/development/compilers/go/1.26.nix` lines 28–33](https://github.com/NixOS/nixpkgs/blob/597283ad8aa0b331c788e97c4c262d58877074ef/pkgs/development/compilers/go/1.26.nix#L28-L33)
  selects **Go 1.26.5**, source archive hash
  `sha256-SVvkvIcXasVnOS5bQRar2YRm0z17SdQedkzMaXay3EI=`; file SHA-256:
  `784dc3ed952c3497706ff7d4da342a64a421f99193bff250ab34ca7da80c7ed0`.
- [Go **go1.26.5**, `src/crypto/x509/pkcs8.go` lines 21–26, 106–107, 140–148](https://github.com/golang/go/blob/go1.26.5/src/crypto/x509/pkcs8.go#L21-L26):
  the private structure's zero-valued version is **0 (PKCS8 v1)** and has no
  public component. Ed25519 marshals the seed using the standard encoder.
  SHA-256: `6b62c951d4fc9ce980f3f47f58c5408c265d56e82f5ded3fa9e61911baf64b20`.
- [`tinfoil/cmd/boot/main.go` lines 105–109](https://github.com/tinfoilsh/cvmimage/blob/35d94c85ad20e9f98cd4ddcf35dfe4574b5215fb/tinfoil/cmd/boot/main.go#L105-L109)
  invokes generation before CPU attestation; SHA-256:
  `96b90389630d3bc781f10f2b8c62596cac896d769682675a40f4d604528c2ef1`.
  [`tinfoil/internal/containers/containers.go` lines 502–504](https://github.com/tinfoilsh/cvmimage/blob/35d94c85ad20e9f98cd4ddcf35dfe4574b5215fb/tinfoil/internal/containers/containers.go#L502-L504)
  binds the granted directory read-only; SHA-256:
  `6733879a4a1ac994e4347369c4c9aca09aa596a1b6545e0a2e94ec0d961af046`.

[Official attested-key documentation](https://docs.tinfoil.sh/containers/attested-keys.md)
independently describes `/run/tinfoil/keys/<id>/`, unencrypted PKCS8 PEM and
SPKI PEM; it does **not** specify the PKCS8 version. Source establishes v1 here.
No config, grants, file loader or runtime key generation is added by this packet.
Ring 0.17.14's supported
[`src/ec/curve25519/ed25519/signing.rs` lines 90–132](https://docs.rs/crate/ring/0.17.14/source/src/ec/curve25519/ed25519/signing.rs)
explicitly implements v1/v2
parsing and v2 seed/public consistency (file SHA-256
`5f5504a259544e214baff8501ffaec3bb3c84d8ca50a9b56f61c86432860ab9b`).
The `maybe_unchecked` name refers to v1's missing public component, not relaxed
v2 checks. No project ASN.1 parser, conversion or dependency was added.

## Offline verification record

Pinned `nix develop`: Go **1.27.2**, Node **24.13.0**, Darwin/arm64, SDK
`v0.17.0-rc.1.0.20261009012920-31c57af7d7b4`. Go module acquisition was disabled
with `GOPROXY=off GOSUMDB=off`; the module cache was verified. No live service
was used.

- `go test -mod=readonly ./...`, `go test -mod=readonly -race ./...`,
  `go vet -mod=readonly ./...` and `go mod verify` pass.
- The complete shared-core Go suite also passes as actual `js/wasm`, including
  the same synthetic SDK-record key accessor/projection negatives and real
  production-wrapper rejection cases as native. Set `CGO_ENABLED=0` for WASM;
  run its Node executor with a minimal environment rather than inheriting Nix's
  large environment. Neither adjustment changes project toolchain files.
- New `verifier/tor_test.go`: public RFC 8032 SPKI projection; missing/wrong ID,
  duplicate (including conflicting format), wrong format, empty/hostile/odd/
  uppercase hex, malformed/truncated/trailing DER, non-Ed25519 algorithm,
  missing/elapsed/rounded expiry, primary expiry, base key/result failures;
  default response-byte preservation; required manifest; wrapper/wire
  rejections; forbidden caller key selectors and fake verified flags. A
  structurally valid SDK-built envelope with a synthetic quote and an SPKI
  **rejects** at production verification; it is never a trusted positive.
- Scratch native helper/WASM builds via `scripts/build-v3-verifier.sh` pass;
  the generated artifact/hash report is retained only in the scratch directory.
  `tests/v3_verifier_wasm.mjs` passes **40 checks** (including artifact hashes).
  New `tests/tor_identity_verifier_wasm.mjs` passes **31 checks** against the
  built bridge: actual WASM input/action/manifest rejections plus explicitly
  synthetic worker projection/schema/expiry/cleanup checks. Synthetic host
  successes are not authentic hardware/app-key acceptance.
- Active changed-source LSP diagnostics report no errors/warnings (only
  advisory style hints); `git diff --check` passes. Full Pi `tsc` is not
  qualified: the local TypeScript package files/executable are absent. No
  dependencies were installed to fill that gap.

Reproduce native checks in `nix develop`, with the above offline Go settings.
For native/WASM parity, from `attestation-helper` inside that environment:

`CGO_ENABLED=0 GOOS=js GOARCH=wasm go test -mod=readonly -exec="env -i PATH=$(dirname "$(command -v node)"):/bin:/usr/bin $(go env GOROOT)/lib/wasm/go_js_wasm_exec" ./verifier`

Scratch build:

`nix develop --command sh scripts/build-v3-verifier.sh`

Use its printed scratch directory for both built bridge suites:

`nix develop --command node tests/v3_verifier_wasm.mjs /absolute/scratch/directory`

`nix develop --command node tests/tor_identity_verifier_wasm.mjs /absolute/scratch/directory`

### Initial signature packet: offline source verification (before v1 continuation)

Pinned `nix develop --offline`: Rust **1.88.0**, Cargo **1.88.0**, Node
**24.13.0**, Darwin/arm64. Cargo uses `--offline --locked`; no packages, keys or
live evidence were acquired. The existing scratch built bridge is
`/tmp/possums-v3-build-xVULmt`; it was reused, not installed or rebuilt here.

- `cargo test --offline --locked --lib tor_endorsement`: **3 tests pass**.
  These cover exact layout, immutable fixed B, deterministic signatures and
  RFC 8032 test-1 empty-message compatibility; altered/truncated payload and
  signature rejection; malformed/truncated/trailing, mismatched-component,
  wrong-algorithm and v1 PKCS8 rejection with content-free errors.
- `tests/tor_endorsement.mjs`: **55 checks pass**. The runner actually invokes
  Rust's focused fixture test and verifies its raw signature with Node, then
  independently reproduces the identical signature using a Node JWK imported
  from the **public RFC 8032 test-1 seed**. Only synthetic public payload/key/
  signature bytes leave that test runner; no private runtime material is used.
  Cross-language negatives include wrong A/challenge/B/signature, valid
  signatures over wrong domain/version/target, alternate signed messages,
  non-byte inputs, exact-length/trailing-byte violations and JSON substitution.
- Production wrapper rejects malformed evidence/missing manifest/pre-abort via
  the **real built WASM bridge**, despite a valid RFC signature. Separately
  labeled synthetic-worker tests cover shared nonce binding, caller mutation,
  frozen no-transport facts, wrong A/B/challenge, expired source evidence,
  expiry/abort after actual asynchronous crypto, hostile synchronous/callback
  crypto exceptions, an unexpected final abort-state access fault and
  primary-failure retention across abort/cleanup faults.
  Synthetic worker positives are not a valid hardware/app-key chain.
- Existing built bridge suites rerun: **40 + 31 checks pass**.
  `cargo fmt --package possums -- --check` and
  `cargo clippy --offline --locked --all-targets --all-features -- -D warnings`
  pass. `cargo fmt --all -- --check` reports pre-existing formatting differences
  in the vendored Tinfoil dependency, left untouched; its existing dead-code
  warning is also unchanged. No full gateway test/server run was undertaken.
  Active LSP checks on the four touched/new Rust/JS source files report no
  errors/warnings; `git diff --check` passes.

Reproduce the signature checks without a server:

`nix develop --offline --command cargo test --offline --locked --lib tor_endorsement`

`nix develop --offline --command cargo fmt --package possums -- --check`

`nix develop --offline --command cargo clippy --offline --locked --all-targets --all-features -- -D warnings`

`nix develop --offline --command node tests/tor_endorsement.mjs /tmp/possums-v3-build-xVULmt`

### V1 continuation: offline generator interoperability

`examples/tor_grant_fixture.go` uses only **public RFC 8032 test-1 seed** through
supported `ed25519.NewKeyFromSeed`, `x509.MarshalPKCS8PrivateKey`,
`x509.MarshalPKIXPublicKey` and `pem.EncodeToMemory`. It reproduces the exact
`tests/fixtures/tor_grant_rfc8032.json` bytes, including v1 DER, both PEM files,
raw public key and canonical endorsement signature. Fixture SHA-256:
`0e276307c6e707bdf98f54bbe37164b902e8af6a715e2137a85a8e48e2cc2e4a`.
The fixture's private-key fields are published test material, **not runtime
secrets or a usable attested grant**. Reproduction executes repository-pinned
Go **1.27.2**, not the CVM's Go 1.26.5 binary; the latter's exact source encoder
is inspected above. No upstream image/toolchain was built or installed.

Reproduce and compare without acquiring any service key:

`nix develop --offline --command sh -c 'GOPROXY=off GOSUMDB=off GOTOOLCHAIN=local go run examples/tor_grant_fixture.go | cmp tests/fixtures/tor_grant_rfc8032.json -'`

The focused Rust suite now has **4 tests**: exact Go-produced v1 DER is accepted;
v1/v2 derive the RFC public key and produce the same Go signature; inconsistent
v2 still rejects. Both versions reject truncation, trailing bytes and wrong
algorithm; SPKI DER and actual fixture PEM are rejected at the DER core.
The existing Node **55-check** runner now consumes the Rust signature from
that v1 fixture (and independently reproduces it with supported Node JWK import).
The fixed-B/challenge-only API and 95/64-byte outputs are unchanged.

All continuation checks pass in `nix develop --offline`: focused Rust **4 tests**,
actual Node **55 checks** against `/tmp/possums-v3-build-xVULmt`, package fmt,
strict all-target/all-feature offline locked clippy, Go fixture byte comparison,
`gofmt` and `go vet examples/tor_grant_fixture.go`. Active LSP checks for the
changed Rust and new Go source are clean; `git diff --check` passes. The existing
vendored Tinfoil dead-code warning remains unchanged. No Tor-client spike tests
were rerun or spike files changed; no full gateway tests were run.

**Gaps:** no valid full hardware/publisher/manifest/freshness chain with A and
an accepted endorsement through the production wrapper; no live mounted-key
positive evidence (encoding is source-qualified, mount authenticity is not); no B
custody/provisioning/address/transport integration, installed asset or rollout.
Offline rejection/parity tests do not qualify those paths, crypto
performance/resource bounds or deployed behavior. Existing public-evidence
acceptance records remain scoped to their own tested keys and releases; they
are not positive evidence for this new key.

## Privacy and external runtime boundary

Applicable policy: [data handling](../PRIVACY.md#data-handling-boundaries),
[forbidden telemetry](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[typed failures](../PRIVACY.md#mandatory-typed-interface-failures) and
[synthetic testing](../PRIVACY.md#synthetic-testing-exception). Tests use public
synthetic material and hostile sentinels only. The offline interoperability
runner emits only its public RFC/synthetic fixture for Node to consume, never a
runtime grant or seed. No real raw evidence, keys/onion identifiers, exceptions,
requests, credentials or history are added to logs, traces, metrics, support
artifacts or persistence. Existing worker output suppression remains;
diagnostics initiate no inference or automatic retry and
make no billing/refund claims. Native helper exit-code diagnostic limits and
Pi/T3 host persistence remain separate boundaries, not whole-process transience.

The operator accepts **unmodified Arti failfast**: unsupported required
protocols may terminate the **whole gateway**. All-owner graceful Tor shutdown
is not a mandatory condition, and this packet adds no Arti patch or lifecycle
framework. Raw Arti failfast diagnostics are an external privacy boundary:
project-controlled closed errors/output suppression do not prove that Arti's
termination path suppresses raw payloads or that every termination can be
classified. No live reproduction or runtime privacy qualification is claimed
here. Earlier spike patches/artifact cleanup are parent-owned and untouched.

## Backend source integration — default-off

Native `verifier.VerifyTor(evidence, nonce, repository)` is now the sibling of
`Verify`: the **same** `verifyEvidence` / official `NewVerifier().VerifyV3`,
`qualifyPublisher` and `projectTor`, with nil downloaded manifest and Tor
projection enabled. Native self-appraisal has no separate downloaded manifest.
`VerifyTorClient` and the opt-in WASM action still **require** their manifest;
no client trust check was weakened. Default native/WASM response schemas and
`GatewayEvidence` remain unchanged.

Explicit native CLI `--tor-nonce-stdin` accepts exactly **32 raw caller bytes**,
including EOF, on stdin, never nonce arguments/environment. It uses the existing
nonce-bound Tinfoil Unix-socket transport. The separate response is exactly one
of `{evidence: <verified Tor evidence>}` or `{failure: <closed code/stage/constraint>}`.
Tor evidence retains quote, issued time, expiry, release digest and TLS fingerprint,
plus `tor_endorsement_key` raw32 lowercase hex. Fetch transport/status/body/size
failures and official verifier categories are closed; SDK/HTTP/panic payloads
are not returned. Non-Tor helper CLI behavior/schema remain unchanged.

Rust's opt-in `EvidenceVerifier::verify_tor` defaults to unsupported for existing
fixtures. The production helper uses raw stdin, kill-on-drop, a 45-second owner
bound, null stderr and the existing 1-MiB stdout/JSON structural limits. Only
exact reviewed failure code/stage/constraint triples map to typed enum variants;
unknown/hostile fields fail closed and are not echoed. Helper exit/loss before a
valid envelope remains a helper-stage unknown, not invented SDK specificity.
Quote transport size remains subject to these existing bounded-output limits,
not a complete hardware-evidence acquisition guarantee.

`PreparedTor` reads only the fixed grant paths
`/run/tinfoil/keys/possums-tor-endorsement-v1/{private_key.pem,public_key.pem}`,
with bounded file reads. Maintained Rustls PEM and Spki DER decoders plus the
existing Ring v1/v2 parser require Ed25519 and derived/mounted public A equality.
It does not acquire evidence, generate a nonce or retain/check an appraisal at
preparation, bootstrap or launch. This is local consistency, not authentication
of the mounted grant. No private-key Debug/export, custom ASN.1/PEM/crypto, new
key generation or secret arguments are introduced. Per-request appraisal still
rejects a rotation away from the held signer before returning endorsement bytes.
Publication can precede any successful self-appraisal; it does not make an
unavailable attestation endpoint work or qualify the live client trust chain.
The [runtime continuation](tor-dual-endpoint-plan.md#backend-runtime-continuation--offline-source-only)
records the native SDK B-custody choice, ephemeral primary and source-removal
fixture, protected/exclusive host assumptions, signal ordering and readiness.

Only approved enabled state adds public `GET /attestation/tor?nonce=<64 lowercase
hex>`. Disabled builds/processes retain 404 and old `/attestation` + `/v1` wire
behavior. This route shares admission, header/control-body deadlines and no-store
middleware. Accept **only** one exact challenge query with an empty body: no B,
address, arbitrary payload/target, extra/duplicate query or encoded alias.
Each request invokes official Tor appraisal with that same challenge, compares
fresh A to the immutable boot-held signer, and rechecks authenticated expiry
after awaiting and immediately before returning bytes. The signer always uses
its held locally approved B. Successful response:
`{evidence: <raw quote>, payload: <190 lowercase hex>, signature: <128 lowercase hex>}`.
These are the existing canonical **95 / 64 raw bytes**, not signed JSON. The
Rust/Node public RFC fixture produces the same signature through the actual
Axum route. Successful synthetic router facts are **not authentic attestation**.

The handler needs no account token, catalog acquisition, prompt or inference.
This does not remove ordinary gateway startup requirements or authorize a new
connector. Returned endorsement facts are **not** Tor connection ownership,
TLS/HPKE/channel authorization, generation completion, settlement or permission
to execute tools. Clients must separately establish the full selected-B/channel
chain. No evidence cache/retry, fallback, setup replay, inference or recovery
policy is introduced. Specific closed appraisal categories survive in the
existing JSON error message; only existing coarse evidence categories and the
existing Attestation endpoint class enter reviewed aggregate observations.

Offline verification: [complete continuation record](tor-dual-endpoint-plan.md#continuation-verification-and-privacy).
Real mounted-grant authenticity/rotation, positive full native v3 A chain,
measured Linux build/resource headroom, actual Tor bootstrap/publication,
strict connector and installed/deployed paired artifacts remain unqualified.
Existing native v3 platform-signer and legacy-v2 freshness gaps remain.
Policies and negative evidence: [data boundaries](../PRIVACY.md#data-handling-boundaries),
[forbidden telemetry](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[typed failures](../PRIVACY.md#mandatory-typed-interface-failures),
[synthetic tests](../PRIVACY.md#synthetic-testing-exception); actual helper-owner
hostile-output/size tests and exact closed-RPC validation, router current-A/
expiry/query rejection and maintained PEM/SPKI/native-key negative fixtures.
No new logs, dimensions, private data artifacts or automatic export were added;
Arti failfast and native Pi/T3 host history remain external boundaries.
