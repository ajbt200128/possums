# Tor prerequisite: pinned Rust upgrade

> Historical evidence only in this inactive HTTPS-only backup. Tor code/assets
> are removed; commands and authorizations below do not apply to this branch.
> Rust 1.92 remains pinned. See [backup verification](without-tor-backup-verification.md).

Source-only on `feat/tor-v0`. The operator approved upgrading the pinned Rust compiler to support released Arti 0.47.0, whose declared minimum is 1.92. Arti remains unpatched; unsupported required Tor protocols may terminate the whole gateway under the accepted fail-fast policy. This is not an installation, deployment or live Tor qualification.

## Changes

- `rust-toolchain.toml`: 1.88.0 → **1.92.0**.
- `Cargo.toml`: package minimum 1.88 → **1.92**.
- The existing locked rust-overlay supplies the compiler; **no flake input or lock update** was necessary.
- Rust 1.92 Clippy rejects seven existing modulo comparisons. Replace only those comparisons with unsigned `is_multiple_of` checks at the same constant nonzero divisors, including two tests. No telemetry labels, values, cadence, export, lifecycle or accounting policy changes. Do not suppress the new lint or reformat the vendor.

Applicable privacy requirements: [mandatory reference/change control](../PRIVACY.md#mandatory-reference-and-change-control), [aggregation integrity](../PRIVACY.md#aggregation-is-necessary-not-sufficient), and [required telemetry evidence](../PRIVACY.md#required-evidence-for-every-telemetry-change). `PRIVACY.md` was read before the mechanical telemetry-source edits. Existing synthetic/offline aggregate, lifecycle, hostile-data and outbound-wire tests run as part of the full suite; no live processor/export activation was performed.

## Parent verification

Pinned Nix, Darwin/aarch64: `rustc 1.92.0 (ded5c06cf 2025-12-08)`.

- Full `cargo test --offline --locked --quiet`: passed; four existing ignored tests remain ignored. Includes **319 library tests** and integration tests.
- Root package formatting and strict all-target/all-feature Clippy: passed.
- Unpatched Arti spike `--features arti`: **14 tests passed**, including three supported key-manager reopen/restore/identity tests. The real HTTP/DataStream embedding function compiled but was not invoked; no Tor bootstrap/publication occurred.
- Arti spike strict all-target Clippy: passed.
- An initial invocation used a nonexistent `http-embedding` feature; corrected to the actual `arti` feature. This was command selection, not a compiler failure.
- Existing vendored `CheckpointSignature::encode` dead-code warning remains; vendor files were not changed.

Reproduce:

`nix develop --offline --command cargo test --offline --locked`

`nix develop --offline --command cargo fmt --package possums -- --check`

`nix develop --offline --command cargo clippy --offline --locked --all-targets --all-features -- -D warnings`

`nix develop --offline --command cargo test --offline --locked --manifest-path spikes/tor-arti/Cargo.toml --features arti`

`nix develop --offline --command cargo clippy --offline --locked --manifest-path spikes/tor-arti/Cargo.toml --all-targets --features arti -- -D warnings`

## Remaining scope

No Linux measured image/build or installed/deployed gateway is qualified here. Arti is still an isolated spike, not wired into the gateway. Stable onion identity/guard persistence, egress, real rendezvous/streams and the app-key-to-onion trust chain require their own integration and authorized runtime qualification. Compiler success does not establish memory headroom, safe diagnostics from upstream fatal paths, graceful accounting on process exit or usable onion service.
