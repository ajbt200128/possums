# Phase 0 verification record

Status values are **VERIFIED**, **FAILED**, and **UNKNOWN**. An UNKNOWN or FAILED mandatory gate blocks production prompt transmission and release.

## Evidence checklist

| Claim | Status | Required evidence |
|---|---|---|
| Tinfoil Rust inference verification API and cache controls | UNKNOWN | Versioned API review and instrumented no-prompt-on-failure tests |
| Upstream SEV-SNP attestation and release provenance | UNKNOWN | Valid and invalid live quotes/releases |
| Serving HTTPS key is bound to gateway attestation | VERIFIED | `v0.0.5` fresh quote verification matched the endorsed and serving TLS SPKI fingerprints |
| Live catalog authentication, rates, and units | VERIFIED | The deployed authenticated UI rendered all seven strictly validated models; production-schema and negative tests cover rates and units |
| Per-model tokenizer/context and authenticated usage semantics | UNKNOWN | Provider specification and boundary vectors for every model |
| Gateway self-attestation API | VERIFIED | The deployed `/attestation` route returned fresh gateway evidence and an upstream verification document after independent `v0.0.5` quote verification |
| OCI build reproducibility | VERIFIED | Publish run 36516962509 produced matching archives in two independent clean Linux jobs |
| OCI digest equals deployed measured configuration | VERIFIED | Release `v0.0.5`, image digest `sha256:cd8075a02ebcfec9c04973196bfee5cf81b4aaf0a693d20c4130cbba41e1946c`, manifest digest `3a54f1ca9a11b9e60b66e4be80887c2129dbdb9f81c6e864e5aac8a429bf4f8f`, and the live quote agree |
| Root/mount/cache/core-dump behavior | UNKNOWN | Runtime mount, write, process-limit, and crash inspection |
| Proxy/platform request logging is disabled | UNKNOWN | Platform configuration plus canary inspection |
| Egress policy is enforced | UNKNOWN | Measured policy and blocked-destination probes |
| Honeycomb US seven-day retention/access controls | UNKNOWN | Account-side configuration evidence and expiry observation |
| Collector exports only non-identifying aggregates | UNKNOWN | Captured export with seeded sensitive canaries and sparse buckets |
| TLS termination and host observability boundary | UNKNOWN | Deployed network architecture and operator/platform documentation |
| No SDK/inference cache persistence | UNKNOWN | Runtime cache inspection and upstream configuration evidence |

## Local verification

Run on 2026-09-26, aarch64-darwin, with the pinned Rust 1.88.0 Nix shell:

| Check | Result |
|---|---|
| `cargo fmt --all -- --check` | VERIFIED |
| `cargo clippy --all-targets --all-features -- -D warnings` | VERIFIED |
| `cargo test --all-targets --all-features` | VERIFIED: 41 tests passed |
| `cargo test --test transport` | VERIFIED: total-body trickle deadline, total-header deadline, and oversized-header rejection |
| `cargo test --test inference --test accounting --test web` | VERIFIED: 22 tests passed, including chunked/absent/oversized/stalled upstream bodies, shared-memory response-lifetime admission, hostile render expansion, unconsumed-response abandonment, disconnect/completion boundary races, panic, settlement/refund races, and reauthentication replay |
| `npm run test:browser` | VERIFIED: real Chromium with JavaScript disabled completed login, recovery download, model selection, and two chat turns without remote requests |
| `go test ./...` in `attestation-helper` | VERIFIED: nonce-bound Unix-socket request and fail-closed socket/input behavior |
| `actionlint .github/workflows/*.yml` | VERIFIED |
| `nix flake check --print-build-logs` | VERIFIED for aarch64-darwin checks, including the Go helper; other systems omitted |
| `nix build .#gateway-image --print-build-logs` | Not exposed on aarch64-darwin; the OCI derivation is Linux-only |
| Linux OCI build and digest comparison | UNKNOWN: no Linux builder was available |

Crane is pinned to v0.21.0, compatible with the pinned nixpkgs 25.05; lock evaluation no longer emits the prior crane/nixpkgs compatibility warning. CI must report unavailable platform checks as blocked, never silently skip them.

### Opt-in live Tinfoil test

`tests/live_tinfoil.rs` uses the exact production attested client to fetch and validate the live catalog, authenticate tokenization, and make one fixed 64-token-maximum canary generation. It is ignored by default because it requires a funded credential and incurs upstream cost. Copy `.env.example` to the Git-ignored `.env`, set mode `0600`, populate `TINFOIL_API_KEY`, and run `nix develop -c cargo test --test live_tinfoil -- --ignored`. The test never prints the credential or sends user content. It passed against the live service on 2026-09-29.

