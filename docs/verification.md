# Phase 0 verification record

Status values are **VERIFIED**, **FAILED**, and **UNKNOWN**. An UNKNOWN or FAILED mandatory gate blocks production prompt transmission and release.

## Evidence checklist

| Claim | Status | Required evidence |
|---|---|---|
| Tinfoil Rust verification API and cache controls | UNKNOWN | Versioned API review and instrumented no-prompt-on-failure tests |
| Upstream SEV-SNP attestation and release provenance | UNKNOWN | Valid and invalid live quotes/releases |
| Serving HTTPS key is bound to gateway attestation | UNKNOWN | Platform quote, endorsed key, TLS endpoint comparison |
| Live catalog authentication, freshness, rates, and units | UNKNOWN | Signed responses and schema/negative tests against production |
| Per-model tokenizer/context and authenticated usage semantics | UNKNOWN | Provider specification and boundary vectors for every model |
| Gateway self-attestation API | UNKNOWN | Quote from deployed measured image and independent verification |
| OCI build reproducibility | UNKNOWN | Two independent clean builds with identical digest |
| OCI digest equals deployed measured configuration | UNKNOWN | Public config, registry digest, and platform measurement chain |
| Root/mount/cache/core-dump behavior | UNKNOWN | Runtime mount, write, process-limit, and crash inspection |
| Proxy/platform request logging is disabled | UNKNOWN | Platform configuration plus canary inspection |
| Egress policy is enforced | UNKNOWN | Measured policy and blocked-destination probes |
| Honeycomb US seven-day retention/access controls | UNKNOWN | Account-side configuration evidence and expiry observation |
| Collector exports only non-identifying aggregates | UNKNOWN | Captured export with seeded sensitive canaries and sparse buckets |
| TLS termination and host observability boundary | UNKNOWN | Deployed network architecture and operator/platform documentation |
| No SDK/inference cache persistence | UNKNOWN | Runtime cache inspection and upstream configuration evidence |

## Local verification

Run on 2026-09-24, aarch64-darwin, with the pinned Rust 1.88.0 Nix shell:

| Check | Result |
|---|---|
| `cargo fmt --all -- --check` | VERIFIED |
| `cargo clippy --all-targets --all-features -- -D warnings` | VERIFIED |
| `cargo test --all-targets --all-features` | VERIFIED: 19 tests passed |
| `nix flake check --print-build-logs` | VERIFIED for aarch64-darwin checks; other systems omitted |
| `nix build .#gateway-image --print-build-logs` | Not exposed on aarch64-darwin; the OCI derivation is Linux-only |
| Linux OCI build and digest comparison | UNKNOWN: no Linux builder was available |

The pinned crane revision warns that it expects nixpkgs 26.05 or newer while the lock uses 25.05. A local `nix flake update nixpkgs` attempt was blocked by execution policy and did not run, so compatibility outside the checks above remains UNKNOWN. CI must report unavailable platform checks as blocked, never silently skip them.

## Independent correctness and security review

The request path was traced in this order: expiring session/CSRF and issued submission-token validation; input parsing; verified gateway evidence; authenticated catalog; maximum-cost reservation; authenticated token count and context quote; bounded generation; prospective authenticated-usage charge; escaped/sanitized render; authenticated delivery acknowledgment and settlement. Tests prove gateway-evidence and reservation failures do not invoke prompt tokenization or generation, and malformed catalog/context failures do not invoke generation.

Release blockers found in review:

- Gateway evidence parsing is explicitly non-authoritative. Production uses an unavailable verifier and therefore sends no prompts until a documented platform adapter cryptographically verifies quote freshness, expected release provenance, and the live serving TLS key. Strict test verifiers cover fabricated, stale, wrong-release, and wrong-key evidence; real platform integration and evidence remain required.
- The assumed authenticated `/v1/models` pricing/freshness schema and `/v1/tokenize` contract have not been verified against Tinfoil production. Tokenization necessarily receives prompt content after reservation; model-specific tokenizer semantics remain UNKNOWN.
- Tinfoil Rust SDK revision `34157e4` verifies attestation/provenance and pins TLS. Its documented `user_cache_secret` scopes caches but does not disable upstream persistence. The gateway uses a fresh random scope per generation and raw pinned requests, but actual cache retention remains UNKNOWN.
- Settlement now requires a bounded authenticated no-JavaScript acknowledgment after the response is displayed. Cancellation before handoff refunds through a drop guard, and unconfirmed reservations expire conservatively. Deployed write-failure, disconnect, and expiry fault injection remains required.
- Upstream response bodies are incrementally capped under one send-and-body deadline, including chunked or absent-length responses. Inbound header/body deadlines, aggregate process memory, read-only root, memory-only writes, core dumps, egress, shim logs, TLS termination, and host observability still require serving-shim and deployed runtime evidence.
- `tinfoil-config.yml` intentionally contains a zero placeholder image digest. It cannot deploy until two independent Linux OCI builds agree and the real digest is pinned.
- Telemetry has no application exporter enabled. The optional collector allowlist and local sparse-bucket logic are tested, but Honeycomb region, retention, access, and end-to-end canary behavior remain UNKNOWN.

## Independent release procedure

1. Check out the public release revision in two independent clean environments.
2. Run `nix flake check` and build `.#gateway-image`; compare OCI digests.
3. Confirm that digest is pinned by the public `tinfoil-config.yml` and distinguish the image digest from the hardware measurement.
4. Verify release provenance, measured image/config association, a fresh hardware quote, and the serving HTTPS endpoint-key binding.
5. Fetch and authenticate a fresh catalog quote without sending a prompt.
6. Exercise every pre-prompt failure gate and observe zero prompt bytes upstream.
7. Inspect runtime mounts, cache paths, egress, core dumps, proxy/platform logs, and telemetry captures with seeded canaries.

No deployment/platform verification has yet occurred. Phase 0 is not production-ready while mandatory rows remain UNKNOWN.
