# Possums for Pi

A Pi extension that connects to the Possums gateway and verifies it before sending
anything: the signed release manifest, the enclave measurement and the serving key
are all checked, and the peer has to prove it holds the endorsed key before a
single byte of credential or prompt goes out.

It verifies the backend. It does not protect your machine — see [Limits](#limits).

## Install

This is the maintainer source procedure, not npm onboarding. The authored
`@possums/pi@0.3.0-rc.1` candidate is still private and not approved for
redistribution; see [private assembly and packing](../../docs/pi-private-distribution.md).
The read-only [source CI route](../../docs/pi-source-ci.md) acquires the pinned
public SDK in isolated scratch and runs compiler/offline checks without requiring
a personal Pi profile, npm credentials, or a Possums client installation.

You need Pi **1.3.0**, exact Node **24.13.0 / npm 11.6.2**, and the pinned
compiler tools. The opt-in shell supplies Go **1.27.2**, Rust **1.92.0** and
inspection tools; select Node/npm separately rather than assuming the shell's
ambient runtime matches:

`nix develop --no-write-lock-file .#pi-client`

`node --version`

`npm --version`

`POSSUMS_PI_ROOT="$HOME/.pi/agent/install/releases/1.3.0" sh clients/pi/build.sh`

The build pulls in `@tinfoilsh/verifier`, `ehbp` and the Sigstore browser client,
compiles the Go v3 verifier to WASM, builds the native SOCKS connector, typechecks,
and prints a private scratch directory. It now uses `native/<platform>-<arch>/`
and a closed manifest, but includes only the build host's helper—not the proposed
four-target public distribution. Linux source-client builds require the separately
provisioned musl target: prepare the helper in the opt-in `pi-native` flake shell,
then pass its package directory through `POSSUMS_TOR_NATIVE_BUILD` to this build.
No script installs missing Rust targets. The standalone native builder still
supports local GNU host builds, which are not client distribution artifacts.

The npm release is not published or licensed for redistribution yet; the source
procedure below does not establish public-package or installation qualification.
For separately authorized source use, register the package it produces:

`pi install /absolute/path/to/possums-pi-build-XXXXXX/package`

Then restart Pi fully. `/reload` is not enough — the extension is loaded at
startup.

To try it without installing:

`pi --no-session --no-extensions -e /absolute/path/to/possums-pi-build-XXXXXX/package/extension.mjs`

Set `POSSUMS_PI_ROOT` to an absolute path. `build.sh` requires it to be nonempty,
checks that Node reports exactly `v24.13.0`, and checks that all three
`@earendil-works/pi-*` packages are present under that root; it does not explicitly
check that the path is absolute. The separate `check.mjs` runner does enforce an
absolute path. Those pins are load-bearing; do not relax them to get past an error.

## Use

`/login`, choose Possums, paste your recovery credential. Pi stores it through its
normal `auth.json`, which is plaintext at rest — see [Credential](https://github.com/ajbt200128/possums#privacy).

Public evidence and key binding are verified **before** the credential prompt
appears, so a wrong or untrustworthy gateway never sees it.

Then select a model with `/model possums`. With `enabledModels` patterns set, Pi
resolves its scoped list at startup — press **Tab** in the picker to switch from
`scoped` to `all` if the list is empty. That changes the view, not your saved filter.

`/possums-status` repeats the last diagnostic or shows the session-pinned release
and expiry. It makes no network call, so it is not proof the connection is healthy.

`/possums-text-only` removes tool declarations for the current session if you pick
an unqualified model. The command alone is not a lock — another extension can
re-enable tools before a turn — and a remaining tool declaration fails closed as
`possums_tools_unsupported` before any inference request.

## Failures

Non-trust failures stop the turn with a warning and never retry inference. Backend
identity failures are fatal and block later requests. A deliberate new turn may
incur another charge — an interrupted stream is **not** a refund, and billing is
**unknown** without an authenticated receipt.

Share only the code, stage, constraint and HTTP status. Never credentials, history,
screenshots or raw server messages.

| Code | Means |
| --- | --- |
| `possums_verification_failed` | Artifact or gateway could not be verified. Start a new Pi session. |
| `possums_evidence_unavailable` | Public evidence could not be fetched. Often GitHub/AMD rate limiting. |
| `possums_manifest_mismatch` | Extension and manifest must be installed together. Re-login cannot fix it. |
| `possums_session_unavailable` | Challenge/login did not complete. Says nothing about credential validity. |
| `possums_catalog_*` | Catalog transport, rejection, decoding or validation failed. Refresh models. |
| `possums_stream_usage_missing` | No terminal usage delivered; charge unknown, not refunded. |
| `possums_tor_native_platform_unsupported` | Tor is unavailable on this platform; no HTTPS fallback occurred. |
| `possums_tor_native_artifact_missing` / `artifact_mismatch` | Selected Tor package files are missing or inconsistent. Restore the exact approved package and fully restart; do not substitute a helper. |
| `possums_tor_native_artifact_unreadable` / `inspection_unknown` | Local package inspection could not complete. Check permissions and share only the content-free code/stage/constraint. |

Unexpected failures report a component-specific fallback that says the cause is
unknown. That is the honest answer, not a placeholder.

## Tor mode

You need a SOCKS listener — Tor Browser on 9150 or a system Tor on 9050. The client
never starts Tor, and never falls back to clearnet.

`pi --possums-tor-onion <v3-onion> --possums-tor-proxy 127.0.0.1:19050`

The proxy flag defaults to `127.0.0.1:19050` and must be numeric loopback
(`127.x.x.x` or `::1`); destination DNS happens at the SOCKS layer. If your
listener wants credentials, set both variables in the environment — never on the
command line:

`POSSUMS_TOR_PROXY_USERNAME=… POSSUMS_TOR_PROXY_PASSWORD=…`

Only valid explicit Tor selection reads the native manifest/helper and proxy
credentials. Default HTTPS startup/status does not inspect them. The resolver
checks the selected package-relative file's mode, size, hash and architecture;
these checks detect corruption, not malicious replacement of code plus metadata.
Selection is locked at first trust; changing it later fails closed. The full wire format and
endorsement checks are in [`docs/tor-client-setup.md`](../../docs/tor-client-setup.md).

## What it verifies

- **Release provenance** — Sigstore-signed manifest tied to an exact
  `repository@tag@sha256:digest`, with complete predicate JSON equality.
- **Endpoint binding** — every GET and POST opens a fresh TLS socket that must prove
  possession of the endorsed SPKI, with native hostname and date validation,
  before any HTTP header or body is sent. No redirects, no proxies, no pooling,
  no session resumption.
- **Freshness** — a new 32-byte nonce per trust session; trust expires at the
  minimum of 12 hours, certificate expiry and witness expiry, rechecked at handoff.
- **Bodies** — EHBP encryption and stream decoding, using the supported library.

A cache in `$XDG_CACHE_HOME/possums/` keeps the public manifest so warm startups
need zero GitHub or AMD downloads. Every hit still runs the complete verifier. No
evidence, verdict, nonce, credential or prompt is ever persisted.

## Limits

- Pi, its installed extensions and tools, and your OS are outside the attestation.
  They can read plaintext and have host authority.
- Pi saves plaintext sessions by default. `--no-session` disables the session file
  only — not your auth file, tool writes, screenshots or exports. Never `/share`
  or `/export` a sensitive session, and never paste a credential into a message.
- Attestation proves a measured code identity. It does not prove retention is
  impossible under runtime compromise.
- The gateway reserves the selected model's maximum cost before sending your
  prompt, and refuses unaffordable requests rather than substituting a model or
  lowering the output cap.
- Tool calls only execute after authenticated successful completion. Provisional
  fragments never authorize execution.
- Compaction uses Pi's native `compact()`, so it is paid harness summarization, not
  a Tinfoil endpoint. It makes one summary call (two for a split turn) with **no
  retry**.
- Changing to another provider can send retained history to them. Start a fresh
  session when changing trust boundaries.

Client build pins: Go 1.27.2, Node 24.13.0, Pi 1.3.0, TypeScript 5.9.3, esbuild
0.25.10, Rust 1.92.0. Evidence for each is in [`docs/`](../../docs/).
