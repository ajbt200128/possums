# Phase 0 verification record

Status values are **VERIFIED**, **FAILED**, and **UNKNOWN**. An UNKNOWN or FAILED mandatory gate blocks production prompt transmission and release.

## Evidence checklist

| Claim | Status | Required evidence |
|---|---|---|
| Tinfoil Rust inference verification API and cache controls | UNKNOWN | Versioned API review and instrumented no-prompt-on-failure tests |
| Upstream SEV-SNP attestation and release provenance | UNKNOWN | Valid and invalid live quotes/releases |
| Serving HTTPS key is bound to gateway attestation | VERIFIED | `v0.0.5` fresh quote verification matched the endorsed and serving TLS SPKI fingerprints |
| Live catalog authentication, rates, and units | VERIFIED | The deployed authenticated UI rendered all seven strictly validated models; production-schema and negative tests cover rates and units |
| Per-model tokenizer/context and authenticated usage semantics | UNKNOWN | Provider specification and boundary vectors for every model |
| Gateway self-attestation API | VERIFIED | The deployed `/attestation` route returned fresh gateway evidence and an upstream verification document after independent `v0.0.5` quote verification |
| OCI build reproducibility | VERIFIED | Publish run 36516962509 produced matching archives in two independent clean Linux jobs |
| OCI digest equals deployed measured configuration | VERIFIED | Release `v0.0.5`, image digest `sha256:cd8075a02ebcfec9c04973196bfee5cf81b4aaf0a693d20c4130cbba41e1946c`, manifest digest `3a54f1ca9a11b9e60b66e4be80887c2129dbdb9f81c6e864e5aac8a429bf4f8f`, and the live quote agree |
| Root/mount/cache/core-dump behavior | UNKNOWN | Runtime mount, write, process-limit, and crash inspection |
| Proxy/platform request logging is disabled | UNKNOWN | Platform configuration plus canary inspection |
| Egress policy is enforced | UNKNOWN | Measured policy and blocked-destination probes |
| Honeycomb US seven-day retention/access controls | UNKNOWN | Account-side configuration evidence and expiry observation |
| Collector exports only non-identifying aggregates | UNKNOWN | Captured export with seeded sensitive canaries and sparse buckets |
| TLS termination and host observability boundary | UNKNOWN | Deployed network architecture and operator/platform documentation |
| No SDK/inference cache persistence | UNKNOWN | Runtime cache inspection and upstream configuration evidence |

## Local verification

Run on 2026-09-26, aarch64-darwin, with the pinned Rust 1.88.0 Nix shell:

| Check | Result |
|---|---|
| `cargo fmt --all -- --check` | VERIFIED |
| `cargo clippy --all-targets --all-features -- -D warnings` | VERIFIED |
| `cargo test --all-targets --all-features` | VERIFIED: 41 tests passed |
| `cargo test --test transport` | VERIFIED: total-body trickle deadline, total-header deadline, and oversized-header rejection |
| `cargo test --test inference --test accounting --test web` | VERIFIED: 22 tests passed, including chunked/absent/oversized/stalled upstream bodies, shared-memory response-lifetime admission, hostile render expansion, unconsumed-response abandonment, disconnect/completion boundary races, panic, settlement/refund races, and reauthentication replay |
| `npm run test:browser` | VERIFIED: real Chromium with JavaScript disabled completed login, recovery download, model selection, and two chat turns without remote requests |
| `go test ./...` in `attestation-helper` | VERIFIED: nonce-bound Unix-socket request and fail-closed socket/input behavior |
| `actionlint .github/workflows/*.yml` | VERIFIED |
| `nix flake check --print-build-logs` | VERIFIED for aarch64-darwin checks, including the Go helper; other systems omitted |
| `nix build .#gateway-image --print-build-logs` | Not exposed on aarch64-darwin; the OCI derivation is Linux-only |
| Linux OCI build and digest comparison | UNKNOWN: no Linux builder was available |

