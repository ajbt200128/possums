# v0.0.20 timeout/renewal rollout — 2026-10-10

Operator authorized pushing and deploying main. Pushed source
`e4e268722f9fe3f6bcd25abbb5d100bcad2ab038`; digest-only PR #47 merged as
`b8a05ee5dd40a1d3bdd84b6242c2d8d25964363d`, the tagged release source.
No Free or separate blue/green implementation branch was merged.

## Outgoing scope

Gateway inference uses 600-second response/header and per-HTTP-read waits,
without an absolute generation lifetime or competing SDK-event idle timer.
Tokenizer/catalog preflight remains finite; verified connection establishment
uses a five-second timeout. HTTP/1 closes after one request instead of killing
active work at an absolute connection age. Incoming upload/header and memory
limits remain. Reference/Pi clients use underlying byte progress for streaming
inactivity; Pi renews expired trust/auth at cancellable explicit-submit resolution.
Closed diagnostics, terminal accounting and no gateway inference replay remain.
See [timeout boundaries](provider-timeout-alignment.md),
[integration qualification](pi-timeout-renewal-integration.md) and
[prior local client installation](pi-timeout-renewal-installation.md).

## Publication evidence

- Source CI `38075658661` initially failed
  `helper_still_rejects_genuinely_future_issuance` at its expected-error assertion.
  One unchanged-source failed-job retry passed. This does not resolve the
  historical intermittent Linux helper-test limitation or establish its cause.
- Image run `38075658533` built both fresh archives successfully on attempt one;
  publication was blocked by source CI. After CI passed, a failed-job rerun
  published the agreeing archives and opened the digest-only PR.
- Image: `sha256:70ce4872e61dc1d349b49a4225221faba97189ec1bd1839cfcb4b437ce040a90`.
- Digest-branch CI `38077026696` passed after workflow approval; exact merged-main
  CI `38077212054` passed. Release preparation `38077375211` and signed publication
  `38077385517` passed.
- Public manifest SHA-256:
  `6c8147ca92e72327d4be3aafd058e0bcbbc721459916f186fbed09ca93069f71`.
- Embedded config SHA-256:
  `59a2b1ef5865c599b4c922a8297b4eb86f4794ee984c761d0f96c40b9258a964`.
- GitHub cryptographic verification bound the exact manifest/custom Tinfoil
  predicate to the tagged source, hosted release-publish workflow, signer digest
  and invocation. Embedded config matched committed config byte-for-byte and
  pinned the reproduced image; CVM version/shape checks passed.

## Deployment evidence and limits

Exactly one supported temporary CLI 0.19.0 update targeted the existing paid
`possum-phase0` container, from running v0.0.19 to v0.0.20. Closed stdin, no
`--yes`, no replacement flags and no mutation retry. The CLI returned success;
immediate readback still showed v0.0.19 with v0.0.20 pending. Same-process
comparisons confirmed opaque variables, secret references, SSH/resource/settings
fields unchanged. Raw control-plane output and errors were not retained.

The reused script's post-call plan validator returned false. The raw plan was
not retained; its precise failing constraint is unknown. Do not count this as a
verified blue/green/no-downtime plan or proof of uninterrupted availability.
Subsequent bounded read-only polling confirmed **v0.0.20 running, no pending
update, debug off, confidential mode on, unheld**. No mutation was replayed.

This verifies public release provenance and control-plane running state, not a
fresh serving endpoint-key/attestation binding, native-v3 CLI verification, live
inference, live timeout/renewal behavior or billing. No inference canary was sent.
No installed client package, approval pins, credentials, runtime or settings were
changed by this rollout. Existing release-pinned clients require a separately
verified approval refresh; automatic renewal does not authorize a new release.
Legacy-v2 freshness and native-v3 signer mismatch limitations remain.

## Privacy scope

Policy: [change control](../PRIVACY.md#mandatory-reference-and-change-control),
[data handling](../PRIVACY.md#data-handling-boundaries),
[allowed/forbidden exports](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[processors/shutdown](../PRIVACY.md#processors-retention-access-and-shutdown), and
[required evidence](../PRIVACY.md#required-evidence-for-every-telemetry-change).
No telemetry family, destination, retention, logging or diagnostic export changed.
Prior synthetic accounting/privacy tests remain scoped local evidence, not fresh
live ingestion or a runtime retention/privacy proof. No Honeycomb query, support
bundle, request-level export, paid inference or credential-value inspection was
performed. Platform privacy/retention and billing unknowns remain unresolved.
