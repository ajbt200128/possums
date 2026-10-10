// Defensive client ceilings, not model/context limits or a whole-runtime RSS bound.
export const LIMITS = Object.freeze({
  chat: 8 * 1024 * 1024, control: 4096, evidence: 64 * 1024,
  bundle: 1024 * 1024, provenance: 512 * 1024, certificate: 16 * 1024,
  key: 41, report: 1184, catalog: 256 * 1024, error: 4096,
  // Nested tool schemas need headroom for the surrounding request envelope.
  depth: 32, nodes: 32768, messages: 4096, models: 256,
  tools: 64, toolArguments: 64 * 1024, totalToolArguments: 256 * 1024,
  chunk: 64 * 1024, frame: 256 * 1024, frames: 65536,
  stream: 64 * 1024 * 1024, sseEvent: 64 * 1024, sseEvents: 65536,
  // Finite control/verification work; streams have no total lifetime.
  operationMs: 600000, bootstrapMs: 10 * 600000, idleMs: 600000, cleanupMs: 100,
});
export class ChannelError extends Error {
  constructor(public readonly code: 'rejected' | 'uncertain' = 'rejected') { super(code); }
}
export type FailureStage = 'trust' | 'challenge' | 'authentication' | 'catalog' | 'request' | 'submission' | 'transport' | 'stream' | 'settlement' | 'provider' | 'hook' | 'balance' | 'compaction';
export type FailureConstraint = 'manifest' | 'attestation' | 'verification' | 'provenance' | 'publisher' | 'certificate' | 'key_config' | 'precision' | 'expired' | 'policy' | 'encoding' | 'fetch' | 'http' | 'body' | 'schema' | 'encryption' | 'endpoint_binding' | 'frames' | 'decryption' | 'utf8' | 'json' | 'envelope' | 'choice' | 'delta' | 'tool' | 'finish' | 'usage' | 'receipt' | 'finish_missing' | 'usage_missing' | 'done_missing' | 'eof' | 'interrupted' | 'idle' | 'deadline' | 'unexpected';
// Content-free observations only: never attach the original exception or cause.
export class DiagnosticFailure extends ChannelError {
  readonly status?: number;
  constructor(readonly stage: FailureStage, readonly constraint: FailureConstraint,
    code: ChannelError['code'] = 'rejected', status?: number) {
    super(code);
    requireThat('trust challenge authentication catalog request submission transport stream settlement provider hook balance compaction'.split(' ').includes(stage));
    requireThat('manifest attestation verification provenance publisher certificate key_config precision expired policy encoding fetch http body schema encryption endpoint_binding frames decryption utf8 json envelope choice delta tool finish usage receipt finish_missing usage_missing done_missing eof interrupted idle deadline unexpected'.split(' ').includes(constraint));
    requireThat(code === 'rejected' || code === 'uncertain');
    if (Number.isInteger(status) && status! >= 100 && status! <= 599) this.status = status;
    this.message = `possums_${stage}_${constraint}`;
    Object.freeze(this);
  }
}
export class OperationFailure extends ChannelError {
  constructor(readonly constraint: 'interrupted' | 'idle' | 'deadline') {
    super();
    requireThat(['interrupted', 'idle', 'deadline'].includes(constraint));
    this.message = `possums_operation_${constraint}`;
    Object.freeze(this);
  }
}
export class JSONDepthError extends ChannelError {
  constructor() { super(); this.message = 'possums_request_json_depth'; }
}
export function requireThat(value: unknown): asserts value {
  if (!value) throw new ChannelError();
}
export const utf8 = new TextEncoder();
export function decode(bytes: Uint8Array): string {
  return new TextDecoder('utf-8', { fatal: true }).decode(bytes);
}
export function base64(value: string, cap: number): Uint8Array<ArrayBuffer> {
  requireThat(typeof value === 'string' && value.length <= Math.ceil(cap / 3) * 4);
  requireThat(/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(value));
  const bytes = Uint8Array.from(atob(value), c => c.charCodeAt(0));
  requireThat(bytes.length <= cap);
  return bytes;
}
export function hex(bytes: Uint8Array): string {
  return Array.from(bytes, b => b.toString(16).padStart(2, '0')).join('');
}
export async function digest(bytes: Uint8Array<ArrayBuffer>): Promise<string> {
  return hex(new Uint8Array(await crypto.subtle.digest('SHA-256', bytes)));
}
// Preflight structure BEFORE JSON.parse. Strings are parsed by the native strict parser.
export function parseJSON(bytes: Uint8Array, cap: number): any {
  requireThat(bytes.length <= cap);
  const text = decode(bytes);
  let depth = 0, nodes = 0, quoted = false, escaped = false, start = 0;
  const stack: { object: boolean; key: boolean; names: Set<string> }[] = [];
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    const top = stack.at(-1);
    if (quoted) {
      if (escaped) escaped = false;
      else if (c === '\\') escaped = true;
      else if (c === '"') {
        quoted = false;
        if (top?.object && top.key) {
          let key: string;
          try { key = JSON.parse(text.slice(start, i + 1)); } catch { throw new ChannelError(); }
          requireThat(!top.names.has(key));
          top.names.add(key);
        }
      }
    } else if (c === '"') { quoted = true; start = i; requireThat(++nodes <= LIMITS.nodes); }
    else if (c === '{' || c === '[') {
      requireThat(++depth <= LIMITS.depth); requireThat(++nodes <= LIMITS.nodes);
      stack.push({ object: c === '{', key: true, names: new Set() });
    }
    else if (c === '}' || c === ']') { requireThat(--depth >= 0); stack.pop(); }
    else if (c === ',' || c === ':') {
      requireThat(++nodes <= LIMITS.nodes);
      if (top?.object) top.key = c === ',';
    }
  }
  requireThat(!quoted && depth === 0);
  try { return JSON.parse(text); } catch { throw new ChannelError(); }
}
// Count exact UTF-8 JSON string size without first making an escaped copy.
function stringSize(text: string, cap: number): number {
  requireThat(typeof text === 'string' && text.length <= cap);
  let size = 2;
  for (let i = 0; i < text.length; i++) {
    const c = text.charCodeAt(i);
    if (c === 34 || c === 92 || c === 8 || c === 9 || c === 10 || c === 12 || c === 13) size += 2;
    else if (c < 32) size += 6;
    else if (c < 128) size++;
    else if (c < 2048) size += 2;
    else if (c >= 0xd800 && c <= 0xdbff && i + 1 < text.length && text.charCodeAt(i + 1) >= 0xdc00 && text.charCodeAt(i + 1) <= 0xdfff) { size += 4; i++; }
    else size += c >= 0xd800 && c <= 0xdfff ? 6 : 3;
    requireThat(size <= cap);
  }
  return size;
}
// Object.assign enumerates native own keys, but the proxy admits ONE descriptor
// and mapped value at a time, before a property is copied. Unlike for-in this also
// rejects hidden fields/accessors. No application-created key/entry/descriptor
// collection: the engine's internal key list is NOT a runtime/RSS bound. Proxies
// supplied by hostile JavaScript and modified intrinsics are not a sandbox target.
export function mapData(value: unknown, max: number, map: (key: string, value: unknown) => unknown): any {
  requireThat(typeof value === 'object' && value !== null);
  const array = Array.isArray(value);
  requireThat(Object.getPrototypeOf(value) === (array ? Array.prototype : Object.prototype) || Object.getPrototypeOf(value) === null);
  const length = array ? Object.getOwnPropertyDescriptor(value, 'length')!.value : 0;
  requireThat(!array || length <= max);
  let count = 0;
  const snapshot = array ? Object.setPrototypeOf([], null) : Object.create(null);
  Object.assign(new Proxy(snapshot, {
    set(target, key, child) {
      requireThat(typeof key === 'string');
      return Reflect.set(target, key, map(key, child));
    },
  }), new Proxy(value, {
    getOwnPropertyDescriptor(target, key) {
      if (array && key === 'length') return Object.getOwnPropertyDescriptor(target, key);
      requireThat(typeof key === 'string' && ++count <= max);
      if (array) requireThat(key === String(count - 1));
      const desc = Object.getOwnPropertyDescriptor(target, key);
      // Pi's pinned TypeBox schemas carry an inert, non-JSON kind marker.
      // Ignore only this non-enumerable data value; never invoke an accessor.
      if (!array && key === '~kind' && desc && 'value' in desc && !desc.enumerable &&
        typeof desc.value === 'string' && /^[A-Za-z][A-Za-z0-9_]{0,63}$/.test(desc.value)) return desc;
      requireThat(desc && 'value' in desc && desc.enumerable);
      return desc;
    },
    get(target, key) {
      const desc = Object.getOwnPropertyDescriptor(target, key);
      requireThat(typeof key === 'string' && desc && 'value' in desc && desc.enumerable);
      return desc.value;
    },
  }));
  requireThat(!array || count === length);
  return snapshot;
}
type AdmittedJSON = string | number | boolean | null | AdmittedJSON[] | { [key: string]: AdmittedJSON };
export function serialize(value: unknown, cap: number): Uint8Array<ArrayBuffer> {
  let nodes = 0, bytesUsed = 0;
  function add(bytes: number): void { bytesUsed += bytes; requireThat(bytesUsed <= cap); }
  function snapshot(v: unknown, depth: number): AdmittedJSON {
    if (depth > LIMITS.depth) throw new JSONDepthError();
    requireThat(++nodes <= LIMITS.nodes);
    if (typeof v === 'string') { add(stringSize(v, cap - bytesUsed)); return v; }
    if (typeof v === 'boolean') { add(v ? 4 : 5); return v; }
    if (typeof v === 'number') { requireThat(Number.isFinite(v)); add(String(v).length); return v; }
    if (v === null) { add(4); return v; }
    const array = Array.isArray(v);
    add(2);
    let count = 0;
    return mapData(v, LIMITS.nodes - nodes, (key, child) => {
      if (count++) add(1);
      if (!array) { add(stringSize(key, cap - bytesUsed)); add(1); }
      return snapshot(child, depth + 1);
    });
  }
  // Never stringify the unchecked input; null prototypes exclude inherited
  // toJSON hooks, and every copied value was admitted to the shared budgets.
  const admitted = snapshot(value, 0);
  const bytes = utf8.encode(JSON.stringify(admitted));
  requireThat(bytes.length === bytesUsed && bytes.length <= cap);
  return bytes;
}
export async function cleanup(action: Promise<unknown>): Promise<void> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  await Promise.race([action.catch(() => {}), new Promise<void>(resolve => { timer = setTimeout(resolve, LIMITS.cleanupMs); })]);
  clearTimeout(timer);
}
export class Operation {
  readonly controller = new AbortController();
  private timer: ReturnType<typeof setTimeout> | undefined;
  private constraint: OperationFailure['constraint'] = 'interrupted';
  private readonly abort = () => {
    const reason = this.parent?.reason;
    if (reason instanceof OperationFailure) this.constraint = reason.constraint;
    this.controller.abort(new OperationFailure(this.constraint));
  };
  private readonly deadline = () => { this.constraint = 'deadline'; this.abort(); };
  constructor(ms: number | null = LIMITS.operationMs, private readonly parent?: AbortSignal) {
    requireThat(parent === undefined || parent instanceof AbortSignal);
    if (ms !== null) this.timer = setTimeout(this.deadline, ms);
    parent?.addEventListener('abort', this.abort, { once: true });
    if (parent?.aborted) this.abort();
  }
  check(): void { if (this.controller.signal.aborted) throw new OperationFailure(this.constraint); }
  close(): void { clearTimeout(this.timer); this.parent?.removeEventListener('abort', this.abort); this.controller.abort(); }
  async wait<T>(promise: Promise<T>, idle: number | null = LIMITS.idleMs): Promise<T> {
    const signal = this.controller.signal;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let abort: () => void = () => {};
    try {
      if (signal.aborted) { void promise.catch(() => {}); throw new OperationFailure(this.constraint); }
      const result = await Promise.race([promise, new Promise<never>((_, reject) => {
        abort = () => reject(new OperationFailure(this.constraint));
        signal.addEventListener('abort', abort, { once: true });
        if (idle !== null) timer = setTimeout(() => { this.constraint = 'idle'; this.abort(); }, idle);
      })]);
      this.check();
      return result;
    } finally { clearTimeout(timer); signal.removeEventListener('abort', abort); }
  }
}
export async function collect(body: ReadableStream<Uint8Array> | null, cap: number, op: Operation): Promise<Uint8Array<ArrayBuffer>> {
  requireThat(body);
  const reader = body.getReader();
  const chunks: Uint8Array[] = [];
  let length = 0;
  try {
    for (;;) {
      op.check();
      const { done, value } = await op.wait(reader.read());
      if (done) break;
      length += value.length;
      requireThat(value.length <= LIMITS.chunk && length <= cap);
      chunks.push(value);
    }
    const bytes = new Uint8Array(length);
    let offset = 0;
    for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.length; }
    return bytes;
  } catch (error) { op.close(); throw error instanceof OperationFailure || error instanceof DiagnosticFailure ? error : new ChannelError(); }
  finally { await cleanup(reader.cancel()); }
}
export async function boundedReport(compressed: Uint8Array<ArrayBuffer>, op: Operation): Promise<Uint8Array<ArrayBuffer>> {
  requireThat(compressed.length <= LIMITS.evidence && typeof DecompressionStream !== 'undefined');
  const body = new Blob([compressed]).stream().pipeThrough(new DecompressionStream('gzip'));
  const bytes = await collect(body, LIMITS.report, op);
  requireThat(bytes.length === LIMITS.report);
  return bytes;
}
