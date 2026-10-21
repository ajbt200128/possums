import { Identity, type RequestContext } from 'ehbp';
import { qualifyV3, qualifyTorV3, V3_EVIDENCE_LIMIT, type V3Input, type TorV3Input, type V3Identity } from '../../clients/pi/v3.js';
import { TorConnection, TorNativeTransportFailure, isTorConnectionFailure, collectTorBody } from '../../clients/pi/tor-connection.js';
import { CatalogFailure, GatewayError, gatewayError } from './client.js';
import { IntakeFailure } from './intake-diagnostics.js';
import { PUBLISHER, qualifyApi, qualifyPublished, requireApiApproval, validateKeyConfig, checkApproval, type PublishedRelease } from './approval.js';
import { LIMITS, ChannelError, DiagnosticFailure, OperationFailure, type FailureConstraint, Operation, cleanup, collect, parseJSON, requireThat, serialize, utf8 } from './limits.js';
import { admitInvocation, fields, type Chat } from './tools.js';
export type { Chat, Invocation, Message, Tool, ToolChoice, ToolCall, JSONObject, JSONValue } from './tools.js';
export type { LiveModel, Receipt, CompletionOptions, CompletionEvent, ChatOptions } from './client.js';
export type ResponseInfo = Readonly<{ status: number; contentType: 'text/event-stream' | 'application/json' | null }>;
export type ResponseOptions = { signal?: AbortSignal; onResponse?: (response: ResponseInfo) => void | Promise<void> };
export { API_APPROVALS, validateKeyConfig, checkApproval } from './approval.js';
export { LIMITS, serialize, parseJSON } from './limits.js';
export { ReferenceClient, PreparedChat, consumeCompletion, validateModels } from './client.js';
declare const __PHASE01_FIXTURE__: boolean;

