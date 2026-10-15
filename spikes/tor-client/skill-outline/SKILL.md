---
name: possums-tor-onboarding-draft
description: Proposed explicit-only walkthrough for user-installed Tor and Possums-scoped Tor-only transport. Design outline, not an installed connection command or production integration.
disable-model-invocation: true
---

# Possums Tor onboarding — draft skill outline

Read `../walkthrough.md` relative to this skill directory before discussing setup.

This is a design artifact only. Do not install/start Tor, change Pi/T3 settings, run live connection tests, request credentials, submit inference or invoke arbitrary executable/shell commands. No production runtime/verifier or host commands are present in this spike.

1. Explain that only Possums-owned network calls are in scope; tools, MCP, other providers, host updates/history and OS traffic are not.
2. Present existing numeric-loopback SOCKS5 with explicit Tor/IsolateSOCKSAuth assumptions and connection-readiness consent. Explain the separately approved owned-launch design is unavailable pending sound control/platform qualification. Never propose bundled Tor/Arti/WASM Tor or system service management.
3. Explain proxy-side DNS and independent random memory-only SOCKS scope. The exact qualified connector rejects no-auth; negotiated username/password is not proof of Tor or actual circuit separation. External connection readiness uses normal public backend verification without control cookies; future owned/control mode requires structured authenticated bootstrap before destination traffic.
4. Explain the dedicated persistent Tor guard directory, separate ephemeral config/control scope and ownership-only cleanup. Never reset guards on logout/test or collect bridge lines/raw torrc.
5. Outline an explicit **Test this connection** action that calls the same normal `prepareConnection` path and existing backend verifier. No shortcut, echo/IP service, credentials, authenticated catalog, inference or diagnostic-triggered retry.
6. Explain point-in-time success and closed actionable diagnostics. Only backend identity/authenticity failure blocks dispatch; operational stops do not automatically replay inference or establish refund/cancellation.
7. Keep status offline. Never ask for raw Tor output, URLs, arguments, credentials or transcripts for support. Follow the documented Pi/T3 history boundary.
8. Before future integration/action, obtain separate authorization, use the parent's exact qualified v3 base, qualify all network seams and a reviewed strict-auth dependency artifact, and keep the readiness/isolation claims scoped. Owned mode remains blocked on controller/secure filesystem/platform qualification. Do not adopt a helper stack without a concrete parent/operator packaging decision. A synthetic fixture or boolean is not production trust.
