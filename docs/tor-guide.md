# Tor mode

Run the Pi client against the Possums gateway over Tor instead of clearnet.

You provide a SOCKS listener. **The client never starts Tor, never falls back to
clearnet, and never replays a request over another route.** Selection is locked at
the first trust establishment; changing the flags later fails closed.

## What you need

A running Tor SOCKS listener on a numeric loopback port. Tor Browser listens on
9150; a system Tor on 9050. An open browser is not proof the listener is ready —
check the port.

The gateway's v3 onion address.

## Use it

`pi --possums-tor-onion <v3-onion> --possums-tor-proxy 127.0.0.1:19050`

The proxy flag is optional and defaults to `127.0.0.1:19050`. The host must be
`127.x.x.x` or `::1`: destination DNS is resolved by the SOCKS layer, not locally.

If your listener needs a username and password, set both in the environment. Never
on the command line, and never reuse your Possums credential:

`POSSUMS_TOR_PROXY_USERNAME=… POSSUMS_TOR_PROXY_PASSWORD=…`

## What still gets verified

Everything. Selecting Tor changes the route, not the trust model:

- The onion address is parsed and pinned, and a fresh 32-byte nonce is signed
  together with the onion identity, so a stale or swapped endorsement is rejected.
- TLS runs over the SOCKS socket, and the peer must prove possession of the
  endorsed SPKI with native hostname and date validation **before any HTTP header
  or body is sent**.
- Request bodies stay EHBP-encrypted, and stream decoding is unchanged.
- Trust still expires at the minimum of 12 hours, certificate expiry and witness
  expiry, and is rechecked at handoff.

## What fails, and what it means

| Symptom | Cause |
| --- | --- |
| `tor_selection_invalid` | Proxy host is not numeric loopback, onion is not a canonical v3 host, or credentials are half-configured. |
| `route_selection_changed` | Flags changed after first trust. Restart Pi. |
| Connection refused / no route | Nothing is listening on the selected port, or Tor has not bootstrapped. |
| `possums_verification_failed` | The onion answered but the evidence did not verify. Do not retry into clearnet. |

There is no direct-connection fallback, and no automatic inference replay. A
deliberate new turn may incur another charge.

## Limits

- The client does not bundle Tor, manage it as a service, or configure bridges.
  Get a listener running yourself, then point the client at it.
- No per-connection circuit isolation and no anonymity guarantee. SOCKS with
  optional credentials is not a circuit-isolation mechanism.
- Proxy credentials are read only after a valid explicit Tor selection and are
  never placed in flags, argv, diagnostics or persisted state.
- Tor availability and routing can fail independently of Possums. A failure here
  says nothing about your credential or your balance.

Wire format, the endorsement payload and backend diagnostic categories are in
[`tor-client-setup.md`](tor-client-setup.md).
