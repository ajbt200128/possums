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

Record commands, dates, toolchain versions, and outcomes here when implementation exists. CI must report unavailable platform checks as blocked, never silently skip them.

## Independent release procedure

1. Check out the public release revision in two independent clean environments.
2. Run `nix flake check` and build `.#gateway-image`; compare OCI digests.
3. Confirm that digest is pinned by the public `tinfoil-config.yml` and distinguish the image digest from the hardware measurement.
4. Verify release provenance, measured image/config association, a fresh hardware quote, and the serving HTTPS endpoint-key binding.
5. Fetch and authenticate a fresh catalog quote without sending a prompt.
6. Exercise every pre-prompt failure gate and observe zero prompt bytes upstream.
7. Inspect runtime mounts, cache paths, egress, core dumps, proxy/platform logs, and telemetry captures with seeded canaries.

No deployment/platform verification has yet occurred. Phase 0 is not production-ready while mandatory rows remain UNKNOWN.
