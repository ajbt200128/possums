# Arti fully-reachable Tor health — source candidate

The operator authorized replacing the strict SDK `Running` health requirement
with Arti's supported `State::is_fully_reachable()` predicate. Pinned Arti0.47.0
returns true for `Running` and `DegradedReachable`; this is an SDK assessment, not
an independently verified connection or attestation.

Only `engine_status`'s readiness assignment changes to
`state.is_fully_reachable() && !fatal`. Existing process/quiescence health,
fatal/Broken/post-start Shutdown, EOF/panic, owned drain, startup preparation,
600s debug grace and trust/accounting controls remain unchanged. The finite
startup observer still records **first SDK Running**, separately from current
health; this patch does not reinterpret historical startup evidence.

## Offline evidence

The actual engine-to-router test now covers initial Shutdown/Bootstrapping,
Running → DegradedReachable (health remains204) → DegradedUnreachable (503) →
DegradedReachable (204) → Recovering (503) → Running (204), then EOF (503).
Fatal DegradedReachable explicitly clears readiness, alongside existing fatal,
EOF and panic cases. Existing quiescence still returns503 even when Tor is ready.

Pinned `nix develop --offline`, Cargo `--offline --locked`: delegated normal Tor
and debug engine suites each25 passed; parent independently reviewed the real
diff and reran debug25 plus the normal actual-router/quiescence test (one passed).
Existing vendor dead-code warning remains. No Tor launch, inference or live
request was made by these tests. The pre-existing formatting-only change in
`src/server/tor_tests.rs` is not part of this commit.

The initial Flash test delegation stopped before inference on closed verification
code `possums_evidence_unavailable`, gateway-attestation HTTP503. It was not
retried and no trust gate was weakened; the terminal failure was followed by a
bounded Sol offline-test task.

## Delivery boundary

This record is source-only, not a deployment or successful rollout. The operator
also selected ongoing sanitized Tor background diagnostics and a later signed
**debug-only** deployment with Honeycomb enablement; that is a separate change
requiring its policy, export, failure/privacy tests and release/runtime evidence.
Applicable privacy policy is [PRIVACY.md](../PRIVACY.md); the health change adds no
telemetry or API/client wire fields. Production/main remain untouched.
