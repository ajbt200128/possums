import type { Duplex } from 'node:stream';
import { createTorNativeTransport, TorNativeTransportFailure, type TorNativeProxy, type TorNativeScope } from './tor-native-transport.mjs';
import { parseTorOnionAddress, TorOnionFailure } from './tor-onion.mjs';
import { TorSetupFailure, TorSetupBackendFailure } from './tor-setup.mjs';
import { TorEndorsementFailure } from './tor-endorsement.mjs';
import { publicTorRequest } from './v3.js';
import { PUBLISHER } from '../../examples/phase01/approval.js';
import { collect, DiagnosticFailure, type Operation } from '../../examples/phase01/limits.js';

// PRIVACY.md data-handling/typed-interface boundaries: memory-only selection,
// no identifiers, proxy credentials, raw causes or socket facts in diagnostics.
export { TorNativeTransportFailure, torNativeTransportDiagnostic } from './tor-native-transport.mjs';
export { TorOnionFailure } from './tor-onion.mjs';
export { TorSetupFailure, TorSetupBackendFailure } from './tor-setup.mjs';
export { TorEndorsementFailure } from './tor-endorsement.mjs';
export function isTorConnectionFailure(error: unknown): error is TorNativeTransportFailure | TorOnionFailure | TorSetupFailure | TorSetupBackendFailure | TorEndorsementFailure {
  return error instanceof TorNativeTransportFailure || error instanceof TorOnionFailure || error instanceof TorSetupFailure
    || error instanceof TorSetupBackendFailure || error instanceof TorEndorsementFailure;
}
// The shared collector deliberately projects unknown errors to ChannelError.
// Observe ONLY the closed native type before that projection, retaining its
// primary category across collector-triggered cancellation. Budgets remain in
// the existing collector; this wrapper does not parse or buffer HTTP.
export async function collectTorBody(body: ReadableStream<Uint8Array> | null, cap: number, op: Operation): Promise<Uint8Array<ArrayBuffer>> {
  if (!body) return collect(body, cap, op);
  const reader = body.getReader();
  let primary: TorNativeTransportFailure | undefined, cancelled = false;
  const observed = new ReadableStream<Uint8Array>({
    async pull(controller) {
      try {
        const { done, value } = await reader.read();
        if (cancelled) return;
        if (done) { reader.releaseLock(); controller.close(); } else controller.enqueue(value);
      } catch (error) {
        if (cancelled) return;
        if (error instanceof TorNativeTransportFailure) primary ??= error;
        reader.releaseLock(); controller.error(error);
      }
    },
    async cancel() {
      cancelled = true;
      try { await reader.cancel(); } finally { reader.releaseLock(); }
    },
  }, { highWaterMark: 0 });
  try { return await collect(observed, cap, op); }
  catch (error) { throw primary ?? error; }
}
const authority = Symbol('local Tor selection');
const gatewayHost = (() => {
  try { return new URL(PUBLISHER.origin).hostname; }
  catch { throw new DiagnosticFailure('trust', 'endpoint_binding'); }
})();
export class TorConnection {
  readonly #transport: TorNativeScope;
  readonly #host: string;
  readonly #rawB: Uint8Array;
  #closed = false;
  private constructor(token: symbol, proxy: TorNativeProxy, onion: string) {
    if (token !== authority) throw new DiagnosticFailure('trust', 'endpoint_binding');
    const parsed = parseTorOnionAddress(onion);
    // Copy B and proxy configuration synchronously, before any async boundary.
    this.#host = parsed.host;
    this.#rawB = new Uint8Array(parsed.identity);
    this.#transport = createTorNativeTransport(Object.freeze({ ...proxy }));
    Object.freeze(this);
  }
  static prepare(proxy: TorNativeProxy, onion: string): TorConnection {
    return new TorConnection(authority, proxy, onion);
  }
  static requireOwned(scope: unknown): asserts scope is TorConnection {
    if (!(scope instanceof TorConnection) || !(#transport in scope)) throw new DiagnosticFailure('trust', 'endpoint_binding');
    scope.check();
  }
  check(): void { if (this.#closed) throw new TorNativeTransportFailure('closed'); }
  get onionIdentity(): Uint8Array { this.check(); return new Uint8Array(this.#rawB); }
  // Raw sockets prove no authority. Only qualifyTorV3 can make a private channel.
  async connectSocket(destination: { host: string; port: number }, signal?: AbortSignal): Promise<Duplex> {
    this.check();
    if (!destination || typeof destination.host !== 'string' || !Number.isInteger(destination.port)
      || destination.port < 1 || destination.port > 65535) throw new TorNativeTransportFailure('destination');
    const remote = destination.host === gatewayHost
      ? { ...destination, host: this.#host, port: 80 } : { ...destination };
    return this.#transport.connectSocket(remote, signal);
  }
  async publicFetch(req: Request, op: Operation): Promise<Response> {
    this.check();
    // Public acquisition is GET-only with no caller-selected credential headers.
    if (req.method !== 'GET' || req.body || [...req.headers].length !== 0 || req.credentials !== 'omit')
      throw new DiagnosticFailure('transport', 'encoding');
    return publicTorRequest(req, op, this);
  }
  close(): Promise<void> { this.#closed = true; return this.#transport.close(); }
}
export const prepareTorConnection = (proxy: TorNativeProxy, onion: string): TorConnection => TorConnection.prepare(proxy, onion);
