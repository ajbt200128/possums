# SOCKS connector research — source review, 2026-10-14

> Historical research only. This is the offline study of using the `socks`
> package over a TLS socket, which was **not** adopted: the client path that
> shipped is the native SOCKS byte connector in
> [`clients/pi/tor-native`](../clients/pi/tor-native), reviewed separately.
> Commands and authorizations below do not apply. Current scope:
> [Tor main restoration](tor-main-restoration.md).

**Current operator decision:** use the user's existing Tor proxy with Monero-style
SOCKS negotiation: optional credentials and no-auth selection are permitted; no
credential-based circuit-isolation guarantee is made. The historical mandatory-auth
requirement below is no longer an integration blocker. Qualification still requires
proxy-side destination DNS, no direct fallback, complete handshake cancellation,
attested application-channel binding and scoped networking for every Possums-owned
dependency. Backend deployment and live Tor/client E2E testing are now authorized;
this authorization is not evidence that either has happened.

Historical public documentation/tagged-source research found no qualified,
unmodified Node connector meeting both mandatory username/password negotiation and
caller-controlled cancellation through every handshake stage. No package
installation, synthetic connector fixtures, live Tor or deployment was performed
for that research.

## Executed Monero-style qualification — both candidates unqualified

Parent independently reproduced these offline fixtures in the pinned Nix shell,
Node **24.20.0**, against scratch-installed, unmodified releases. Neither candidate
is imported by the installed provider, and package/lockfiles were not changed.

- **Undici 8.12.1: 37 pass / 3 fail** (`tests/tor_transport.mjs`). After truncated
  method, authentication or CONNECT replies, abort/close leaves a referenced SDK
  negotiation timer. Its supported connector surface also does not provide the
  raw socket required by the current pre-HTTP reserved-SPKI path.
- **socks 2.8.10: 66 pass / 4 fail** (`tests/tor_socks_transport.mjs`). The supported
  `SocksClient.createConnection({existing_socket})` factory accepts a parent-owned,
  already-connected numeric-loopback socket. Actual TCP/handshake cancellation,
  truncated EOF, late delivery suppression, remote DNS/no direct fallback and
  synthetic native-TLS pin layering passed. But bad password-subnegotiation
  version, unsupported CONNECT ATYP, nonzero reserved byte and an unoffered
  password method each returned an established socket instead of rejection.
  Domain/IPv6 reply variants remain untested. SDK internal unref'd timer/options
  retention is bounded, not instant-zero-owner cleanup (default 30 seconds,
  configured maximum 300 seconds).

These are **failed research candidates**, not installed or qualified transport.
No SDK patch, custom SOCKS parser, weakened gate or real inference was added.
Synthetic TLS tests are not official application-key or same-fresh-context
endorsement qualification. SDK failures retain closed cause-unknown diagnostics;
exception strings are neither classified nor surfaced by the adapter.

This is the current maintained-Node-connector blocker, **not** the superseded
strict-auth requirement. Libcurl is the next native candidate, but cancellation,
pre-HTTP TLS/SPKI/date/hostname checks, scoped acquisition and supported-client
packaging remain unqualified. `CURLAUTH_ONLY` and its predicted Node signed-flag
issue below are unnecessary under the current Monero policy. No successful
backend deployment, installed Tor-only client or live E2E is claimed.

## Inspected candidates

