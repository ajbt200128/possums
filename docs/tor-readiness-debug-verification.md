# Tor attestation readiness — guarded diagnostic verification

## Scope and privacy

This is a nonproduction diagnostic, not production release or full Tor qualification.
Only the original `ajbt200128/possums` repository and existing `possums-debug`
instance were used. No production/intake inspection or mutation, new repository or
Tinfoil instance, nested agent, inference, native B private-key read, signing,
endorsement, runtime identity replacement/generation or onion publication occurred.

`AGENTS.md` and `PRIVACY.md` were read fully before implementation. Applicable
policy: [data boundaries](../PRIVACY.md#data-handling-boundaries),
[forbidden content/identifiers/raw errors](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[startup diagnostics](../PRIVACY.md#startup-diagnostics-exception),
[processors and persistence](../PRIVACY.md#processors-retention-access-and-shutdown),
and [required evidence](../PRIVACY.md#required-evidence-for-every-telemetry-change).
All five managed-secret name bindings and opaque Admin settings were compared in
memory and preserved. Managed-secret values, runtime environment/arguments/config
and raw logs/errors were not exported. VM observations were closed-projected on
the VM before SSH output. No log bundle or telemetry activation was added.
`OTEL_SDK_DISABLED=true` remained set; this does not disable native platform
capture or Pi/T3 history, nor establish whole-process transience or erasure.

## Isolated source and minimal fix

Original checkout remained clean `feat/tor-v0` at
`c4979988cc20308f9eabda39df6d90b7943bf932`; siblings were not edited. Work used
`/tmp/possums-readiness-runtime`, branch `debug/readiness-acquisition`.

Separate commits:

- `b3fb03b`: independently usable readiness fix and tests only.
- `e7dc098`: diagnostic-only compile-time pre-key/pre-client stop and tests.
- `828c170dd74df48d0c748b26dead89e07e2891ef`: isolated diagnostic image build.

The fix changes only Tor-native acquisition. Observed local HTTP 503 permits a
paced 250-ms reacquisition within **one 30-second total acquisition deadline**.
Every other HTTP status, transport/body/size failure, malformed evidence or
verification rejection remains terminal. A fresh nonce is still generated once
by the existing Rust owner and bound unchanged through acquisition/appraisal.
No default native/WASM response schema, verification/publisher/freshness/A/B/TLS/
HPKE gate, inference, accounting, cancellation or drain policy changed. The
existing Rust 45-second helper bound and kill-on-drop startup cancellation remain.
An exhausted 503 wait retains the observed numeric status rather than inventing
its underlying cause. The helper does not inspect the upstream 503 body.

The diagnostic feature `tor-appraisal-diagnostic` is not enabled by the ordinary
shipping `--features tor` build. Its config path parses the public expected-onion
configuration but never requests the native B private-key/directory configuration.
`PreparedTor::prepare` performs the same fresh official appraisal and current-key
check, then unconditionally stops **before grant reads, signer creation, B import,
state, TorClient construction/bootstrap or publication**, even on unexpected
appraisal success. Synthetic accepted/expired/failed verifier fixtures test that
boundary; they are not real attestation success. Do not merge the diagnostic
flake/workflow as production release configuration.

The separate startup-delivery change `fe7a4747` was not combined into this image.
No Honeycomb ingestion or startup-export reliability qualification is claimed.

## Tests and actual artifact

Pinned existing environment: Go **1.27.2**, Rust **1.92.0**; Go module acquisition
and Cargo network access disabled for local synthetic checks.

Passed:

- `go test -race -mod=readonly ./...` in `attestation-helper`: real Unix transport,
  503→ready, permanent 503, other statuses, malformed/wrong-nonce/unauthenticated
  evidence, request timeout, active-request/wait cancellation, hostile error bodies,
  closed/default wire preservation and no post-rejection reacquisition.
- Actual freshly built helper executable, synthetic Unix server: 503/503/200 made
  exactly three acquisitions, then rejected unauthenticated ready bytes without
  retry; HTTP 401 made one acquisition; permanent 503 made 119 acquisitions in
  30.02 seconds; SIGTERM during readiness reaped the child after one acquisition.
  The challenge stayed unchanged. No authentic ready-success fixture is claimed.
- Normal Tor runtime tests: 16; diagnostic-feature runtime tests: 16; Tor helper
  owner/projection tests: 3. Full all-feature library: **403 passed**; binary:
  **6 passed**. Package formatting and strict all-target/all-feature Clippy passed.
  The vendored SDK's existing dead-code warning was unchanged.
- Active LSP probes were unavailable for four scratch paths, not clean evidence.
  Executed pinned compilers/tests above provide the actual qualification.

One isolated existing-dispatcher invocation, exact repository/tag/attempt-1 gated:
[run 38996662476](https://github.com/ajbt200128/possums/actions/runs/38996662476),
tag `v0.0.49-debug-readiness.1`, source
`828c170dd74df48d0c748b26dead89e07e2891ef`, succeeded. It used pinned Nix inputs and
the existing supported image build, published one uniquely tagged GHCR image and
verified the registry digest against the built archive. Source and diagnostic
labels were checked before publication. Build dependencies/cache reuse was
allowed: this is **not** two independent production builds, official gateway
measurement, production provenance qualification or a GitHub release. No protected
production guard, default-main workflow or activation variable changed.

Exact diagnostic image:
`ghcr.io/ajbt200128/possums-gateway@sha256:918db0b9deb6d54d3bbea7931a1e6772170af5f9ef3ae93de5ea1c311232ac6b`.
The downloaded archive/identity artifact and immutable registry image matched the
source-bearing successful invocation. Linux/amd64, UID/GID 65532, source revision
and appraisal-only labels were independently checked. Two actual-image offline
runs, network disabled/read-only, with missing versus hostile **synthetic** native
identity input, both reached Tor appraisal and failed with closed acquisition
transport diagnostics, not identity parsing. Neither reached connect/bootstrap/
launch. The owned local containers were removed; native Docker image/metadata
retention is a separate boundary.

## Existing debug runtime iterations

Fresh native/Admin private revalidation preceded each SSH operation: exact existing
name/repository, debug=true, current diagnostic key-only tag, no pending update,
all five bindings and opaque settings unchanged. Current SSH metadata was selected
rather than reusing a stale port; accept-new host keys, no replacement or agent
forwarding. Installed toolbox README/AGENTS/help were read. Only its documented
Docker and `tindbg boot` container-scope mechanisms were used.

Before mutation, the old exact v49 image was healthy/HTTP 204 with Tor off/export
disabled. One fresh official helper invocation returned acquisition HTTP 503.
That helper-only observation is not a Tor-on startup test.

Pinned CVM 0.14.12 debug boot waits for Docker healthy/unhealthy without recognizing
an exited application. To avoid stranding that manager on a deliberate fail-closed
appraisal exit, only the gateway's Docker healthcheck was temporarily **omitted**,
not replaced with a fake successful test. The reserved toolbox healthcheck was
preserved. Manager acknowledgement was never treated as application health/trust.
Each config edit was inverse-byte-compared to permit only its explicit delta.

Exactly three acknowledged boots/config edits:

1. New exact diagnostic image, Tor off, gateway healthcheck omitted. Direct HTTP
   204 and six closed startup records, ending in initialization complete; no OOM.
2. Same image, **Tor on**, sole subsequent config delta. Entered Tor preparation;
   failed closed with `possums_v3_acquisition_http_status`, stage `acquisition`,
   constraint `http_status`, **status 503**. Exit 10, no OOM, five closed startup
   records, no initialization completion or connect/bootstrap/launch stage.
   This qualifies guarded startup/acquisition only, not successful Tor preparation.
3. Same image, Tor off; original healthcheck restored (300-second start period,
   10-second interval, 5-second timeout, three retries). Running, **healthy/HTTP
   204**, initialization complete, no OOM, read-only root and /tmp 512 MiB.

No uncertain boot was replayed, cancelled or rolled back. Counts for this packet:
one atomic branch/tag push; one diagnostic workflow dispatch/image publication;
zero GitHub releases/official measurements; one debug image pull; **three toolbox
boots**, three config edits, one Tor-on experiment; **zero instance-update POSTs**,
production actions, inference, B private-key reads, endorsements or publications.
All three boot acknowledgements were recorded separately from application outcomes.

## Narrowed acquisition gate and remaining unknowns

After restoring the healthy control, one additional disposable, socket-only
observer made a single fresh nonce acquisition with a five-second bound. It used
a self-reviewed small Go tool built with the pinned compiler and supported Docker tools,
transferred as a public image archive (no GHCR publication). Archive/image/source/
binary identity was checked on the VM. It ran read-only, UID/GID 65532, no network,
all capabilities dropped, no-new-privileges, with **only the attestation socket**
mounted: no environment or key grants copied. The observer closed-projects only
three known shim 503 codes and never returns response messages, quotes or raw
errors. Hostile/malformed projections were tested offline. It neither appraises
nor authorizes anything. Its container was removed; healthy control remained 204.

Observed: HTTP **503**, closed shim category **`attestation_unavailable`**, not
`service_starting`. In pinned public CVM source commit
`35d94c85ad20e9f98cd4ddcf35dfe4574b5215fb`, `cmd/shim/api.go` maps
`collateralSource.Current` failure to that 503 category. This narrows the current
debug barrier to **collateral acquisition/availability before evidence appraisal**.
Its underlying failure is unknown; no raw platform exception/log was inspected or exported and
no authentication rejection, signer failure or production cause is inferred.
Another unchanged reboot would not resolve or explain this boundary. Further
provider-side classification must preserve appraisal and publication gates.

Final control-plane tag remains `v0.0.49-debug-keys.1`, no pending update. The
container-scope override uses the **new diagnostic digest above**; the old v49
image/tag is not claimed to contain the readiness fix. Tor is off and export is
disabled. All five bindings/opaque settings remain unchanged. No full Tor E2E,
verified serving via onion, real successful self-appraisal, native-v3 CLI signer
success, production rollout/cause, invoice parity, billing/refund or platform
privacy/retention/erasure property is established. Legacy-v2 freshness limitations
and the unresolved native-v3 platform-signer boundary remain unchanged.
