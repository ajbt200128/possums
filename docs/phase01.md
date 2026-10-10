# Phase 0.1 — feasibility and acceptance record

> **API-only cutover notice (pending runtime/infrastructure verification):** Browser routes, browser fixtures and six-lane/New chat observations below are historical evidence for earlier artifacts, not current product scope or evidence of an API-only release. The target retains JSON `/attestation`, `/v1` and Pi; it removes gateway Web UI and New chat admission. New five-lane API-only fixture/build, negative-route, telemetry-wire and deployed checks must be recorded separately. Historical passes and deployment observations are not retroactive API-only passes.

**Latest status:** scoped `v0.0.9` technical acceptance gates passed; see the final
section. Earlier BLOCKED/local-only statements below are historical records.
Android is excluded by the user's decision; Phase 0.2 has not started.

**BLOCKED. Packet 02 is not authorized by this result.** This is a source-level investigation and preservation record, not an API implementation, a qualified reference client, deployed verification, or Android evidence. Reviewed checkout: `4cfa9d390f88fc8cfce3e3a6e141080e12a0b493` (2026-10-01).

Only packet **01 — reviewed-channel-feasibility-and-preservation** was performed. No existing repository file was edited; this document is the sole addition. No dependency installation, SDK execution, build, browser execution, inference, deployment, staging, or commit occurred. No transport spike or transport test was added: there is no qualified transport to exercise, and a mock/configuration test would not satisfy the actual-transport gate. Phase 0.2 remains out of scope.

## Decision

The inspected published browser stack is `tinfoil@1.2.1`, `@tinfoilsh/verifier@1.2.1`, and `ehbp@0.3.2`. It is **not a qualified Possums candidate**:

1. Its verifier accepts SEV-SNP Guest **v2**, not the helper's nonce-bound attestation **v3**. Neither the existing `/attestation` wrapper nor a renamed route resolves this semantic mismatch.
2. EHBP encrypts bodies, not ordinary authorization headers. Bodyless authenticated discovery has no encrypted request context or encrypted response. Bootstrap in an encrypted body alone would not protect subsequent session/submission headers.
3. The high-level client automatically re-attests and retries on a key-mismatch response. The lower-level transport can return unauthenticated non-success response bodies; redirect rejection is not enforced by default. These defaults cannot authorize uncertain replay or plaintext fallback.
4. No reviewed browser-compatible v3 verifier plus header-protecting channel/server integration was established. Possums has no demonstrated measured ingress adapter owning the endorsed decryption key. No approved API workload/release identity exists.

Node/Rust pinned TLS can protect both headers and content **when** the actual connection terminates at the endorsed key. That does not establish the required browser path. This bounded investigation does **not** prove that every Tinfoil SDK/protocol solution is impossible. A reviewed supported encapsulation of logical method, authorization, and content inside an endorsed-key-bound channel could be investigated separately; none was established here, and no bespoke envelope or cryptography was implemented.

## Preservation and write isolation

Immutable baseline (read only):
`/var/folders/5v/1zw417197236y_k11j994g8r0000gn/T/possums-phase01-baseline-v18pnt38`

Separate scratch evidence/output root, abbreviated **S** below:
`/var/folders/5v/1zw417197236y_k11j994g8r0000gn/T/possums-phase01-packet01-jpa5dbkc`

Before source acquisition or any dependency/build execution:

- Compared all seven baseline/current documents byte-for-byte and against every SHA-256 in `manifest.json`: `AGENTS.md`, `OVERALL_PLAN.md`, `SPEC.md`, `docs/phase0.md`, `docs/verification.md`, `docs/obsidian-plugin.md`, `docs/pi-client.md`.
- Compared the exact binary-capable Git diff of the original **five tracked documents only** with `tracked-user-changes.patch`; staged diff was empty. Recorded HEAD and raw index SHA-256 in `S/baseline-verification.json`.
- `S/preserve.py` recursively inventoried the checkout using `lstat`, readable directory enumeration, and streamed SHA-256 of every regular file. Each record contains relative path, type, size, permission mode, and symlink target where applicable. It fails on unreadable/unsupported entries or changes during hashing, and never follows directory symlinks. This is content evidence, not `git status` evidence.
- `S/checkout-before.json` contains **186,356 entries / 21,698,806,035 regular-file bytes**, including all pre-existing tracked/untracked files except Git internals and active harness metadata (`.git`, `.pi`). The ignored credential file was hashed without printing or copying its contents. No credentials were loaded into a client.
- Other build artifacts were identified: root `target/` and `result`, the latter pointing to `/nix/store/isr4b3bkfhn4vyicxbdwqwp0qnh6i7qs-possums-attestation-0.1.0`. Both the symlink and its immutable referent were inventoried (`helper-result-before.json`: three entries, 20,101,792 regular-file bytes). No other checkout build-output roots were found. Snapshot contents, including its manifest and patch, were separately inventoried in `snapshot-before.json`.

| Preserved tree | Entries (including root) | Regular-file bytes |
|---|---:|---:|
| `node_modules/` | 218 | 17,734,662 |
| `vendor/tinfoil/target/` | 10,428 | 1,881,201,615 |
| `target/` | 175,560 | 19,798,012,705 |

Before inventories' SHA-256:

- `checkout-before.json`: `0341b0e9f15a2837e88852fa318490db0f5455f1c1b38999858510fbbab15d7e`
- `snapshot-before.json`: `d53c5acfdb49127054d6ca93cc9958768ef71d49687bee6b3145af6f3b6c695c`
- `helper-result-before.json`: `8dbf45a21429fd04552a3682a330544e73aac82c8edd4215ca4afdd3fe947b1a`

`S/isolation.json` records absolute executable resolution and reserved fresh directories: `cargo-target`, `cargo-home`, `go-cache`, `go-mod`, `go-path`, `go-bin`, `go-tmp`, `npm-install`, `npm-cache`, `browser-profiles`, `browser-binaries`, `bundles`, `tmp`, `home`, and `sources`. No links were placed over existing trees. These directories were created, not populated by a build/install. Future runs must explicitly set Cargo target/home, all Go outputs/caches, npm prefix/cache and installation cwd, browser executable/userDataDir, and bundle outdir there, with a clean environment inherited by every child. Use locked/offline Cargo resolution, readonly Go modules with `GOPROXY=off`, `GOSUMDB=off`, `GOTOOLCHAIN=local`, and disabled npm lifecycle/browser downloads. An empty isolated cache is not evidence that offline dependencies are available.

Resolution observed without invoking the toolchains:

| Tool | Absolute resolved executable |
|---|---|
| Python (inventory/download scripts) | `/nix/store/2ibhk4gk6yydk4k1kr90v1y5qsm3af5g-python3-3.15.0a7/bin/python3.15` |
| Git (read-only diffs/status) | `/nix/store/rfbzyqcygzpyh5ncb79n07x8bhws2pq3-git-2.53.0/bin/git` |
| Node (not executed) | `/nix/store/lj2m82qrps2pkcg23w3gi209lmaz6ijj-nodejs-slim-24.13.0/bin/node` |
| npm entry point (not executed) | `/nix/store/lnm0rmws82nfirx1vdyq79a94hac3avx-nodejs-slim-24.13.0-npm/lib/node_modules/npm/bin/npm-cli.js` |
| Cargo/rustc (not executed) | both resolve to `/nix/store/6h6jsqm3jw1j4cyg7sfcfxaip9lqbwgw-rustup-1.28.2/bin/rustup` |
| Go | absent from PATH; helper binary was not executed |

A Rust proxy is not a pinned compiler resolution: only `1.88.0-aarch64-apple-darwin` was present under the inspected rustup toolchains directory. Future build execution must resolve the required actual compiler/linker first without implicit installation. `tests/browser_support.mjs:startStreamingFixture` currently spawns bare `cargo run --quiet --example browser_fixture`, without `--offline --locked`, inheriting the environment and stderr. It was inspected, **not run**. A future harness must freeze that child resolution and sanitize output before use; merely moving the parent npm installation is insufficient. No SDK/package/browser child processes ran. Inventory scripts use Python's standard library; their only invoked child during baseline verification was the resolved Git executable. LSP/ast-grep tools were unavailable; inspection used direct reads and scoped text searches.

Final content re-inventory passed: **all pre-existing entries matched** (type, regular-file SHA-256, size/mode and link target); the sole addition was `docs/phase01.md`. Only the expected parent `docs/` directory-size change was exempted. The immutable snapshot and helper-result referent matched exactly. `S/*-after.json` retains the repeated inventories. Seven-document hashes/bytes, exact five-document patch, HEAD, raw index hash, empty staged diff and allowed-path checks were also rechecked before handoff; no unexpected concurrent changes were found. No existing file was restored, deleted or rewritten.

## Exact review sources (not production dependency approval)

Existing sources:

- Rust SDK `tinfoil 0.2.1`: upstream import revision `34157e497a747c191852d52af39cfbdb8dbd9eb7`, plus checkout-local patches recorded in `vendor/tinfoil/POSSUMS.md`. Its exact local source identity is the checkout, not an unmodified upstream release. Both Cargo locks pin `tinfoil-ehbp 0.3.1` to `93cc1fa1ad61e19f9a34fb23cb5b5d5635d3edb0`.
- Go helper: `github.com/tinfoilsh/tinfoil-go v0.15.8-0.20260926043514-91771656ad70`; module `h1:FwelmICERYJpkRgm9fmy9yam9UIsu/+U35GYpmESOy0=`, go.mod `h1:MOaNONFTDd0R+j0lEOcG+ROgdpfutdDXU2FZFPzs94M=` (`attestation-helper/go.mod`, `go.sum`). The cached module's **actual file contents** independently matched the module h1 using Go's sorted module-prefixed per-file SHA-256/Hash1 algorithm in Python; no Go command or module download ran. Source inspected under `/Users/mb5/go/pkg/mod/github.com/tinfoilsh/tinfoil-go@v0.15.8-0.20260926043514-91771656ad70/`.
- Repository npm manifests pin only `playwright 1.61.1`, not a verifier/client. Existing modules were not imported or modified.

Public npm registry metadata and three exact tarballs were retrieved solely for source review into `S/sources`, over HTTPS without credentials, proxies, redirect following, install scripts, or SDK execution. The registry's current tag selected an **investigation version only**, never a trusted workload or release. Tarball bytes matched their metadata SHA-512 integrities. `gitHead` values below are registry-reported revisions, not independently verified signed package provenance. Transitive ranges were neither installed nor fully audited; this is not a complete frozen executable dependency graph.

| Package | Registry-reported revision | Tarball SHA-256 |
|---|---|---|
| `tinfoil@1.2.1` | `50e664521d69f45de84c494b1fdca2131465d028` | `b9f31356f855b86ffd5c88cc41f4e299ce9e2cf741f3d3e55eb89e02cb8c33c0` |
| `@tinfoilsh/verifier@1.2.1` | `50e664521d69f45de84c494b1fdca2131465d028` | `25db47d016f39f5300c1fcba02da1835d7099d9ea81c378de10d02716f717914` |
| `ehbp@0.3.2` | `8528a8dc5ad45c213f14f9bab46a748fda7e18cc` | `4e009d1433354a2d56de416bf52f70d77d0385afdab0f105ed23f763a2c9465d` |

Exact SHA-512 integrities, in the same order:

- `sha512-goql//0KY6nViV96wBdmIT6ZZ7rYy9yEhJrtIPdg9YHw3vyDsyNsuXNq/sZgu0Bu+sm5crxQObxrMGwgN572OQ==`
- `sha512-1DPIPKtyU6YHFvobidQI+gsPJOiidrqR/1103Fe7b8x0WhAXeJ0R82MU5tegTOteOFH2OeTUXrBiPhVsCh1hhg==`
- `sha512-t6aIXrztsQC7jGypZ4Mvc7oMP2LN1q5zAWRKZUcVbgEZgXq/OoD9MAQGFOaCv5LMexx6BOKYpEfIhM/kaRMrtg==`

