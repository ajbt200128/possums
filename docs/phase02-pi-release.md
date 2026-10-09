# Minimal Pi integration release

## Initial v0.0.10 scope

The initial Pi 0.99.2 client used the verified Possums provider: memory-only login/catalog,
SDK transcript and tool events, caller-owned execution checks, and matching-result
continuation. The Pi adapter has no credit reservation or spending-budget engine.
Reservations and final-usage settlement remain gateway behavior. No dependency,
public signup, payment, replica, or unrelated user-document change is included.

Only exact `kimi-k3` gets the OpenAI-functions wire profile. Existing live named
(F), auto and none (H) cases passed the verified SDK transport, final usage and
EOF path. H's required case had valid usage/EOF but ended at the diagnostic output
limit; this is not proof of a complete multiple-call round. Historical reports
remain `qualified:false` under their broader test criteria. This release decision
accepts the narrower wire evidence plus actual-SDK/local accounting and caller
fixtures; it does not retrospectively mark those reports fully qualified.

The user explicitly requested this minimum integration rather than another
standalone qualification matrix. Multiple-call execution, real Pi continuation,
and balance reconciliation remain live acceptance items until observed. A paid
partial response never authorizes execution or automatic regeneration.

## Removed test-only prerequisite

The private qualification harness is not part of this release. Its I check
subtracted the selected maximum output from the context and required a positive
remainder. The catalog schema has no separate output limit and represents the
maximum as the context window, so that private check necessarily rejected before
prompt transmission. The gateway instead tokenizes after reservation and uses
the remaining context. This does not change gateway credit checks, rates or
settlement, and does not justify silently exceeding a test spending ceiling.

## Release verification

Release/source/image/config/workflow/invocation pins, independent Linux builds,
real hardware/TLS/HPKE bindings, and the updated client approval must be verified
before the changed client sends credentials or prompts. The old v0.0.9 approval
is not authority for changed code. No fake evidence, fixture capability or
measurement-only provenance bypass is allowed.

## 2026-10-06 production rollout

- Feature PR #9 merged; independent image publication run `37493659098` built
  source `26f3c06aec9f61f595b097547326746af060425b` twice and preserved the matching
  registry digest `sha256:a3f603d7188dfc0ee42ecf4fb8591481ce547a746b4ae26496089edaf3d4b7aa`.
- Digest-only PR #10 merged. Release tag **v0.0.10** identifies source/config
  `142f5d22823750f34c45d565b5c1f9788fb5de46`; measurement/publication invocation
  is `37495820434/attempts/1`.
- Manifest SHA-256: `3efd59e19458dd71cafd5c9593b8a9906e8fcd5f2c84a563bb38eca9951fd336`.
  Decoded measured config SHA-256:
  `3d1a527f8f3e49d493f4be22d2c6531d7e9803e53a7bf4de5053cf9099f1e5ee`.
  Config bytes match the tag, exact signed predicate matches the manifest, and
  Sigstore verification checked the expected signer workflow/source digest.
- Direct production update used the existing container with `hold:false`.
  The read-only plan selected blue/green with no required downtime. The serving
  container reports `current_tag:v0.0.10`, running, with no pending update;
  GitHub's latest release is v0.0.10.
- The installed native verifier accepted the serving production v3 hardware,
  provenance and endorsed keys with exact tag/manifest pins. Separately, the
  actual rebuilt Pi extension accepted its legacy-v2 public bootstrap through
  the pinned JS verifier and reached the secret prompt; it deliberately stopped
  there with **zero submitted credentials and zero inference requests**.
  Accepted external-v2 freshness/revocation limitations remain unchanged.
- Local release checks: **285 Rust tests**, three ignored live tests, strict
  format/Clippy, **436 reference checks and 17 pinned-Pi checks**, strict TS/build.
  Linux flake/browser/image/startup checks passed. The first image test job had
  one attestation-helper assertion failure; the failed job passed on rerun.
  Its transient cause was not established and no runtime gate was weakened.
- Client approval changed to these exact v0.0.10 pins without extending the
  administrative expiry (**2026-10-11**). WEB v0.0.8 approval is untouched.

The private real-Pi runner's loader smoke passed with fetch blocked and zero
sends. **Funded Pi multiple-call execution/continuation and balance reconciliation
remain unverified.** No new generation or usage quote was added by this rollout;
historical quoted use/holds remain 11 attempts / 7,792,060 microunits under the
10,000,000 quoted ceiling. This is not a provider-invoice or full Phase 0.2
acceptance claim.

## Subsequent user-observed Pi tool round