## Deployed verification

Release `v0.0.5` was deployed on 2026-09-29 to `possum-phase0.possums.containers.tinfoil.dev`. Independent Tinfoil verification returned a fresh AMD SEV-SNP quote, the expected custom release predicate and manifest digest, matching endorsed/connection TLS-key fingerprints, and status `ok`. GitHub exposes one artifact attestation for the release manifest digest.

An authenticated no-JavaScript production canary then loaded all seven catalog models, counted input tokens through `/v1/chat/completions/input_tokens`, completed a bounded `gpt-oss-120b` generation, and rendered the expected marker with HTTP 200. Replaying the same submission token returned the terminal "already completed" notice without regeneration, demonstrating that complete response-body consumption reached settlement. The public `/attestation` route also returned HTTP 200 with both gateway evidence and the upstream verification document. This canary verifies the happy path, not every model's tokenizer semantics or deployed fault injection.

## Independent correctness and security review

The request path was traced in this order: expiring session/CSRF and issued submission-token validation; input parsing; verified gateway evidence; authenticated catalog; maximum-cost reservation; authenticated token count and context quote; bounded generation; prospective authenticated-usage charge; escaped/sanitized render; response-body completion and settlement. Tests prove gateway-evidence and reservation failures do not invoke prompt tokenization or generation, and malformed catalog/context failures do not invoke generation.

Review findings and current dispositions:

- The production gateway invokes a measured helper that requests a fresh random nonce-bound v3 quote from `/tinfoil/attestation.sock`. The pinned Tinfoil Go verifier authenticates quote hardware, signed release provenance, freshness witnesses, and the endorsed TLS-key fingerprint; missing, malformed, stale, oversized, or unverifiable evidence fails closed before prompt tokenization. Local tests cover socket request shape and fail-closed evidence handling. Independent `v0.0.5` verification matched the endorsed key to the public serving certificate, and the deployed helper returned gateway evidence successfully.
- Tinfoil's live `/v1/models` response uses an OpenAI-style `data` array, chat endpoint capabilities, context windows, and decimal USD-per-million-token prices; it does not carry a catalog issuance timestamp or separate maximum-output field. The gateway strictly adapts that production schema over the attested, endpoint-key-bound channel, uses remaining context as the output limit, and reserves the most expensive full-context allocation with fixed-point arithmetic. The authenticated `/v1/chat/completions/input_tokens` contract and one model's generation/settlement path passed both the opt-in test and deployed canary. Tokenization necessarily receives prompt content after reservation; every model's tokenizer semantics remain UNKNOWN.
- Tinfoil Rust SDK revision `34157e4` verifies attestation/provenance and pins TLS. Its documented `user_cache_secret` scopes caches but does not disable upstream persistence. The gateway uses a fresh random scope per generation and raw pinned requests, but actual cache retention remains UNKNOWN.
- Settlement occurs when the complete response body is consumed by the HTTP transport. Reservation ownership remains attached to the body: cancellation, panic, body error, or abandonment before completion refunds, while complete consumption settles the authenticated usage exactly once and releases the unused reservation. Local unconsumed-response drop, socket-disconnect, and settlement/refund race tests cover the boundary. Transport completion is not proof of browser receipt, and a later disconnect may still charge the user. Deployed write-failure injection remains required.
- Upstream response bodies are incrementally capped under one send-and-body deadline. Local HTTP peers test chunked and absent lengths, declared and streamed oversize bodies, and a body stalled after headers. The application server enforces connection admission, header count/buffer/deadline, and an 8 MiB body under one total 30-second deadline. A 512 MiB shared fail-fast application budget reserves a conservative 128 MiB envelope before each body is consumed and holds it through response drop, limiting admission to four requests. Rendering preflights hostile JSON/HTML expansion before allocation and caps output at 32 MiB. Concurrent maximum-body and hostile-expansion tests verify overload rejection and permit release. These envelopes cover application-controlled request parsing, upstream buffers, escaped rendering, and outstanding responses, but allocator/library overhead and whole-process memory require runtime measurement.
- `tinfoil-config.yml` pins the independently reproduced production image digest. The release-only publish workflow builds in two independent jobs, compares OCI manifest digests, publishes the exact first archive with digest preservation, and opens a config-digest pull request. A separately built CI-only fixture image starts with a runtime-injected account secret under a read-only root, memory-only `/tmp`, and no network; its test verifier is not included in the production image. Publish run 36516962509 and merged PR #3 established the `v0.0.5` digest chain.
- The deployed config selects CVM v0.14.12, enables the measured attestation socket, and passes its fixed path to the gateway; live gateway evidence proves socket access. Actual mounts, egress rejection, core/process limits, shim logs, and host observability remain UNKNOWN pending direct runtime inspection.
- Telemetry has no application exporter enabled. The current scaffold uses allowlisted lifetime counters and sparse-value suppression, not time buckets; it must remain disabled until real time bucketing plus resource/datapoint sanitization and Honeycomb region, retention, access, and end-to-end canary evidence exist.