Reproducible source references are the versioned npm tarballs (`https://registry.npmjs.org/tinfoil/-/tinfoil-1.2.1.tgz`, `https://registry.npmjs.org/@tinfoilsh/verifier/-/verifier-1.2.1.tgz`, `https://registry.npmjs.org/ehbp/-/ehbp-0.3.2.tgz`) and paths below. `S/sources/reviewed-source-pins.json` retains metadata; `S/review-source-sha256.json` records local Rust, cached Go verifier, and downloaded JS source hashes. These are source identities, not attestation trust roots.

## Evidence and channel findings

### Existing gateway and Rust verifier

- `src/web.rs:router_with_body_deadline` exposes the web routes and `/attestation`, not `/v1/*` or a gateway-owned `/.well-known/tinfoil-attestation`. `attestation` serializes **`{gateway: GatewayEvidence, upstream: ...}`**. `src/attestation.rs:GatewayEvidence` wraps a raw `quote` plus helper-generated timestamps, release digest and TLS fingerprint. The Rust wrapper checks syntax/age/bounds, not external-client cryptographic trust.
- `attestation-helper/main.go:main/fetchEvidence` generates its **own** 32-byte nonce, requests the Unix-socket well-known endpoint with that nonce, calls `VerifyV3`, and exports the TLS key fingerprint. An external client did not choose this nonce. `issued_at_unix` is helper wall-clock metadata, not independently authenticated client freshness. The exported upstream verification document concerns the **inference provider**, not Possums ingress.
- Go `verifier/document/document.go` defines `https://tinfoil.sh/predicate/attestation/v3`: challenge, CPU evidence, base64 crypto-material/device-evidence sections and collateral. The quote's report data binds the client nonce and exact decoded-section hashes. `verifier/verifier.go:VerifyV3` authenticates code provenance, platform endorsements, both freshness witnesses and the hardware quote; expiration is the earlier witness deadline (default maximum age seven days), not verification time. `verification.go` exposes separately typed endorsed TLS-SPKI and X25519 HPKE items. The caller must enforce expiration before new requests and bind traffic to those keys.
- Rust `verifier/attestation/mod.rs:fetch/verify_full_with_options`, `types.rs:AttestationDocument/PredicateType`, and `sev/mod.rs` instead fetch a no-nonce `{format, body}` v2 document, authenticate the AMD chain/report and policy, and extract TLS fingerprint/HPKE key directly from the two report-data halves. v3 uses those bytes differently. TDX is explicitly unsupported in this Rust verification path. No external nonce/witness-expiry check was found in this path; a local `verified_at` is not one.
- Rust `client.rs:verify_attestation` combines hardware/measurement verification, provenance and TLS/SAN binding; `with_measurement` explicitly **skips Sigstore**. `verifier/sigstore/mod.rs:verify_repo` discovers the latest release. Neither behavior is the Phase 0.1 approval policy below.
- Rust `verifier/tls.rs:PinnedCertVerifier` checks WebPKI certificate/hostname validity and attested SPKI on the actual handshake. `OriginBindingMiddleware` enforces origin; the local redirect policy permits up to ten same-origin redirects. This provides a native TLS foundation, not browser verification and not the required reject-all-redirect policy.
- Rust `client.rs:new_with_proxy` explicitly exposes the bearer header to its proxy. `ehbp_transport.rs:send` rejects authorization-bearing bodyless requests; `send_replayable/retry_after_mismatch` can refresh/rekey and send again. Its proxy client disables redirects, but body encryption still does not hide headers. These paths cannot be used unchanged for Possums client authentication.

### Published JavaScript verifier and transport

Paths in this section are inside the exact npm packages above.

| Requirement | Source observation | Qualification |
|---|---|---|
| Evidence compatibility/freshness | Verifier `dist/attestation.js:verifyAttestation` accepts only SEV Guest v2; extracts report-data halves as TLS/HPKE keys. `dist/client.js:verifyBundle` takes an ATC-style bundle with `enclaveAttestationReport`, `vcek`, `digest`, `sigstoreBundle`, `domain`, `enclaveCert`. No client nonce or v3 freshness-witness expiry appears in this path. | Incompatible with current v3 helper output; renaming/wrapping is insufficient. |
| Provenance | Verifier `dist/sigstore.js` verifies DSSE using embedded Sigstore roots, GitHub OIDC issuer, configured repository and a tag ref, subject digest and optional selected tag, then compares measurements. | Useful verification, but no independently approved Possums API digest/measurement. Bundle-selected values are not client approval. |
| Key endorsement | Verifier `dist/cert-verify.js` compares certificate SAN domain, HPKE key and attestation hash; `dist/attestation.js` obtains HPKE key from authenticated report data. | Does not identify a deployed Possums EHBP decryption owner or make browser HTTPS header traffic terminate there. |
| Bootstrap and authorization | Tinfoil `dist/encrypted-body-fetch.js:createEncryptedBodyFetch` constructs `Headers` and sends them to EHBP unchanged. EHBP `dist/esm/identity.js:encryptRequestWithContext` seals only body bytes. | Outer authorization/session/submission headers remain visible to TLS terminators/intermediaries. No reviewed full-request encapsulation was found. |
| Authenticated logical GET | EHBP `dist/esm/client.js:request` and `identity.js` pass empty-body requests through with no HPKE context, returning the ordinary response. Browser Request cannot fix this by adding a GET body. | Logical `/v1/models` needs a reviewed protected method/header encapsulation, not an ordinary bearer GET or ad hoc POST workaround. |
| Response progress | EHBP `identity.js:decryptResponseWithToken/createDecryptStream` authenticates/decrypts each complete frame and exposes a `ReadableStream`; cancellation cancels the source. Requests are collected with `arrayBuffer`; response frame ceiling is 64 MiB. | Source-level progressive decryption exists. Not a browser/Android execution result or a Possums resource bound. Bounded request buffering is distinct from forbidden complete-answer buffering. |
| Plaintext error fallback | EHBP `client.js:shouldDecryptResponse` accepts non-OK responses without the response nonce as ordinary responses after checking for mismatch. | Must not become authenticated application errors or a fallback. No safe integration qualified. |
| Redirects | EHBP `request-options.js` forwards caller `redirect`; default Request/fetch follows redirects. Tinfoil `dist/atc.js` fetches bundles without a reject-redirect policy. | Reject-all redirects is not enforced by this stack's defaults. |
| Retry/rekey | Tinfoil `dist/secure-client.js:fetch` catches `KeyConfigMismatchError`, resets, re-verifies and sends once more. EHBP derives that error from an unencrypted 422 problem response. `ready` also retries some verification failures. | No sensitive request may be automatically replayed based on such a response. Pre-send verification retry and post-send replay are distinct. |
| Native alternative | Tinfoil `dist/pinned-tls-fetch.js` uses Node `crypto`, `https`, `tls`, `stream`, certificate SPKI pinning and a streaming response; Bun has its own fetch path. `dist/secure-fetch.js` rejects TLS-only operation in real browsers. | Headers/content can be protected natively, but this is not the mandatory shared browser candidate. Node does not follow redirects here; Bun uses fetch semantics. Neither was executed. |
| Browser/runtime dependencies | Package browser mappings replace TLS/WebSocket/cache-file modules. EHBP uses `Request`, `Response`, `Headers`, streams, WebCrypto and `hpke`/`@panva/hpke-noble`; verifier uses browser Sigstore/crypto/TUF packages and `DecompressionStream`, with a dynamic `zlib` fallback. Tinfoil declares Node >=20 and OpenAI, AI-SDK, `ws` dependencies. | Entry points are promising, not runtime compatibility or audited bundles. Transitive resolution, CORS, bounded evidence decompression and device execution remain unverified. |
| Persistence/cache | `dist/secure-client.js:createTransport` calls `resolveUserCacheSecret`; `dist/user-cache-secret.js` injects `user_cache_secret` for chat and resolves nonempty option/env, then disk or process-lifetime generation. Empty string does **not** disable it. Browser store is a stub; Node store persists under `~/.tinfoil`. | No persistence/cache-disable configuration was qualified. Do not import/execute defaults against the user's home or advertise prompt-cache disablement from these sources. |

### Required measured server support remains missing

`src/web.rs:serve_with_header_deadline` accepts HTTP/1 on the application port. The current read-only inspection of `tinfoil-config.yml` shows `attestation: true`, the Unix attestation socket, and shim forwarding to port 8080. This does **not** demonstrate an application-owned HPKE private key, reviewed EHBP ingress decryption, protected-header reconstruction, or progressive encrypted responses. A shim may have platform capabilities, but its measured implementation, key ownership/placement and actual ingress protocol were not established here. No platform service was contacted.

The server requirement is stronger than returning an endorsed public key: the reviewed measured endpoint must own the corresponding private key, terminate/decrypt the **actual** protected request there, authenticate the protected logical method/headers, enforce resource admission before decryption/parsing expansion, and progressively encrypt replies without complete-response buffering. Bootstrap and authenticated discovery must use that same channel; keys/cookies/headers must not escape through outer metadata. Evidence acquisition must allow client-chosen freshness without first requiring a sensitive credential.

## Fail-closed approval policy for subsequent review

This is a required policy, **not implemented client configuration**. Current approved Phase 0.1 API identities: **empty set; send nothing**.

- Repository identity is exactly `ajbt200128/possums`, not the inference-router repository. Expected release publication workflow is `https://github.com/ajbt200128/possums/.github/workflows/tinfoil-release-publish.yml@refs/tags/<explicitly-approved-new-tag>` with GitHub Actions OIDC issuer `https://token.actions.githubusercontent.com`. The placeholder is not a usable pin. Authenticate the actual build/provenance identity and immutable source commit, not only a repository name or tag supplied by the server. Current workflow source pins `tinfoilsh/measure-image-action` at `f2ec2fdf4510459730f0a5158c5817f5361e9461`; that observation alone does not approve a future build.
- Require independently approved, exact release-manifest SHA-256, immutable source/build identity, measured configuration and OCI image digest, permitted hardware/platform identity and measurement register values, and exact serving origin. All remain **missing for the API**. The manifest digest is not interchangeable with the OCI digest, a package integrity, or HEAD.
- Verify hardware quote, allowed policy/TCB and platform endorsements; authenticate provenance and compare approved code measurements against the quote. A measurement pin is an **additional constraint**, never a provenance bypass. Reject unknown format/platform, missing/revoked approval, expired witnesses, malformed evidence and mismatched keys.
- Require fresh client-chosen nonce-bound evidence using a supported reviewed verifier; check authenticated witness expiration before each new sensitive request. Server timestamps or local verification timestamps cannot extend it. Reverification/rekey must reapply the same explicit approval; it never authorizes replay of an uncertain request.
- Establish the actual endorsed-key-bound protected channel before providing recovery/session credentials, authorization/submission material, prompt/history or tool results. Reject redirects, plaintext/buffered fallback and automatic uncertain replay. No default-router or latest-release trust discovery. No successful standalone `/attestation` followed by unbound fetch.
- `v0.0.8` approval is scoped **streaming WEB** evidence only. It approves neither changed API code nor a new key-owning adapter. No historical digest/measurement was copied into an API approval list. Synthetic fixtures must have separate trust roots and cannot enable production.

## Exact adaptation review surfaces; no edits authorized by this record

These identify where subsequent decisions would land, **not a request to execute packet 02 now**:

| Surface | Exact proposed paths | Required prior evidence |
|---|---|---|
| External client freshness/evidence | `src/attestation.rs`, `src/web.rs`, `attestation-helper/main.go`, `attestation-helper/main_test.go`, `tests/attestation.rs`; helper `go.mod`/`go.sum` only for an approved verifier change | Reviewed external-client v3 verification and bounded challenge/evidence protocol. Preserve the web evidence contract or explicitly version the new one; never trust helper summaries instead of client verification. |
| Measured ingress/channel adapter | Existing `src/web.rs`, `src/main.rs`, `Cargo.toml`, `Cargo.lock`; new synthetic fixture `examples/phase01_fixture.rs` and `tests/phase01_channel.mjs` | A reviewed supported server transport with endorsed private-key ownership, protected method/headers/bootstrap/discovery, resource bounds, progressive responses and no replay. No new gateway module, custom crypto or speculative adapter now. |
| Pinned reference policy/transport | New `examples/phase01-approval.ts`, `examples/phase01-client.ts`, conditionally `examples/phase01-transport-spike.ts`, `tests/phase01_transport.mjs`; `package.json`/`package-lock.json` only once a candidate qualifies | Compatible reviewed verifier/channel, complete immutable dependency graph, explicit approval, no uncertain retries/redirects/cache persistence. |
| Native vendored SDK changes, only if later justified | `vendor/tinfoil/src/client.rs`, `vendor/tinfoil/src/ehbp_transport.rs`, `vendor/tinfoil/src/verifier/attestation/mod.rs`, `vendor/tinfoil/src/verifier/attestation/types.rs`, `vendor/tinfoil/src/verifier/tls.rs`; provenance selection would additionally implicate `vendor/tinfoil/src/verifier/sigstore/mod.rs` and `vendor/tinfoil/src/verifier/github.rs` | Independent review of exact edits first. Native changes alone do not solve browser verification. No dependency-cache or target edits. |

