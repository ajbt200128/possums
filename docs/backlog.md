# API-only backlog

These proposals are not implemented or authorized telemetry changes. The gateway target is JSON `/attestation` and `/v1` only; no Web UI or `/app` is deferred.

## Optional Tinfoil web search through API clients

- Explicit per-turn opt-in; disabled by default. Disclose that conversation-derived search terms may reach Tinfoil's external provider Exa. Treat results/citations as hostile data, rendered safely by each independent client, not the gateway.
- Verify authenticated usage and fee. Reserve maximum model cost plus any search fee before prompt transmission; settle once from authenticated usage. Do not assume the fee is included in catalog token prices.
- Clients may instead let users search independently and paste excerpts. Never automatically put a private prompt in a search URL.

## Ephemeral document upload through API clients

- Opt-in single-turn attachments via verified document-processing/chat path, without persistent gateway library or index.
- Verify attestation, endpoint binding, formats, limits, conversions and charges. Bound uploads/extracted content and account for model context and maximum billable cost before upstream transmission.
- Treat filenames and content as hostile; no gateway persistence or telemetry export. Review service retention and privacy before release.
