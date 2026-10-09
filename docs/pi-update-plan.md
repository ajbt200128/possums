# Pi-compatible gateway updates — proposed plan

Status: planning only, 2026-10-09. The operator requested preparation and explicitly withheld rollout. No release publication, deployment (including a held candidate), promotion, client installation, credential access or inference is authorized by this document. No runtime changes are implemented here.

## Why updates currently interrupt Pi

- `examples/phase01/approval.ts` compiles one exact v0.0.15 API approval: origin, manifest/image/config/source hashes, signer workflow and invocation. `requireApiApproval()` requires exactly one entry. Its administrative qualification expires **2026-10-11**; that is not quote freshness.
- `clients/pi/bootstrap.ts` requires the adjacent manifest to match that approval before public hardware/provenance/key verification. A changed gateway cannot be authorized by merely downloading its advertised manifest or latest GitHub release.
- `examples/phase01/transport.ts` constructs a channel with a snapshot of the verified HPKE key and a per-request approval check. A running process retains its old code/pins/key even after the installed-package symlink changes. `/reload` has previously been insufficient; a full restart was required.
- `clients/pi/provider.ts` caches the client/bearer. Restoration runs only when no client is present; catalog failures clear model listings, not the cached client. There is no transparent release/session recovery state machine.
- `src/auth.rs` creates a fresh epoch and empty session/challenge/submission maps on restart. `src/accounting.rs` and `AppState::new` create a new demo ledger from provisioned credit. New software trusting the new measurement does not preserve an old bearer, conversation/submission binding, reservation or balance.
- `src/main.rs` stops serving on a signal and shuts down telemetry; it does not rendezvous with all active connection/preflight/inference/settlement tasks before process exit. `src/web.rs` spawns connection tasks, and generation work can outlive downstream delivery. Waiting only for HTTP connections would be insufficient.

The verification failure is correct fail-closed behavior. Availability needs an explicit transition protocol, not weaker verification.

## Recommended first increment: preapproved overlap and safe recovery

This solves the next paired rollout without introducing an automatic software updater or a new signing authority.

1. **Qualify the exact next release before promotion.** Preserve independent image reproduction, exact manifest/config/source/workflow/invocation checks and the accepted pinned JavaScript legacy-v2 Pi verifier. The gateway's official v3 helper remains separate. Native CLI v3 incompatibility and legacy-v2 freshness limitations remain unresolved; neither is silently waived nor made a new prerequisite.
2. **Distribute one transition client first.** It contains bounded exact approvals and matching immutable manifests for the current and next release, including all client fixes `fd87d5a`, `670f9ce`, `5e9d966`. Adapt approval selection/qualification/channel construction together; merely adding a second array entry currently fails. Old-only clients cannot learn a new approved measurement without an initial client update/restart. Do not promote until the relevant running Pi sessions have loaded the transition client.
3. **Select only an approved release.** Public evidence may identify a candidate but is not authority. Match it to a locally approved entry, then verify the complete hardware/provenance/key/manifest chain. An unknown, expired or revoked release fails before new credentials or prompt transmission. Never use an arbitrary server-selected origin or manifest as approval. Overlap is temporary, not permission to accept every tag from the repository.
4. **Use an immutable connection context.** Bind selected approval, manifest, verified channel/key, bearer, catalog and server conversation state together. Each invocation keeps one context; do not change its policy/key/bearer underneath it. Prefer the next release for new work only when verified. Keep an active agent/tool round or compaction on its existing context; activate a prepared replacement at a safe idle boundary.
5. **Reconnect controls, never inference.** At an idle/pre-invocation boundary, verify the serving release again and establish a fresh login/catalog using Pi's existing stored recovery credential. Publish the replacement context only after all stages succeed. A known pre-submission stale session may trigger one bounded recovery. Authenticate a new session as a new server conversation without discarding Pi's local history or resetting replay authorization to make a failed attempt executable. Once submission issuance starts or delivery is uncertain, do not automatically replay that invocation, its tools or compaction. Unknown failures are not proof of an upgrade, cancellation, refund or safe retry. Prepare recovery for the next deliberate run instead.
6. **Preserve diagnostics and account cancellation.** Distinguish update approval, public verification, session and catalog stages with locally authored content-free diagnostics. Stale success/failure must not replace a newer login or survive logout/account change. No automatic diagnostics, request logs or traces.

The existing SDK encrypts payload bodies, not bodyless GETs or request headers. Catalog requests use ordinary HTTPS. Do not call them HPKE-authenticated merely because a channel object exists. The connection transition must review the supported SDK/runtime TLS/key-binding mechanisms for credential-bearing controls, including routing/key changes between verification and use; do not handwrite a provider protocol or invent an authenticated restart signal.

## Request continuity is a separate rollout gate

