# Gateway custom domain: backend provisioning

## Completed live changes

Operator authorized backend-first provisioning of `gateway.possums.dev`.
The existing Tinfoil CLI organization login registered this custom domain.
Cloudflare DNS records were created in the active `possums.dev` zone:

| Type | Name | Value |
| --- | --- | --- |
| CNAME | `gateway.possums.dev` | `18d472f3c2667c7dca25564a8cf636ec.tf-dns.com` |
| TXT | `_tinfoil.gateway.possums.dev` | Tinfoil-issued public ownership-verification value |

Both records use a 300-second TTL. Cloudflare returned `proxied: false`;
CNAME flattening is disabled for this record. The TXT value is deliberately
not duplicated here; retain the live record for continued domain verification.

Evidence:

- Cloudflare reported successful creation of both records (HTTP 200).
- Local DNS lookup returned the expected CNAME and ownership TXT.
- `tinfoil domain verify gateway.possums.dev --output json` returned
  `verified: true`, `cname_configured: true`, and `used_by: []`.
- A subsequent read of `possum-phase0` returned `running`, `current_tag:
  v0.0.34`, no pending update, and both `domain` and `internal_domain` equal
  to `possum-phase0.possums.containers.tinfoil.dev`.

Domain verification proves DNS ownership/readiness, **not** successful gateway
serving, TLS provisioning, attestation, or inference at the new hostname.
At the DNS-provisioning checkpoint no container update, restart, release
publication, client change/installation, credential-bearing gateway probe, or
inference request had been performed. The subsequent attachment attempt is
recorded below.

## Initial attachment compatibility concern

The documented Tinfoil mechanism attaches a verified domain using a container
create/deploy/update operation's `--custom-domain` option. The reviewed public
documentation does not establish that changing an existing instance's domain
preserves its default hostname's verified connection contract.

That distinction matters: `examples/phase01/approval.ts` requires the returned
attestation bundle's domain to exactly match the pinned origin. Pi bootstrap
also supplies the selected hostname to serving-certificate verification.
DNS resolution or HTTP availability alone is insufficient old-client evidence.
The ordinary production deployment wrapper treats `domain` as stable state;
do not silently relax that invariant or use an ordinary image update to rename
production.

The initial safe-migration plan was:

1. Establish supported simultaneous default/custom-host routing and endpoint
   binding through Tinfoil documentation, provider confirmation, or an
   explicitly authorized isolated qualification environment. Do not create
   another paid instance without approval.
2. Exercise the previously shipped Pi artifact or exact pinned decoder against
   candidate old-host evidence as well as the current client. Cover identity,
   endpoint keys, authentication/catalog, streams and terminal receipts as
   applicable; disclose any artifact or coverage gap.
3. Prepare a narrowly scoped, reviewed domain-only rollout that preserves all
   other configuration and the verified release. Identify restart/session and
   in-flight accounting implications before invoking an update.
4. Attach only when compatibility is demonstrated; otherwise obtain explicit
   approval for a breaking migration with qualified client activation and a
   staged rollback plan. Do not weaken hostname checks or use redirects/proxy
   termination as a workaround.
5. Verify public TLS, signed release identity and hostname/key binding on both
   endpoints without inference first. Credential-bearing or paid probes require
   separate authorization. Preserve the recorded native CLI v3 platform-signer
   mismatch and legacy-v2 freshness limits; this DNS work resolves neither.

## Operator-authorized breaking-cutover attempt

The operator subsequently explicitly accepted breaking-cutover risk if Tinfoil's
docs did not establish dual-host support. The custom-domain and update docs were
rechecked; they did not establish preserved verified access at the old hostname.
This approval accepts compatibility risk, not weakened attestation, accounting,
automatic inference replay, or automatic mutation retry.

Preflight observed the existing confidential, non-debug, CPU-only, volume-free
instance running `v0.0.34` with no pending update. A fresh domain verification
returned `verified: true` and `cname_configured: true`. Domain listing returns
`cname_configured: null`; use verification's live result, not that nullable
listing field, as DNS-readiness evidence. One initial preflight stopped before
any container mutation for this reason.

One container mutation was then attempted using the installed official CLI
`0.18.9`'s supported `container relaunch` command, setting only the current tag,
new custom domain, `promote-release=false`, and `staging=false`. Its versioned
source was inspected to confirm omitted variables, secrets, SSH keys and other
settings are not sent. The mutation command returned nonzero; its underlying
cause and acceptance outcome were not established. No raw CLI error was printed
or retained, and no mutation was replayed or automatically rolled back.

Bounded control-plane readback over approximately four minutes consistently
reported the original hostname, `running`, `v0.0.34`, and no pending update.
Selected opaque configuration (identity, repo, CPU/GPU/RAM, variables, secret
references, SSH keys, confidential/debug settings, host and volume fields)
matched the in-memory preflight snapshot. This is evidence of observed state,
not proof that the failed command had no side effects.

A scratch serving verifier with only its fixed hostname changed to the new
hostname passed the existing focused Go tests. Its public check used the pinned
SDK and `v0.0.34` manifest digest
`9e7599a175f8f299075e57b91c197e3ea1fa5bc2ef77affe4dff86f304f29f3e`.
It returned `SERVING_SDK_FETCH`; serving at the new hostname was **not accepted**.
No inference or account-credential-bearing gateway request was sent. The
repository serving verifier's hostname was restored to the still-observed old
hostname so ordinary rollout verification does not prematurely target the new
endpoint. Pi remains unchanged.