The user ran the approved production Pi integration and reported native `ls`
and `read` executions against public release metadata, followed by Kimi's final
answer with the correct `v0.0.10` tag and matching public commit/image information.
This is **user-observed live tool execution/result continuation**, not just a
model's claim that it ran tools. No further paid probe is required to demonstrate
the minimum tool loop. The pinned caller's receipt/epoch/result guards and
synthetic actual-SDK tests remain the supporting execution-policy evidence.

Exact model-turn count, single-batch timing, charges, account balance
reconciliation and provider invoice were not independently instrumented. The
user's other manual prompts and aborted/resubmitted request must not be assigned
invented counts, costs or refunds, or silently folded into the historical direct
upstream qualification ledger. An abort still does not prove provider cancellation.
This observation does not complete a whole-runtime privacy/accounting audit.

Pi 0.99.2 displays capability text in the highlighted **Model Name** detail below
the picker list, not beside the model ID in each row. The restricted `read,ls`
launch was for harmless testing; normal built-in coding tools can be selected
without a Possums-specific implementation or client-side spending reservation.

## 2026-10-07 catalog-wide release and native login

- PR #13 merged as `e189da8ed78911c09576cba95183de999af19761`. Every
  authenticated catalog model now gets the assumed shared OpenAI-functions
  wire profile. Unknown models still fail before tokenizer/generation; catalog,
  context, maximum reservation, settlement and caller execution checks remain.
  No paid per-model qualification or inference retry was added. The v0.0.10
  Kimi observation is not evidence that every other model works.
- Independent publication run `37572656156` reproduced and preserved image
  `sha256:11dde8e753af5d2dcb438e8494d59b42e93d4d0a58e88c85d9917f1e38c88655`.
  Digest-only PR #14 merged; **v0.0.11** pins source/config
  `c369340d5306d1e3f2e17c7d3f3d3fcbda528944`. Measurement/publication invocation
  is `37574318773/attempts/1`.
- Manifest SHA-256:
  `47aeaf9afa969814d2a8cb64d7f3008e3206d4173045f4f87bc27bdd8ea7b5ab`.
  Decoded config SHA-256:
  `a7777c6096d4d1b528105e581bfadd190d566880aa9c72d7c4f8999ad65725a0`.
  Config bytes match the tag. Cryptographic Sigstore verification checked the
  exact workflow/tag, source/signer digest, invocation and signed manifest.
- Direct, unheld blue/green production update completed: serving container is
  running **v0.0.11**, with no pending update. Native v3 verification accepted
  the serving hardware/provenance/endpoint keys with exact tag/manifest pins.
  Separately, the actual rebuilt Pi extension accepted the serving legacy-v2
  bootstrap through its pinned JS verifier and stopped at the secret prompt:
  zero credentials and zero inference requests. Accepted external-v2 freshness/
  revocation limitations remain unchanged.
  Runtime/CVM and administrative API approval expiry (**2026-10-11**) are
  unchanged; independent WEB v0.0.8 approval is untouched.
- Native Pi auth PR #15 merged as `fd5f50413353726e71933c6a5265e103defdc922`.
  The pinned SDK saves the recovery key through its ordinary private plaintext
  auth store, optionally resolves `POSSUMS_RECOVERY_CREDENTIAL`, restores a
  verified bearer/catalog at session startup without inference, and handles
  native `/logout`. Bearer/catalog remain memory-only. Request hooks receive a
  non-secret marker. Old session-only markers require one fresh `/login`.
  No global provider/default/history settings or separate Keychain store changed.
- Fresh strict TS/build and **25 actual pinned-Pi / 436 reference-admission
  checks** passed; native scratch-store/cancellation/logout/replacement tests
  and an independent bounded review passed. Rust checks counted **285 tests**,
  three ignored. Linux flake/browser/image/startup gates passed. Active LSP
  probes were inconclusive and are not counted as clean certificates.
- A memory-lease assertion exposed that generation-slot availability alone was
  not a release rendezvous; a test-only bounded full-permit wait fixed that
  assertion and passed 30 focused runs. Legacy attestation-helper and small-
  deadline startup fixture assertions each passed one bounded Linux rerun;
  their transient causes remain unestablished. An offline virtual-clock
  experiment timed out and was fully reverted. No production timeout, security
  gate or assertion was weakened.

No new live generation was sent for this release or auth work. Earlier manual
user prompts still have unknown counts/charges and are not folded into the
historical upstream qualification ledger. Exact live balance/accounting and
whole-runtime privacy acceptance remain unverified. At that release, compaction
remained blocked: paid harness summarization needed a separate safe
call/retry/token-cap integration (implemented below).

## Installed Pi 1.0.4 native compaction

