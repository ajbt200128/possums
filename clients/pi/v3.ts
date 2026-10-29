import { createHash, X509Certificate } from 'node:crypto';
import { Agent, request as httpsRequest } from 'node:https';
import { Buffer } from 'node:buffer';
import type { IncomingMessage } from 'node:http';
import { connect as tlsConnect, type TLSSocket } from 'node:tls';
import { Readable, PassThrough, type Duplex } from 'node:stream';
import { createGunzip, createInflate, createBrotliDecompress } from 'node:zlib';
import { verifyV3, verifyTorV3, VerifierFailure } from './verifier.mjs';
import { TorConnection, isTorConnectionFailure, TorNativeTransportFailure } from './tor-connection.js';
import { checkTorEndorsementSignature } from './tor-endorsement.mjs';
import { PUBLISHER, checkApproval, validateKeyConfig, requireReleaseTag, requirePublishedKeys, requireSupportedCvmVersion } from './core/approval.js';
import { LIMITS, DiagnosticFailure, Operation, OperationFailure, base64, digest, parseJSON, requireThat } from './core/limits.js';

export const V3_EVIDENCE_LIMIT = 16 * 1024 * 1024;
export type V3Input = { evidence: Uint8Array; nonce: Uint8Array; manifest: Uint8Array<ArrayBuffer>;
  tag: string; certificate: string; config: Uint8Array };
export type TorV3Input = V3Input & { payload: Uint8Array; signature: Uint8Array };
export type V3Identity = Readonly<{ tag: string; digest: string; tls: string; config: string; freshness: number; onionIdentity?: string }>;
const spki = (cert: X509Certificate) => createHash('sha256').update(cert.publicKey.export({ type: 'spki', format: 'der' })).digest('hex');
function certificate(bytes: string | Buffer, host: string, fingerprint: string, peer = false): X509Certificate {
  let cert: X509Certificate, actual: string;
  try {
    cert = new X509Certificate(bytes);
    requireThat(cert.checkHost(host, { subject: 'never' }) !== undefined);
    const now = Date.now();
    requireThat(now >= Date.parse(cert.validFrom) && now < Date.parse(cert.validTo));
    actual = spki(cert);
  } catch { throw new DiagnosticFailure('trust', 'certificate'); }
  // Only a real, otherwise valid TLS peer can nominate pre-dispatch recovery.
  // Public certificate/evidence mismatches remain terminal verification errors.
  if (actual !== fingerprint) throw new DiagnosticFailure('trust', peer ? 'peer_key_mismatch' : 'certificate');
  return cert;
}
export async function qualifyV3(input: V3Input, signal?: AbortSignal) {
  return qualify(input, signal);
}
export async function qualifyTorV3(input: TorV3Input, scope: TorConnection, signal?: AbortSignal) {
  TorConnection.requireOwned(scope);
  if (!(input && input.payload instanceof Uint8Array && input.payload.length === 95
    && input.signature instanceof Uint8Array && input.signature.length === 64)) throw new DiagnosticFailure('trust', 'schema');
  const tor = { scope, onion: scope.onionIdentity, payload: new Uint8Array(input.payload), signature: new Uint8Array(input.signature) };
  // One controlled official Tor projection supplies BOTH A and the normal v3
  // release/TLS/HPKE/freshness authority. Standalone facts cannot enter here.
  return qualify(input, signal, tor);
}
async function qualify(input: V3Input, signal?: AbortSignal,
  tor?: { scope: TorConnection; onion: Uint8Array; payload: Uint8Array; signature: Uint8Array }) {
  let constraint: 'manifest' | 'verification' | 'key_config' = 'manifest';
  try {
    input = { ...input, evidence: new Uint8Array(input.evidence), nonce: new Uint8Array(input.nonce),
      manifest: new Uint8Array(input.manifest), config: new Uint8Array(input.config) };
    requireReleaseTag(input.tag);
    const hash = await digest(input.manifest);
    const manifest = parseJSON(input.manifest, LIMITS.provenance);
    const config = base64(manifest.config, LIMITS.certificate);
    requireSupportedCvmVersion(manifest.hashes.version);
    requireThat(config.length > 0 && typeof manifest.cmdline === 'string');
    const hashes = manifest.cmdline.split(/\s+/).filter((part: string) => part.startsWith('tinfoil-config-hash='));
    requireThat(hashes.length === 1 && hashes[0] === `tinfoil-config-hash=${await digest(config)}`);
    constraint = 'verification';
    const facts = tor ? await verifyTorV3(input.evidence, input.nonce, input.manifest, PUBLISHER.repository, signal)
      : await verifyV3(input.evidence, input.nonce,
        `${PUBLISHER.repository}@${input.tag}@sha256:${hash}`, signal, input.manifest);
    if (tor) {
      requireThat('tor_endorsement_key' in facts && typeof facts.tor_endorsement_key === 'string');
      await checkTorEndorsementSignature(Buffer.from(facts.tor_endorsement_key, 'hex'), input.nonce,
        tor.onion, tor.payload, tor.signature);
    }
    requireThat(facts.release_tag === input.tag && facts.release_digest === hash);
    requirePublishedKeys(hash, facts.hpke_key);
    const cert = certificate(input.certificate, new URL(PUBLISHER.origin).hostname, facts.tls_key_sha256);
    constraint = 'key_config';
    validateKeyConfig(input.config, facts.hpke_key);
    const freshness = facts.freshness_expires_at_unix * 1000;
    const expires = Math.min(Date.now() + 12 * 60 * 60 * 1000, Date.parse(cert.validTo), freshness);
    const check = () => { tor?.scope.check(); checkApproval(Date.now(), expires, false); };
    check(); requireThat(!signal?.aborted);
    const identity: V3Identity = Object.freeze({ tag: input.tag, digest: hash, tls: facts.tls_key_sha256,
      config: await digest(new Uint8Array(input.config)), freshness,
      ...(tor ? { onionIdentity: Buffer.from(tor.onion).toString('hex') } : {}) });
    return { release: Object.freeze({ tag: input.tag, expires }), identity, check,
      send: (req: Request, op: Operation) => {
        try { requireThat(new URL(req.url).origin === PUBLISHER.origin); }
        catch { throw new DiagnosticFailure('trust', 'endpoint_binding'); }
        return nativeRequest(req, op, identity.tls, check, tor?.scope);
      } };
  } catch (error) {
    if (error instanceof DiagnosticFailure || isTorConnectionFailure(error)) throw error;
    // Preserve the bridge's reviewed closed category, not an upstream exception.
    if (error instanceof VerifierFailure) throw error;
    throw new DiagnosticFailure('trust', signal?.aborted ? 'interrupted' : constraint);
  }
}
export function sameV3Endpoint(a: V3Identity, b: V3Identity): boolean {
  return a.tag === b.tag && a.digest === b.digest && a.tls === b.tls && a.config === b.config && a.onionIdentity === b.onionIdentity;
}

