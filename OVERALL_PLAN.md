# Possums — API-only roadmap

**Target, not a deployment claim.** `AGENTS.md` and `SPEC.md` govern the gateway. Earlier no-JavaScript HTML streaming, cookie/session and buffered proposals were explored and released in prior phases; their dated outcomes remain in [phase records](docs/phase0.md), [API investigation](docs/phase01.md) and [verification](docs/verification.md), not in the active roadmap. The current recorded deployed release is v0.0.15; an API-only image and deployment have not been verified by this documentation change.

## Product and boundaries

The measured gateway exposes JSON `/attestation` and the authenticated `/v1` streaming inference API, not a gateway Web UI. There is no future `/app`, no-JavaScript page, web Markdown renderer, browser cookie auth or browser recovery page. Pi remains a local verified API client. A proposed Obsidian plugin is an **independent** API client on user devices; its UI is not a gateway-hosted interface and its Android/browser-compatible verification remains a separate qualification question. General CLI/SDK integrations remain possible; native-v3 CLI platform-signer and legacy-v2 freshness limits remain unresolved.

The gateway and Tinfoil handle plaintext in memory. Attestation is scoped to measured identities and protected transport, not to all operator/platform behavior or impossible retention. Demo credentials/balances and prompt-free short-lived reservations are memory-only. Provider billing semantics and maximum billable cost, durable accounting, Tinfoil platform collection, runtime privacy and traffic-correlation risks require separate evidence. Follow [privacy policy](PRIVACY.md) for aggregate telemetry; Honeycomb is outside the attested trust boundary.

## Milestones

1. **API-only cutover:** remove HTML routes `/`, `/login`, `/logout`, `/chat`, `/chat/new`, `/recovery`, `/recovery/download`, `/claims`; browser cookies/CSRF, rendering/web stream and browser fixtures. Preserve JSON `/attestation`, `/v1`, Pi and accounting/attestation protections. Remove browser package/CI/Nix target and obsolete telemetry Web labels/New chat lane. Capacity target is connection 64, generation 4 (effective heavy bound), heavy 4, ingress 4, control 1. Verify API-only fixture, negative routes, privacy payloads, accounting, reproducible image and deployed identity separately; documentation is not a pass.
2. **Pi client:** preserve verified streaming provider/tool/compaction and locally scoped client approval; do not infer every live-model or device result from synthetic checks. See [release record](docs/phase02-pi-release.md) and [client instructions](clients/pi/README.md).
3. **Obsidian client (proposed):** qualify supported Android/macOS/Linux attestation and protected API credentials, scoped local note access and safe client rendering. It is not a gateway Web UI. See [plugin plan](docs/obsidian-plugin.md).
4. **Later Tor and paid credits (separate decisions):** Tor endpoint and address-to-measurement binding; Stripe/Monero intake, durable failure-tested billing and reconciliation. Audit and independent general CLI/SDK integrations require separate scoping. No browser gateway tier is part of these milestones.

Acceptance requires measured release/provenance, negative trust/key-binding/catalog/reservation tests, scoped live billing and runtime/privacy checks; preserve explicit UNKNOWNs. Historical browser and buffered tests in [verification](docs/verification.md) remain genuine but do not qualify an API-only release.