- **Undici 8.12.1:** [tagged decoder](https://github.com/nodejs/undici/blob/v8.12.1/lib/core/socks5-client.js) adds username/password when supplied, then unconditionally adds no-auth. Its credentialed greeting advertises `[0x02, 0x00]`; a no-auth selection is accepted. The [agent](https://github.com/nodejs/undici/blob/v8.12.1/lib/dispatcher/socks5-proxy-agent.js) has stage timers, but request-abort propagation, prompt negotiation settlement and late delivery suppression were not established. Remote destination DNS and TLS support do not resolve these requirements.
- **socks 2.8.10 / socks-proxy-agent 10.1.0:** [tagged socks decoder](https://github.com/JoshGlazebrook/socks/blob/2.8.10/src/client/socksclient.ts) starts with no-auth, adding username/password when supplied: `[0x00, 0x02]`. Selecting no-auth advances directly to CONNECT. Custom authentication callbacks do not remove that offer. `connect(existingSocket)` is described as internal use; it is not a qualified AbortSignal contract. The [agent](https://github.com/TooTallNate/proxy-agents/blob/socks-proxy-agent%4010.1.0/packages/socks-proxy-agent/src/index.ts) supports Node HTTP/HTTPS agents; `socks5h:` is required for proxy-side destination DNS, unlike `socks5:`. Neither authentication enforcement nor the complete cancellation/race matrix is qualified.
- **libcurl 8.23.0:** [tagged SOCKS5_AUTH documentation](https://github.com/curl/curl/blob/curl-8_23_0/docs/libcurl/opts/CURLOPT_SOCKS5_AUTH.md) explicitly introduces `CURLAUTH_ONLY`. Combined with `CURLAUTH_BASIC`, it omits no-auth, rejects a proxy choosing it and excludes unauthenticated connection reuse. [Tagged parser](https://github.com/curl/curl/blob/curl-8_23_0/lib/socks.c) and upstream tests [1742](https://github.com/curl/curl/blob/curl-8_23_0/tests/data/test1742), [1743](https://github.com/curl/curl/blob/curl-8_23_0/tests/data/test1743), [1744](https://github.com/curl/curl/blob/curl-8_23_0/tests/data/test1744) provide source evidence. SOCKS5_HOSTNAME delegates destination DNS. Supported multi-handle removal and [premature connection cleanup](https://github.com/curl/curl/blob/curl-8_23_0/lib/multi.c) are promising cancellation primitives, not a tested Node lifecycle guarantee.
- **node-libcurl 5.1.2:** [tagged Easy.cc](https://github.com/JCMais/node-libcurl/blob/v5.1.2/src/Easy.cc) converts generic integer options through signed `Int32Value()` before casting to `long`. Source analysis predicts a sign-extension problem with the high-bit `CURLAUTH_ONLY` flag on LP64 macOS/Linux. **This prediction was not runtime-reproduced.** Existing binaries do not establish linkage to libcurl 8.23.0. A corrected released binding, pinned native packaging and a supported Fetch/stream adaptation remain necessary before proposing it as a qualified solution.

The parent independently fetched the tagged Undici/socks authentication code, libcurl option documentation and Node binding integer conversion. The other lifecycle findings remain delegated source-review evidence, not executed fixtures. Research does not prove which libraries are installed or working on supported client platforms.

## Required acceptance, whichever approach is selected

Use offline synthetic SOCKS fixtures to exercise accepted no-auth negotiation,
optional credentials, authentication refusal, unsupported selections,
fragmented/truncated replies and abort before connect, during TCP, method selection,
password exchange, CONNECT reply and TLS. Establish prompt socket disposal, timer/listener cleanup and suppression of late success. Verify remote DNS and absence of direct fallback; do not treat a timeout alone as cancellation.

One prepared Possums-owned transport must cover gateway API/evidence, GitHub manifest/provenance and redirects, AMD public certificate acquisition and every TypeScript network bridge used by the Go/WASM verifier. Preserve the official verifier, existing cache boundaries and unrelated providers/tools. Public Setup/Test and normal Tor-only access must share preparation and diagnostics.

Optional SOCKS credentials must remain separate from Possums account credentials.
Respect the user's proxy configuration; do not require credential-based isolation
or claim that supplying credentials proves Tor identity or circuit separation;
see the [Tor SOCKS extensions](https://spec.torproject.org/socks-extensions.html).
Endpoint changes remain limited to public setup before secrets or potentially
accepted inference; never replay uncertain generation.

Follow [forbidden telemetry](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data) and [typed interface failures](../PRIVACY.md#typed-interface-failures-and-token-aggregate-extension): no credentials, proxy/onion identifiers, DNS records, raw errors or request data in diagnostics/export. Use closed locally authored stages, retain observed outcomes and disclose native host history boundaries. No diagnostics-triggered network work or retry is authorized.
