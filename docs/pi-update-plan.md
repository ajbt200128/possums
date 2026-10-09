# Routine gateway updates without restarting Pi

Status: **implementation plan, not implemented or deployed**, 2026-10-09. This replaces the earlier overlap/hot-refresh proposal. The operator selected session-start verification, accepts interrupted sessions and downtime, and requires only one gateway instance running at a time. Publication, deployment, installation and paid probes remain on hold.

## Selected contract

- One initial extension update/full Pi restart installs the mechanism. Later **compatible gateway releases** require neither rebuilding the extension nor restarting Pi.
- Refresh signed public release approvals and verify the serving gateway **once when a new trust session starts**, before sending credentials or prompts. No periodic, background or per-turn approval refresh; no automatic release migration within that session.
- A trust session starts on Pi startup, `/new`, transcript resume/switch/fork, or explicit `/reload`. Recommended rule: reopening an existing transcript creates fresh trust, not a persisted old channel. Local history is not an authority to reconnect to an old release. Confirm actual qualified Pi lifecycle behavior in native tests; do not rely on mocked callbacks alone.
- Bind an immutable approved release, manifest and verified channel/key to that session. Check its signed approval expiry locally before each new request. Existing request completion is not turned into a fabricated refund at expiry.
- Authentication/catalog refresh within the session uses that same verified context. Model catalog validation and gateway reservation/settlement remain per invocation. A gateway/key change or lost session fails closed; the user starts a new session to reconnect. Never replay uncertain inference, tools or compaction automatically.
- Stop the old gateway before deploying the new one. No held candidate, blue/green overlap, request-drain prerequisite, shared-session migration or zero-downtime promise.
- Metadata/key/API-policy changes are not automatic JavaScript updates. Extension, verifier or Pi-runtime changes may still require a separate approved install/restart.

Revocations published after startup will not be discovered by an existing session until its next trust-session boundary. Local expiry bounds continued new requests; it is not immediate revocation or legacy-v2 quote freshness. These limitations must appear in user documentation.

## Current code and required changes

`examples/phase01/approval.ts` compiles one exact v0.0.15 API approval and requires a singleton. `clients/pi/bootstrap.ts` requires its adjacent hash-approved manifest. `examples/phase01/transport.ts` captures a verified key plus a local policy check. Adding a second array entry or downloading an advertised latest release cannot safely update that authority.

`clients/pi/provider.ts` caches a bearer/client and can currently establish verification during authentication. `newSession()` resets conversation/run state but does not itself discard the cached client. `clients/pi/index.ts` handles Pi session startup. They must be changed together so a new trust session clears the old binding, exactly one startup task establishes fresh trust, and later authentication cannot fetch a newer approval behind the user's back.

`src/auth.rs` and the demo ledger start fresh on gateway restart. Lost bearer/submission/idempotency state and reset demo credit remain explicit Phase 0 limitations. This mechanism does not introduce durable purchased-credit accounting or imply that old charges were undone.

## Trust architecture

### Independent release approval

Use a pinned update-metadata root to authenticate **operator-approved release identities**. GitHub/Sigstore build provenance still proves the artifact's build identity; it does not authorize every future repository build. An untrusted gateway, GitHub latest tag or metadata hostname cannot choose a new trust root.

