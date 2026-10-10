# End-to-end diagnostic transparency — discussion draft

**Scope superseded, 2026-10-10:** the operator selected Possums-only improvements, without Pi/T3 changes. The active scoped draft is `docs/possums-only-diagnostics-plan.md`. Host changes below remain historical proposals, not approved work.

Prepared by delegated Sol (`openai-codex/gpt-6.1-sol`), 2026-10-10; condensed into this review document by the parent assistant. **Plan only, not implementation or qualification.** GitHub quota mitigation is separate work.

## Goal and evidence

Within a qualified Possums → Pi → RPC → T3 orchestration → UI path, preserve an observed failure's closed code, component/stage, constraint, observed HTTP status, actionable next step and evidence-supported outcome. A later failure must not erase an earlier diagnostic or authenticated receipt.

This does not guarantee delivery after process death, a root cause for every failure, whole-process transience or newest-release discovery. Failures before project code executes need host-authored, component-specific fallbacks that explicitly acknowledge unknown cause and outcome.

Observed probes reproduced `possums_evidence_unavailable`, stage `release_discovery`, constraint `rate_limited`, HTTP 403, followed by an empty catalog and failed `set_model`. They do not establish that cause for every earlier incident. Missing-model evidence alone establishes neither attestation failure nor invalid authentication.

Relevant source boundaries:

- `clients/pi/diagnostics.ts` already has `ConnectionFailure`, `EvidenceObservation` and locally authored descriptions. `bootstrap.ts::acquire` classifies rate limiting without retaining response text.
- `examples/phase01/{limits,client,transport}.ts` distinguish encoding, verified transport, stream validation and accounting evidence. Receipts/refunds require the existing authenticated terminal sequence and EOF.
- `clients/pi/provider.ts::perform` preserves receipts before display conversion; inspect whether its abort branch masks an earlier non-gateway diagnostic.
- `clients/pi/index.ts` keeps `lastFailure` and warning suppression independently of provider epochs; qualify reset and ownership semantics.
- Inspected T3 snapshot `/private/tmp/possums-t3-interrupt-source`, HEAD `0a728cd`, drops notifications without an active turn in `packages/provider-pi/src/server/adapter.ts::handleExtensionUiRequest`. `startTurn` applies model selection before installing the turn.
- T3 discovery and execution use separate Pi processes. Discovery failures must not be attributed to another process's submission.
- T3 `provider-core/src/server/failure.ts::causeMessage` substitutes generic startup wording. Inspect raw RPC causes and extension error forwarding: truncation is not a closed diagnostic contract.
- Pi 1.1.0 exits on extension-load failure before RPC starts. Possums hooks cannot cover that boundary. Native retry-backoff abort can drop final `errorMessage` while prior callbacks retain it.

The T3 snapshot is **not proven identical to the installed nightly**. Existing full qualification is Pi 1.0.4; Pi 1.1.0 factory/RPC probes do not establish complete behavioral qualification. Preserve the separately authorized runtime-guard removal; do not overwrite it.

## Diagnostic contract

Extend existing carriers rather than build a speculative framework. Use a versioned, bounded projection with:

- Closed component, code, stage and constraint.
- Optional observed HTTP status, integer 100–599, and locally observed versus authenticated-gateway provenance.
- Closed action identifier; the receiver authors display wording.
- Separate delivery/operation and billing evidence: not submitted, accounting unknown, authenticated settlement or authenticated refund.
- At most one closed secondary consequence, such as model selection unavailable.

Reject unknown properties, invalid combinations and hostile/free-form values. Never forward raw exception/cause/message, stderr, panic payload, stack, URL, header, credential, identifier or request content. A valid-looking `possums_…` prefix is not producer attribution or authenticity.

Associate observations with trusted local producer/host ownership, not message text. This cannot defend against a compromised Pi process or privileged extension. Billing still requires existing validated accounting evidence.

Use existing ownership state to scope observations to provider/process/session/account epoch and attempt. Clear current status on accepted recovery or replacement; reject late updates. Preserve an earlier retry's unknown billing without presenting that failure as the later attempt's failure.

Diagnostics and offline status initiate no inference, replay, renewal or recovery. Native retry budgets, classifiers, trust renewal, cancellation and accounting remain unchanged.

## Small implementation phases

### A. Specify and reproduce losses

Files: `docs/pi-diagnostic-verification.md`, `clients/pi/README.md`, `tests/phase02_diagnostics.mjs`, `tests/phase02_pi.mjs`; T3 adapter, status, failure and orchestration tests.

Add synthetic failing regressions for pre-turn notification loss, discovery-event loss, generic startup mapping, stale diagnostic ownership, abort masking, extension-load failure and hostile carriers. Decide persistence before implementing it. Fixtures must use no external networking or inference.

### B. Preserve Possums observations

Files/symbols: `diagnostics.ts::{ConnectionFailure,connectionFailure,diagnosticDescription}`, `index.ts` lifecycle/report callback, `provider.ts::{reportConnection,logout,newSession,perform,safeFailure}` and existing reference carriers where necessary.

Expose immutable safe metadata beside authored text; do not recover fields by parsing prose. Unexpected failures get explicitly unknown component-specific fallbacks. Make diagnostic resets explicit without changing authentication lifecycle. Preserve primary failure across cleanup/abort and settled evidence across display failure.

Existing security/accounting suites must pass with unchanged retry classification and submission counts. Do not restore a version-number block or weaken attestation, publisher or build checks.

### C. Carry startup diagnostics through Pi/RPC and T3

