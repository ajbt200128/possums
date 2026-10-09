# Possums provider for Pi 1.0.4

The catalog-wide gateway tool profile is deployed as **v0.0.15** (the official gateway v3 helper compatibility fix is included; the separate 16-KiB tool-description cap remains removed, while other limits and the shared tool profile are unchanged). All authenticated catalog models advertise the assumed shared OpenAI-functions wire profile; this is not per-model compatibility qualification. A real Kimi Pi round with local `ls`/`read` execution and a correct final continuation was observed on v0.0.10; exact live billing/balance reconciliation remains unverified. See [release scope](../../docs/phase02-pi-release.md).

## Build and load

Requires Node **24.13.0**, npm, and the existing **Pi 1.0.4** installation. The build creates a fresh scratch source/install/package tree, installs the locked verification dependencies with lifecycle scripts disabled, checks TypeScript, verifies the three Pi package versions, and emits `extension.mjs` plus a source/artifact hash report. It does not install into the repository or modify the Pi installation.

`POSSUMS_PI_ROOT="$HOME/.pi/agent/install/releases/1.0.4" sh clients/pi/build.sh`

The command also prints a scratch-only **local checks** command. Run it for the synthetic reference/Pi and shared admission suites; it makes no live inference request and does not qualify production models. Test-only bundles remain outside the package directory.

The build links the qualified installed Pi peers into its package. To enable normal automatic loading, put the independently approved **public release manifest** beside `extension.mjs`, then register the package:

`cp /absolute/path/to/approved/tinfoil-deployment.json /absolute/path/to/package/tinfoil-deployment.json`

`pi install /absolute/path/to/package`

Restart Pi or run `/reload`. No manifest flag is needed: the extension defaults to the adjacent public manifest. Installation adds only the package declaration; it does not change providers, defaults, model filters, credentials, or history settings. Use `pi --no-session` when you do not want Pi to save the conversation.

For an isolated one-session load, use the **package path printed by the build command** and an explicit public manifest, never an account credential file:

`pi --no-session --no-extensions --no-tools -e /absolute/path/to/package/extension.mjs --possums-manifest /absolute/path/to/tinfoil-deployment.json`

The current API approval is **v0.0.15**, with unchanged administrative expiry **2026-10-11**. Its independent image builds, exact signed release provenance and serving hardware/key binding through Pi's pinned JavaScript legacy-v2 verifier were checked. The patched gateway helper also passed strict v3 checks against public nonce evidence through the real Rust validator, including verified expiry and peer TLS key equality; this does not directly observe the deployed Unix socket or authenticate a catalog. Pi keeps its accepted legacy-v2 path and freshness limitations. The separate native CLI platform-signer incompatibility remains unresolved. See the [scoped release evidence](../../docs/verification.md#2026-10-09--v0015-official-v3-helper-release-and-paired-pi-refresh). Login and restoration independently verify the supplied public manifest and serving channel before requesting or transmitting a credential. Do not change pins to make a verification error disappear.

1. Use `/login`, choose Possums, and paste the manually issued recovery credential into its masked prompt. Public evidence and key binding are verified **before** the credential is requested. Pi saves the recovery credential through its normal `auth.json` credential store; subsequent starts restore a fresh verified bearer/catalog without inference. The old memory-only marker needs one fresh `/login`. An optional `POSSUMS_RECOVERY_CREDENTIAL` environment variable is supported; a stored recovery credential takes precedence. Do not put credentials in shell commands or conversation messages.
2. Select a live Possums model in Pi's usual model picker. With `enabledModels` patterns configured, Pi 1.0.4 resolves its scoped list at startup, before Possums can authenticate. After successful login or startup restoration, use `/model possums` and press **Tab** to switch from `scoped` to `all` if the results are empty. This changes the picker view, not your saved provider filter. Selecting a model adds it to the current session's scope. Pi's list rows show only the model ID/provider. The highlighted **Model Name** detail below the list shows `text only` or `tools` and an **indicative**, not promised, maximum reservation.
3. For a text-only model, use the isolated launch above and run `/possums-text-only` explicitly. `--no-extensions` still permits the explicit Possums `-e` entry, but excludes other extensions that can re-enable tools before a turn (for example the interactive `ask_user_question` reconciler). `--no-tools` starts with no tool declarations. The command alone is a one-time loadout change, not a lock against another extension; a remaining tool declaration causes local `possums_tools_unsupported` before inference submission. Active tools are never silently removed, and models or output allowances are never substituted.
4. All authenticated catalog models expose the shared OpenAI-functions wire profile. Their compatibility is assumed, not individually tested; unsupported upstream behavior fails closed without fallback or replay. For ordinary coding, omit `--no-tools`; `--tools read,bash,edit,write,grep,find,ls` enables the complete native coding-tool set. Select the desired model after login or startup restoration. No Possums-specific tool implementation or client-side spending reservation is installed. The gateway still reserves prepaid credit for the selected model's maximum before upstream prompt transmission and settles actual final usage.
5. Use native `/logout` to remove the stored Possums credential and clear its local bearer/catalog. The obsolete `/possums-logout` alias is removed. Accepted work can still settle remotely. An environment credential remains configured until you remove it from the process environment. `/new` resets conversation/run authorization without forgetting the saved login. Select Possums through the picker after startup; cold `--provider possums --model ...` selection occurs before the authenticated catalog is restored.

