// UNQUALIFIED offline candidate: truncated-reply cancellation regressions fail.
// Do not wire this module into the installed provider or send real requests.
import { connect, isIP } from 'node:net';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const failures = new WeakSet();
const definitions = Object.freeze({
  configuration: ['configuration', 'invalid_selection', 'Select a numeric loopback SOCKS5 proxy and optional username/password pair.'],
  dependency: ['configuration', 'dependency_unavailable', 'Install the qualified, unmodified Undici 8.12.1 dependency.'],
  closed: ['operation', 'scope_closed', 'Explicitly create a new transport scope.'],
  cancelled: ['operation', 'caller_cancelled', 'The local transport was stopped; check authoritative request state before another submission.'],
  deadline: ['connection', 'deadline_elapsed', 'Check the selected proxy before deliberately making another submission.'],
  proxy: ['proxy_tcp', 'connection_unavailable', 'Check that the selected local SOCKS5 listener is running. No direct fallback was attempted.'],
  method: ['socks_method', 'negotiation_rejected', 'Check the selected proxy supports SOCKS5 and the offered authentication methods.'],
  authentication: ['socks_authentication', 'authentication_rejected', 'Check the optional SOCKS5 credentials locally.'],
  tunnel: ['socks_connect', 'tunnel_rejected', 'Check the selected proxy can reach the requested endpoint. No direct fallback was attempted.'],
  tls: ['tls', 'verification_rejected', 'Check the endpoint certificate and trust configuration; do not disable TLS verification.'],
  request: ['request', 'encoding_rejected', 'Check request construction locally before a deliberate new submission.'],
  connection_unknown: ['connection', 'cause_unknown', 'The connection cause is unknown. Share only this code and stage; check the proxy and endpoint locally.'],
  delivery_unknown: ['delivery', 'cause_unknown', 'The delivery cause is unknown. Share only this code and stage; check authoritative request state.'],
  cleanup_unknown: ['cleanup', 'cause_unknown', 'Local disposal could not be confirmed. Close this transport scope before further use.'],
});

// PRIVACY.md: typed interface failures, forbidden raw diagnostics, no client telemetry.
export class TorTransportFailure extends Error {
  constructor(kind) {
    const key = typeof kind === 'string' && Object.hasOwn(definitions, kind) ? kind : 'connection_unknown';
    const [stage, constraint, action] = definitions[key];
    super(`possums_tor_transport_${key}`);
    this.name = 'TorTransportFailure';
    this.diagnostic = Object.freeze({
      code: this.message, stage, constraint, action,
      outcome: 'Remote completion and billing are unknown. No request was replayed; a deliberate new submission may incur another charge.',
    });
    failures.add(this);
  }
}

export function torTransportDiagnostic(error) {
  return (failures.has(error) ? error : new TorTransportFailure('connection_unknown')).diagnostic;
}

function requireConfiguration(condition) {
  if (!condition) throw new TorTransportFailure('configuration');
}

function configuration(input) {
  requireConfiguration(input && typeof input === 'object' && !Array.isArray(input));
  requireConfiguration(Object.keys(input).every(key => ['host', 'port', 'username', 'password', 'timeoutMs'].includes(key)));
  const { host, port, username, password, timeoutMs = 30_000 } = input;
  requireConfiguration(typeof host === 'string' && isIP(host) !== 0 && (host === '::1' || host.startsWith('127.')));
  requireConfiguration(Number.isInteger(port) && port > 0 && port <= 65535);
  requireConfiguration(Number.isInteger(timeoutMs) && timeoutMs > 0 && timeoutMs <= 300_000);
  requireConfiguration((username === undefined && password === undefined)
    || [username, password].every(value => typeof value === 'string' && Buffer.byteLength(value) > 0 && Buffer.byteLength(value) <= 255));
  // Undici's supported debug mode can print raw destinations and exceptions.
  requireConfiguration(!process.env.NODE_DEBUG);
  return { host, port, username, password, timeoutMs };
}

function loadSDK() {
  try {
    if (require('undici/package.json').version !== '8.12.1') throw new TorTransportFailure('dependency');
    return require('undici');
  } catch { throw new TorTransportFailure('dependency'); }
}

