# Ten-minute Tor readiness debug iteration

## Authorization and boundary

Operator requested a600s grace period, continued readiness observation instead
of stopping at180s, a readiness/failure record within grace, then debug deployment
and checking. This remains an experiment on `debug/tor-no-health-gate-v53`, based
on v0.0.53, for the existing `possums-debug` and `ajbt200128/possums` only. The
previous successful configured-onion transport probe remains scoped in
[the first iteration](tor-no-health-debug-verification.md#direct-onion-reachability-confirmed-on-a-later-probe).
No production/main activation, new instance/repository, SDK fork, inference or
private-original key read is authorized. Existing shared-key authorization does
not establish exclusive publication or physical cleanup of older publishers.

Read AGENTS and PRIVACY before changes. Applicable policy:
[startup exception and explicit debug extension](../PRIVACY.md#startup-diagnostics-exception),
[data boundaries](../PRIVACY.md#data-handling-boundaries),
[closed diagnostics](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[persistence/processors](../PRIVACY.md#processors-retention-access-and-shutdown).
Export remains disabled in this debug config. No new logger, exporter, dataset,
request/peer callback, spool, retry or periodic log is added.

## Minimal behavior

Only the explicit `debug-tor-no-health` image changes observer semantics:

- Main creates and owns the observer before preparation/bootstrap; the grace
  clock is not reset at service launch. Off remains off. Failure during original
  startup retains the original closed startup owner and exit status.
- Waiting record after30s if pending. No180s cutoff.
- At590s, if no actual first SDK Running/engine-failure was observed, attempt one
  `readiness_timeout` / `tor_startup_readiness_deadline` record. This is a failed
  readiness deadline, **not** an invented engine fault, health transition or
  successful/cancelled service outcome. OTLP severity is ERROR for this timeout.
- Continue the same bounded cell until actual first SDK Running or engine-failure
  (or host drain). A late real terminal is not masked by the timeout. At most
  three readiness records and one existing local-only delivery warning.
- Existing delivery stays nonblocking/drop-only, one owned one-second attempt
  per record, cancellable with the operation. No detached observer/upload.
- Debug Docker health grace changes300s to600s. Interval10s/timeout5s/retries3,
  all non-Tor checks and the debug Tor-independent health feature remain intact.
  The normal non-debug Tor observer still has its180s/two-record behavior.

A running/scheduled process attempts the deadline record before600s. No source
change can guarantee platform capture after OOM, process loss, unscheduled/blocking
host work, unavailable/full stderr or other presentation loss; no blocking fallback
or fake readiness is introduced to claim such a guarantee. Local records do not
prove remote reachability, current readiness after the first terminal, attestation
or any billing/settlement outcome.

## Scoped evidence

Pinned flake Rust1.92, locked/offline Cargo: debug readiness suite18 passed;
normal Tor readiness suite16 passed; debug library432 passed/one existing ignored;
binary6 passed. Formatting, strict all-target/all-feature Clippy and no-default-
feature compilation passed. The existing vendored SDK dead-code warning remains
unchanged. These source checks include paused-time deadlines, readiness after
180s and after600s, one-second stalled export bounds, races, closed timeout OTLP
severity/schema and untouched engine readiness/failure behavior. Existing actual
TLS/OTLP hostile-response/drop and Linux FIFO tests remain part of the suite.

[Hosted build39150178658](https://github.com/ajbt200128/possums/actions/runs/39150178658),
attempt1, succeeded from exact source `0aebc70c33dc0a66c8799b32df7b0229124bf9fa`.
Normal/debug lifecycle checks, formatting, Clippy, pinned Nix image build and
archive/registry digest equality passed. Actual downloaded Linux/amd64 image was
loaded and inspected: UID/GID65532, matching source and diagnostic labels. Digest:
`sha256:a660c5c4461c00c76bfeac224eebd04287b5812890f4e5d0ba0f8bdb9ecea33a`.
An actual-image offline/read-only/no-capabilities run with Tor enabled but absent
approved identity configuration failed promptly with closed
`tor_configuration_invalid`, startup failure, exit10; no network or service
launch occurred. This verifies the early failure diagnostic, not Tor readiness.

## Signed release and actual deployment

[Publication39151322202](https://github.com/ajbt200128/possums/actions/runs/39151322202),
attempt1, succeeded for `v0.0.53-debug-tor-health.2`, release commit
`3b2abdb818d92e1e77079d6b8c24512e8dd14235`. The existing repository manifest
validator checked the downloaded manifest/hash against exact committed config
and image. Supported `gh attestation verify` checked the exact tagged diagnostic
workflow, source/signer commit, predicate and hosted runner; checked statement
predicate/subject and invocation match that run/attempt. Manifest SHA256:
`7fdd7286656b5471508bd1bd9176577427b1603f1c8341bdb724bc3c6b35a3d4`.
No independently reproduced production release or main promotion is claimed.

Fresh exact existing debug selection: current `.1`, running, no pending update,
five managed bindings. Two read-only plans (one standalone, one immediately
before submission) approved `.2`, blue-green/no downtime/no hold, unchanged
resources/opaque variables/references/SSH settings and the exact existing five
secret refresh names. Promotion was explicitly disabled for this experiment.
One update POST was acknowledged, then observed `.1/pending`, `.1/started` and
finally `.2/running` with no pending update. Opaque settings/references remained
unchanged; `mark_latest_release=false` was confirmed. No replay, cancellation,
rollback, direct toolbox boot or production action occurred. Metadata completion
is not proof of physical cleanup or shared-key publication exclusivity.

## Actual readiness and Tor checks

Fresh SSH metadata was used with accept-new/never-replace-changed host-key policy,
no forwarding, bounded noninteractive commands. Installed toolbox README and
AGENTS were reread before inspection. No raw logs, environment/config dumps,
credentials, keys, reports, URLs or onion/peer identifiers left the safe projection.

Actual runtime matched the exact new digest and source/debug image labels.
Tor was enabled and export disabled. Docker health grace was **600s**, with the
existing10s interval/5s timeout/three retries, read-only root and /tmp512MiB.
Gateway was running/healthy, exit0, no observed OOM and direct loopback HTTP204.
Nine closed startup records included local initialization completion.

Bounded read-only observation captured these actual records:

| Elapsed bucket from pre-preparation origin | Outcome | SDK state | Problem |
| --- | --- | --- | --- |
| 30s to under60s | waiting | bootstrapping | none |
| 180s to under590s | first_local_ready | running | none |

The actual ready record was therefore captured **after the old180s cutoff and
before the600s grace ended**. Its code is `tor_startup_first_local_ready`; this
was not a deadline-generated/fake Running state. The590s timeout was unnecessary
in this live run; its emission/continued-observation behavior is qualified by the
paused-time/actual-sender tests, not invented live failure evidence. Exact ready
time is deliberately not retained. First-ready is not a perpetual current-health
or full attestation guarantee.

One deliberate post-update direct Tor check used the same isolated supported
C Tor/Stem/PyCURL helper as the prior iteration, with only exact tag/image constants
updated (public helper SHA256
`153248d41a8073c4ab14ed1ad1a8dafc5d27376a2a9fca2a324609f50339a329`).
Independent client bootstrapped; public HTTPS HEAD control returned200; explicit
v3 HSFETCH was accepted and a descriptor was received (two request events).
Configured-onion stream succeeded, TLS peer/hostname verification remained enabled,
and HEAD `/healthz` returned **HTTP204**, libcurl0. No insecure fallback,
inference, redirect following, raw header/body capture or automated HTTP retry.
The owned client was reaped and temporary client state removed, not claimed
securely erased. Routing values remained in memory, never diagnostic output.

Final fresh selection: `.2/running`, no pending update, five bindings. This
establishes observed debug SDK first-readiness and configured-onion transport
reachability, not attestation/HPKE/accounting qualification, exact invoice parity,
independent attribution of a shared onion publisher to one VM, exclusive
publication or cleanup of earlier publishers. Production and main were untouched.

## Subsequent debug attestation availability check

Operator requested checking whether attestation works on this debug deployment.
One fresh nonce-bound public platform acquisition, selected from fresh exact
`.2/running/debug=true` metadata with no pending update, returned **HTTP503** and
the allowlisted closed protocol code **`attestation_unavailable`**. No success
report, quote, certificate payload, hardware identifier, nonce, URL, headers or
raw exception was returned or retained in this record. No appraisal, retry,
inference, instance mutation or production probe was performed. Opaque settings
and references were compared in memory and remained unchanged.

In the pinned CVM source already documented in the preceding investigation,
this code marks collateral acquisition/cache unavailability before fresh device
evidence construction/appraisal. The underlying provider/cache cause is unknown;
this is not evidence that debug policy rejected an otherwise valid report or
that Tor caused the failure. Debug-mode evidence availability can be checked,
but a debug-enabled instance must not qualify normal production confidentiality/
client trust. Tor readiness and successful transport do not establish attestation
availability or authorization. Current debug fresh attestation remains unavailable.