If key delivery, shim behavior, CORS or measurements require `tinfoil-config.yml`, `flake.nix`, `deploy/`, `.github/workflows/`, new gateway modules, or external SDK source changes, **stop for separate explicit scope approval**. Those paths are outside this packet's edit authority; no deployment/config change was attempted.

## Gates and permission-scoped next actions

1. Supply or authorize further review of a **specific pinned browser-capable v3 verifier and credential-protecting channel**, including reviewed server-side protocol/key ownership. The inspected body-only EHBP defaults are not sufficient. Do not dispatch API implementation or ask Sol to invent a transport workaround.
2. Establish source-backed measured ingress/key support and exact permitted adaptations before requesting packet 02 authorization. If platform or deployment changes are necessary, ask for that scope separately. Public source review is not permission to access production or deploy.
3. Only after candidate qualification may local synthetic actual-transport tests be designed: failed verification must cause zero credential/content transmission through the actual path, with successful controls, counters/stages only, reject-all redirects, no replay and progressive decryption. No such tests ran here; no zero-byte runtime result is claimed.
4. Packet 03 real-browser execution/qualification still gates API packets. Physical Android execution, independently approved new API release/deployment, fresh provenance/channel evidence, and the full local regression/end-to-end gates remain absent. External credentials/access, release/deployment, any funded probe and physical-device execution need their own permission.
5. Routine Sol work remains separable: after Astra freezes a qualified transport/policy/fixture, Sol can implement the bounded browser harness. There is no safe browser-harness implementation to delegate yet.

Validation in this packet is limited to document/patch/index and artifact preservation, public tarball integrity, cached Go module integrity, source inspection and whitespace/scope checks. Root Rust, standalone vendor, helper, existing browser and new transport suites were **not run**; none are reported passing. The stronger packet-08 regression matrix is not absorbed into packet 01.

Historical `v0.0.5` buffered/refund-on-incomplete-delivery and additive packet-1 staging records remain unchanged. Scoped accepted `v0.0.8` streaming-web evidence remains unchanged. Provider-invoice/maximum-billable-cost, runtime/privacy/cache/egress/logging, live negative injection/zero-upstream-byte, process RSS/scoped allocation and detached-driver lifetime UNKNOWNs are not upgraded by this investigation.

---

## Superseding packet 01 decision — published external v2 permitted

**Current result: BLOCKED on protected browser authentication and unchanged bodyless authenticated discovery, not on v3 availability or local edit permission.** No supported transport candidate is frozen. Packets 02–08 and Phase 0.2 remain stopped. This appended section supersedes conflicting decisions, permission restrictions and missing-ingress inferences above; the preceding record is preserved as historical evidence.

The user explicitly chose “ok lets do v2 for now, thats ok” and authorized necessary local edits, endpoint checks and minimal funded probes. Published legacy v2 is permitted for the **external client**. Quote age and client-chosen nonce/freshness-witness guarantees are **unprovable in this path**. Server/helper timestamps and local verification time do not prove quote freshness. This caveat is accepted, not a reason to demand external v3 again. Malformed/signature-invalid evidence, supported collateral validity failures, known revoked evidence/approvals, unapproved releases and endorsed-key mismatches must still fail closed. Acceptance of v2 is not acceptance of unchecked revocation or a latest-release approval policy.

Local path-scope restrictions and the separate funded-probe permission requirement in the historical record are superseded. Publishing, pushing, releasing, production configuration changes and deployment still need separate permission. This packet used only source/preservation work; no credential-bearing request, SDK execution, inference or spending occurred. `tinfoil-config.yml` remains at **0.14.12**, and `attestation-helper/main.go` still requests its own nonce and calls **VerifyV3**. The external decision does not downgrade gateway self/upstream gates.

### Preservation and source identities

Fresh isolated scratch, abbreviated **R**:
`/var/folders/5v/1zw417197236y_k11j994g8r0000gn/T/possums-phase01-v2-a2dwha4p`

`R/baseline-before.json` and `R/baseline-after.json` record focused checks against the original baseline and `S/baseline-verification.json`: all seven original documents have identical bytes/SHA-256; the exact binary five-document tracked patch remains `22927902fa16c0b2ccdf0164762c3be1ca99838e6027a08a318f14ac01eb3d1a`; HEAD remains `4cfa9d390f88fc8cfce3e3a6e141080e12a0b493`; raw index SHA-256 remains `79cc4cf566ebd937726d5b32cfc7af042a4851ef1fb5638a0db5f725f8ed39aa`; staging is empty. The previous `docs/phase01.md` bytes are an unchanged prefix of this file. Only this appendix was added to the checkout. Existing `node_modules/`, `vendor/tinfoil/target/`, other outputs and baseline artifacts were not modified. No recursive checkout inventory, reset, stash, deletion, dependency installation, staging or commit was performed; no claim of a new full-tree content inventory is made.

Reused the three already-acquired npm tarballs/extracted trees, without downloading or installing them again. `R/reused-packages.json` records matching original SHA-256/SHA-512 integrities and byte comparisons of all regular tar members against existing extracted files: **94** for `tinfoil@1.2.1`, **83** for `@tinfoilsh/verifier@1.2.1`, **89** for `ehbp@0.3.2`.

Additional upstream source identities, resolved through public GitHub metadata and then acquired at immutable commit URLs into R only:

| Source | Tag / commit | Important content identity |
|---|---|---|
| `tinfoilsh/cvmimage` | `v0.14.12` / `35d94c85ad20e9f98cd4ddcf35dfe4574b5215fb` | `tinfoil/cmd/shim/api.go` SHA-256 `cb10faccb34a2479916def133bf84cb6892e2716bc5ff7856055f96e6a62a15e` |
| `tinfoilsh/encrypted-http-body-protocol` | `v0.3.2` / `8528a8dc5ad45c213f14f9bab46a748fda7e18cc` | `identity/middleware.go` SHA-256 `b6c7762b14dafc226631b052c32c6fa5a890772da7e7b55212bb13561a460171` |
| `tinfoilsh/tinfoil-config` | `v0.1.14` / `70d5811ce0f931e4c9a0605a0c479eba94001c74` | `shim.go` SHA-256 `12fc70d02acc6d3dfdd93fb182be5ec9be4e4aafda14b53222b8a37b2dc16c2b` |

`R/source-pins.json` records acquired paths, sizes and SHA-256 values; its SHA-256 is `c63280fd0ec4d5ec73bc1b89b579432233997c8f5685f5b74cec2bec9199b75b`. These are review identities, **not** verified release provenance or an executable dependency freeze. The actual cvm module declares **Go 1.26.5**, EHBP **v0.3.2**, and config **v0.1.14**. Its `go.mod`/`go.sum` SHA-256 values are respectively `b7b75d1b840ca23e53c464c0be0a95baad29b66f99e15a4990c47138c5171c8a` / `e6e1b2f5beee994d6c59e0c01f53f80abb9a70befb4405ee225209d3021f09cc`. EHBP module/go.mod checksums are `h1:yKxfC3pVJ4ORwLeHwflvv2x8ld2Eo3fpcKt+wYIJorc=` / `h1:THDK0GFNny7Pcc+nO3AQi4f6Wf1cDkfLatmHAUlIn5s=`. No Go resolution/build ran. LSP/ast-grep tooling was unavailable; inspection used direct reads and scoped text searches.

### Corrected server evidence: measured ingress exists

The exact cvm sources establish a real shim ingress, not merely the Possums helper wrapper:

- `cmd/shim/api.go:registerObservabilityHandlers` publicly serves `/.well-known/tinfoil-attestation`. **Without a nonempty nonce** it serializes the boot legacy document. A nonempty nonce must decode to 32 bytes; it then collects device evidence/collateral and calls `BuildAttestation` for fresh **v3**. These observability routes are registered separately from the workload path allowlist/authentication handler. A public no-nonce v2 source therefore exists without changing the helper or CVM version.
- `cmd/shim/attestation_socket.go:newLocalAttestationHandler` deliberately restricts the inherited **Unix stream socket** to exact escaped well-known path, GET, exactly one nonce query parameter of 64 hex characters, and zero content length. It is not the public handler's no-nonce policy. `cmd/pid1/local_attestation.go` creates/retains that listener; `internal/boot/paths.go` distinguishes the private socket and granted `/tinfoil/attestation.sock` container mount. Possums already declares `attestation: true`.
- `cmd/boot/identity.go` generates the P-384 TLS key and loads/creates an X25519 HPKE identity in the private ramdisk, creating the HPKE file mode 0600. `cpuattest.go` obtains `CPUAttestation.V2Doc`. `internal/attestation/attestation.go:BodyV2.Marshal` places SHA-256 TLS SPKI fingerprint in the first report-data half and the HPKE public key in the second. Additional workload keys are **v3-only**, not endorsed by this legacy layout. Production obtains a hardware report; localhost/dummy configuration is explicitly noncryptographic and cannot qualify production.
- `cmd/boot/cert.go` uses that TLS key to obtain a certificate, embeds HPKE and (by default) legacy-attestation-hash SANs, and writes its private key mode 0600. `cmd/shim/main.go` loads the same TLS certificate/key and HPKE file, constructs the v3 identity body from them, and supplies the identity to `NewShimServer`. TLS termination and EHBP decryption thus belong to the shim, **not the Rust application**. Private key possession is necessary but does not itself protect outer browser headers.
- `nix/go.nix` builds `cmd/shim` as `tinfoil-shim` with the CVM runtime. Boot `config.go:loadAndVerifyConfig` checks the measured-config hash from kernel command line before decoding it. Shim configuration aliases pinned `tinfoil-config.ShimConfig`; external metadata/domain/secrets are explicitly separate, unmeasured inputs. This source-backed measured-ingress architecture corrects the earlier unsupported absence inference; it is not independent verification of a currently serving image/configuration.

Existing configuration selects upstream port 8080 and `/*`. Pinned defaults include `authenticated: false`, no rate limiter, no origin allowlist, cert-proxy TLS with DNS challenge, attestation publication enabled, dummy attestation disabled and zero expected GPUs. With a validator, nil `authenticated-endpoints` defaults to only `/v1/chat/completions`; these shim keys are not Possums account authentication. `NewShimServer` applies CORS outside routing, workload path checks before EHBP, then EHBP before shim authorization and reverse proxy/application admission. The proxy removes the EHBP encapsulated-key header, rewrites host/forwarding headers, and strips upstream response CORS-origin/response-nonce headers. Public server timeouts are ten seconds for headers and two minutes idle; no explicit public body `ReadTimeout`, response `WriteTimeout`, or application-style memory lane is configured there.

CORS reflects any supplied origin when the allowlist is empty, permits credentials, echoes requested preflight headers, exposes EHBP nonce/key headers, and returns 204 for allowed OPTIONS before application handling. That is **source behavior**, not live CORS evidence or a substitute for client credential exclusion. HTTP/2 CONNECT support in `tunnel.go` is not an EHBP full-request browser transport: it uses outer authorization, bypasses that middleware path, and requires published ports. No published ports are configured here; native tunneling cannot satisfy the browser requirement.

### Transport gate: concrete failure, even with accepted v2