// TLS authentication here is exact SDK-endorsed SPKI plus native hostname/date
// validation, not Web PKI. Endorsed self-signed leaves are normal. No HTTP object
// exists until secureConnect proves possession and validates the peer. Each
// request owns a fresh socket: no proxies, redirects, pool or session resumption.
export async function pinnedRequest(req: Request, op: Operation, fingerprint: string, check: () => void): Promise<Response> {
  return nativeRequest(req, op, fingerprint, check);
}
// Scope-only public acquisition retains native WebPKI defaults. An untrusted
// self-signed public gateway needs a separately reviewed public-only policy;
// this path never relaxes trust or sends private request headers/bodies.
export async function publicTorRequest(req: Request, op: Operation, scope: TorConnection): Promise<Response> {
  TorConnection.requireOwned(scope);
  requireThat(req.method === 'GET' && !req.body && [...req.headers].length === 0 && req.credentials === 'omit', 'encoding');
  return nativeRequest(req, op, undefined, () => scope.check(), scope);
}
async function nativeRequest(req: Request, op: Operation, fingerprint: string | undefined, check: () => void,
  scope?: TorConnection): Promise<Response> {
  let url: URL;
  try { url = new URL(req.url); requireThat(url.protocol === 'https:' && !url.username && !url.password); }
  catch { throw new DiagnosticFailure('transport', 'encoding'); }
  op.check(); check();
  let socket: TLSSocket | undefined, raw: Duplex | undefined, handedOff = false;
  let primary: TorNativeTransportFailure | undefined;
  try {
    if (scope) {
      raw = await scope.connectSocket({ host: url.hostname, port: Number(url.port || 443) }, op.controller.signal);
      raw.on('error', (error: unknown) => { if (error instanceof TorNativeTransportFailure) primary ??= error; });
      op.check(); check();
    }
    socket = await op.wait(new Promise<TLSSocket>((resolve, reject) => {
      const peer = tlsConnect({ ...(raw ? { socket: raw } : { host: url.hostname, port: Number(url.port || 443) }),
        servername: url.hostname, rejectUnauthorized: fingerprint === undefined, ALPNProtocols: ['http/1.1'] });
      // Abort can race the native pipe error and produce a second TLS error.
      // Keep an owned observer through close; the first failure still wins.
      if (scope) peer.on('error', () => {});
      const abort = () => peer.destroy(new DiagnosticFailure('transport', 'interrupted'));
      op.controller.signal.addEventListener('abort', abort, { once: true });
      peer.once('close', () => op.controller.signal.removeEventListener('abort', abort));
      if (op.controller.signal.aborted) abort();
      peer.once('error', () => reject(primary ?? new DiagnosticFailure('transport', 'fetch')));
      peer.once('secureConnect', () => {
        try {
          check(); op.check();
          requireThat(!peer.isSessionReused() && peer.getPeerCertificate().raw);
          if (fingerprint !== undefined) certificate(peer.getPeerCertificate().raw, url.hostname, fingerprint, true);
          else requireThat(peer.authorized);
          resolve(peer);
        } catch (error) { peer.destroy(); reject(error instanceof DiagnosticFailure || isTorConnectionFailure(error)
          ? error : new DiagnosticFailure('trust', 'endpoint_binding')); }
      });
    }));
    const body = req.body ? Buffer.from(await op.wait(req.arrayBuffer())) : undefined;
    op.check(); check();
    const agent = new Agent({ keepAlive: false, maxCachedSessions: 0 });
    // Supported Agent hook returns ONLY the already authenticated owned socket.
    const owned = socket;
    agent.createConnection = () => owned;
    const response = await op.wait(new Promise<Response>((resolve, reject) => {
      // GitHub requires a User-Agent. Public-only acquisition uses a fixed value,
      // never host/user metadata; private headers and clearnet behavior are unchanged.
      const headers = fingerprint === undefined ? { 'user-agent': 'possums-pi' } : Object.fromEntries(req.headers);
      const request = httpsRequest(url, { method: req.method, headers, agent,
        signal: op.controller.signal }, (incoming: IncomingMessage) => {
        try {
          const status = incoming.statusCode!;
          const headers = new Headers();
          const incomingHeaders: Record<string, string | string[] | undefined> = incoming.headers;
          for (const [name, value] of Object.entries(incomingHeaders)) {
            if (value !== undefined) headers.set(name, Array.isArray(value) ? value.join(', ') : value);
          }
          let stream: Readable = incoming;
          // Node owns HTTP framing. Project Tor byte/decoder errors locally,
          // including post-header failures, before exposing the Web body.
          let ownedDecoder: Readable | undefined;
          const safe = scope ? new PassThrough({ destroy(error: Error | null, callback: (error: Error | null) => void) {
            // Stop queued flowing reads before the Web adapter closes. Native
            // Node cancellation may otherwise emit buffered data after close.
            this.pause(); ownedDecoder?.unpipe(this); ownedDecoder?.destroy();
            incoming.unpipe(this); incoming.destroy(); callback(error);
          } }) : undefined;
          if (safe) {
            safe.on('error', () => {});
            incoming.once('error', () => safe.destroy(primary ?? new DiagnosticFailure('transport', 'fetch', 'uncertain', status)));
            incoming.once('aborted', () => safe.destroy(primary ?? new DiagnosticFailure('transport', 'eof', 'uncertain', status)));
            safe.once('close', () => incoming.destroy());
          }
          const encoding = headers.get('content-encoding');
          if (encoding && encoding !== 'identity') {
            const decoder = encoding === 'gzip' ? createGunzip() : encoding === 'deflate' ? createInflate()
              : encoding === 'br' ? createBrotliDecompress() : undefined;
            if (!decoder) throw new DiagnosticFailure('transport', 'body', 'uncertain', status);
            ownedDecoder = decoder;
            incoming.once('error', () => decoder.destroy(new DiagnosticFailure('transport', 'fetch', 'uncertain', status)));
            stream = incoming.pipe(decoder);
            stream.once('close', () => incoming.destroy());
            if (safe) decoder.once('error', (error: unknown) => safe.destroy(error instanceof DiagnosticFailure
              || error instanceof TorNativeTransportFailure ? error : new DiagnosticFailure('transport', 'body', 'uncertain', status)));
          }
          const noBody = [204, 205, 304].includes(status);
          if (safe) stream = stream.pipe(safe);
          const response = new Response(noBody ? null : Readable.toWeb(stream, { strategy: { highWaterMark: stream.readableHighWaterMark,
            size: (chunk: Uint8Array) => chunk.byteLength } }) as ReadableStream<Uint8Array>, { status, headers });
          // Native Response wrappers otherwise have an empty URL. No SDK or
          // HTTP redirects are followed; record exactly the locally requested URL.
          if (scope) Object.defineProperty(response, 'url', { value: req.url });
          resolve(response);
          if (noBody) incoming.resume();
        } catch (error) {
          incoming.destroy();
          reject(error instanceof DiagnosticFailure ? error : new DiagnosticFailure('transport', 'unexpected', 'uncertain', incoming.statusCode));
        }
      });
      request.once('error', () => reject(primary ?? new DiagnosticFailure('transport', 'fetch', 'uncertain')));
      request.once('close', () => agent.destroy());
      request.end(body);
    }));
    if (scope) { op.check(); check(); }
    handedOff = true;
    return response;
  } catch (error) {
    if (error instanceof DiagnosticFailure || error instanceof OperationFailure || isTorConnectionFailure(error)) throw error;
    throw new DiagnosticFailure('transport', 'unexpected', 'uncertain');
  } finally { if (!handedOff) { socket?.destroy(); raw?.destroy(); } }
}
