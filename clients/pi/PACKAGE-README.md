# Possums for Pi — private release candidate

**Not published on npm. No Possums license or redistribution approval has been
established. Do not redistribute this candidate.** It is for local build and
synthetic qualification, not a supported installation or an activation request.
The proposed registry name is `@possums/pi`; availability and ownership remain
operator prerequisites. No placeholder publication is part of this process.

After an approved release exists, the intended version-pinned installation is:

`pi install npm:@possums/pi@<approved-version>`

Fully restart the Pi/provider process after an authorized update. A reload is
not claimed to activate existing processes. The complete package is prebuilt:
users do not compile Go/Rust or install Nix, and installation does not start Tor,
authenticate, send a prompt, or prove the gateway healthy. There are no install
hooks or executable downloads.

## Runtime and transport

The manifest's exact Node engine and the build report's Pi version describe the
current qualification target, not universal compatibility. Wildcard Pi peers are
host-resolution metadata; Pi itself is not bundled. Read the release's actual
platform/OS evidence before using it. Merely including four native files does not
establish runtime qualification or support on older macOS versions.

HTTPS is the default. Tor requires an existing SOCKS listener and explicit onion
selection; only macOS/Linux arm64/x64 have candidate native helpers. Linux
artifacts must pass independent static-musl inspection and clean glibc/musl
execution before release. Windows and other unmatched tuples are not supported
for Tor. Native integrity checks run only for a valid explicit Tor selection;
missing assets never trigger a direct-network fallback or a helper download.

`pi --possums-tor-onion <v3-onion> --possums-tor-proxy 127.0.0.1:19050`

Supply the actual numeric loopback listener. The client does not install or start
Tor. Optional proxy credentials are an environment-only pair
`POSSUMS_TOR_PROXY_USERNAME` / `POSSUMS_TOR_PROXY_PASSWORD`, not Possums recovery
credentials; never put credentials in command arguments, messages or support
artifacts.

## Authentication and failures

Once separately authorized and installed, use Pi's `/login` credential interaction
and choose Possums. Backend public evidence and endpoint/channel binding are
verified before credential transmission. Then choose a model with `/model`.
Submitting a deliberate prompt is separate work and may incur a charge.

`/possums-status` is local and makes no network request. It neither renews trust
nor proves health. Ordinary eligible expiry renewal preserves the current
conversation; it does not replay a previous generation or require `/new`.
Non-trust failures stop gracefully with warnings and no automatic inference
retry. Backend identity/authenticity failures remain fatal. Missing terminal
billing evidence means **unknown**, not a refund; a deliberate new submission
may incur another charge. Incomplete tools never authorize execution.

Share only the content-free code, stage, constraint and observed HTTP status.
Native package failures distinguish unsupported platform, missing/unreadable
assets, integrity mismatch and unknown inspection cause. Restore the exact
approved package and restart; do not bypass verification or substitute a helper.

## Privacy and limits

The gateway and inference service see plaintext in memory. Attestation binds
measured code/configuration and keys; it does not prove retention is impossible
under compromise or prevent traffic correlation. Package hashes detect local
corruption, not malicious replacement of code plus metadata; release provenance
is a separate supply-chain gate.

Pi, T3, installed extensions/tools and the local OS are outside that boundary.
Pi's native credential store is plaintext at rest and native history may persist
conversation content. `--no-session` disables a session file, not auth storage,
tool writes, screenshots or exports. Current v3 public-manifest caching does not
cache a verdict or extend freshness. No client telemetry upload, request logging,
crash reporting or support export is introduced by this package.

This candidate's `THIRD-PARTY-NOTICES` is explicitly incomplete. `GO-LICENSE`
covers the Go runtime only; it is not a Possums or dependency-closure license.
See the immutable reviewed source and release evidence in the
[Possums repository](https://github.com/ajbt200128/possums).
