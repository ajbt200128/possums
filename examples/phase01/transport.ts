import { Identity, type RequestContext } from 'ehbp';
import { CatalogFailure } from './client.js';
import { WEB_APPROVAL, PUBLISHER, qualifyWeb, qualifyApi, qualifyPublished, requireApiApproval, validateKeyConfig, checkApproval, type PublishedRelease } from './approval.js';
import { LIMITS, ChannelError, Operation, base64, boundedReport, cleanup, collect, hex, parseJSON, requireThat, serialize } from './limits.js';
import { admitInvocation, fields, type Chat } from './tools.js';
export type { Chat, Invocation, Message, Tool, ToolChoice, ToolCall, JSONObject, JSONValue } from './tools.js';
export type { LiveModel, Receipt, CompletionOptions, CompletionEvent, ChatOptions } from './client.js';
export type ResponseInfo = Readonly<{ status: number; contentType: 'text/event-stream' | 'application/json' | null }>;
export type ResponseOptions = { signal?: AbortSignal; onResponse?: (response: ResponseInfo) => void | Promise<void> };
export { WEB_APPROVAL, API_APPROVALS, qualifyWeb, validateKeyConfig, checkApproval } from './approval.js';
export { LIMITS, serialize, parseJSON } from './limits.js';
export { ReferenceClient, consumeCompletion, validateModels } from './client.js';
declare const __PHASE01_FIXTURE__: boolean;

const publicPaths = ['/.well-known/tinfoil-attestation', '/.well-known/tinfoil-certificate', '/.well-known/hpke-keys'] as const;
const provenanceURL = 'https://api.github.com/repos/ajbt200128/possums/attestations/sha256:' + WEB_APPROVAL.manifest;
// AMD KDS has no browser CORS. Browser callers may supply a bounded, independently
// verified bundle; a missing CORS-capable acquisition path never permits a bypass.
function allowedAcquisition(url: string): boolean {
  return publicPaths.some(p => url === WEB_APPROVAL.origin + p) || url === provenanceURL ||
    /^https:\/\/kdsintf\.amd\.com\/vcek\/v1\/Genoa\/[0-9a-f]{128}\?blSPL=\d{1,3}&teeSPL=\d{1,3}&snpSPL=\d{1,3}&ucodeSPL=\d{1,3}$/.test(url);
}
// Fetch decodes HTTP compression. Every consumer caps decoded bytes while reading;
// browser/network decompressor allocations and encoded bytes are not observable here.
function request(url: string, op: Operation, method = 'GET', body?: Uint8Array<ArrayBuffer>, bearer?: string): Request {
  op.check();
  const headers = new Headers();
  if (body) headers.set('Content-Type', 'application/json');
  if (bearer !== undefined) {
    requireThat(typeof bearer === 'string' && /^[A-Za-z0-9_-]{43}$/.test(bearer));
    headers.set('Authorization', 'Bearer ' + bearer);
  }
  return new Request(url, { method, headers, body, signal: op.controller.signal,
    credentials: 'omit', redirect: 'error', cache: 'no-store', referrerPolicy: 'no-referrer' });
}
async function send(req: Request, op: Operation, observeStatus?: (status: number) => void): Promise<Response> {
  op.check();
  const response = await op.wait(fetch(req), LIMITS.operationMs);
  // Catalog-only observation, before transport rejection/cleanup (e.g. redirects).
  observeStatus?.(response.status);
  if (response.redirected || (response.status >= 300 && response.status < 400) ||
      (response.headers.has('content-encoding') && !['identity', 'gzip', 'deflate', 'br'].includes(response.headers.get('content-encoding')!))) {
    op.close();
    if (response.body) await cleanup(response.body.cancel());
    throw new ChannelError();
  }
  return response;
}
async function acquire(url: string, cap: number): Promise<Uint8Array<ArrayBuffer>> {
  requireThat(allowedAcquisition(url));
  const op = new Operation();
  let res: Response | undefined;
  try {
    res = await send(request(url, op), op);
    requireThat(res.ok);
    return await collect(res.body, cap, op);
  } catch { throw new ChannelError(); }
  finally { op.close(); if (res?.body && !res.body.locked) await cleanup(res.body.cancel()); }
}
// No latest tag, SDK acquisition retries, ambient Authorization or server-selected approval.
// Manifest bytes are independently hash pinned; supplying them also avoids GitHub's
// redirecting binary-asset endpoint. Acquisition URLs are not caller controlled.
export async function observeWeb(manifest: Uint8Array<ArrayBuffer>) {
  requireThat(manifest.length <= LIMITS.provenance);
  const evidenceBytes = await acquire(WEB_APPROVAL.origin + publicPaths[0], LIMITS.evidence);
  const doc = parseJSON(evidenceBytes, LIMITS.evidence);
  requireThat(doc.format === 'https://tinfoil.sh/predicate/sev-snp-guest/v2');
  const op = new Operation();
  let raw: Uint8Array<ArrayBuffer>;
  try { raw = await boundedReport(base64(doc.body, LIMITS.evidence), op); } finally { op.close(); }
  // Untrusted routing fields only; the pinned verifier subsequently authenticates
  // the entire report and verifies Genoa product/TCB/chip binding to this VCEK.
  const tcb = raw.slice(0x180, 0x188);
  const kds = `https://kdsintf.amd.com/vcek/v1/Genoa/${hex(raw.slice(0x1a0, 0x1e0))}?blSPL=${tcb[0]}&teeSPL=${tcb[1]}&snpSPL=${tcb[6]}&ucodeSPL=${tcb[7]}`;
  const vcek = await acquire(kds, LIMITS.certificate);
  const cert = parseJSON(await acquire(WEB_APPROVAL.origin + publicPaths[1], LIMITS.certificate), LIMITS.certificate);
  const provenance = parseJSON(await acquire(provenanceURL, LIMITS.provenance), LIMITS.provenance);
  requireThat(provenance.attestations?.length === 1);
  const bundle = {
    domain: 'possum-phase0.possums.containers.tinfoil.dev', enclaveAttestationReport: doc,
    enclaveCert: cert.certificate, vcek: btoa(String.fromCharCode(...vcek)),
    digest: WEB_APPROVAL.manifest, releaseTag: WEB_APPROVAL.tag,
    sigstoreBundle: provenance.attestations[0].bundle,
  };
  // Acquired structures are bounded; JSON encoding here includes numeric provenance fields.
  const bytes = new TextEncoder().encode(JSON.stringify(bundle));
  requireThat(bytes.length <= LIMITS.bundle);
  return qualifyWeb(bytes, manifest, await acquire(WEB_APPROVAL.origin + publicPaths[2], LIMITS.key));
}

