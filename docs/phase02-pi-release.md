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
whole-runtime privacy acceptance remain unverified. Compaction remains blocked:
this is paid harness summarization, not a missing Tinfoil endpoint, and needs a
separate safe call/retry/token-cap integration.
