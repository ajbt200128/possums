import { Verifier, type AttestationBundle, type AttestationResponse } from '@tinfoilsh/verifier';
import { X509Certificate, AllOf, Identity as SignerIdentity, GitHubWorkflowSHA,
  OIDCBuildConfigURI, OIDCBuildConfigDigest, OIDCRunInvocationURI,
  GitHubWorkflowRepository, GitHubWorkflowRef, OIDCSourceRepositoryURI,
  OIDCSourceRepositoryDigest, OIDCSourceRepositoryRef, OIDCBuildSignerURI,
  OIDCBuildSignerDigest } from '@freedomofpress/sigstore-browser';
import { LIMITS, Operation, base64, boundedReport, digest, parseJSON, requireThat, DiagnosticFailure, OperationFailure, type FailureConstraint } from './limits.js';

// Independently checked release provenance and directly promoted production endpoint.
// The admin-expiry compiled approval that used to live here was removed: its
// administrative expiry elapsed and it pinned a pre-custom-domain hostname, so a
// shipped client carried a record that could only ever fail closed. Publisher
// authority and the finite 12-hour/certificate trust-session bound below are the
// only trust inputs; discovery never supplies a policy.
export const PUBLISHER = Object.freeze({
  origin: 'https://gateway.possums.dev',
  repository: 'ajbt200128/possums',
  workflow: 'https://github.com/ajbt200128/possums/.github/workflows/tinfoil-release-publish.yml',
});
export type PublishedRelease = Readonly<{ tag: string; expires: number }>;
export function requireSupportedCvmVersion(version: unknown): void {
  // Retain the prior runtime while admitting only the explicitly reviewed upgrade.
  requireThat(version === 'v0.14.12' || version === 'v0.14.16');
}
export function requireReleaseTag(value: unknown): asserts value is string {
  // Stable semantic releases only; discovery never supplies a URL or policy.
  requireThat(typeof value === 'string' && value.length <= 64 && /^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$/.test(value));
}
// Locally distributed denylist; no claim of AMD CRL/OCSP coverage.
const REVOKED_MANIFESTS: ReadonlySet<string> = new Set();
const REVOKED_KEYS: ReadonlySet<string> = new Set();
export function requirePublishedKeys(manifest: string, hpke: string): void {
  requireThat(!REVOKED_MANIFESTS.has(manifest) && !REVOKED_KEYS.has(hpke));
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
export async function qualifyPublished(bundleBytes: Uint8Array, manifestBytes: Uint8Array<ArrayBuffer>, keyConfig: Uint8Array, signal?: AbortSignal) {
  return qualifyApproved(bundleBytes, manifestBytes, keyConfig, signal);
}
async function qualifyApproved(bundleBytes: Uint8Array, manifestBytes: Uint8Array<ArrayBuffer>, keyConfig: Uint8Array, signal?: AbortSignal) {
  const op = new Operation(LIMITS.operationMs, signal);
  let constraint: FailureConstraint = 'manifest';
  try {
    op.check();
    const policy = PUBLISHER;
    const bundle = parseJSON(bundleBytes, LIMITS.bundle) as AttestationBundle;
    const tag = bundle.releaseTag;
    requireThat(typeof tag === 'string' && bundle.domain === new URL(policy.origin).hostname);
    requireReleaseTag(bundle.releaseTag);
    requireThat(/^[0-9a-f]{64}$/.test(bundle.digest) && !REVOKED_MANIFESTS.has(bundle.digest));
    requireThat(manifestBytes.length <= LIMITS.provenance && await digest(manifestBytes) === bundle.digest);
    const manifest = parseJSON(manifestBytes, LIMITS.provenance);
    const config = base64(manifest.config, LIMITS.certificate);
    // The publisher authorizes the WHOLE config (including image), not a YAML
    // substring interpreted as a second policy. Bind its exact bytes to the
    // signed command line; canonical predicate equality below binds all fields.
    requireThat(config.length > 0 && typeof manifest.cmdline === 'string');
    const hashes = manifest.cmdline.split(/\s+/).filter((part: string) => part.startsWith('tinfoil-config-hash='));
    requireThat(hashes.length === 1 && hashes[0] === `tinfoil-config-hash=${await digest(config)}`);
    requireSupportedCvmVersion(manifest.hashes.version);
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
    const workflow = `${PUBLISHER.workflow}@refs/tags/${bundle.releaseTag}`;
    const commit = signer.extSourceRepositoryDigest?.sourceRepositoryDigest;
    const invocation = signer.extRunInvocationURI?.runInvocationURI;
    requireThat(typeof commit === 'string' && /^[0-9a-f]{40}$/.test(commit));
    requireThat(typeof invocation === 'string');
    requireThat(new RegExp(`^https://github\\.com/${PUBLISHER.repository}/actions/runs/[1-9][0-9]*/attempts/[1-9][0-9]*$`).test(invocation));
    await new AllOf([
      new SignerIdentity({ identity: workflow, issuer: 'https://token.actions.githubusercontent.com' }),
      new GitHubWorkflowSHA(commit),
      new OIDCBuildConfigURI(workflow),
      new OIDCBuildConfigDigest(commit),
      new OIDCRunInvocationURI(invocation),
      new GitHubWorkflowRepository(PUBLISHER.repository),
      new GitHubWorkflowRef(`refs/tags/${bundle.releaseTag}`),
      new OIDCSourceRepositoryURI(`https://github.com/${PUBLISHER.repository}`),
      new OIDCSourceRepositoryDigest(commit),
      new OIDCSourceRepositoryRef(`refs/tags/${bundle.releaseTag}`),
      new OIDCBuildSignerURI(workflow),
      new OIDCBuildSignerDigest(commit),
    ]).verify(signer);
    constraint = 'certificate';
    const cert = X509Certificate.parse(bundle.enclaveCert);
    requireThat(cert.validForDate(new Date()));
    const tlsFingerprint = await digest(new Uint8Array(cert.publicKey));
    requireThat(tlsFingerprint === verified.tlsPublicKeyFingerprint);
    requireThat(verified.hpkePublicKey && !REVOKED_KEYS.has(verified.hpkePublicKey));
    constraint = 'key_config';
    validateKeyConfig(keyConfig, verified.hpkePublicKey);
    // Finite local trust-session lifetime, NOT an administrative approval or quote
    // freshness. Certificate SAN verification is not Node TLS socket pinning.
    const expires = Math.min(Date.now() + 12 * 60 * 60 * 1000, cert.notAfter.getTime());
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
