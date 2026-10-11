# Authorized gateway sizing fallback

The operator requested 1 CPU / 4096 MiB only if supported, and explicitly
permitted restoring exactly 2 CPUs / 8192 when authoritative platform
configuration or documentation establishes that the smaller shape is unsupported.

## Public contract checked during integration (2026-10-11)

- [Official configuration reference](https://docs.tinfoil.sh/containers/configuration.md),
  **Valid resource values**: CPUs are `2, 4, 8, 16, 32`; memory is
  `8192, 16384, 32768, 65536, 131072, 262144, 524288` MB.
- [Official resource limits](https://docs.tinfoil.sh/containers/limits.md),
  **Resource options**: the smallest CPU tier is `cpus: 2` and the smallest
  memory tier is `memory: 8192` (described as 8 GB).
- Both pages state that CPU/memory/GPU counts are measured configuration values;
  create, deploy and update requests cannot override them.
- Installed CLI **0.18.9** help for `container create`, `deployment update`,
  and `container update` exposes no CPU/memory override or standalone
  nonmutating local-config platform-validation command. No control-plane call,
  credential/config-secret read, login, provisioning or paid probe was made.

The documented valid-value lists exclude 1/4096. This is authoritative public
support-contract evidence, **not an observed control-plane rejection**. The
previous cached official Go decoder/schema fixture accepted 1/4096 statically;
that does not establish platform support. The repository's independent
`release.py` canonical manifest guard requires 2/8192/0 GPUs/3 disks; it is an
internal provenance gate, not independent evidence of platform rejection, and
was not weakened or changed to manufacture support.

## Narrow fallback

Only the two root values are restored: `cpus: 2`, `memory: 8192`. Image,
cvm-version **0.14.12**, routing, runtime restrictions, secret declarations and
telemetry configuration are unchanged. The request commit remains separately
visible in history, followed by this explanatory fallback commit. The final
config equals current main's config; release automation must still rebuild and
verify the forwarding source and paired receipt contract.

No claim of deployed shape, runtime memory headroom, availability, immediate
compute termination or invoice parity follows. Existing native CLI V3 signer
mismatch and Pi legacy-V2 freshness limits remain unresolved. This task stops
before merge/publication/protected approval/deployment.

Privacy references: [data boundaries](../PRIVACY.md#data-handling-boundaries),
[forbidden diagnostic/telemetry data](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
[processors](../PRIVACY.md#processors-retention-access-and-shutdown), and
[evidence](../PRIVACY.md#required-evidence-for-every-telemetry-change).
Only public resource-contract evidence and closed local outcomes are recorded;
no telemetry family, persistence, retention or diagnostic collection changed.
