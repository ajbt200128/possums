# Bounded runtime-path regression control

## Operator decision and intentional regression

**This candidate intentionally removes later verified upstream-client renewal.**
It restores v0.0.43's direct startup-owned client and TCP-only serving path,
plus Rust 1.88, on exact main
`7b1a1d1c6329431058aff1fd3b5c0629a9c8a75f` (v56 source). This is a bounded
operator-authorized regression control, **not a proven fix for native HTTP 503
or collateral acquisition**, and not a full source rollback.

The [renewal implementation record](upstream-client-renewal.md) is historical
for this candidate. A resident client no longer invalidates/reconstructs itself
after a connection failure, even for a later distinct operation. Upstream key
rotation may therefore leave it unavailable until a separately authorized
restart/replacement constructs a freshly verified client. Readiness does not
prove upstream reachability. No request is automatically retried or replayed;
no unverified observed key, fallback pin or weaker trust path is introduced.

## Exact scope

- `rust-toolchain.toml` selects **1.88.0** and `Cargo.toml` declares minimum Rust
  **1.88**. All dependencies, SDK/helper pins and lockfiles remain unchanged;
  no compiler-compatibility changes were required.
- `src/inference.rs` uses the SDK `Client` directly. Initial `Client::new` retains
  its 30-second bound and bounded `verification_document()` check. Catalog,
  tokenizer, generation and the ignored funded diagnostic use the same direct
  client. `http`/`authenticate` borrow `&self`; verification evidence is borrowed
  directly and immutably through the SDK. No renewal lock, acquired `Arc<Client>`
  snapshot or invalidation hook remains.
- The only differences from v0.0.43 in `src/inference.rs` are the retained
  tokenizer-send classifier/test module and its deadline/connect/unknown mapping.
  Both finer variants retain the shipped `tokenizer_send_failed` wire alias;
  endpoint-binding checks and all other validation remain.
- `src/server.rs` is byte-identical to v0.0.43: direct `TcpListener::accept`, the
  TCP-specific `DisconnectIo`, original lifecycle module inclusion and route
  formatting. Generalized ingress and its eight transport-only tests are removed.
  Existing TCP lifecycle/cause/observer tests are unchanged. Quiescing, disconnect
  cancellation, original-owner drain and serving-primary failure precedence remain.
- Delete `src/inference/verified_client.rs` and its tests. Preserve their useful
  loopback SDK certificate-rejection fixture in `tokenizer_send_tests.rs`, now
  checking the retained connect category and hostile URL-marker exclusion.
  Seven renewal-state tests are intentionally removed with the mechanism.

The current [NSS/all-startup-diagnostics removal](nss-startup-removal-verification.md)
remains intact: `src/main.rs` is untouched and still matches v0.0.43. Numeric
UID/GID `65532:65532`, image contents, and all three restored measured runtime
settings remain unchanged: egress allowlist, health `start_period: 60s`, and
memory-only `/tmp: "size=64m,mode=1777"`. No workflow, release, SDK/provenance,
attestation, accounting, API/client schema or signal/drain policy is changed.

## Local offline evidence

Existing pinned `devenv shell`, **rustc 1.88.0 / cargo 1.88.0**, Darwin arm64,
`CARGO_NET_OFFLINE=true`, `OTEL_SDK_DISABLED=true`:

- `cargo fmt --package possums -- --check`: passed.
- `cargo test --offline --locked --all-targets --all-features`: **496 passed,
  zero failed, four funded/live tests ignored**, across 26 test binaries.
- `cargo clippy --offline --locked --all-targets --all-features -- -D warnings`:
  passed. The vendored SDK's existing unused `CheckpointSignature::encode`
  warning remains; no project Clippy warning or compatibility patch was needed.
- All three tokenizer tests passed: predicate precedence/wire aliases, real
  loopback timeout/connect/request-build errors with hostile-marker exclusion,
  and the preserved SDK pin rejection before any authenticated HTTP request.
  The TLS fixture uses an intentionally untrusted local certificate; it is not
  successful attestation or a rotation/production transport fixture.
- All **14 TCP lifecycle, 14 cause and six observer tests** passed, including
  actual disconnect during tokenization, original-owner shutdown drain,
  shutdown-before-first-poll, fatal-accept drain and once-only failure reporting.
  The full suite also includes four silent-startup process fixtures, healthcheck,
  trust, accounting/refund, endpoint-binding, privacy and ordinary telemetry
  off/outage/owned-I/O/local OTLP-wire coverage.
- Active LSP checks on all three changed Rust files reported zero diagnostics;
  `git diff --check` passed. Source comparisons confirm exact v0.0.43 server/main
  and unchanged `Cargo.lock`, `flake.nix`, `flake.lock`, `tinfoil-config.yml`,
  vendored SDK and attestation helper relative to the starting source.

## Privacy and limits

Applied [data boundaries](../PRIVACY.md#data-handling-boundaries),
[forbidden telemetry](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[typed failures](../PRIVACY.md#mandatory-typed-interface-failures),
[startup silence](../PRIVACY.md#startup-diagnostics-exception),
[processor/shutdown limits](../PRIVACY.md#processors-retention-access-and-shutdown)
and [required evidence](../PRIVACY.md#required-evidence-for-every-telemetry-change).
The offline privacy, hostile-diagnostic and local-wire tests above are scoped
regression evidence, not a new whole-runtime privacy qualification. No metrics,
logs/traces, request-level signals, support export, credentials or live endpoint
access were added or used. Native host/platform history remains a separate boundary.

No isolated historical image rebuild, Linux artifact/runtime execution,
installation, activation, publication, deployment, live inference or live recovery
is established here. Those are separate parent/operator work. Shipped-client
wire fields/aliases are unchanged, but old client artifacts were not newly
requalified end-to-end. Native v3 CLI signer mismatch, legacy-v2 freshness,
collateral-fetch cause, provider retention/invoice parity and physical cleanup
remain unresolved. A passing local control does not identify why native serving
failed or authorize a release, retry or additional charge.