## Connection diagnostics

Startup and model refresh show a transient, content-free warning when public verification, session establishment or catalog refresh fails. Repeated warnings for the same category are suppressed until authentication and catalog loading complete successfully; public verification alone does not clear a session/catalog failure. `/possums-status` repeats the last diagnostic or displays the compiled approval version/expiry **without a network call**; local approval information is not proof of a healthy connection. Pi may still show its generic cached-models warning, but cached entries do not authorize inference.

- `possums_approval_expired` / `possums_approval_unavailable`: obtain a newly independently approved client release; do not extend expiry or bypass pins locally.
- `possums_manifest_unavailable` / `possums_manifest_mismatch`: install the extension **and its matching approved public manifest together**, then `/reload` or restart. Re-entering credentials cannot repair this pair.
- `possums_evidence_unavailable`: public evidence could not be retrieved within connection limits. Check connectivity and refresh; no inference is replayed.
- `possums_verification_failed`: the gateway cannot be verified against this client's approval. A gateway update is one possible cause, not an authenticated diagnosis. If updated, install the independently approved client/manifest pair and reload. Do not trust a version merely advertised by the server.
- `possums_session_unavailable`: public verification passed, but the challenge/login exchange did not complete. This does not establish that the credential is invalid. Check connectivity; a deliberate `/login` can establish a fresh session without inference.
- `possums_catalog_request_failed` / `possums_catalog_http_rejected`: catalog transport failed before a response, or the gateway returned an HTTP rejection. The latter includes only the observed status and recognized gateway code/detail, never response text or billing claims.
- `possums_catalog_body_invalid` / `possums_catalog_validation_failed` / `possums_catalog_conversion_failed`: response reading/JSON decoding, catalog schema/quote validation, or conversion to Pi metadata failed, respectively. Refresh models and share only the closed diagnostic if it persists.
- `possums_catalog_unavailable`: fallback for an unexpected catalog-stage failure. A previously authenticated session is not proof that it remains valid now. None of these catalog errors establishes an inference charge or refund.

After replacing an installed extension/manifest pair, fully restart Pi: `/reload` alone did not activate an earlier replacement in the operator's session. `/possums-status` should show the new compiled approval; it is not a live connection check.

Version wording comes from the compiled approval, not a hard-coded latest-version message or remote advertisement. These connection attempts send no inference and verify the channel before prompting for or transmitting a recovery credential. They do not establish the billing outcome of any earlier request. The extension's notifications and status command do not log, export telemetry or append session entries; native Pi error handling and ordinary session persistence remain separate. No automatic installer, remote update discovery or approval bypass is introduced.

## Safety and billing

