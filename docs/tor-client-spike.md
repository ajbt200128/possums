# Bounded client Tor spike

> Historical research only in this inactive HTTPS-only backup. Tor code/spikes
> are removed; commands and authorizations below do not apply to this branch.
> Results and limitations remain historical. See
> [backup verification](without-tor-backup-verification.md).

Status: **offline spike only**, updated 2026-10-14. No production client/reference/root dependency/config/privacy changes, v3 changes, Tor installation/start, real control cookies/bridges/user configuration, public connectivity tests, gateway calls, credentials/inference, telemetry, deployment or commits. The parent owns integration on the exact qualified v3 client base. Backend PR76's merge does not qualify this transport. Independent Arti work is untouched.

## Result and smallest decision

Keep the selected V0 packaging: **users install Tor**, with an existing numeric-loopback listener or separately approved owned launch. Do not bundle Tor, manage a system service, adopt Python/native helpers or grow the offline Go/WASM verifier into a networking runtime.

This packet closes **strict SOCKS negotiation in the spike**, not actual Tor circuit qualification. It also implements a real **existing-listener runtime adapter** with explicit connection-readiness semantics. **Owned control/bootstrap remains blocked**: the newer Node candidate has concrete protocol/lifecycle defects that cannot be fixed by a tiny glue adapter. No replacement control/SOCKS parser was written.

Smallest parent/operator decision: choose **existing-listener-only V0 with explicitly acknowledged Tor configuration and connection readiness**, retaining owned launch as unavailable, or separately authorize qualification/packaging of a maintained control helper. Do not silently substitute weaker readiness when control bootstrap was selected. No new stack has been adopted. Do not ship the spike's synthetic verifier or developer patch workflow.

## Exact dependencies and patch

Spike-only `package.json`/lockfile:

| Dependency | Scope and evidence |
| --- | --- |
| `undici` **8.11.2** | Runtime SOCKS/HTTP/TLS client, public `Socks5ProxyAgent` API (experimental), plus the exact local qualification patch below. Stock 8.11.2 is **not** isolation-qualified. Registry source: `https://registry.npmjs.org/undici/-/undici-8.11.2.tgz`; SHA-512 integrity is pinned in the lockfile. |
| `socksv5-server` **1.0.2** | Dev-only maintained SOCKS parser/server fixtures; forwarding is intercepted and mapped only to bound numeric-loopback synthetic HTTP. No fixture destination DNS or automatic forwarding. |
| `tor-ctrl` **1.0.0** | Dev-only **rejected controller candidate**, never imported by runtime code. Registry source: `https://registry.npmjs.org/tor-ctrl/-/tor-ctrl-1.0.0.tgz`; integrity pinned. Published 2025-10-17; inspected repository head `428e66fa790709941b9a629afe20440d071ef7e0`, unarchived. A recent package/push is not soundness or maintenance qualification. |

`patches/undici-8.11.2-require-auth.patch` modifies the upstream decoder/agent, public option type and API documentation only:

- `requireAuth: true` requires both username and password, offers **only** username/password and rejects an explicit no-auth selection with existing typed `UND_ERR_SOCKS5_AUTH_METHOD`, before authentication/CONNECT/destination bytes.
- Default `requireAuth: false` preserves upstream optional-auth behavior. This patch neither implements a new SOCKS parser nor changes HTTP/TLS/retry behavior.
- A local `Socks5ProxyAgent.supportsRequireAuth` capability reflects the **loaded decoder**, preventing a previously loaded stock module from silently ignoring the option after files have been patched. This is local fork API, **not upstream acceptance**.
- `undici-qualification.mjs` checks exact version and per-file SHA-256 hashes from `patches/undici-8.11.2-hashes.json`; preparation also requires the loaded capability. Missing, stock or unknown bytes fail closed with `possums_tor_connector_qualification`, before any SOCKS connection.
- `qualify-undici.mjs` applies the patch only to exact original bytes via `git apply`, with output ignored, or accepts exact already-patched bytes. It is an explicit **offline developer step**, never startup auto-patching/downloading. Patch application and idempotence passed both inside this checkout and a fresh scratch copy. Unknown bytes are not repaired speculatively.