The current client targets the qualified installed **Pi 1.0.4** peers and normally
loads through `pi install`, using the independently approved public manifest
beside `extension.mjs`. Historical Pi 0.99.2, v0.0.10 and v0.0.11 evidence above
is unchanged; this client change does not update gateway code or qualify models.

Manual and threshold compaction now return the root-exported native `compact()`
result through `session_before_compact`. Native Pi owns prompts, serialization,
previous summaries, split turns, file operations, successful combined usage and
checkpoint/context pruning. It makes one summary call, or two sequential calls
for a split turn; omitting the retry policy disables native summarization retries.
Recovery with `willRetry=true` is cancelled rather than authorizing regeneration.

The private callback reuses the existing verified provider receipt/EOF path,
selected model and native auth resolution. Only this callback removes the native
output-token hint. Gateway maximum reservation and submitted-rate accounting
remain authoritative; ordinary output overrides still fail closed. Summary
requests have no tools, cannot emit executable tool calls, and neither consume
ordinary run authorization nor change a pending receipted-tool continuation or
its conversation state. The callback closes after compaction; signal, run,
session and auth changes invalidate it before another send or checkpoint.

Failure cancels the hook, including when notification fails, instead of allowing
ExtensionRunner's default fallback. Safe, content-free category/stage and
observed settled charges appear only in a transient notification. A paid first
summary remains charged if the second fails; failing compaction creates no
checkpoint or persistent local error ledger and does not undo settled charges.
Consequently session totals need not include failed-summary charges. Abort does
not establish cancellation or refund. A deliberate `/compact` may pay again;
`/new` is the fresh-context alternative. Other providers are untouched.

Verification uses Node 24.13.0, scratch builds and actual installed Pi 1.0.4 with
synthetic credentials/transport only. Fresh strict TypeScript/build, **37 Pi
checks (mocks and actual SDK) and 436 reference/admission checks** passed. Active LSP probes were
inconclusive and are not counted as clean certificates. The redundant shell peer-link step was
removed: `build.mjs` already creates these links, and relinking through them can
write inside the installed SDK; the three self-links created by the initial
builds were removed without changing SDK code or metadata. Compaction changes
no dependencies or settings; normal installation adds only its package
declaration. No credential inspection, paid inference, live summary or new live
model qualification is part of this work.

## 2026-10-08 telemetry release and client approval