- The recovery credential is saved by ordinary Pi authentication, normally in `~/.pi/agent/auth.json` (or the configured Pi agent directory). It is **plaintext at rest**, accessible to local tools/extensions with your permissions. Pi creates new auth files with mode `0600` and parent directories with `0700`; existing modes/ACLs are preserved. Bearer/catalog state remains memory-only, and request hooks receive a non-secret marker rather than the recovery key or bearer.
- Discovery failure clears the provider's usable catalog. Every invocation independently refreshes and validates the authenticated catalog before obtaining a model-bound submission.
- Only text content and OpenAI function declarations/results are supported. Images, grammar/custom namespaces, incompatible assistant-provider history, arbitrary fetch/sampling/output-cap overrides, incomplete tool batches, and unsupported models fail closed.
- Tool deltas are provisional. Identity and argument fragments may arrive separately; incomplete calls are never executable. Pi receives completed executable calls only after validated arguments and ordered **finish → settled usage → DONE → authenticated EOF**. See the [split-stream fix evidence](../../docs/verification.md#live-pi-split-tool-stream-compatibility-fix).
- Ordinary inference requires a user-authorized run start or a continuation containing all receipted tool results. Duplicate continuations and uncertain/error attempts cannot be replayed automatically. Cache warming remains blocked. Native compaction has a separate, scoped summary path that does not consume or replace ordinary run/continuation authorization.
- Pi 1.0.4 removes recoverable `length` attempts before its compaction hook. A valid gateway `length` therefore becomes a **non-retryable terminal partial-answer error** in Pi, with paid text/usage retained and the original finish in `possums_settled_receipt` diagnostics. It is not a refund or an unpaid failure.
- Receipt charges are authoritative. Submitted quote rates supply the displayed cost components; rounding remainder belongs to output. Legacy receipts without quote rates retain their settled total without invented components. Exact microunit strings are in receipt diagnostics.
- A disconnect/abort does **not** promise server cancellation or a refund. Without a valid receipt, billing is **unknown**, even when Pi's numeric usage placeholders are zero. A deliberate new request may pay for both generations.

## Request encoding diagnostics

`possums_request_json_depth` means local request encoding exceeded the defensive **32-level** JSON nesting limit. Simplify deeply nested tool schemas or history; this attempt sent no inference request and was not replayed. Tool schemas count inside the full request envelope, not in isolation. Byte, node and tool-count ceilings still apply. This diagnostic does not establish billing for earlier attempts; post-submission transport failures still report unknown billing. See the [offline fix evidence](../../docs/verification.md#local-pi-json-depth-compatibility-fix).

## Compaction

Manual `/compact` and automatic threshold compaction use the installed **Pi 1.0.4 native `compact()` helper**, with the selected model and ordinary native authentication. Pi owns serialization, prompts, previous-summary merging, split turns, file tracking, successful usage totals and checkpoint/context pruning. Other providers are unchanged; no settings are registered or changed.

Compaction is paid harness summarization, not a new Tinfoil endpoint. It makes one summary call, or at most two sequential native calls for a split turn, with **no retry policy**. Only the private, model-bound summary callback ignores Pi's advisory output-token hint; ordinary output overrides remain rejected. The gateway still reserves the selected model's full output allowance at its submitted rate. Summary requests have no tools and cannot emit executable calls.

Recovery compaction with `willRetry=true` is cancelled: compaction cannot authorize replay of an uncertain prior generation. Errors, length-limited/aborted/empty summaries, tool attempts and stale session/auth responses create **no checkpoint** and do not fall back to another summarizer. A short transient notification preserves the safe failure category/stage and observed settled charges, including a successful first summary when the second fails. Failed compaction does not undo settled charges; abort does not promise cancellation or refund. Failure charges are not added to a persistent local error ledger or checkpoint, so session totals need not include them. A deliberate `/compact` may incur new charges; `/new` starts fresh context instead.

Offline actual-SDK regressions cover this path. No live summary or additional model qualification is claimed.

## Local privacy boundary

This is an ordinary Pi extension, outside the gateway measurement. Pi and its installed extensions/tools can see plaintext and have host authority. Pi normally saves plaintext sessions; `--no-session` disables that session file, not saved authentication or every possible cache, log, tool write, screenshot, bug report, or export. Do not use `/share`, `/export`, or transcript-bearing support artifacts for sensitive sessions. Do not paste credentials into conversation messages or shell commands.

The custom provider uses no prompt cache and makes no telemetry export. This does not establish a no-retention or no-telemetry guarantee for the whole Pi process, its other extensions, or the operating system. Changing away from Possums can send retained history through another provider; start a fresh session when changing trust boundaries.
