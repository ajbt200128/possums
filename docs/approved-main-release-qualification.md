# Approved-main releases — hosted qualification (2026-10-10)

This is a chronological qualification record, not a deployed-runtime/privacy
claim. See the [design and activation guide](approved-main-releases.md).

## Admission and disabled-state evidence

[PR 48](https://github.com/ajbt200128/possums/pull/48) merged to
`a487d203bb1cf54a78e4c2e4f1c36377551c15a8` after successful first-attempt
[PR checks](https://github.com/ajbt200128/possums/actions/runs/38081860521).
[Merged-main checks](https://github.com/ajbt200128/possums/actions/runs/38082719030)
also passed: release unit tests, standalone Go verifier tests, flake checks,
image build and isolated synthetic read-only startup smoke. The downstream
[automatic publication](https://github.com/ajbt200128/possums/actions/runs/38083349892)
skipped while activation was off.

The operator authorized setup of `production`: sole reviewer `ajbt200128`,
self-review allowed, branch-only `main`. The operator entered the admin secret
privately. Local authenticated checks verified policy and secret-name metadata,
not the secret value. REST does not expose the administrator-bypass setting;
actual operator review history remains mandatory for mutation.

The standalone pinned Go SDK verifier then passed against currently served
`v0.0.20` with manifest digest
`6c8147ca92e72327d4be3aafd058e0bcbbc721459916f186fbed09ca93069f71`:
SDK V3 appraisal, exact manifest, witness freshness and endpoint-key-bound
public HTTP GET passed. No credentials, inference or deployment were sent.
This does not resolve the historical native CLI V3 signer mismatch or Pi
legacy-V2 freshness limitations.

## First publication qualification and correction

The operator subsequently authorized activation and qualification through
protected deployment. Release-only activation admitted
[run 38083818696](https://github.com/ajbt200128/possums/actions/runs/38083818696).
Exact-main admission and both fresh independent image builds passed; their
manifest digests agreed. The config-only release child is
`73f1a14331d4fed9eb2cc8f3eec0df64f0d4aeb3`, tagged `v0.0.21`.
[Tagged checks](https://github.com/ajbt200128/possums/actions/runs/38084512679)
and [official signed publication](https://github.com/ajbt200128/possums/actions/runs/38084715617)
both passed first attempt.

The parent run failed at its final custom verifier with `RELEASE_MANIFEST`.
The official canonical VM shape includes `disks: 3`; the custom verifier and
synthetic fixture incorrectly expected only CPU, memory and GPU dimensions.
The pinned SDK's shape definition counts root, config, external-config and
per-model disks. This volume/model-free configuration requires three disks.
The correction requires the exact four-field shape with true integers; it
does not ignore extra dimensions or relax config/signature checks. All 43 local
release tests passed, including missing/changed/Boolean/string dimension
rejections. Focused Astra review approved the correction with no blocker.

The corrected local verifier reconciled the **existing immutable** `v0.0.21`:
manifest digest `9b5bea6c99ff816c138c5a91549f80885e128203c5df20b08f5a1f5be24afc47`,
exact config/image, signature/predicate, tagged source/signer, actual signed
attempt, originating independent-build evidence and tag checks all passed.
No tag or release asset was replaced and no publisher was rerun. No production
candidate was dispatched by the failed parent. Both activation switches were
returned to `false` while the correction was prepared. A later merged source
must qualify as a new candidate, not replay the old publisher.

## Remaining boundary at this checkpoint

Hosted deployment-token access to variables, environment policy, secret-name
metadata and approval history is not qualified by successful **local-account**
API reads. Protected approval and one-update production behavior are likewise
pending. No automatic retry, rollback, inference canary or Pi approval update
is authorized by this evidence.

Privacy references: [data boundaries](../PRIVACY.md#data-handling-boundaries),
[forbidden diagnostics/content](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[processor/history boundaries](../PRIVACY.md#processors-retention-access-and-shutdown),
[synthetic testing](../PRIVACY.md#synthetic-testing-exception) and
[evidence](../PRIVACY.md#required-evidence-for-every-telemetry-change).
Only public validated pins and content-free outcomes are recorded here. Raw
control-plane configuration, serving quotes/keys and secret values were not
retained/exported. GitHub's native workflow history and public release artifacts
persist. No telemetry destination, family, retention or runtime privacy rule
changed; live billing/retention residual risks remain unresolved.