Crane is pinned to v0.21.0, compatible with the pinned nixpkgs 25.05; lock evaluation no longer emits the prior crane/nixpkgs compatibility warning. CI must report unavailable platform checks as blocked, never silently skip them.

### Opt-in live Tinfoil test

`tests/live_tinfoil.rs` uses the exact production attested client to fetch and validate the live catalog, authenticate tokenization, and make one fixed 64-token-maximum canary generation. It is ignored by default because it requires a funded credential and incurs upstream cost. Copy `.env.example` to the Git-ignored `.env`, set mode `0600`, populate `TINFOIL_API_KEY`, and run `nix develop -c cargo test --test live_tinfoil -- --ignored`. The test never prints the credential or sends user content. It passed against the live service on 2026-09-29.

## Deployed verification

Release `v0.0.5` was deployed on 2026-09-29 to `possum-phase0.possums.containers.tinfoil.dev`. Independent Tinfoil verification returned a fresh AMD SEV-SNP quote, the expected custom release predicate and manifest digest, matching endorsed/connection TLS-key fingerprints, and status `ok`. GitHub exposes one artifact attestation for the release manifest digest.

An authenticated no-JavaScript production canary then loaded all seven catalog models, counted input tokens through `/v1/chat/completions/input_tokens`, completed a bounded `gpt-oss-120b` generation, and rendered the expected marker with HTTP 200. Replaying the same submission token returned the terminal "already completed" notice without regeneration, demonstrating that complete response-body consumption reached settlement. The public `/attestation` route also returned HTTP 200 with both gateway evidence and the upstream verification document. This canary verifies the happy path, not every model's tokenizer semantics or deployed fault injection.

## Independent correctness and security review

The request path was traced in this order: expiring session/CSRF and issued submission-token validation; input parsing; verified gateway evidence; authenticated catalog; maximum-cost reservation; authenticated token count and context quote; bounded generation; prospective authenticated-usage charge; escaped/sanitized render; response-body completion and settlement. Tests prove gateway-evidence and reservation failures do not invoke prompt tokenization or generation, and malformed catalog/context failures do not invoke generation.

Review findings and current dispositions:

