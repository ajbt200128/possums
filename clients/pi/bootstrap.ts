import { API_APPROVALS, PUBLISHER, requireApiApproval, requireReleaseTag, provenanceForTag } from '../../examples/phase01/approval.js';
import { ConnectionFailure, EvidenceObservation, connectionFailure, type EvidenceConstraint, type EvidenceStage } from './diagnostics.js';
import { ReferenceClient } from '../../examples/phase01/client.js';
import { randomBytes } from 'node:crypto';
import { V3_EVIDENCE_LIMIT, sameV3Endpoint } from './v3.js';
import { TorConnection, isTorConnectionFailure, collectTorBody } from './tor-connection.js';
import { decodeTorSetupResponse, TorSetupBackendFailure } from './tor-setup.mjs';
import { LIMITS, DiagnosticFailure, OperationFailure, Operation, base64, boundedReport, cleanup, collect, digest, hex, parseJSON, requireThat } from '../../examples/phase01/limits.js';

// Public acquisition only: callers below construct every destination locally.
// No SDK acquisition retries, credentials, HTTP cache or metadata URLs. Only the
// public manifest download permits one explicit GitHub-to-asset-CDN redirect.
function rateLimited(response: Response): boolean {
  return response.status === 429 || (response.status === 403 && response.headers.get('x-ratelimit-remaining') === '0');
}
async function acquire(url: string, cap: number, op: Operation, stage: EvidenceStage, scope?: TorConnection, torSetup = false): Promise<Uint8Array<ArrayBuffer>> {
  let response: Response | undefined;
  let constraint: EvidenceConstraint = 'request';
  try {
    op.check();
    const request = new Request(url, {
      signal: op.controller.signal, credentials: 'omit', redirect: 'error',
      cache: 'no-store', referrerPolicy: 'no-referrer',
    });
    response = await op.wait(scope ? scope.publicFetch(request, op) : fetch(request), LIMITS.operationMs);
    const httpFailure = response.ok ? undefined : new ConnectionFailure('evidence_unavailable',
      new EvidenceObservation(stage, rateLimited(response) ? 'rate_limited' : 'http', response.status));
    if (httpFailure && !torSetup) throw httpFailure;
    constraint = 'binding';
    requireThat(!response.redirected && response.url === url);
    constraint = 'body';
    if (httpFailure) {
      // Only the fixed Tor setup route may project a closed public diagnostic.
      // A success-shaped error body is never setup authority; unknown/read
      // failures retain the observed HTTP failure, not hostile response text.
      try { decodeTorSetupResponse(await collectTorBody(response.body, cap, op)); }
      catch (error) { if (error instanceof TorSetupBackendFailure) throw error; }
      throw httpFailure;
    }
    return await (scope ? collectTorBody(response.body, cap, op) : collect(response.body, cap, op));
  } catch (error) {
    if (error instanceof ConnectionFailure || isTorConnectionFailure(error)) throw error;
    throw new ConnectionFailure('evidence_unavailable', new EvidenceObservation(stage, error instanceof OperationFailure ? error.constraint : constraint, response?.status));
  }
  finally {
    try { if (response?.body && !response.body.locked) await cleanup(response.body.cancel()); }
    catch { /* Cleanup must not replace the primary acquisition diagnostic. */ }
  }
}