| Gate | Exact reviewed behavior | Result |
|---|---|---|
| Protect logical method and explicit credential/session/submission headers | JS `encrypted-body-fetch.js` forwards headers; EHBP `identity.js:encryptRequestWithContext` copies method/URL/headers and seals only the body. Go `identity/crypt.go` uses body encryption with no method/header binding as associated data. Shim forwards the ordinary headers, without protected-header reconstruction. | **FAIL.** Ordinary HTTPS protection is not a browser-verifiable connection to the endorsed TLS key. |
| Unchanged bodyless authenticated `GET /v1/models` | JS `Transport.request`/`Identity.encryptRequestWithContext` return no HPKE context for an empty body; Go `Middleware` explicitly passes bodyless requests through. Response is ordinary plaintext at the EHBP layer. | **FAIL.** Bootstrap in an encrypted POST does not protect later GET/auth headers. Adding a body to browser GET or inventing a POST envelope changes the required protocol. |
| Progressive protected response | JS authenticates complete frames and yields a `ReadableStream`; Go derived response writer encrypts writes and implements Flush. | Source support for body-bearing EHBP only; **not** executed browser/Android or full-request evidence. HTTP status/ordinary headers are not authenticated by body encryption. |
| Non-overridable ambient-auth exclusion on every fetch | Verifier `bundle.js:fetchOk`, Tinfoil `atc.js`, and `getServerIdentity` use bare fetch; default credentials/redirect/cache policy is not hardened. EHBP `Transport.create` accepts caller init. `request-options.js` forwards caller `credentials`, `redirect`, `cache`; Request/RequestInit normalization preserves or overrides them. | **NOT QUALIFIED.** No mandatory `credentials:'omit'`, reject-redirect, no-store policy across acquisition and application fetches. Even `omit` alone does not strip explicitly supplied Authorization. Allowlisting/reconstruction must prevent both caller overrides and explicit outer auth; no such policy was implemented. |
| No uncertain replay or plaintext fallback | High-level `SecureClient.fetch` resets/re-attests/replays once on `KeyConfigMismatchError`. Low-level `Transport.request` has no such replay loop but accepts unencrypted non-OK responses and inherits redirect policy. | Defaults rejected. Low-level use could avoid high-level replay, but cannot solve missing header/GET protection. A post-send 422/redirect is uncertainty, not authenticated proof of non-admission/refund. |
| Persistence/cache controls | High-level `resolveUserCacheSecret("")` resolves environment/disk/process secret; Node store can persist under `~/.tinfoil`. Browser mapping avoids that filesystem store, but process secret scoping is not cache disablement. Direct EHBP source has no corresponding disk-secret injection; session recovery tokens are exportable secrets. Fetch cache policy is caller-controlled. | **NOT QUALIFIED** end to end. Low-level empty-secret injection no-op is not proof of high-level or provider cache disablement. No SDK was run or runtime write behavior tested. |

Published verifier compatibility is narrower than “all v2”: `dist/attestation.js` accepts SEV-SNP Guest v2; `sev/cert-chain.js` requires Genoa/VCEK and checks certificate dates/signatures, while report policy validates TCB/hardware binding. It does not provide the v3 nonce/witness guarantee. No CRL/revocation check was identified in the inspected AMD chain path; do **not** report revoked-evidence coverage as passed. Supported collateral validity and known revocation/approval rejection remain qualification requirements. `verifyBundle` permits separately acquired input, but `assembleAttestationBundle` fetches latest release and retries acquisitions. Neither bundle-selected tag/digest nor the verifier's `securityVerified` flag is an independent Possums release approval. Existing exact repository/workflow/build/manifest/image/measurement/origin policy above remains required, with the mandatory **external v3** clause replaced by this explicitly scoped v2 decision. Approved new API workload identities remain **empty**.

Native SPKI-pinned TLS, certificate/evidence retrieval followed by ordinary browser fetch, a draft PR verifier, an invented full-request envelope, and renamed/unauthenticated discovery do not cure these failures. No draft cryptography was adopted. This is a rejection of the inspected supported stack for the specified contract, not a proof that every future transport is impossible.

### Resource and fixture consequences; no packet 02 freeze

Actual EHBP `identity/middleware.go` probes a decrypted byte **before** shim authorization and before Rust request admission. `identity/crypt.go:StreamingDecryptReader.Read` reads the four-byte unsigned frame length and allocates that ciphertext length without a request-frame ceiling, then decrypts the complete frame. It recursively skips zero-length frames. Thus the first probe can allocate far beyond the application's later body/lane limits; the application's 8-MiB/4-KiB ceilings are not evidence of bounded shim decryption. No hostile live probe was attempted.

Published JS collects request bytes, seals the whole nonempty body as one frame, and copies framing buffers. For the default AES-256-GCM suite a frame adds a 16-byte authentication tag plus four-byte length prefix; that wire expansion does not bound simultaneous plaintext/ciphertext copies. JS response frames have a 64-MiB ciphertext ceiling and incremental buffering/copies; Go response writes encrypt each supplied write, without an independent small-write bound. Neither property proves process allocation/RSS safety or bounded pre-admission decryption. These observations leave existing shim/process allocation, RSS and detached-driver UNKNOWNs intact; they do not impose or claim a new whole-process memory proof.

Because the primary browser authentication/discovery gate fails, **no viable server/client/policy or packet-02 execution contract is frozen and no fixture is implemented**. If a supported solution is supplied, packet 02 must use a test-only Go driver in an isolated cvmimage source overlay, calling the actual `NewShimServer`/`registerObservabilityHandlers` and real pinned EHBP `Middleware` with synthetic isolated identity and a local backend. It must explicitly freeze proxy/auth/CORS/path/timeouts/limits and distinguish synthetic evidence, local certificates/backend and absence of hardware/boot/control-plane execution from production. A rewritten Rust crypto server or synthetic ingress is not equivalent. Go 1.26.5 (including `crypto/hpke`), complete readonly modules, compiler/linker/platform compatibility and isolated execution are prerequisites; **Go is absent from current PATH and this actual server path was not built or exercised**. Prerequisite resolution and execution qualification remain packet 02, not implied by these source pins.

### Public checks and remaining evidence

**Required deployed checks were not executed: the exact deployed Possums public origin was not supplied or identified in the scoped repository/baseline review.** Public repository metadata has no homepage. A bounded hostname-only review of prior task context did not establish the target either; no credential or raw context was printed. The missing hostname was requested, without requesting secrets. No guessed domain, provider router or synthetic server was substituted for Possums. Consequently public no-nonce/v3 response status/format, certificate-to-report/SPKI/HPKE binding and deployed CORS are **NOT OBSERVED in this reopening**. This is an explicit incomplete investigation item, not a claim of live compatibility. Earlier `v0.0.8` streaming-WEB evidence remains historical and cannot approve new API code.

New public **source** acquisitions used a clean environment, direct HTTPS with certificate verification, an explicit empty proxy handler, no auth/cookie handlers, redirect rejection, twenty-second operation timeouts and at most 2 MiB per response. Only source files/identity metadata were retained; failure output was categorical. These controls qualify the source downloads only, not browser fetch policy or a deployed channel.

Next credential-free deployed evidence requires the exact public origin, then bounded direct no-redirect GETs of `/.well-known/tinfoil-attestation` (no nonce), `/.well-known/tinfoil-certificate`, `/.well-known/hpke-keys`, one random-nonce v3 compatibility check if needed, and OPTIONS with a synthetic origin/requested headers. Exclude inherited proxies/auth/cookies, bound response/decompression/time, report only status/format/digests/binding categories, and do not use server time as freshness. Structural/key consistency is distinct from hardware/provenance verification; ordinary Python TLS observation is not a browser pinning proof. No application credential or prompt is needed. Supplying the origin would resolve this missing observation, **not** the proven header/bodyless-channel defect.

**Stop / next capability:** obtain a specific maintained, published, reviewed browser transport and matching measured server support that protects logical method, explicit authentication/submission headers, unchanged bodyless authenticated GET discovery, bodies and progressive responses to the endorsed key. It must also permit non-overridable no-ambient-auth acquisition/outer fetches, no redirects/replay/plaintext fallback, qualified persistence controls and bounded decryption before admission. Merely changing EHBP fetch options cannot supply the missing protocol. Until that evidence exists, do not dispatch Sol's harness, implement API routes, spend credits or start packets 02–08/Phase 0.2.

Verification here consists of focused preservation checks, reused npm integrity/extracted-byte checks, pinned source review and appendix whitespace/scope checks. No transport capture, negative verifier/runtime suite, actual shim execution, browser/device run, production verification or funded test passed in this packet. Physical Android, full API/lifecycle/resource tests and an independently approved new API release/provenance/channel remain later mandatory gates; publication/deployment still need permission. All pre-existing runtime/privacy/cache/egress/logging, invoice/maximum-billable-cost and resource UNKNOWNs remain unchanged.

---

## 2026-10-03 — packet 01 executable TLS/v2 channel qualification

**Supersedes the historical header/bodyless-GET and mandatory-external-v3 blockers above.** The user accepts ordinary validated HTTPS for credentials, ordinary headers and authenticated bodyless discovery, and legacy external v2 without client-nonce/witness/quote-age guarantees. Prompt bodies and streamed replies still require endorsed-key EHBP. The gateway remains on **CVM 0.14.12** and its helper still calls **VerifyV3**; neither was edited.

**Local implementation and execution now exist; Phase 0.1 is NOT accepted.** Only packet 01 was implemented: `examples/phase01/{approval.ts,transport.ts,limits.ts,package.json,package-lock.json,tsconfig.json,build.mjs}`, `tests/phase01_transport.mjs`, and the isolated overlay `tests/phase01_shim_test.go`. No Rust API/accounting extraction, UI harness, new deployment or Phase 0.2 work occurred. Production API approval is empty and cannot be enabled by fixture trust. One genuine qualification limitation remains: **unattended live browser collateral acquisition is not qualified** (details below). This does not invalidate the executed browser verifier/EHBP interoperability or require waiting for a new deployed API identity/physical Android before safe local API work. Sol's UI harness remains separate, after packet 05 supplies its final client/fixture bundle.

### Implemented boundary

- `qualifyWeb` accepts bounded evidence bytes, never a caller's verified flag. It invokes the actual pinned `Verifier.verifyBundle`, checks the independently approved manifest bytes, exact tagged repository/workflow/commit/build invocation, byte-bound measured configuration and OCI pin, entire signed predicate versus that manifest, SNP measurement comparison, certificate dates/SPKI endorsement, certificate SAN endorsements, and exact HPKE configuration/key equality. It returns a **WEB-observation-only** result, not an API credential-send capability. The manifest was reused from `/tmp/possums-v008-release.0TypUy/tinfoil-deployment.json` only after matching the independently recorded SHA-256; no server-selected latest release was used.
- Approval pins: manifest `7363e7637ccbb84c2b243baee91ad08a65245feef1483e8b39d1111e5f65daae`; OCI `sha256:2a4694aa952e9102eb1786a5f7940633ba8b9cd6c5ecb8983797db1268c4a51e`; source commit `2af94f852d9467db33f473ae38b6333412342571`; measured config SHA-256 `483d8cf1d9aa2491fbb0c57926bc24ae3e7cf9a62f0c51fac1d6bf0acb15102c`; release workflow `ajbt200128/possums/.github/workflows/tinfoil-release-publish.yml@refs/tags/v0.0.8`, invocation `36822521244/attempts/1`. Config bytes also match `git show v0.0.8:tinfoil-config.yml`. The signed SNP measurement is `da0001c48bc44650207ca36ca4910faa46d10c4e0d53fa735e3f7deaddb8c1dd6cf1fd0def2815ff8950b0d03b80320d`. The qualification approval expires **2026-10-08 UTC** administratively; that is not evidence freshness and must not silently extend itself.
- The transport uses published `Identity.encryptRequestWithContext` / `decryptResponseWithContext`, not `SecureClient`, `Transport.create`, retry/rekey machinery, recovery-token persistence, or cloned plaintext-error inspection. Caller-supplied Request/stream/fetch options are not accepted. Typed control/chat schemas reject unknown fields including tools. There is no API production channel constructor: it fails closed; the fixture factory is compile-time disabled in production artifacts, restricted to HTTPS localhost in fixture artifacts, and guarded by a module-private runtime constructor capability/private fields.
- Every application/acquisition request reconstructs exact permitted destinations/methods, `redirect:error`, `credentials:omit`, `cache:no-store`, `referrerPolicy:no-referrer` and abort signals. Explicit bearer headers are permitted only on the established channel. Normal production Fetch certificate/hostname validation is unchanged. An encrypted request requires a response nonce/body; missing nonce/plaintext errors are cancelled without draining/cloning. Any post-send error, redirect, framing failure, timeout or key mismatch is **uncertain**, with no replay or plaintext/buffered fallback. Ordinary outer status/headers are not EHBP-authenticated application facts.
- UTF-8 serialization is size-counted before stringify/library collection/encryption, including escapes, surrogate pairs and sparse-array rejection. All response bodies are capped during reads, not trusted Content-Length. Key config must be exactly **41 bytes**, key ID 0, X25519/HKDF-SHA256/AES-256-GCM, one exact suite and the endorsed 32-byte key, before library parsing. Evidence gzip is first bounded to the exact **1184-byte** SNP report before the published verifier's internal collection. Fatal streamed UTF-8 and SSE envelope budgets are enforced without collecting an answer. **Ordered role/finish/usage/DONE/clean-EOF receipt semantics are not implemented here**; they belong to packets 04/05. Transport EOF alone must not become successful inference receipt.

