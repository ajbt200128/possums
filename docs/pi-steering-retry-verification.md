# Pi native steering and transient-retry candidate

## Decision and scope

The operator explicitly approved replacing the Possums client's conservative continuation/replay authorization with ordinary Pi steering, follow-up and bounded transient retries. This is a **client source candidate**, not an installation, gateway deployment or live-inference qualification. Receipt-to-balance reconciliation remains deferred.

The target remains Pi **1.0.4** with Node **24.13.0**. Native agent retries use existing settings: enabled by default, three additional attempts per failing-response chain, with exponential 2/4/8-second delays. A successful response resets that chain's budget; this is not a session spending cap. No additional provider retry loop or settings override is introduced.

Each retry obtains a new submission and refreshes the authenticated catalog before reservation. It cannot resume or deduplicate a lost generation. An earlier request may still settle after delivery fails, so a new attempt may incur another charge. Only an authenticated terminal receipt/error establishes settled/refunded accounting; otherwise charge status stays unknown. Gateway duplicate-token handling, disconnect-independent settlement and reservation/refund behavior are unchanged.

## Native compatibility and boundaries

Pinned `pi-ai/dist/utils/retry.js` classifies failed assistant messages by locally authored error text, not structured diagnostic metadata. Its quota/budget exclusion includes the word `billing`; retryable messages must preserve unknown-charge warnings without inadvertently matching that exclusion. Pinned agent-session recovery omits failed attempts from model context while retaining raw history; this is outside gateway attestation and is not a no-persistence guarantee.

Closed gateway operational failures are mapped to native-compatible transient wording: unavailable inference, tokenizer/generation send or HTTP failures, and upstream stream transport/idle/deadline failures. Preserve stable codes and observed refund/unknown status. Gateway HTTP-failure categories do not preserve upstream status; no exact upstream status or cancellation is inferred.

Ordinary steering/follow-up is owned by Pi rather than a Possums-only continuation guard. This does not authorize execution of provisional tool deltas: validated arguments, finish, settled receipt, DONE and authenticated EOF remain prerequisites. Epoch/account replacement and abort checks remain.

Authentication, insufficient credit, request/duplicate rejection, catalog/attestation/integrity/decoding validation, settlement errors, abort and paid length answers remain terminal. Generic local `ChannelError(uncertain)` remains terminal because transport, framing, decryption and hook failures are currently coarsened into the same category. Selective local connection-error retries would require preserving a narrower failure category; this candidate does not claim full SDK connection-error parity. Cache warming remains blocked. Native compaction's separate no-retry/overflow-cancellation behavior remains unchanged.

## Verification status

Fresh scratch build passed strict TypeScript 5.9.3 checking and esbuild bundling with Node 24.13.0 and all three installed Pi peers pinned to 1.0.4. Candidate extension SHA-256: `7f370154c4845de1c7485818db7591bbf9a0081776b22f561d509f2b85edcc3e`.

The complete scratch local-check command passed:

- 31 reference-client checks.
- 382 client/admission checks, with offline fetch fixtures only.
- 98 synthetic/local release-policy checks. These mock hardware/crypto/public responses as declared; they are not production attestation verification.
- 66 actual pinned-Pi/provider checks (counted from their `results.json`; the separately reported 71 belongs to shared admission regressions), including native steering during held text/tool responses, queued follow-up, refunded/unknown transient retry then success, exact default three-retry exhaustion, 2/4/8-second default delay calculation, budget reset after success, abort during backoff, retained raw diagnostics/native context omission, and no execution of incomplete tool fragments from a failed attempt.
- 71 shared admission regressions: 65,540 wide properties, with zero whole-object collections, stringify/encryption/network on rejection, or getter calls in the reported vectors.

Negative checks retain terminal auth/credit/request/integrity/decoding/settlement/paid-length outcomes and hostile-content privacy. Existing compaction, cache-warming, session verification, logout/replacement and receipt gating checks passed. Synthetic native retries use millisecond fixture delays; production defaults are verified from the pinned SDK, not measured with a live provider.

Scratch package: `/var/folders/5v/1zw417197236y_k11j994g8r0000gn/T/possums-pi-build-rcXQ2g/package`. Local check output: `/tmp/possums-pi-steering-retry-check.log`. These temporary local artifacts are not a published audit or installed extension.

Active LSP probing covered the three changed production TypeScript paths but did not confirm every file clean; it also reported five auxiliary style warnings in existing provider patterns. Strict scratch TypeScript checking is the build evidence, not a clean-LSP certificate. Independent read-only review found no concrete blockers in classifier compatibility, accounting statements, steering, retry boundaries or incomplete-tool protection; its `git diff --check` passed. The reviewer did not run live inference.

No live request, credential read, gateway deployment or balance reconciliation was performed for local qualification.

## Operator-approved local installation

After the operator approved installing together, the stable `/Users/mb5/.local/share/possums/pi/installed` symlink was atomically switched from `v0.0.16-pi-startup-catalog-56c692fbe878` to `pi-native-steering-retry-7f370154c484`. The previous directory is retained for rollback. Copied extension SHA-256 matches the qualified candidate above; package metadata and build report were copied alongside it, with all three peer symlinks targeting installed Pi 1.0.4. No auth store, settings, gateway configuration or running process was changed.

Importing the installed bundle succeeded without invoking its extension factory, opening a connection or sending inference. The shell's default `pi` launcher reports 1.1.0; this client still requires an explicit Pi 1.0.4 launch. Interactive full restart and user-observed steering/retry behavior remained unverified at installation. No paid failure probe was authorized or run.

## Isolated installed-runtime startup check

On the operator's request to perform startup, a fresh actual Pi 1.0.4 SDK session loaded the installed bundle with only that extension, in-memory settings/model/session stores, no tools, no other resources, cache warming and installation telemetry disabled. Native AuthStorage restored existing saved authentication internally; no credential values were displayed or copied into artifacts. The check did not call `prompt()` and explicitly blocked submission/generation fetch paths.

Observed: the installed extension loaded, selected and verified session-pinned gateway **v0.0.17**, and restored a live authenticated catalog containing **six models**. The offline `/possums-status` handler reported the pinned release and local expiry; it is not independently a current connection check. Session messages: **zero**; persistent session: **false**; inference/submission attempts: **zero**. The isolated session was disposed and its provider shut down afterward; no existing interactive session/process was stopped or changed. Startup harness: `/tmp/possums-pi-startup-check.mjs`, temporary local tooling, not a distributed client artifact.

This is installed-runtime startup/auth/catalog evidence, not paid steering/retry qualification, live balance reconciliation, native CLI v3 verification, or whole-runtime privacy acceptance. Existing interactive Pi sessions still require their own full restart to activate the replacement.

## Policy references

- [Repository security and error policy](../AGENTS.md#security-model): Pi-specific retry exception; observed accounting only; closed, content-free errors.
- [Phase 0 contract](phase0.md): unchanged gateway no-replay and exactly-once accounting boundaries.
- [Privacy: telemetry permitted signals and forbidden data](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data) and [required evidence](../PRIVACY.md#required-evidence-for-every-telemetry-change): no telemetry change, request/account events or diagnostic export is introduced. Test data must remain isolated synthetic; hostile-content assertions establish only their tested paths, not whole-Pi privacy.