The serving gateway at this checkpoint was **v0.0.12**. Its tool/API profile and Pi 1.0.4
integration are unchanged; this release adds the reviewed aggregate telemetry
runtime, not new model qualification. See [privacy scope](../PRIVACY.md#reviewed-mvp-release-scope)
and [release evidence](verification.md#2026-10-08--v0012-measured-telemetry-deployment).

- Independent image publication `37733469152` produced matching digests:
  `sha256:11b9e8e80d21b22c5b38642135307c6fa719af4311316a6b038366a9f1830da1`.
- PR #18 pins the image; release source/config commit is
  `362089ea560d22b2ec8499862aa800de0ac124f6`.
- Manifest SHA-256 is
  `3218e00bd17906e9ba08b528f378163e5748161f7988a7b101c65371b83c6f6b`;
  decoded configuration SHA-256 is
  `c82a1d0eed2d91a8a7e162033765d9b84cf3f6e96de43d099b43ce0c3b100ec2`.
- Publication `37735269234/attempts/1` passed exact Sigstore workflow/tag,
  source/signer digest, invocation and signed-manifest verification. Config
  bytes match the tag; CVM remains `0.14.12`.
- The unheld blue/green update is serving v0.0.12. Native pinned verification
  accepted its hardware/provenance and matching endorsed/connection TLS keys.
  The rebuilt extension also accepted the serving legacy-v2 public bootstrap
  and stopped before credentials: no login or inference was sent.
- Fresh strict TypeScript/build, **37 Pi checks and 436 reference/admission
  checks** passed. The API approval alone changes; its administrative expiry
  remains **2026-10-11**, and the independent WEB approval is untouched.

Rebuild/reinstall the client package with the new approved public manifest;
previously installed packages retain their old pin and must fail closed rather
than silently trusting the new release. No global Pi settings or installed
package were changed by this verification. Earlier funded observations and
unresolved billing/privacy/legacy-v2 freshness properties remain historical;
this release does not retrospectively qualify them.

## 2026-10-08 configured-capacity release and paired client refresh

The serving gateway at this checkpoint was **v0.0.13**, adding only the six approved fixed
application admission-capacity gauges; the tool/API profile and linked request
suppression are unchanged. See [privacy scope](../PRIVACY.md#reviewed-mvp-release-scope)
and [scoped evidence](verification.md#2026-10-08--v0013-measured-capacity-release).

- Image publication `37831871064` produced matching independent digests:
  `sha256:d3d6bbbc5f856fd525792deeec1dfdcf3f717daba6004bf1d7a57cd198a8222e`.
- Release source is `ca4de91aeca61d1b7ac8501db27b8fed357ba444`;
  manifest SHA-256 is `d92ef5447f894f395c6b9b8dbfcfa97764d82a408cc88d604815446fc70d23f9`;
  config SHA-256 is `def47fe4df15aebb4f48b8caa5af8882a6bd4a3f96efc52a472cb4efc7dd1134`.
- Publication `37834605451/attempts/1` passed exact cryptographic identity,
  tag, source/signer digest, hosted-runner, invocation and signed-predicate
  checks. Config bytes match source; CVM remains `0.14.12`.
- The supported unheld blue/green update reached v0.0.13 with no pending update,
  preserving variables, secret references, SSH keys and resources. Debug is off
  and confidential mode on. Native manifest-pinned serving verification passed,
  with matching endorsed/connection TLS keys and a future explicit expiry.
  This does not prove uninterrupted availability or legacy-v2 freshness.
- Fresh strict TypeScript, **42 Pi checks and 436 reference/admission checks**
  passed. Actual rebuilt and installed extensions accepted the serving public
  legacy-v2 bootstrap and deliberately stopped before credentials: no login,
  catalog request, inference or compaction was sent.
- The operator-authorized installed pair now contains extension SHA-256
  `98b21c39b60733c8ceb116c493efa38beca2d738743b8b4ba711e1eb0d4ea7c8`
  and the matching v0.0.13 manifest. The previous versioned package is retained.
  Credentials, settings and Pi runtime are unchanged. Fully restart Pi to load
  the replacement; offline `/possums-status` reports v0.0.13.

Only API identity pins change. The independent WEB approval and API administrative
expiry **2026-10-11** remain unchanged. Earlier funded tool/billing observations
and unresolved model, retention and privacy properties are not requalified.

## 2026-10-09 description-cap release and accepted v2 paired refresh

The deployed gateway is **v0.0.14**. It removes only the separate 16-KiB
input tool-description cap. Aggregate body/parser, context, accounting, tool
history and generated-completion bounds remain unchanged. This does not prove
that the cap caused the operator's earlier rejected request or establish its
billing outcome. See the [scoped release evidence](verification.md#2026-10-09--v0014-description-cap-release-and-accepted-v2-pi-refresh).

- Independent builds matched image
  `sha256:11f9505c55ae5eb5b9c77bd2a4fc27f1a392782e28c96cf28b76846c78aa48d5`.
  Release source is `6e58d4be7e36afcf9372737ec7d235fe9d9dd423`; exact signed
  provenance, manifest/config equality and publication invocation
  `37877498948/attempts/1` passed independent checks.
- Manifest SHA-256 is
  `c6b418a1ce23d19882ae3ad3b2ac2f2d49189900dca824fa27c2548a90542202`;
  config SHA-256 is
  `9c7959ab4be5a6aedf2cac8a8688fe8da96bc4a2ee97ee53d4bc58d2a6310e23`.
- One supported blue/green update reached v0.0.14 running with no pending
  update, preserving variables, every secret reference, SSH and resources.
  No claim of independently measured uninterrupted availability is made.
- Native CLI v3 verification failed at its platform-endorsement signer check:
  the publisher moved to `cvmimage/platform-release.yml`, while CLI 0.19.0's
  SDK expects `platform-endorsements/build.yml`. This remains unresolved;
  native v3 quote/key verification and explicit freshness did not pass.
- The operator reaffirmed the already accepted **pinned JavaScript legacy-v2**
  path used by Pi. Treating the additional native v3 check as a prerequisite
  was an assistant-added scope error, not a newly agreed security requirement.
  No verifier dependency, signer policy, pin check or expiry was weakened.
- Fresh strict TypeScript/build, **42 Pi and 436 reference/admission checks**
  passed. Actual candidate, versioned and installed extension paths accepted
  the v2 public bootstrap and stopped before credentials. No login, catalog,
  inference or compaction was sent by these verification attempts.
- The installed extension hash is
  `cc76615c4467ec4837244db3132794d3235ee32b39fdf618b737702a16974d10`, paired
  with the v0.0.14 manifest. Atomic replacement retained the previous v0.0.13
  directory; credentials, settings and Pi runtime were untouched. Offline
  status reported v0.0.14 and the unchanged **2026-10-11** expiry.

Fully restart Pi after replacement. WEB approval remains independent and
unchanged. Legacy-v2 freshness/revocation limitations, earlier funded evidence,
live billing/model compatibility and telemetry/privacy unknowns remain as
recorded; passing v2 is not a retrospective v3 verification pass.
