import { Identity, type RequestContext } from 'ehbp';
import { CatalogFailure, GatewayError, gatewayError } from './client.js';
import { PUBLISHER, qualifyApi, qualifyPublished, requireApiApproval, validateKeyConfig, checkApproval, type PublishedRelease } from './approval.js';
import { LIMITS, ChannelError, DiagnosticFailure, OperationFailure, type FailureConstraint, Operation, cleanup, collect, parseJSON, requireThat, serialize } from './limits.js';
import { admitInvocation, fields, type Chat } from './tools.js';
export type { Chat, Invocation, Message, Tool, ToolChoice, ToolCall, JSONObject, JSONValue } from './tools.js';
export type { LiveModel, Receipt, BalanceSnapshot, CompletionOptions, CompletionEvent, ChatOptions } from './client.js';
export type ResponseInfo = Readonly<{ status: number; contentType: 'text/event-stream' | 'application/json' | null }>;
export type ResponseOptions = { signal?: AbortSignal; onResponse?: (response: ResponseInfo) => void | Promise<void> };
export { API_APPROVALS, validateKeyConfig, checkApproval } from './approval.js';
export { LIMITS, serialize, parseJSON } from './limits.js';
export { ReferenceClient, PreparedChat, BalanceFailure, consumeCompletion, validateModels } from './client.js';
declare const __PHASE01_FIXTURE__: boolean;

