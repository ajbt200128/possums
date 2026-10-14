import { API_APPROVALS, PUBLISHER, requireApiApproval, requireReleaseTag, provenanceForTag } from '../../examples/phase01/approval.js';
import { ConnectionFailure, EvidenceObservation, connectionFailure, type EvidenceConstraint, type EvidenceStage } from './diagnostics.js';
import { ReferenceClient } from '../../examples/phase01/client.js';
import { publicEvidence } from './evidence-cache.js';
import { LIMITS, DiagnosticFailure, OperationFailure, Operation, base64, boundedReport, cleanup, collect, digest, hex, parseJSON, requireThat } from '../../examples/phase01/limits.js';

// Public acquisition only: callers below construct every destination locally.
// No SDK acquisition retries, credentials, HTTP cache or metadata URLs. Only the
// public manifest download permits one explicit GitHub-to-asset-CDN redirect.
function rateLimited(response: Response): boolean {
  return response.status === 429 || (response.status === 403 && response.headers.get('x-ratelimit-remaining') === '0');
}
async function acquire(url: string, cap: number, op: Operation, stage: EvidenceStage): Promise<Uint8Array<ArrayBuffer>> {
  let response: Response | undefined;
  let constraint: EvidenceConstraint = 'request';
  try {
    op.check();
    response = await op.wait(fetch(new Request(url, {
      signal: op.controller.signal, credentials: 'omit', redirect: 'error',
      cache: 'no-store', referrerPolicy: 'no-referrer',
    })), LIMITS.operationMs);
    if (!response.ok) throw new ConnectionFailure('evidence_unavailable', new EvidenceObservation(stage, rateLimited(response) ? 'rate_limited' : 'http', response.status));
    constraint = 'binding';
    requireThat(!response.redirected && response.url === url);
    constraint = 'body';
    return await collect(response.body, cap, op);
  } catch (error) {
    if (error instanceof ConnectionFailure) throw error;
    throw new ConnectionFailure('evidence_unavailable', new EvidenceObservation(stage, error instanceof OperationFailure ? error.constraint : constraint, response?.status));
  }
  finally {
    if (response?.body && !response.body.locked) await cleanup(response.body.cancel());
  }
}

async function downloadManifest(tag: string, op: Operation): Promise<Uint8Array<ArrayBuffer>> {
  const url = `https://github.com/${PUBLISHER.repository}/releases/download/${tag}/tinfoil-deployment.json`;
  let response: Response | undefined;
  let constraint: EvidenceConstraint = 'request';
  try {
    op.check();
    response = await op.wait(fetch(new Request(url, {
      signal: op.controller.signal, credentials: 'omit', redirect: 'manual',
      cache: 'no-store', referrerPolicy: 'no-referrer',
    })), LIMITS.operationMs);
    constraint = 'binding';
    requireThat(!response.redirected && response.url === url);
    constraint = 'redirect';
    if (response.status !== 302) throw new ConnectionFailure('evidence_unavailable', new EvidenceObservation('manifest_redirect', rateLimited(response) ? 'rate_limited' : 'redirect', response.status));
    const target = new URL(response.headers.get('location') ?? '');
    requireThat(target.protocol === 'https:' && target.hostname === 'release-assets.githubusercontent.com'
      && !target.port && !target.username && !target.password && !target.hash);
    if (response.body) await cleanup(response.body.cancel());
    return await acquire(target.href, LIMITS.provenance, op, 'manifest_download');
  } catch (error) {
    if (error instanceof ConnectionFailure) throw error;
    throw new ConnectionFailure('evidence_unavailable', new EvidenceObservation('manifest_redirect', error instanceof OperationFailure ? error.constraint : constraint, response?.status));
  }
  finally { if (response?.body && !response.body.locked) await cleanup(response.body.cancel()); }
}

// Metadata is attached only after the same SDK has verified the serving channel.
// The pinned EHBP 0.3.2 public configuration is 41 wire bytes (one format
// byte plus the 40-byte key configuration); hash the exact verified wire value.
const identities = new WeakMap<ReferenceClient['channel'], Readonly<{ report: string; config: string }>>();
function changed(previous: ReferenceClient, candidate: ReferenceClient): boolean {
  const old = identities.get(previous.channel), next = identities.get(candidate.channel);
  if (!old || !next) throw new ConnectionFailure('verification_failed');
  return old.report !== next.report || old.config !== next.config;
}
async function probe(origin: string, baseline: Readonly<{ report: string; config: string }>, op: Operation): Promise<Uint8Array<ArrayBuffer>> {
  const report = await acquire(origin + '/.well-known/tinfoil-attestation', LIMITS.evidence, op, 'gateway_attestation');
  const config = await acquire(origin + '/.well-known/hpke-keys', LIMITS.key, op, 'gateway_keys');
  if (config.length !== LIMITS.key) throw new ConnectionFailure('verification_failed', new EvidenceObservation('gateway_keys', 'schema'));
  if (await digest(report) === baseline.report && await digest(config) === baseline.config)
    throw new ConnectionFailure('evidence_unchanged');
  return report;
}

