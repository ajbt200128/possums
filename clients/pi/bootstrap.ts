import { PUBLISHER } from './core/approval.js';
import { ConnectionFailure, EvidenceObservation, connectionFailure, type EvidenceConstraint, type EvidenceStage } from './diagnostics.js';
import { ReferenceClient } from './core/client.js';
import { randomBytes } from 'node:crypto';
import { V3_EVIDENCE_LIMIT, sameV3Endpoint } from './v3.js';
import { verifyV3 } from './verifier.mjs';
import { publicManifest } from './evidence-cache.js';
import { TorConnection, isTorConnectionFailure, collectTorBody } from './tor-connection.js';
import { decodeTorSetupResponse, TorSetupBackendFailure } from './tor-setup.mjs';
import { LIMITS, DiagnosticFailure, OperationFailure, Operation, cleanup, collect, digest, hex, parseJSON, requireThat } from './core/limits.js';

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
    if (response.status !== 302) throw new ConnectionFailure('evidence_unavailable', new EvidenceObservation('manifest_redirect',
      rateLimited(response) ? 'rate_limited' : response.status >= 400 ? 'http' : 'redirect', response.status));
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
  const nonce = new Uint8Array(randomBytes(32));
  const setup = scope ? decodeTorSetupResponse(await acquire(`${PUBLISHER.origin}/attestation/tor?nonce=${hex(nonce)}`,
    V3_EVIDENCE_LIMIT + 2048, op, 'gateway_attestation', scope, true)) : undefined;
  const evidence = setup?.evidence ?? await acquire(`${PUBLISHER.origin}/.well-known/tinfoil-attestation?nonce=${hex(nonce)}`,
    V3_EVIDENCE_LIMIT, op, 'gateway_attestation');
  // Authenticate the serving release with the supported offline verifier before
  // selecting public bytes. These facts never authorize a channel: Channel.v3
  // independently verifies the same fresh evidence AND exact manifest below.
  const release = await op.wait(verifyV3(evidence, nonce, PUBLISHER.repository, op.controller.signal));
  return await publicManifest(release.release_tag, release.release_digest, op,
    () => downloadManifest(release.release_tag, op, scope), async manifest => {
      const certificateBytes = await acquire(PUBLISHER.origin + '/.well-known/tinfoil-certificate', LIMITS.certificate, op, 'gateway_certificate', scope);
      let certificate: string;
      try {
        const cert = parseJSON(certificateBytes, LIMITS.certificate);
        requireThat(typeof cert?.certificate === 'string' && cert.certificate.length <= LIMITS.certificate);
        certificate = cert.certificate;
      } catch { throw new ConnectionFailure('verification_failed', new EvidenceObservation('gateway_certificate', 'schema')); }
      const config = await acquire(PUBLISHER.origin + '/.well-known/hpke-keys', LIMITS.key, op, 'gateway_keys', scope);
      op.check();
      const client = await ReferenceClient.v3({ evidence, nonce, manifest, tag: release.release_tag, certificate, config,
        ...(setup ? { payload: setup.payload, signature: setup.signature } : {}) }, op.controller.signal, scope);
      op.check();
      if (scope) torScopes.set(client.channel, scope);
      return client;
  });
}