async function downloadManifest(tag: string, op: Operation, scope?: TorConnection): Promise<Uint8Array<ArrayBuffer>> {
  const url = `https://github.com/${PUBLISHER.repository}/releases/download/${tag}/tinfoil-deployment.json`;
  let response: Response | undefined;
  let constraint: EvidenceConstraint = 'request';
  try {
    op.check();
    const request = new Request(url, {
      signal: op.controller.signal, credentials: 'omit', redirect: 'manual',
      cache: 'no-store', referrerPolicy: 'no-referrer',
    });
    response = await op.wait(scope ? scope.publicFetch(request, op) : fetch(request), LIMITS.operationMs);
    constraint = 'binding';
    requireThat(!response.redirected && response.url === url);
    constraint = 'redirect';
    if (response.status !== 302) throw new ConnectionFailure('evidence_unavailable', new EvidenceObservation('manifest_redirect', rateLimited(response) ? 'rate_limited' : 'redirect', response.status));
    const target = new URL(response.headers.get('location') ?? '');
    requireThat(target.protocol === 'https:' && target.hostname === 'release-assets.githubusercontent.com'
      && !target.port && !target.username && !target.password && !target.hash);
    if (response.body) await cleanup(response.body.cancel());
    return await acquire(target.href, LIMITS.provenance, op, 'manifest_download', scope);
  } catch (error) {
    if (error instanceof ConnectionFailure || isTorConnectionFailure(error)) throw error;
    throw new ConnectionFailure('evidence_unavailable', new EvidenceObservation('manifest_redirect', error instanceof OperationFailure ? error.constraint : constraint, response?.status));
  }
  finally { if (response?.body && !response.body.locked) await cleanup(response.body.cancel()); }
}

// Legacy compiled/reference metadata only, attached after SDK verification.
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
  try { return await serving(approval, new Uint8Array(manifest), op); }
  catch (error) { throw connectionFailure(error); }
  finally { op.close(); }
}

// A nonce changes every raw v3 report. Only fully verified stable release and
// endpoint identities can qualify recovery; unchanged witness deadlines cannot
// renew trust. No credential enters this function.
export async function recoverPublished(previous: ReferenceClient, signal?: AbortSignal): Promise<ReferenceClient> {
  const op = new Operation(LIMITS.bootstrapMs, signal);
  try {
    const old = previous.channel.endpointIdentity;
    if (!old || old.onionIdentity !== undefined) throw new ConnectionFailure('verification_failed');
    const candidate = await published(op);
    const next = candidate.channel.endpointIdentity;
    if (!next) throw new ConnectionFailure('verification_failed');
    if (sameV3Endpoint(old, next) && next.freshness <= old.freshness) throw new ConnectionFailure('evidence_unchanged');
    candidate.channel.requireRenewal(previous.channel);
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
    const candidate = await serving(approval, new Uint8Array(manifest), op, report);
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
    return await published(op);
  } catch (error) { throw connectionFailure(error); }
  finally { op.close(); }
}

const torScopes = new WeakMap<ReferenceClient['channel'], TorConnection>();
export async function connectTorPublished(preparedScope: TorConnection, signal?: AbortSignal): Promise<ReferenceClient> {
  TorConnection.requireOwned(preparedScope);
  const op = new Operation(LIMITS.bootstrapMs, signal);
  try { return await published(op, preparedScope); }
  catch (error) { if (isTorConnectionFailure(error)) throw error; throw connectionFailure(error); }
  finally { op.close(); } // Scope remains owned by the caller and serving channel.
}
export async function recoverTorPublished(previous: ReferenceClient, sameScope: TorConnection, signal?: AbortSignal): Promise<ReferenceClient> {
  TorConnection.requireOwned(sameScope);
  const op = new Operation(LIMITS.bootstrapMs, signal);
  try {
    const old = previous.channel.endpointIdentity;
    if (torScopes.get(previous.channel) !== sameScope || !old?.onionIdentity) throw new ConnectionFailure('verification_failed');
    const candidate = await published(op, sameScope);
    const next = candidate.channel.endpointIdentity;
    if (!next || next.onionIdentity !== old.onionIdentity) throw new ConnectionFailure('verification_failed');
    if (sameV3Endpoint(old, next) && next.freshness <= old.freshness) throw new ConnectionFailure('evidence_unchanged');
    candidate.channel.requireRenewal(previous.channel);
    return candidate;
  } catch (error) { if (isTorConnectionFailure(error)) throw error; throw connectionFailure(error); }
  finally { op.close(); }
}

