# Shared GitHub public-evidence acquisition cache — agreed direction

2026-10-10. **Planning only.** The operator selected this direction and deferred GitHub CLI/token authentication. This document does not authorize installation, deployment, retries or changes to trust validity. No cache implementation or qualification is claimed.

## Scope

Fresh Pi processes on the same machine may reuse public GitHub release manifest and provenance bytes. They do not reuse another process's authorization decision, verified client, bearer session, recovery credentials, account state or conversation.

Each process still performs its required verification of the evidence, current gateway hardware identity and endpoint-key binding before authentication or prompt submission. Existing publisher/workflow/artifact policy, trust deadlines and legacy-v2 freshness limitations remain unchanged.

## Proposed acquisition sequence

1. Acquire the fixed gateway's public attestation without credentials, within existing transport/parser bounds.
2. Compute a hash of that response as a cache-selection/invalidation hint, never evidence of authenticity or freshness.
3. When the hint matches a usable cache entry, reuse its exact GitHub manifest/provenance bytes and run the required verification in this new process.
4. On a changed hint, absent or unusable entry, acquire release discovery and the exact manifest/provenance through the existing approved path. Coordinate concurrent local processes so one acquisition serves their equivalent requests.
5. Publish an acquisition entry only when its artifact associations have passed validation. Cache contents remain untrusted inputs on later reads; cached bytes or metadata must not authorize inference themselves.
6. A failed acquisition or verification fails closed with its specific diagnostic. Do not fall back to mismatched evidence, substitute models, extend trust or replay inference.

A new deployment can change the hint. Report generation, compression or serialization can also change it and cause a conservative extra acquisition. An unchanged response does not establish fresh attestation, newest release, immediate revocation or rollback resistance.

GitHub discovery and gateway deployment must still agree when acquiring new evidence. A deployment between observation and submission remains subject to existing channel/authentication checks; cache refresh must not automatically resend an uncertain generation.

## Separation from other work

- The blue/green thread owns existing-session recovery/reverification eligibility. Its current client recovery blocker must not be bypassed here. This cache serves evidence acquisition for fresh processes and is not automatic setup recovery.
- `docs/end-to-end-diagnostics-plan.md` separately proposes preserving safe diagnostics through Pi/RPC/T3/UI. Cache failures must retain their category through that path.
- GitHub authentication is deferred: do not discover `gh` credentials, add token configuration or change authentication headers.
- No 15-minute latest-discovery TTL was approved. The agreed direction uses the attestation-change hint; any additional age/eviction/refresh policy requires explicit design review.

## Implementation decisions and acceptance

Before implementation, choose the smallest reusable acquisition helper, local public-cache location, entry schema and bounded cross-process coordination using existing libraries. Read `PRIVACY.md` before diagnostic/cache tooling; do not add logs, traces, support bundles or user-linked cache keys. Do not cache live attestation reports, credentials or request content as part of this GitHub-only cache.

Use synthetic/offline tests for concurrent fresh processes, unchanged and changed attestation hints, corrupt/partial entries, wrong manifest/provenance associations, failed download, rate-limit status, publisher/key/measurement mismatch, cancelled acquisition and stale publication. Ensure no cached verdict can bypass verification and no diagnostic/control action sends inference. Test actual Pi startup and generated artifacts, not just the cache helper.

Record source, build, installed and activated evidence separately. Preserve current installed package for rollback. Full Pi 1.1.0 qualification and installed T3 presentation remain separate acceptance requirements.
