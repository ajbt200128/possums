# Possums provider for Pi 0.99.2

Local implementation, not Phase 0.2 release acceptance. See [handoff](../../docs/phase02-handoff.md).

## Build and load

Requires Node **24.13.0**, npm, and the existing **Pi 0.99.2** installation. The build creates a fresh scratch source/install/package tree, installs the locked verification dependencies with lifecycle scripts disabled, checks TypeScript, verifies the three Pi package versions, and emits `extension.mjs` plus a source/artifact hash report. It does not install into the repository or modify the Pi installation.

`POSSUMS_PI_ROOT="$HOME/.pi/agent/install/releases/0.99.2" sh clients/pi/build.sh`

The command also prints a scratch-only **local checks** command. Run it for the synthetic reference/Pi and shared admission suites; it makes no live inference request and does not qualify production models. Test-only bundles remain outside the package directory.

Use the **package path printed by that command**; it is not an installation into your normal Pi profile. Load the extension with the independently approved **public release manifest**, never an account credential file:

`pi --no-session --no-extensions --no-tools -e /absolute/path/to/package/extension.mjs --possums-manifest /absolute/path/to/tinfoil-deployment.json`

The current approval is still for accepted **v0.0.9**, with administrative expiry **2026-10-11**. The changed gateway needs a new measured release and explicitly approved evidence before this client can use it. Do not change pins to make a verification error disappear.

1. Use `/login`, choose Possums, and paste the manually issued recovery credential into its masked prompt. Public evidence and key binding are verified **before** the credential is requested.
2. Select a live Possums model in Pi's usual model picker. With `enabledModels` patterns configured, Pi 0.99.2 resolves its scoped list at startup, before Possums can authenticate. After successful login **in the same process**, use `/model possums` and press **Tab** to switch from `scoped` to `all` if the results are empty. This changes the picker view, not your saved provider filter. Selecting a model adds it to the current session's scope. Labels show `text only` or `tools` and an **indicative**, not promised, maximum reservation.
3. For the currently text-only release, use the isolated launch above and run `/possums-text-only` explicitly. `--no-extensions` still permits the explicit Possums `-e` entry, but excludes other extensions that can re-enable tools before a turn (for example the interactive `ask_user_question` reconciler). `--no-tools` starts with no tool declarations. The command alone is a one-time loadout change, not a lock against another extension; a remaining tool declaration causes local `possums_tools_unsupported` before inference submission. Active tools are never silently removed, and models or output allowances are never substituted.
4. Use ordinary Pi tools only after an independently qualified tool profile is published by an approved gateway release. No Possums-specific tool implementation is installed.
5. `/possums-logout` forgets the local bearer/catalog. Accepted work can still settle remotely. A new Pi process or explicit logout requires the recovery credential again; `/new` retains the process-local login but resets conversation/run authorization.

## Safety and billing

- The recovery credential and bearer stay in memory. Pi may store a **constant, non-secret marker** in its credential store; that marker cannot authenticate at the gateway.
- Discovery failure clears the provider's usable catalog. Every invocation independently refreshes and validates the authenticated catalog before obtaining a model-bound submission.
- Only text content and OpenAI function declarations/results are supported. Images, grammar/custom namespaces, incompatible assistant-provider history, arbitrary fetch/sampling/output-cap overrides, incomplete tool batches, and unsupported models fail closed.
- Tool deltas are provisional. Pi receives completed executable calls only after validated arguments and ordered **finish → settled usage → DONE → authenticated EOF**.
- Only a user-authorized run start or a continuation containing all receipted tool results authorizes an invocation. Duplicate continuations and uncertain/error attempts cannot be replayed automatically. Cache warming and compaction requests are blocked for Possums.
- Pi 0.99.2 removes recoverable `length` attempts before its compaction hook. A valid gateway `length` therefore becomes a **non-retryable terminal partial-answer error** in Pi, with paid text/usage retained and the original finish in `possums_settled_receipt` diagnostics. It is not a refund or an unpaid failure.
- Receipt charges are authoritative. Submitted quote rates supply the displayed cost components; rounding remainder belongs to output. Legacy receipts without quote rates retain their settled total without invented components. Exact microunit strings are in receipt diagnostics.
- A disconnect/abort does **not** promise server cancellation or a refund. Without a valid receipt, billing is **unknown**, even when Pi's numeric usage placeholders are zero. A deliberate new request may pay for both generations.

## Local privacy boundary

This is an ordinary Pi extension, outside the gateway measurement. Pi and its installed extensions/tools can see plaintext and have host authority. Pi normally saves plaintext sessions; `--no-session` disables that session file, not every possible cache, log, tool write, screenshot, bug report, or export. Do not use `/share`, `/export`, or transcript-bearing support artifacts for sensitive sessions. Do not paste credentials into conversation messages or shell commands.

The custom provider uses no prompt cache and makes no telemetry export. This does not establish a no-retention or no-telemetry guarantee for the whole Pi process, its other extensions, or the operating system. Changing away from Possums can send retained history through another provider; start a fresh session when changing trust boundaries.
