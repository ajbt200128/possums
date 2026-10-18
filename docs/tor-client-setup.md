# Source-only Tor public setup response adapter

> Current source-restoration scope: [Tor main restoration](tor-main-restoration.md).
> The integrated native-helper path below is restored, not the alternative
> transport spikes; historical test counts are not new qualification.
>
> Historical evidence only in this inactive HTTPS-only backup. Tor code/assets
> are removed; setup commands and authorizations below do not apply here.
> No installed package was changed. See [backup verification](without-tor-backup-verification.md).

## Current client integration — offline only

The following historical adapter packet is now used by the source candidate's
Tor bootstrap. A locally owned `TorConnection` scopes public acquisition and
private requests through the official `tor-socksproto` 0.47.0 helper, with remote
destination DNS and no global dispatcher, direct fallback or inference replay.
One fresh official Tor appraisal supplies release/TLS/HPKE/A facts; its same
nonce and selected B must verify the endorsement before private channel creation.
Each private HTTP request still requires actual peer possession, endorsed SPKI,
original hostname/date and unexpired trust before headers or body are sent.
Public acquisition retains native WebPKI; a self-signed public acquisition
failure is not permission to bypass it.

Source setup accepts `--possums-tor-onion` and optional
`--possums-tor-proxy` (default `127.0.0.1:19050`, numeric loopback only). Selection
is locked at first trust establishment, after Pi applies flags. Tor cannot be
combined with the legacy `--possums-manifest` mode. Optional proxy-only
`POSSUMS_TOR_PROXY_USERNAME` / `POSSUMS_TOR_PROXY_PASSWORD` are read only after
valid explicit Tor selection, both present or both absent; do not pass them in
flags or use Possums credentials. No daemon is launched by selection or status.
The helper is selected from the matching package, not an arbitrary CLI path.
Native scope closure waits for owned child exit; later sessions recreate a
scope from the locked route. The operator accepts the official SDK's unused
CONNECT reserved-byte tolerance, not invalid authentication versions or an
isolation/Tor-traversal guarantee.

Parent reproduced **69 actual-helper channel checks**, **36 direct-v3 checks**,
**13 selection/host diagnostic check groups**, and pinned TypeScript checking
(Node 24.13.0, TypeScript 5.9.3, Pi 1.0.4). Selection lifecycle fixtures substitute
acquisition and do not qualify installed native-child execution. Independent
channel source review found no scoped issue but could not rerun its checks;
parent reproduction supplies that separate evidence. ReferenceClient failure
preservation, full packaged checks, installed host presentation, measured
release, stable identity provisioning and live Tor E2E remain pending.