Likely Pi files: extension loader, `main`, `modes/rpc/{rpc-mode,rpc-types}`. T3: `rpc.ts`, `adapter.ts`, `status.ts`.

Preferred path: a small upstream structured RPC carrier with producer attribution and an offline current-state read. Cover session/startup observations, command failures and existing assistant/retry events. The loader authors import/factory-stage fallbacks; it must not infer identity by parsing exceptions.

A Possums bridge through supported extension events/RPC UI APIs may offer a limited interim path. It cannot cover failures before hooks load and must not be called full end-to-end acceptance. Reject arbitrary notification forwarding, prefix recognition, direct stdout writes and parsing “Model not found” as general solutions.

T3 retains bounded pre-turn observations and attaches only the current process/session's primary cause to selection failure. Discovery stays separately scoped. Without a correlated primary cause, emit a closed model-selection fallback with unknown underlying cause. No automatic refresh or model substitution.

### D. Preserve orchestration and presentation

Likely T3 files: `contracts/src/orchestrationV2.ts`, `provider-core/src/server/failure.ts`, `apps/server/src/orchestration-v2/RunExecutionService.ts`, client-runtime error/work-log helpers and actual UI renderer.

Validate before wrapping/terminalizing errors; never expose nested raw causes. Prevent raw Possums RPC/extension errors from entering request-level logging. Preserve prior retry diagnostics through abort without changing retry policy.

Display actionable failures before a provider turn exists, not buried in collapsed notification tool items. Use closed transport/rendering fallbacks if presentation fails.

Matching renderer source is absent from the inspected partial checkout. Obtain matching source/artifact access before claiming UI coverage; do not patch installed bundles.

### E. Qualify artifacts and runtime

Record source revisions, dependency versions, extension hashes, host builds and tested surfaces. Distinguish source, built, installed, activated and deployed evidence. No gateway deployment is implied.

Qualify offline Pi 1.0.4 and 1.1.0 paths, then built T3 web UI and desktop UI paired to the service. Installation/activation and actual installed-artifact checks require separate approval. Retain previous builds for rollback; never roll back trust pins or replay failed work.

## Acceptance matrix

Every row needs real SDK/RPC/adapter/orchestration/render integration, not formatter tests alone:

| Boundary | Required result and invariant |
| --- | --- |
| Startup/session creation | Closed stage-specific failure reaches UI before a turn; no inferred credential failure or prompt submission. |
| Extension import/factory | Real CLI fixture covers failures before hooks; loader metadata or explicit unknown process-exit fallback. Hostile stderr never displayed or persisted. |
| Evidence/trust | Preserve release, provenance, certificate and key-binding categories. Rate-limit fixture retains 403 and `rate_limited`; fail closed before credentials/inference. |
| Authentication/catalog | Distinguish rejection, transport, body, validation and conversion. Empty catalog does not imply expiry or attestation failure. |
| Model selection | Correlated same-session primary cause or selection-only fallback; no cross-process attribution, substitution or automatic rediscovery. |
| Encoding/transport/gateway | Preserve known encoding, binding, SDK decoding and gateway-validation distinctions. Unknown upstream status stays unknown. |
| Stream/accounting | Separate missing finish/usage/DONE/EOF and settlement failures; no tool execution/refund claim without required terminal evidence. |
| Abort/disconnect/retry/cleanup | Inject before-send, streaming, backoff and post-receipt races. Preserve primary failure and earlier unknown billing; retry and accounting counts unchanged. |
| Display/process/UI failure | Keep authenticated settlement despite display errors. Without terminal evidence, billing remains unknown; transport/render failure cannot imply cancellation. |

Use synthetic hostile sentinels, including prefix-looking codes, ANSI/HTML, secrets, URLs and nested causes. Inspect synthetic RPC, state transport, DOM, logger sinks and isolated persistence. No new diagnostic/support export may contain sentinels. Diagnostics/status must initiate zero inference.

## Privacy and decisions

Read and apply `PRIVACY.md`: **Mandatory reference and change control**, **Data handling boundaries**, **Telemetry: permitted signals and forbidden data**, and **Required evidence for every telemetry change**. Prefer volatile startup/status metadata; no diagnostic journal, logs, traces, support bundle or telemetry labels.

Safe transport is not permission to persist diagnostics. T3 terminal failure items enter its persistent orchestration event store. Proposed minimal option: explicitly authorize closed metadata in existing terminal failure records and disclose that host-history boundary. Without authorization, keep detail volatile and do not promise it after reconnect/restart. Native Pi assistant-error persistence remains separate; `--no-session` does not disable T3 history.

Decisions before implementation:

1. Authorize the precise safe-metadata persistence boundary, or require volatile-only presentation.
2. Choose upstream structured carrier versus explicitly limited interim bridge.
3. Obtain matching T3 renderer source/build access.
4. Select runtime/package combinations for qualification and separately approve installation/activation.

Example user-visible failure:

> `[possums_evidence_unavailable] Stage: release_discovery; constraint: rate_limited. Observed HTTP status: 403. Public release evidence was unavailable, so the selected model could not become available. Wait before deliberately starting a new verification session; re-entering credentials will not fix this. Do not bypass verification. This connection attempt sent no inference; earlier billing is not established.`

The operator selected shared public-evidence acquisition caching as the separate agreed direction in `docs/pi-public-evidence-cache-plan.md`. GitHub authentication is deferred. Neither that direction nor this diagnostic draft authorizes implementation, installation, deployment or changes to trust validity.
