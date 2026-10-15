# Preserving the existing latest-release setting — source candidate

## Scoped diagnosis

The combined Tor/renewal/fixture source `d44bedb2be1858c1b71d9830329c081882068270` produced the verified public `v0.0.47` release. Protected deployment run `38773633192`, attempt 1, stopped at `RELEASE_PLAN_REJECTED` / `pre-update-plan` before invoking an update. The rollout owner reported no update-success or serving-success receipt. This is not deployment, upstream recovery or Tor-publication acceptance.

One coordinated, bounded diagnosis acquired five read-only Admin API plans using the documented native credential boundary. Authentication and raw control configuration/plans stayed in process memory; only closed boolean/category projections were returned. The existing native credential file was not changed; this does not claim whole-host transience or erase native conversation history. No inference, update, publication rerun, tag/asset replacement, rollback or raw diagnostic export occurred.

The current request reproduced `plan_configuration`: identity, tags, resources, blue/green policy, no hold/downtime, opaque-name preservation and exact first-Tor additions matched; the settings list was exactly the known `mark_latest_release` field. The current setting was a JSON boolean `false`. Omitting the unchanged hostname did not resolve the rejection. The otherwise identical read-only request explicitly preserving `false` produced an empty settings list, preserved the returned flag, and passed the **original unchanged** plan inspector. The pinned official CLI v0.19.0 documents that omitting `--mark-latest` defers to the control-plane default (true), and supports explicit `--mark-latest=true/false`.

This establishes the **currently reproduced planning mechanism**, not definitive attribution of historical v0.0.44/v0.0.47 job payloads. The gate correctly prevented an unapproved configuration change; relaxing its settings allowlist is not the repair.

## Minimal source repair

- Require `type(value) is bool` for the observed current setting before planning or final admission; never substitute a default for missing/null/numeric/string/other values.
- Send that exact snapshot in the read-only API request and supported CLI flag. Preserving `true` preserves its existing latest-release marking effects; the patch does not disable them.
- Independently require a real boolean with the matching snapshot value in **every** parsed pre-call and post-call plan. The settings-change allowlist is unchanged: `mark_latest_release` is not an allowed exception.
- Check completion with its own strict boolean comparison, not ordinary opaque equality (`False == 0`, `True == 1`). Missing/untyped/changed completion metadata remains `configuration_changed`, with an unknown outcome and no replay or rollback.
- Preserve existing provenance, approval, current-main, endpoint/domain, resource, first-Tor secret-reference and publication-exclusivity gates. No automatic retry, extra update invocation, telemetry, credential persistence or deployment-policy expansion is added.

**Non-atomicity:** observing a snapshot, planning and invoking an update are not an atomic control-plane transaction. Carrying the exact observed setting is not new concurrency protection against another operator changing configuration. Current-main/approval rechecks remain the existing immediately-before-invocation gates, not configuration compare-and-swap.

Privacy references: [data handling](../PRIVACY.md#data-handling-boundaries), [forbidden diagnostics/telemetry](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data), [typed failures](../PRIVACY.md#typed-interface-failures-and-token-aggregate-extension), and [verification evidence](../PRIVACY.md#required-evidence-for-every-telemetry-change). This packet adds no logs, raw-error retention, traces, metrics or support bundle. Existing closed diagnostics retain pre-call zero-update versus post-call uncertain-outcome distinctions.

## Offline qualification

The release/deployment safety suite passes **60 synthetic tests** (the complete workflow `test_release*.py` selection passes **81**), including both boolean settings; missing/null/numeric/string/hostile current state; missing/untyped/lying plan flags despite empty settings; multiple CLI reviews; actual control-response decoding with invalid completion flags; one-update/no-replay outcomes; and zero final admission/update on rejected preplans. Existing hostname/Tor/provenance/approval tests remain in the suite.

The pinned `devenv shell` uses Python 3.15.0a7. Its default `SOURCE_DATE_EPOCH` makes an unrelated existing ZIP-artifact fixture fail in the standard library. The 60-test qualification unsets that variable **only for the test process**; no ZIP fixture or release code was changed to hide the limitation. Fresh Linux CI is separately required before merge coordination.

Full locked all-target/all-feature Rust regression: **565 passed, four live probes ignored**, telemetry disabled. Strict package Clippy, formatting, diff checks and active LSP probes on both Python files passed. A fresh independent Sol implementation review found no blocker, including the separate typed completion invariant and uncertain-outcome handling; it made no live calls or duplicate test runs.

Source/offline review is not a production image, installation, native CLI update, successful mutation, serving, upstream renewal or Tor exclusivity qualification. The sole rollout owner remains paused; no merge or new deployment is authorized by this record.