// Preserve the independent compiled/reference entry point and its admin expiry.
export async function connect(manifest: Uint8Array<ArrayBuffer>, signal?: AbortSignal): Promise<ReferenceClient> {
  if (API_APPROVALS.length === 1 && Date.now() >= API_APPROVALS[0].expires) throw new ConnectionFailure('approval_expired');
  let approval;
  try { approval = requireApiApproval(); }
  catch { throw new ConnectionFailure('approval_unavailable'); }
  if (manifest.length > LIMITS.provenance || await digest(manifest) !== approval.manifest) throw new ConnectionFailure('manifest_mismatch');
  const op = new Operation(LIMITS.bootstrapMs, signal);
  try { return await serving(approval, new Uint8Array(manifest), op, false); }
  catch (error) { throw connectionFailure(error); }
  finally { op.close(); }
}

// An unchanged public hint is terminal; a changed hint is NEVER approval.
// The full verifier may observe a different report from the probe, so compare
// identities again only after it completes. No credential enters this function.
export async function recoverPublished(previous: ReferenceClient, signal?: AbortSignal): Promise<ReferenceClient> {
  const op = new Operation(LIMITS.bootstrapMs, signal);
  try {
    const baseline = identities.get(previous.channel);
    if (!baseline) throw new ConnectionFailure('verification_failed');
    const report = await probe(PUBLISHER.origin, baseline, op);
    const candidate = await published(report, op);
    if (!changed(previous, candidate)) throw new ConnectionFailure('evidence_unchanged');
    return candidate;
  } catch (error) { throw connectionFailure(error); }
  finally { op.close(); }
}

export async function recoverCompiled(previous: ReferenceClient, manifest: Uint8Array<ArrayBuffer>, signal?: AbortSignal): Promise<ReferenceClient> {
  if (API_APPROVALS.length === 1 && Date.now() >= API_APPROVALS[0].expires) throw new ConnectionFailure('approval_expired');
  let approval;
  try { approval = requireApiApproval(); }
  catch { throw new ConnectionFailure('approval_unavailable'); }
  if (manifest.length > LIMITS.provenance || await digest(manifest) !== approval.manifest) throw new ConnectionFailure('manifest_mismatch');
  const op = new Operation(LIMITS.bootstrapMs, signal);
  try {
    const baseline = identities.get(previous.channel);
    if (!baseline) throw new ConnectionFailure('verification_failed');
    const report = await probe(approval.origin, baseline, op);
    const candidate = await serving(approval, new Uint8Array(manifest), op, false, report);
    if (!changed(previous, candidate)) throw new ConnectionFailure('evidence_unchanged');
    return candidate;
  } catch (error) { throw connectionFailure(error); }
  finally { op.close(); }
}

// Once per new Pi trust session. Latest is discovery, never approval; neither
// target_commitish nor asset URLs nor any caller-supplied policy are consulted.
export async function connectPublished(signal?: AbortSignal): Promise<ReferenceClient> {
  const op = new Operation(LIMITS.bootstrapMs, signal);
  try {
    // Unauthenticated response bytes select evidence only; never trust/freshness.
    const report = await acquire(PUBLISHER.origin + '/.well-known/tinfoil-attestation', LIMITS.evidence, op, 'gateway_attestation');
    return await published(report, op);
  } catch (error) { throw connectionFailure(error); }
  finally { op.close(); }
}

