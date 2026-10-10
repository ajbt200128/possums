# Pi diagnostic packet — local source verification

## Scope and authority

This candidate is based on main's local API-only revert state `1358856` / `23792a1`. No merge, installation, deployment, public verification fetch, real credential access, inference, telemetry query or support upload was performed. The free endpoint experiment remains out of scope on `feature/free-endpoint`. The reported generic client error's cause remains **unknown**; this is not evidence of trust or authentication expiry.

Inspected recovery work at `ee80c8b` and reused its closed evidence-stage/constraint/status observation and acquisition classification narrowly. Main already contained its catalog-stage carrier/mapping. No trust renewal, startup/account race correction, replay guard or recovery branch merge was imported. Interruption evidence in `fix/interrupt-session-recovery` (`61dc3fc`) was inspected as historical, separate evidence; its T3 patch was not applied. Provider timeout alignment and drain policy remain separate work. Published trust remains at most 12 hours, capped by certificate expiry; explicit approvals retain their administrative expiry.

Policy reviewed completely before work: [AGENTS user-facing errors](../AGENTS.md#user-facing-error-policy), [PRIVACY change control](../PRIVACY.md#mandatory-reference-and-change-control), [data handling/support](../PRIVACY.md#data-handling-boundaries), [forbidden errors/content/identifiers](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data), [synthetic testing](../PRIVACY.md#synthetic-testing-exception) and [required evidence](../PRIVACY.md#required-evidence-for-every-telemetry-change). Detailed notifications are not telemetry labels or support artifacts. No telemetry/logging/export pipeline changed; outbound OTLP/privacy release qualification was therefore not rerun or retrospectively claimed.

## Failure map and smallest implementation

Existing catches erased known constraints into `ChannelError(rejected/uncertain)`, and the provider fallback then displayed generic rejection. The patch keeps the existing validation and SDK wire handling, recording only closed diagnostic observations at those catches. It does not parse provider exception strings to guess a cause.

| Boundary | Preserved diagnostic | Accounting / action |
|---|---|---|
| Extension/runtime and manifest loading | Pinned runtime version, unavailable/mismatched manifest | No inference; use matching approved runtime/client/manifest |
| Public evidence acquisition | Eight closed acquisition stages; request, HTTP/rate-limit, destination/redirect binding, body/schema, interrupted/idle/deadline | No inference; check connectivity, wait for rate limits, start a new verification session; do not bypass pins |
| Trust checks | Expired vs policy; manifest, attestation preflight, combined SDK verification, publisher, certificate, key configuration | No automatic renewal or release substitution; expiry is not credential expiry |
| Credential selection, entry, challenge/login | Missing credential, local selection/entry fallback, challenge vs authentication fetch/body/schema/HTTP | No inference; do not share credentials; HTTP rejection does not establish expiry |
| Catalog | Request, HTTP, bounded body, schema/quote validation, Pi metadata conversion | Clear unusable catalog; do not authorize from cached models or assert an inference charge |
| Request and submission | JSON depth, encoding/schema, unsupported history/options, hook failure, submission transport/body/schema | Encoding failures send no inference; uncertain submission is not automatically replayed by this client |
| Verified transport | Encryption, fetch/HTTP, endpoint binding, frame bounds, decryption, UTF-8, operation constraint | Unknown accounting without an authenticated terminal outcome; check connectivity/report only closed fields |
| Gateway / SDK errors | Allowlisted gateway reason/detail; distinct gateway validation/SDK decoding/settlement stages | Only existing closed gateway operational failures opt into native bounded retry |
| Stream validation / terminal delivery | JSON/schema/envelope/choice/delta/tool/finish/usage/receipt; missing finish/usage/DONE/EOF | No tool authorization; interrupted claimed receipt/refund remains unknown |
| Local provider presentation | Stable component-specific unexpected fallback, paid output limit, post-receipt precision failure | Unknown cause explicitly stated; an already authenticated receipt remains settled even if numeric display fails |
| Balance/reconciliation and compaction | Closed stage-specific fallbacks; known transport observation retained for balance | Diagnostics/balance commands send no inference; compaction can charge but does not fall back or replay after failure |

The EHBP fixture exposed a concrete error-preservation race: `encryptedFrames` cancelled its operation after detecting a frame violation; that abort could win the SDK wait race and mask the violation as interruption. Similarly, evaluating diagnostics after cleanup could replace a UTF-8 violation with its subsequent cancellation. The correction snapshots one closed request-local frame diagnostic before the **existing** cancellation, and chooses the diagnostic before cleanup. No raw exception/cause is retained, no SDK decoder is reimplemented, and cleanup order, timers, ceilings and drain behavior are unchanged. Actual encrypted fixtures now distinguish corrupted cipher, invalid UTF-8 and oversized encrypted frame prefix with observed HTTP 200.

An additional parsed, authenticated synthetic u64 receipt demonstrated local numeric display overflow. Recording the receipt before display conversion prevents the client from incorrectly losing its known settled outcome or mislabelling it as a catalog error. Gateway reservation/settlement behavior is unchanged.

## Offline evidence

Used Node 24.13.0, TypeScript 5.9.3, esbuild 0.25.10 and the existing Pi 1.0.4 runtime packages. Locked verification dependencies were reused from an existing scratch tree; no dependency download or client installation was performed. Fresh source and production/test bundles were built under `/private/tmp/possums-pi-build-6AodEJ`.

Passed the full scratch `clients/pi/check.mjs` integration harness:

- **33** phase01 reference-client checks.
- **506** phase02 tool/client/transport checks, including actual encrypted EHBP frame/decryption/UTF-8 distinctions and hostile constructor fields.
- **128** synthetic release-policy checks, including every public-acquisition stage with hostile fetch/body/HTTP/rate-limit responses and expired trust before any request.
- **83** Pi checks, including eight new diagnostic groups, 21 closed component failures through actual native sessions, native authentication-cause preservation, hostile credential selection/entry, post-receipt precision, native retry-abort reproduction, and existing steering/retry/logout/compaction/reconciliation behavior.
- **71** shared admission checks; 65,540-property rejection fixture still has zero stringify/encryption/network/getter calls on rejection.

The diagnostic groups cover invalid JSON/UTF-8, event schema/choice/delta/tool/finish/usage/receipt, missing finish/usage/DONE/EOF, read failures, hooks, parent interruption/idle/deadline provenance, bounded HTTP observations, trust vs authentication rejection, catalog stages, encoding and submission. Existing tests cover all allowlisted gateway details, including SDK decode, upstream idle/deadline, validation and settlement. Hostile prompt/credential/header/URL/error sentinels cannot escape through authored messages or retained gateway diagnostic fields. A claimed refund is confirmed only after authenticated EOF; interrupting its delivery never manufactures a refund. Fixtures use synthetic content and unusable credentials, local in-memory responses and isolated native stores, not real inference or platform credentials.

Pinned full-project TypeScript checking and `git diff --check` passed. Active LSP confirmed the bootstrap clean after the obsolete `operation` advisory disappeared. Three earlier dependency-resolution diagnostics were stale from before local dependency links existed; the pinned compiler resolved all three, and they were marked false-positive in the session without source suppressions. The temporary checkout dependency links were removed after verification; a renewed bare-checkout `ehbp` advisory was classified as the same dependency-resolution limitation, not hidden by source changes or an installation. No blanket clean-LSP workspace claim is made.

Final scratch production `extension.mjs` SHA-256: `70cc73c01165f2be8112530ad0d885c605057ac52a9957df8f2977bfc066a9dd` (artifact/build report is scratch-only, not installed). The release suite builds production and isolated hardware-fixture bundles separately, checks production mock exclusion, and uses real SDK orchestration/policy helpers/SHA-256/gzip/EHBP config with synthetic hardware/DSSE/X509/network fixtures. This is not real hardware, deployed catalog, retention or live billing verification.

Independent read-only review found no unresolved defect. Its initial concern about terminal local transport failures was refuted against HEAD's existing gateway-only retry allowlist and actual-SDK tests: this packet preserves that policy rather than expanding it.

## Native Pi / T3 boundary and remaining unknowns

Actual SDK fixtures show that `ModelsError.withCauseDetail` preserves the locally authored authentication diagnostic, and actual sessions retain closed assistant errors. Unexpected provider failures remain terminal, while existing gateway transient failures still use Pi's bounded native retry budget/backoff. Each new attempt may charge again; no safe-retry/refund claim follows delivery failure.

One external loss is reproduced exactly: Pi 1.0.4 `pi-ai/dist/utils/retry.js`, `retryAssistantCall`'s `RetrySleepAbortError` catch removes `errorMessage` from its returned aborted assistant. The preceding retry callback retains the diagnostic; the existing actual-session abort fixture retains the earlier error assistant and unknown-billing diagnostic. No retry/cancellation policy or SDK installation was modified to compensate.

Read-only source inspection of `/tmp/possums-t3-interrupt-source/packages/provider-pi/src/server/adapter.ts` shows assistant `errorMessage` forwarded as the provider-failure message (lines 1713–1717 in that checkout) and retry diagnostics forwarded at lines 1805–1809. That is source evidence, not installed T3 rendering, packaging or activation evidence. The installed bundle, deployed gateway and exact generic-error incident were not inspected or reproduced. Process termination, extension/module-load failure before this extension registers, native credential-store failure outside provider callbacks, a missing/throwing UI, and final host rendering remain external paths; no whole-app “never generic” guarantee is claimed.

Connection/status/compaction notices remain transient and are not appended as diagnostic reports. Assistant error messages and existing receipt diagnostics still travel through ordinary native Pi/T3 session history; persistence, other extensions, process-level logging and support/export behavior belong to that separate client boundary. No history setting was changed, no raw native exception/cause was introduced, and no support bundle or telemetry was added. Share only the content-free code/stage/constraint/observed status manually, never a transcript or credential.

## User-visible examples

- `[possums_trust_expired] Stage: trust; constraint: expired` — session-pinned trust elapsed. Start a new Pi session to verify again; do not bypass verification or assume credential expiry. Fresh-submission renewal remains separate work.
- `[possums_authentication_http] Stage: authentication; constraint: http. Observed HTTP status: 401` — observed authentication rejection. Check recovery credential locally/use `/login`; this connection attempt sent no inference.
- `[possums_stream_usage_missing] Stage: stream; constraint: usage_missing` — no validated terminal usage; no tool authorization. Billing unknown; not replayed by this client; a new submission may charge again.
- `[possums_provider_unexpected] Stage: provider; constraint: unexpected` — unexpected local failure, underlying cause unknown. Share only this code/stage/constraint for support; billing unknown without a receipt.
- `[possums_settlement_precision] Stage: settlement; constraint: precision` after a complete receipt — local safe-integer display range exceeded. Authenticated charge is settled, not undone; no fabricated refund.

## Operator-authorized local merge and installation

The operator subsequently authorized merging and installing, then explicitly selected **local main + install**, leaving published main unchanged. Local main was fast-forwarded from `1358856` to diagnostic source `45ec576`; both free-feature reverts remain ancestors. `origin/main` remains `55b9213`, with its separate image-update commits neither reconciled nor published by this operation. The independently active timeout worktree was not checked out, modified or merged.

Copied the qualified production extension, package metadata and build report into `~/.local/share/possums/pi/pi-diagnostics-70cc73c01165`, retaining the existing Pi 1.0.4 peer links. The existing `installed` symlink was atomically replaced using filesystem rename, from `pi-reconciliation-v18-fa47e2f7b6c1` to the diagnostic directory. The old package remains available for rollback. Installed extension SHA-256 exactly matches the verified source build: `70cc73c01165f2be8112530ad0d885c605057ac52a9957df8f2977bfc066a9dd`.

Offline installed-package verification passed import, all three peer-version checks, the real extension factory's runtime guard/provider/hook/command registration, and `/possums-status` with a synthetic UI. Fetch was explicitly forbidden and **zero fetch attempts** occurred; this is not a whole-process networking/telemetry audit. No authentication, public evidence fetch, catalog refresh, submission or inference was invoked. No auth store, history, settings, default providers, gateway configuration or running process was changed. Temporary check script: `/tmp/possums-installed-diagnostics-check.mjs`; it emits only content-free check results, not a support bundle.

A full restart of the existing **Pi 1.0.4** session/process is needed to activate the replacement. Running-session activation, installed live trust/auth/catalog startup and T3 UI rendering were not tested or claimed. The installed client change does not deploy gateway code, renew trust automatically, establish the original incident's cause or resolve the documented external retry-abort boundary. The earlier source-only/no-install statements remain scoped to the pre-authorization verification, not this subsequent authorized operation.