Current upstream `main` at inspected head `96ca7441d2effa7dde9ed9efcf4902b619e4f3c4` still offered no-auth; no supported strict-auth option was found. Production needs an accepted upstream option or an explicitly reviewed, pinned patched build/artifact, not a runtime patcher or an unpatched ignored option. The independently random memory-only username/password values each use 32 bytes; they are not account, credential or prompt-cache derived. Scope equality tests keep only synthetic transient values and clear them on teardown.

**Guarantee:** the qualified connector requires username/password negotiation on every new tunnel and cannot accept no-auth. **Not proved:** that the listener is Tor, honors `IsolateSOCKSAuth`, uses distinct circuits, or provides anonymity. A proxy can accept/ignore these values. Existing Tor must be configured by its owner; the client never reconfigures it. Owned plans still request `IsolateSOCKSAuth`, but no real Tor/circuit test ran.

## Real existing-listener adapter and readiness

`existing-socks-runtime.mjs` exports `createExistingSocksRuntime()`, usable with `mode: 'existing'`, explicit `readiness: 'connection'` and the existing Tor-assumptions acknowledgement. It does not discover/read control cookies, require control access, launch/reconfigure/stop processes, inspect the user's state directory or do its own port/echo/IP probe. It admits proceeding to the **normal strict-auth connector and backend public-verification path**. Only completion of that path returns a prepared connection.

A successful verified public acquisition through the selected SOCKS listener is sufficient **point-in-time connectivity readiness**, given the user's explicit assumption that this is their configured Tor. Authenticated control bootstrap is not intrinsically necessary for external connectivity: it would add privileged cookie/path handling and restrict otherwise usable listeners without proving remote backend identity. SOCKS connectivity is not authenticated Tor bootstrap/identity evidence. It can involve destination acquisition before any control progress evidence; that distinction is deliberately selected, not hidden.

`readiness: 'control_bootstrap'` remains the default and the only allowed owned mode. It requires strictly numeric structured `progress: 100` before destination traffic. The real external adapter rejects that policy instead of fabricating a snapshot. A missing readiness choice therefore does **not** silently downgrade the prior gate. Control bootstrap adapters are still synthetic only. `waitReadiness` names the shared runtime contract; no stdout/status-summary parsing is present.

Normal preparation and `setupConnectionTest` still share exactly the same configuration snapshot, runtime preparation/readiness, strict connector, random scope, injected verifier and closed diagnostics. The test calls `prepareConnection`, then closes that same scope. There is no alternate connector/verifier, cached verdict, inference, automatic replay, authenticated catalog or credential submission. Offline status initiates no work. Real external no-auth, cancellation and success tests exercise this shared path.

## Controller investigation: precise blocker

Npm catalog searches for Tor/control and Tor keywords, package source and public upstream APIs were inspected before changing code. Earlier options remain unselected: `tor-control` 0.0.3, old granax forks, and Bine v0.2.0 do not establish a maintained current Node path. `socks` 2.8.10, `socks-proxy-agent` 10.1.0 and `fetch-socks` 1.3.3 do not solve strict negotiation: reviewed `socks` offers no-auth alongside password. No additional runtime dependency was adopted.

The newer `tor-ctrl` 1.0.0 is insufficient:

1. `sendCommand` installs `once('data')` and treats that chunk as the whole reply. Fragmented replies resolve prematurely; no incremental framing, multiline/data-block handling or request queue exists.
2. There is no structured event emitter/subscription/bootstrap field parser. Interleaved `650 STATUS_CLIENT` events are treated as command errors. `SETEVENTS` text alone does not establish a supported race-safe subscription API.
3. `authenticateCookieFile` reads an arbitrary-sized file and sends its hex representation **quoted as ASCII**, not as unquoted encoded cookie bytes. No 32-byte check, SAFECOOKIE server authentication, private-file ownership/permission scope or clearing of cookie representations is implemented. It does not send mandatory NULL `AUTHENTICATE` in unauthenticated configuration.
4. A pending command does not reject on peer close; no signal/cancellation API exists. Raw response/error text is returned or retained in exceptions. The adapter would need a new protocol/event/queue/authentication/lifecycle stack, not tiny glue.

