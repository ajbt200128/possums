# Instance recreation recovery and exact v54 re-release

## Recovery record — 2026-10-18

The native fresh-attestation investigation observed collateral-unavailable HTTP
503 on the old instance, including after successful promotion of v58. That v58
control reproduced both v43's image and measured manifest exactly; its protected
production run `39165334632` confirmed one successful update but failed the
separate serving gate with `SERVING_SDK_FETCH`. See the
[v58 outcome](https://github.com/ajbt200128/possums/pull/95#issuecomment-6255122962).

The operator subsequently reported:

1. They deleted and recreated the instance, and **v58 then worked**.
2. They then confirmed **v54 works**.
3. They requested a written note and **v59 as an exact v54 re-release**.
   When explicitly offered v55's tighter settings instead, they selected
   **exact v54**, including open egress, 300-second startup grace and a 512 MiB
   memory-only `/tmp`.

These are **operator-reported recovery observations**, not new independent
post-recreation Admin API readback or pinned fresh-verifier transcripts collected
by this release packet. No recreation command, credential/secret selection,
instance identifier, quote, hardware identifier or raw platform error is retained.
The original failure remains historical evidence; it is not erased by recovery.

### Interpretation, not an invented root cause

Recovery with the same v58 release after recreation supports an
instance/provisioning/host/runtime-state or time-dependent collateral explanation.
It does not establish precisely what recreation reset, nor a universal
"v3 keys require deleting the instance" rule. Image changes are unnecessary to
reproduce the original failure, and the earlier feature-removal batches were not
proven fixes. Do not carry their speculative removals forward as required repairs.

Reviewed official evidence:

- [Attested-key lifecycle](https://docs.tinfoil.sh/containers/attested-keys#key-lifetime)
  and [CVM PR373](https://github.com/tinfoilsh/cvmimage/pull/373): a CVM reboot
  rotates keys; shim/container-process restart retains them.
- [Instance lifecycle](https://docs.tinfoil.sh/containers/lifecycle): updates can
  boot a fresh enclave without deleting the control-plane instance. Deletion
  permanently removes instance configuration, variables and secret bindings;
  retained volumes are a separate lifecycle. Recreation is not a routine retry.
- [CVM PR351](https://github.com/tinfoilsh/cvmimage/pull/351) described a 48-hour
  collateral bootstrap token and new control-plane preparation after expiry,
  but is **closed unmerged**, not the implementation in pinned CVM 0.14.12.
- In [CVM 0.14.12's native handler](https://github.com/tinfoilsh/cvmimage/blob/35d94c85ad20e9f98cd4ddcf35dfe4574b5215fb/tinfoil/cmd/shim/api.go#L331-L369),
  collateral acquisition precedes fresh device evidence and attestation building.
  The observed 503 identifies that availability boundary, not the upstream
  service's status or a proven key/publisher failure.

No further app rollback, instance deletion, weakened verification, diagnostic
retry or paid inference follows from this note. Historical native CLI v3 signer
compatibility, Pi freshness, billing and platform-retention limits remain scoped
unknowns. If root-cause work resumes, request closed collateral-stage/provider
status and provisioning-difference categories from Tinfoil, not raw evidence.

## v59 source and release contract

Restore the complete v54 runtime/build/test inputs from source
`28cbaf6418ff7e4f85f3b1080248147fa50f8cfc` on current main
`64694f061935c1f695c8b4a73ae0a9eee95d7e9b`, with a new recovery record. Keep
all current release/deployment scripts, workflows, SDK pins and Pi unchanged.

Expected image:

`sha256:d35ee72b6219a09e1bdf6b1423496817291bd39ef51b7077290d70ae54d5aad8`

The restored/current and historical source both evaluate to Linux image derivation
`/nix/store/hpbx5ydy4insn6d720c660ps8ss906gd-possums-gateway.tar.gz.drv`.
The normal release-only commit will replace the source template's image line;
its full measured config must then equal v54, not v55. Fresh A/B must reproduce
the expected digest; require exact public manifest/config comparison and normal
signature/provenance/current-main checks. Independent public verification of v54
release `8cf3e7dfa5c38ae30915f884105dd9249438904f` passed; its manifest digest is
`77b0cd278603d7d19c9f0bacea1952f4af1a46786154e2752c0f4f9196ca3eaa`.
The candidate config equals that verified config after the normal image binding.
Do not manually tag old code, substitute
an archive, replace historical assets, or bypass publication gates to force v59.

Restored behavior includes verified upstream-client renewal without inference
replay, tokenizer timeout/connect diagnostics, general TCP transport support,
Rust 1.92, NSS records and bounded general startup diagnostics. **Tor remains
unsupported and absent**, including its readiness observer and deployment
exceptions. Startup records remain closed and content-free, with at most six
initialization records and bounded best-effort delivery to the existing approved
Honeycomb destination. `OTEL_SDK_DISABLED=true` disables export, not platform
stderr capture. No request logs/traces or new destination/credential are added.

## Local verification checkpoint

- Pinned Rust 1.92: package formatting, strict all-target/all-feature Clippy and
  full offline locked suite **525 passed; four funded/live tests ignored**.
  Existing vendored unused-method warning remains. The unchanged devenv greeting
  says 1.88; the actual compiler follows `rust-toolchain.toml`, not that greeting.
- Pinned Go 1.27.2: offline helper/shared-verifier race tests passed with the
  existing module cache; no provider inference was invoked.
- Python 3.12: release suite **80 tests, one opt-in graph skip**; enabled Linux
  release-cache graph **13 passed**; image-health harness **9 passed**.
- Restored runtime, Rust tests, helper, manifests, toolchain, flake and source
  config compare exactly to the historical source. NSS freshness membership is
  restored with its strict graph tests; no build/provenance guard is weakened.
- Active LSP checks of 12 changed Rust/Go/Nix/Python paths were clean;
  `git diff --check` passed.
- Full suites include synthetic trust/accounting/cancellation tests, startup
  process hostile-data checks, local OTLP wire/off/outage/owned-I/O cases and
  renewal races. Historical shipped decoder evidence remains in the
  [renewal record](upstream-client-renewal.md); old client artifacts and whole-host
  E2E were not newly exercised here. No wire schema or Pi source changes.

Applied [data boundaries](../PRIVACY.md#data-handling-boundaries),
[forbidden telemetry](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[typed failures](../PRIVACY.md#mandatory-typed-interface-failures),
[restored startup exception](../PRIVACY.md#startup-diagnostics-exception),
[processor limits](../PRIVACY.md#processors-retention-access-and-shutdown) and
[required evidence](../PRIVACY.md#required-evidence-for-every-telemetry-change).
This is scoped source evidence, not a new live privacy/retention qualification.

At this checkpoint hosted checks, fresh A/B, v59 publication and public artifact
verification are pending. The release request is not an instruction to approve
another production update; any automatically queued deployment remains behind
its protected approval gate. The operator-reported working v54 instance is not
modified by this packet.
