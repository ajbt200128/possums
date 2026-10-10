# Authorized local timeout/renewal client installation

After independent **read-only source review** found no semantic blocker and the
parent accepted it, the first-party formatting gate passed and installation
proceeded under the operator's authorization. The reviewer did not rerun tests.
[Integrated verification, commands, privacy references and baseline formatting](pi-timeout-renewal-integration.md)
remain the scoped evidence, not a deployment or live-expiry claim.

## Source and copied artifacts

- Qualified JS build source: `e622af19a0a763e610ebb422eca83e0fb27ab0e3`.
- Formatting-only Rust commit: `3b30b0105779da97b5fab4b48ffe459ecbb51fd5`.
- Clean local main at installation: `01eb9ca20e37fb878382d019096cd618cda1a3e1`.
- Source package: `/private/tmp/possums-pi-build-integrated-t1Dq1i/package`.
- Extension SHA-256: `569e4cd5ce0a53613a9f99980fa677184bdf830ec940cd3fc39fce45ccb88f6c`.
- Build report SHA-256: `00b1c219dcb0d3dabf4174eff4895f94561e1a156bffb04e4092e72087b058f9`.
- Package metadata SHA-256: `fe79acc3a88aacc6deb045d5c31d61b71aeff4f5726620c0139457323604dda1`.

No client code changed after the qualified build. Before copying, all **152**
bundled input hashes were rechecked, including all **10** bundled repository
modules against committed current main, plus the source lock and three pinned
Pi manifest hashes. The original build report was copied unchanged, not relabelled
as a rebuild. Runtime pins remain Node **24.13.0**, Pi **1.0.4**, TypeScript
**5.9.3** and esbuild **0.25.10**. Formatting-only source passed all 302 Rust tests
(three live tests ignored) and strict Clippy again. First-party Cargo formatting
passes; vendor-inclusive formatting still reports the same 22 baseline failures,
including pre-existing TLS-file formatting, with no new timeout-line discrepancy.

## Atomic install and readback

Created the new versioned directory
`~/.local/share/possums/pi/pi-timeout-renewal-569e4cd5ce0a`, copied only the
production extension, package metadata and build report, and linked the three
existing Pi 1.0.4 peers. Package-owned files/directories were made read-only
without following or modifying runtime peer targets. Existing packages were not
rewritten. A same-directory temporary symlink was atomically renamed over
`installed`, after verifying its expected predecessor had not changed.

Readback: `installed -> pi-timeout-renewal-569e4cd5ce0a`; extension/report hashes
match the qualified copies. Predecessor **`pi-diagnostics-70cc73c01165`** remains
available unchanged for rollback, with extension SHA-256
`70cc73c01165f2be8112530ad0d885c605057ac52a9957df8f2977bfc066a9dd`.

Both the new versioned path before switching and stable path after switching
passed import, pinned peers/runtime guard, provider/lifecycle-hook registration
and offline `/possums-status`, using synthetic UI and fetch forbidden: **zero
fetch attempts**, no authentication or inference. Scratch-only commands were
`python3 "$S/install-qualified.py" "$S"` and its bounded Node
`installed-package-check.mjs` checks; logs/result remain under `S` above.

No auth.json, settings, package registrations, credentials, history, Pi runtime,
current T3/Pi process or gateway configuration was changed. No public evidence
request, live inference, deployment or push occurred. Root's six dirty Rust files
and empty index remain unchanged. This is local package installation, **not
running-session activation**: the user controls the required full restart with
the compatible Pi 1.0.4 runtime. The current Pi 1.1.0 main-loop is not claimed to
have loaded this package. Live startup, expiry renewal, external UI behavior and
platform billing/privacy remain unqualified by these offline installation checks.