async function published(report: Uint8Array<ArrayBuffer>, op: Operation): Promise<ReferenceClient> {
    return await publicEvidence(await digest(report), op, async legacy => {
      if (legacy) return { ...legacy, vcek: await amdCertificate(report, op) };
      const bytes = await acquire(`https://api.github.com/repos/${PUBLISHER.repository}/releases/latest`, LIMITS.provenance, op, 'release_discovery');
      let latest;
      try { latest = parseJSON(bytes, LIMITS.provenance); requireReleaseTag(latest.tag_name); }
      catch { throw new ConnectionFailure('verification_failed', new EvidenceObservation('release_discovery', 'schema')); }
      const tag = latest.tag_name;
      requireReleaseTag(tag);
      const manifest = await downloadManifest(tag, op);
      const provenance = await acquire(`https://api.github.com/repos/${PUBLISHER.repository}/attestations/sha256:${await digest(manifest)}`, LIMITS.provenance, op, 'release_provenance');
      return { tag, manifest, provenance, vcek: await amdCertificate(report, op) };
    }, async entry => serving({ ...PUBLISHER, tag: entry.tag, manifest: await digest(entry.manifest) }, entry.manifest, op, true, report, entry.provenance, entry.vcek));
}

async function amdCertificate(report: Uint8Array<ArrayBuffer>, op: Operation): Promise<Uint8Array<ArrayBuffer>> {
  try {
    const doc = parseJSON(report, LIMITS.evidence);
    requireThat(doc.format === 'https://tinfoil.sh/predicate/sev-snp-guest/v2');
    const raw = await boundedReport(base64(doc.body, LIMITS.evidence), op);
    // Untrusted routing fields from the CURRENT report, never a cached HWID/TCB.
    const tcb = raw.slice(0x180, 0x188);
    const kds = `https://kdsintf.amd.com/vcek/v1/Genoa/${hex(raw.slice(0x1a0, 0x1e0))}?blSPL=${tcb[0]}&teeSPL=${tcb[1]}&snpSPL=${tcb[6]}&ucodeSPL=${tcb[7]}`;
    return await acquire(kds, LIMITS.certificate, op, 'amd_certificate');
  } catch (error) {
    if (error instanceof ConnectionFailure) throw error;
    throw new ConnectionFailure('verification_failed', new EvidenceObservation('gateway_attestation', error instanceof OperationFailure ? error.constraint : 'schema'));
  }
}

type Candidate = { origin: string; repository: string; tag: string; manifest: string };
async function serving(candidate: Candidate, manifest: Uint8Array<ArrayBuffer>, op: Operation, published: boolean,
  report?: Uint8Array<ArrayBuffer>, publicProvenance?: Uint8Array<ArrayBuffer>, publicVcek?: Uint8Array<ArrayBuffer>): Promise<ReferenceClient> {
  let stage: EvidenceStage = 'gateway_attestation';
  try {
    const currentReport = report ?? await acquire(candidate.origin + '/.well-known/tinfoil-attestation', LIMITS.evidence, op, stage);
    const doc = parseJSON(currentReport, LIMITS.evidence);
    requireThat(doc.format === 'https://tinfoil.sh/predicate/sev-snp-guest/v2');
    await boundedReport(base64(doc.body, LIMITS.evidence), op);
    stage = 'amd_certificate';
    const vcek = publicVcek ?? await amdCertificate(currentReport, op);
    stage = 'gateway_certificate';
    const cert = parseJSON(await acquire(candidate.origin + '/.well-known/tinfoil-certificate', LIMITS.certificate, op, stage), LIMITS.certificate);
    stage = 'release_provenance';
    const provenance = parseJSON(publicProvenance ?? await acquire(`https://api.github.com/repos/${candidate.repository}/attestations/sha256:${candidate.manifest}`, LIMITS.provenance, op, stage), LIMITS.provenance);
    const signatureBundle = await op.wait(provenanceForTag(provenance, candidate.tag), LIMITS.operationMs);
    stage = 'gateway_keys';
    const config = await acquire(candidate.origin + '/.well-known/hpke-keys', LIMITS.key, op, stage);
    const bundle = new TextEncoder().encode(JSON.stringify({
      domain: new URL(candidate.origin).hostname, enclaveAttestationReport: doc,
      enclaveCert: cert.certificate, vcek: btoa(String.fromCharCode(...vcek)),
      digest: candidate.manifest, releaseTag: candidate.tag,
      sigstoreBundle: signatureBundle,
    }));
    requireThat(bundle.length <= LIMITS.bundle);
    op.check();
    const client = published
      ? await ReferenceClient.published(bundle, manifest, config, op.controller.signal)
      : await ReferenceClient.verified(bundle, manifest, config);
    op.check();
    identities.set(client.channel, Object.freeze({ report: await digest(currentReport), config: await digest(config) }));
    op.check();
    return client;
  } catch (error) {
    if (error instanceof ConnectionFailure || error instanceof DiagnosticFailure) throw connectionFailure(error);
    throw new ConnectionFailure('verification_failed', new EvidenceObservation(stage, 'schema'));
  }
}
