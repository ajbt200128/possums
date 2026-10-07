# Obsidian client plan

**Status: planned, not implemented or verified.** This is the Phase 0.3 follow-on to the verified streaming inference API and [Phase 0.2 Pi integration](pi-client.md) in [the roadmap](../OVERALL_PLAN.md#phases). These client phases are outside Phase 0: [SPEC.md](../SPEC.md) and the [Phase 0 contract](phase0.md) still exclude a public inference API and additional clients. This plan does not add JavaScript, a vault service, or client assets to the Phase 0 gateway image.

## Goal and scope

Install one mobile-compatible Obsidian plugin on Android, macOS, and Linux. It provides verified chat, model-controlled search/read of permitted local notes, automatic conversation saving, and user-approved summary-note creation. Ship a plugin bundle (`manifest.json`, `main.js`, optional `styles.css`), not an Obsidian fork, custom APK, Termux environment, native helper, or separately managed proxy.

| In the first version | Explicitly out of scope |
|---|---|
| Authenticated live model selection and streaming chat | Web search, web-page fetching, remote resource previews |
| Client-side gateway verification on every supported platform | Unverified transport fallbacks |
| Explicit note attachments and scoped local text search/read | Whole-vault uploads, cloud embeddings, semantic-search services |
| Saved conversations that can be reopened and continued | Model-directed edits, overwrites, deletion, moves, or renames |
| Preview and approval before creating a summary note | Shell execution, arbitrary filesystem tools, MCP, background agents |
| Existing vault storage and explicit sync disclosures | A new sync service, encryption-at-rest guarantees, credential syncing |

The plugin and Obsidian are outside the gateway measurement and are trusted with plaintext. Gateway attestation does not attest the plugin, its dependencies, other installed plugins, or the user's device. It identifies approved measured code/configuration and channel keys, not an impossibility of runtime compromise or retention.

## Prerequisite: verified inference API

Phase 0.1 must supply an authenticated OpenAI-style model catalog (`GET /v1/models`) and **streaming-only** chat-completions API (`POST /v1/chat/completions`) while retaining the gateway's context validation, quoted maximum-cost reservation, authenticated-usage settlement/refund, and idempotency behavior. No buffered inference fallback. Specify interrupted-stream billing for API clients rather than assuming OpenAI compatibility supplies it. On downstream disconnect, the gateway consumes upstream until completion or error and charges authenticated final usage even if the plugin did not receive the answer; upstream error or missing/invalid final usage refunds the reservation, with operator cost exposure. No paid-credit durability claim follows from Phase 0's demo balances.

- Fetch the catalog through the verified gateway connection. Catalog contents remain live data outside the gateway measurement; invalid or unauthenticated entries fail closed. Show all supported models and an indicative maximum reservation. If the submission-time maximum quote exceeds available credit (for example, a $5 account requesting Kimi when its full-context maximum costs more), present the gateway's insufficient-credit error before any upstream prompt transmission. Do not substitute models or impose a lower product output limit.
- For model-directed note access, establish which selected models and API paths support tool definitions, tool calls, and tool-result messages. Ordinary chat compatibility is insufficient. Unsupported models remain usable with user-selected attachments, without an improvised tool parser.
- Each model invocation, including tool continuations and summary generation, is a separately accounted inference request. Give users bounded tool-round and spend controls without silently reducing a model's permitted maximum output.
- Specify cancellation, duplicate submissions, expired sessions/tokens, reconnects, and uncertain completion. Never automatically replay inference after it may have started; a deliberate new submission after an interrupted stream can be a second paid generation. A complete upstream generation with authenticated final usage can be charged even when Obsidian did not receive or save the answer.

### Reuse Tinfoil's verification and transport model

Prefer compatibility with Tinfoil's supported attestation documents, release-provenance verification, freshness rules, attested keys, and verified transports. Reuse a reviewed, pinned SDK rather than inventing a Possums quote format or cryptographic verifier. OpenAI API compatibility and attestation/transport compatibility are separate requirements.

The intended plugin implementation uses browser-compatible TypeScript/JavaScript. The first milestone must prove these properties inside actual Obsidian on Android as well as macOS/Linux:

1. Independently verify raw signed evidence, hardware trust, expected Possums repository/release policy, and freshness before releasing credentials, prompts, or note excerpts. A server-supplied digest, timestamp, or `verified` flag is not evidence by itself. Revalidate when evidence expires or channel keys change; unapproved releases block requests.
2. Bind the actual request/response transport to the verified gateway key. A successful standalone verification followed by an unrelated HTTP request is insufficient. Native clients can use attested TLS pinning; ordinary mobile JavaScript and Obsidian's generic HTTP API do not expose that control.
3. Establish gateway support for the browser-compatible Encrypted HTTP Body Protocol (EHBP), or another reviewed Tinfoil-compatible channel available in the plugin runtime. Test the deployed protocol, not just a configured base URL. EHBP body encryption alone does **not** protect an ordinary `Authorization` header: select and document authentication inside the attested protected channel, and prove an unattested intermediary cannot receive a usable credential. If stock components cannot meet this requirement, document the necessary measured gateway adaptation and keep the plugin blocked; do not add an unverified proxy or weaken authentication.
4. Bind freshness to client-verifiable evidence, using supported challenges and/or verified freshness witnesses. The current gateway helper's server-generated nonce and `/attestation` wrapper are not proof of a complete external-client protocol. Never accept an old quote merely because the gateway supplies a recent timestamp.
5. Verify Possums itself, not only a default Tinfoil inference router. The approved gateway code remains responsible for its upstream Tinfoil connection; showing an upstream quote does not independently prove which connection handled a particular request.
6. Review SDK retries, cache-scoping, persistence, diagnostics, redirects, and network destinations. Cache scoping is not cache disabling. No credentials/content in logs or support artifacts; no direct-HTTP fallback. Record unresolved upstream retention properties as unknowns.

Configuration and research are not compatibility evidence. Existing unknowns in [the verification record](verification.md) stay unresolved until measured tests demonstrate otherwise.

## User experience

- A ribbon button and command-palette action open a chat view: a tab or side pane on desktop, a full-width tab on Android. Verification status lives inside the view, not the desktop-only status bar.
- Show the selected model, gateway identity state, note-access scope, and whether the conversation is saved. Verification details expose the approved/accepted release and freshness, not a blanket privacy badge.
- States include checking, verified, sending, complete, and blocked. Missing evidence, a changed unapproved identity, or connection-binding failure blocks sending and explains the failure without sensitive payloads.
- Settings cover gateway/trust policy, credential, default model, conversation folder, and note exclusions. Credentials are session-memory-only by default and re-entered after restart. Never save them in Markdown, canonical chat state, or ordinary plugin settings. Persistent credential storage is deferred until its actual protections on every platform are established.
- Only explicitly selected text/notes are attached by default. Merely opening a note does not transmit it. Display the notes/excerpts actually used with local citations and an inspectable context receipt.

## Local note permissions

Obsidian does not provide a per-plugin read-only or folder sandbox. These are restrictions enforced by our tool dispatcher and request builder against model requests, not protection against a malicious plugin or compromised host application.

The user chooses a scope per conversation and destination gateway/model profile:

| Scope | Allowed model access |
|---|---|
| Attached notes only (default) | Search/read only the notes or text the user attached |
| Selected folders | Search/read Markdown notes within the explicitly selected folders |
| Whole vault, except exclusions | Search/read eligible Markdown notes throughout this vault |

A grant permits automatic reads within that scope; do not prompt for every read. Broadening scope or changing the destination gateway/model requires confirmation before note-bearing context is reused. Label the grant honestly: **Allow this conversation to send relevant excerpts from these notes to the selected gateway/model.**

Expose only `search_notes` and `read_note` to tool-capable models. Keep these rules shared across attachments, search results, snippets, direct reads, and outgoing context:

- Enforce scope before searching or returning filenames, excerpts, metadata, or citations. Exclusions are access rules, not just index or display filters; a direct filename cannot bypass them.
- Restrict access to canonical, in-vault Markdown paths. Reject traversal, out-of-vault resolutions, hidden/configuration folders such as `.obsidian`, and unsupported attachment types. Do not follow embeds or links into new sources without applying the same policy.
- Start with local lexical search and no external embedding/ranking service. Bound work and tool results; tell the user when context cannot fit rather than silently discarding history. The gateway remains authoritative for model-specific tokenization and context limits.
- Notes, model text, and tool results cannot grant permissions or approve actions. Reject unknown tool names and malformed arguments; do not expose a generic command, file-write, or URL-fetch tool.
- Check scope again when assembling each outgoing request, including retained tool results. Revocation stops new reads and invalidates pending context. Previously sent content cannot be recalled; if past messages or generated summaries retain withdrawn material, require a clean conversation for the narrower scope rather than claiming to erase that knowledge.
- Bound tool rounds and cumulative inference spend. Show which sources were used; keep context receipts local, not in production telemetry.

## Saving conversations and summaries

### Automatic conversation saving

After the user chooses a chat folder (default `Possums/Chats/`), save conversations there and support reopening, renaming through the UI, and continuing them. Maintain canonical structured messages and request state alongside a safe, readable Markdown transcript. Displayed Markdown is not reparsed into authoritative roles or permissions.

This is a deterministic plugin write, **not a model `write_note` tool**. The plugin chooses collision-resistant identifiers and owns only its own chat files. Model-generated titles/content are data, never paths or commands. Do not overwrite unrelated files or silently clobber external edits/sync conflicts. Mark incomplete/uncertain requests accurately; reopening or recovering a file must never trigger inference replay.

Local history intentionally persists prompts, excerpts, and responses on the user's device. Files may also enter vault sync, backups, search indexes, and other plugins. Disclose that boundary; no independent sync service or client encryption-at-rest guarantee is introduced. Default note-search exclusions should omit chat/draft output folders to avoid recursive retrieval; reading them as vault sources requires an explicit scope/exclusion change. Resuming a conversation's own saved history is separate from vault retrieval and still rechecks its source grants before transmission.

### User-approved summary creation

Provide **Save summary as note** as a user action:

1. Generate a summary of the selected conversation through verified inference; disclose it as another potentially charged request.
2. Preview the exact title, destination, and content. Allow the user to edit them.
3. Create a new note only after explicit approval, with a local link to the saved conversation.

Use create-only semantics: an existing target, stale preview, cancellation, or path escape must not cause an overwrite. Repeated approval must not create duplicate files. The model cannot approve its own output, choose an arbitrary write destination, or modify an existing note. General edit/diff tools remain deferred.

## Rendering and privacy

Treat note content, tool output, and model Markdown as hostile. Use a controlled renderer and safe Markdown projections: no raw HTML, automatic external resources, transclusions, executable plugin code blocks, or automatic Obsidian-command links in the chat, saved transcript, or generated summary. Do not run model content through arbitrary plugin render hooks. Safe explicit links/citations are allowed; clicking them remains a user action.

Do not export content or request-level data through diagnostics/telemetry. Intended inference traffic goes to the verified gateway; explicitly documented verification dependencies receive no note content or credentials. Saving a transcript is deliberate local product state, not a diagnostic log. The gateway and Tinfoil see transmitted plaintext in memory; the store and telemetry pipeline must not receive it. Other plugins, device compromise, vault sync/backups, and inference caches remain outside this client's guarantees.

## Delivery sequence and acceptance

| Milestone | Deliverable | Acceptance gate |
|---|---|---|
| 1. Compatibility spike | A minimal plugin invoking the pinned verifier and protected API transport | A real Android device plus macOS/Linux complete a verified canary request; missing/forged/stale evidence, unapproved release, wrong key, redirects, and intermediary capture tests prove zero credential/content leakage on failure |
| 2. Verified chat and history | Responsive view, authenticated model selector, streaming replies, save/reopen | Catalog failure blocks inference; all supported chat models remain selectable; interrupted delivery shows an uncertain result, and restart, duplicate submission, and sync/write conflicts do not trigger replay or lose/overwrite unrelated data |
| 3. Scoped note search/read | Attachments, scope UI, local lexical search, two validated model tools | Allowed-source retrieval works; exclusions, traversal, linked notes, prompt injection, scope revocation, changed destinations, and cached-context bypasses are rejected before transmission |
| 4. Approved summaries and release | Preview/create-only flow and one mobile-compatible plugin bundle | Approved content creates exactly one note; rejection/collision/stale approval creates none; malicious rendering, exported artifacts, and seeded secrets produce no unintended network traffic, execution, or diagnostics on all three platforms |

Before release, record exact Obsidian/SDK/protocol versions, distribution artifacts, test results, and remaining unknowns. Desktop emulation is not Android verification. Inspect the built bundle for Node/Electron-only dependencies, native-process requirements, unreviewed runtime code downloads, and unexpected persistence/network behavior. Do not broaden scope or silently replace the plugin-only design if the compatibility spike fails.

## Implementation starting points

These sources establish available APIs, not a completed security review. Pin and revalidate the versions actually used:

- [Obsidian custom views](https://docs.obsidian.md/Plugins/User+interface/Views) and [mobile development](https://docs.obsidian.md/Plugins/Getting+started/Mobile+development).
- [Obsidian plugin security](https://obsidian.md/help/plugin-security) and [Vault API](https://docs.obsidian.md/Plugins/Vault).
- [Tinfoil TypeScript SDK and verifier](https://github.com/tinfoilsh/tinfoil-js), including browser transport and custom-enclave configuration.
- [Tinfoil Go SDK](https://github.com/tinfoilsh/tinfoil-go) and [verified proxy](https://github.com/tinfoilsh/tinfoil-proxy) as protocol/reference implementations, not mandatory client services.