Frozen `limits.ts` ceilings: chat **8 MiB**; controls/auth/error **4 KiB**; public evidence **64 KiB**; complete verification bundle **1 MiB**; provenance/manifest **512 KiB**; certificate/VCEK/config **16 KiB**; catalog **256 KiB**; JSON depth **16**, structural nodes **32768**; messages **4096**, designated model count **256**; network chunks **64 KiB**, ciphertext frames **256 KiB**, frames **65536**, stream **64 MiB**; SSE event **64 KiB**, events **65536**; acquisition/verification operation **30 s**, encrypted stream **300 s**, idle/read/header **15 s**, cleanup wait **100 ms**. Provenance additionally permits one signature, one transparency entry and at most 64 inclusion-proof hashes. Catalog field/model-count semantics remain the later typed client parser's responsibility; the lower transport enforces its byte/JSON budgets now. HTTP gzip/deflate/br decoding belongs to Fetch; these are **decoded-byte caps**, not a claim to observe encoded byte counts or bound the browser's native decompressor allocation. Application-controlled copies are bounded; runtime/allocator/RSS and uninterruptible in-flight WebCrypto work are not thereby bounded. Rejected sources are cancelled and operations aborted; cleanup waits are finite.

### Frozen executable graph and artifacts

Scratch root **Q**: `/var/folders/5v/1zw417197236y_k11j994g8r0000gn/T/possums-phase01-channel-6zimy67o`. Final run **E**: `Q/run-ZF65jh`. Installs/caches, full pinned CVM source copy, Go outputs, browser profiles and bundles are isolated there; existing repository `node_modules`, vendor targets and other user outputs were not installation/build targets.

`package-lock.json` freezes **48 package entries**, each with an exact registry URL/version/integrity (including optional esbuild platform binaries); SHA-256 **`cf2d674871ca9ee6bcb4d0af2fd0668537c14c8685dfb360fbebda09d854ebda`**. Direct versions: verifier **1.2.1**, EHBP **0.3.2**, Sigstore browser policy API **0.1.15**, TypeScript **5.9.3**, esbuild **0.25.10**, Playwright **1.61.1**. Other resolved versions: crypto-browser **0.1.8**, tuf-browser **0.1.11**, hpke and hpke-noble **1.1.7**, noble ciphers **2.4.0**, noble curves **1.9.7/2.4.0**, noble hashes **1.8.0/2.4.0**, noble post-quantum **0.7.1**, playwright-core **1.61.1**, optional fsevents **2.3.2**. No CDN/runtime package resolution or lifecycle scripts were used. Two builds after a fresh **offline `npm ci`** produced identical bundle digests.

`E/bundles/resolved-modules.json` lists resolved Node/browser modules, per-file SHA-256, import edges, externals, lock digest and tool identities; its SHA-256 is **`a4e825d29542269a22f65e8991615acd97740d80f89bdae2c626dda11accb7c0`**. Node bundles resolve 145 source modules; browser bundles 146. Conditional noble crypto exports select Node `node:crypto` versus browser WebCrypto. EHBP uses its ESM export; verifier/Sigstore use their published ESM/default exports. Sigstore's dynamic import is bundled. Browser zlib fallback is replaced with a fail-closed stub; `DecompressionStream` is required. Node's only bundle externals are `node:crypto` and `zlib` from the pinned Node runtime. No browser external imports remain. SDK bundle-assembler/latest/retry code and TUF/localStorage support can remain as dormant class dependencies in the artifact; this boundary calls only `verifyBundle` with supplied VCEK and embedded Sigstore roots, never those acquisition/TUF methods. Runtime tests blocked unexpected fetches and storage access; no such fetch/storage calls occurred. This is scoped execution/source review, not an audit proving compromised dependencies cannot exfiltrate.

| Artifact in `E/bundles` | SHA-256 |
|---|---|
| `channel-node.mjs` | `aa5944c5d402b546665838106b6ede828c73a2c475c701480eebe6963d7a9def` |
| `fixture-node.mjs` | `8b06f28c88a34784bff7535646c5a9281608e6aab5591c48cab2c00234fc0355` |
| `channel-browser.mjs` | `f68e6cd7ad256012c2cb6bea683c4e2ba9e2ad59e0ef83f6b19b12eafeaf128e` |
| `fixture-browser.mjs` | `b92a9f36f71101d34d79f5096ece47c76c55abfd3c8879f821e6c138962b1164` |

Actual Node: `/nix/store/lj2m82qrps2pkcg23w3gi209lmaz6ijj-nodejs-slim-24.13.0/bin/node`, **24.13.0**, executable SHA-256 `960db776301a6774a4ca73e19e826c6173d0c70663ead8f13fe5c524a49a52d1`; npm **11.6.2**. TypeScript `_tsc.js` SHA-256 `e8f349eabd48486bdb2bf9dc1a00c89d58297270c54b745838879e2859194419`; executed esbuild Darwin-arm64 binary SHA-256 `921b19d2a6e983de6aa861582476e4f983c8509cb40c462e572ce64ca1bcb5be`. Actual Chromium **149.0.7827.55**, Playwright revision **1228**, executable `/Users/mb5/Library/Caches/ms-playwright/chromium-1228/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing`, SHA-256 `b1b9e2dd063115031f08eadc10ed381ca0fa05b2284baff8f721d87f5f0f61b7`. Its existing installation was read/executed, not modified; profile/artifacts were fresh scratch outputs. These identify the executed binaries, not a whole operating-system/browser-framework measurement.

### Actual shim and qualification execution

Reused CVM commit **`35d94c85ad20e9f98cd4ddcf35dfe4574b5215fb`**, acquiring its complete source archive into scratch to compile the package. The archive SHA-256 is `c3b904240e1d6ae3ae5acf26ebfe21930e7fe6bb78e559e3b1cb4c56a3a11191`. Its original Go manifest/sum still match the hashes in the preceding record. Only the new test file was overlaid into `tinfoil/cmd/shim`; **no shim, observability, EHBP, config or hardware-quote implementation was replaced**.

The Darwin build exposed genuine Linux-only APIs (`Openat2`, SEV configfs); CGO-disabled cross-compilation also cannot compile NVML. Neither was hidden with stubs. Go was resolved through the existing locked flake's **`nixpkgs-unstable.go_1_27`**, not ambient PATH: Darwin `/nix/store/9w6fm5c9v3lx0vxd4pqy7i8qmdfsmq7g-go-1.27.1`, Linux `/nix/store/40wlcfzp30vzja0qnrr2in1y24fvysz8-go-1.27.1`; both **Go 1.27.1**. Linux GCC wrapper `/nix/store/z2qkl46kv87zcs2sfwfjgc75nq0hbzhf-gcc-wrapper-15.3.0` came from that same locked input. Their immutable Linux closures were copied into scratch because Docker does not share host `/nix/store`; no Docker settings were changed. Builds used `-mod=readonly`, isolated module/cache/tmp paths, `GOTOOLCHAIN=local`, `GOPROXY=off`, `GOSUMDB=off` after initial checksum-verified module acquisition. A pre-existing ARM64 Linux image was used only as the local execution substrate, overriding its entrypoint: `sha256:be60aee15997daca475b710b734bc6bfe52cd544dcd7e9fd2ff58210b6747d83`. Compilation had no network; runtime published only host loopback port 18443. Test binary SHA-256: `81433b82070e293cafccdddb4167edb2b0082461718c81b3debc036c84e1a199`.

The fixture invokes actual **`NewShimServer`**, its registered public observability handlers, reverse proxy and pinned EHBP middleware with an in-memory synthetic identity/backend. Shim control-plane auth is explicitly off; the synthetic backend enforces its test bearer (except challenge/session), strips/observes the actual proxy header behavior, checks ambient cookies are absent, and reports only counters. Paths are exactly the five listed fixture API routes; CORS permits only `https://phase01.invalid`; proxies are disabled; proxy response headers time out at 5 s; TLS is at least 1.3; headers at 5 s/16 KiB, read/write/idle at 30 s, fixture lifetime at 120 s. No boot, hardware quote production, platform secret injection or Rust accounting runs in this fixture.

Node trusts only the fixture's generated public CA additionally via `NODE_EXTRA_CA_CERTS`, retaining hostname validation. Chromium's ephemeral context uses a **synthetic-leaf SPKI allowlist** for that localhost certificate and an explicit local-network permission for the isolated test origin; it does not use a blanket `ignoreHTTPSErrors` setting. These are isolated test trust, **not evidence of browser-pinned production TLS**. Service workers are blocked, network routes are restricted, storage calls are trapped, and a planted ambient cookie must not reach the backend. An initial cross-origin browser attempt failed until the local-network permission was explicitly granted; the final run passed. Production artifacts have neither fixture authority nor these runner settings.

Actual commands/results:

- `nix eval --impure --raw --expr 'let f = builtins.getFlake (toString ./.); p = import f.inputs.nixpkgs-unstable { system = builtins.currentSystem; }; in p.go_1_27.outPath'` resolved the pinned compiler. Linux Go/GCC were realized with `nix build --impure --no-link` against the same input, selecting `system = "aarch64-linux"`.
- Reproducible bundle operation, **from a fresh scratch copy of the seven `examples/phase01` files**, with scratch HOME/cache/TMPDIR and the recorded Node/npm executables: `npm ci --ignore-scripts --no-audit --no-fund`, then `node node_modules/typescript/bin/tsc --noEmit`, then `PHASE01_OUT="$SCRATCH/bundles" node build.mjs`. The final run used `npm ci --offline` from the isolated acquired cache; browser downloads were disabled. Never run installation over the repository's user `node_modules`.
- The complete already-resolved local command is `bash /var/folders/5v/1zw417197236y_k11j994g8r0000gn/T/possums-phase01-channel-6zimy67o/run-qualification.sh` (script SHA-256 `542dfa8428f44873de038d747d3d889fc9267c85c01e75f60c94b92f03100fc7`). It makes a fresh run directory, performs offline install/typecheck/two matching builds, overlays/compiles the real shim in isolated Linux, runs focused shim regressions, and invokes `node tests/phase01_transport.mjs` with explicit `PHASE01_BUILD`, `PHASE01_INSTALL`, `PHASE01_EVIDENCE`, `PHASE01_FIXTURE_DIR`, `PHASE01_BROWSER`, scratch TMPDIR and test CA. The source tests document those inputs; acquired public bundle/manifest/VCEK bytes remain in Q. Verification after the administrative approval expires must fail until a separately reviewed policy renewal, not silently change time/approval to rerun a positive control.
- **PASS:** pinned TypeScript check; fresh offline locked install; identical repeated bundles; **52** transport/verifier/browser checks; **34** upstream shim regression test/subtest passes; actual fixture process **PASS**, exit **0**. Evidence/signature corruption (AMD report and Sigstore), malformed/gzip-bomb input, wrong key/config/suite, unapproved tag/digest/domain, API-approval absence and fixture/production separation reject. Verification-negative capture counters are **0 credential sends / 0 content sends / 0 acquisition fetches** for the supplied-bundle tests. A future-clock test exercises the actual pinned AMD chain-validity failure. Local administrative expiry/revoked predicates reject; they are not fabricated revoked-hardware evidence.
- **PASS:** actual Node and Chromium HPKE request encryption, progressive authenticated decryption **before backend terminal release**, ordinary TLS authenticated model GET, fixture observability/key/CORS/path checks, encrypted control responses, unknown/oversized/sparse outgoing input, hostile UTF-8/SSE-event ceilings over real encrypted responses, framing ceiling before library buffering, fragmented oversized evidence with misleading Content-Length, catalog oversize/idle timeout, cancellation and finite cleanup. Actual shim plaintext 422 with no EOF and redirect responses reject promptly; synthetic never-resolving cancellation finishes within the cleanup bound. Post-send network failure, redirect and plaintext error produce exactly one send, no replay/fallback. This is channel evidence, not API admission/settlement or strict terminal receipt evidence.
- `git diff --check` passed. Root Rust/vendor/helper/full browser regressions and physical Android were **not** run in this packet; their code was not changed. The repository-local diagnostics cannot resolve scratch-only packages; the isolated pinned `tsc --noEmit` result is the authoritative typecheck here. No funded inference was needed or attempted.

