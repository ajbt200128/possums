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

Signed release and actual debug deployment remain pending. Their results will be
recorded separately; existing startup logs are not retrospectively relabeled.
