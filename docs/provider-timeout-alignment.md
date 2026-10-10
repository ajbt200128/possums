# Provider timeout alignment — local source candidate

No installation, deployment, push or live inference. Historical release records
remain historical. The reverted Free path is not restored. This work is separate
from the `fix/specific-client-errors` diagnostic worktree.

## Decision and scope

The approved audit found that Tinfoil JS passes options to OpenAI without its own
timeout override. OpenAI 7.19's default 600,000-ms fetch timeout ends at response
headers. Tinfoil Python delegates `NOT_GIVEN` to OpenAI's 600-second
read/write/pool and five-second connect settings; read inactivity is per chunk,
not total generation duration. These are client semantics, **not evidence of a
numerical Tinfoil inference, edge, availability or deployment-drain guarantee**.

### Gateway

- Both text and tool generation allow 600 seconds for send/response headers, then
  600 seconds per HTTP read. There is no total stream/generation timer. Comments
  and partial JSON count as byte progress. SDK events have no additional idle
  timer: one event may require many healthy HTTP reads.
- Tokenizer, its upload-release wait, outer context preflight and authenticated
  catalog control waits are bounded at 600 seconds, not 30/300 seconds. These are
  finite preflight operations, not a limit on inference output duration.
- The existing verified reqwest builder supports `connect_timeout(5s)` directly.
  It bounds connection establishment (including TLS), not just a raw TCP syscall.
  No duplicate transport, TLS adaptation, origin relaxation or retry was added.
- Hyper HTTP/1 serves **one request per connection** (`Connection: close`). This
  replaces the 600-second absolute connection-age kill, which could truncate
  active work. Clients reconnect normally; pipelined subsequent requests are not
  served. The 64-connection admission bound, ten-second incoming-header timeout,
  30-second incoming-upload deadline, and parser/memory limits remain unchanged.
  No graceful shutdown/drain change is included.
- Existing detached generation ownership is unchanged: downstream disconnection
  does not cancel upstream generation. Consume the authenticated terminal outcome
  and settle/refund exactly once. No gateway replay, text-estimated billing,
  relaxed usage/finish/EOF validation, or new accounting authority was introduced.
  The retired deadline error category stays recognizable for compatibility with
  older gateways; the new stream consumer does not emit it.

## Local verification

All workloads are synthetic/offline; no real credentials or provider inference.
Rust 1.88.0 was supplied by the existing Nix toolchain because the rustup shim's
configured toolchain lacks Cargo. No toolchain or dependencies were installed.

```sh
PATH=/nix/store/89rqzskr6m71aqpxrglhyifrszxf3a54-rust-minimal-1.88.0/bin:$PATH \
  cargo test --offline --lib --tests
PATH=/nix/store/89rqzskr6m71aqpxrglhyifrszxf3a54-rust-minimal-1.88.0/bin:$PATH \
  cargo test --offline --manifest-path vendor/tinfoil/Cargo.toml verifier::tls::tests
```

Results: **302 passed, three live tests ignored**; **10 vendor TLS tests passed**.
The root `cargo test -p tinfoil` shortcut cannot run the non-workspace package's
dev-dependencies, so the standalone manifest command above was used.

- Paused-time Rust tests advance healthy SDK comments/fragmented JSON and raw
  SSE reads beyond 600 elapsed seconds. Stalled reads still fail, including after
  terminal usage/DONE before EOF. Malformed usage and transport faults fail closed.
- The production Hyper builder delivers a held body after 1,200 simulated seconds,
  closes instead of serving a pipelined second request, and still times out initial
  idle headers. Real loopback HTTP tests cover reconnect and oversized uploads.
- Existing ownership/accounting/privacy suites cover disconnect, exact-once
  terminal settlement/refund, malformed terminal data, cancellation, leases,
  no automatic replay and content-free aggregate observations.

## Privacy and limitations

Policy references: [Data handling boundaries](../PRIVACY.md#data-handling-boundaries),
[permitted and forbidden telemetry](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[processors and shutdown](../PRIVACY.md#processors-retention-access-and-shutdown),
[synthetic testing](../PRIVACY.md#synthetic-testing-exception).

No new logging, request traces, metrics, labels, exports or credential persistence.
The existing connection-rejection observation now classifies header timeout rather
than an absolute connection-age timeout; metric vocabulary and release gates are
unchanged. These source tests are not renewed live privacy, billing, retention,
attestation or platform-limit evidence. Existing legacy-v2 freshness and native
v3 CLI platform-signer limitations remain unresolved.