### Credential-free live results and remaining qualification defect

Direct HTTPS checks used the supplied exact origin, no redirects, empty inherited auth/proxy/cookie environment, normal certificate/hostname validation, numeric response/operation limits and sanitized status/digest output. Initial public no-nonce evidence, certificate and key responses were **200**; OPTIONS was **204**. Sizes/SHA-256 respectively: evidence **605 bytes**, `f8eac9a00b99f537472a0e8066be61e5e038ef38883e4c013798f71f725e7ad9`; certificate wrapper **2039 bytes**, `311408efd3f26ad6e4d71b8029ab19756f7181fd2fa88b3673f9b748fc9679f6`; key config **41 bytes**, `b9e62981249e2edd868b7a43957f345ba9371a15cfce03dc50cb3ff463f19810`. CORS reflected the synthetic origin, allowed the requested authorization/content-type/encapsulated-key headers and exposed the response nonce. These are observations, not attested outer headers.

The final compiled **Node `observeWeb` executed live bounded acquisitions and `qualifyWeb` successfully**, authenticating the approved manifest/provenance/config/measurements, AMD signature/chain and endorsed key/certificate equality. Credential/content sends: **zero**. GitHub provenance came from the fixed digest-specific `api.github.com` URL; its gzip decoding was bounded on decoded reads. AMD collateral came directly from the exact derived Genoa VCEK URL at `kdsintf.amd.com`; its DER was **1347 bytes**, SHA-256 `c1e04c42e0ab065c89207573f29bdeb0a5d93cb998253b43bb88e33a2640375f`. No live API was presumed deployed and no HTTP success/outer status was treated as authenticated EHBP success.

**Genuine remaining browser-bootstrap limitation:** Tinfoil's KDS proxy returned **403 without CORS**; direct AMD KDS returned the valid certificate but **no CORS**. Thus this packet proves browser verification of a supplied bounded bundle and real browser EHBP against the isolated shim, **not autonomous live browser evidence assembly**. The current `observeWeb` acquisition path is qualified in Node only. A later browser integration must use a reviewed bounded CORS-capable collateral/bundle distribution path (the collateral remains cryptographically verified), or explicitly provision the bounded evidence bundle; it may not use `no-cors`, weaken verification, invent a verified flag or silently fall back. No deployed proxy/configuration change was made to hide this gap. This is distinct from—and does not resurrect—the accepted TLS/header/bodyless-GET or v2 nonce caveats.

Revocation/freshness scope: pinned AMD verification checks chain signatures, current certificate validity, Genoa/VCEK/TCB/hardware/policy binding; pinned Sigstore verifies its DSSE/certificate/embedded-root/transparency constraints. **No AMD CRL/OCSP or v2 quote-age/client-nonce/witness guarantee is provided.** Local approval/key denylists are presently empty and require an independently distributed policy update; unit rejection of a revoked predicate is not evidence of an online revocation service or a revoked production quote test. The qualification approval expiry cannot establish quote age. TLS terminators/intermediaries can see ordinary credentials, headers and catalog traffic. EHBP authenticates body frames, not outer status/headers/method. Disconnect/timeout receipt remains uncertain; later API/client work must preserve no replay and delivery-independent accounting.

### Preservation and next boundaries

Seven baseline document bytes/SHA-256, the exact original five-document binary patch, HEAD `4cfa9d390f88fc8cfce3e3a6e141080e12a0b493`, raw index SHA-256 `79cc4cf566ebd937726d5b32cfc7af042a4851ef1fb5638a0db5f725f8ed39aa`, and empty staging were checked before work and rechecked after this appendix. Prior `docs/phase01.md` bytes remain an unchanged prefix. No full-tree inventory, reset, stash, staging, commit, publication, deployment or production configuration change occurred. Existing authored untracked documents, node_modules and vendor/build targets were not overwritten. Focused preservation evidence is in Q, not a new whole-tree claim.

The shim's source-known pre-admission unbounded frame allocation and recursion, native/library allocator overhead, process RSS, driver lifetime and all prior runtime/privacy/invoice UNKNOWNs remain scoped risks. The fixture's container resource limits are test isolation, **not a new 512-MiB proof gate**. Packet 01 does not establish the not-yet-implemented API/accounting/terminal protocol or approve it for production. A new independently approved deployed API identity, separate deployment permission, required deployed/device evidence and a fresh independent implementation review remain later acceptance gates; physical Android is still unknown. Continue only separable safe local Phase 0.1 packets under those boundaries—no Phase 0.2.

---

## 2026-10-03 — packet 01a correction: outbound admission and requalification

**Packet 01a local repair complete; Phase 0.1 is NOT accepted.** This corrects the preceding packet-01 claim that outgoing admission bounded application-created copies before serialization. The prior `serialize` first materialized `Object.getOwnPropertyDescriptors` and `Object.entries`, and chat/control schemas were checked afterward. A 65,540-property input could therefore allocate its complete descriptor map and entry array before rejecting against the 32,768-node ceiling. Zero network sends alone did not establish correct admission. Prior evidence/artifacts above remain history, not evidence that this defect was absent.

Only `examples/phase01/limits.ts`, `examples/phase01/transport.ts`, `tests/phase01_transport.mjs`, and this appended section changed. No manifest, lockfile, compiler/build configuration, approval, shim test, Rust/API, gateway/helper, deployment or production configuration changed. No Sol handoff occurred.

### Repaired boundary and consequential decision

- Fixed chat/message/session/submission schemas now build bounded data-only snapshots **before stringify**. Each property is inspected with its individual descriptor, never by reading a caller accessor. Field sets reject unknown/missing/non-enumerable/accessor fields, malformed values and unsupported types. Symbol fields now reject explicitly rather than being silently omitted. Message arrays require dense ordered indices, at most 4096 entries and no extra fields. Valid frozen data objects work.
- `mapData` uses descriptor-aware source and destination proxies with native `Object.assign`: the source exposes only individually inspected data values; the destination admits/maps each value **before copying it**. This deliberately preserves non-enumerable-field rejection, which a `for-in`-only traversal would miss. There is no application-created whole-object key array, descriptor map or entry array. **The engine still enumerates its native own-key list, potentially proportional to the input width.** This repair is not a bound on native enumeration, allocator/RSS, caller-created inputs or hostile JavaScript/Proxy traps/modified intrinsics. It does not upgrade the existing runtime or shim allocation UNKNOWNs.
- `serialize` charges the shared node/depth and cumulative exact escaped-UTF-8 byte budgets incrementally before admitting each copied value. It stringifies only that bounded snapshot, not the original object or unchecked aliases. Snapshot null prototypes exclude inherited `toJSON` hooks. String accounting includes controls, escaping, multibyte characters, paired and lone surrogates. Chat **8 MiB**, controls **4 KiB**, structural nodes **32768** and depth **16** remain unchanged; the fixed schemas additionally constrain their shapes/field lengths.

### Executed tests and artifacts

Preservation/repair scratch **A**: `/var/folders/5v/1zw417197236y_k11j994g8r0000gn/T/possums-phase01-repair-xwu5_6qi`.
Final qualification **F**: `/var/folders/5v/1zw417197236y_k11j994g8r0000gn/T/possums-phase01-channel-6zimy67o/run-2JpJ12`.

The unchanged isolated qualification harness made fresh scratch installs and production/fixture Node/browser outputs using the exact existing locked graph. Node **24.13.0**, TypeScript **5.9.3**, esbuild **0.25.10**, Playwright **1.61.1**, Chromium **149.0.7827.55** (revision **1228**), verifier **1.2.1**, EHBP **0.3.2**, Go **1.27.1**, Linux compiler/image/source pins and executable identities are unchanged from the preceding record. Graph review compared every module name/hash/import edge, external import and tool identity with `Q/run-ZF65jh`: **only `limits.ts` and `transport.ts` content changed**; Node still resolves **145** modules, browser **146**, with the same conditional exports and fail-closed browser zlib stub. No dependency or build-graph exception was necessary. Lock SHA-256 remains `cf2d674871ca9ee6bcb4d0af2fd0668537c14c8685dfb360fbebda09d854ebda`.

Current `F/bundles/resolved-modules.json` SHA-256: **`f57098f4f6066b56209def566e9ebe347cc6c311b9744ca10087e8bb72d82512`**.

| Artifact in `F/bundles` | SHA-256 |
|---|---|
| `channel-node.mjs` | `980381862c983f45f3c61b20c98b446af19296c054499546dda28438a6257721` |
| `fixture-node.mjs` | `79a4f4a24fedee83a5357f5adaee5a414cbbe62b73b88283a6745e33abc46c5a` |
| `channel-browser.mjs` | `782c9077f3892aa1448470ad684c8e1a9f6b03c5267835b986efd2fcd2e92b70` |
| `fixture-browser.mjs` | `c9e49b2b93f1a6eac851cb3b7a738d86fb1de4da365e688100ab1ea4701474fc` |

Exact complete execution command (unchanged harness SHA-256 `542dfa8428f44873de038d747d3d889fc9267c85c01e75f60c94b92f03100fc7`):

`bash /var/folders/5v/1zw417197236y_k11j994g8r0000gn/T/possums-phase01-channel-6zimy67o/run-qualification.sh`

Its scratch-only build commands were `npm ci --offline --ignore-scripts --no-audit --no-fund`, `node node_modules/typescript/bin/tsc --noEmit`, `PHASE01_OUT="$RUN/bundles" node build.mjs`, and `PHASE01_OUT="$RUN/rebuild" node build.mjs`. The harness fixes the isolated environment, offline cache, pinned Linux compiler/source overlay, no-network compilation, localhost fixture trust and Chromium executable; it invokes `node "$REPO/tests/phase01_transport.mjs"` with the explicit scratch inputs described above. No installation/build targeted existing repository dependencies/artifacts. Both builds' **four bundles and module manifests were byte-identical**. Final test-source SHA-256: `140a30be8d0e957f8c5ca727b989f8c3bdf2009665cb9b1220f80d861b01d414`.

