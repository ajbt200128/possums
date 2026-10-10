# Shared public-evidence acquisition cache — agreed direction

## Original GitHub-only direction

2026-10-10. **Historical planning.** The operator selected this direction and deferred GitHub CLI/token authentication. This document does not authorize installation, deployment, retries or changes to trust validity. No cache implementation or qualification is claimed.

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

Before implementation, choose the smallest reusable acquisition helper, local public-cache location, entry schema and bounded cross-process coordination using existing libraries. Read `PRIVACY.md` before diagnostic/cache tooling; do not add logs, traces, support bundles or user-linked cache keys. Do not cache live attestation reports, credentials or request content. The original GitHub-only/hardware-ID exclusion is superseded only by the narrow public AMD certificate exception below.

Use synthetic/offline tests for concurrent fresh processes, unchanged and changed attestation hints, corrupt/partial entries, wrong manifest/provenance associations, failed download, rate-limit status, publisher/key/measurement mismatch, cancelled acquisition and stale publication. Ensure no cached verdict can bypass verification and no diagnostic/control action sends inference. Test actual Pi startup and generated artifacts, not just the cache helper.

Record source, build, installed and activated evidence separately. Preserve current installed package for rollback. Full Pi 1.1.0 qualification and installed T3 presentation remain separate acceptance requirements.

## Authorized bounded AMD extension

After supplied actual T3 startup failure `possums_evidence_unavailable`, stage `amd_certificate`, constraint `rate_limited`, observed HTTP **429**, the operator requested “New cache should include amd too.” Authorization covers local retention of public AMD VCEK DER **including its embedded gateway hardware ID**. This is a narrow hardware-public-certificate exception to the original exclusion, now explicit in [PRIVACY.md](../PRIVACY.md#data-handling-boundaries). It does not permit raw reports, bare chip IDs, full KDS URLs, credentials, auth/account/request/history data, failures or verdicts, nor telemetry/log/support artifacts.

- Extend the same one-entry cache and owner lock. Keep `pi-public-evidence-v1` as the directory name; strict schema **version 2** adds bounded base64 `vcek` DER. Accept only exact schema fields for versions 1 and 2; validate newly acquired fields before verification/publication.
- A complete matching v2 entry supplies exact public bytes without GitHub or AMD download. Every process independently reruns the pinned SDK's complete chain/date/TCB/HWID/current-report signature and measurement checks, publisher/workflow/artifact validation and current gateway-certificate/live-HPKE/key binding.
- A well-formed matching v1 entry enters the existing owner lock, is re-read, reuses its GitHub manifest/provenance, and acquires **only AMD** from the current bounded report. Publish v2 atomically only after full qualification; concurrent consumers acquire once and verify independently.
- Absent, malformed, unknown-field, wrong-hint or oversized entries are full acquisition misses. A v2 entry missing/malformed/empty `vcek` is not a partial migration entry. Structurally valid DER bytes that fail SDK cryptographic/date/TCB/HWID verification remain terminal: no fallback/reacquisition to mask rejection.
- Changed report bytes conservatively reacquire both sources; never match by cached HWID or TCB. No TTL, expiry, freshness, retry, renewal, inference or authenticated quote policy changes. Explicit compiled-manifest connection remains independent and uncached.

The earlier GitHub-only candidate was locally installed; this AMD-inclusive source extension is **not installed or live-T3 accepted**. See [separate verification scopes](pi-public-evidence-cache-verification.md). No installation/activation/public probe/host restart/deployment/push/merge is authorized by this packet; rollout remains held for the separate automatic-release/deployment work.
