import { Verifier, type AttestationBundle, type AttestationResponse } from '@tinfoilsh/verifier';
import { X509Certificate, AllOf, Identity as SignerIdentity, GitHubWorkflowSHA,
  OIDCBuildConfigURI, OIDCBuildConfigDigest, OIDCRunInvocationURI } from '@freedomofpress/sigstore-browser';
import { LIMITS, Operation, base64, boundedReport, decode, digest, parseJSON, requireThat, ChannelError } from './limits.js';

// Independent v0.0.8 WEB-only approval, never authorization for changed API code.
export const WEB_APPROVAL = Object.freeze({
  origin: 'https://possum-phase0.possums.containers.tinfoil.dev',
  repository: 'ajbt200128/possums', tag: 'v0.0.8',
  manifest: '7363e7637ccbb84c2b243baee91ad08a65245feef1483e8b39d1111e5f65daae',
  image: 'sha256:2a4694aa952e9102eb1786a5f7940633ba8b9cd6c5ecb8983797db1268c4a51e',
  commit: '2af94f852d9467db33f473ae38b6333412342571',
  config: '483d8cf1d9aa2491fbb0c57926bc24ae3e7cf9a62f0c51fac1d6bf0acb15102c',
  workflow: 'https://github.com/ajbt200128/possums/.github/workflows/tinfoil-release-publish.yml@refs/tags/v0.0.8',
  invocation: 'https://github.com/ajbt200128/possums/actions/runs/36822521244/attempts/1',
  // Administrative qualification expiry, NOT v2 quote freshness.
  expires: Date.parse('2026-10-08T00:00:00Z'),
});
export type Approval = { origin: string; repository: string; tag: string; manifest: string;
  image: string; commit: string; config: string; workflow: string; invocation: string; expires: number };
// Independently checked release provenance and directly promoted production endpoint.
// Administrative expiry is NOT v2 quote freshness or Phase 0.2 acceptance.
export const API_APPROVALS: readonly Approval[] = Object.freeze([Object.freeze({
  origin: 'https://possum-phase0.possums.containers.tinfoil.dev',
  repository: 'ajbt200128/possums', tag: 'v0.0.13',
  manifest: 'd92ef5447f894f395c6b9b8dbfcfa97764d82a408cc88d604815446fc70d23f9',
  image: 'sha256:d3d6bbbc5f856fd525792deeec1dfdcf3f717daba6004bf1d7a57cd198a8222e',
  commit: 'ca4de91aeca61d1b7ac8501db27b8fed357ba444',
  config: 'def47fe4df15aebb4f48b8caa5af8882a6bd4a3f96efc52a472cb4efc7dd1134',
  workflow: 'https://github.com/ajbt200128/possums/.github/workflows/tinfoil-release-publish.yml@refs/tags/v0.0.13',
  invocation: 'https://github.com/ajbt200128/possums/actions/runs/37834605451/attempts/1',
  expires: Date.parse('2026-10-11T00:00:00Z'),
})]);
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
  requireThat(Number.isFinite(now) && Number.isFinite(expires) && now < expires && !revoked);
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
export type WebQualification = Readonly<{ scope: 'web-observation-only'; hpkeKey: string; tlsFingerprint: string }>;
// Bytes, not caller-supplied verified flags. All library input is locally bounded first.
export async function qualifyWeb(bundleBytes: Uint8Array, manifestBytes: Uint8Array<ArrayBuffer>, keyConfig: Uint8Array): Promise<WebQualification> {
  const keys = await qualifyApproved(bundleBytes, manifestBytes, keyConfig, WEB_APPROVAL);
  return Object.freeze({ scope: 'web-observation-only', ...keys });
}
export async function qualifyApi(bundleBytes: Uint8Array, manifestBytes: Uint8Array<ArrayBuffer>, keyConfig: Uint8Array) {
  return qualifyApproved(bundleBytes, manifestBytes, keyConfig, requireApiApproval());
}
async function qualifyApproved(bundleBytes: Uint8Array, manifestBytes: Uint8Array<ArrayBuffer>, keyConfig: Uint8Array, approval: Approval) {
  const op = new Operation();
  try {
    checkApproval(Date.now(), approval.expires, REVOKED_MANIFESTS.has(approval.manifest));
    const bundle = parseJSON(bundleBytes, LIMITS.bundle) as AttestationBundle;
    requireThat(bundle.domain === new URL(approval.origin).hostname && bundle.digest === approval.manifest && bundle.releaseTag === approval.tag);
    requireThat(manifestBytes.length <= LIMITS.provenance && await digest(manifestBytes) === approval.manifest);
    const manifest = parseJSON(manifestBytes, LIMITS.provenance);
    const config = base64(manifest.config, LIMITS.certificate);
    requireThat(await digest(config) === approval.config);
    requireThat(decode(config).includes(`    image: ghcr.io/${approval.repository}-gateway@${approval.image}\n`));
    requireThat(manifest.hashes.version === 'v0.14.12');
    const doc = bundle.enclaveAttestationReport;
    requireThat(doc.format === 'https://tinfoil.sh/predicate/sev-snp-guest/v2');
    // The published verifier collects decompression output internally. Preflight the
    // same immutable bytes to an exact SNP report first, excluding gzip bombs/trailing data.
    await boundedReport(base64(doc.body, LIMITS.evidence), op);
    const vcek = base64(bundle.vcek, LIMITS.certificate);
    requireThat(vcek.length > 0 && typeof bundle.enclaveCert === 'string' && bundle.enclaveCert.length <= LIMITS.certificate);
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
    const verifier = new Verifier({ configRepo: approval.repository });
    const verified = await op.wait<AttestationResponse>(verifier.verifyBundle(bundle), LIMITS.operationMs);
    const signer = X509Certificate.parse(base64(signatureBundle.verificationMaterial.certificate.rawBytes, LIMITS.certificate));
    await new AllOf([
      new SignerIdentity({ identity: approval.workflow, issuer: 'https://token.actions.githubusercontent.com' }),
      new GitHubWorkflowSHA(approval.commit),
      new OIDCBuildConfigURI(approval.workflow),
      new OIDCBuildConfigDigest(approval.commit),
      new OIDCRunInvocationURI(approval.invocation),
    ]).verify(signer);
    const cert = X509Certificate.parse(bundle.enclaveCert);
    requireThat(cert.validForDate(new Date()));
    requireThat(await digest(new Uint8Array(cert.publicKey)) === verified.tlsPublicKeyFingerprint);
    requireThat(verified.hpkePublicKey && !REVOKED_KEYS.has(verified.hpkePublicKey));
    validateKeyConfig(keyConfig, verified.hpkePublicKey);
    return Object.freeze({ hpkeKey: verified.hpkePublicKey, tlsFingerprint: verified.tlsPublicKeyFingerprint! });
  } catch { throw new ChannelError(); }
  finally { op.close(); }
}
