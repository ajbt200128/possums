# API-only backlog

These proposals are not implemented or authorized telemetry changes. The gateway target is JSON `/attestation` and `/v1` only; no Web UI or `/app` is deferred.

## Honeycomb dashboard: interface errors and token aggregates

- Follow up on [the error/token source candidate](interface-error-token-metrics-pr.md) after its production qualification and separately authorized export enablement. This backlog item does not enable metrics or change the dashboard.
- Add panels to the existing restricted Honeycomb dashboard for `possums.interface.errors` by closed `error_type`, and `possums.tokens.input`, `possums.tokens.output` and `possums.tokens.cached` by reviewed `possums.model`. Derive across-model totals by summing disjoint model partitions, not adding exported global marginals.
- Use Honeycomb metrics-dataset aggregation appropriate to integer DELTA counts/sums and complete five-minute windows. Show absent, incomplete, invalid or cache-unqualified data as unavailable, never healthy zero. Label cached tokens as a subset of input and usage as observed totals, not provider invoices.
- Verify panel queries and missing-data behavior against qualified ingestion; record scoped evidence in [verification](verification.md). Follow [approved signals](../PRIVACY.md#typed-interface-failures-and-token-aggregate-extension), [aggregation rules](../PRIVACY.md#aggregation-is-necessary-not-sufficient) and [access/retention rules](../PRIVACY.md#processors-retention-access-and-shutdown): no new dimensions, request-level data, public sharing, secondary archives, live inference probes or alerts in this follow-up. Disclose sparse-activity/correlation limits.

## Optional Tinfoil web search through API clients

- Explicit per-turn opt-in; disabled by default. Disclose that conversation-derived search terms may reach Tinfoil's external provider Exa. Treat results/citations as hostile data, rendered safely by each independent client, not the gateway.
- Verify authenticated usage and fee. Reserve maximum model cost plus any search fee before prompt transmission; settle once from authenticated usage. Do not assume the fee is included in catalog token prices.
- Clients may instead let users search independently and paste excerpts. Never automatically put a private prompt in a search URL.

## Ephemeral document upload through API clients

- Opt-in single-turn attachments via verified document-processing/chat path, without persistent gateway library or index.
- Verify attestation, endpoint binding, formats, limits, conversions and charges. Bound uploads/extracted content and account for model context and maximum billable cost before upstream transmission.
- Treat filenames and content as hostile; no gateway persistence or telemetry export. Review service retention and privacy before release.