- The production gateway invokes a measured helper that requests a fresh random nonce-bound v3 quote from `/tinfoil/attestation.sock`. The pinned Tinfoil Go verifier authenticates quote hardware, signed release provenance, freshness witnesses, and the endorsed TLS-key fingerprint; missing, malformed, stale, oversized, or unverifiable evidence fails closed before prompt tokenization. Local tests cover socket request shape and fail-closed evidence handling. Independent `v0.0.5` verification matched the endorsed key to the public serving certificate, and the deployed helper returned gateway evidence successfully.
- Tinfoil's live `/v1/models` response uses an OpenAI-style `data` array, chat endpoint capabilities, context windows, and decimal USD-per-million-token prices; it does not carry a catalog issuance timestamp or separate maximum-output field. The gateway strictly adapts that production schema over the attested, endpoint-key-bound channel, uses remaining context as the output limit, and reserves the most expensive full-context allocation with fixed-point arithmetic. The authenticated `/v1/chat/completions/input_tokens` contract and one model's generation/settlement path passed both the opt-in test and deployed canary. Tokenization necessarily receives prompt content after reservation; every model's tokenizer semantics remain UNKNOWN.
- Tinfoil Rust SDK revision `34157e4` verifies attestation/provenance and pins TLS. Its documented `user_cache_secret` scopes caches but does not disable upstream persistence. The gateway uses a fresh random scope per generation and raw pinned requests, but actual cache retention remains UNKNOWN.
- Settlement occurs when the complete response body is consumed by the HTTP transport. Reservation ownership remains attached to the body: cancellation, panic, body error, or abandonment before completion refunds, while complete consumption settles the authenticated usage exactly once and releases the unused reservation. Local unconsumed-response drop, socket-disconnect, and settlement/refund race tests cover the boundary. Transport completion is not proof of browser receipt, and a later disconnect may still charge the user. Deployed write-failure injection remains required.
- Upstream response bodies are incrementally capped under one send-and-body deadline. Local HTTP peers test chunked and absent lengths, declared and streamed oversize bodies, and a body stalled after headers. The application server enforces connection admission, header count/buffer/deadline, and an 8 MiB body under one total 30-second deadline. A 512 MiB shared fail-fast application budget reserves a conservative 128 MiB envelope before each body is consumed and holds it through response drop, limiting admission to four requests. Rendering preflights hostile JSON/HTML expansion before allocation and caps output at 32 MiB. Concurrent maximum-body and hostile-expansion tests verify overload rejection and permit release. These envelopes cover application-controlled request parsing, upstream buffers, escaped rendering, and outstanding responses, but allocator/library overhead and whole-process memory require runtime measurement.
- `tinfoil-config.yml` pins the independently reproduced production image digest. The release-only publish workflow builds in two independent jobs, compares OCI manifest digests, publishes the exact first archive with digest preservation, and opens a config-digest pull request. A separately built CI-only fixture image starts with a runtime-injected account secret under a read-only root, memory-only `/tmp`, and no network; its test verifier is not included in the production image. Publish run 36516962509 and merged PR #3 established the `v0.0.5` digest chain.
- The deployed config selects CVM v0.14.12, enables the measured attestation socket, and passes its fixed path to the gateway; live gateway evidence proves socket access. Actual mounts, egress rejection, core/process limits, shim logs, and host observability remain UNKNOWN pending direct runtime inspection.
- Telemetry has no application exporter enabled. The current scaffold uses allowlisted lifetime counters and sparse-value suppression, not time buckets; it must remain disabled until real time bucketing plus resource/datapoint sanitization and Honeycomb region, retention, access, and end-to-end canary evidence exist.

## Prior finding disposition

| Finding | Local disposition | Regression evidence |
|---|---|---|
| Terminal accounting state exhausted capacity | Terminal tombstones are retained through token validity and reclaimed only after expiry | `tests/accounting.rs::terminal_state_is_reclaimed_only_after_token_expiry` |
| Unbounded credential work | Encoded credentials over 128 bytes are rejected before decoding or hashing | `tests/auth.rs::rejects_short_oversized_or_wrong_credentials` |
| Missing connection/header/total-body limits | Added zero-queue connection admission, shared fail-fast memory envelopes held through response drop, HTTP/1 header limits/deadline, one total body deadline, pre-render expansion checks, and a rendered-response ceiling | `tests/transport.rs`; `tests/web.rs::shared_memory_admission_is_fail_fast_and_held_until_response_drop`; `tests/web.rs::concurrent_maximum_bodies_are_bounded_and_release_memory_on_drop`; `tests/web.rs::oversized_request_body_is_rejected_before_upstream_calls` |
| Recovery download absent | Added authenticated no-store plain-text attachment route | `tests/web.rs::recovery_download_is_authenticated_and_never_cached` |
| Manual delivery confirmation burdened every turn | Complete response-body consumption settles automatically; abandonment before completion refunds | `tests/web.rs::no_javascript_chat_sets_security_headers_and_settles`; `tests/web.rs::dropping_unconsumed_response_refunds_reservation` |
| Deployment startup variables/hardening absent | Proposed config supplies inference and gateway repositories, enables the Tinfoil attestation socket, and declares read-only/memory-only/process/egress controls; image entrypoint disables core dumps. Live platform behavior is not yet validated | CI-only isolated startup smoke plus helper socket tests; deployed behavior remains UNKNOWN |
| Incompatible Nix pins | Crane pinned to v0.21.0 for nixpkgs 25.05 | `nix flake metadata --no-write-lock-file` |
| Independent image comparison absent | The release-only publish workflow builds in two independent Linux jobs, compares OCI manifest digests, publishes with digest preservation, and opens a digest-pin PR | `.github/workflows/publish-image.yml`; successful release evidence still required |
| Upstream body boundary evidence absent | Local HTTP peers cover chunked, absent/oversized lengths, streamed oversize, and stalled bodies under the production collector | `tests/inference.rs` |
| Lifecycle fault evidence absent | Local cancellation, socket disconnect, panic-after-reservation, expiry, settlement/refund race, and replay-after-reauthentication tests preserve full refunds and zero prompt calls where required | `tests/web.rs`; `tests/accounting.rs`; `tests/lifecycle.rs` |
| Browser/deployed privacy evidence absent | A pinned Playwright test exercises the complete JavaScript-disabled local browser flow and rejects hostile-output network attempts. Seeded startup errors emit no canary and create no working-directory artifacts; the production binary suppresses panic-hook payloads before serving. Deployed runtime artifacts, platform logs, and production canaries remain release blockers | `tests/browser.mjs`, `examples/browser_fixture.rs`, and `tests/privacy.rs`; external rows above remain UNKNOWN |