`test/control-candidate.test.mjs` reproduces incomplete snapshot acceptance, interleaved-event rejection, wrong synthetic cookie wire representation and a command still pending after peer close. Its server emits **fixed TCP fragments**, with no request/protocol parser. These are bounded **falsification fixtures**, not a maintained Tor control server or control acceptance evidence. No raw candidate errors/paths/cookies are projected to users or exported. Dev tests use only a private throwaway 32-byte synthetic file; no real cookie is read. No controller acceptance suite was manufactured from a bespoke parser.

Consequently, supported snapshot-plus-subscription race handling, SAFECOOKIE scope, authenticated read-only external control, real owned cancellation/exit/cleanup and secure private runtime/state creation/locking/symlink/ownership handling **remain unimplemented/unqualified**. Existing synthetic lifecycle tests do not close those gates. Do not start an owned Tor until a sound controller and platform path are chosen. `txtorcon` 26.6.0 has a maintained structured control path but introduces Python/Twisted packaging and logging/default-process concerns; Stem 1.8.2 is older; Bine would introduce a Go helper with maintenance/packaging qualification needed. Any such choice requires the parent/operator's concrete approval first; none was installed or adopted.

## Owned plan and lifecycle boundaries

`childLaunchPlan` remains a pure **non-executing proposal**. It uses dedicated ephemeral defaults/torrc/control endpoint/cookie locations, approved persistent `DataDirectory`, loopback SOCKS, `IsolateSOCKSAuth`, `SafeSocks`, `ClientOnly`, cookie auth and reduced disk writes. Executable and arguments are literal, with `shell:false`, `detached:false`, ignored output. Path validation rejects control characters/traversal and overlapping runtime/state directories. The `/dev/null` log plan is POSIX-only and **not actual Tor/Windows acceptance**.

Do not copy Tor Browser/system state or delete/reseed guards on test/login/logout/scope rotation. Future owned code must secure and lock dedicated state, refuse symlinks/insecure ownership/permissions and port conflicts, ignore inherited/unrelated torrc, close authenticated control and stop **only the captured owned child**, then delete only owned ephemeral files after observed exit. Persistent guard state survives. No PID lookup, kill-by-name or external shutdown/NEWNYM is permitted. Cross-platform ACLs, locking, executable behavior and bounded orphan/cleanup behavior remain unknown. No helper was added merely to create a misleading partial launcher.

Synthetic Node-child fixtures still use ignored stdout/stderr and fixed IPC progress; they deliberately ignore the proposed executable/args and cannot launch Tor. They cover progress gating, cancellation/deadline/exit, owned-only and idempotent/stale cleanup, synthetic guard survival and preserving a primary failure through cleanup. A real adapter never inherits those fixture assurances automatically.

## Transport/trust seam (parent-owned production packet)

Use one provider-owned `Request -> Promise<Response>` transport for **all** Possums acquisitions and privately captured verified Channel calls. DNS stays at the proxy; no global dispatcher/fetch patch, environment proxy routing, direct fallback or reuse of a warmed direct pool. Other providers/tools/MCP are untouched.

Previously inspected production sites (no edits here): Pi bootstrap `acquire` and `downloadManifest`, and reference transport `send`. Required coverage includes gateway attestation/certificate/app-key acquisition; GitHub release/provenance/assets and explicit validated redirects; AMD VCEK acquisition; public-cache misses/migration; challenge/login/catalog/balance/submissions/generation; expiry/renewal and approved recovery. Existing URL/redirect policy, body/parser/read-idle limits, release/publisher/endpoint-key/certificate verification, evidence-cache semantics, SDK encryption/decoding, authenticated billing/tool gates and no-uncertain-inference-replay stay unchanged. Qualify the exact v3 built artifact's offline-only verification; no hidden verifier/helper downloads may bypass the seam. App-key/socket binding is not proved by generic HTTPS fixtures.

