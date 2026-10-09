# MVP: routine gateway updates without restarting Pi

Status: selected plan implemented as a source candidate; independent review and combined quota/telemetry integration remain in progress. Rollout remains held. Scoped evidence: [session-update verification](pi-session-update-verification.md). This supersedes the earlier TUF, separate approval-signing, overlap and seamless-migration proposals.

## Contract

- One initial extension update/restart installs this mechanism. Later compatible gateway releases do not require rebuilding or restarting Pi.
- At each new Pi trust session, acquire and verify the published release artifact and the serving gateway before sending credentials or prompts. Startup, `/new` and opening/resuming another transcript start fresh trust; confirm the actual Pi lifecycle in native tests.
- Capture the verified release/manifest/key for that session. No background/per-turn release refresh or automatic migration. Keep local validity checks and normal authenticated catalog/reservation/receipt checks.
- If a gateway update breaks the session, fail closed without replay. Start a new session to reconnect. Interrupted billing remains unknown absent an authenticated terminal receipt.
- Stop the old gateway, wait for stopped, then deploy the new tag. Downtime, lost in-memory sessions and reset Phase 0 demo ledger are accepted limitations, not preserved credit or confirmed refunds.

## Existing artifact and explicit trust change

Use **Tinfoil's `tinfoil-deployment.json` and its existing GitHub/Sigstore attestation**. Current publication is `.github/workflows/tinfoil-release-publish.yml`, using the pinned Tinfoil measurement action. A release listing/download is untrusted discovery, not proof.

The client will trust the fixed Possums repository and approved release-publishing workflow as authority for future releases, rather than compile each release's manifest/source/invocation pins into the extension. The operator accepted this policy change. Compromise or misuse of that publishing authority can authorize unwanted code; attestation does not independently approve its safety.

Using existing pinned verifier libraries, authenticate the artifact's signature, signer identity, signed subject/hash and predicate, and its source/tag/invocation relationships. Check exact config/image/measurement and the serving endorsed key against that authenticated artifact. Never accept an arbitrary repository/workflow, caller-supplied verified flag or server-selected trust policy.

Retain Pi's accepted JavaScript legacy-v2 hardware verification and its freshness limitations; gateway v3 helper and unresolved native CLI v3 mismatch remain separate. Call the artifact what its verified predicate establishes: do not claim a SLSA level or add SLSA infrastructure just for the label. Existing HTTPS/header/bodyless-GET and EHBP body/reply boundaries remain accurately documented.

No persistent update/version database means no new anti-rollback or immediate revocation guarantee. Older legitimately signed releases may remain valid under this authority; release discovery and serving checks are not a proof of newestness. Existing sessions do not discover later revocations. The implemented published context expires at the earlier of 12 hours after verification and the serving certificate's expiry, checked locally before transport. This is not quote freshness. Compiled/reference administrative expiry is unchanged. Preserve independent compiled/reference and WEB approvals.

## Three implementation packets

1. **Artifact bootstrap and qualification:** in `clients/pi/bootstrap.ts` and `examples/phase01/approval.ts`/`transport.ts`, acquire public release/manifest/attestation bytes from fixed destinations and reuse existing verifier helpers. Replace Pi's per-release compiled selection with an immutable authenticated release context; keep strict compiled/reference paths intact. No custom signature or provider-wire decoder.
2. **Pi session binding:** in `clients/pi/index.ts`/`provider.ts`, create one cancellable verification task per trust session. Subsequent login/catalog/turn/compaction uses that context. Separate public trust from authentication epochs so logout/account changes cannot trigger hidden release refresh or publish stale work. Keep transient content-free diagnostics and offline pinned-release status.
3. **Tests and runbook:** prove in one running Pi process that gateway A replacement interrupts its old session safely and `/new` verifies B without a Pi restart. Test mixed artifact/key, bad signature/signer/hash, expiry, acquisition failure, actual lifecycle events, account races and zero automatic inference/tool/compaction replay. Update the README and scoped verification record. Synthetic/local tests only unless live inference is separately authorized.

No TUF, extra signing keys, approval service, cache database, automatic code updater, hot swapping, blue/green candidate or drain project. Reuse the existing release workflow; add only a missing artifact/provenance check if actual inspection finds one.

## Held release sequence

1. Merge independently reviewed source with exact-head CI; independently reproduce the image and publish/measure the tagged release through the existing workflow.
2. Verify the actual published manifest/attestation and artifact identities. Build/install the initial client from the integrated reviewed source only after authorization, preserving client commits `fd87d5a`, `670f9ce`, `5e9d966` and any subsequently approved fixes.
3. Only after explicit rollout authorization, stop/wait/deploy with preserved configuration and secret references. No overlapping gateway instance, credential/env dump or claimed refund. Rollback also uses stop/deploy and full artifact/serving verification; never replay uncertain inference.
4. Open a fresh session in the already running Pi process. Verify public bootstrap/session/catalog as scoped; no paid/filler traffic. Telemetry query/dashboard work remains separately gated by actual data and approval.

Keep this mechanism independent of telemetry and the other thread's ongoing limits/history changes. Do not alter their files or absorb unreviewed changes. Policy: [data handling](../PRIVACY.md#data-handling-boundaries), [forbidden exports](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data), [evidence requirements](../PRIVACY.md#required-evidence-for-every-telemetry-change).

Implementation evidence belongs to the linked verification record. No publication, deployment, installation or paid request is established by this planning document.
