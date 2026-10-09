# Synthetic telemetry schema and release oracles — packet 2A

**Historical packet contract with subsequent implementation updates.** The operator's **2026-10-09** [five-minute MVP decision](telemetry-mvp-five-minute.md) supersedes the minimum-ten and linked-sparsity rules/oracles below. The current contract permits sparse counts/bins in complete valid five-minute aggregates; it provides no anonymity or differential privacy. Retained metric names, dimensions, bucket boundaries, transport/ownership and complete-window integrity requirements remain applicable unless explicitly superseded. One-minute process CPU/RSS has the same disclosed correlation limitation. Historical packet gates and passes are not retrospective deployment evidence; current local/live status is in [verification](verification.md) and the [MVP rollout record](telemetry-mvp-five-minute.md).

Policy: [permitted signals / forbidden data](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data), [aggregation](../PRIVACY.md#aggregation-is-necessary-not-sufficient), [processors / shutdown](../PRIVACY.md#processors-retention-access-and-shutdown), [synthetic exception](../PRIVACY.md#synthetic-testing-exception), [required evidence](../PRIVACY.md#required-evidence-for-every-telemetry-change). Read the [plan](telemetry-plan.md), [accepted packet-1 evidence and blockers](verification.md#2026-10-07--telemetry-packet-1-capability-evidence-and-export-blockers) and [current release scope](phase02-pi-release.md). Historical buffered/delivery-owned charging is **not** this contract's generation population. Existing invoice, runtime/privacy and availability unknowns are unchanged.

## Source and ownership contract

The [existing telemetry scaffold](../src/telemetry.rs) has four unwired lifetime counters and independent threshold filtering. [Existing privacy tests](../tests/privacy.rs) test that scaffold, startup silence and rendering, not these windows or payloads. Neither is safe export evidence. No source change occurs here.

| Observation | Sole boundary / owner to instrument later | Exclusions and invariants |
| --- | --- | --- |
| HTTP start | Outermost application request lifetime in [web.rs](../src/web.rs), before admission/header/body middleware | One per request presented to the application, including duplicates, wrong routes/methods and early middleware rejection. Not one per TCP connection, SSE event or retry. Never inspect a body for labels. |
| HTTP terminal / duration | Same lifetime guard through handler cancellation, empty/end-stream body, body error, EOF or drop | Exactly once, elapsed monotonic start-to-terminal. Status is the selected response status if known, otherwise `unknown`. EOF/empty body is `eof`, an observed body error is `error`, cancellation/unclassified drop is `unknown`. HTTP 200 is not generation success or proof of receipt. Frames retained after EOF can retain occupancy, not extend HTTP duration. |
| Generation start | [generation.rs](../src/generation.rs): immediately after a **new** successful reservation, with [ReservedGeneration](../src/generation_owner.rs), no cancellation gap | Includes detached tokenizer/context preflight. Duplicate reserve is HTTP traffic only. Pre-reservation failures do not start generations. Telemetry owns neither reservation nor permit. |
| Generation terminal / duration | The existing `Terminal::finish` attempt, after accounting returns and releases its lock, including armed guard drop | Exactly one observation, regardless of supervisor/receiver/body disappearance. Preserve guard/drop order, sealed receipt and leases. No extra ledger attempt. Success requires validated upstream completion **and** observed settled result; refund alone is not an error cause. Ledger error never proves refund. |
| Dispatch / first output | [inference.rs](../src/inference.rs) text **and** invocation paths: just before actual generation `request.send()`, after request encoding and verified transport acquisition; first qualifying validated upstream delta | Not renderer entry, tokenizer send, HTTP headers, role-only delta, usage or finish. First nonempty text/reasoning or nonempty tool-call presence qualifies, once; do not pass text, tool names/arguments/IDs to telemetry. This is dispatch-to-first-output, **not tokenizer TTFT**. Store only its bucket index until terminal. |
| Delivery terminal | [stream_owner.rs](../src/stream_owner.rs), with explicit producer disposition from [streaming_chat.rs](../src/streaming_chat.rs) and [api_stream.rs](../src/api_stream.rs) | One per created streaming delivery body, not every HTTP response. `completed` needs explicit normal producer finish and consumer EOF; `interrupted` needs observed producer detachment/failure or consumer abandonment before normal completion; accidental channel close/drop without identified cause is `unknown`. Queue drain after detachment is not completed. A fully delivered error response may be completed delivery and failed generation. |
| Rejection | Actual fail-fast boundary in listener, middleware, auth/reservation admission | No queue-length fiction; no rejection inferred from HTTP status. Duplicate is not rejection. Context/preflight failure after reserve is generation failure, not admission rejection. |
| Occupancy | Actual shared permit lifetime in listener, request admission, ingress and worker owners | Count a permit once, not Arc clones. Include detached work, serialized uploads and retained body frames/slices until the final real owner releases it. Generation slot and heavy memory-admission lease are different. No telemetry I/O, blocking or await under auth/accounting locks. |

[main.rs](../src/main.rs) installs a content-suppressing panic hook and starts this server; it does not wire telemetry. [api.rs](../src/api.rs) currently merges six API routes. In `api_stream.rs`, preserve the original closed failure **before** its generic `InvalidResponse` settlement conversion. `Terminal::Drop` cannot determine why it ran: do not label it panic/cancel/failure merely from `finish(None)`. Later wiring may propagate a known preflight cause but cannot invent one from observer loss. Success is never inferred from a receipt being delivered.

Grounding tests inspected (not rerun here): [catalog](../tests/catalog.rs), [API streaming](../tests/api_chat.rs), [generation ownership](../src/generation_owner_tests.rs), [streaming composition](../src/streaming_chat_tests.rs), [delivery ownership](../tests/stream_owner.rs), [transport](../tests/transport.rs). In particular, detached work settles after disconnect, conflicting accounting terminals are absorbing, and retained slices outlive body EOF. These tests require new telemetry assertions later; their old passes do not verify this schema.

## Closed label maps

Runtime resource attributes are exactly `service.name="possums-gateway"`, `deployment.environment.name="production"`, `possums.gateway.slot="gateway-01"`. Historical test-owned wire fixtures below use `test`; a Honeycomb `dev` destination never changes the production source label. **S = 1 slot**: `gateway-01` is bound to the operator's single shared gateway process in `tinfoil-config.yml`, never a customer, account, hostname, address or provider resource ID. Do not run additional configured exporters under the same slot or infer mappings for native Tinfoil/cgroup resources. No raw inventory, restart UUID or version/digest attributes are exported.

Scope is exactly name `possums.telemetry`, empty version, no attributes and empty schema URL; resource schema URL is empty. No automatic SDK resource discovery. Other names/keys/values fail closed, never stringify into labels. Datapoint keys below are exactly `possums.endpoint`, `possums.model`, `status_class`, `http_terminal`, `outcome`, `failure_stage`, `reason`, `lane`, `source`, `scope`; fallback alone adds `bucket`. Only each table's listed subset is allowed. No HTTP method label.

### Endpoint map

**E = 16 application endpoints**, plus `not_applicable` solely for pre-router rejection. Match the exact method and URI path, without decoding, case folding, slash normalization or query labels. Query text never enters telemetry. Web query strings do not change path mapping. API queries are rejected by current middleware but keep their method/path endpoint. API HEAD is rejected and maps to `other`; web HEAD follows Axum GET behavior as explicitly listed.

| Exact method(s) | Exact path | `possums.endpoint` |
| --- | --- | --- |
| GET, HEAD | `/` | `home` |
| POST | `/login` | `login` |
| POST | `/logout` | `logout` |
| POST | `/chat` | `chat_web` |
| POST | `/chat/new` | `new_chat` |
| GET, HEAD | `/recovery` | `recovery` |
| GET, HEAD | `/recovery/download` | `recovery_download` |
| GET, HEAD | `/claims` | `claims` |
| GET, HEAD | `/attestation` | `attestation` |
| GET | `/v1/auth/challenge` | `api_challenge` |
| POST | `/v1/sessions` | `api_session` |
| POST | `/v1/submissions` | `api_submission` |
| DELETE | `/v1/sessions/current` | `api_logout` |
| GET | `/v1/models` | `models` |
| POST | `/v1/chat/completions` | `chat_api` |
| All other method/path pairs | No raw path retained | `other` |

Thus POST `/chat/`, POST `/%63hat`, GET `/chat`, HEAD `/v1/models`, OPTIONS `/`, and `/v1` map to `other`. Pre-router connection/header failures have no invented endpoint/model/status; their rejection uses `not_applicable` for endpoint and model. No new routes or public metrics endpoint.

### Model map

Author-selected named map: exact catalog ID `kimi-k3` → `kimi-k3`; exact ID `glm-5-3` → `glm-5-3`. These names have historical source/release evidence, not new live catalog qualification. This deliberately small map is complete for this pass; all other valid supported models remain usable.

`authenticated_catalog` uses the verified origin-bound client followed by [Catalog::parse_authenticated](../src/catalog.rs); validation bounds bytes to 256 KiB, catalog entries to 256, safe IDs to 128 bytes, validates chat endpoint/type, uniqueness, positive context/prices and quote arithmetic. **Only a successful selected-model lookup/representable reservation quote in that authenticated validated catalog may intersect the named map.** Merely matching a submitted string is insufficient. A syntactically valid but absent ID is `unknown`, not `other`. Catalog/trust failure gives no named label. Catalog content remains outside gateway measurement.

Closed model values **M_all = 5**: `kimi-k3`, `glm-5-3`, `other` (validated, unmapped), `unknown` (unvalidated/unresolved selection), `not_applicable` (no model selection). Generation and delivery use only **M = 3** validated values, snapshotted at new reservation; later catalog changes cannot relabel them. Admission rejection may use all five. HTTP tables intentionally have **no model dimension**: model is unavailable at HTTP start and must not be read from hostile bodies. Model reliability/latency are answered by generation tables, not relabelled HTTP starts. Model names, slot values or enums cannot be extended by runtime strings.

### Status, outcomes and rejection maps

- `status_class`: `1xx`, `2xx`, `3xx`, `4xx`, `5xx`, `unknown` (**6**); determined only from known response status. `http_terminal`: `eof`, `error`, `unknown` (**3**).
- Generation `(outcome, failure_stage)` has exactly **G = 12 valid pairs**: `(success, none)`; `(failure, admission|verification|catalog|encoding|transport|decoding|validation|terminal_usage|settlement|internal)` (ten pairs); `(unknown, unknown)`. Other pairings are invalid. These are operational outcomes, **not exported billing receipts**.
- Precedence: failed/inconsistent ledger attempt (including returned InFlight or a settled result without validated completion) → `(failure, settlement)`; otherwise observed closed generation failure → its failure pair; otherwise validated completion plus `Settled` → `(success, none)`; otherwise unidentified drop/absence of cause → `(unknown, unknown)`. Successful adapter result with unexpected refund is `(failure, settlement)`, not a fabricated charge or claimed upstream error. An identified preflight failure can be `failure` after the actual attempt; an unclassified refunded drop remains `unknown`. Conflicting/duplicate terminal calls add nothing and never change an earlier observation.
- Closed stage mapping from [InferenceFailure](../src/inference/errors.rs): `VerificationFailed`, `EndpointBindingFailed` → verification; `CatalogFailed` → catalog; `RequestEncodingFailed` → encoding; `TokenizerSendFailed`, `TokenizerHttpFailed`, `TokenizerUploadIncomplete`, `GenerationSendFailed`, `GenerationHttpFailed`, `StreamTransportFailed`, `StreamIdleTimeout`, `StreamDeadlineExceeded`, `UpstreamErrorEvent` → transport; `SdkStreamDecodeFailed` → decoding; `StreamFinishMissing`, `StreamUsageMissing`, `StreamUsageInvalid`, `StreamUsageUnexpected` → terminal_usage; `SettlementFailed` → settlement; `ToolProfileUnqualified`, `TokenizerResponseInvalid`, `StreamContentTypeInvalid`, `StreamEventSchemaInvalid`, `StreamChoiceInvalid`, `StreamDeltaUnsupported`, `ToolIndexInvalid`, `ToolIdentityInvalid`, `ToolNameNotAllowed`, `ToolChoiceViolated`, `ToolCallIncomplete`, `ToolArgumentsTooLarge`, `StreamFinishInvalid`, `StreamOutputAfterFinish`, `UpstreamResponseInvalid` → validation; `InferenceUnavailable` → internal (generic observed error, no invented transport cause). Known post-reserve context rejection → admission; explicit preflight deadline/startup failure → internal. Do not export the detailed code or message.
- Delivery `outcome`: `completed`, `interrupted`, `unknown` (**3**). It has no billing or generation-failure dimension.
- Rejection `reason` (**R = 14**): `connection_capacity`, `header_protocol`, `connection_deadline`, `transport_unknown`, `request_capacity`, `ingress_capacity`, `generation_capacity`, `account_limit`, `credit`, `auth`, `input`, `verification`, `catalog`, `internal`. First four only apply before the first application request on a connection: exhausted connection semaphore; typed header/protocol failure; observed pre-request deadline; other unidentified pre-request transport termination respectively. Clean idle close with no identified rejection emits nothing. Do not parse raw Hyper errors to improve classification. After application entry, body/header/shape/wrong-method rejection is `input`; actual request/ingress/generation permit failures map to corresponding capacity values; accounting concurrency → account_limit; insufficient credit → credit; failed auth/session/CSRF/submission binding → auth; trust → verification; catalog authentication/validation → catalog; selected model absent/invalid → input with model unknown; other actual admission failure → internal. Ordinary route-not-found traffic has HTTP status only. Duplicates and post-reserve failures are excluded. Only the first actual rejection per attempt is counted.

Unknown values received by the **telemetry API** are sanitizer failures, not dynamic `unknown` strings: withhold the affected request family (or infrastructure point). The closed `unknown` enums above mean an explicitly observed inability to classify an allowed lifecycle, not a permissive escape hatch.

## Exported tables and encoding

All names are exact. Request timestamps are the enclosing fixed UTC-grid five-minute interval `[start,end)` boundaries, never event timestamps. All counts are nonnegative integer **delta sums**, temporality DELTA, monotonic true, with no cumulative snapshot. Histograms are DELTA explicit non-cumulative bucket populations plus count. No exemplars, trace/span IDs, samples, sum, min, max, flags carrying additional data, descriptions or unapproved fields. Distribution count equals sum of bins. Dimensions implicitly include the fixed resource/slot above; no extra marginal-total instruments.

| Name | Operational question / population | Unit; type | Datapoint dimensions |
| --- | --- | --- | --- |
| `possums.http.requests` | How many application HTTP lifetimes began? | `{request}`; delta sum | endpoint |
| `possums.http.completed` | How did HTTP lifetimes end, including selected status versus body error/drop? | `{request}`; delta sum | endpoint, status_class, http_terminal |
| `possums.http.duration` | HTTP service/body lifetime by terminal category, not upstream speed | `s`; histogram | endpoint, status_class, http_terminal |
| `possums.generation.started` | How many newly reserved generations, including detached preflight? | `{generation}`; delta sum | endpoint, model |
| `possums.generation.completed` | How many sole-owner terminal attempts and known failures? | `{generation}`; delta sum | endpoint, model, outcome, failure_stage |
| `possums.generation.duration` | Reservation-to-terminal-attempt duration by terminal outcome | `s`; histogram | endpoint, model, outcome, failure_stage |
| `possums.generation.first_output` | Dispatch-to-first qualifying output among terminal generations that produced output | `s`; histogram | endpoint, model, outcome, failure_stage |
| `possums.delivery.completed` | How did streaming downstream delivery terminate? | `{delivery}`; delta sum | endpoint, model, outcome |
| `possums.admission.rejected` | Where did an attempt fail before reserve/application entry? | `{rejection}`; delta sum | endpoint, model, reason |
| `possums.admission.occupancy` | How often were actual shared permits occupied? | `{permit}`; histogram | lane |

Generation/delivery endpoint is only `chat_web` or `chat_api` (**2**). No separate success count/status table: those are partitions of completed. Query rates sum approved cells locally, not additional exported totals; HTTP server-error share uses 5xx / HTTP terminals in the same window, with unknown/error/drop shown separately. Generation failure share uses failure / all generation terminals with unknown explicit. **Never completions divided by same-window starts.** Do not add HTTP/delivery/generation failure counts as disjoint incidents.

### Histograms and fallback

All three duration distributions use finite upper bounds in seconds **[0.1, 0.5, 1, 5, 15, 30, 60, 120, 300, 600]**, plus overflow: **B = 11** disjoint bins. Bin 0 is `[0,0.1]`, bin 1 `(0.1,0.5]`, and so on, bin 10 `(600,+infinity)`. Nonfinite/negative/overflowed durations invalidate the family. Exactly-at-boundary samples belong to the lower bin; 0 is valid only for an actually measured zero duration, never missing output. Quantize at observation and discard elapsed scalar; only monotonic start/dispatch instants needed for a live lifecycle remain local, never request wall timestamps or stored duration samples. Keep **one optional first-output bucket**, not exact first-output time or a list of deltas.

Occupancy: lane enum **L = 6** is `connection`, `generation`, `heavy`, `ingress`, `new_chat`, `control`. Schedule occupancy sampling at each one-second midpoint in a complete request window (300 samples/lane). The first actual poll at or after that midpoint and before the next midpoint records the current occupied permits once. The final sample must precede window closure. Timer jitter does not create synthetic midpoint readings; duplicate polls do not resample, and a wholly missed sampling period invalidates the window. Finite upper bounds **[0,1,2,3,4,8,16,32,64]**, overflow (**O = 10** bins); native integer counts at exact bounds are inclusive. Occupancy above configured capacity invalidates the family. A missed tick invalidates the whole request family; never duplicate/interpolate samples. Do not export a lane with no permit held at any time in the window, even though its internal idle samples are zero. For a touched lane, zero-occupancy bins may be published only with the released full distribution. No model-specific/global-overlapping occupancy tables: this pass measures shared lane saturation only, not per-model capacity.

Internally count distinct **permit lifetimes intersecting each window**, once per lease using a bounded window marker, including old-epoch leases; no request IDs or history. A briefly held lease counts even if between sample ticks. This gate-only contributor count is not exported. It is not a user count/privacy guarantee. One long-held lease must not manufacture ten contributors from ten ticks.

Preferred wire form is the supported Rust-1.88-compatible OTel SDK/export representation with **absent** histogram sum/min/max (not zero values). Packet 2C must choose/pin and inspect actual supported serialization/defaults. This packet does not claim any SDK can already do this and adds no dependency or custom OTLP adapter.

Deterministic fallback, if the supported histogram representation cannot omit prohibited fields: replace each histogram name by exactly `<name>.bucket` (four names: `possums.http.duration.bucket`, `possums.generation.duration.bucket`, `possums.generation.first_output.bucket`, `possums.admission.occupancy.bucket`). Each is an integer monotonic DELTA sum, unit `{observation}`, same dimensions plus `bucket`. Duration bucket values are `b00` through `b10`; occupancy `b00` through `b09`, in the order above. Emit the full bin vector for a released populated distribution, including true zeros; no separate `.count` or native histogram alongside it. Sum bins to derive the distribution count. This is a bounded table, not one recording per request. Fallback cannot support native Honeycomb histogram percentiles; only locate a percentile's bin from cumulative populations (no exact percentile, mean, interpolation within bin or finite upper estimate for overflow). A deployment selects one encoding for the schema, never both or per-window switching. Native histogram percentiles also remain bucket-resolution estimates, subject to later backend verification.

### Separately windowed infrastructure

These **20 maximum gauge series** use independent complete **one-minute** windows, no endpoint/model/outcome and no request-family threshold. Their finer cadence offers **no protection from correlation with requests**. Process CPU/RSS is live; on 2026-10-08 the operator additionally approved the six configured admission-capacity gauges for the next verified production release. Other resource scopes remain unavailable. Use only numeric finite nonnegative values, no provider IDs/errors. Packet 1 left all live CPU/memory sources unavailable; later `c93cf87` implemented gateway-process CPU/RSS with local Darwin tests. The subsequent v0.0.12 Linux process ingestion evidence is recorded separately in [verification](verification.md), not retroactively attributed to that local packet. Configured production export uses the reviewed MVP scope; deployment and live ingestion still require verification. Native Tinfoil and cgroup sources/capacities remain unavailable; fixtures do not establish their access, units, scope, cadence, lag or CVM compatibility. Missing authority is not a demonstrated gap justifying a new collector.

| Name | Question / unit / type | Permitted `(source, scope)` pairs |
| --- | --- | --- |
| `possums.resource.cpu.used` | Mean CPU cores used; `{cpu}`; gauge | `(tinfoil_native, enclave)`, `(tinfoil_native, workload)`, `(process, process)`, `(cgroup, cgroup)` — 4 |
| `possums.resource.memory.used` | Mean used bytes in the named accounting scope; `By`; gauge | Same four pairs — 4 |
| `possums.resource.cpu.capacity` | Verified allocated/quota cores, not guessed usage denominator; `{cpu}`; gauge | `(tinfoil_native, enclave)`, `(tinfoil_native, workload)`, `(cgroup, cgroup)` — 3 |
| `possums.resource.memory.capacity` | Verified finite memory capacity/limit; `By`; gauge | Same three pairs — 3 |
| `possums.admission.capacity` | Configured shared admission permits; `{permit}`; gauge | source=`configuration`, scope=`gateway`, lane one of six — 6 |

For resource tables, dimensions are **source, scope only**; capacity adds lane only for `possums.admission.capacity`. The paired whitelist is not a Cartesian permission to mix sources/scopes. Process RSS, charged cgroup memory and enclave/workload usage are not aliases; no model GPU metrics. Native used cores require verified percentage scale/denominator; CPU-seconds deltas / monotonic elapsed seconds already give cores and must not be divided twice. Unit conversion needs source evidence (MB versus MiB is not guessed). Unknown/unlimited capacities are absent; no utilization percentage with an unavailable denominator. `2 CPUs / 8192 MB` in enclave configuration is neither measured gateway capacity nor gateway cgroup limit. The 512-MiB allocation target is not RSS capacity.

Configured lane capacities in the current code are: connection **64**, generation **4**, heavy **4**, ingress **4**, new_chat **1**, control **1**. For the approved live source, derive them from the same constants used to construct the server admission semaphores, never from available-permit counts or the telemetry validator's expected-value table. Observe all six on each accepted ten-second sampler tick and insert them alongside CPU/RSS in the same infrastructure batch, before collection. Six valid intervals release the fixed-minute capacity independently of sparse request traffic or timely failed process readings; missed or late sampling remains unavailable. A mismatch against the reviewed schema capacity disables that point. These are configured application limits, not runtime CPU/memory enforcement. No per-model lane capacity or invented queue.

MVP process source: read once at each ten-second boundary or within the following inclusive one-second grace. CPU cores used are checked delta process CPU-seconds divided by actual elapsed monotonic seconds between consecutive readings, not a guessed exact ten seconds or a machine-normalized percentage. Memory readings are process RSS bytes. The published minute value is the arithmetic mean of six valid interval rates/readings. Duplicate polls cannot reread/revise; missed, failed or late intervals are unavailable, never backfilled. Reads happen outside the aggregation lock; CPU/RSS enter together before the sender's boundary collection. Request observations defer resource closure during grace. The process source publishes no capacity/limit, PID, command, path or source error. Linux uses numeric statm, not smaps; its RSS is approximate. Native/resource fixtures below retain their stated input semantics.

Normalization fixture: six complete consecutive ten-second source intervals per minute, CPU core averages `[1,1,1,1,1,1]`, corresponding memory readings `[1048576,1048576,1048576,1048576,1048576,1048576]`, constant verified capacities 2 cores and 8388608 bytes → at minute end cpu.used=1, memory.used=1048576, cpu.capacity=2, memory.capacity=8388608 for **one** chosen valid source/scope pair. All six application capacities above may appear separately. No alternative source/scope pair is filled. One missing/stale/revised interval → affected resource series absent, not a zero or replacement. A changing capacity during the minute → capacity absent. CPU uses six equal ten-second interval averages; memory uses readings at interval ends; their arithmetic mean is the defined gauge. No max/min or raw readings exported. Live sources must establish this exact alignment/cadence or remain unavailable; do not split a native five-minute average into minutes. Gauges carry end-boundary timestamp only; interval semantics are fixed here, not fabricated OTLP gauge start times.

## Linked release decision

**Historical minimum-ten policy, superseded by explicit operator approval on 2026-10-09.** See [the current contract](telemetry-mvp-five-minute.md#current-contract): complete-window and lifecycle/arithmetic consistency validation remains, but sparse cells/bins/contributor counts/complements no longer withhold a valid family. The vectors and threshold-specific predicates below describe the earlier contract, not the current release gate.

One all-or-none decision for **all ten request tables per slot and fixed five-minute window**. No separate endpoint, model, outcome, occupancy or rejection release; no overlapping marginal counters, suppression counts or withheld/idle-health zeros. Infrastructure is separate as above.

1. Validate epoch/clock/completeness, enum membership, integer arithmetic and bounded state. Any lost eligible observation, overflow, pool exhaustion, sanitizer failure or contention that prevents consistent family updates invalidates that entire request window. Intentionally ineligible old/off/warmup contexts are not lost eligible observations. Inference proceeds unchanged. Do not export a partially updated table.
2. Every populated scalar atomic cell and every populated histogram-bin population must be **at least 10**. This includes occupancy bins, all terminal categories, rejection reasons and singleton `other`/`unknown` partitions. Zero bins are not threshold failures.
3. Gate complements as well: per generation terminal cell, `missing_first_output = completed - sum(first_output bins)`; it must be 0 or >=10. All other histogram sums must equal their matched terminal counts (HTTP duration and generation duration). No first-output observation without dispatch; absent output adds no zero-duration sample. On an outcome cell with no output the first-output distribution is absent, not an all-zero histogram.
4. Gate-only HTTP terminal disposition partitions are `new_generation`, `duplicate`, `pre_reservation_rejected`, `control_or_other`, `unknown`, per endpoint/status/http_terminal cell. Each count must be 0 or >=10 and their sum must equal HTTP completed. They identify, for example, the nine-generation/one-duplicate complement hidden inside ten HTTP completions without exporting another total. They record the observed disposition at terminal, even if a generation's start belongs to another window; no subtraction of unrelated start/terminal cohorts. Unknown disposition does not imply a rejection. A control endpoint that fails before starting a generation is still control_or_other unless an actual rejection was observed. Pre-router rejection has no HTTP complement.
5. Each touched occupancy lane must have >=10 contributing permit lifetimes in that window and all populated sample bins >=10. Inactive lanes are absent. Old leases can contribute only actual occupancy, not starts/terminals resurrected from an old epoch. No user counting or identifiers.
6. Sums/complements of these disjoint partition atoms cannot introduce a positive value smaller than 10 if the atoms pass. The scope is **these defined complements**, not a proof against cross-window linkage, cross-population joins, bucket-margin intersections, repeated-user or external correlation attacks. Any failure suppresses the **entire** family, including otherwise common cells and starts. If there is no activity/touched occupancy, output is empty. A successfully released family contains only active scalar cells and populated distributions (all bins within each such distribution). Never publish a standalone inactive zero.

Starts are attributed at observation. Completions, terminal-duration and retained first-output buckets are attributed to **terminal window and terminal outcome**, even if dispatch/output occurred earlier. Delivery has its own terminal window. No reopening prior windows or revising their totals. In-flight generations are not missing completions or refunds. Same-window starts and completions describe different cohorts, so there is no starts-minus-completions privacy guard or success denominator.

## Exact request vectors

The notation below is an exact expansion, not a scenario name. All unlisted series are **absent**; infrastructure is absent unless separately supplied. Resource slot is gateway-01. Vectors use native histograms; fallback substitutes the four `.bucket` names and bNN points without changing populations. `D_i(n)` is the full **11-element** duration vector with n at index i and zero elsewhere; `P(z,a)` is the full **10-element** occupancy vector `[z,a,0,0,0,0,0,0,0,0]` (idle/one permit). Count is the sum of each vector. Gate-only counts below are never exported.

`C(n,e,m,o,s)` expands to exactly:

| Series / fixed dimensions beyond e,m,o,s | Exact output |
| --- | --- |
| http.requests(e) | n |
| http.completed(e,2xx,eof) | n |
| http.duration(e,2xx,eof) | D_3(n) (3-second HTTP lifetime) |
| generation.started(e,m) | n |
| generation.completed(e,m,o,s) | n |
| generation.duration(e,m,o,s) | D_2(n) (1-second reservation lifetime) |
| generation.first_output(e,m,o,s) | D_1(n) (0.2-second dispatch-to-output) |
| delivery.completed(e,m,completed) | n |

The `possums.` prefix is omitted only in this oracle notation. `Q(n)` adds four occupancy histograms: connection=P(300-3n,3n), heavy=P(300-3n,3n), ingress=P(300-n,n), generation=P(300-n,n); new_chat/control absent. Each touched lane's contributor count is n. HTTP gate-only new_generation disposition is n; all other dispositions zero. No rejection.

A concrete complete cohort input producing C+Q: for j=0..n-1, within the same eligible window, HTTP/connection/heavy start at 10j seconds, ingress holds `[10j,10j+1)`, new reservation and dispatch at 10j+1, first qualifying output at 10j+1.2, terminal ledger attempt at 10j+2, explicit normal producer finish and consumer EOF/HTTP/connection/heavy end at 10j+3. Generation lease ends at 10j+2. All selected statuses 200; model authenticated; terminal pair supplied as below. The midpoint occupancy sampler produces Q. These are synthetic input times, never exported timestamps. Identical timing with a normally delivered error ending at +3 can use a failure pair without changing HTTP 200 or delivery completed.

| ID | Concrete input | Exact expected request output |
| --- | --- | --- |
| V0 | No observations; all lanes idle for 300 ticks | Empty |
| V1 | C+Q input, n=1, e=chat_api, m=kimi-k3, pair success/none | Empty (not even starts/occupancy) |
| V9 | Same, n=9 | Empty |
| V10 | Same, n=10 | C(10,chat_api,kimi-k3,success,none) + Q(10): durations D3(10), D2(10), D1(10); connection/heavy P(270,30), ingress/generation P(290,10) |
| V11 | Same, n=11 | C(11,chat_api,kimi-k3,success,none) + Q(11): D3(11), D2(11), D1(11); connection/heavy P(267,33), ingress/generation P(289,11) |
| Vrare-error | V10 plus one same-timed later cohort member at j=10 with pair failure/transport | Empty, including common success/HTTP totals and occupancy |
| Vrare-model | V10 plus one member j=10 with authenticated glm-5-3 instead | Empty |
| Vrare-endpoint | V10 plus one member j=10 with chat_web instead | Empty |
| Vrare-bin | V11 but last first output at dispatch+0.6 (D2 instead of D1), terminals unchanged | Empty (first-output bins 10 and 1) |
| Vmixed | n=20: j=0..9 success/none, j=10..19 failure/transport; all have output at dispatch+0.2, ledger attempt at +2, normal error/success delivery at +3 | http.requests=20; http.completed(2xx,eof)=20; http.duration=D3(20); generation.started=20; two generation.completed cells=10 each, duration=D2(10) and first_output=D1(10) in **each** terminal pair; delivery.completed(completed)=20; Q(20): P(240,60) connection/heavy and P(280,20) ingress/generation. Exactly 15 native datapoints, no rejection |
| Vmissing-rare | n=11 successes, one has no qualifying output (otherwise same timeline) | Empty: terminal=11, first-output=10, missing complement=1 |
| Vmissing-release | n=20 successes, j=0..9 have output, j=10..19 have none; valid terminal usage for all | C(20,chat_api,kimi-k3,success,none), except first_output=D1(10), plus Q(20); missing complement=10 (not exported) |
| Vmissing-all | n=10 successes, no qualifying output, otherwise V10 | C(10,chat_api,kimi-k3,success,none) **without first_output**, plus Q(10); missing complement=10 |
| Voutput-failure | n=10 with output then StreamUsageMissing; observed refund, complete error delivery | C(10,chat_api,kimi-k3,failure,terminal_usage)+Q(10). No success cell; no billing/refund metric |
| Vduplicate-complement | V10 but one HTTP terminal is an observed duplicate instead of a new generation; only nine generation and streaming delivery starts/terminals | Empty (also gate-only HTTP disposition 9/1). Never invent a tenth generation |
| Vreject | Ten pre-router connection-capacity rejections, no leases/HTTP requests | Only admission.rejected(not_applicable,not_applicable,connection_capacity)=10 |
| Vreject-rare | Vreject plus one such rejection | Same sole series=11 |
| Vunknown | Ten new generations with unidentified armed-guard drop, no dispatch/output; generation starts at t=1, drop at t=2; no HTTP/delivery observations in this aggregator fixture | generation.started(chat_api,kimi-k3)=10; generation.completed(unknown,unknown)=10; generation.duration(unknown,unknown)=D2(10). No first_output, success, refund or delivery observation |

Vmixed uses the same chat_api/kimi-k3 dimensions throughout. The five C count tables coalesce shared dimensions; three generation tables separate terminal pairs. Vunknown is a typed aggregator fixture, not permission for real wiring to omit HTTP/occupancy.

**Cross-window oracle Vsplit (typed aggregation fixture):** W=[0,300), W+1=[300,600) relative to a fixed eligible boundary. Ten HTTP and generation starts at t=299, dispatch at t=299, first output at t=299.2; ten generation terminal attempts and normally finished delivery/HTTP terminals at t=301, pair success/none. No occupancy input in this isolated table fixture. W exports **only** http.requests(chat_api)=10 and generation.started(chat_api,kimi-k3)=10. W+1 exports **only** http.completed(2xx,eof)=10; http.duration=D3(10) (2 seconds); generation.completed(success,none)=10; generation.duration=D3(10); generation.first_output=D1(10); delivery.completed(completed)=10. HTTP disposition is ten new_generation in W+1. Starts in W+1 are absent, not zero. First output is not exported in W. Recollecting W emits nothing. Vsplit-failure changes the terminal pair to failure/terminal_usage with identical buckets, still no first-output success series.

Ten simultaneous cross-boundary generations exceed the serving global-four limit: Vsplit/Vunknown are **aggregation input oracles**, not claimed reachable server loads or reasons to relax admission. Later real-owner tests use <=4 overlapping generations and expect suppression unless a whole eligible window otherwise satisfies every populated cell. The independent HTTP boundary fixture can use ten connections within the actual 64 cap. Actual wired tests must include all occupancy and rejection observations; they cannot silently reuse table-only fixtures to claim lifecycle integration passed.

## Epoch, time and export lifecycle oracles

These are acceptance requirements, not implemented behavior. Fake-clock time notation: start/re-enable at wall=125 seconds, monotonic=0. Request grid is multiples of 300 Unix seconds; infrastructure grid multiples of 60. Window progression/durations use monotonic time mapped once to the UTC grid, not repeated wall-time subtraction. Boundary equality belongs to the new window. Only grid timestamps leave memory. Check wall/monotonic correspondence before observation, closure and sending.

| Input | Required output / state |
| --- | --- |
| Default/off, or enabled without deployment-controlled isolation (including any real/mixed traffic mode); feed V10 | Nothing: no pending payload, no later lifecycle emission. A client header/field cannot enable isolation. |
| Start at wall=125; V10 wholly inside [125,300) | Nothing. Discard current partial window. First request-eligible window is [300,600), emitted only after 600 if complete; infrastructure first eligible [180,240), emitted only after 240. Enabling exactly on a boundary still discards that current window. |
| V10 wholly inside [300,600), collect at 600 | Exact V10 with start/end boundaries 300/600. Second collection at 600 or 600.5 adds nothing; no replacement. |
| Create HTTP/generation/delivery context at 299 while warming, terminal at 301 or 601 | No later start/terminal/duration/first-output from that context. A generation created from an ineligible HTTP admission context stays ineligible even if reservation occurs after 300. Occupancy is separate actual leases only. |
| Active epoch A context at 310, off at 320, re-enable at 325, terminal at 650 | Invalidate A and clear its windows/pending. [325,600) is discarded; [600,900) can be eligible. A context emits no terminals/output/starts at 650. No accounting/inference cancellation or replay. |
| Ten old connection leases actually held [600,610), idle afterwards, no other activity in [600,900) | Only occupancy(connection)=[290,0,0,0,0,0,10,0,0,0], count=300 (ten samples at occupancy 10 → bin 6 `(8,16]`). Contributor=10. No resurrected HTTP/generation events. |
| One old connection lease held [600,610) instead | Entire family withheld: contributor=1, despite ten occupied samples. |
| Abrupt process death at 450 during [300,600) | No terminal/refund telemetry, no partial flush or persisted backlog. Restart at 460 discards [460,600); earliest request window [600,900). Accounting restart semantics remain separate; telemetry does not replay inference. |

The ten-old-connection vector distinguishes occupancy **value 10** from a ten-sample population at value 1. Old lease state remains bounded with its existing owner; invalidating an epoch does not release permits or allocate replacement lifecycle records.

Clock-discontinuity rule: any sampled backward wall movement, or absolute discrepancy **>1 second** between wall elapsed and monotonic elapsed from the epoch anchor (forward or backward), latches telemetry off, invalidates epoch, drops open/pending windows and cancels in-flight export. Exactly 1 second without backward movement is tolerated; it never moves an established boundary. Monotonic regression/overflow is also fail closed. The aggregation controller remains invalidated until re-enabled. The configured MVP sender automatically re-enables with a new anchor and fresh complete windows; it never catches up or replays discarded batches. Deployment-level disablement still requires process configuration/restart and does not auto-enable. Retain in-memory attempted-window end watermarks through off/on; eligibility must start at or after the respective watermark. Thus stepping wall from 610 to 100 cannot reopen [300,600); stepping 610 to 1210 cannot create four missing windows. At 610 with expected wall 610: actual 608 (backward) or 612 (>1 second ahead) → zero new sends and discard. Already accepted bytes cannot be recalled.

In-process scheduling: a required midpoint-to-next-midpoint occupancy sampling period wholly missed, a final sample not taken before closure, or closure first processed **more than one second** after its boundary discards the affected request window. A six-sample infrastructure point with a missing interval is unavailable. On any skipped whole window, invalidate the epoch (including old lifecycle contexts), cancel its unsent/in-flight export, discard stale/partial state, and automatically re-enter warmup until the next boundary, requiring a fresh complete window. This scheduling recovery does not override a latched clock-fault/off state. Example: last request collection at 599, next at 901 → neither [300,600) nor [600,900) exports, [900,1200) is partial/discarded, [1200,1500) is earliest eligible. A timely 600.5 closure with all required ticks may emit [300,600) once with timestamps **300/600**, never 300/600.5. Duplicate collection and delayed source revisions cannot replace an attempted window. No catch-up zeros, backfilled samples, overlapping snapshots or late terminals attributed backwards.

**Cross-restart UTC continuity is an external deployment gate**, not proved by an in-memory watermark. Restart must not reuse a wall interval already exported by a prior process; operator/platform clock continuity and single-writer slot ownership must be evidenced before external export. If unavailable, keep export off; do not add persisted request state or reboot labels to conceal the limitation. Restarts never restore backlog.

**MVP shutdown decision (2026-10-08):** the operator withdrew the one-second global kill-acknowledgement requirement and bespoke DNS-cancellation work. Telemetry is expected to remain enabled. Keep the deployment-level `OTEL_SDK_DISABLED=true` setting and discard pending batches/drop the current export attempt on shutdown; no live-toggle framework is required. Standard system DNS for the fixed Honeycomb hostname may outlive that attempt, but has no credential or metric payload. Do not claim complete resolver disposal or bounded whole-process exit latency. Do not detach payload/HTTP/TLS work to continue sending after local cancellation. Bytes already accepted cannot be recalled. This does not disable Tinfoil platform collection or delete Honeycomb records. Local epoch/cancellation fixtures remain scoped implementation tests, not a global shutdown guarantee or an MVP rollout prerequisite. Restart repeats warmup. No shutdown partial flush, persistent spool or retries/re-export of uncertain windows. Transport attempt timeout <=1 second; unsent batch expires <=2 seconds after closure; stale batches are dropped, not retried. Any loss leaves data unavailable, without fine-grained loss counters. Telemetry failure must not affect HTTP, reservations, quotes, refunds, inference, permits or retries.

## Numeric cardinality and memory ceilings

These are conservative **documentation arithmetic / design ceilings**, not measured runtime allocation, RSS, serialization or Collector evidence. Cartesian overestimates include invalid admission combinations which the sanitizer still rejects; smaller reachability never grants new labels.

| Request table | Maximum native series | Maximum u64 bin/count cells per window |
| --- | ---: | ---: |
| http.requests | 16 | 16 |
| http.completed | 16×6×3 = 288 | 288 |
| http.duration | 288 | 288×11 = 3168 |
| generation.started | 2×3 = 6 | 6 |
| generation.completed | 2×3×12 = 72 | 72 |
| generation.duration | 72 | 72×11 = 792 |
| generation.first_output | 72 | 72×11 = 792 |
| delivery.completed | 2×3×3 = 18 | 18 |
| admission.rejected | 17×5×14 = 1190 | 1190 |
| admission.occupancy | 6 | 6×10 = 60 |
| **Request total** | **2028** | **6402** |
| Infrastructure (separate minute) | **20** | **20 gauges** |
| **Maximum co-closing export** | **2048 native series** | **6422 fallback/scalar points** |

Native histogram `count` is derived from bins, not another stored cell or independently released marginal. Fallback replaces distributions by bucket sums, so worst-case wire series/points is **6422**, not 2048. No raw per-tick arrays for request occupancy; increment the bounded bins. Additional gate-only u64 counters: HTTP disposition **288×5=1440**, generation missing-output **72**, occupancy contributors **6**. Per request window: `(6402+1440+72+6)×8 + 4096 metadata = 67,456 bytes`. Metadata budget includes validity bits, monotonic/grid anchors and table bookkeeping; fixed arrays/interned enums, no hash map/string growth.

At most one active request window and **one immutable closed request slot**, which is either pending **or** in flight, never one of each. Same rule for infrastructure: each of its two slots <=2048 bytes (20×6×8=960 numeric sample bytes plus validity/alignment metadata). If a frozen slot is occupied at next close, drop the new closed family/point rather than allocate or block; never mutate a batch already selected for send. No multi-window backlog.

Lifecycle pool: **256 records ×128 bytes =32,768 bytes**, plus at most **256 non-cloneable 16-byte handles =4096 bytes**. Accounts/request IDs, content and arbitrary labels are absent. Record includes epoch/index/generation counter, enum labels, optional start/dispatch monotonic instants, one optional output bucket, terminal/disposition bits or lease window marker; no exact output time. Baseline simultaneous budget: 128 HTTP guards (bounded telemetry pool, not a new HTTP admission policy), 4 generation guards, 4 streaming delivery guards and 78 lease records (64+4+4+4+1+1), total **214 <=256**. Slot exhaustion drops telemetry and invalidates the family, never rejects work; unsupported clone/retention patterns must be tested, not assumed covered by the serving connection limit. Existing shared lease owners carry one marker, not extra observation handles on every frame clone. Records live only with their existing owner, no new expiry/cancellation of work.

Additional ceilings: controller/static tables/bookkeeping **65,536 bytes**; supported OTel materialization scratch **8 MiB**; serialized payload buffers **8 MiB**; exporter/client task/transport allocations **8 MiB**. One send at a time across request/infrastructure; maximum **6422 points ×1024 bytes =6,576,128 bytes** serialized (fixed labels/scope only), within 8 MiB. A supported encoder exceeding either wire or allocation budget must drop before send and fail acceptance, not introduce a custom adapter. Streaming/chunking a linked family into independently releasable batches is not a workaround.

Accounted ceiling: `2×67456 + 2×2048 + 256×128 + 256×16 + 65536 + 3×8388608 = 25,407,232 bytes`. Hard incremental requested-allocation ceiling for this gateway telemetry pipeline **32 MiB =33,554,432 bytes**, remaining headroom **8,147,200 bytes** for layout/alignment/library overhead. Neither is a process RSS limit or proof against allocator fragmentation; separately budget Collector/reader processes in their later packets. Allocation-size/layout, SDK materialization/serialization/transport and pool-pressure tests on Rust 1.88 must validate these assumptions; if they fail, stop for a reviewed contract adjustment, not a silent cap increase. No dependency selection is made here.

## Local arithmetic check and later acceptance

The following read-only Python oracle is intentionally independent of future Rust aggregation code. It checks schema products, explicit cohort arithmetic, sparse/complement rejection and occupied-bin placement, **not runtime implementation correctness**. From repository root run `python3 -c 'from pathlib import Path; s=Path("docs/telemetry-schema.md").read_text(); exec(s.rsplit("<!-- arithmetic-check -->",1)[1].split("```python\n")[1].split("```")[0])'`.

<!-- arithmetic-check -->
```python
E, M, G, B, L, O = 16, 3, 12, 11, 6, 10
series = [E, E*6*3, E*6*3, 2*M, 2*M*G, 2*M*G, 2*M*G, 2*M*3, 17*5*14, L]
cells = [n*b for n,b in zip(series, [1,1,B,1,1,B,B,1,1,O])]
assert sum(series) == 2028 and sum(cells) == 6402
assert sum(series)+20 == 2048 and sum(cells)+20 == 6422
window = (sum(cells)+288*5+72+6)*8+4096
assert window == 67456
base = 2*window+2*2048+256*128+256*16+65536
assert 128+4+4+78 == 214 <= 256
assert base+3*8388608 == 25407232
assert 33554432-(base+3*8388608) == 8147200
assert 6422*1024 == 6576128 < 8388608
bounds = [.1,.5,1,5,15,30,60,120,300,600]
def bucket(x, edges):
    return next((i for i,e in enumerate(edges) if x <= e), len(edges))
assert [bucket(x,bounds) for x in [0,.1,.2,.5,.6,1,2,3,600,601]] == [0,0,1,1,2,2,3,3,9,10]
def allowed(atoms):
    return bool(atoms) and all(x == 0 or x >= 10 for x in atoms)
for n in [1,9,10,11]:
    atoms = [n, 3*n, 300-3*n, n, 300-n]
    assert allowed(atoms) == (n >= 10)
assert (300-30,30,300-10,10) == (270,30,290,10)
assert (300-33,33,300-11,11) == (267,33,289,11)
assert (300-60,60,300-20,20) == (240,60,280,20)
assert not allowed([])
for rare in ['error','model','endpoint','bin','duplicate']:
    assert not allowed([10,1])
assert allowed([20,10,10,240,60,280,20])  # mixed terminals
assert not allowed([11,10,11-10])          # missing-output complement
assert allowed([20,10,20-10]) and allowed([10,0,10-0])
assert bucket(10,[0,1,2,3,4,8,16,32,64]) == 6
assert sum([290,0,0,0,0,0,10,0,0,0]) == 300
assert 8+4 == 12 and 8+3+4 == 15          # V10 and Vmixed native points
assert 2+6 == 8                          # split starts vs terminal-only points
print('PASS: schema products, memory arithmetic, cohort/complement/bin oracles')
```

Independent acceptance must review the **whole contract**, not just these arithmetic assertions: label maps and route behavior, unknown/drop classification, terminal ownership, release complements, exact nonempty/withheld vectors, native/fallback wire obligations and epoch/clock/kill rules. Then packet 2B may implement aggregation and fake-clock/allocation/privacy tests; packet 2C selects supported export types and proves **actual serialized payloads** omit prohibited fields/defaults. Packets 3–4 separately test all lifecycle paths, full API/web/transport/ownership/accounting invariance, hostile canaries, resource units, Collector allowlisting/egress/self-diagnostics, Linux hermetic build, outages and in-flight cancellation. No local documentation result claims those tests have run.

External rollout remains blocked on packet 1's Honeycomb native metrics, US routing, seven-day metric retention/deletion/backups/access, **separate transport/audit** retention/access/disclosure, Tinfoil resource semantics/platform collection, measured isolated deployment/network/resource budget, clock continuity and operator authorization/cost gates. No external export, production change, paid probe or capability re-research in this packet. Sol's later fixed Collector round trip remains separable and is not ready for dispatch.
