# Retained evidence and one monitored Tor-gated debug attempt

## Authorization and privacy boundary

After the [initial Tor-gated rollout failed](tor-health-grace-debug-verification.md),
the operator explicitly authorized both retained-evidence inspection and one
monitored debug deployment. Scope remains the existing `possums-debug`, repository
`ajbt200128/possums`, v0.0.53-based experimental branch only. This separately
approved clearing that experiment's failed staged candidate before the single
new attempt; it did not authorize automatic retries or production operations.

Read AGENTS and PRIVACY. Applicable sections: [data boundaries](../PRIVACY.md#data-handling-boundaries),
[closed diagnostic signals](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[startup diagnostics](../PRIVACY.md#startup-diagnostics-exception), and
[persistence/processors](../PRIVACY.md#processors-retention-access-and-shutdown).
No gateway/source/health/telemetry policy change, new release/image, inference,
attestation probe, new instance/repository, key provisioning or private-original
key read. Existing settings/references/resources remain preserved; export remains
disabled. The separate attestation investigation was not duplicated or changed.

Private metadata, identifiers, exact timestamps and subprocess streams were held
only in memory; only closed projections left the helpers. No raw logs, Docker
configuration/output, SSH/SDK exceptions, credentials or reports were retained as
support artifacts. Scratch artifacts contain authored scripts/public source and
synthetic fixtures, not private snapshots. Native CLI/OS/OpenSSH known-host pins,
platform retention and Pi/T3's retention of closed messages remain separate
boundaries; no whole-host transience or secure-erasure claim.

## Retained evidence and preparation

Fresh exact selection still found `.2/running`, `.3/failed`. Supported native
exact-ID update status and root metadata exposed none of the checked structured
stage/boot/container/health/error-code fields. No free-form exception was used to
infer a cause. The unavailable old candidate SSH probe was not repeated during
this retained-evidence phase. No supported retained runtime-history endpoint was
established; the old candidate's Docker/startup/grace evidence remained unavailable.

Astra prepared finite, parent-controlled scratch monitoring and synthetic tests.
The parent read the actual control, shell projection, parser, tests and reviewed
public toolbox documents, then reran **26 pinned offline tests**, shell syntax
and diff checks successfully. Current-toolbox installed-document equality and
Docker/clock capability checks passed; these were not new-candidate health proof.

Fresh exact public GitHub signing, manifest/config/image and downloaded archive
checks passed for the unchanged `.3` release and image recorded in the initial
record. These checks were not fresh device attestation or production admission.
The exact health command,600s grace,10s interval,5s timeout and three retries were
not edited; the real current Tor `Running && !fatal` health gate remained enabled
in that signed source. No source rebuild or workflow dispatch occurred.

## One authorized control-plane attempt — failed

The explicit execution entrypoint used a content-free exclusive-create consumed
marker before any lifecycle request. That marker remains consumed and must not
be removed to replay the attempt.

1. Fresh exact `.2/running/.3/failed` snapshot and unchanged settings passed.
2. **One** native exact-ID cancellation, explicitly `rollback-latest=false`,
   returned exit0. Independent readback confirmed `.2/running`, no pending
   update, source SSH and opaque settings/references preserved. This is metadata
   cleanup evidence, not proof of physical VM cleanup or exclusive publication.
3. A fresh read-only plan approved `.3`, blue-green/no downtime/no hold,
   promotion false, unchanged resources/configuration and only the five existing
   secret refresh names. Snapshot equality was checked immediately before POST.
4. **One** update POST was acknowledged. Monitoring began at acknowledgment,
   not after completion. Readback progressed pending → started → **failed**;
   `.2` remained current/running. No accept, rollback, relaunch, direct toolbox
   boot, additional cancellation/update or production/main operation followed.

The root/native final read-only status again confirmed `.3/failed`, with opaque
settings unchanged. This is a second separately authorized failed experiment,
not successful activation of `.3` or an automatic replay of the first request.

## What the monitor actually captured

| Boundary | Observed result |
| --- | --- |
| Fresh public artifact gates/current-toolbox preflight | Passed |
| Failed-stage cancellation/independent metadata readback | Confirmed |
| Planned new update | One acknowledgment, then failed |
| Candidate installed-document SSH | Exit255, then exit0 with exact reviewed hashes |
| Candidate runtime sample | Exit2, later SSH exit255; no validated sample |
| Final current release | `.2/running` |
| Final staged release | `.3/failed` |

The known primary failure remained `CANDIDATE_FAILED` despite later SSH/coverage
faults. The summary reported `health_sequence=unavailable`, coverage gaps and
`never_healthy_proven=false`. Candidate image identity, effective health controls,
Docker starting/healthy/unhealthy transitions, OOM/exit, actual SDK state and
failure timing relative to grace were **not qualified**. The brief successful
document read proves access at that moment, not that gateway creation/start or
any image/control check passed. Exit255's underlying cause remains unknown.

## Confirmed monitoring defect — not a rollout root cause

The original shell sampler compared a combined Docker-template output string
against numeric nanosecond durations. A separate bounded read-only check of the
**old current `.2` reference**, after exact document/image/source checks, found:

- Exact health command, remaining controls and Tor/export switches matched.
- Rendered-string comparisons for start period/interval/timeout failed.
- Typed Docker-template comparisons against600s/10s/5s all passed.

Docker renders these typed durations as duration strings, so the sampler could
reject valid settings. Its synthetic Docker fixture had returned numeric values,
missing this real interface behavior. This is a confirmed instrumentation
false-negative on the reference, **not proof of the candidate's failing guard**:
the original candidate sample collapsed guard failures and retained no specific
stage. It neither proves that the candidate ran nor explains the platform's
rollout failure. The read-only reference check did not mutate/probe the gateway,
initiate inference, or count `.2` as candidate evidence.

Astra corrected only the scratch sampler/parser/tests offline, using typed
comparisons without relaxing any actual health constraint and retaining explicit
closed guard stages. The parent reviewed actual diffs and reran **34 pinned offline
tests** successfully, plus shell syntax checks. Fixtures reproduce duration
Stringer rendering, reject wrong typed durations, cover each closed guard stage,
hostile/partial output, absent records, exact candidate identity and no replay.
The original attempt's scripts and consumed marker were preserved. This correction
cannot retrospectively recover missing runtime observations; no additional attempt
was started under the consumed authorization. A subsequent operator authorization
is required for any newly monitored deployment.

## Remaining decision

The retained structured failure cause remains unavailable. An early successful
health check followed by Tor readiness loss is still a source-grounded possible
mechanism, not observed behavior here. Never reaching strict Running, an earlier
boot failure or an external control-plane deadline also remain unresolved. Neither
attempt establishes that600s expired or that the platform ignored the setting.

Another lifecycle/deployment action requires a new operator decision and fresh
exact selection, accounting for the retained failed staged update. Do not remove
the consumed marker, blindly retry, weaken health/trust checks, or relabel earlier
`.2` Tor reachability as `.3` evidence. Production remains untouched.
