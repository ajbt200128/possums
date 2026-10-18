import { createHash, X509Certificate } from 'node:crypto';
import { Agent, request as httpsRequest } from 'node:https';
import { Buffer } from 'node:buffer';
import type { IncomingMessage } from 'node:http';
import { connect as tlsConnect, type TLSSocket } from 'node:tls';
import { Readable } from 'node:stream';
import { createGunzip, createInflate, createBrotliDecompress } from 'node:zlib';
import { verifyV3, VerifierFailure } from './verifier.mjs';
import { PUBLISHER, checkApproval, validateKeyConfig, requireReleaseTag, requirePublishedKeys } from '../../examples/phase01/approval.js';
import { LIMITS, DiagnosticFailure, Operation, OperationFailure, base64, digest, parseJSON, requireThat } from '../../examples/phase01/limits.js';

export const V3_EVIDENCE_LIMIT = 16 * 1024 * 1024;
export type V3Input = { evidence: Uint8Array; nonce: Uint8Array; manifest: Uint8Array<ArrayBuffer>;
  tag: string; certificate: string; config: Uint8Array };
export type V3Identity = Readonly<{ tag: string; digest: string; tls: string; config: string; freshness: number }>;
const spki = (cert: X509Certificate) => createHash('sha256').update(cert.publicKey.export({ type: 'spki', format: 'der' })).digest('hex');
function certificate(bytes: string | Buffer, host: string, fingerprint: string): X509Certificate {
  try {
    const cert = new X509Certificate(bytes);
    requireThat(spki(cert) === fingerprint && cert.checkHost(host, { subject: 'never' }) !== undefined);
    requireThat(Date.now() >= Date.parse(cert.validFrom) && Date.now() < Date.parse(cert.validTo));
    return cert;
  } catch { throw new DiagnosticFailure('trust', 'certificate'); }
}
export async function qualifyV3(input: V3Input, signal?: AbortSignal) {
  let constraint: 'manifest' | 'verification' | 'key_config' = 'manifest';
  try {
    input = { ...input, evidence: new Uint8Array(input.evidence), nonce: new Uint8Array(input.nonce),
      manifest: new Uint8Array(input.manifest), config: new Uint8Array(input.config) };
    requireReleaseTag(input.tag);
    const hash = await digest(input.manifest);
    const manifest = parseJSON(input.manifest, LIMITS.provenance);
    const config = base64(manifest.config, LIMITS.certificate);
    requireThat(config.length > 0 && manifest.hashes.version === 'v0.14.12' && typeof manifest.cmdline === 'string');
    const hashes = manifest.cmdline.split(/\s+/).filter((part: string) => part.startsWith('tinfoil-config-hash='));
    requireThat(hashes.length === 1 && hashes[0] === `tinfoil-config-hash=${await digest(config)}`);
    constraint = 'verification';
    const facts = await verifyV3(input.evidence, input.nonce,
      `${PUBLISHER.repository}@${input.tag}@sha256:${hash}`, signal, input.manifest);
    requireThat(facts.release_tag === input.tag && facts.release_digest === hash);
    requirePublishedKeys(hash, facts.hpke_key);
    const cert = certificate(input.certificate, new URL(PUBLISHER.origin).hostname, facts.tls_key_sha256);
    constraint = 'key_config';
    validateKeyConfig(input.config, facts.hpke_key);
    const freshness = facts.freshness_expires_at_unix * 1000;
    const expires = Math.min(Date.now() + 12 * 60 * 60 * 1000, Date.parse(cert.validTo), freshness);
    const check = () => checkApproval(Date.now(), expires, false);
    check(); requireThat(!signal?.aborted);
    const identity: V3Identity = Object.freeze({ tag: input.tag, digest: hash, tls: facts.tls_key_sha256,
      config: await digest(new Uint8Array(input.config)), freshness });
    return { release: Object.freeze({ tag: input.tag, expires }), identity, check,
      send: (req: Request, op: Operation) => {
        try { requireThat(new URL(req.url).origin === PUBLISHER.origin); }
        catch { throw new DiagnosticFailure('trust', 'endpoint_binding'); }
        return nativeRequest(req, op, identity.tls, check);
      } };
  } catch (error) {
    if (error instanceof DiagnosticFailure) throw error;
    // Preserve the bridge's reviewed closed category, not an upstream exception.
    if (error instanceof VerifierFailure) throw error;
    throw new DiagnosticFailure('trust', signal?.aborted ? 'interrupted' : constraint);
  }
}
export function sameV3Endpoint(a: V3Identity, b: V3Identity): boolean {
  return a.tag === b.tag && a.digest === b.digest && a.tls === b.tls && a.config === b.config;
}