// Fetch decodes HTTP compression. Consumers cap decoded bytes while reading;
// browser/network decompressor allocations and encoded bytes are not observable here.
function request(url: string, op: Operation, method = 'GET', body?: Uint8Array<ArrayBuffer>, bearer?: string): Request {
  op.check();
  const headers = new Headers();
  headers.set('x-possums-intake-diagnostics', 'v1');
  if (body) headers.set('Content-Type', 'application/json');
  if (bearer !== undefined) {
    requireThat(typeof bearer === 'string' && /^[A-Za-z0-9_-]{43}$/.test(bearer));
    headers.set('Authorization', 'Bearer ' + bearer);
  }
  return new Request(url, { method, headers, body, signal: op.controller.signal,
    credentials: 'omit', redirect: 'error', cache: 'no-store', referrerPolicy: 'no-referrer' });
}
type Send = (req: Request, op: Operation) => Promise<Response>;
async function send(req: Request, op: Operation, observeStatus?: (status: number) => void, transport: Send = req => fetch(req)): Promise<Response> {
  op.check();
  const response = await op.wait(transport(req, op), LIMITS.operationMs);
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
  try { return utf8.encode(JSON.stringify(admitInvocation(chat, true))); }
  catch (error) { throw error instanceof DiagnosticFailure ? error : new DiagnosticFailure('request', 'encoding'); }
}
// Check complete framing and the authentication-tag minimum only.
// Authentication, decryption and native decoder limits remain published EHBP.
type TransportFailure = DiagnosticFailure | TorNativeTransportFailure;
function encryptedFrames(body: ReadableStream<Uint8Array>, op: Operation, onFailure: (failure: TransportFailure) => void): ReadableStream<Uint8Array> {
  const reader = body.getReader();
  let sawFrame = false, length = 0, prefix = 0, remaining = 0;
  const cancel = async () => { op.close(); await cleanup(reader.cancel()); };
  op.controller.signal.addEventListener('abort', () => { void cleanup(reader.cancel()); }, { once: true });
  return new ReadableStream({
    async pull(controller) {
      try {
        op.check();
        const { done, value } = await op.wait(reader.read(), LIMITS.idleMs);
        if (done) {
          if (prefix !== 0 || remaining !== 0 || !sawFrame) throw new DiagnosticFailure('transport', 'eof', 'uncertain');
          controller.close(); return;
        }
        for (let i = 0; i < value.length;) {
          if (remaining) { const consumed = Math.min(remaining, value.length - i); i += consumed; remaining -= consumed; }
          else {
            length = length * 256 + value[i++];
            if (++prefix === 4) {
              requireThat(length >= 16);
              sawFrame = true;
              remaining = length; length = 0; prefix = 0;
            }
          }
        }
        controller.enqueue(value);
      } catch (error) {
        const failure = error instanceof DiagnosticFailure || error instanceof TorNativeTransportFailure ? error : new DiagnosticFailure('transport', error instanceof OperationFailure ? error.constraint : error instanceof ChannelError ? 'frames' : 'fetch', 'uncertain');
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
function plaintext(body: ReadableStream<Uint8Array>, op: Operation, sse: boolean, frameFailure: () => TransportFailure | undefined): ReadableStream<Uint8Array> {
  const reader = body.getReader();
  const decoder = new TextDecoder('utf-8', { fatal: true });
  return new ReadableStream({
    async pull(controller) {
      let constraint: FailureConstraint = 'decryption';
      try {
        op.check();
        const { done, value } = await op.wait(reader.read(), null);
        constraint = 'utf8';
        if (done) { if (sse) decoder.decode(); op.close(); controller.close(); return; }
        if (sse) decoder.decode(value, { stream: true });
        controller.enqueue(value);
      } catch (error) {
        const failure = frameFailure() ?? (error instanceof DiagnosticFailure || error instanceof TorNativeTransportFailure ? error : new DiagnosticFailure('transport', error instanceof OperationFailure ? error.constraint : constraint, 'uncertain'));
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
  readonly #send: Send;
  readonly #v3: V3Identity | undefined;
  get endpointIdentity(): V3Identity | undefined { return this.#v3; }
  requireRenewal(previous: Channel): void {
    if (previous.#v3 && (!this.#v3 || this.#v3.onionIdentity !== previous.#v3.onionIdentity
      || this.#v3.freshness <= previous.#v3.freshness))
      throw new DiagnosticFailure('trust', 'policy');
  }
  get release(): PublishedRelease | undefined { return this.#release; }
  private constructor(authority: symbol, origin: string, identity: Identity, policyCheck: () => void = () => {}, release?: PublishedRelease,
    transport: Send = req => fetch(req), v3?: V3Identity) {
    requireThat(authority === channelAuthority);
    let url: URL;
    try { url = new URL(origin); } catch { throw new ChannelError(); }
    requireThat(url.protocol === 'https:' && url.origin === origin && !url.username && !url.password);
    this.#origin = origin; this.#identity = identity; this.#policyCheck = policyCheck; this.#release = release;
    this.#send = transport; this.#v3 = v3;
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
  static async v3(input: V3Input | TorV3Input, signal?: AbortSignal, scope?: TorConnection): Promise<Channel> {
    // Never accept standalone facts or caller-provided authority. Qualify exactly
    // the immutable bytes from which this instance's keys and policy are made.
    if (!(input && typeof input === 'object' && input.evidence instanceof Uint8Array && input.evidence.length > 0 && input.evidence.length <= V3_EVIDENCE_LIMIT &&
      input.nonce instanceof Uint8Array && input.nonce.length === 32 && input.manifest instanceof Uint8Array &&
      input.manifest.length > 0 && input.manifest.length <= LIMITS.provenance &&
      typeof input.certificate === 'string' && input.certificate.length <= LIMITS.certificate)) throw new DiagnosticFailure('trust', 'schema');
    if (!(input.config instanceof Uint8Array && input.config.length === LIMITS.key)) throw new DiagnosticFailure('trust', 'key_config');
    const snapshot = { ...input, evidence: new Uint8Array(input.evidence), nonce: new Uint8Array(input.nonce),
      manifest: new Uint8Array(input.manifest), config: new Uint8Array(input.config) };
    const verified = scope === undefined ? await qualifyV3(snapshot, signal) : await qualifyTorV3(snapshot as TorV3Input, scope, signal);
    let identity: Identity;
    try { identity = await Identity.unmarshalPublicConfig(snapshot.config); }
    catch { throw new DiagnosticFailure('trust', 'key_config'); }
    verified.check();
    if (signal?.aborted) throw new DiagnosticFailure('trust', 'interrupted');
    return new Channel(channelAuthority, PUBLISHER.origin, identity, verified.check, verified.release, verified.send, verified.identity);
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
      if (error instanceof CatalogFailure || error instanceof DiagnosticFailure || isTorConnectionFailure(error)) throw error;
      throw new CatalogFailure('request', error instanceof ChannelError ? error.code : 'rejected');
    }
  }
  async challenge(signal?: AbortSignal): Promise<unknown> { return this.#get('/v1/auth/challenge', LIMITS.control, undefined, signal); }
  #collect(body: ReadableStream<Uint8Array> | null, cap: number, op: Operation): Promise<Uint8Array<ArrayBuffer>> {
    return this.#v3?.onionIdentity === undefined ? collect(body, cap, op) : collectTorBody(body, cap, op);
  }
  async #get(path: '/v1/models' | '/v1/auth/challenge', cap: number, bearer?: string, signal?: AbortSignal): Promise<unknown> {
    this.#policyCheck();
    const op = new Operation(LIMITS.operationMs, signal);
    let sent = false, res: Response | undefined, status: number | undefined;
    let constraint: FailureConstraint = 'encoding';
    const catalog = path === '/v1/models';
    try {
      const req = request(this.#origin + path, op, 'GET', undefined, bearer);
      if (catalog) req.headers.set('x-possums-reasoning-metadata', 'true');
      constraint = 'fetch';
      sent = true; res = await send(req, op, value => { status = value; constraint = 'http'; }, this.#send);
      if (!res.ok) {
        let value: unknown;
        try { value = parseJSON(await this.#collect(res.body, LIMITS.error, op), LIMITS.error); }
        catch { /* Keep the observed HTTP rejection even if its bounded body fails. */ }
        if (!catalog && (value as any)?.error?.code === 'service_quiescing') {
          let failure: GatewayError | undefined;
          try { failure = gatewayError(value, status); } catch { /* Reject malformed envelopes as ordinary HTTP failures. */ }
          if (failure) throw failure;
        }
        const code = sent && op.controller.signal.aborted ? 'uncertain' : 'rejected';
        if (!catalog) {
          let failure: GatewayError | undefined;
          try { failure = gatewayError(value, status); } catch { /* No server text fallback. */ }
          if (failure?.intakeDiagnostic) throw new IntakeFailure('challenge', status, failure.intakeDiagnostic, code);
          if (status !== 503) throw new DiagnosticFailure('challenge', 'http', code, status);
        }
        throw new CatalogFailure('http', code, status, value);
      }
      requireThat(res.ok);
      constraint = 'body';
      return parseJSON(await this.#collect(res.body, cap, op), cap);
    } catch (error) {
      const code = sent && op.controller.signal.aborted ? 'uncertain' : 'rejected';
      if (error instanceof DiagnosticFailure || error instanceof GatewayError || isTorConnectionFailure(error)) throw error;
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
      const value = parseJSON(await this.#collect(body, LIMITS.control, op), LIMITS.control);
      // Only classify a rejection envelope here; successful control admission remains unchanged.
      if (status !== undefined && (status < 200 || status >= 300) && value && Object.hasOwn(value, 'error')) {
        if (status === 503 && value.error?.code === 'service_quiescing') {
          let failure: GatewayError | undefined;
          try { failure = gatewayError(value, status); } catch { /* Malformed envelope remains an HTTP rejection. */ }
          if (failure) throw failure;
        }
        let failure: GatewayError | undefined;
        try { failure = gatewayError(value, status); } catch { /* No server text fallback. */ }
        if (failure?.intakeDiagnostic) throw new IntakeFailure(path === '/v1/sessions' ? 'authentication' : 'submission', status, failure.intakeDiagnostic);
        throw new DiagnosticFailure(path === '/v1/sessions' ? 'authentication' : 'submission', 'http', 'uncertain', status);
      }
      return value;
    }
    catch (error) {
      if (error instanceof GatewayError || error instanceof IntakeFailure || isTorConnectionFailure(error)) throw error;
      if (error instanceof DiagnosticFailure) throw new DiagnosticFailure(error.stage, error.constraint, error.code, error.status ?? status);
      throw new DiagnosticFailure(path === '/v1/sessions' ? 'authentication' : 'submission', error instanceof OperationFailure ? error.constraint : 'body', 'uncertain', status);
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
    let frameFailure: TransportFailure | undefined;
    try {
      const encrypted = await op.wait<{ request: Request; context: RequestContext | null }>(this.#identity.encryptRequestWithContext(request(this.#origin + path, op, 'POST', bytes, bearer)));
      requireThat(encrypted.context && !op.controller.signal.aborted);
      sent = true; // Once handed to fetch, receipt/admission is uncertain on ANY failure.
      constraint = 'fetch';
      res = await send(encrypted.request, op, value => { status = value; constraint = 'http'; }, this.#send);
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
      if (observedFailure instanceof TorNativeTransportFailure) throw observedFailure;
      if (observedFailure) throw new DiagnosticFailure(observedFailure.stage, observedFailure.constraint, observedFailure.code, status);
      if (error instanceof DiagnosticFailure || isTorConnectionFailure(error)) throw error;
      throw new DiagnosticFailure(path === '/v1/sessions' ? 'authentication' : path === '/v1/submissions' ? 'submission' : 'transport', error instanceof OperationFailure ? error.constraint : constraint, sent ? 'uncertain' : 'rejected', status);
    }
  }
}