async function published(op: Operation, scope?: TorConnection): Promise<ReferenceClient> {
  // Latest is only untrusted discovery. The exact downloaded bytes and pinned
  // repo/tag/digest are qualified by the offline v3 core inside Channel.v3.
  const bytes = await acquire(`https://api.github.com/repos/${PUBLISHER.repository}/releases/latest`, LIMITS.provenance, op, 'release_discovery', scope);
  let tag: string;
  try { const latest = parseJSON(bytes, LIMITS.provenance); requireReleaseTag(latest.tag_name); tag = latest.tag_name; }
  catch { throw new ConnectionFailure('verification_failed', new EvidenceObservation('release_discovery', 'schema')); }
  const manifest = await downloadManifest(tag, op, scope);
  const nonce = new Uint8Array(randomBytes(32));
  const setup = scope ? decodeTorSetupResponse(await acquire(`${PUBLISHER.origin}/attestation/tor?nonce=${hex(nonce)}`,
    V3_EVIDENCE_LIMIT + 2048, op, 'gateway_attestation', scope, true)) : undefined;
  const evidence = setup?.evidence ?? await acquire(`${PUBLISHER.origin}/.well-known/tinfoil-attestation?nonce=${hex(nonce)}`,
    V3_EVIDENCE_LIMIT, op, 'gateway_attestation');
  const certificateBytes = await acquire(PUBLISHER.origin + '/.well-known/tinfoil-certificate', LIMITS.certificate, op, 'gateway_certificate', scope);
  let certificate: string;
  try {
    const cert = parseJSON(certificateBytes, LIMITS.certificate);
    requireThat(typeof cert?.certificate === 'string' && cert.certificate.length <= LIMITS.certificate);
    certificate = cert.certificate;
  } catch { throw new ConnectionFailure('verification_failed', new EvidenceObservation('gateway_certificate', 'schema')); }
  const config = await acquire(PUBLISHER.origin + '/.well-known/hpke-keys', LIMITS.key, op, 'gateway_keys', scope);
  op.check();
  const client = await ReferenceClient.v3({ evidence, nonce, manifest, tag, certificate, config,
    ...(setup ? { payload: setup.payload, signature: setup.signature } : {}) }, op.controller.signal, scope);
  op.check();
  if (scope) torScopes.set(client.channel, scope);
  // No nonce-bound reports, collateral or verdicts enter the legacy cache.
  return client;
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
async function serving(candidate: Candidate, manifest: Uint8Array<ArrayBuffer>, op: Operation,
  report?: Uint8Array<ArrayBuffer>): Promise<ReferenceClient> {
  let stage: EvidenceStage = 'gateway_attestation';
  try {
    const currentReport = report ?? await acquire(candidate.origin + '/.well-known/tinfoil-attestation', LIMITS.evidence, op, stage);
    const doc = parseJSON(currentReport, LIMITS.evidence);
    requireThat(doc.format === 'https://tinfoil.sh/predicate/sev-snp-guest/v2');
    await boundedReport(base64(doc.body, LIMITS.evidence), op);
    stage = 'amd_certificate';
    const vcek = await amdCertificate(currentReport, op);
    stage = 'gateway_certificate';
    const cert = parseJSON(await acquire(candidate.origin + '/.well-known/tinfoil-certificate', LIMITS.certificate, op, stage), LIMITS.certificate);
    stage = 'release_provenance';
    const provenance = parseJSON(await acquire(`https://api.github.com/repos/${candidate.repository}/attestations/sha256:${candidate.manifest}`, LIMITS.provenance, op, stage), LIMITS.provenance);
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
    const client = await ReferenceClient.verified(bundle, manifest, config);
    op.check();
    identities.set(client.channel, Object.freeze({ report: await digest(currentReport), config: await digest(config) }));
    op.check();
    return client;
  } catch (error) {
    if (error instanceof ConnectionFailure || error instanceof DiagnosticFailure) throw connectionFailure(error);
    throw new ConnectionFailure('verification_failed', new EvidenceObservation(stage, 'schema'));
  }
}