export type Control = { challenge: string; credential: string } | { model: string; new_conversation: boolean };
export function encodeChat(chat: Chat): Uint8Array<ArrayBuffer> {
  return serialize(admitInvocation(chat, true), LIMITS.chat);
}
// Frame-size validation precedes the library's 64-MiB buffering boundary. This
// parses framing only; authentication/decryption remains entirely published EHBP.
function encryptedFrames(body: ReadableStream<Uint8Array>, op: Operation): ReadableStream<Uint8Array> {
  const reader = body.getReader();
  let total = 0, frames = 0, length = 0, prefix = 0, remaining = 0;
  const cancel = async () => { op.close(); await cleanup(reader.cancel()); };
  op.controller.signal.addEventListener('abort', () => { void cleanup(reader.cancel()); }, { once: true });
  return new ReadableStream({
    async pull(controller) {
      try {
        op.check();
        const { done, value } = await op.wait(reader.read(), LIMITS.streamMs);
        if (done) { requireThat(prefix === 0 && remaining === 0 && frames > 0); controller.close(); return; }
        total += value.length;
        requireThat(value.length <= LIMITS.chunk && total <= LIMITS.stream);
        for (let i = 0; i < value.length;) {
          if (remaining) { const consumed = Math.min(remaining, value.length - i); i += consumed; remaining -= consumed; }
          else {
            length = length * 256 + value[i++];
            if (++prefix === 4) {
              requireThat(length >= 16 && length <= LIMITS.frame && ++frames <= LIMITS.frames);
              remaining = length; length = 0; prefix = 0;
            }
          }
        }
        controller.enqueue(value);
      } catch { await cancel(); controller.error(new ChannelError('uncertain')); }
    }, cancel,
  }, { highWaterMark: 0 });
}
function plaintext(body: ReadableStream<Uint8Array>, op: Operation, sse: boolean): ReadableStream<Uint8Array> {
  const reader = body.getReader();
  const decoder = new TextDecoder('utf-8', { fatal: true });
  let total = 0, eventBytes = 0, lineBytes = 0, events = 0;
  return new ReadableStream({
    async pull(controller) {
      try {
        op.check();
        const { done, value } = await op.wait(reader.read(), sse ? LIMITS.streamMs : LIMITS.idleMs);
        if (done) { if (sse) decoder.decode(); op.close(); controller.close(); return; }
        total += value.length;
        requireThat(value.length <= LIMITS.frame && total <= LIMITS.stream);
        if (sse) {
          decoder.decode(value, { stream: true });
          // Envelope budgets only. Ordered role/finish/usage/DONE/EOF semantics
          // belong to the later client parser, not this transport qualification.
          for (const byte of value) {
            requireThat(++eventBytes <= LIMITS.sseEvent);
            if (byte === 10) {
              if (lineBytes === 0) { requireThat(++events <= LIMITS.sseEvents); eventBytes = 0; }
              lineBytes = 0;
            } else if (byte !== 13) lineBytes++;
          }
        }
        controller.enqueue(value);
      } catch { op.close(); await cleanup(reader.cancel()); controller.error(new ChannelError('uncertain')); }
    }, async cancel() { op.close(); await cleanup(reader.cancel()); },
  }, { highWaterMark: 0 });
}
const channelAuthority = Symbol('isolated channel authority');
export class Channel {
  readonly #origin: string;
  readonly #identity: Identity;
  readonly #policyCheck: () => void;
  readonly #release: PublishedRelease | undefined;
  get release(): PublishedRelease | undefined { return this.#release; }
  private constructor(authority: symbol, origin: string, identity: Identity, policyCheck: () => void = () => {}, release?: PublishedRelease) {
    requireThat(authority === channelAuthority);
    let url: URL;
    try { url = new URL(origin); } catch { throw new ChannelError(); }
    requireThat(url.protocol === 'https:' && url.origin === origin && !url.username && !url.password);
    this.#origin = origin; this.#identity = identity; this.#policyCheck = policyCheck; this.#release = release;
  }
  static requireVerified(value: unknown): asserts value is Channel {
    requireThat(typeof value === 'object' && value !== null && #identity in value);
  }
  static async api(bundleBytes?: Uint8Array, manifestBytes?: Uint8Array<ArrayBuffer>, keyConfig?: Uint8Array): Promise<Channel> {
    const approval = requireApiApproval();
    requireThat(bundleBytes && manifestBytes && keyConfig && bundleBytes.length <= LIMITS.bundle && manifestBytes.length <= LIMITS.provenance && keyConfig.length === LIMITS.key);
    // Keep verification and channel construction on the SAME immutable snapshots.
    const bundle = new Uint8Array(bundleBytes), manifest = new Uint8Array(manifestBytes), config = new Uint8Array(keyConfig);
    await qualifyApi(bundle, manifest, config);
    return new Channel(channelAuthority, approval.origin, await Identity.unmarshalPublicConfig(config), () => {
      requireThat(requireApiApproval() === approval);
    }, Object.freeze({ tag: approval.tag, expires: approval.expires }));
  }
  static async published(bundleBytes: Uint8Array, manifestBytes: Uint8Array<ArrayBuffer>, keyConfig: Uint8Array, signal?: AbortSignal): Promise<Channel> {
    try {
      requireThat(bundleBytes && manifestBytes && keyConfig && bundleBytes.length <= LIMITS.bundle && manifestBytes.length <= LIMITS.provenance && keyConfig.length === LIMITS.key);
      const bundle = new Uint8Array(bundleBytes), manifest = new Uint8Array(manifestBytes), config = new Uint8Array(keyConfig);
      const { release } = await qualifyPublished(bundle, manifest, config, signal);
      const identity = await Identity.unmarshalPublicConfig(config);
      const check = () => checkApproval(Date.now(), release.expires, false);
      check(); requireThat(!signal?.aborted);
      return new Channel(channelAuthority, PUBLISHER.origin, identity, check, release);
    } catch { throw new ChannelError(); }
  }
  static async fixture(origin: string, config: Uint8Array, independentKey: string): Promise<Channel> {
    // Build-time elimination, plus loopback restriction: fixture trust cannot
    // authorize the deployed origin or be switched on through runtime options.
    requireThat(__PHASE01_FIXTURE__ && /^https:\/\/localhost:[1-9][0-9]{3,4}$/.test(origin));
    requireThat(config.length === LIMITS.key);
    const snapshot = new Uint8Array(config);
    validateKeyConfig(snapshot, independentKey);
    return new Channel(channelAuthority, origin, await Identity.unmarshalPublicConfig(snapshot));
  }
  async models(bearer: string, signal?: AbortSignal): Promise<unknown> {
    try { return await this.#get('/v1/models', LIMITS.catalog, bearer, signal); }
    catch (error) {
      if (error instanceof CatalogFailure) throw error;
      throw new CatalogFailure('request', error instanceof ChannelError ? error.code : 'rejected');
    }
  }
  async challenge(signal?: AbortSignal): Promise<unknown> { return this.#get('/v1/auth/challenge', LIMITS.control, undefined, signal); }
  async #get(path: '/v1/models' | '/v1/auth/challenge', cap: number, bearer?: string, signal?: AbortSignal): Promise<unknown> {
    this.#policyCheck();
    const op = new Operation(LIMITS.operationMs, signal);
    let sent = false, res: Response | undefined, status: number | undefined;
    const catalog = path === '/v1/models';
    try {
      const req = request(this.#origin + path, op, 'GET', undefined, bearer);
      sent = true; res = await send(req, op, catalog ? value => { status = value; } : undefined);
      if (catalog && !res.ok) {
        let value: unknown;
        try { value = parseJSON(await collect(res.body, LIMITS.error, op), LIMITS.error); }
        catch { /* Keep the observed HTTP rejection even if its bounded body fails. */ }
        throw new CatalogFailure('http', sent && op.controller.signal.aborted ? 'uncertain' : 'rejected', status, value);
      }
      requireThat(res.ok); return parseJSON(await collect(res.body, cap, op), cap);
    } catch (error) {
      const code = sent && op.controller.signal.aborted ? 'uncertain' : 'rejected';
      if (catalog) {
        if (error instanceof CatalogFailure) throw error;
        throw new CatalogFailure(status === undefined ? 'request' : status >= 200 && status < 300 ? 'body' : 'http', code, status);
      }
      throw new ChannelError(code);
    }
    finally { op.close(); if (res?.body && !res.body.locked) await cleanup(res.body.cancel()); }
  }
  async control(path: '/v1/sessions' | '/v1/submissions', payload: Control, bearer?: string, signal?: AbortSignal): Promise<unknown> {
    requireThat(path === '/v1/sessions' || path === '/v1/submissions');
    const admitted = fields(payload, path === '/v1/sessions' ? ['challenge', 'credential'] : ['model', 'new_conversation']);
    if (path === '/v1/sessions') {
      requireThat(typeof admitted.credential === 'string' && typeof admitted.challenge === 'string' && /^[A-Za-z0-9_-]{32,512}$/.test(admitted.credential) && /^[A-Za-z0-9_-]{32,512}$/.test(admitted.challenge) && bearer === undefined);
    } else { requireThat(typeof admitted.model === 'string' && /^[A-Za-z0-9._:/-]{1,128}$/.test(admitted.model) && typeof admitted.new_conversation === 'boolean' && bearer !== undefined); }
    const body = await this.#encrypted(path, serialize(admitted, LIMITS.control), bearer, { signal });
    const op = new Operation(LIMITS.operationMs, signal);
    try { return parseJSON(await collect(body, LIMITS.control, op), LIMITS.control); }
    catch { throw new ChannelError('uncertain'); } finally { op.close(); }
  }
  async chat(chat: Chat, bearer: string, options: ResponseOptions = {}): Promise<ReadableStream<Uint8Array>> {
    return this.#encrypted('/v1/chat/completions', encodeChat(chat), bearer, options);
  }
  async #encrypted(path: string, bytes: Uint8Array<ArrayBuffer>, bearer?: string, options: ResponseOptions = {}): Promise<ReadableStream<Uint8Array>> {
    this.#policyCheck();
    const op = new Operation(LIMITS.streamMs, options.signal);
    let sent = false, res: Response | undefined;
    try {
      const encrypted = await op.wait<{ request: Request; context: RequestContext | null }>(this.#identity.encryptRequestWithContext(request(this.#origin + path, op, 'POST', bytes, bearer)));
      requireThat(encrypted.context && !op.controller.signal.aborted);
      sent = true; // Once handed to fetch, receipt/admission is uncertain on ANY failure.
      res = await send(encrypted.request, op);
      if (options.onResponse) {
        const mime = res.headers.get('content-type')?.split(';', 1)[0].trim().toLowerCase();
        // Never hand the Response, arbitrary header values, or EHBP diagnostics to hooks.
        const info: ResponseInfo = Object.freeze({ status: res.status,
          contentType: mime === 'text/event-stream' || mime === 'application/json' ? mime : null });
        op.check(); await op.wait(Promise.resolve(options.onResponse(info)), LIMITS.streamMs);
      }
      requireThat(res.body && /^[0-9a-f]{64}$/.test(res.headers.get('Ehbp-Response-Nonce') ?? ''));
      const bounded = new Response(encryptedFrames(res.body, op), { headers: res.headers });
      const decrypted = await op.wait<Response>(this.#identity.decryptResponseWithContext(bounded, encrypted.context));
      requireThat(decrypted.body);
      return plaintext(decrypted.body, op, path === '/v1/chat/completions');
    } catch {
      op.close();
      if (res?.body && !res.body.locked) await cleanup(res.body.cancel());
      throw new ChannelError(sent ? 'uncertain' : 'rejected');
    }
  }
}