- **PASS:** pinned TypeScript `--noEmit`; **172** transport/verifier/browser checks, including **60 identical admission checks in Node and Chromium**; **34** pinned shim test/subtest passes; actual fixture **PASS**, container exit **0**. Shim binary remains byte-identical at `81433b82070e293cafccdddb4167edb2b0082461718c81b3debc036c84e1a199`.
- Wide regressions use exactly **65,540 own properties** at the chat root, nested message, session and submission boundaries. Public `encodeChat`, `Channel.chat`, `Channel.control` and generic `serialize` paths are exercised. Tests trap whole-object descriptor/key/entry collection, stringify, EHBP request collection/WebCrypto encryption and Fetch; invalid inputs must produce the expected rejection **with every trapped work counter zero**, not merely fail eventually. Schema-wide cases inspect at most 10/16/6 individual descriptors respectively (top/message/each control), not all 65,540. Generic traversal also proves bounded incremental inspection at the structural ceiling. Getter invocations: **zero**.
- Inclusive/exceeded node, depth, 8-MiB chat and 4-KiB serializer byte boundaries, cumulative bytes across siblings, message counts, sparse arrays, array accessors/extra fields, hidden and symbol fields, malformed controls, unsupported types, `toJSON`, valid frozen inputs, exact Unicode/escaping and non-aliasing snapshots pass. Fixed control schemas cannot fill all 4 KiB with valid fields; their smaller field-length checks and oversized public-control rejection are tested separately from the underlying exact 4-KiB serializer boundary.
- Actual pinned verifier negatives and production/fixture separation remain passing, including separation assertions in both runtimes. Failed-verification capture counters remain **zero credentials / zero content / zero acquisitions**. Valid Unicode/escaping chat bodies traverse real EHBP in Node and Chromium; authenticated deltas arrive **before fixture terminal release**. Actual encrypted session/submission responses, allowed TLS model discovery, no replay on redirect/plaintext-422/network failure, no plaintext fallback, caps, deadlines and cleanup retain the prior suite's passing coverage. No strict API terminal/accounting semantics are claimed.
- Regression sensitivity was checked against the historical `Q/run-ZF65jh` bundle: the new admission suite fails at the first wide `encodeChat` case when the old whole-object collection is trapped. `A/admission-only.mjs` extracts the same test function for this local comparison. Initial repair runs exposed a test descriptor-count allowance error (two native-array-length descriptor inspections), then an occupied loopback fixture port after that early failure; both were resolved without changing production code, trust or the harness. A standalone runner also required resolving Node's lazy web globals before installing allocation traps. Final results above are fresh complete runs, not those failed attempts.

Exact final graph/preservation verification command:

`python3 /var/folders/5v/1zw417197236y_k11j994g8r0000gn/T/possums-phase01-repair-xwu5_6qi/verify-repair.py`

`A/verification.json` records matching baseline bytes/hashes for all seven user documents, the exact original five-document patch, unchanged HEAD/index and empty staging, identical prior Phase-0.1 document prefix, source-to-build equality, graph review and repeat-build equality. `git diff --check`, scoped `git diff --no-index --check` against A's before-copies of all four edited files, and `node --check tests/phase01_transport.mjs` passed. Automatic repository-local diagnostics cannot resolve deliberately scratch-only dependencies; the pinned isolated compiler passed against the actual complete copied sources. No dependency installation was used to silence repository-local diagnostics.

### Unchanged acceptance boundaries and unexecuted work

No live endpoint checks, funded inference, new collateral acquisition, root Rust/vendor/helper/full web-browser regressions or physical Android checks ran in this repair. Those are not silently reported passing. The existing approval's **2026-10-08 UTC** administrative expiry was unchanged and had not expired at execution; no clock change or renewal was used for positive qualification. Historical live observations were not refreshed. Accepted ordinary TLS credential/header/catalog visibility and external-v2 nonce/witness/quote-age and revocation limitations remain exactly as disclosed above. Gateway/helper gates are unchanged; production API approval remains **empty**.

Browser collateral/bundle provisioning, packets **02–08**, a fresh independent implementation review, separately authorized new API release/deployment and required deployed/device evidence remain separate unfinished work. The actual router remains web-only. This packet neither absorbs that work nor treats it as a reason to refuse the bounded repair. No staging, commits, publishing, production changes, deployment or Phase 0.2 occurred.

---

## 2026-10-03 — packet 02 shared detached admission/preflight

**Packet 02 implemented and locally verified; Phase 0.1 is NOT accepted.** Only `src/generation.rs` (new), `src/lib.rs`, `src/web.rs`, `src/streaming_chat.rs`, `src/web/resource_streaming_tests.rs` and this appendix changed. `ReservedGeneration` and its interface are unchanged. No API routes/authentication scheme, SSE protocol, reference client, dependency graph, helper/runtime, browser harness or Phase 0.2 work was added.

### Extraction and ownership decisions

- `Generation::submit` is the shared internal boundary: verified gateway evidence → authenticated catalog snapshot → maximum reservation quote → fail-fast generation permit → locked `Auth::admit_submission` → immediate sole `ReservedGeneration` → detached tokenizer/context preflight → synchronous renderer handoff. There is no intervening await or prompt processing between a new reserve and its guard/spawn, and duplicates never construct a guard or invoke tokenizer/handoff. Typed rejections carry only fixed enums/accounting outcomes, not input or provider errors.
- Adapters supply already bounded/validated owned `GenerationInput` and borrowed admission material. The existing web decoder, cookie/CSRF checks, duplicate HTML and status mapping stay in `web.rs`, including explicit insufficient-credit rejection before headers or prompt calls. No new history byte/product cap was added. `PreparedGeneration` moves history/prompt into the renderer without copying histories or buffering an answer; only small web continuation metadata is copied. The ledger retains the submitted rates and maximum; tokenization changes only the model's context-legal output allowance. The original maximum separately reaches the existing continuation disclosure.
- The detached `PreflightInput` envelope is now constructed **before** its future, explicitly dropping input and renderer captures before its sole refund owner. Its result sender drops **after** the owner, so panic/pre-poll cleanup cannot notify an observer before refund. `ChargedPreflight` keeps all work/unclaimed output ahead of the final heavy lease. Successful preflight always transfers the same owner, even after observer loss; the unchanged `spawn_settling` remains terminal accounting authority. The request/result observer has no cancellation authority.
- Middleware admission, raw-body destruction, retained frame/slice leases, four generation lanes, account limits, reset/logout lock order, resource hooks and HTML continuation behavior remain unchanged. Test-only resource hooks now accept borrowed history/prompt instead of a web form. No production hook await was introduced.

### Actual commands and results

Scratch **G**: `/var/folders/5v/1zw417197236y_k11j994g8r0000gn/T/possums-phase01-core-ahma7j89`. Cargo registry/git caches were copied into `G/cargo-home`; all builds use `CARGO_HOME=G/cargo-home` and `CARGO_TARGET_DIR=G/target`, never existing checkout targets. The final runner additionally sets `TMPDIR=G/tmp`. The locked Nix development shell resolved `/nix/store/89rqzskr6m71aqpxrglhyifrszxf3a54-rust-minimal-1.88.0/bin/{cargo,rustc}`: **rustc 1.88.0 (6b00bc388 2025-06-23)**, **cargo 1.88.0 (873a06493 2025-05-10)**, aarch64-darwin. No dependency/lockfile updates or client installs occurred.

Exact final execution (exit **0**):

`bash /var/folders/5v/1zw417197236y_k11j994g8r0000gn/T/possums-phase01-core-ahma7j89/run-checks.sh`

The runner executes these commands under `nix develop -c bash -c`, with the isolated environment above; results are in `G/final-results.log`:

| Command | Actual result |
|---|---|
| `cargo fmt --package possums -- --check` | PASS |
| `cargo check --offline --locked --all-targets --all-features` | PASS |
| `cargo clippy --offline --locked --all-targets --all-features -- -D warnings` | PASS |
| `cargo test --offline --locked --test web --test lifecycle --test auth --test accounting --test trust --test catalog --test resource_ownership --test resource_inputs` | **80 PASS**: web 31, lifecycle 4, auth 5, accounting 11, trust 3, catalog 11, resource ownership 7, resource inputs 8; zero failed/ignored |
| `cargo test --offline --locked --lib -- --test-threads=1` | **87 PASS**, zero failed/ignored; includes 20 generation-owner, 9 streaming-composer, 5 auth-lock/race and 17 web resource tests |
| `cargo test --offline --locked --lib web::resource_streaming_tests::combined_resource_gate -- --exact --test-threads=1 --nocapture` | **Exactly 1 PASS**, 86 filtered; fresh process, 8.65 s |
| `git diff --check` | PASS |

The two new `shared_preflight_*` tests also passed alone (2 selected, 85 filtered). They exercise the actual shared core without HTML: original allocation transfer and price/max snapshot after catalog repricing, duplicate rejection without handoff/tokenization, observer loss while tokenizer is held, detached authenticated-format completion/settlement; and runtime shutdown before the first preflight poll, with captured input destruction before leases, zero prompt calls and full refund. Existing tests freshly passed for observer loss during tokenizer/before compose/after compose, reset/logout races, deadlines, panic, context failure, duplicates/restart, trust/catalog/credit rejection with zero tokenizer/generation calls, raw/body/frame/slice retention and exactly-once absorbing settlement/refund. These are local synthetic providers/loopback evidence, not deployed fault injection.

**Check limitation:** the initial `cargo fmt --all -- --check` failed on pre-existing formatting in unchanged `vendor/tinfoil/`; its chained check/clippy commands did not run in that invocation. No vendor formatting was changed. The root-package fmt plus all-target/all-feature check/clippy above subsequently passed. The unchanged vendored `CheckpointSignature::encode` dead-code warning remains. Automatic edit diagnostics reported Rust clean; dedicated LSP navigation/diagnostics tools were not exposed, so pinned compilation/tests are the verification evidence.

The isolated resource run reported a **119,241,028-byte requested-allocation lifetime peak** for its exercised workload. This is not an RSS or worst-case bound, not renewed proof of the 512-MiB target, and does not close existing shim pre-admission allocation, upstream-driver lifetime, platform/privacy or provider-invoice UNKNOWNs.

### Preservation and remaining boundaries

`python3 /var/folders/5v/1zw417197236y_k11j994g8r0000gn/T/possums-phase01-core-ahma7j89/verify-preservation.py` checks all seven original document bytes and baseline-manifest hashes, the exact original five-document patch, unchanged HEAD/index, empty staging, this record's prior prefix, unchanged client sources/locks/tests and the permitted tracked scope. It passed before and after this appendix; `G/preservation.json` records results. HEAD remains `4cfa9d390f88fc8cfce3e3a6e141080e12a0b493`, index SHA-256 `79cc4cf566ebd937726d5b32cfc7af042a4851ef1fb5638a0db5f725f8ed39aa`. Existing user dependency/artifact trees were not build/install targets or modified by this work; no recursive inventory, staging, commits or deployment occurred.

No full all-target Rust **test** matrix, standalone vendor/helper/Go, browser/client-channel qualification, live/funded endpoint or physical Android checks ran here; all-target compilation is not execution of those suites. There was no implementation scope deviation; the vendor-wide fmt limitation is recorded rather than repaired outside scope. Packets 03–08, including new API/client protocol work and fresh independent implementation review, remain separate. Sol's frozen browser harness/documentation remain later separable work. Production API approval/deployment/device evidence remain absent; accepted TLS visibility and external-v2 freshness caveats are unchanged and are not blockers to the next bounded local packet.

## Direct continuation — local streaming API/client gates (2026-10-03)

This supersedes the unfinished-packet status above, not its historical evidence.
Workflow `0c4a9950` failed on an agent SSE-header timeout; implementation and
verification then continued directly, without another implementation delegation.
The saved `/Users/mb5/.pi/agent/workflows/orchestrated-build.yaml` now permits one
initial planning critique and one revision only. Unresolved planning findings
stop implementation; the later implementation/review loop is unchanged. YAML
parsing and structural assertions passed; no new workflow was launched.

### Implemented locally

- `src/api.rs` exposes challenge/session creation and revocation, authenticated
  live model discovery, model-bound submission issuance, and streaming-only
  `POST /v1/chat/completions`. Cookie authentication is not accepted for the API.
- Chat uses the same detached `Generation::submit` preflight/owner/accounting
  machinery as the existing web flow. Maximum reservation precedes every
  prompt-bearing tokenizer/inference call; context limits, session epochs,
  transient concurrency, duplicate outcomes and original-price settlement remain
  enforced. All supported chat models stay visible; an unaffordable selection
  returns `insufficient_credit` without substitution or credit-driven output caps.