// Fetch decodes HTTP compression. Consumers cap decoded bytes while reading;
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
  // Observe status before transport rejection/cleanup; never retain response text.
  observeStatus?.(response.status);
  if (response.redirected || (response.status >= 300 && response.status < 400) ||
      (response.headers.has('content-encoding') && !['identity', 'gzip', 'deflate', 'br'].includes(response.headers.get('content-encoding')!))) {
    op.close();
    if (response.body) await cleanup(response.body.cancel());
    throw new ChannelError();
  }
  return response;
}
export type Control = { challenge: string; credential: string } | { model: string; new_conversation: boolean };
export function encodeChat(chat: Chat): Uint8Array<ArrayBuffer> {
  return serialize(admitInvocation(chat, true), LIMITS.chat);
}
// Frame-size validation precedes the library's 64-MiB buffering boundary. This
// parses framing only; authentication/decryption remains entirely published EHBP.
function encryptedFrames(body: ReadableStream<Uint8Array>, op: Operation, onFailure: (failure: DiagnosticFailure) => void): ReadableStream<Uint8Array> {
  const reader = body.getReader();
  let total = 0, frames = 0, length = 0, prefix = 0, remaining = 0;
  const cancel = async () => { op.close(); await cleanup(reader.cancel()); };
  op.controller.signal.addEventListener('abort', () => { void cleanup(reader.cancel()); }, { once: true });
  return new ReadableStream({
    async pull(controller) {
      try {
        op.check();
        const { done, value } = await op.wait(reader.read(), LIMITS.idleMs);
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
      } catch (error) {
        const failure = error instanceof DiagnosticFailure ? error : new DiagnosticFailure('transport', error instanceof OperationFailure ? error.constraint : error instanceof ChannelError ? 'frames' : 'fetch', 'uncertain');
        // EHBP's wait can observe our cancellation before its decoder sees this error.
        onFailure(failure);
        await cancel(); controller.error(failure);
      }
    }, cancel,
  }, { highWaterMark: 0 });
}
// Only these bodies already enforce inactivity at the HTTP byte boundary.
// Higher layers must not time decrypted frames/events while fragments progress.
const byteTimedBodies = new WeakSet<ReadableStream<Uint8Array>>();
export const hasByteReadTimeout = (body: ReadableStream<Uint8Array>): boolean => byteTimedBodies.has(body);
function plaintext(body: ReadableStream<Uint8Array>, op: Operation, sse: boolean, frameFailure: () => DiagnosticFailure | undefined): ReadableStream<Uint8Array> {
  const reader = body.getReader();
  const decoder = new TextDecoder('utf-8', { fatal: true });
  let total = 0, eventBytes = 0, lineBytes = 0, events = 0;
  return new ReadableStream({
    async pull(controller) {
      let constraint: FailureConstraint = 'decryption';
      try {
        op.check();
        const { done, value } = await op.wait(reader.read(), null);
        constraint = 'utf8';
        if (done) { if (sse) decoder.decode(); op.close(); controller.close(); return; }
        total += value.length;
        constraint = 'envelope';
        requireThat(value.length <= LIMITS.frame && total <= LIMITS.stream);
        if (sse) {
          constraint = 'utf8';
          decoder.decode(value, { stream: true });
          constraint = 'envelope';
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
      } catch (error) {
        const failure = frameFailure() ?? (error instanceof DiagnosticFailure ? error : new DiagnosticFailure('transport', error instanceof OperationFailure ? error.constraint : constraint, 'uncertain'));
        op.close(); await cleanup(reader.cancel()); controller.error(failure);
      }
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
    let constraint: FailureConstraint = 'schema';
    try {
      requireThat(bundleBytes && manifestBytes && keyConfig && bundleBytes.length <= LIMITS.bundle && manifestBytes.length <= LIMITS.provenance && keyConfig.length === LIMITS.key);
      const bundle = new Uint8Array(bundleBytes), manifest = new Uint8Array(manifestBytes), config = new Uint8Array(keyConfig);
      const { release } = await qualifyPublished(bundle, manifest, config, signal);
      constraint = 'key_config';
      const identity = await Identity.unmarshalPublicConfig(config);
      const check = () => checkApproval(Date.now(), release.expires, false);
      check(); requireThat(!signal?.aborted);
      return new Channel(channelAuthority, PUBLISHER.origin, identity, check, release);
    } catch (error) { throw error instanceof DiagnosticFailure ? error : new DiagnosticFailure('trust', signal?.aborted ? 'interrupted' : constraint); }
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
      if (error instanceof CatalogFailure || error instanceof DiagnosticFailure) throw error;
      throw new CatalogFailure('request', error instanceof ChannelError ? error.code : 'rejected');
    }
  }
  async challenge(signal?: AbortSignal): Promise<unknown> { return this.#get('/v1/auth/challenge', LIMITS.control, undefined, signal); }
  async #get(path: '/v1/models' | '/v1/auth/challenge', cap: number, bearer?: string, signal?: AbortSignal): Promise<unknown> {
    this.#policyCheck();
    const op = new Operation(LIMITS.operationMs, signal);
    let sent = false, res: Response | undefined, status: number | undefined;
    let constraint: FailureConstraint = 'encoding';
    const catalog = path === '/v1/models';
    try {
      const req = request(this.#origin + path, op, 'GET', undefined, bearer);
      constraint = 'fetch';
      sent = true; res = await send(req, op, value => { status = value; constraint = 'http'; });
      if (!res.ok && (catalog || status === 503)) {
        let value: unknown;
        try { value = parseJSON(await collect(res.body, LIMITS.error, op), LIMITS.error); }
        catch { /* Keep the observed HTTP rejection even if its bounded body fails. */ }
        if (!catalog && (value as any)?.error?.code === 'service_quiescing') {
          let failure: GatewayError | undefined;
          try { failure = gatewayError(value, status); } catch { /* Reject malformed envelopes as ordinary HTTP failures. */ }
          if (failure) throw failure;
        }
        throw new CatalogFailure('http', sent && op.controller.signal.aborted ? 'uncertain' : 'rejected', status, value);
      }
      requireThat(res.ok);
      constraint = 'body';
      return parseJSON(await collect(res.body, cap, op), cap);
    } catch (error) {
      const code = sent && op.controller.signal.aborted ? 'uncertain' : 'rejected';
      if (error instanceof DiagnosticFailure || error instanceof GatewayError) throw error;
      if (catalog) {
        if (error instanceof CatalogFailure) throw error;
        throw new CatalogFailure(status === undefined ? 'request' : status >= 200 && status < 300 ? 'body' : 'http', code, status);
      }
      throw new DiagnosticFailure('challenge', error instanceof OperationFailure ? error.constraint : constraint, code, status);
    }
    finally { op.close(); if (res?.body && !res.body.locked) await cleanup(res.body.cancel()); }
  }
  async control(path: '/v1/sessions' | '/v1/submissions', payload: Control, bearer?: string, signal?: AbortSignal): Promise<unknown> {
    requireThat(path === '/v1/sessions' || path === '/v1/submissions');
    const admitted = fields(payload, path === '/v1/sessions' ? ['challenge', 'credential'] : ['model', 'new_conversation']);
    if (path === '/v1/sessions') {
      requireThat(typeof admitted.credential === 'string' && typeof admitted.challenge === 'string' && /^[A-Za-z0-9_-]{32,512}$/.test(admitted.credential) && /^[A-Za-z0-9_-]{32,512}$/.test(admitted.challenge) && bearer === undefined);
    } else { requireThat(typeof admitted.model === 'string' && /^[A-Za-z0-9._:/-]{1,128}$/.test(admitted.model) && typeof admitted.new_conversation === 'boolean' && bearer !== undefined); }
    let status: number | undefined;
    const body = await this.#encrypted(path, serialize(admitted, LIMITS.control), bearer, { signal, onResponse: response => { status = response.status; } });
    const op = new Operation(LIMITS.operationMs, signal);
    try {
      const value = parseJSON(await collect(body, LIMITS.control, op), LIMITS.control);
      // Only classify a rejection envelope here; successful control admission remains unchanged.
      if (status !== undefined && (status < 200 || status >= 300) && value && Object.hasOwn(value, 'error')) {
        if (status === 503 && value.error?.code === 'service_quiescing') {
          let failure: GatewayError | undefined;
          try { failure = gatewayError(value, status); } catch { /* Malformed envelope remains an HTTP rejection. */ }
          if (failure) throw failure;
        }
        throw new DiagnosticFailure(path === '/v1/sessions' ? 'authentication' : 'submission', 'http', 'uncertain', status);
      }
      return value;
    }
    catch (error) {
      if (error instanceof GatewayError) throw error;
      if (error instanceof DiagnosticFailure) throw new DiagnosticFailure(error.stage, error.constraint, error.code, error.status ?? status);
      throw new DiagnosticFailure(path === '/v1/sessions' ? 'authentication' : 'submission', error instanceof OperationFailure ? error.constraint : 'body', 'uncertain', status);
    } finally { op.close(); }
  }
  async balance(bearer: string, signal?: AbortSignal): Promise<unknown> {
    // Reject missing/invalid authority before asking EHBP to encrypt anything.
    requireThat(typeof bearer === 'string' && /^[A-Za-z0-9_-]{43}$/.test(bearer));
    let status: number | undefined;
    const body = await this.#encrypted('/v1/balance', serialize({}, LIMITS.control), bearer,
      { signal, onResponse: response => { status = response.status; } });
    const op = new Operation(LIMITS.operationMs, signal);
    let constraint: FailureConstraint = 'body';
    try {
      const bytes = await collect(body, LIMITS.control, op);
      constraint = 'json';
      const value = parseJSON(bytes, LIMITS.control);
      if (status === 503 && value?.error?.code === 'service_quiescing') {
        let failure: GatewayError | undefined;
        try { failure = gatewayError(value, status); } catch { /* Malformed envelope remains an HTTP rejection. */ }
        if (failure) throw failure;
      }
      if (status !== 200) throw new DiagnosticFailure('balance', 'http', 'uncertain', status);
      return value;
    } catch (error) {
      if (error instanceof GatewayError) throw error;
      if (error instanceof DiagnosticFailure) throw new DiagnosticFailure(error.stage, error.constraint, error.code, error.status ?? status);
      throw new DiagnosticFailure('balance', error instanceof OperationFailure ? error.constraint : constraint, 'uncertain', status);
    } finally { op.close(); }
  }
  async chat(chat: Chat, bearer: string, options: ResponseOptions = {}): Promise<ReadableStream<Uint8Array>> {
    return this.#encrypted('/v1/chat/completions', encodeChat(chat), bearer, options);
  }
  async #encrypted(path: string, bytes: Uint8Array<ArrayBuffer>, bearer?: string, options: ResponseOptions = {}): Promise<ReadableStream<Uint8Array>> {
    this.#policyCheck();
    const op = new Operation(path === '/v1/chat/completions' ? null : LIMITS.operationMs, options.signal);
    let sent = false, res: Response | undefined, status: number | undefined;
    let constraint: FailureConstraint = 'encryption';
    let frameFailure: DiagnosticFailure | undefined;
    try {
      const encrypted = await op.wait<{ request: Request; context: RequestContext | null }>(this.#identity.encryptRequestWithContext(request(this.#origin + path, op, 'POST', bytes, bearer)));
      requireThat(encrypted.context && !op.controller.signal.aborted);
      sent = true; // Once handed to fetch, receipt/admission is uncertain on ANY failure.
      constraint = 'fetch';
      res = await send(encrypted.request, op, value => { status = value; constraint = 'http'; });
      if (options.onResponse) {
        const mime = res.headers.get('content-type')?.split(';', 1)[0].trim().toLowerCase();
        // Never hand the Response, arbitrary header values, or EHBP diagnostics to hooks.
        const info: ResponseInfo = Object.freeze({ status: res.status,
          contentType: mime === 'text/event-stream' || mime === 'application/json' ? mime : null });
        op.check();
        try { await op.wait(Promise.resolve(options.onResponse(info)), LIMITS.idleMs); }
        catch (error) { throw new DiagnosticFailure('hook', error instanceof OperationFailure ? error.constraint : 'unexpected', 'uncertain', status); }
      }
      constraint = 'endpoint_binding';
      requireThat(res.body && /^[0-9a-f]{64}$/.test(res.headers.get('Ehbp-Response-Nonce') ?? ''));
      const bounded = new Response(encryptedFrames(res.body, op, failure => { frameFailure ??= failure; }), { headers: res.headers });
      constraint = 'decryption';
      const decrypted = await op.wait<Response>(this.#identity.decryptResponseWithContext(bounded, encrypted.context), null);
      requireThat(decrypted.body);
      const body = plaintext(decrypted.body, op, path === '/v1/chat/completions', () => frameFailure);
      byteTimedBodies.add(body);
      return body;
    } catch (error) {
      const observedFailure = frameFailure;
      op.close();
      if (res?.body && !res.body.locked) await cleanup(res.body.cancel());
      if (observedFailure) throw new DiagnosticFailure(observedFailure.stage, observedFailure.constraint, observedFailure.code, status);
      if (error instanceof DiagnosticFailure) throw error;
      throw new DiagnosticFailure(path === '/v1/sessions' ? 'authentication' : path === '/v1/submissions' ? 'submission' : 'transport', error instanceof OperationFailure ? error.constraint : constraint, sent ? 'uncertain' : 'rejected', status);
    }
  }
}