// TLS authentication here is exact SDK-endorsed SPKI plus native hostname/date
// validation, not Web PKI. Endorsed self-signed leaves are normal. No HTTP object
// exists until secureConnect proves possession and validates the peer. Each
// request owns a fresh socket: no proxies, redirects, pool or session resumption.
export async function pinnedRequest(req: Request, op: Operation, fingerprint: string, check: () => void): Promise<Response> {
  return nativeRequest(req, op, fingerprint, check);
}
async function nativeRequest(req: Request, op: Operation, fingerprint: string, check: () => void): Promise<Response> {
  let url: URL;
  try { url = new URL(req.url); requireThat(url.protocol === 'https:' && !url.username && !url.password); }
  catch { throw new DiagnosticFailure('transport', 'encoding'); }
  op.check(); check();
  let socket: TLSSocket | undefined, handedOff = false;
  try {
    socket = await op.wait(new Promise<TLSSocket>((resolve, reject) => {
      const peer = tlsConnect({ host: url.hostname, port: Number(url.port || 443),
        servername: url.hostname, rejectUnauthorized: false, ALPNProtocols: ['http/1.1'] });
      const abort = () => peer.destroy(new DiagnosticFailure('transport', 'interrupted'));
      op.controller.signal.addEventListener('abort', abort, { once: true });
      peer.once('close', () => op.controller.signal.removeEventListener('abort', abort));
      if (op.controller.signal.aborted) abort();
      peer.once('error', () => reject(new DiagnosticFailure('transport', 'fetch')));
      peer.once('secureConnect', () => {
        try {
          check(); op.check();
          requireThat(!peer.isSessionReused() && peer.getPeerCertificate().raw);
          certificate(peer.getPeerCertificate().raw, url.hostname, fingerprint);
          resolve(peer);
        } catch (error) { peer.destroy(); reject(error instanceof DiagnosticFailure
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
      const headers = Object.fromEntries(req.headers);
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
          const encoding = headers.get('content-encoding');
          if (encoding && encoding !== 'identity') {
            const decoder = encoding === 'gzip' ? createGunzip() : encoding === 'deflate' ? createInflate()
              : encoding === 'br' ? createBrotliDecompress() : undefined;
            if (!decoder) throw new DiagnosticFailure('transport', 'body', 'uncertain', status);
            incoming.once('error', () => decoder.destroy(new DiagnosticFailure('transport', 'fetch', 'uncertain', status)));
            stream = incoming.pipe(decoder);
            stream.once('close', () => incoming.destroy());
          }
          const noBody = [204, 205, 304].includes(status);
          const response = new Response(noBody ? null : Readable.toWeb(stream, { strategy: { highWaterMark: stream.readableHighWaterMark,
            size: (chunk: Uint8Array) => chunk.byteLength } }) as ReadableStream<Uint8Array>, { status, headers });
          resolve(response);
          if (noBody) incoming.resume();
        } catch (error) {
          incoming.destroy();
          reject(error instanceof DiagnosticFailure ? error : new DiagnosticFailure('transport', 'unexpected', 'uncertain', incoming.statusCode));
        }
      });
      request.once('error', () => reject(new DiagnosticFailure('transport', 'fetch', 'uncertain')));
      request.once('close', () => agent.destroy());
      request.end(body);
    }));
    handedOff = true;
    return response;
  } catch (error) {
    if (error instanceof DiagnosticFailure || error instanceof OperationFailure) throw error;
    throw new DiagnosticFailure('transport', 'unexpected', 'uncertain');
  } finally { if (!handedOff) socket?.destroy(); }
}