## Independent release procedure

1. Run `Publish reproducible gateway image` from the `main` branch.
2. Confirm its two clean Linux builds produced the same OCI digest and that the exact archive was published to GHCR.
3. Review and merge its digest-pin pull request after `check` succeeds on that branch.
4. Run `Tinfoil Release` with the next `vX.Y.Z`; confirm the measured release and Sigstore record were published from the tagged commit.
5. Confirm the public config digest, registry digest, and measured image/config association agree; distinguish the OCI digest from the hardware measurement.
6. Verify release provenance, a fresh hardware quote, and the serving HTTPS endpoint-key binding.
7. Fetch and authenticate a fresh catalog quote without sending a prompt.
8. Exercise every pre-prompt failure gate and observe zero prompt bytes upstream.
9. Inspect runtime mounts, cache paths, egress, core dumps, proxy/platform logs, and telemetry captures with seeded canaries.

Deployment, attestation, authenticated catalog, token counting, generation, rendering, and successful settlement have been verified for `v0.0.5`. Claims that remain UNKNOWN above are not implied by that happy-path canary and must not be advertised as guarantees.

## 2026-09-29 — packet 1 streaming financial-contract gate: BLOCKED

**Decision: BLOCKED for every supported model. Packets 2–8 must not start.** Seven small live streams were observed, but final billing semantics, the general tokenizer/usage relationship, and authoritative maximum billable-token constraints are not established. This is not a provider outage finding: the diagnostic's candidate single-usage-event contract was contradicted by otherwise normally terminated streams. No production streaming, accounting, catalog correction, model hiding, output-cap reduction, release, or deployment was performed. The historical buffered evidence above remains unchanged; its reservation assumptions are **not** accepted as a proof for the replacement.

### Probe scope and provenance

Implementation/exercised revision: `eb540212a23511e1987222a2ab6415087ca82dba`, based on `bd9971af31ef8260d18e003cf35077c3f063ed17`. Path: `tests/live_tinfoil.rs::streaming_contract_probe_all_catalog_models` → `src/inference.rs::TinfoilInference::probe_streaming_contract`. SDK remains pinned to `34157e497a747c191852d52af39cfbdb8dbd9eb7`. Existing user-owned documentation modifications and untracked client notes/`node_modules/` remained present but were not included in the implementation commit.

The ignored test requires `TINFOIL_STREAM_PROBE_FUNDED=yes` in addition to a local credential; it makes at most eight sequential fixed-input requests, each with test-only `max_tokens:64`. This run covered all seven authenticated catalog models. It uses the same verified origin-bound HTTP client and bearer authentication as production, fresh random `user_cache_secret` per generation, `stream:true`, and `stream_options:{include_usage:true}`. It bypasses neither attestation nor origin binding, and has no route or Possums ledger calls. No uncertain generation is automatically replayed. The original buffered canary remains ignored historical coverage, not streaming evidence.

