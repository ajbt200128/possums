import { Identity, type RequestContext } from 'ehbp';
import { WEB_APPROVAL, qualifyWeb, qualifyApi, requireApiApproval, validateKeyConfig } from './approval.js';
import { LIMITS, ChannelError, Operation, base64, boundedReport, cleanup, collect, hex, mapData, parseJSON, requireThat, serialize } from './limits.js';
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
  const headers = new Headers();
  if (body) headers.set('Content-Type', 'application/json');
  if (bearer !== undefined) {
    requireThat(typeof bearer === 'string' && /^[A-Za-z0-9_-]{43}$/.test(bearer));
    headers.set('Authorization', 'Bearer ' + bearer);
  }
  return new Request(url, { method, headers, body, signal: op.controller.signal,
    credentials: 'omit', redirect: 'error', cache: 'no-store', referrerPolicy: 'no-referrer' });
}
async function send(req: Request, op: Operation): Promise<Response> {
  const response = await op.wait(fetch(req), LIMITS.operationMs);
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

export type Chat = { model: string; stream: true; submission: string;
  messages: { role: 'system' | 'user' | 'assistant'; content: string }[] };
export type Control = { challenge: string; credential: string } | { model: string; new_conversation: boolean };
function fields(value: unknown, keys: readonly string[]): Record<string, unknown> {
  requireThat(!Array.isArray(value));
  const snapshot = mapData(value, keys.length, (key, field) => {
    requireThat(keys.includes(key));
    return field;
  });
  for (const key of keys) requireThat(Object.hasOwn(snapshot, key));
  return snapshot;
}
export function encodeChat(chat: Chat): Uint8Array<ArrayBuffer> {
  const admitted = fields(chat, ['model', 'stream', 'submission', 'messages']);
  requireThat(admitted.stream === true && typeof admitted.model === 'string' && typeof admitted.submission === 'string' && /^[A-Za-z0-9._:/-]{1,128}$/.test(admitted.model) && /^[A-Za-z0-9_-]{43}$/.test(admitted.submission));
  requireThat(Array.isArray(admitted.messages));
  admitted.messages = mapData(admitted.messages, LIMITS.messages, (_key, message) => {
    const admittedMessage = fields(message, ['role', 'content']);
    requireThat(['system', 'user', 'assistant'].includes(admittedMessage.role as string) && typeof admittedMessage.content === 'string');
    return admittedMessage;
  });
  requireThat((admitted.messages as unknown[]).length > 0);
  return serialize(admitted, LIMITS.chat);
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
  private constructor(authority: symbol, origin: string, identity: Identity, policyCheck: () => void = () => {}) {
    requireThat(authority === channelAuthority);
    let url: URL;
    try { url = new URL(origin); } catch { throw new ChannelError(); }
    requireThat(url.protocol === 'https:' && url.origin === origin && !url.username && !url.password);
    this.#origin = origin; this.#identity = identity; this.#policyCheck = policyCheck;
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
    });
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
  async models(bearer: string): Promise<unknown> { return this.#get('/v1/models', LIMITS.catalog, bearer); }
  async challenge(): Promise<unknown> { return this.#get('/v1/auth/challenge', LIMITS.control); }
  async #get(path: '/v1/models' | '/v1/auth/challenge', cap: number, bearer?: string): Promise<unknown> {
    this.#policyCheck();
    const op = new Operation();
    let res: Response | undefined;
    try { res = await send(request(this.#origin + path, op, 'GET', undefined, bearer), op);
      requireThat(res.ok); return parseJSON(await collect(res.body, cap, op), cap);
    } catch { throw new ChannelError(); }
    finally { op.close(); if (res?.body && !res.body.locked) await cleanup(res.body.cancel()); }
  }
  async control(path: '/v1/sessions' | '/v1/submissions', payload: Control, bearer?: string): Promise<unknown> {
    requireThat(path === '/v1/sessions' || path === '/v1/submissions');
    const admitted = fields(payload, path === '/v1/sessions' ? ['challenge', 'credential'] : ['model', 'new_conversation']);
    if (path === '/v1/sessions') {
      requireThat(typeof admitted.credential === 'string' && typeof admitted.challenge === 'string' && /^[A-Za-z0-9_-]{32,512}$/.test(admitted.credential) && /^[A-Za-z0-9_-]{32,512}$/.test(admitted.challenge) && bearer === undefined);
    } else { requireThat(typeof admitted.model === 'string' && /^[A-Za-z0-9._:/-]{1,128}$/.test(admitted.model) && typeof admitted.new_conversation === 'boolean' && bearer !== undefined); }
    const body = await this.#encrypted(path, serialize(admitted, LIMITS.control), bearer);
    const op = new Operation();
    try { return parseJSON(await collect(body, LIMITS.control, op), LIMITS.control); }
    catch { throw new ChannelError('uncertain'); } finally { op.close(); }
  }
  async chat(chat: Chat, bearer: string): Promise<ReadableStream<Uint8Array>> {
    return this.#encrypted('/v1/chat/completions', encodeChat(chat), bearer);
  }
  async #encrypted(path: string, bytes: Uint8Array<ArrayBuffer>, bearer?: string): Promise<ReadableStream<Uint8Array>> {
    this.#policyCheck();
    const op = new Operation(LIMITS.streamMs);
    let sent = false, res: Response | undefined;
    try {
      const encrypted = await op.wait<{ request: Request; context: RequestContext | null }>(this.#identity.encryptRequestWithContext(request(this.#origin + path, op, 'POST', bytes, bearer)));
      requireThat(encrypted.context && !op.controller.signal.aborted);
      sent = true; // Once handed to fetch, receipt/admission is uncertain on ANY failure.
      res = await send(encrypted.request, op);
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