function project(error, sdk, fallback) {
  if (failures.has(error)) return error;
  if (failures.has(error?.cause)) return error.cause;
  const cause = error?.cause ?? error;
  // Match only supported typed SDK faults + closed codes, never exception text.
  if (cause instanceof sdk.errors.UndiciError) {
    const code = cause.code;
    if (['UND_ERR_CONNECT_TIMEOUT', 'UND_ERR_HEADERS_TIMEOUT', 'UND_ERR_BODY_TIMEOUT'].includes(code)) return new TorTransportFailure('deadline');
    if (['UND_ERR_SOCKS5_VERSION', 'UND_ERR_SOCKS5_AUTH_REJECTED', 'UND_ERR_SOCKS5_AUTH_METHOD'].includes(code)) return new TorTransportFailure('method');
    if (['UND_ERR_SOCKS5_AUTH_VERSION', 'UND_ERR_SOCKS5_AUTH_FAILED'].includes(code)) return new TorTransportFailure('authentication');
    if (['UND_ERR_SOCKS5_REPLY_VERSION', 'UND_ERR_SOCKS5_ADDR_TYPE',
      'UND_ERR_SOCKS5_REPLY_1', 'UND_ERR_SOCKS5_REPLY_2', 'UND_ERR_SOCKS5_REPLY_3', 'UND_ERR_SOCKS5_REPLY_4',
      'UND_ERR_SOCKS5_REPLY_5', 'UND_ERR_SOCKS5_REPLY_6', 'UND_ERR_SOCKS5_REPLY_7', 'UND_ERR_SOCKS5_REPLY_8'].includes(code)) return new TorTransportFailure('tunnel');
  }
  if (cause instanceof Error && ['ERR_TLS_CERT_ALTNAME_INVALID', 'DEPTH_ZERO_SELF_SIGNED_CERT',
    'SELF_SIGNED_CERT_IN_CHAIN', 'UNABLE_TO_VERIFY_LEAF_SIGNATURE', 'CERT_HAS_EXPIRED',
    'CERT_NOT_YET_VALID', 'UNABLE_TO_GET_ISSUER_CERT_LOCALLY'].includes(cause.code)) return new TorTransportFailure('tls');
  return new TorTransportFailure(fallback);
}

/**
 * Numeric local proxy only; destination names go to SOCKS5, never local DNS.
 * Optional credentials do NOT require auth selection or promise circuit isolation.
 * SDK injection is for offline qualification, not a selectable network fallback.
 * No TLS override/trust callback is exposed: public HTTPS uses Node defaults.
 * Undici/Node diagnostics-channel subscribers and host history are outside this
 * module's no-logging boundary; do not enable request-level instrumentation.
 */
