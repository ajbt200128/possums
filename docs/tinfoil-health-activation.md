# Tinfoil health activation candidate

## Scope and operator decision

The operator initially requested a production healthcheck test on 2026-10-10,
rather than additional independent reviews of small changes. The later paired
instruction required finishing E before one bounded paid real-Pi pass and one
ordinary health-config rollout through the protected release/deployment path.
Neither instruction authorizes an unseen candidate, installed-client update,
held promotion, paid overlap experiment or storage migration.

The measured gateway healthcheck executes `/bin/possums --healthcheck`, with a
10-second interval, five-second timeout, three retries and a 60-second start
period. The command makes only the fixed local readiness request, before
configuration, credentials or SDK initialization. Success is silent; failure
has a fixed content-free message. Readiness requests bypass request aggregates.

The source image pin is the independently qualified v0.0.25 image. The release
publisher replaces that pin with the exact newly built image, qualifies the
actual fresh A archive before upload, and preserves the independent A/B check.
Offline helper and release-transformation tests run in required CI. Real-image
CI must exercise healthy, quiescing and absent-listener outcomes under the
image's configured user and isolated, read-only runtime.

## Production qualification still required

At source preparation, the total application start-to-readiness duration is
**unknown**. The 30-second SDK connection bound does not establish a 45-second
application startup margin. The selected start period is a production-test
setting, not measured startup evidence. Do not report health enabled or healthy
until the protected deployment and authenticated platform readback establish
those states; record successful boot/readiness separately from exact duration.

Preserve current-main, public source/release/tag/image/measurement binding,
release provenance, native eligible-owner and actual candidate-review gates.
Issue no duplicate update on an uncertain operation. The live public health
check must use the existing fully verified SDK-bound connection, without
credentials or inference. An empty HTTP 204 alone does not prove platform
health configuration, billing, retained owners or credit/session continuity.

## Privacy and remaining limitations

Applicable policy: [data handling boundaries](../PRIVACY.md#data-handling-boundaries),
[permitted telemetry](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data)
and [synthetic testing](../PRIVACY.md#synthetic-testing-exception). Image tests
use isolated synthetic fixtures; production is not relabeled synthetic.
No request logs, traces, support bundles or raw platform errors are collected.
Docker retains local images/metadata; fixture cleanup is best-effort, not
whole-host transience. Platform health may expose the fixed failed-probe message.
Existing metrics enablement, destinations and retention are unchanged.

Old-enclave owner retention, forced termination behavior, memory-only credit
and session continuity, native-v3 signer mismatch and legacy-v2 freshness
limitations remain unresolved. Health configuration does not settle them.

## Paired v0.0.26 production evidence

PR #54 paired recovery E and H1/H2 in one publication/rotation, with successful
first-attempt exact-head CI `38101186287`. It merged as:

- S: `56c9b4cec3fb791ac21834507e62ee158f57f5ae`
- R: `71893cc50cd045a098cd218596bb4831941a1c01`
- V: `v0.0.26`
- D: `sha256:7f35311fbd688e814a03326a668da70ea2ba6e12500a5d0b39cea48cb83e4363`
- M: `490d7db9a2686f029d88edd0c9312428774e9afaafb86fc626b9f683c41b0408`

Main CI `38101409484`, fresh-A/B publisher `38102100667`, tag CI `38102653625`,
official publication `38102851378`, and protected production `38102913167`
all succeeded on attempt 1. Parent independently verified public signed
S/R/V/D/M and build provenance, current main, both activation flags, existing
native production policy, eligible operator and pending exact candidate. One
approval POST was followed by an actual authenticated review re-read. No
approval from v25 was reused.

Parent parsed only the two closed deployment/serving receipts in memory:
`mutationCommandCount: 1`, successful post-call blue/green plan review,
unchanged opaque configuration, target running/no pending update, and the
later SDK-bound serving verification. No update was replayed. A separate
credential-free, fully pinned official-SDK `/healthz` request qualified empty
HTTP 204 against V/M and endpoint-key binding; it initiated no inference.
The same resident real-Pi memory session obtained authenticated settlement
receipts before/after rotation and closed successfully; see [Pi scope](blue-green-pi-qualification.md#subsequent-paired-real-pi--v0026-qualification).
Private billing values/transcripts are not published here.

**What is observed:** measured source/release health configuration, qualified
production packaging, ordinary protected rotation completion, attested
public serving/readiness and the scoped same-Pi before/after paid pass.
**What remains unobserved:** an explicit authenticated platform/API or
Dashboard health-configured/healthy flag. The existing sanitized CLI receipt
exposes running/no-pending, not that health flag; public Admin API docs do not
specify it. Do not substitute an empty204, source declaration or inferred
startup readiness for that separate readback. No extra credential request,
status-triggered generation, second rollout or installed-link change was
performed. Exact application startup duration remains unknown; 60 seconds
is the test configuration, not a measured 45-second guarantee. Owner/credit
retention and the native-v3 CLI/legacy-v2 limitations above remain open.