Diagnostic limits: 16 KiB per framed event (including line endings), 1 MiB total received body, 512 data events, bounded JSON parsing with serde's default recursion limit, one 90-second send/body deadline inside a 120-second tokenization/probe deadline. Only the current event is parsed; answer/reasoning text is discarded. Transport/library buffers are additional to these application bounds, not a whole-process memory proof. The report includes only validated model IDs and allowlisted numbers/enums/booleans: no generated text, credentials, raw headers, provider error bodies, request IDs, attestation identifiers, or account artifacts. No telemetry was enabled. The diagnostic framing parser is not a production stream adapter; unsupported shapes fail rather than being silently accepted. Cache scoping is not evidence that upstream persistence is disabled.

### Authoritative references reviewed (documentation/source, not live guarantees)

Retrieved on 2026-09-29:

- [Tinfoil chat models](https://docs.tinfoil.sh/models/chat.md): model contexts and configurations; explicitly says reasoning counts toward `max_tokens` for GLM-5.3 and GLM-5.3 Flash. Rounded model-page contexts do not define a maximum billable region for every backend. SHA-256 of retrieved Markdown: `422acfec1ceb57bfb7bda5ef3bb9f3b59679a09869d1f30fd3953e5f9fe240cc`.
- [Reasoning effort](https://docs.tinfoil.sh/guides/reasoning.md): per-model effort values, always-on Kimi/GLM reasoning, separate `reasoning` and `content` fields; Llama does not produce reasoning traces. Does not establish a common final billing-detail contract. SHA-256: `b6795072cfb153a91e722c5e6d6a472f82046e9f06a2539bd52156f62925180a`.
- [Rust SDK](https://docs.tinfoil.sh/sdk/rust-sdk.md) and pinned [origin-bound transport](https://github.com/tinfoilsh/tinfoil-rs/blob/34157e497a747c191852d52af39cfbdb8dbd9eb7/src/verifier/tls.rs): streaming examples and verified raw transport. SDK [SSE parser](https://github.com/tinfoilsh/tinfoil-rs/blob/34157e497a747c191852d52af39cfbdb8dbd9eb7/src/sse.rs) discards `[DONE]` and can include malformed payloads in errors, so the probe uses a separate sanitized bounded observer over that same transport. OpenAI-compatible types/examples are not a billing specification.
- [Prompt caching](https://docs.tinfoil.sh/sdk/prompt-caching.md): cache scope/persistence and cached-token rates. The pinned SDK's raw client bypasses its high-level retry/cache-secret middleware; this probe explicitly supplies the per-request scope, as the buffered production path does.
- [Error handling](https://docs.tinfoil.sh/sdk/error-handling.md): **documented** HTTP 429 for lifetime per-key spend/input/output caps; HTTP 402 for exhausted account/organization balance or inactive subscription. Does not say that the provider reserves the gateway's maximum before generation, nor establish partial-stream refunds. Neither quota exhaustion nor balance deductions were tested live.
- Provider router source snapshot [`9716d140dc70aa6aa2b10ff1a21defbe4d23d442`](https://github.com/tinfoilsh/confidential-model-router/tree/9716d140dc70aa6aa2b10ff1a21defbe4d23d442), inspected read-only: `main.go` requests usage; `tokencount/extractor.go` processes continuous usage, keeps positive token updates, recognizes `[DONE]`, and can invoke its usage handler after partial-stream termination; `manager/proxy.go` constructs billing events from that handler; `manager/pricing.go` prices prompt, cached prompt, and completion tokens; `input_tokens.go` dispatches model-specific `/tokenize`. This supports a **source-level** explanation for multiple usage events and potential operator costs on failures. It is not an invoice observation, a complete financial audit, or proof that this source snapshot equals the measured live router/backend configuration.

### Sanitized live observations — all supported catalog models

Each row is one fixed prompt, not a maximum-size generation or general per-model proof. `I/O/T` are the last observed integer `prompt_tokens/completion_tokens/total_tokens`; `R` is `completion_tokens_details.reasoning_tokens` (absent is not zero). `U` is the count of non-null usage events. `F/Ulast/D` are one-based finish, last-usage, and `[DONE]` data-event positions. All final prompt counts matched the authenticated tokenizer on this input, all final `I+O=T`, all `O<=64`, and every stream reached EOF without a reported transport/provider error. All advertised `text/event-stream` and used LF framing, not CRLF.

| Model | Authenticated context | Tokenizer | Last I/O/T | R | Content/reasoning events | Finish | U | F/Ulast/D | Unknown usage fields | Financial gate |
|---|---:|---:|---|---|---|---|---:|---|---|---|
| `deepseek-v4-1-flash` | 1048576 | 40 | 40/46/86 | 37 | 2/13 | stop | 16 | 15/16/17 | yes | BLOCKED |
| `glm-5-3` | 1048576 | 22 | 22/64/86 | 64 | 0/23 | length | 25 | 24/25/26 | yes | BLOCKED |
| `glm-5-3-flash` | 1048576 | 22 | 22/64/86 | 64 | 0/19 | length | 21 | 20/21/22 | yes | BLOCKED |
| `kimi-k3` | 262144 | 97 | 97/64/161 | 0 | 0/64 | length | 66 | 65/66/67 | yes | BLOCKED |
| `llama3-3-70b` | 131072 | 45 | 45/7/52 | absent | 6/0 | stop | 9 | 8/9/10 | no | BLOCKED |
| `gemma4-31b` | 262144 | 24 | 24/8/32 | absent | 3/0 | stop | 5 | 4/5/6 | no | BLOCKED |
| `gpt-oss-120b` | 131072 | 76 | 76/59/135 | absent | 5/44 | stop | 53 | 52/53/54 | no | BLOCKED |

`prompt_tokens_details.cached_tokens` was zero where present (DeepSeek, both GLMs, Kimi, Gemma), absent for Llama/GPT-OSS. Every JSON data event had non-null usage in these streams. The probe intentionally did **not** retain intermediate usage values or unknown field names/values; cumulative-versus-incremental semantics, their monotonicity, and the meaning of extra fields therefore remain UNKNOWN from this observation. The last event's choices shape was not retained either. Zero visible content on a reasoning-budget exhaustion is not evidence of zero upstream cost. Kimi's reasoning deltas with `R=0`, and GPT-OSS's absent reasoning detail, specifically prevent treating that detail field as a common billing validator.

| Contract question | Evidence class | Result / remaining gap |
|---|---|---|
| `stream`, `include_usage`, `max_tokens` request options | OBSERVED live | Accepted for all seven small requests; omission/false alternatives and maximum-limit enforcement not tested |
| Framing and deltas | OBSERVED live | LF-delimited `data:` JSON; content and/or reasoning deltas counted, no answer retained; arbitrary SSE variants not established |
| Finish and termination ordering | OBSERVED live | Exactly one stop/length finish, later last usage, then `[DONE]`, then EOF in every row; this does not establish error ordering or terminal billing authority |
| Final usage fields/units | OBSERVED integers; provider source DOCUMENTED | Token-named counts as above; relationship to actual provider charge, unknown fields and all reasoning modes remains UNKNOWN |
| Tokenizer versus final prompt count | OBSERVED live | Equality for this one fixed input on seven models only; chat templates, history/boundaries and backend variants remain UNKNOWN |
| Reasoning-inclusive output/context bound | DOCUMENTED for GLM `max_tokens`; otherwise UNKNOWN | Small `O<=64` observations do not prove maximum billable output or common reasoning accounting |
| Failure before output / after partial output / after usage | SIMULATED locally; live UNKNOWN | Local HTTP 402 body rejection, JSON/provider error after partial output, incomplete frame, duplicate terminal and stalled body exercised. No live fault injection, provider refund, or partial-failure invoice evidence |
| Provider balance/quota versus gateway reservation | DOCUMENTED quota statuses; live UNKNOWN | No provider maximum-reservation policy inferred from Possums. Probe costs belong to the operator; it created no user reservation or user charge |
| Planned error refund policy | Planned, NOT IMPLEMENTED by probe | Missing/invalid final usage or upstream error must refund the user's reservation exactly once while operator absorbs possible upstream cost. This is separate from provider billing and from v0.0.5 delivery-based refunds |

### Reservation proof obligation — conditional mathematics, not a passed provider gate

Let `I` be **all billable input tokens**, `O` **all billable output including reasoning**, `C` an authoritative *shared* context bound, `M` a distinct output bound if one exists, and `p,q` the snapshotted, upward-rounded prices in USD microunits per million tokens. With nonnegative prices and no additional fees, the 30%-marked-up user charge is:

`charge(I,O) = ceil(130 * (p*I + q*O) / (100 * 1_000_000))` microunits.

**Only if** the provider establishes `I>=0`, `O>=0`, `I+O<=C`, `O<=M<=C`, with reasoning already inside `O`, the feasible-region vertices are `(0,0)`, `(C,0)`, `(C-M,M)`, `(0,M)`. The maximum numerator is `max(p*C, p*(C-M)+q*M)`; positive rounding is monotone, so round that maximum upward once. For `M=C`, this reduces to `C*max(p,q)`. That is the missing condition behind the existing `max(all-input, all-output)` implementation—not an unconditional proof. If `M<C`, the mixed vertex must be considered. If context instead bounds input alone, the rectangle permits `(C,M)` and the bound is `p*C+q*M`. If hidden reasoning is outside `O` or context, another authoritative bound is required. Tokenizer counts must be related to **billable** input, not merely accepted because a monetary charge happens to fit.

All products, sum, markup multiplication and rounding addition must use checked `u128`; reject on overflow or a result above `u64::MAX`, never saturate/wrap. Existing `src/catalog.rs::marked_up_cost` has those checks and upward fixed-point rounding. Existing tests exercise rounding, context rejection and overflow, but cannot certify provider constraints.

The following **synthetic conditional vectors**, not provider observations, were checked by exhaustive integer enumeration for the five listed envelopes. Prices here are USD per million (`p,q` in code are multiplied by 1,000,000); costs are USD microunits:

| C / M | Input / output price | Boundary or maximum | Result |
|---|---|---|---:|
| 100 / 100 | 2 / 3 | `(0,100)`; full triangle, output-expensive | 390 |
| 100 / 40 | 2 / 3 | `(60,40)`; mixed maximum | 312, exceeding old endpoint-only maximum 260 |
| 100 / 40 | 3 / 2 | `(100,0)`; input-expensive | 390 |
| 100 / 40 | 1 / 1 | all points with `I+O=100` tie | 130 |
| 1 / 1 | 0.000001 / 0.000001 | conservative fractional-microunit rounding | 1 |
| 100 / 100 | 2 / 3 | `(99,1)` context boundary | 262 |
| 100 / 40 | 2 / 3 | `I=60`, visible=10, reasoning=30 inside `O=40` | 312, reasoning must not be omitted or counted twice |
| hypothetical visible-only context 100 | 2 / 3 | `I=60`, visible=40, additional reasoning=30 gives billable `O=70` | 429, exceeding shared-triangle bound 390 |
| u64 maxima | u64-max fixed-point price | markup numerator above `u128::MAX` | must reject; existing overflow regression passes |

The `(100,0)` points are conservative closed-region endpoints; the production request quote currently requires room for at least one output token and rejects input equal to/exceeding context. No claim is made that any model will accept or bill these synthetic pairs.

**Concrete missing authority:** the authenticated catalog adapter consumes `context_window` and prices but supplies `max_output_tokens=context_window` itself. We have not established that shared context bounds all billed tokens, absence of a smaller output cap, or enforcement/meaning of `max_tokens` for every served backend/reasoning variant. The smallest prospective source is a provider-defined authenticated `/v1/models` limit contract (or a versioned provider specification explicitly defining the existing fields) covering shared billable context, distinct maximum output, reasoning inclusion, actual output-limit parameter, and tokenizer/usage relationship. If a separate version-pinned limits mapping is necessary, bind it to authenticated model/backend identities and fail closed on unknown/changed entries; documentation is provider authority, not hardware-attested runtime evidence. Live catalog contents remain outside the gateway measurement. No such mapping or production catalog correction was introduced. Packet 3 must not integrate one until a separate review passes this gate.

### Exact checks and stopping point

Platform: aarch64-darwin, pinned Nix development shell, `rustc 1.88.0 (6b00bc388 2025-06-23)`. Commands run at the probe revision above with only the preserved pre-existing dirty documentation/untracked paths:

- `nix develop -c cargo fmt --all -- --check` — PASS.
- `nix develop -c cargo clippy --all-targets --all-features -- -D warnings` — PASS after correcting a test peer's unused read count; the initial clippy attempt found that lint, not a provider issue.
- `nix develop -c cargo test --lib stream_probe_tests` — PASS, 4 diagnostic tests (split UTF-8/CRLF across every two-part split, frame/total bounds, malformed/hostile events and sanitized errors, partial-output error, HTTP error-body rejection, incomplete frames, EOF and stalls).
- `nix develop -c cargo test --test inference --test catalog --test live_tinfoil` — PASS, 3 existing buffered collector tests plus 6 catalog arithmetic/schema tests; both funded live tests ignored. These are not production streaming tests.
- `TINFOIL_STREAM_PROBE_FUNDED=yes nix develop -c cargo test --test live_tinfoil streaming_contract_probe_all_catalog_models -- --ignored --exact --nocapture` — **FAILED**, exit 101: seven completed diagnostic streams, candidate single-usage-event check contradicted on all seven; unknown usage fields additionally flagged on four. No second run/retry, buffered live canary, or fault injection performed. Failure is recorded, not relabeled as a successful financial gate.
- `git diff --check` — PASS.

Exact arithmetic-only check (system Python, not a provider request or pinned-Rust test):

`python3 -c 'from itertools import product; cost=lambda i,o,p,q:(130*(i*p+o*q)+100000000-1)//100000000; vectors=[(100,100,2000000,3000000,390),(100,40,2000000,3000000,312),(100,40,3000000,2000000,390),(100,40,1000000,1000000,130),(1,1,1,1,1)]; assert all(max(cost(i,o,p,q) for i,o in product(range(c+1),range(m+1)) if i+o<=c)==expected for c,m,p,q,expected in vectors); assert cost(60,40,2000000,3000000)==312>max(cost(100,0,2000000,3000000),cost(0,40,2000000,3000000))==260; assert cost(99,1,2000000,3000000)==262; assert cost(60,70,2000000,3000000)==429>390; assert ((2**64-1)**2)*130>2**128-1; print("5 exhaustive conditional envelopes, mixed-extremum/context/reasoning counterexamples and overflow vector: PASS (not a provider proof)")'`

Result: PASS for the stated conditional envelopes/counterexamples only. **Required to unblock:** provider-authoritative final versus continuous usage/error billing semantics (including extra fields and Kimi/GPT-OSS reasoning representation), general tokenizer-to-billable-usage rules and authoritative maximum constraints for every model/backend; then targeted sanitized live/error observations and a reviewed reservation proof. No downstream implementation, browser/model-reset work, release, or deployment verification is implied.
