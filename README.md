# Possums

[Possums.dev](https://possums.dev) is a cryptographically verifiable no log inference provider. Currently we proxy requests to [Tinfoil](https://tinfoil.sh). We also host our payment processing and inference gateway there.

The gateway and payment processing do not keep any per user data and logs, beyond an account number and balance. This means no prompts, nor metadata are recorded. We record aggregate metrics at a 5 minute cadence. We use Tinfoil's Trusted Execution Environment (TEE) so folks can cryptographically verify that the code built in this repo is what's running on the gateway, and that this gateway does not log. We will open source the payments gateway soon so folks can also verify that.

Currently Possums provides an API, along with a [Pi](https://pi.dev/) client that checks attestation of the gateway before sending any prompts.

## Quickstart: the verified Pi client

You need Pi 1.3.0, Node 24.13.0, and Go 1.27.2 for the verifier. The pinned devenv shell has Node and Go:

`devenv shell`

Build and install the extension:

`POSSUMS_PI_ROOT="$HOME/.pi/agent/install/releases/1.3.0" sh clients/pi/build.sh`
`pi install /absolute/path/to/possums-pi-build-XXXXXX/package`

`build.sh` prints the scratch directory it used. Restart Pi fully; `/reload` is not enough. Then `/login`, choose Possums, and paste your recovery credential. The gateway is verified before the credential prompt appears.

See [`clients/pi/README.md`](clients/pi/README.md) for the failure codes, tool and compaction behaviour, and the full list of verification steps.

## Using the API directly (degraded)

You get catalog discovery, reservations, receipts, streaming and billing. You do not get enclave measurement, release provenance, endpoint-key binding or EHBP body encryption. Use this only if you accept trusting the operator's TLS alone.

The flow is six calls: `GET /v1/auth/challenge`, `POST /v1/sessions` with the challenge and your credential, `GET /v1/models`, `POST /v1/submissions` for a model-bound submission token, then `POST /v1/chat/completions` with that token. `DELETE /v1/sessions/current` logs out. All authenticated calls use `Authorization: Bearer`.

Two things catch out OpenAI SDK users: the request schema rejects unknown fields, and `submission` is required with no OpenAI equivalent. Generic SDKs will not work unmodified.

Billing comes from an authenticated receipt at the end of the stream. Without one, the charge is unknown — not zero, and not refunded.

## Tor mode

You need a running SOCKS listener; the client never starts Tor. Then:

`pi --possums-tor-onion <v3-onion> --possums-tor-proxy 127.0.0.1:19050`

Everything still verifies: the onion identity is bound into a nonce-signed endorsement, and TLS peer keys are checked before headers or body. There is no clearnet fallback. Full guide: [`docs/tor-guide.md`](docs/tor-guide.md).

## Development

The gateway and payment processing are not expected to run standalone yet — the gateway authenticates and bills through the paired intake service.

`devenv shell`
`possums-check`
`possums-test`

Run reproducible checks and images at checkpoints rather than after every edit:

`nix flake check --print-build-logs`
`nix build .#gateway-image --print-build-logs`

See [`docs/development.md`](docs/development.md) for incremental builds, the shared Cargo cache, and the client build scripts.

## Privacy

No prompts, responses, conversation history, request bodies, URLs, headers, IPs, user agents, credentials or account identifiers leave the gateway as telemetry. What we do export is aggregate only: request counts, status classes, latency buckets, typed failure counts and token sums, at a five minute cadence.

The intake store is outside the attested boundary and sees payment-to-account association and reservation/settlement events. It is not anonymous and not hardware-verifiable.

Full policy, including exactly which fields are forbidden and how the policy is tested: [`PRIVACY.md`](PRIVACY.md).

## Licence

The repository is not yet licensed. Vendored trees under `vendor/` carry their own licences; `clients/pi/GO-LICENSE` covers the Go runtime.