export function createTorTransport(input, { sdk = loadSDK() } = {}) {
  const config = configuration(input);
  const operations = new Set();
  let closed = false;
  let closing;

  async function scopedFetch(resource, init) {
    if (closed) throw new TorTransportFailure('closed');
    let request;
    try { request = new Request(resource, init); }
    catch { throw new TorTransportFailure('request'); }
    const url = new URL(request.url);
    // HTTP is permitted for onion endpoints and synthetic reserved-name fixtures
    // only. Onion transport is not backend authentication; the caller owns trust.
    if (!(url.protocol === 'https:' || (url.protocol === 'http:'
      && (url.hostname.endsWith('.onion') || url.hostname.endsWith('.invalid'))))
      || url.username || url.password) throw new TorTransportFailure('configuration');
    if (request.signal.aborted) throw new TorTransportFailure('cancelled');

    const controller = new AbortController();
    const sockets = new Set();
    let primary;
    let agent;
    let timer;
    let disposal;
    const operation = { stop, dispose };
    operations.add(operation);

    function stop(failure) {
      primary ??= failure;
      controller.abort();
      // An error can wake active SDK waiters. Qualification shows it cannot
      // clear pending timers after truncated replies already closed the socket.
      // No SOCKS framing or SDK modifications here.
      for (const socket of sockets) socket.destroy(primary);
      clearTimeout(timer);
    }
    function dispose() {
      return disposal ??= (async () => {
        clearTimeout(timer);
        request.signal.removeEventListener('abort', onAbort);
        for (const socket of sockets) socket.destroy(primary ?? new TorTransportFailure('cancelled'));
        let failure;
        try { await agent?.destroy(primary); }
        catch { failure = new TorTransportFailure('cleanup_unknown'); }
        operations.delete(operation);
        return failure;
      })();
    }
    function onAbort() {
      stop(new TorTransportFailure('cancelled'));
      // Keep disposal owned even if the caller abandons its response body.
      void dispose();
    }
    request.signal.addEventListener('abort', onAbort, { once: true });
    timer = setTimeout(() => { stop(new TorTransportFailure('deadline')); void dispose(); }, config.timeoutMs);

    try {
      const proxyHost = config.host === '::1' ? '[::1]' : config.host;
      agent = new sdk.Socks5ProxyAgent(`socks5://${proxyHost}:${config.port}`, {
        username: config.username, password: config.password,
        connectTimeout: config.timeoutMs,
        // The maintained Node TLS hook receives the SAME request lifetime signal.
        // This reaches TLS before Undici has a connected pool socket to destroy.
        requestTls: { signal: controller.signal, rejectUnauthorized: true },
        connect(_options, callback) {
          if (controller.signal.aborted) { callback(primary, null); return; }
          let socket;
          try { socket = connect({ host: config.host, port: config.port }); }
          catch { callback(new TorTransportFailure('proxy'), null); return; }
          sockets.add(socket);
          let delivered = false;
          socket.on('error', () => {
            if (delivered) return;
            delivered = true;
            callback(primary ?? new TorTransportFailure('proxy'), null);
          });
          socket.once('close', () => sockets.delete(socket));
          socket.once('connect', () => {
            if (delivered) return;
            delivered = true;
            if (controller.signal.aborted) { socket.destroy(primary); callback(primary, null); }
            else callback(null, socket);
          });
        },
      });
      if (controller.signal.aborted) throw primary;
      const response = await sdk.fetch(request.url, {
        method: request.method, headers: request.headers, body: request.body ?? undefined,
        duplex: 'half', dispatcher: agent, signal: controller.signal,
        redirect: 'manual', credentials: 'omit', cache: 'no-store', referrerPolicy: 'no-referrer',
      });
      if (controller.signal.aborted) throw primary;
      clearTimeout(timer);
      if (!response.body) {
        const failure = await dispose();
        if (failure) throw failure;
        return response;
      }
      const reader = response.body.getReader();
      const body = new ReadableStream({
        async pull(stream) {
          try {
            if (primary) throw primary;
            const { done, value } = await reader.read();
            if (primary) throw primary;
            if (done) {
              const failure = await dispose();
              if (failure) throw failure;
              stream.close();
            }
            else stream.enqueue(value);
          } catch (error) {
            primary ??= project(error, sdk, 'delivery_unknown');
            stop(primary);
            try { await reader.cancel(); } catch { /* Preserve the closed primary. */ }
            const cleanup = await dispose();
            if (cleanup) primary.diagnostic = Object.freeze({ ...primary.diagnostic, cleanup: cleanup.diagnostic });
            stream.error(primary);
          }
        },
        async cancel() {
          stop(new TorTransportFailure('cancelled'));
          try { await reader.cancel(); } catch { /* Preserve the closed primary. */ }
          await dispose();
        },
      }, { highWaterMark: 0 });
      const wrapped = new Response(body, { status: response.status, statusText: response.statusText, headers: response.headers });
      Object.defineProperties(wrapped, { url: { value: response.url }, redirected: { value: response.redirected }, type: { value: response.type } });
      return wrapped;
    } catch (error) {
      primary ??= project(error, sdk, 'connection_unknown');
      stop(primary);
      const cleanup = await dispose();
      if (cleanup) primary.diagnostic = Object.freeze({ ...primary.diagnostic, cleanup: cleanup.diagnostic });
      throw primary;
    }
  }

  function close() {
    if (closing) return closing;
    closed = true;
    const active = [...operations];
    for (const operation of active) operation.stop(new TorTransportFailure('closed'));
    return closing = Promise.all(active.map(operation => operation.dispose())).then(results => {
      const failure = results.find(Boolean);
      if (failure) throw failure;
    });
  }
  return Object.freeze({ fetch: scopedFetch, close });
}