- `src/api_stream.rs` delivers bounded progressive SSE. Successful settlement
  precedes finish/usage/`[DONE]` markers. Disconnect/backpressure detaches delivery,
  not accepted inference or settlement. Upstream failure/missing or invalid usage
  refunds once; no text-based usage estimate or complete-response fallback exists.
- The Tinfoil streaming adapter retains authenticated `stop`/`length` finish
  metadata. Unqualified inference implementations fail closed for API completion.
- `examples/phase01/client.ts` provides the pinned reference flow and an ordered,
  bounded SSE parser. It accepts a genuine opaque `Channel`, snapshots verification
  inputs, rechecks installed approval before sends, retains only memory session
  tokens, refreshes discovery, and never automatically replays a generation.
  A receipt requires authenticated EOF after ordered settled usage. Fixed codes,
  not raw provider errors, communicate failures. Caller/UI retention is separate.
- The intentionally narrow protocol accepts text system/user/assistant messages,
  `stream: true`, a submission token, optional `n: 1`, and optional
  `stream_options: { include_usage: true }`. Tool calls/results, multimodal input,
  buffered requests, unknown fields and output-cap overrides are rejected. No
  model's tool capability or general OpenAI/Pi drop-in compatibility is advertised.

### Fresh executed gates

| Gate | Actual result |
|---|---|
| Root-package fmt; locked/offline all-target/all-feature clippy with warnings denied | PASS |
| Locked/offline all-target/all-feature Rust test execution | **254 PASS, 0 failed; 3 live tests deliberately ignored** |
| API chat integration cases | **6 PASS**, included above: progressive delivery/original rate, duplicate outcomes, disconnect/backpressure settlement, upstream refund, preflight cancellation, trust/catalog/credit/context/stale-token gates, strict input and restart behavior |
| Published verifier + actual pinned Go shim transport qualification | **172 PASS**; failed verification sent **0 credentials and 0 content**; no automatic replay |
| Upstream shim regressions | **34 test/subtest PASS** (7 top-level suites) |
| New client catalog/ordered-SSE cases | **30 Node + 30 Chromium PASS**, including forged-channel rejection, UTF-8 splits, malformed/duplicate fields, missing/reordered terminal usage, cancellation and zero-network unapproved API factory |
| Actual EHBP/shim → Rust API → synthetic inference | PASS in **Node and Chromium**: progressive protected output, final usage/charge, unaffordable Kimi visibility and refusal with no extra tokenizer/inference calls, safe DOM text rendering, no browser session storage |
| Existing no-JavaScript web browser acceptance harness | **PASS, exit 0** |
| Locked scratch TypeScript checks and repeated Node/browser bundle builds | PASS; identical rebuild reports |

Rust evidence is `G/direct-final-all.log`; existing web-browser evidence is
`G/direct-web-browser.log`. Final verifier/shim artifacts are
`/var/folders/5v/1zw417197236y_k11j994g8r0000gn/T/possums-phase01-channel-6zimy67o/run-mdWNQ3/`
(`test.log`, `shim-regressions.log`, `build.json`, `rebuild.json`). The final combined
API/client artifact directory is `/tmp/possums-phase01-api-fjk2JE/`, with
`typescript.log`, `build.log`, `api.log` and separate production/fixture bundles.
Local test startup was stabilized with an explicit backend/proxy readiness check;
shim tests sharing port 18443 must run serially. Earlier failed attempts are not
promoted to passing evidence.

### Scope and acceptance still open

**Phase 0.1 is not accepted, and Phase 0.2 has not started.** No new measured API
image/provenance release, deployment, production configuration change, or funded
live API inference was performed. `API_APPROVALS` remains empty and fails closed;
the independently approved `v0.0.8` web identity is not authority for this new API.
Fresh API release/channel and real upstream runtime evidence require a separately
authorized rollout and independent approval.

The browser transport spike executes in actual Chromium on macOS, including the
real shim/API path and safe progressive rendering. It is **not physical Android
execution**. Browser verification of supplied real v2 collateral remains scoped
web-observation evidence; autonomous browser collateral acquisition remains
unqualified (AMD KDS CORS/Tinfoil proxy constraints). No synthetic fixture trust
or test TLS-leaf override belongs in production.

The accepted contract remains ordinary HTTPS for outer authorization/discovery
(TLS intermediaries can see them), and attested-key EHBP for this reference
client's prompt/stream bodies. Arbitrary API callers do not prove reference-client
verification/encryption. External v2 does not add v3 nonce/witness freshness,
quote-age, AMD CRL/OCSP, invoice accuracy, whole-process RSS or runtime-compromise
retention guarantees. Existing resource tests are scoped workloads, not renewed
proof of the full 512-MiB target or unresolved shim/driver/platform properties.

All seven original user documents and their exact original five-document patch
remain unchanged. Existing root `node_modules/` and vendored target trees were
not install/build targets. Only the new client-local dependency directory was
installed; root Cargo work stayed in scratch G. Nothing was staged or committed.
`examples/phase01/README.md` describes the local protocol and honest limitations.

## Authorized release preparation — v0.0.9 (supersedes local-only status above)

The user authorized feature-only commits, publication, deployment, independent
API identity approval and funded live API checks. Android is explicitly outside
this acceptance scope. Phase 0.2 remains stopped.

- API commit `f9383a8` and reference-client/qualification commit `200a69b` were
  pushed without including any original user document changes.
- Linux check run `37165272700` passed all four jobs: flake, browser, image and
  image-startup. Independent source review found no concrete release blocker;
  that review did not inspect a live rollout or rerun these tests.
- Image publication run `37166197125` reproduced the same OCI digest twice:
  `sha256:45395fad53d7df0390da1cbed2a2cf46202a5a566fbc45694ba434dbcc41a22b`.
  Published-image inspection confirmed user `65532:65532`, working directory
  `/tmp`, and absence of the two checked local credentials. This is a bounded
  artifact scan, not proof of runtime privacy or all-secret absence.
- Digest-only PR #8 passed check run `37166594184`, merged to
  `d9ccbf0de0a9b4de73d2d465dfd6e14424253990`, and is tagged `v0.0.9`.
  Preparation `37167116982` and measured publication `37167124305` succeeded.
- Published manifest SHA-256:
  `8c819d1b26f857d45e9aa9ccb7915cebc4448c3111d94707c094af232a17e28f`.
  Its measured config SHA-256 is
  `36fc7ff23870d3a08428246f903f42f7fea242b7b6c723420e03b4d722f266b4`;
  the config pins the independently reproduced image and CVM `v0.14.12`.
- Independent `gh attestation verify` passed with repository, publication
  workflow, exact tag and immutable source digest constraints. The signed
  predicate equals the published manifest byte-derived JSON value.

Evidence is in `/tmp/possums-phase01-release-tzhk3q5b/`, including
`api-release-policy.json`, `v0.0.9/provenance-verification.json` and the reproduced
image artifacts. The policy file is a candidate release record, **not an installed
API approval**.

The initial CLI/control-plane mismatch was resolved using the public dashboard's
actual update protocol: POST `/api/containers/{id}/update/plan`, then `/update`
with `hold: true` and `mark_latest_release: false`. The read-only plan confirmed
blue/green replacement, unchanged resources/variables/secret and SSH references,
retained volume data, hold availability and no downtime. The held candidate was
then promoted with `/update/promote`; no new parallel product replica was added.

## Scoped Phase 0.1 technical acceptance — serving v0.0.9

The actual pinned JavaScript verifier authenticated the held candidate on port
4443, then independently rechecked the promoted public origin. Both checks
verified hardware evidence, approved provenance/measurements, certificate
endorsements and HPKE-key configuration **before any API credential/content send**.
Their reports record zero sensitive sends. `API_APPROVALS` now pins this new API
identity, not the historical web approval, with administrative expiry
2026-10-11T00:00:00Z. Expiry is not v2 quote freshness.

The initial anonymous challenge attempt failed before sensitive sends. A later
login succeeded, but catalog discovery failed closed before any inference prompt:
GitHub still marked `v0.0.8` latest, while the measured helper's unversioned
repository verifier uses latest-release discovery. After the independently
verified `v0.0.9` was explicitly marked latest, `/attestation` returned 200 and
catalog/inference gates succeeded. No verifier or reservation check was bypassed.

### Executed release/runtime gates

- Serving status: running `v0.0.9`, debug disabled, confidential mode enabled,
  no pending update. The existing measured image/config remained unchanged.
- Funded API canaries discovered **7 models**, decrypted progressive text,
  validated an ordered final settled-usage receipt and authenticated EOF, and
  observed terminal duplicate outcome `settled` without replay.
- A distinct canary disconnected after receiving the first protected frame;
  explicit old-token diagnostics subsequently observed `settled`. This proves
  the sampled gateway outcome after disconnect, not guaranteed client receipt,
  a provider invoice, or direct observation of every upstream byte.
- The funded account also successfully settled a distinct catalog quote above
  five dollars. It is therefore **not evidence of a $5 insufficient-credit
  refusal**. No balance was deliberately drained. The exact $5/expensive-model
  rejection and zero tokenizer/inference calls remain deterministic local
  negative evidence; live insufficient-credit refusal is **not exercised**.
- One separate reference-flow canary became uncertain at the original 15-second
  read-idle cutoff. It was not replayed. Client-only commit `ee0447e` removes
  that shorter cutoff for streaming reads while retaining the five-minute
  absolute deadline and bounded control/header waits. Distinct later canaries
  exercised the actual `ReferenceClient.verified/login/models/chat` flow in
  **Node and Chromium**, with progressive output and authenticated receipts;
  Chromium used supplied collateral, plain-text DOM and empty cookies/local/
  session storage. No browser trace, prompt/answer or credential was exported.
- After this client-only correction: locked TypeScript checks and identical
  repeated bundle reports passed; client parser/timeout/negative checks passed
  **31 Node + 31 Chromium**. Fresh actual-verifier/shim qualification passed
  **172 checks**, including **0 credential/content sends on failed verification**
  and no automatic replay. Fresh real-shim → Rust API synthetic qualification
  passed in Node/Chromium. Existing Linux gateway CI and 254 local Rust tests
  remain the release-code evidence; no Rust/application code changed in this
  correction.

Release workspace evidence: `candidate-verification.json`,
`primary-verification.json`, `update-plan-safe.json`, `promotion-safe.json`,
`live-api-gates.json`, `credit-check.json`, `reference-live.json`, and the approved
bundle hash reports. Fresh transport artifacts are
`/var/folders/5v/1zw417197236y_k11j994g8r0000gn/T/possums-phase01-channel-6zimy67o/run-AH3Ffc/`;
fresh combined API/client artifacts are `/tmp/possums-phase01-api-MN4JcD/`.
Final Linux CI `37172373997` at client/approval commit
`7a43c0a66a4b22ea3e4fa6a4515e6b4d33420242` passed all four jobs: flake, browser,
image and image-startup. A final public-origin pinned verification also passed
with zero credential/content sends; GitHub latest is `v0.0.9`.

### Acceptance scope and remaining unknowns

**The scoped Phase 0.1 technical gates pass under the user's accepted v2/HTTPS
contract and Android exclusion. Phase 0.2 is not started.** Text-only streaming,
all-model discovery, fail-closed approval/key binding, no replay, and shared
accounting/context/resource invariants are supported by the release, local
negative matrix and selected live canaries. Upstream-error/invalid-usage refunds,
reservation races, stale tokens and restart behavior remain covered by local
failure tests, not injected live faults in this rollout.

Tools/multimodal/general Pi or OpenAI SDK compatibility are not supported or
advertised. Autonomous browser collateral acquisition, physical Android,
nonce/witness freshness for this v2 external client, quote-age/revocation coverage,
provider invoices/maximum upstream cost, runtime compromise, mount/egress/cache/
platform logging enforcement and whole-process RSS remain unverified or outside
scope. Existing provider reliance and availability/operator-cost risks remain
explicit; this is not a full privacy audit or a new 512-MiB proof. Recovery grants
and prompt-free reservation/idempotency state remain transient, not durable paid
accounting. A deliberate new request after uncertainty can incur another charge.

All seven original user documents and the original tracked-document patch remain
unchanged; only implementation-owned files were committed. Client fixes and the
release-approval/evidence record are separate logical commits.