## Prior finding disposition

| Finding | Local disposition | Regression evidence |
|---|---|---|
| Terminal accounting state exhausted capacity | Terminal tombstones are retained through token validity and reclaimed only after expiry | `tests/accounting.rs::terminal_state_is_reclaimed_only_after_token_expiry` |
| Unbounded credential work | Encoded credentials over 128 bytes are rejected before decoding or hashing | `tests/auth.rs::rejects_short_oversized_or_wrong_credentials` |
| Missing connection/header/total-body limits | Added zero-queue connection admission, shared fail-fast memory envelopes held through response drop, HTTP/1 header limits/deadline, one total body deadline, pre-render expansion checks, and a rendered-response ceiling | `tests/transport.rs`; `tests/web.rs::shared_memory_admission_is_fail_fast_and_held_until_response_drop`; `tests/web.rs::concurrent_maximum_bodies_are_bounded_and_release_memory_on_drop`; `tests/web.rs::oversized_request_body_is_rejected_before_upstream_calls` |
| Recovery download absent | Added authenticated no-store plain-text attachment route | `tests/web.rs::recovery_download_is_authenticated_and_never_cached` |
| Manual delivery confirmation burdened every turn | Complete response-body consumption settles automatically; abandonment before completion refunds | `tests/web.rs::no_javascript_chat_sets_security_headers_and_settles`; `tests/web.rs::dropping_unconsumed_response_refunds_reservation` |
| Deployment startup variables/hardening absent | Proposed config supplies inference and gateway repositories, enables the Tinfoil attestation socket, and declares read-only/memory-only/process/egress controls; image entrypoint disables core dumps. Live platform behavior is not yet validated | CI-only isolated startup smoke plus helper socket tests; deployed behavior remains UNKNOWN |
| Incompatible Nix pins | Crane pinned to v0.21.0 for nixpkgs 25.05 | `nix flake metadata --no-write-lock-file` |
| Independent image comparison absent | The release-only publish workflow builds in two independent Linux jobs, compares OCI manifest digests, publishes with digest preservation, and opens a digest-pin PR | `.github/workflows/publish-image.yml`; successful release evidence still required |
| Upstream body boundary evidence absent | Local HTTP peers cover chunked, absent/oversized lengths, streamed oversize, and stalled bodies under the production collector | `tests/inference.rs` |
| Lifecycle fault evidence absent | Local cancellation, socket disconnect, panic-after-reservation, expiry, settlement/refund race, and replay-after-reauthentication tests preserve full refunds and zero prompt calls where required | `tests/web.rs`; `tests/accounting.rs`; `tests/lifecycle.rs` |
| Browser/deployed privacy evidence absent | A pinned Playwright test exercises the complete JavaScript-disabled local browser flow and rejects hostile-output network attempts. Seeded startup errors emit no canary and create no working-directory artifacts; the production binary suppresses panic-hook payloads before serving. Deployed runtime artifacts, platform logs, and production canaries remain release blockers | `tests/browser.mjs`, `examples/browser_fixture.rs`, and `tests/privacy.rs`; external rows above remain UNKNOWN |

## Independent release procedure

1. Run `Publish reproducible gateway image` from the `main` branch.
2. Confirm its two clean Linux builds produced the same OCI digest and that the exact archive was published to GHCR.
3. Review and merge its digest-pin pull request after `check` succeeds on that branch.
4. Run `Tinfoil Release` with the next `vX.Y.Z`; confirm the measured release and Sigstore record were published from the tagged commit.
5. Confirm the public config digest, registry digest, and measured image/config association agree; distinguish the OCI digest from the hardware measurement.
6. Verify release provenance, a fresh hardware quote, and the serving HTTPS endpoint-key binding.
7. Fetch and authenticate a fresh catalog quote without sending a prompt.
8. Exercise every pre-prompt failure gate and observe zero prompt bytes upstream.
9. Inspect runtime mounts, cache paths, egress, core dumps, proxy/platform logs, and telemetry captures with seeded canaries.

Deployment, attestation, authenticated catalog, token counting, generation, rendering, and successful settlement have been verified for `v0.0.5`. Claims that remain UNKNOWN above are not implied by that happy-path canary and must not be advertised as guarantees.
