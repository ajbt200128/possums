# Arti embedding spike (offline only)

Independent library package; not linked into Possums. There is no executable,
listener, bootstrap command, connector or trust/auth implementation.

- Default feature: synthetic Tokio duplex transport/lifecycle tests (Rust 1.88).
- `arti` feature: compile the real released embedding API and exercise native
  key-manager state in disposable owner-only scratch directories (Arti MSRV 1.92).
- `compile_only_http_embedding` has no caller. **Do not invoke it**: launching
  would bootstrap/publish, outside this spike's authorization, and released Arti
  still has an unconditional host-process exit path.

- `patches/run_probe.py`: checksum-pinned scratch build of actual Arti with a
  test-only private monitor probe. Three synthetic tests demonstrate the callback
  boundary, including that independently spawned runtime work is **not** stopped.
  No production fatal handler is replaced. A host-safe all-owner shutdown remains
  blocked; do not treat passing probe tests as a Tor lifecycle fix.

Findings, commands, source versions, checks and limits:
[`../../docs/tor-arti-spike.md`](../../docs/tor-arti-spike.md).
