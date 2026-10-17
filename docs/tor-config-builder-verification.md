# Tor client configuration: offline NSS failure candidate

**Shipping acceptance remains on hold.** The factory-only fix is insufficient
for actual SDK construction under missing NSS; see the follow-up below.

## Scope

Source candidate on base `dbc623391940131229b90c32df6cc6eb4e8af679`.
No production mutation, credentials, private identity reads, inference, signing,
TorClient construction, bootstrap, publication, endorsement, deployment or nested
agents were used for this investigation. Existing public deterministic A/B
fixtures exercise `PreparedTor::prepare_with_grants` and ephemeral insertion.
The ready-cancellation fixture does not poll its bootstrap future.

Policy: [data boundaries](../PRIVACY.md#data-handling-boundaries),
[forbidden telemetry](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[startup diagnostics](../PRIVACY.md#startup-diagnostics-exception), and
[startup trust boundary](tor-startup-removal-verification.md).
No telemetry, diagnostic grammar, API/client wire contract, SDK dependency or
trust/accounting policy changes are included.

## Reproduced mechanism and minimal change

Pinned Arti 0.47.0 `OnionServiceConfig::builder().nickname(...).build()` passes
independently. The production client builder also passes on ordinary Darwin,
including an empty-environment child with `HOME=/tmp` and network denied.

The client builder constructs fs-mistrust 0.16.0. Its default
`TrustedGroup::SelfNamed` resolves the current username through `pwd-grp` 1.0.2
before resolving the group. An NSS lookup error propagates through
`MistrustBuilder::build_for_arti` as `ConfigBuildError::Invalid`, with the finite
nested field **`storage.permissions.permissions`**. No formatted SDK cause was
printed or retained. Sources: fs-mistrust `src/user.rs`, tor-config `src/mistrust.rs`
and `src/err.rs`, arti-client `src/config.rs` in the pinned registry packages.

An isolated local Linux/aarch64 glibc 2.42 process, UID/GID 65532, read-only root,
no capabilities, no network and no passwd database returned `ENOENT` from
`getpwuid_r`. This is a libc-boundary reproduction, **not the shipping image or
its exact libc/architecture**.

A public Darwin test-only dylib interposed `getpwuid_r` and `getpwnam_r`, returning
`ENOENT` and a null result pointer without reading any database. Under the same
production Cargo features, a network-denied, empty-environment Rust test child
then reproduced the client builder's typed field above while the service builder
still passed. The actual extracted production factory and full public-fixture
preparation both failed before the setting and passed after it.

The fix selects the supported `trust_no_group_id()` API only for the client
configuration. It avoids the default NSS lookup and **tightens** filesystem
permissions: current-user/root ownership trust remains; no group access is
trusted. It preserves `ignore_environment()`, explicit scratch-relative cache and
state paths, 0700 scratch permissions, and the ephemeral primary keystore.
The factory extraction allows direct key-free regression testing of the exact
configuration production uses. No disk keystore or fallback is introduced.

## Checks

Pinned root `nix develop --offline`: Rust/Cargo **1.92.0**, Darwin/aarch64.
The image's `torCommon.cargoExtraArgs` is `--locked --features tor`; default
features are empty, and the shipping package disables build-time tests.

- `cargo test --offline --locked --all-targets --features tor`: **588 passed,
  4 ignored** (funded tests remain ignored).
- Default `cargo test --offline --locked --all-targets`: **531 passed, 4 ignored**.
- `cargo test --offline --locked --all-targets --all-features`: **588 passed,
  4 ignored**, not substituted for the exact production-feature run.
- `cargo build --offline --locked --features tor`: passes.
- Package fmt check and all-target strict Clippy in no-default-feature and
  all-feature modes: pass. An existing vendored tinfoil dead-code warning remains;
  it does not fail these root-package checks.
- Final focused injection: two independent builder tests and existing full
  preparation test pass. The client test verifies ephemeral selection, empty
  scratch contents, 0700 acceptance and 0750/0770/0777 rejection. It also passed
  under injection with both SDK permission-disable environment variables set,
  confirming those environment values do not disable the configured checks.

The all-target suites use existing synthetic local loopback fixtures; the
focused injection children deny network entirely. No Go/Node/receipt-contract
changes were made, and unrelated language suites were not repeated.

## Remaining limits

The operator reported that the newer failed v51 attempt reached Tor preparation
and `tor_configuration_invalid`, unlike the earlier identity-secret failure;
there was no reported connect/bind/bootstrap/launch milestone. This is supplied
progress/temporal evidence, not a captured underlying SDK cause or HTTP status.
The NSS fault mechanism is reproduced locally and fits that boundary; **its
attribution to the deployed failure remains unconfirmed**. No claim of successful
rollout, reachability or native verification follows from these tests.

A pinned Linux Rust 1.92 toolchain was not available locally for executing the
actual SDK builders. Shipping non-dev feature-closure equivalence and the
candidate image's actual NSS behavior still need independent qualification.
The ordinary Darwin preparation result alone does not establish either.
The independently owned combined integration and any production action remain
with the parent/operator. All fresh per-request/client attestation gates and the
approved B check remain unchanged; startup self-appraisal is not restored.

## Follow-up: SDK builder default and measured-image account records

Independent synthetic-only, network/write-denied acceptance found that factory
preparation succeeds with the first fix but the actual SDK builder still panics
under unconditional missing-NSS injection. Split stage projection isolated
**`TorClient::builder()` itself**, before config setting, Manual behavior,
`create_unbootstrapped` or client key-manager access. The test catches the panic
without inspecting/formatting its payload and reports only its closed stage code.
Ordinary non-injected constructor acceptance passes. Combined ingress/TLS/stream/
accounting/drain acceptance did not construct the SDK and is not constructor
coverage.

Pinned `arti-client/src/builder.rs:90–99` initializes `TorClientConfig::default()`
in its private builder constructor. Both public `TorClient::builder()` and
`TorClient::with_runtime()` call it. `tor-config/src/derive.rs:2429–2433` generates
Default by unwrapping the default builder's result. Thus the SDK default's NSS
lookup happens before our explicit stricter configuration can replace it. No
config-first public constructor was found in the pinned API. No SDK fork, version
change, panic-based recovery or global environment shim was introduced.

Follow-up source candidate **`73e0e40c8b6af3f2ca95ebdfccc06eb03b102830`** adds
pinned Nix `dockerTools.fakeNss` to the gateway image, with a locked `possums`
account and group at UID/GID 65532, `/tmp` home and nonexistent `/noshell` shell.
The helper also supplies its standard root/nobody records, `hosts: files dns`
nsswitch file and empty `/var/empty`. No login program or usable password is
added. Image UID/GID, read-only runtime, existing strict permission checks,
ephemeral identity handling, SDK constructor chain and all trust/accounting gates
remain unchanged. The previous stricter no-group-trust factory setting remains.

A new `checks.gateway-accounts` reads the exact helper included in the image and
checks the fixed account/group records. It builds and passes under pinned,
offline Darwin Nix. The generated files match the evaluated x86_64-linux image
account-file derivations byte-for-byte. The Linux/aarch64 glibc 2.42 fixture at
UID/GID 65532, with no capabilities, network or writable root, now compares
**real absent versus available files**, without a lookup interposer:

- Absent passwd database: closed database-absent result, exit 2.
- Generated passwd/group/nsswitch files mounted read-only: UID, username and group
  lookups all validate the expected fixed values, exit 0.

This remains libc-boundary evidence, **not execution of the actual shipping
Linux gateway/SDK or inspection of the completed shipping image**. The exact
x86_64 image derivation evaluates, but its offline dry run needs 5,706 uncached
derivations; no local pinned Linux Rust 1.92 toolchain or shipping image artifact
was available. An appropriate offline artifact/build path is still required for
actual shipping-image NSS and constructor before/after acceptance. Forced
unconditional `ENOENT` injection necessarily still defeats the SDK default even
with correct image records; it must not be relabeled a passing image test.
No candidate is shipping-qualified, and deployed missing-NSS attribution remains
unconfirmed. No new private identity read, deployment or production action was
performed for this follow-up.
