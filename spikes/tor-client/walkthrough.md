# Proposed Possums Tor onboarding

**Draft onboarding only.** The spike has a real external-listener transport adapter and an exact strict-auth Undici qualification patch, but no installed Pi command, sound owned/control adapter or production verifier. Do not run a real connection from these instructions. Acceptance remains offline/synthetic.

## 1. Explain the boundary first

“Tor-only sends every Possums-owned network request through your configured Tor proxy—including public gateway evidence, GitHub release/provenance/assets, AMD certificates and later authentication/renewal/recovery. It does not route unrelated Pi/T3 providers, tools, MCP, update checks or other extensions. Tor changes network visibility, not the fact that the gateway and inference service see plaintext. It does not promise anonymity or prevent timing/account correlation.”

Do not use Pi's global proxy setting, environment-wide proxy variables, `setGlobalDispatcher` or a global fetch patch. A skill cannot enforce this boundary; a provider-owned transport must.

## 2. Ask how the user wants to provide Tor

- **Use my existing Tor:** numeric loopback address and SOCKS port; leave its process/configuration/state alone. Ask the user to confirm their listener is Tor and is configured for `IsolateSOCKSAuth`. Explicitly explain/select **connection readiness**: successful required username/password negotiation plus public backend verification, not a blind open-port check, Tor identity proof or authenticated bootstrap snapshot. No control cookie is needed/read. The client cannot prove the proxy honors circuit isolation; never present acknowledgement as proof.
- **Let Possums launch my installed Tor — unavailable pending control qualification:** retain this separately approved V0 design, but do not activate it from this spike. A future qualified adapter asks for an absolute executable and dedicated persistent state directory, never arbitrary flags/shell/service/admin commands or automatic installation. It stops only its captured child. No sound maintained Node controller was qualified; do not fill the gap with a handwritten parser/helper.

If Tor is not installed, provide links to the official [Tor download](https://www.torproject.org/download/tor/) and [Tor support](https://support.torproject.org/) instructions. Let the user choose and install it outside this workflow. Do not automatically run an OS package-manager command or start a system service. Tor Browser process/state management is not authorized by this choice.

## 3. Explain state and collect consent

Keep a private persistent Tor state/guard directory independent of credentials, login/logout and random in-memory SOCKS isolation scope. Do not rotate/delete guards on every connection test. Use a separate private ephemeral config/control directory, with user-only access. No bridges or raw torrc input are accepted in this prototype. A bridge/censorship workflow would need a separate review; never ask users to paste bridge lines into chat or diagnostics.

The proposed wizard may remember the non-secret mode/endpoint/executable and approved state location only through a separately reviewed provider-owned configuration. This spike persists **no user configuration**. Do not put process arguments, cookie material or circuit scope into Pi session entries or status.

## 4. Offer the deliberate action

Buttons: **Test this connection** / **Not now**. Make clear that testing uses the **same normal preparation function**, selected readiness policy, strict SOCKS connector and backend verifier. External connection mode proceeds to public verification without claiming control bootstrap; future owned/control mode must await authenticated structured bootstrap. No IP/echo service or alternate verifier. Testing supplies only independently random memory-only SOCKS scope values; it will not ask for account credentials, authenticate to Possums, fetch an authenticated catalog, submit inference or retry an earlier operation. Disclose that public evidence acquisition can be observable and rate limited; Tor/third-party services still see network metadata.

No network activity from opening the wizard, viewing status, cancellation before approval or rereading an error. `Test` requires explicit user confirmation each time. Choosing “Not now” leaves setup untested and inference unavailable, not silently direct.

## 5. Show bounded, friendly progress

“Preparing your configured proxy” → “Connecting through authenticated SOCKS5” → “Verifying backend identity.” Show “Waiting for Tor bootstrap” and numeric progress only for a future qualified control mode; never fabricate it for external connection readiness. Use fixed locally authored labels. Never display control-event summaries, bridge configuration, process stdout/stderr, addresses/URLs, raw exceptions or shell arguments.

On success: “Your configured proxy, required SOCKS username/password negotiation and backend checks completed for this attempt. This is point-in-time connectivity, not proof that the proxy is Tor, honors circuit isolation, or will remain available. No account credentials or inference were sent.” A future normal operation still uses its normal verification/expiry requirements; no cached test verdict authorizes it.

## 6. Make failures useful

Show the closed code, stage, constraint and next action from the shared diagnostic projection. Example: `possums_tor_socks_unavailable`, stage `socks_connect`, constraint `unavailable`: “Check that your configured loopback SOCKS listener is running. No direct fallback was attempted.”

Backend trust rejection blocks backend dispatch and asks for fresh verified trust; operational Tor failure/cancellation is a graceful stop, not a dead account or automatic inference retry. Neither interruption nor closing a socket establishes cancellation, settlement or refund. Preserve received authenticated billing in the existing client, warn that another deliberate inference submission may charge again, and suppress provisional tools under the existing failure policy.

Offer **Review settings**, **Test again deliberately**, and **Offline status**. **Stop own child** belongs only to a future qualified owned mode, never existing Tor. Do not change Tor mode to direct from an error. Share only content-free code/stage/constraint; no screenshots, raw Tor logs, URLs, credentials or transcript support bundle.

## 7. Pi integration outline (not implemented)

Use user-only registered commands, e.g. `/possums-tor-setup`, `/possums-tor-test`, `/possums-tor-status`, not a model-callable tool. Commands use `ctx.ui.select/confirm/input` and `ctx.ui.notify` where `ctx.hasUI`; avoid custom TUI for this simple flow. RPC supports these dialogs but notifications can be ignored. Print/JSON needs explicit approved configuration and a closed result, never hidden prompting or fallback. No global Pi/T3 setting changes.

Do not start Tor/resources in the extension factory. Start only on the approved command/normal authorized operation, close idempotently on provider/session shutdown/reload, and never stop an external process. Keep the approved guard state. Do not persist raw errors or append setup/test entries. Ordinary Pi/T3 history and host logging remain external boundaries, not whole-process transience guarantees.

A draft explicit-only skill lives in `skill-outline/SKILL.md`; it teaches the workflow but is not loaded or registered by this package. The parent owns the exact qualified v3 integration base. Production still needs complete transport coverage, a reviewed strict-auth dependency artifact and explicit external-readiness policy. Owned mode additionally needs sound control/secure filesystem/platform qualification. See `../../docs/tor-client-spike.md` for the exact blocker and smallest parent decision.