No installed/activated/deployed client or gateway, positive hardware/application
key chain, live Tor, inference, refund or whole-host privacy claim follows.
[Data boundaries](../PRIVACY.md#data-handling-boundaries) and
[typed failures](../PRIVACY.md#mandatory-typed-interface-failures) remain applicable;
Pi/T3 history and host instrumentation are separate boundaries.

## Historical adapter packet

Status: **offline source candidate only**. The paired diagnostic follow-up below
supersedes the initial success-only decoder description, not deployment status.
The initial packet adds only
`clients/pi/tor-setup.mjs`, `clients/pi/tor-setup.d.mts`, `tests/tor_setup.mjs`
and this document. It does not change production bootstrap, provider entry,
transport, installed client, gateway, deployment or runtime configuration.

## Proposed public wire and shared appraisal seam

The parent/backend proposal is a public `GET /attestation/tor?nonce=<64 lowercase
hex characters>` with the caller's fresh 32-byte challenge. The JSON response
has exactly three fields: `evidence` (a v3 quote **object**), `payload` (190
lowercase hex characters / 95 bytes), and `signature` (128 lowercase hex
characters / 64 bytes). This is a proposal, not deployed route or wire
compatibility evidence. The parent owns reconciliation of any backend gap.

`decodeTorSetupResponse(responseBytes)` accepts bytes only, copies them and
rejects responses larger than **16 MiB + 2 KiB** before parsing. It uses fatal
UTF-8 decoding and native strict JSON syntax, rejects truncated/trailing JSON,
non-record envelopes, missing/unknown fields, null/array/non-record evidence,
and prototype-related keys (`__proto__`, `constructor`, `prototype`, including
escaped keys) throughout parsed JSON. It validates hex type, exact length and
lowercase alphabet **before** `Buffer.from`. The quote object is re-encoded as
JSON bytes and must fit the SDK's **16 MiB** evidence limit. Nested v3 evidence
schema and cryptography remain the production SDK's responsibility. Returned
byte buffers are independent copies; the frozen container has literal
`transport_authorized: false`, not a trust verdict.

`verifyTorSetupResponse(responseBytes, challenge, manifest,
selectedOnionIdentity, repository?, signal?)` snapshots all caller-owned bytes
before its first await. It rejects pre-aborted work before decoding/appraisal.
It delegates only to existing `authorizeTorEndorsement`, with no injectable
verifier. That helper uses production `verifyTorV3` (official SDK,
manifest/publisher/freshness checks), the endorsed application key and Node's
Ed25519 verification, followed by expiry/cancellation checks. Existing
`VerifierFailure` and `TorEndorsementFailure` pass through unchanged. The paired
follow-up additionally preserves `TorSetupBackendFailure` as unverified metadata. New
`TorSetupFailure` diagnostics contain only a closed code/stage/constraint and
locally authored next step; unexpected causes are explicitly unknown. They
retain neither parser input, original exception/cause nor stack.

The signed payload remains the existing fixed 95-byte format:
`possums/tor-onion-endorsement\0` (30 ASCII bytes), version `1` (one byte),
challenge (32 bytes), selected onion identity B (32 bytes). B must eventually
come from a qualified pinned onion parser; raw B here does not establish an
address checksum, identity custody or a connection. All returned facts remain
`transport_authorized: false`. No secret/inference authorization is exposed.

## Native JSON limits

The existing duplicate/depth/node preflight is in
`examples/phase01/limits.ts`. Direct import in pinned Node **24.13.0** fails
with `ERR_UNSUPPORTED_TYPESCRIPT_SYNTAX` because of parameter properties; there
is no dependency-free MJS helper to reuse. This packet adds no scanner or
parser framework. Native JSON therefore **does not reject duplicate keys**:
last value wins, covered explicitly by an offline limitation test. There is no
independent depth/node preflight or whole-process memory/RSS guarantee. Native
parse/stringify resource failures fail closed with a content-free unknown
response diagnostic. The byte ceilings bound admitted input/SDK bytes, not
peak allocation. Do not claim duplicate-key rejection or the TS helper's
structural budgets.

## Deferred transport and product integration

Unmodified **Undici 8.11.2 / socks 2.8.10** are not qualified for strict SOCKS
authentication plus handshake cancellation. The [spike](tor-client-spike.md)
records local patch evidence, not production transport qualification. No onion
parser, network connector, provider dispatcher, failover coordinator, cache,
persistence, telemetry or setup UI is added here.

A later separately qualified public Setup/Test action and normal Tor-only path
must share this appraisal seam; neither is wired now. Any future dual-address
selection is limited to public pre-credential setup. Never fall back or replay
a possibly accepted submission. Actual application-key chain, pinned onion
parser, address/connection binding, strict handshake/cancellation, expiry at
secret dispatch and real Tor/circuit behavior remain unqualified. There is no
positive hardware/application-key-chain fixture in this packet; a valid public
RFC signature is not appraisal evidence.

## Privacy and offline verification

Applicable policy: [data handling](../PRIVACY.md#data-handling-boundaries),
[forbidden telemetry](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[typed interface failures](../PRIVACY.md#typed-interface-failures-and-token-aggregate-extension)
and [synthetic testing](../PRIVACY.md#synthetic-testing-exception). Diagnostics
initiate no requests/retries and export nothing. No credentials, account data,
URLs, onion identifiers, raw errors or request content enter them. Native
Pi/T3 history and host instrumentation remain separate boundaries; this is not
whole-process transience or a whole-app no-logs claim. Public setup says nothing
about generation, billing, refund or cancellation outcomes.

Executed with pinned `nix develop --offline`, Node **24.13.0**:

`nix develop --offline --command node --check clients/pi/tor-setup.mjs`

`nix develop --offline --command node --check tests/tor_setup.mjs`

`nix develop --offline --command node tests/tor_setup.mjs /tmp/possums-v3-build-xVULmt`

**93 checks passed**, using the already available production WASM scratch
artifact, current production bridge/worker/helper source copied into temporary
scratch, and `tests/fixtures/tor_grant_rfc8032.json`. That fixture stores the
public RFC key/signature; its existing Go generator defines the payload with
challenge `0x23` repeated 32 times and B `0x42` repeated 32 times. Tests decode
those exact payload/signature bytes and exercise the existing Node signature
helper's wrong-challenge/B/signature rejection without appraisal claims.

Coverage: exact schema/hex/byte ceilings (including inclusive limits), malformed
UTF-8/JSON, unknown/prototype fields, hostile diagnostic canaries, independent
copy/mutation behavior, closed unknown fallbacks, typed-failure preservation,
pre-abort and in-flight real-worker cancellation. An observing Worker subclass
runs the **unchanged production worker/WASM**, checks delegated action/nonce/
manifest bytes and never fabricates a verdict. Fabricated quote objects fail
closed with production `possums_v3_envelope_rejected`. Static seam checks ensure
signature/appraisal are delegated, not reimplemented. A throwing fetch stub
observes zero main-thread requests; the unchanged production worker separately
disables fetch. No synthetic positive SDK callback is used. Because appraisal
rejects, B's post-appraisal/expiry paths are not positively exercised here.

An active path-scoped LSP probe checked the two modules and test script: no
errors/warnings, with two advisory ast-grep hints on the declaration (parameter
count/class shape). No API redesign was made for those hints. `git diff --check`
also passed for the existing tracked diff.

No agents, commits, installation, dependency downloads, live Tor/bootstrap,
public probes, inference, Rust/backend checks, publication or deployment ran.
The declarations accompany the source; the Node checks are syntax/runtime
evidence, not a full installed Pi/T3 or TypeScript integration qualification.

## Paired closed backend diagnostics — source-only follow-up

Only the new opt-in Tor route returns `{error: {code, stage, constraint,
message, action}}`, with optional numeric `status` **only** for
`possums_v3_acquisition_http_status`. Success remains exactly the original
three fields. Default helper, WASM bridge and shipped route schemas are unchanged.
The decoder recognizes the exact error envelope before success decoding and
validates the code/stage/constraint tuple against locally closed Tor/backend
categories and the existing `VerifierFailure` vocabulary (excluding bridge and
client-only input-schema categories). Unknown categories/types/extra fields,
prototype keys and invalid status fail closed as response/schema errors.

`TorSetupBackendFailure` preserves the original known helper code, for example
`possums_v3_keys_tor_endorsement_spki`, **without** echoing upstream message or
action. Its own text/action are locally authored; `metadata_authenticated` and
`transport_authorized` are both false. These public diagnostics establish no
backend authenticity, service liveness, transport, credential dispatch or
accounting conclusion and are not authenticated appraisal/dispatch-blocking
verdicts. A status must be an integer from 100 through 999, refers to attestation
**acquisition**, and is not necessarily the gateway response HTTP status. No
root cause, retry, renewal or billing outcome is inferred from 503 or any status.
Backend diagnostic handling stops before any SDK worker/appraisal and starts no
request. Caller snapshots, quote/manifest/hex bounds and native duplicate/depth
limitations above remain unchanged.

Focused pinned offline follow-up: **193 Node checks passed**, using the existing
`/tmp/possums-v3-build-SsJ3PA` production WASM artifact. Node syntax checks passed.
Additional coverage includes hostile backend message/action/code/prototype
strings, unknown tuples, exact optional status typing/range/category, known
failure preservation through the wrapper, and zero SDK-worker creation for
backend failures. Actual Axum-generated negative JSON is separately fed directly
to this production decoder by `src/server/tor_tests.rs`; no positive chain or
caller-injected appraisal verdict is used. The same
[privacy boundaries](../PRIVACY.md#data-handling-boundaries),
[forbidden telemetry](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data)
and [typed-failure policy](../PRIVACY.md#mandatory-typed-interface-failures)
apply: no telemetry, logs, persistence, diagnostic-triggered work, installation
or live transport is added.
