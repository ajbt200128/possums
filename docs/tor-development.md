# Local Tor developer tooling

Development convenience only: not installed-client, deployed-backend or transport
qualification. The pinned Nix/devenv environments include Tor; neither starts it
automatically. No flake input or lock update is required.

The gateway's separately opted-in Tor startup now checks local A-grant consistency
and approved B custody without self-appraisal or cached freshness gates. It may
publish before `/attestation/tor` can return verified evidence; clients must still
refuse an unverified connection. This does not fix an attestation acquisition 503
or qualify live E2E; see [the operator-approved boundary](tor-startup-removal-verification.md).
The foreground client helper below is unchanged.

## Existing user configuration first

Keep using your existing Tor/Browser SOCKS listener when available. Select its
numeric-loopback endpoint in the client. Port 9150 is common for Tor Browser, but
has not been confirmed on this host; do not treat an open browser as proof that
its proxy is ready. No private browser profiles or browsing history are inspected.

## Optional foreground client

`devenv shell possums-tor`

The explicit helper runs the pinned Tor executable in the foreground on
`127.0.0.1:19050`, with `ClientOnly` and `SafeSocks`. It bypasses system torrc
defaults, creates no generated torrc and supplies directory paths as quoted argv
rather than interpolated configuration. Guard/cache state persists under
`$XDG_STATE_HOME/possums-tor-client/data`, or
`$HOME/.local/state/possums-tor-client/data`. State/data directories are owner-only
0700; non-owned or symlinked leaf directories are refused. This is not a
same-user compromise or filesystem-race guarantee.

For an owner-selected configuration:

`devenv shell possums-tor --config /absolute/path/to/torrc`

Alternatively set `POSSUMS_TOR_CONFIG` locally. The helper passes this path to
Tor without reading, rewriting or replacing it. Tor itself reads the config,
chooses its configured state/log locations and owns its native diagnostics.
Credentials must not be supplied in command arguments or Nix configuration.

## Privacy and limits

The helper's default mode suppresses Tor stdout/stderr and sends Tor logging to
`/dev/null`. Its own configuration/state failures use locally authored codes,
without echoing arguments or paths. This does not sanitize another Tor process,
prove bootstrap, identify the underlying network, or prevent native host history.
Tor maintains its own guard/cache files; they are not application telemetry and
are not a promise of no local persistence. Foreground exit status alone does not
establish the startup cause. Check the selected listener through the separately
qualified client; the helper performs no connectivity probe.

Monero-style SOCKS negotiation is permitted. No credential-based circuit isolation,
per-connection circuit, anonymity or direct-fallback guarantee follows from this
helper. Application transport must independently enforce remote DNS, no direct
fallback, cancellation and attested channel identity before credentials/prompts.
No onion-service identity, account credential or prompt is generated here.

Policy: [data boundaries](../PRIVACY.md#data-handling-boundaries),
[forbidden telemetry](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[typed failures](../PRIVACY.md#mandatory-typed-interface-failures).

## Scoped checks

`nix-instantiate --parse devenv.nix`

`devenv eval scripts.possums-tor.exec`

Parent Nix parsing/evaluation and **seven offline stub tests passed**:
quoted/space-containing state paths, private directory modes, persistent guard
marker reuse, config/environment passthrough, non-echoing invalid arguments,
state/data symlink refusal and relative-state refusal. The stub replaces Tor
before execution; these are not bootstrap or transport results. Prior checks of
the superseded generated-torrc helper remain historical. No helper Tor bootstrap,
real keys, client inference or deployment has been performed.

`python3 -m unittest discover -s tests -p 'test_tor_development.py'`
