# Backlog

These are proposals, not implemented or verified Phase 0 behavior. Finish and verify the planned streaming-only replacement in `SPEC.md` and `docs/phase0.md` before enabling either feature.

## Optional web search via Tinfoil

- Add an explicit per-turn opt-in to Tinfoil's built-in web search, with safely rendered citations. Keep search off by default.
- Disclose that search terms derived from the conversation can reach Tinfoil's external search provider, Exa. Optional PII and prompt-injection filters reduce risk but do not guarantee privacy or safety; treat results as hostile input.
- Verify the current fee and authenticated usage contract. Reserve the maximum possible search fee plus model cost before transmitting prompt content, then settle once from authenticated upstream usage. Tinfoil currently charges the search fee once per request that uses search, rather than per search call; do not assume this fee is included in the model catalog's token prices.
- Preserve a no-JavaScript, no-search alternative: users can search in another tab and paste a relevant excerpt and source URL into Possums. A URL alone does not give Possums access to the page. Warn that the search site sees the user's query and network address; never automatically put a private chat prompt in a search URL.

## Ephemeral document upload via Tinfoil

- Add opt-in, single-turn file attachments through Tinfoil's attested document-processing/chat path; start without a persistent document library or retrieval index.
- Verify the document-service attestation, release provenance, endpoint binding, conversion behavior, supported formats, limits, and any conversion/OCR/image charges. Bound uploads, parsing, extracted content, and transport memory; count the converted content against the selected model's context before inference.
- Reserve every possible billable component before sending document or prompt content upstream, and settle only from authenticated usage. Fail closed if prices, limits, or usage cannot be verified; never assume conversion is free.
- Treat filenames, extracted text, and document instructions as hostile. Do not persist document contents or export them through telemetry; document the additional service and any residual retention risk before making privacy claims.
