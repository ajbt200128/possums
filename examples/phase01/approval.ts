import { Verifier, type AttestationBundle, type AttestationResponse } from '@tinfoilsh/verifier';
import { X509Certificate, AllOf, Identity as SignerIdentity, GitHubWorkflowSHA,
  OIDCBuildConfigURI, OIDCBuildConfigDigest, OIDCRunInvocationURI,
  GitHubWorkflowRepository, GitHubWorkflowRef, OIDCSourceRepositoryURI,
  OIDCSourceRepositoryDigest, OIDCSourceRepositoryRef, OIDCBuildSignerURI,
  OIDCBuildSignerDigest } from '@freedomofpress/sigstore-browser';
import { LIMITS, Operation, base64, boundedReport, decode, digest, parseJSON, requireThat, DiagnosticFailure, OperationFailure, type FailureConstraint } from './limits.js';

export type Approval = { origin: string; repository: string; tag: string; manifest: string;
  image: string; commit: string; config: string; workflow: string; invocation: string; expires: number };
// Independently checked release provenance and directly promoted production endpoint.
// Administrative expiry is NOT v2 quote freshness or Phase 0.2 acceptance.
export const API_APPROVALS: readonly Approval[] = Object.freeze([Object.freeze({
  origin: 'https://possum-phase0.possums.containers.tinfoil.dev',
  repository: 'ajbt200128/possums', tag: 'v0.0.15',
  manifest: 'cbbd5c5adce7594d7ccee19e96d88b38d096ee3290f880d4c2cf548a2387960f',
  image: 'sha256:a303de37e84e2ec948452d185fc633a1dd50fd6e26187bbdb56677d6819f6c90',
  commit: '30f480a48129c445de3a7dbe9c04ccdf589a67c6',
  config: '73ab4c9bfdc041d39b235458d241d4d8191fb673b7aeb33f0ae69ce36771d465',
  workflow: 'https://github.com/ajbt200128/possums/.github/workflows/tinfoil-release-publish.yml@refs/tags/v0.0.15',
  invocation: 'https://github.com/ajbt200128/possums/actions/runs/37891319005/attempts/1',
  expires: Date.parse('2026-10-11T00:00:00Z'),
})]);
// Future-release authority is independent of the compiled administrative approvals.
export const PUBLISHER = Object.freeze({
  origin: 'https://possum-phase0.possums.containers.tinfoil.dev',
  repository: 'ajbt200128/possums',
  workflow: 'https://github.com/ajbt200128/possums/.github/workflows/tinfoil-release-publish.yml',
});
export type PublishedRelease = Readonly<{ tag: string; expires: number }>;
export function requireReleaseTag(value: unknown): asserts value is string {
  // Stable semantic releases only; discovery never supplies a URL or policy.
  requireThat(typeof value === 'string' && value.length <= 64 && /^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$/.test(value));
}
// Locally distributed denylist; no claim of AMD CRL/OCSP coverage.
const REVOKED_MANIFESTS: ReadonlySet<string> = new Set();
const REVOKED_KEYS: ReadonlySet<string> = new Set();
export function requireApiApproval(): Approval {
  requireThat(API_APPROVALS.length === 1);
  const approval = API_APPROVALS[0];
  checkApproval(Date.now(), approval.expires, REVOKED_MANIFESTS.has(approval.manifest));
  return approval;
}
export function checkApproval(now: number, expires: number, revoked: boolean): void {
  if (Number.isFinite(now) && Number.isFinite(expires) && now >= expires && !revoked) throw new DiagnosticFailure('trust', 'expired');
  if (!(Number.isFinite(now) && Number.isFinite(expires) && now < expires && !revoked)) throw new DiagnosticFailure('trust', 'policy');
}
export function validateKeyConfig(config: Uint8Array, endorsed: string): void {
  requireThat(config.length === 41 && /^[0-9a-f]{64}$/.test(endorsed));
  requireThat(config[0] === 0 && config[1] === 0 && config[2] === 0x20);
  requireThat(config[35] === 0 && config[36] === 4 && config[37] === 0 && config[38] === 1 && config[39] === 0 && config[40] === 2);
  requireThat(Array.from(config.slice(3, 35), b => b.toString(16).padStart(2, '0')).join('') === endorsed);
}
function canonical(value: any): string {
  if (Array.isArray(value)) return '[' + value.map(canonical).join(',') + ']';
  if (value && typeof value === 'object') return '{' + Object.keys(value).sort().map(k => JSON.stringify(k) + ':' + canonical(value[k])).join(',') + '}';
  return JSON.stringify(value);
}
// Bytes, not caller-supplied verified flags. All library input is locally bounded first.
export async function qualifyApi(bundleBytes: Uint8Array, manifestBytes: Uint8Array<ArrayBuffer>, keyConfig: Uint8Array) {
  const keys = await qualifyApproved(bundleBytes, manifestBytes, keyConfig, requireApiApproval());
  return Object.freeze({ hpkeKey: keys.hpkeKey, tlsFingerprint: keys.tlsFingerprint });
}
export async function qualifyPublished(bundleBytes: Uint8Array, manifestBytes: Uint8Array<ArrayBuffer>, keyConfig: Uint8Array, signal?: AbortSignal) {
  return qualifyApproved(bundleBytes, manifestBytes, keyConfig, undefined, signal);
}
async function qualifyApproved(bundleBytes: Uint8Array, manifestBytes: Uint8Array<ArrayBuffer>, keyConfig: Uint8Array, approval?: Approval, signal?: AbortSignal) {
  const op = new Operation(LIMITS.operationMs, signal);
  let constraint: FailureConstraint = 'manifest';
  try {
    op.check();
    if (approval) checkApproval(Date.now(), approval.expires, REVOKED_MANIFESTS.has(approval.manifest));
    const policy = approval ?? PUBLISHER;
    const bundle = parseJSON(bundleBytes, LIMITS.bundle) as AttestationBundle;
    const tag = bundle.releaseTag;
    requireThat(typeof tag === 'string' && bundle.domain === new URL(policy.origin).hostname);
    if (approval) requireThat(bundle.digest === approval.manifest && bundle.releaseTag === approval.tag);
    else {
      requireReleaseTag(bundle.releaseTag);
      requireThat(/^[0-9a-f]{64}$/.test(bundle.digest) && !REVOKED_MANIFESTS.has(bundle.digest));
    }
    requireThat(manifestBytes.length <= LIMITS.provenance && await digest(manifestBytes) === bundle.digest);
    const manifest = parseJSON(manifestBytes, LIMITS.provenance);
    const config = base64(manifest.config, LIMITS.certificate);
    if (approval) {
      requireThat(await digest(config) === approval.config);
      requireThat(decode(config).includes(`    image: ghcr.io/${approval.repository}-gateway@${approval.image}\n`));
    } else {
      // The publisher authorizes the WHOLE config (including image), not a YAML
      // substring interpreted as a second policy. Bind its exact bytes to the
      // signed command line; canonical predicate equality below binds all fields.
      requireThat(config.length > 0 && typeof manifest.cmdline === 'string');
      const hashes = manifest.cmdline.split(/\s+/).filter((part: string) => part.startsWith('tinfoil-config-hash='));
      requireThat(hashes.length === 1 && hashes[0] === `tinfoil-config-hash=${await digest(config)}`);
    }
    requireThat(manifest.hashes.version === 'v0.14.12');
    constraint = 'attestation';
    const doc = bundle.enclaveAttestationReport;
    requireThat(doc.format === 'https://tinfoil.sh/predicate/sev-snp-guest/v2');
    // The published verifier collects decompression output internally. Preflight the
    // same immutable bytes to an exact SNP report first, excluding gzip bombs/trailing data.
    await boundedReport(base64(doc.body, LIMITS.evidence), op);
    const vcek = base64(bundle.vcek, LIMITS.certificate);
    requireThat(vcek.length > 0 && typeof bundle.enclaveCert === 'string' && bundle.enclaveCert.length <= LIMITS.certificate);
    constraint = 'provenance';
    const signatureBundle = bundle.sigstoreBundle as any;
    requireThat(JSON.stringify(signatureBundle).length <= LIMITS.provenance);
    requireThat(signatureBundle.dsseEnvelope.signatures.length === 1);
    requireThat(signatureBundle.verificationMaterial.tlogEntries.length === 1);
    requireThat((signatureBundle.verificationMaterial.tlogEntries[0].inclusionProof?.hashes?.length ?? 0) <= 64);
    const statement = parseJSON(base64(signatureBundle.dsseEnvelope.payload, LIMITS.provenance), LIMITS.provenance);
    requireThat(statement._type === 'https://in-toto.io/Statement/v1' && statement.subject.length === 1 && statement.subject[0].name === 'tinfoil-deployment.json');
    requireThat(canonical(statement.predicate) === canonical(manifest));
    // This is the actual pinned verifier: hardware signature/chain/policy, Sigstore
    // DSSE/Rekor/root/tag, measurement comparison, certificate SAN endorsements.
    constraint = 'verification'; // The SDK combines hardware, provenance and measurement verification.
    const verifier = new Verifier({ configRepo: policy.repository });
    const verified = await op.wait<AttestationResponse>(verifier.verifyBundle(bundle), LIMITS.operationMs);
    constraint = 'publisher';
    const signer = X509Certificate.parse(base64(signatureBundle.verificationMaterial.certificate.rawBytes, LIMITS.certificate));
    // These candidate values acquire authority ONLY through the library's DSSE,
    // certificate-chain and Rekor verification above, never from release metadata.
    const workflow = approval?.workflow ?? `${PUBLISHER.workflow}@refs/tags/${bundle.releaseTag}`;
    const commit = approval?.commit ?? signer.extSourceRepositoryDigest?.sourceRepositoryDigest;
    const invocation = approval?.invocation ?? signer.extRunInvocationURI?.runInvocationURI;
    requireThat(typeof commit === 'string' && /^[0-9a-f]{40}$/.test(commit));
    requireThat(typeof invocation === 'string');
    if (!approval) requireThat(new RegExp(`^https://github\\.com/${PUBLISHER.repository}/actions/runs/[1-9][0-9]*/attempts/[1-9][0-9]*$`).test(invocation));
    await new AllOf([
      new SignerIdentity({ identity: workflow, issuer: 'https://token.actions.githubusercontent.com' }),
      new GitHubWorkflowSHA(commit),
      new OIDCBuildConfigURI(workflow),
      new OIDCBuildConfigDigest(commit),
      new OIDCRunInvocationURI(invocation),
      ...(!approval ? [
        new GitHubWorkflowRepository(PUBLISHER.repository),
        new GitHubWorkflowRef(`refs/tags/${bundle.releaseTag}`),
        new OIDCSourceRepositoryURI(`https://github.com/${PUBLISHER.repository}`),
        new OIDCSourceRepositoryDigest(commit),
        new OIDCSourceRepositoryRef(`refs/tags/${bundle.releaseTag}`),
        new OIDCBuildSignerURI(workflow),
        new OIDCBuildSignerDigest(commit),
      ] : []),
    ]).verify(signer);
    constraint = 'certificate';
    const cert = X509Certificate.parse(bundle.enclaveCert);
    requireThat(cert.validForDate(new Date()));
    const tlsFingerprint = await digest(new Uint8Array(cert.publicKey));
    requireThat(tlsFingerprint === verified.tlsPublicKeyFingerprint);
    requireThat(verified.hpkePublicKey && !REVOKED_KEYS.has(verified.hpkePublicKey));
    constraint = 'key_config';
    validateKeyConfig(keyConfig, verified.hpkePublicKey);
    // Finite local trust-session lifetime, NOT administrative approval or quote
    // freshness. Certificate SAN verification is not Node TLS socket pinning.
    const expires = approval?.expires ?? Math.min(Date.now() + 12 * 60 * 60 * 1000, cert.notAfter.getTime());
    checkApproval(Date.now(), expires, false);
    op.check();
    return Object.freeze({ hpkeKey: verified.hpkePublicKey, tlsFingerprint,
      release: Object.freeze({ tag, expires }) });
  } catch (error) {
    if (error instanceof DiagnosticFailure) throw error;
    throw new DiagnosticFailure('trust', error instanceof OperationFailure ? error.constraint : constraint);
  }
  finally { op.close(); }
}
