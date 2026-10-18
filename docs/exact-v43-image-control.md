# Exact v43 image under a new release identity

> **Historical v58 control:** promotion succeeded but fresh serving failed.
> The operator then reported v58 working after instance recreation, followed by
> v54 working. The [recovery/v59 record](v54-rerelease-v59.md) distinguishes those
> reports from the original failed verification and restores exact v54 inputs.

## Operator decision and binding

The operator approved a new measured release containing the exact v43 image,
not another partial feature removal. Base source is v57 main
`937369cd1e57340cec4c2e0c840d2689d0164354`.

The separate fresh A/B control `39156837645` checked out exact v43 release
`c22a55f0f710b8343422a9b03776161e0dba67d5`, passed strict cold-output isolation
and packaged health, and reproduced the original image:

`sha256:f05f08b739f067d6f5acdadbe3d3ced9cfab1514c1f0fe1e42d16d60c6a06d25`

This candidate restores the historical image inputs on current main so normal
source/image binding remains truthful. It does **not** substitute an unrelated
archive into today's source, bypass freshness checks, replace v43 assets/tags,
or weaken current-main/provenance/protected-approval rules. Current release and
deployment scripts/workflows, Python guards and Pi sources remain unchanged.
A new release tag/provenance does not necessarily produce a new measurement:
identical image/config/platform inputs may yield the same measured digest.
Compare actual publication artifacts rather than assume a different digest.

## Exact changes and intentional regressions

Restore `src`, Rust test inputs, `Cargo.toml`, `Cargo.lock`, `flake.nix` and the
attestation-helper inputs exactly to v43. This removes the remaining tokenizer
send-category refinement, helper acquisition wrapper, direct `ring` edge,
telemetry spelling changes and newer compiled test/source-filter inputs.
Restoring test bytes is necessary for the original Nix source/derivation identity;
it is not permission to skip tests. No package version, SDK or verifier pin changes.

- Tokenizer send errors again use the existing coarse `tokenizer_send_failed`
  wire detail and `tokenizer_transport_failed` metric category. The additional
  deadline/connect categories are absent, **not healthy zeroes**. Error labels
  return from 56 to 54; token aggregates and complete-window gates are unchanged.
- Helper failures again use the old internal errors and nonzero process exit.
  `main` discards those errors without formatting/logging them; the Rust owner
  retains its closed unavailable/invalid projection. Finer acquisition causes
  are intentionally unavailable in this exact-artifact control.
- Startup diagnostics/NSS remain absent, Rust stays 1.88, and later upstream
  client renewal remains absent. A rotated upstream pin may require separately
  authorized replacement; TLS still fails closed and no request is replayed.
- Attestation, nonce/provenance/endpoint-key verification, accounting, settlement,
  disconnect/drain, parser bounds and the existing shipped wire contract remain
  those of v43. Existing egress allowlist, 60-second grace and 64 MiB memory-only
  `/tmp` are preserved. Production configuration stays opaque and unchanged.

These scoped diagnostic regressions are part of the explicit exact-image
experiment, not a general relaxation of future typed-interface requirements.
Earlier richer diagnostic/test records remain historical, not current coverage.

## Local verification checkpoint

- Current candidate and exact v43 both evaluate to Linux image derivation
  `/nix/store/czyh8flrkyvndww1qqh875a7h6di96yq-possums-gateway.tar.gz.drv`.
  The shared derivation binds the complete build recipe/inputs, not a deployed
  artifact or a newly observed archive digest. Normal fresh A/B must still pass
  and the published image must equal the expected digest above before approval.
- Pinned devenv Rust/Cargo 1.88: formatting, full offline all-target/all-feature
  suite **488 passed, four funded/live tests ignored**, and strict Clippy passed.
  Existing vendored unused-method warning remains.
- Pinned Go 1.27.2 helper/shared verifier race tests passed offline using the
  existing module cache. The first invocation used devenv's unpopulated module
  cache and stopped at dependency lookup; selecting the already-populated cache
  resolved that setup limitation without changing pins or fetching dependencies.
- Pinned Python 3.12 release tests: **80 tests, one opt-in graph test skipped**;
  enabled Linux graph suite **12 passed**. No release/deployment guard is removed.
- Active LSP probes for nine changed production Rust/Go paths were clean;
  `git diff --check` passed. Shipped-client artifacts were not newly exercised
  end-to-end; existing wire details remain unchanged and Pi sources are untouched.
- `git diff v0.0.43 -- src Cargo.toml Cargo.lock rust-toolchain.toml flake.nix
  flake.lock attestation-helper ':(glob)tests/**/*.rs'` is empty.

## Privacy and interpretation limits

Applied [data boundaries](../PRIVACY.md#data-handling-boundaries),
[forbidden telemetry](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[typed-failure control scope](../PRIVACY.md#mandatory-typed-interface-failures),
[startup silence](../PRIVACY.md#startup-diagnostics-exception),
[processor limits](../PRIVACY.md#processors-retention-access-and-shutdown) and
[required evidence](../PRIVACY.md#required-evidence-for-every-telemetry-change).
Full offline suites retain hostile-data, local OTLP wire, off/outage, accounting,
trust and privacy tests. No request data, credentials, raw platform diagnostics,
quotes, new telemetry destination or paid inference is part of this control.

At this checkpoint, new-candidate hosted CI, fresh publication, production update
and fresh serving acceptance remain pending. A pass would support an image-linked
explanation but not prove which residual change caused it; a failure would show
those residual application changes are unnecessary to reproduce the failure.
VM/provisioning conditions, release/platform processing, physical cleanup, native
collateral transport/status/format/expiry cause, native CLI v3 signer mismatch,
Pi legacy-v2 freshness and runtime billing/privacy properties remain unresolved.
