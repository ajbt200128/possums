# Configured NSS release-cache boundary followup

> Historical graph/image evidence for the exact sources below. This inactive
> HTTPS-only backup removes Tor graph members without changing the strict
> checker or NSS account recipe. Its fresh partition check passed, but it has
> no new Linux image identity or A/B evidence. See
> [backup verification](without-tor-backup-verification.md).

## Scope and cause

Source S: `bc34561fc8d4670a56e25778d01676d296b95c92` (PR #87).
The parent classified automatic image run `39067029684`, attempt 1, as
`release_cache_boundary_changed` at `release_cache_isolation`, before the fresh
OCI build. This is not an Arti, provider, transport or SDK failure.

Independent local evaluation of the locked `x86_64-linux` recursive derivation
graph confirms the mismatch: **14** image-only derivations versus **9** declared
fresh derivations. The five omitted nodes are:

| Configured NSS stage | Derivation under `/nix/store/` |
| --- | --- |
| join | `7vphass467nhrall0y0qh6zqcgn98i9v-fake-nss.drv` |
| passwd | `zq8mxhnyy1rn9fpk9y8b429v22an8hqh-passwd.drv` |
| group | `lxyh0117yla3xxmjzi28nmncp4x02y9x-group.drv` |
| nsswitch | `i3jm1x9b30l0qmmknkgy9wvnibdpgk1d-nsswitch.conf.drv` |
| empty home directory | `pjcyna39plmvwsw1kavysmw2ha8a03jb-var-empty.drv` |

The pinned fakeNss recipe uses a symlink join of four separately built paths.
Wrapping the join's inputs would prewarm the configured passwd/group outputs.
Instead, inspect the recipe's explicit `paths` using its supported `symlinkJoin`
override, wrap only those component derivations' **inputs**, and declare the join
plus all four components fresh. This is not a graph-derived expected-set escape:
`imageStages`, application outputs and the explicit NSS recipe still determine
the expected set, independently of the recursive graph being checked.

## Graph and identity evidence

On the fixed source, the locked Linux image graph still has 6,798 derivations;
the prewarm graph has 6,802 (previously 6,798). The exact difference is **14**,
equal to the declared set, with no missing/extra member. None of those 14
application/helper/entrypoint/image/NSS nodes is reachable from the prewarm graph.
All **31** selected external `(derivation, output)` pairs directly required by
fresh nodes are reachable from the prewarm recipe. Thus realizing
`release-build-deps` cannot realize any of those application-specific outputs.
These are graph proofs, not an actual Linux prewarm or independent fresh build.
The unchanged strict checker also accepts this graph with modeled cold-store
validity (only external outputs valid).

The complete recursive gateway-image derivation JSON, including every output
identity, is identical before/after and to both PR #87 head
`74b7223c210ba361b0a76962805f5ab764d5395a` and independent qualification head
`2268ffe877408f652ff093017a401a67794376a6`:

| Output | Derivation under `/nix/store/` | Output under `/nix/store/` |
| --- | --- | --- |
| gateway | `4wc68xc91c4521hmxjrcnsj0j3msy8c0-possums-0.1.0.drv` | `zmirswdv9z1lznpabibrvj1rza6bacri-possums-0.1.0` |
| helper | `l8vg2wr88bsq44bda5qlhbn93lrwsksi-possums-attestation-0.1.0.drv` | `x8qs1ca4b1pcz5fsvxs0fc7lnplrvdhg-possums-attestation-0.1.0` |
| entrypoint | `6asz5scca6640cvbwciqwfvb5cprpng5-possums-entrypoint.drv` | `yx33xig68igsj0b23f222wf1mdpdx7pl-possums-entrypoint` |
| image archive | `mdl5xn11zlyschqh83769754d2gjm0mb-possums-gateway.tar.gz.drv` | `6cgzv3aw8996zszbdlsp6cx7sp559vvf-possums-gateway.tar.gz` |

The source filter excludes these boundary/test/workflow/docs edits from the
application source. No gateway account contents, image recipe, SDK, application,
source filter, permissions or runtime behavior changed.

Read-only GitHub metadata confirmed PR CI `39061355628`, attempt 1, all three
jobs successful, and independent Linux qualification `39061607613`, attempt 1,
all four jobs successful. Only exact locally authored image-identity lines under
the full GitHub timestamp grammar were extracted in memory; no raw logs were
retained. The qualified Docker image ID is:

`sha256:fc6f42ba336309bbe6f676feab1d1849cba3e753ef7edaf0050c348d7daf5e27`

This carries forward evidence for an identical image derivation; it does not
claim a new A/B construction, archive-byte comparison, OCI manifest digest or
production acceptance. Normal PR CI must still report its actual image identity.

## Regression checks and limits

- Pinned nixpkgs Python 3.12.12: all 87 release tests passed with the graph check
  enabled; image-health tests 9 passed; Tor-image projection tests 7 passed.
- Opt-in pinned Linux graph suite: all 13 tests passed, including actual recursive
  partition equality and selected external-output reachability without building
  application outputs. Existing `check.yml` now runs it before image builds.
- Synthetic NSS fixture rejects omitted declarations, incorrectly prewarmed
  join/record nodes, and already-realized join/record outputs. Existing strict
  cache checks and hostile-parser/process-error privacy tests remain intact.
- Changed Nix/Python/workflow files: active LSP probes reported no diagnostics;
  `git diff --check` passed.
- Pinned `checks.aarch64-darwin.fmt` passed. The broader offline dev-shell command
  was unavailable/failed; the direct pinned formatting check is the evidence.
- No local Linux build, fresh A/B build, release rerun, tag, publication, production
  mutation, real key, attestation or inference. No substitutes/remote-builder
  policy, expected-graph cap, CLI framework, telemetry or logging change.

Privacy references: [data boundaries](../PRIVACY.md#data-handling-boundaries),
[forbidden raw errors and identifiers](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data),
and [typed failures](../PRIVACY.md#typed-interface-failures-and-token-aggregate-extension).
Only public derivation identities, public check states and closed image-identity
projections are recorded. GitHub retains its native workflow history.

Parent review and fresh independent review are required before merge. After merge,
the parent independently qualifies normal main CI, fresh A/B, sole-child R/tag,
signed publication and fresh protected admission; none is authorized by this note.