The callback `{ verified: true }` is an **injected spike-only contract**, never production trust. Tests name it `syntheticVerifier` and acquire four `.invalid` sources with synthetic public bytes. Public acquisition allows header/credential-free GETs only; general transport is unavailable before callback acceptance. Future integration must keep the verifier's privately constructed Channel/expiry/binding authority and existing typed failures, not a caller-supplied boolean. No production verifier is imported or qualified here.

Next minimal integration packet, after the parent's readiness/build decision: integrate **existing-listener transport only** on the exact qualified v3 base, capture the same scoped transport in every bootstrap/cache/Channel/renewal/recovery path, qualify real typed trust failures and HTTPS/TLS behavior offline, and implement explicit user-only setup/Test/status UI with the same preparation function. Preserve ordinary non-Tor/shipped-client behavior; no deployment/install/live probe is included. Keep owned mode visibly unavailable until its separate controller/platform packet passes. Do not claim the spike meets full production acceptance.

## Diagnostics/privacy

Policy read before this work: [data handling](../PRIVACY.md#data-handling-boundaries), [forbidden telemetry](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data), [typed failures](../PRIVACY.md#typed-interface-failures-and-token-aggregate-extension), [synthetic testing](../PRIVACY.md#synthetic-testing-exception) and [Pi failure policy](pi-client-failure-policy.md).

One operation owner preserves primary failure; a later cleanup fault appears separately. Closed diagnostics contain code/stage/constraint/local action/disposition and explicitly unestablished completion/cancellation/billing. Known typed SDK SOCKS and transport codes are distinguished; unexpected causes become stage-specific `possums_tor_<stage>_unknown`. No exception-text classification, raw response/event summaries, URLs, paths, identifiers, cookies/scopes, headers, process output/arguments, logs/traces/metrics/support bundles or retry are added. `NODE_DEBUG` is refused, not globally rewritten. `possums_tor_connector_qualification` is actionable and content-free; absent patch is not backend distrust.

Operational failures stop gracefully; backend identity/authenticity rejection blocks dispatch. Future Pi integration uses existing warning/host-stop delivery, preserves actually received authenticated money and suppresses provisional tools. Setup success or Tor disconnect/cleanup proves neither settlement/refund nor upstream compute termination. A deliberate new submission may charge again. Existing native Pi/T3 history, host instrumentation, OS metadata and third-party debug/trace settings remain separate unqualified boundaries; no whole-process transience or whole-app no-logs claim is made. The test runner emits synthetic test results and Undici's fixed experimental warning; inherited tracing/instrumentation is not qualified.

## Friendly walkthrough

`spikes/tor-client/walkthrough.md` and explicit-only `skill-outline/SKILL.md` are updated draft onboarding, not installed commands. They explain externally assumed Tor vs authenticated control evidence, required SOCKS negotiation vs actual isolation, persistent guards, owned unavailability and deliberate Test/Not now consent. No hidden startup/status/error-triggered work, credential/paid probe or echo/IP service. A skill teaches the workflow; only transport code can enforce it.

The earlier spike read Pi 1.2.0 extension/skill/TUI/RPC/security/provider docs and linked examples; that remains historical design work, not current shipped host acceptance. Prefer supported `ctx.ui.select/confirm/input/notify`, `ctx.hasUI`, explicit user commands, no custom TUI/global settings/session diagnostic entries. RPC notifications can be ignored. Start resources only on approved actions, close on provider lifecycle, never stop existing Tor. Pi/T3 installed behavior needs separate qualification.

## Offline verification

`nix develop --offline --command sh spikes/tor-client/check.sh`

Pinned **Node 24.13.0**: syntax checks on every spike/test `.mjs`, exact offline patch application and **38/38 tests passed**, none skipped. Baseline was 27/27 before this packet. Public dependency download was authorized; installation used `--ignore-scripts --no-audit --no-fund`, spike-only cache/lock. `check.sh` does no install/download, real Tor start or external test. A fresh checkout must populate these spike lock dependencies separately. No installed/bundled/deployed artifact is claimed.

Coverage includes all prior malformed-config/public-authority/DNS/proxy/fallback/redirect/body interruption/cancellation/bootstrap/deadline/owned-fixture/cleanup/privacy tests, plus:

- No-auth-only server fails before destination traffic; explicit peer no-auth selection, complete or fragmented, fails in the maintained upstream decoder before CONNECT.
- Exact strict greeting, credential validation, optional-auth default compatibility, unknown/missing build rejection, fresh exact patch application, idempotence and already-loaded stock decoder fail-closed guard.
- Real external adapter's normal/Test success, strict no-auth rejection, cancellation, offline status, no control/process authority and refusal to fabricate control/owned evidence.
- Four reproducible controller defects, with fixed loopback transport fixtures and synthetic cookie only. **No control acceptance, bootstrap subscription-race or actual Tor isolation claim.**

Earlier advisory LSP coverage was four confirmed-clean / one unconfirmed paths, not full type checking. Current Node syntax/tests are primary evidence; any current LSP result is recorded separately below. Offline HTTP fixtures do not qualify HTTPS pinning/app-key binding, real Tor DNS/egress/circuit behavior, bridge/censorship networks, IPv6 sockets or Windows/macOS/Linux owned packaging. IPv6 configuration validation only is tested. Streaming/accounting/retry and old shipped-client compatibility require the production seam's own offline acceptance. Real/live probes still need separate authorization.

## Owned files

Only `spikes/tor-client/**` and this document changed. New bounded runtime/qualification modules, exact patch/hash manifest, controller-rejection and strict-negotiation tests supplement the existing client, fixtures, launch plan, check script and walkthrough/skill. `tor-ctrl` is dev-only. No production/root/privacy/v3/Arti edits, agents, staging, commits, install/deployment or live inference.

## Public sources

- [Undici SOCKS API](https://github.com/nodejs/undici/blob/main/docs/docs/api/Socks5ProxyAgent.md), [connector](https://github.com/nodejs/undici/blob/main/docs/docs/api/Connector.md), pinned registry sources and exact inspected heads above.
- [tor-ctrl controller source](https://github.com/shahradelahi/node-tor-control/blob/428e66fa790709941b9a629afe20440d071ef7e0/src/index.ts), [upstream tests](https://github.com/shahradelahi/node-tor-control/blob/428e66fa790709941b9a629afe20440d071ef7e0/src/index.test.ts), npm published metadata. Its upstream tests use Docker/testcontainers, not a maintained offline control parser/server fixture suitable for this spike.
- [Tor control commands/authentication](https://spec.torproject.org/control-spec/commands.html): cookie must be 32 bytes; deprecated plain COOKIE can disclose it to a non-Tor peer; SAFECOOKIE authenticates server before controller proof. [Tor manual](https://manpages.debian.org/testing/tor/tor.1.en.html), [guard persistence](https://spec.torproject.org/guard-spec/appendices.html).
- [socksv5-server](https://github.com/zxdong262/socksv5-server), [socks](https://github.com/JoshGlazebrook/socks), [proxy-agents](https://github.com/TooTallNate/proxy-agents), [fetch-socks](https://github.com/Kaciras/fetch-socks).
- Prior researched [txtorcon](https://txtorcon.readthedocs.io/en/latest/)/[PyPI](https://pypi.org/pypi/txtorcon/json), [Stem](https://stem.torproject.org/), [Bine](https://github.com/cretz/bine), [old Node controller](https://github.com/atd-schubert/node-tor-control). None was installed/qualified or repurposed as a verifier networking helper.