The operator subsequently authorized preparing a reviewed PR and protected
rollout, alongside a separately reviewed DeepSeek client source patch. Fresh
readback again confirmed the original hostname running `v0.0.34`, confidential
mode enabled, debug disabled and no pending update; domain verification confirmed
ready and unused. The earlier command's cause remains unknown. This is a new
operator-authorized protected operation, not an automatic replay of the old
command. A fresh enclave may discard memory-only sessions/in-flight state;
delivery failure alone does not prove refunds, cancellation or safe replay.

## Protected rollout source candidate

The backend candidate changes only deployment policy, serving verification,
focused offline tests and documentation:

- `scripts/deploy.py` passes `--custom-domain gateway.possums.dev` to its existing
  official checksum-pinned CLI `0.19.0` update. It accepts only the known old/new
  production source hostnames and requires the new hostname at completion.
- A read-only Admin API plan is validated before invocation, including instance
  identity, current resources and the same approved target/configuration
  constraints as the post-call CLI plan. The initial plans must report only
  `custom_domain` in changed settings;
  subsequent releases at the new hostname must report no setting changes. All
  other existing opaque configuration comparisons and release/current-main,
  protected environment, approval, no-replay and no-downtime gates remain.
- `scripts/verify-serving/main.go` qualifies the new hostname through the same
  pinned SDK appraisal, witness freshness and TLS-key-bound public request.
- No measured gateway application or `tinfoil-config.yml` change is necessary for
  the hostname itself. This repository's normal reviewed current-main release
  path remains the rollout mechanism; no local admin mutation is repeated.

Independent review found no confirmed defect but identified unverified spelling
of the server's hostname-change plan label. A credentialed local request to the
documented read-only plan route returned HTTP 403; no deployment was submitted,
and neither a missing permission nor another root cause is inferred from that
status. The existing saved CLI login was consumed in memory for this fixed-host,
TLS-protected request and was not printed, copied to CI, or persisted elsewhere.

The candidate resolves this admission gap by acquiring and validating the
read-only plan using the existing production environment credential after
approval, before the one CLI mutation. HTTP failures retain their observed
status through cleanup; transport and plan validation fail closed in separate
content-free categories. The request uses fixed HTTPS control-plane routing,
no proxies/redirects, bounded response reading and no automatic retry. The CLI
also retains its own planning and post-call review. Unsupported setting spelling
stops **before** mutation; never widen it from an exception string or replay.
This is not transactional head/configuration admission or a reservation: main
and policy are rechecked immediately before invocation, and CLI closed stdin
and absence of `--yes` still refuse downtime/changed-review confirmation.

Offline backend qualification: `python3 -m unittest discover -s tests -p
 'test_release*.py'` passed 63 tests; the focused Go serving-verifier tests passed.
Fixtures cover the exact old-to-new transition, unapproved/missing source
hostnames, extra/duplicate/wrong plan settings, unchanged old-host terminal
state, hostile post-update hostname, and unchanged one-mutation/current-main,
configuration and secret/privacy contracts. A second independent review identified
that unknown change families could be ignored. The guard now rejects unsupported
families before mutation and in post-call review, while retaining the documented
optional `secrets_refreshed` metadata. Regression fixtures exercise both cases.
Live new-host serving has not passed
at this checkpoint. Client implementation/review and installation are separate.

Rollout order: review both source packets, pass required PR CI, merge, verify the
exact successful current-main candidate and public release pins, approve the
matching protected production environment/run, then inspect observed deployment
and credential-free new-host serving outcomes. No paid canary is included.

If serving fails after an update, report deployed state separately from failed
serving acceptance. Do not rerun the mutating job or automatically roll back.
Reconcile pending/terminal state and request a separately approved rollback.
Tinfoil documents clearing `custom_domain` with an empty string to restore the
auto-generated hostname; that rollback requires a reviewed protected policy
change because this candidate intentionally enforces the new hostname. An image
rollback alone does not revert per-instance domain configuration. Retain the DNS
records during reconciliation. Existing clients may fail closed after the
approved breaking cutover; never relax their verification as recovery.

## Privacy scope

Reviewed [PRIVACY.md mandatory change control](../PRIVACY.md#mandatory-reference-and-change-control),
[data handling boundaries](../PRIVACY.md#data-handling-boundaries), and
[telemetry restrictions](../PRIVACY.md#telemetry-permitted-signals-and-forbidden-data).
Only public DNS/control-plane provisioning metadata is recorded here. Existing
CLI credentials were not printed/exported or copied to CI; the existing login
was read only in memory for the separately documented read-only Admin API plan
check. CLI/API control-plane responses and errors were captured in memory and
reduced to selected fields/local categories.
No diagnostic logs, telemetry, collector, request tracing or support bundle was
added. Native host/tool conversation persistence remains a separate boundary;
this is not a whole-process transience claim. DNS-only configuration avoids
adding Cloudflare HTTP/TLS termination but does not prove absence of network
metadata collection or prevent timing correlation.

No application privacy tests were run: application and telemetry code were not
changed. A domain-only deployment operation was attempted, but successful
attachment was not established. Focused serving-verifier tests passed for the
scratch new-host candidate; the live serving check failed as recorded above.
DNS settings and Tinfoil verification are scoped live evidence, not a substitute
for runtime/client qualification.

## Sources

- [Tinfoil custom domains](https://docs.tinfoil.sh/containers/custom-domains)
- [Tinfoil CLI lifecycle and custom domains](https://docs.tinfoil.sh/containers/cli)
- [Tinfoil Admin API](https://docs.tinfoil.sh/admin/admin-api)