Recommended starting point: [TUF](https://theupdateframework.io/docs/overview/) with the maintained [`@tufjs/client`](https://github.com/theupdateframework/tuf-js/tree/main/packages/client). Its supported metadata versions, signatures, hashes, expiry and root rotation should handle update-protocol verification; do not reimplement them. Before choosing/pinning dependencies, test Node 24/Pi packaging, bounded acquisition, cache behavior and safe concurrent use. This is a library candidate, not a qualification claim.

The signed target contains a small gateway-approval document and authenticates its referenced public deployment manifest. Approval fields preserve exact tag, manifest/image/config/source hashes, signer workflow and invocation, plus client/API compatibility and a signed validity deadline. Keep the production origin, repository and permitted approval schema constrained by the installed client. Metadata cannot widen them silently.

Retain the accepted pinned JavaScript **legacy-v2** hardware/provenance/key verifier, and preserve independent WEB approval. The gateway's official v3 helper is separate; native CLI v3 mismatch and legacy-v2 freshness limitations remain unresolved. No v3 migration or generic trust bypass is a prerequisite added by this plan.

### Session binding

After metadata authentication and schema/compatibility validation, qualify the immutable manifest, serving hardware evidence, provenance and endorsed key through the existing verifier. Only then construct the session's verified client context and permit credential transmission. Do not expose a caller-supplied `verified: true`/bare approval object as authority.

Subsequent operations use the captured context and local validity checks. Stored credentials may establish a new bearer against that context; `/login`, model refresh and compaction cannot select another gateway release. No repeated attestation is required on ordinary turns. Separate the public trust-session epoch from the account/authentication epoch: logout or an account change invalidates authenticated work, but must not cause a second metadata fetch within the same trust session. Login/logout/account-change and session replacement races must invalidate stale tasks without resetting replay guards to authorize a failed attempt.

Preserve the actual transport boundary: the current EHBP SDK encrypts bodies/replies, not headers or bodyless catalog GETs. Those controls use ordinary validated HTTPS; the current channel does not pin its actual TLS sockets to the attested fingerprint. Do not claim stronger socket binding from metadata or a cached channel object. Review supported SDK/runtime behavior for cutover between evidence acquisition and credential-bearing requests, retaining fail-closed prompt-key binding without writing custom provider wire handling.

### Public storage and availability

Cache only authenticated public metadata/manifests and update-version state, with safe atomic/concurrent handling. No new credentials, bearer, history, request identifiers, receipts or diagnostics store. Let the maintained library enforce rollback/expiry checks; verify its actual cache/crash behavior instead of assuming atomicity.

For the first implementation, require a successful authenticated metadata refresh at the new-session boundary. Do not add an offline-fallback policy that could conceal tampering or rollback. An unavailable metadata host may prevent new sessions; an already valid bound session continues without contacting it, until its local approval expires or the gateway connection fails. No background retries or inference resend.

The metadata host observes ordinary connection timing/network metadata, like existing public evidence hosts. Do not append credentials, account/session identifiers or client telemetry to acquisition URLs. HTTPS hosting is transport, not release-approval authority.

## Implementation packets

1. **Choose authority and pin update tooling.** Approve root/key custody, release-approval gate, public metadata location and validity policy. Prototype the maintained client with synthetic signed fixtures and inspect generated package/dependency artifacts. Keep private keys out of the gateway and Pi; do not invent production keys or put them in chat/repository.
2. **Authenticate approval artifacts.** Add a Pi-owned module such as `clients/pi/approval-metadata.ts` for acquisition and application-schema validation, using the library's verifier/cache. Reject incompatible targets and restrict origins/repositories. Preserve typed, content-free errors for acquisition, signature/root, rollback, expiry, artifact hash and compatibility failures.
3. **Parameterize qualification with a trusted snapshot.** Update `examples/phase01/approval.ts`, `transport.ts`, `client.ts` and `clients/pi/bootstrap.ts` to capture the authenticated approval and manifest together. Preserve existing compiled/reference-client paths and the unchanged verifier's exact provenance/hardware/key checks. Per-request policy checks inspect the selected session snapshot, not a floating latest approval.
4. **Bind real Pi session lifecycle.** Update `clients/pi/index.ts` and `provider.ts`: one cancellable public bootstrap per new trust session; no acquisition in later turns/login/catalog/compaction; clear old binding on replacement. Establish trust without needing a recovery credential. Respect the installed Pi's startup/new/resume/fork/reload lifecycle and stale-task publication guards. Status reports the pinned release/expiry offline, not presumed current deployment health.
5. **Add release approval publication.** After independently qualifying an exact measured release, a separately authorized signing step publishes the signed target/manifests using approved repository tooling. Do not allow ordinary build success alone to mint approval. Changes to `.github/workflows/tinfoil-release-publish.yml` or a separate approval workflow follow the agreed authority boundary; no signing/admin key enters the inference image.
6. **Document/install once, then operate stop/deploy.** Update `clients/pi/README.md` and scoped verification/runbook records. Independently verify the package from the integrated reviewed source, install only after authorization, and restart once. Future compatible releases update public approval artifacts and the gateway only.

Keep this mechanism independently reviewable from telemetry and the other thread's history/conversation/tool/parallelism/size-limit work. Do not edit their ongoing files or bundle an unreviewed fix. Preserve the three already committed client fixes `fd87d5a`, `670f9ce`, `5e9d966`; later fixes enter the compatibility baseline only through their own reviewed commits.

## Acceptance criteria

Use local synthetic gateway/evidence/metadata fixtures and the actual qualified Pi SDK; no paid/filler production requests are needed.

- **No Pi restart:** keep one Pi process alive across synthetic gateway A stop and gateway B start. Its A trust session fails closed without replay; `/new` creates a new trust session, authenticates approved B and succeeds without restarting/rebuilding the extension.
- **Exactly one session refresh:** measure acquisition/qualification calls for startup/new/resume/fork/reload. Multiple turns, catalog refreshes, `/login` and compaction do not refresh release approvals. Authentication without established startup trust cannot smuggle a fresh approval acquisition.
- **Full verification:** reject unknown releases, wrong/mixed manifest/config/key, invalid signer/workflow/invocation, bad metadata signature/root/hash, expired approval, replayed/downgraded metadata, incompatible API/client and disallowed destinations before credential/prompt transmission.
- **Expiry/revocation semantics:** new requests stop at local approval expiry. Metadata removal blocks subsequent new trust sessions; do not assert immediate effect on existing sessions. Clock errors fail closed. No silent extension of the historical **2026-10-11** approval.
- **Session/auth races:** cancel/replace/resume/logout/account changes cannot publish stale trust, bearer or catalog; concurrent Pi processes/cache writes and interrupted storage cannot lower the trusted update version.
- **Billing and tools:** cut streams before output, mid-text/tool deltas, after usage or before authenticated EOF. Retain confirmed receipts where observed; otherwise report unknown billing. Never replay inference, authorize provisional tools, create a failed compaction checkpoint or reset guards to hide the interruption.
- **Privacy:** hostile artifact/parser/SDK errors remain locally authored closed diagnostics, transient only. Inspect generated bundles for private material. No new logs/traces/telemetry, credential access in tests, request payload archive or support upload.

Policy: [data handling](../PRIVACY.md#data-handling-boundaries), [forbidden exports](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data), [required evidence](../PRIVACY.md#required-evidence-for-every-telemetry-change). Existing [five-minute telemetry evidence](telemetry-mvp-five-minute.md) is separate; this document adds no runtime test pass or deployment claim.

## Single-instance release runbook — held

The [Tinfoil Admin API](https://docs.tinfoil.sh/admin/admin-api) chooses update strategy from resource/volume configuration. A CPU-only update can use blue/green by default. To satisfy **no two gateway instances**, use the supported **stop, wait for stopped, then deploy target tag** sequence, not an ordinary update/hold/promote operation. Verify platform billing/termination semantics rather than promising that a status flag proves charges ended.

1. Merge only independently reviewed source and passing exact-head CI. Independently build/publish the image and measure the exact tagged release. Record verified manifest/config/provenance identities; never guess pins.
2. Independently approve/sign/publish its metadata and compatible manifest artifacts. An approval pointing at a not-yet-serving release may temporarily prevent new sessions; that is accepted downtime, not permission to bypass checks.
3. Obtain explicit rollout authorization. Announce that ongoing requests may be interrupted; no cancellation/refund assurance. Stop the old gateway, wait for its stopped state, then deploy the exact target with preserved configuration/secret references/resources. Do not display or archive secret values. Do not start a second candidate.
4. Verify the serving public release using the accepted pinned path. Start a fresh trust session in the **already running Pi process**. Session/catalog checks do not manufacture inference; any paid live test requires separate authorization.
5. For failure/rollback, stop before deploying the separately approved rollback release and publish newer authenticated approval metadata as needed. Never accept replayed old metadata, revive lost ledger/session state or replay an uncertain generation.
6. For the telemetry rollout, inspect natural traffic after warmup/complete five-minute windows, verify actual request queries, then preview the full dashboard and obtain creation approval. No filler traffic or fabricated missing values.

## Decisions before implementation

- **Authority:** recommended independent operator-approved signing, an offline bootstrap/root role and a protected release-approval publisher; agree key custody/rotation and the metadata host. No authority/key is provisioned yet.
- **Validity:** choose metadata and release approval lifetimes plus renewal policy. Shorter validity bounds stale approvals but long sessions may then stop accepting new work. These deadlines do not repair v2 quote freshness.
- **Resume rule:** recommended fresh trust on every session-start reason, including resuming the same transcript. No persisted per-transcript channel/key authority. This keeps the mechanism small and permits `/new`/resume to recover without restarting Pi.

**Hold remains:** no publishing workflow, signing-key provisioning, gateway stop/deploy, client installation or paid request has been executed for this mechanism. Implement and qualify the selected design first; rollout remains a separate authorization.
