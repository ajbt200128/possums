# Tinfoil health activation candidate

## Scope and operator decision

The operator requested a production healthcheck test on 2026-10-10, rather than
additional independent reviews of small changes. This authorizes one ordinary
health-config rollout through the existing protected release/deployment path,
not an unseen candidate, inference canary, client installation, E recovery,
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