[Current Tinfoil Admin API documentation](https://docs.tinfoil.sh/admin/admin-api) supports read-only update plans and blue/green `hold:true`, with a separate review connection and explicit promotion. A hold boots a new enclave: it is a deployment, not preparation, and is currently prohibited. Documentation says promotion switches traffic; it does not establish old-stream draining, client affinity, old-session persistence or accounting handoff.

Before promising uninterrupted active turns:

- Verify actual platform behavior for long streams and old-instance termination using an isolated synthetic qualification, not paid/filler production inference. A review URL must have an explicit qualification scope and correctly verified hostname/key identity; a control-plane descriptor is not attestation.
- Add and test gateway draining if platform behavior alone is insufficient: stop new admission, keep accepted preflight/inference/settlement owners alive even after disconnect, allow receipt/EOF delivery, then stop. Track accepted work as well as sockets. Keep existing inference/resource deadlines; do not invent an upgrade grace cutoff or kill accepted work to meet a rollout timer.
- Coordinate promotion with quiescence. With the current memory-only Phase 0 ledger, do not route concurrent billable work across two independent ledgers and claim shared reservations, concurrency, idempotency or preserved credit. Demo-credit reset remains an explicit restart limitation. Durable purchased-credit continuity or overlapping active replicas needs a separately approved accounting design; do not pull it into this telemetry rollout.
- If drain, routing or ledger continuity cannot be established, plan a controlled idle cutover with a short wait for new work, followed by verified reauthentication. Call that a coordinated update, not proven zero downtime. Retaining an old package permits local software rollback, not resurrection of its bearer/ledger or permission to replay failed inference.

## Subsequent increment: approval metadata separate from extension code

For routine future gateway updates without a Pi restart, a stable provider can refresh **signed public release-approval metadata** at idle boundaries. This changes the client's trust policy and requires explicit approval before implementation.

- Pin an independent release-approval authority/root in the client. GitHub build provenance proves how an artifact was built; it does not itself authorize every future build. Reuse a maintained update-metadata library/standard where practical rather than inventing signature or rollback handling.
- Metadata approves exact release identities/manifests and compatible API/client versions, contains sequence/expiry/revocation policy, and authenticates every referenced artifact. Limit the active overlap. Authenticated operator rollback requires new metadata, not accepting replayed old metadata.
- Persist only public metadata and an anti-rollback high-water mark, atomically. No credentials, history or receipt ledger. Keep a currently valid approved release usable during metadata-service failure; if no valid approval remains, fail closed. A withheld/frozen update cannot be made safe indefinitely without renewed trusted authority.
- Metadata never silently extends old approval expiry, authorizes remote JavaScript, modifies Pi settings or updates verifier/SDK dependencies. Extension/runtime changes retain an independently verified package-install path and may still require restart.
- New authority/key rotation, validity windows and emergency revocation behavior need an explicit operator decision. Do not claim metadata freshness repairs the existing v2 quote-freshness limitation.

## Acceptance tests before enabling the transition

- Old and next approved release accepted; unknown release, mixed manifest/config/key, wrong signer/invocation, expired/revoked approval and disallowed origin rejected before secret/prompt transmission.
- Hardware/HPKE key rotation and cutover between evidence acquisition and request use fail closed; every credential-bearing control follows its reviewed transport binding.
- One bounded session recovery, stale account/logout races, failed catalog, offline operation and unavailable approval updates do not send inference or wipe run/replay guards.
- Active text/tool streams, tool-result continuations, two-call compaction, downstream disconnect and receipt delivery remain bound to their selected context. Forced interruption remains unknown without an authenticated terminal receipt, with zero automatic replay or tool execution from provisional deltas.
- Signal/drain races, charged preflight, upstream error/missing usage and simultaneous old/new admission preserve exactly-once settlement/refund ownership. No assertion that HTTP quiescence alone establishes accounting quiescence.
- Later metadata tests cover tampering, freeze/replay/downgrade, bad artifact hashes, atomic crash recovery, expiry, explicit rollback and incompatible client versions.
- Privacy negatives use synthetic sentinels and content-free assertions only; no automatic support archive. Refer to [data handling](../PRIVACY.md#data-handling-boundaries), [forbidden exports](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data) and [required evidence](../PRIVACY.md#required-evidence-for-every-telemetry-change). Existing local telemetry source/wire evidence remains in [the five-minute record](telemetry-mvp-five-minute.md); this plan adds no runtime qualification.

## Held telemetry rollout checklist

A separate paired gateway/Pi history-tool-count fix is under investigation in another thread. It is not part of this plan or the telemetry changes and must not be included without its own reviewed commits and acceptance evidence. Do not edit its files or infer that an unrelated no-output interruption was caused by release verification. Future package preparation must explicitly identify the independently approved compatibility baseline rather than silently dropping approved fixes or absorbing ongoing work.

- [x] Local telemetry implementation committed and reviewed; `devenv test` passed. Three independent client commits remain in ancestry. Shared checkout clean before this document.
- [ ] Agree first-increment scope, request-drain/cutover limitations and approval validity; implement/test the transition client before promotion.
- [ ] Choose integration PR layout without dropping/rebuilding old client fixes; merge exact reviewed heads after CI.
- [ ] Only after authorization: independently build/publish the image and exact measured release; verify provenance/artifact pins. No candidate tag/digest/invocation is guessed here.
- [ ] Build the transition Pi package from the integrated source, not the old v15 tag; package both independently approved manifests. Verify source/artifact hashes and offline tests before any install.
- [ ] Only after authorization: deploy a held candidate if supported, perform scoped public checks, distribute/load the transition client while old production still serves, establish the safe cutover conditions, then separately authorize promotion.
- [ ] Verify the serving release and Pi's verified session/catalog path without manufacturing inference. Paid live probes require separate authorization.
- [ ] Observe natural traffic after warmup/full five-minute windows; verify real `prod/metrics` request queries. Missing populations remain unavailable. Preview the full dashboard and obtain approval before creation.

**Current hold remains absolute:** no publishing workflows, candidate deployment, promotion, installed-package replacement or paid production requests have been performed for this plan.
