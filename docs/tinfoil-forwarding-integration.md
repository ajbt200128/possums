# Forwarding integration onto current main

**Scope:** committed source, local offline/synthetic qualification and PR CI only.
This packet stops before merge, publication, production approval, deployment or
client installation. Parent-owned independent acceptance is still required.
The historical [accounting/review record](tinfoil-aligned-accounting.md) remains
intact; its 346-test and uncommitted-state statements are pre-integration evidence.

## Source and preserved decisions

Rebased parent feature `b3e79a6` and separate sizing `57525d9` from `48dcd2a`
onto fetched main `b97d3926484c61d8685f50109ae893fdd7ae6e3b` (PR57).
The integrated feature is `b10fee2`; requested sizing is `7985a45`; documented
conditional fallback is `a44bf4f`. No overlapping file was selected wholesale.
Conflict resolutions combined native forwarding with actual current-main behavior:

- Keep `b6c1868`'s 32-MiB full HTTP/client/tokenizer/generation envelopes and
  600-second chat upload/send-header/read-idle baselines. Control bodies retain
  4-KiB / 30-second limits. No whole-generation or connected-backpressure deadline.
- Keep `0e629bc`'s removal of cumulative stream bytes/frame/event quotas.
  Per-record, ciphertext/chunk, parser depth/node and terminal-evidence checks
  remain. Native SSE events use the forwarding candidate's larger individual
  record allowance; they do not restore a cumulative quota.
- Keep `d2af9a2`'s removal of shared Heavy/Ingress chat admission. Generation,
  Heavy and Ingress are passive lifetime markers; only the existing connection
  capacity is exported. No replacement generation semaphore or control gate.
- Keep `428ab02`'s removal of live tool-count/per-call/total-argument quotas.
  Rust retains bounded metadata and the client charges retained tool state
  against the request-derived 32-MiB envelope, before growth. Keep
  `ToolStateTooLarge` and all current transport/timeout categories.
- Keep `d67aad0`'s bounded asynchronous delivery-capacity waits, including
  dequeued/sliced frame credit and wakeup on body closure. Native forwarding
  awaits that same storage primitive; connected slow readers are not detached.
- Production uses verified native reqwest DATA/TRAILERS and the existing SDK SSE
  decoder, not `ProtocolParser` or synthetic role/delta/finish reconstruction.
  The legacy synthetic adapter now awaits each borrowed event directly instead
  of adding a `try_send` bridge/overflow policy. Legacy callback consumers use an
  async adapter over the same native consumer, not another provider decoder.
- Actual disconnect cancels upstream/tokenization while the sole owner settles
  once from last wholly validated cumulative evidence. Original quote, one
  30% markup/rounding, cache split and reservation cap remain. Provider cost
  stays separate. Incomplete receipts never authorize success/tools; opt-in
  native final-metrics trailers additionally require complete settled receipt
  and authenticated upstream EOF. The invalid-model/namespace refutation and
  missing-finish/no-trailer regression remain unchanged in substance.

Main's overlapping-worker fixture now holds nine **connected** workers before
asserting passive counts and no admission gate, then drops all bodies without
releasing upstream gates and verifies nine refunds/no replay. Retaining the old
expectation that disconnected workers drain would contradict approved cancellation.
The terminal-delivery fixture uses valid native grouped finish/usage to fill the
last queue credit, then proves settlement precedes a blocked receipt and survives
body loss. Neither fixture restores a production gate or parser.

Two small CI/source lints were corrected: use one input pair in `compose_core`
instead of eight arguments, and directly validate `other_cost_usd` rather than
looping over one field. Formatting changes are limited to newly integrated code.
The standalone SDK patch/provenance in `vendor/tinfoil/POSSUMS.md` remains valid;
no SDK dependency/version, attestation policy or wire parser was replaced.

## Local rerun evidence

No dependency installation or paid/live inference. Existing pinned node_modules,
compiler/module closures and isolated Linux image were reused. Rust telemetry was
disabled. Fixture service stdout/stderr were discarded, Docker log driver disabled,
core dumps disabled, environment sanitized and generated fixture material removed.

- `OTEL_SDK_DISABLED=true cargo +stable test --offline --locked --lib --tests --quiet`:
  **356 passed, 3 live tests ignored, 0 failed** (210 library, 146 integration).
  Includes new-main envelopes/no-admission/tool-state/backpressure cases, closed
  diagnostics/privacy negatives, native usage/trailers/cancellation and accounting.
- Standalone vendored SDK SSE tests: **11 passed**, 217 filtered, no failure.
- Rebuilt `phase01_fixture` from current source offline and used it for the actual
  pinned shim → production `server::serve_until` → synthetic upstream runs.
- Fresh current-source scratch package build/typecheck passed with Node
  **24.13.0**, TypeScript **5.9.3**, esbuild **0.25.10**, Pi **1.0.4**, EHBP **0.3.2**.
  Copied TypeScript sources matched checkout bytes; no root-client install occurred.
- Full `clients/pi/check.mjs`: **39 reference**, **642 phase02 client**, **177 release**,
  **39 public-cache**, **148 Pi harness**, **28 discovery**, and **71 admission**
  checks passed, plus forwarding native/cache/cap/partial-receipt checks.
  Pi counts include existing permission/Stop/retry-presentation blockers, not
  resolved guarantees. No production approvals or live inference were added.
- New-main `phase01_stream_limits`: **65,541 encrypted frames**, **76,809,934 bytes**
  pass without cumulative quotas; malformed/oversized records, damaged framing,
  missing terminal receipt/DONE and interrupted authenticated EOF still fail.
  Its oversized SSE fixture now splits one record across individually valid
  ciphertext frames so it tests the record ceiling, not the independent frame cap.
- `build.sh` now actually copies `phase01_stream_limits` (already required by
  main's check runner) and `tinfoil_forwarding_client`; the check runner executes
  the forwarding test. Dependencies were reused rather than invoking npm install.
- Recompiled current Go shim overlay with cached **Go 1.27.1**, `-mod=readonly`,
  `GOPROXY=off`, `GOSUMDB=off`, `GOTOOLCHAIN=local`, network disabled.
  Official config **v0.1.14** static fixture passed at restored **2/8192** and
  confirms no other config difference from main. Public source hashes still match
  the historical shim record (API/go.mod/go.sum); no platform appraisal fabricated.
- All **three rebuilt shim fixtures exited 0**: native opt-in trailer/body EOF;
  actual reference API progress/credit refusal/cancellation (last snapshot **4**
  microunits); and production Rust/shim native negotiated/unnegotiated trailers,
  allowlist, provider cost separate from **7**-microunit receipt charges, plus
  cancellation without drain (**4** microunits). Same deliberate request counts
  as the historical qualification; HTTP/1.1 `TE: trailers` remains required for
  wire trailer delivery. No browser/physical Android rerun.
- Release/cache Python suite: **45 passed**. Focused rustfmt/diff checks passed.
  Active LSP batch: 12 integration paths confirmed, no primary-language diagnostics;
  auxiliary style hints/warnings remain, with no whole-checkout clean claim.

Local stable Rust/Cargo are **1.99.0**; configured **1.88.0** still lacks Cargo.
Strict stable clippy additionally reports existing main telemetry/tool lints and
1.99-only deprecations/style changes. Only the two source-owned lints above were
fixed; no unrelated telemetry cleanup, warning suppression or toolchain install.
Pinned hosted CI is the release qualification gate; local stable clippy is **not**
claimed clean and no pinned local Cargo pass is claimed.

## Sizing and release boundary

Official public valid-value lists establish that 1/4096 is outside the supported
resource options. The authorized fallback restores exactly **2/8192** in a
separate explanatory commit; [sizing evidence](tinfoil-sizing-fallback.md) distinguishes
static schema acceptance, public support contract, internal canonical verifier
and an actual control-plane rejection (not performed). Final config matches main.
`release.py`'s exact canonical-shape/signature/config guard was **not altered**.

Authenticated read-only GitHub checks confirmed user `ajbt200128`, current main
above and both automation switches `true`. Opening/pushing this PR does not
merge it. A later main merge can trigger automatic image publication and protected
production deployment approval. This packet does not tag, rerun a publisher,
approve an environment, merge or deploy. Parent must inspect the actual integration
range and exact successful PR head/checks before taking the separately authorized
next steps. No hosted success is inferred from local tests.

## Privacy and remaining unknowns

Policy references: [data boundaries](../PRIVACY.md#data-handling-boundaries),
[forbidden diagnostics/content/identifiers](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[reviewed aggregate scope](../PRIVACY.md#reviewed-mvp-release-scope),
[processor/history boundaries](../PRIVACY.md#processors-retention-access-and-shutdown),
[synthetic testing](../PRIVACY.md#synthetic-testing-exception) and
[evidence](../PRIVACY.md#required-evidence-for-every-telemetry-change).
No raw provider errors/identifiers, credentials, requests or request telemetry
enter this record; no observability family, retention, exporter or persistence
expanded. Current main's admission export removal is retained, not reversed.
GitHub public PR/CI artifacts and native host history persist; this is not a
whole-process-transience claim.

Live interim usage/trailer availability, immediate upstream compute termination,
invoice parity, deployed cancellation latency, whole-process memory headroom and
runtime/processor retention remain unknown. The official Go helper's V3 path is
not native CLI V3 acceptance; historical native platform-signer mismatch and
Pi legacy-V2 freshness limitations remain unresolved. Paired receipt deployment,
release provenance/attestation/endpoint binding and installed-client acceptance
belong to subsequent authorized verification, not these synthetic results.
