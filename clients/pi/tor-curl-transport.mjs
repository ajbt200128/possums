// UNQUALIFIED native research candidate: stock libcurl 8.17/8.22 malformed
// SOCKS authentication-version / reserved-byte gates fail. Do not integrate.
// Not installed or application authorization itself.
// PRIVACY.md: data handling / typed interface failures / forbidden raw diagnostics.
import { isIP } from 'node:net';
import { createRequire } from 'node:module';
import { X509Certificate, createHash } from 'node:crypto';
import { Readable } from 'node:stream';

const require = createRequire(import.meta.url);
const failures = new WeakSet();
const definitions = Object.freeze({
  configuration: ['configuration', 'invalid_selection', 'Select a numeric loopback SOCKS proxy, valid optional credentials and authenticated gateway binding.'],
  dependency: ['startup', 'native_dependency_unavailable', 'Install unmodified node-libcurl 5.1.2 for this Node ABI; integration qualification remains required.'],
  closed: ['operation', 'scope_closed', 'Create a new transport scope explicitly.'],
  cancelled: ['operation', 'caller_cancelled', 'Check authoritative request state before another submission.'],
  deadline: ['connection', 'deadline_elapsed', 'Check the selected local proxy and endpoint.'],
  proxy: ['proxy_tcp', 'connection_unavailable', 'Check the numeric loopback SOCKS listener; no direct fallback was attempted.'],
  socks: ['socks_handshake', 'negotiation_rejected', 'Check proxy authentication and endpoint reachability locally.'],
  tls_handshake: ['tls', 'native_handshake_failed', 'Check endpoint TLS availability; no HTTP completion is established.'],
  tls: ['tls', 'certificate_verification_rejected', 'Reverify endpoint identity, hostname and certificate validity; never disable verification.'],
  pin: ['tls', 'appraised_spki_rejected', 'Acquire and authenticate fresh endpoint evidence before another submission.'],
  request: ['request', 'encoding_rejected', 'Check request construction locally.'],
  http: ['http_headers', 'native_response_rejected', 'Check endpoint HTTP response construction locally.'],
  headers: ['http_headers', 'control_limit_exceeded', 'Check endpoint response headers locally.'],
  redirect: ['http_redirect', 'redirect_rejected', 'Use an explicit trusted HTTPS acquisition; gateway redirects and body replay are forbidden.'],
  partial: ['delivery', 'truncated_response', 'Check authoritative completion and accounting state.'],
  transport: ['delivery', 'native_transport_failure', 'Check the proxy and endpoint; request delivery may be incomplete.'],
  setup_unknown: ['setup', 'cause_unknown', 'The native setup cause is unknown; share only the code and stage.'],
  delivery_unknown: ['delivery', 'cause_unknown', 'The delivery cause is unknown; share only the code and stage.'],
  cleanup_unknown: ['cleanup', 'cause_unknown', 'Local disposal is unconfirmed; close this scope before further use.'],
});
export class TorCurlTransportFailure extends Error {
  constructor(kind) {
    const key = typeof kind === 'string' && Object.hasOwn(definitions, kind) ? kind : 'delivery_unknown';
    const [stage, constraint, action] = definitions[key];
    super(`possums_tor_curl_${key}`);
    this.name = 'TorCurlTransportFailure';
    this.stack = `${this.name}: ${this.message}`;
    this.diagnostic = Object.freeze({ code: this.message, stage, constraint, action,
      outcome: 'Remote completion and billing are unknown. No request was replayed; a deliberate new submission may incur another charge.' });
    failures.add(this);
  }
}
export function torCurlTransportDiagnostic(error) {
  return (failures.has(error) ? error : new TorCurlTransportFailure('delivery_unknown')).diagnostic;
}
function check(condition) { if (!condition) throw new TorCurlTransportFailure('configuration'); }
function loadSDK() {
  try {
    if (require('node-libcurl/package.json').version !== '5.1.2') throw 0;
    return require('node-libcurl');
  } catch { throw new TorCurlTransportFailure('dependency'); }
}
function nativeFailure(code) {
  // Supported CURLcode numeric evidence only: never formatted exception text.
  if (code === 28) return new TorCurlTransportFailure('deadline');
  if ([5, 7].includes(code)) return new TorCurlTransportFailure('proxy');
  if ([67, 97].includes(code)) return new TorCurlTransportFailure('socks');
  if (code === 35) return new TorCurlTransportFailure('tls_handshake');
  if ([51, 58, 60, 77, 83].includes(code)) return new TorCurlTransportFailure('tls');
  if (code === 8) return new TorCurlTransportFailure('http');
  if (code === 90) return new TorCurlTransportFailure('pin');
  if (code === 18) return new TorCurlTransportFailure('partial');
  if ([16, 52, 55, 56, 92, 95].includes(code)) return new TorCurlTransportFailure('transport');
  return new TorCurlTransportFailure('delivery_unknown');
}
function selection(input) {
  check(input && typeof input === 'object' && !Array.isArray(input));
  check(Object.keys(input).every(key => ['host', 'port', 'username', 'password', 'timeoutMs', 'gateway'].includes(key)));
  const { host, port, username, password, timeoutMs = 30_000, gateway } = input;
  check(typeof host === 'string' && isIP(host) && (host === '::1' || host.startsWith('127.')));
  check(Number.isInteger(port) && port > 0 && port <= 65535);
  check(Number.isInteger(timeoutMs) && timeoutMs > 0 && timeoutMs <= 300_000);
  check((username === undefined && password === undefined) || [username, password].every(value =>
    typeof value === 'string' && !value.includes('\0') && Buffer.byteLength(value) > 0 && Buffer.byteLength(value) <= 255));
  check(!process.env.NODE_DEBUG && !process.env.NODE_LIBCURL_DEBUG);
  let binding;
  if (gateway !== undefined) {
    check(gateway && Object.keys(gateway).every(key => ['origin', 'onion', 'expectedLeafPem', 'expectedSpkiSha256'].includes(key)));
    try {
      const origin = new URL(gateway.origin);
      check(origin.protocol === 'https:' && origin.href === `${origin.origin}/` && !origin.username && !origin.password && !isIP(origin.hostname));
      check(typeof gateway.onion === 'string' && /^[a-z2-7]{56}\.onion$/.test(gateway.onion));
      check(typeof gateway.expectedLeafPem === 'string' && Buffer.byteLength(gateway.expectedLeafPem) <= 16_384);
      const leaf = new X509Certificate(gateway.expectedLeafPem);
      const spki = createHash('sha256').update(leaf.publicKey.export({ type: 'spki', format: 'der' })).digest('base64');
      check(spki === gateway.expectedSpkiSha256);
      binding = { origin: origin.origin, onion: gateway.onion, leaf: leaf.toString(), pin: `sha256//${spki}` };
    } catch { throw new TorCurlTransportFailure('configuration'); }
  }
  return { host, port, username, password, timeoutMs, binding };
}

/**
 * Fetch-shaped HTTPS-only scope. Caller authenticates fresh expectedLeafPem and
 * exact reserved TLS-SPKI before creating this factory: this is not attestation.
 * Gateway URL/SNI/Host stay original; only CONNECT_TO changes to B.onion:80.
 * Native OpenSSL validates date/hostname/chain and pin before any HTTP bytes.
 * Public acquisitions use native default roots, with no gateway trust override.
 * Optional SOCKS credentials do not require auth selection or promise isolation.
 * No persistence, logs, debug callbacks, inference retry, or raw socket adoption.
 * Host history/native instrumentation are outside this module's privacy boundary.
 */
export function createTorCurlTransport(input, { sdk = loadSDK() } = {}) {
  let config;
  try { config = selection(input); }
  catch (error) { throw failures.has(error) ? error : new TorCurlTransportFailure('configuration'); }
  const operations = new Set();
  let closed = false;
  let closing;

  function transfer(request, url) {
    return new Promise((resolve, reject) => {
      let curl, multi, responseStream, upload, bodyController, primary, timer, disposing;
      let delivered = false, ended = false, headerBytes = 0;
      const operation = { stop, dispose };
      operations.add(operation);
      function onAbort() { stop(new TorCurlTransportFailure('cancelled')); }
      function dispose() {
        return disposing ??= new Promise(done => setImmediate(() => {
          clearTimeout(timer);
          request.signal.removeEventListener('abort', onAbort);
          let cleanup;
          try {
            // Curl.close() uses its global Multi: remove from our owned Multi first.
            // Separate turn also drains SDK completion callbacks already queued.
            if (curl?.handle.isInsideMultiHandle) multi.removeHandle(curl.handle);
          } catch { cleanup = new TorCurlTransportFailure('cleanup_unknown'); }
          setImmediate(() => {
            for (const release of [
              () => curl?.setUploadStream(null), () => curl?.close(), () => multi?.close(),
              // Successful EOF may precede consumption of bounded buffered bytes.
              () => { if (!ended || primary) responseStream?.destroy(); }, () => upload?.destroy(),
            ]) {
              try { release(); }
              catch { cleanup ??= new TorCurlTransportFailure('cleanup_unknown'); }
            }
            if (cleanup && primary) primary.diagnostic = Object.freeze({ ...primary.diagnostic, cleanup: cleanup.diagnostic });
            operations.delete(operation);
            done(cleanup);
          });
        }));
      }
      function stop(failure) {
        primary ??= failure;
        if (!delivered) reject(primary);
        else { try { bodyController?.error(primary); } catch { /* Already terminal. */ } }
        void dispose();
      }
      request.signal.addEventListener('abort', onAbort, { once: true });
      timer = setTimeout(() => stop(new TorCurlTransportFailure('deadline')), config.timeoutMs);
      try {
        curl = new sdk.Curl();
        multi = new sdk.Multi();
        curl.setMulti(multi);
        curl.enable(sdk.CurlFeature.StreamResponse);
        curl.setStreamResponseHighWaterMark(16_384);
        const opts = {
          URL: url.href, PROXY: config.host === '::1' ? '[::1]' : config.host,
          PROXYPORT: config.port, PROXYTYPE: sdk.CurlProxy.Socks5Hostname,
          SOCKS5_AUTH: config.username === undefined ? sdk.CurlAuth.None : sdk.CurlAuth.Basic,
          NOPROXY: '', PRE_PROXY: '', NETRC: sdk.CurlNetrc.Ignored,
          PROTOCOLS_STR: 'https', REDIR_PROTOCOLS_STR: 'https', FOLLOWLOCATION: false,
          SSL_VERIFYPEER: true, SSL_VERIFYHOST: 2, SSL_SESSIONID_CACHE: false,
          HTTP_VERSION: sdk.CurlHttpVersion.V1_1, HTTP09_ALLOWED: false,
          ACCEPT_ENCODING: '',
          FRESH_CONNECT: true, FORBID_REUSE: true, VERBOSE: false,
          CONNECTTIMEOUT_MS: config.timeoutMs, NOSIGNAL: true,
          CUSTOMREQUEST: request.method, USERAGENT: null,
          HTTPHEADER: [...request.headers].filter(([key]) => key !== 'host').map(([key, value]) => `${key}: ${value}`).concat('Expect:'),
        };
        if (config.username !== undefined) {
          opts.PROXYUSERNAME = config.username; opts.PROXYPASSWORD = config.password;
        }
        if (url.origin === config.binding?.origin) {
          opts.CONNECT_TO = [`${url.hostname}:${url.port || 443}:${config.binding.onion}:80`];
          opts.CAINFO_BLOB = config.binding.leaf;
          opts.CAPATH = null;
          opts.PINNEDPUBLICKEY = config.binding.pin;
        }
        if (request.method === 'HEAD') opts.NOBODY = true;
        for (const [key, value] of Object.entries(opts)) curl.setOpt(key, value);
        if (request.body) {
          upload = Readable.fromWeb(request.body, { highWaterMark: 16_384 });
          upload.on('error', () => stop(new TorCurlTransportFailure('request')));
          curl.setOpt('UPLOAD', true);
          curl.setUploadStream(upload);
        }
        curl.setStreamProgressCallback(() => primary ? 1 : 0);
        curl.on('header', chunk => {
          headerBytes += chunk.length;
          if (headerBytes > 65_536) {
            primary ??= new TorCurlTransportFailure('headers');
            // Throw only our closed fault; native callback boundary aborts parsing.
            throw primary;
          }
        });
        curl.on('error', (_raw, code) => stop(primary ?? nativeFailure(code)));
        curl.on('end', () => {
          ended = true;
          void dispose().then(cleanup => { if (cleanup) stop(cleanup); });
        });
        curl.on('stream', (stream, status, parsed) => {
          responseStream = stream;
          stream.on('error', () => stop(primary ?? new TorCurlTransportFailure('delivery_unknown')));
          if (primary) { void dispose(); return; }
          clearTimeout(timer);
          try {
            // Headers are decoded by the maintained SDK, never a second HTTP parser.
            const headers = new Headers();
            const last = parsed.at(-1);
            for (const [key, value] of Object.entries(last ?? {})) {
              if (key.toLowerCase() === 'result') continue;
              for (const item of Array.isArray(value) ? value : [value]) headers.append(key, String(item));
            }
            const reader = Readable.toWeb(stream, { strategy: { highWaterMark: 0 } }).getReader();
            const body = new ReadableStream({
              start(controller) { bodyController = controller; },
              async pull(controller) {
                try {
                  if (primary) throw primary;
                  const item = await reader.read();
                  if (primary) throw primary;
                  if (item.done) {
                    const cleanup = await dispose();
                    if (cleanup) throw cleanup;
                    controller.close();
                  } else controller.enqueue(item.value);
                } catch (error) {
                  stop(failures.has(error) ? error : new TorCurlTransportFailure('delivery_unknown'));
                }
              },
              async cancel() {
                stop(new TorCurlTransportFailure('cancelled'));
                await dispose();
              },
            }, { highWaterMark: 0 });
            const noBody = request.method === 'HEAD' || [204, 205, 304].includes(status);
            const response = new Response(noBody ? null : body, { status, headers });
            Object.defineProperties(response, { url: { value: url.href }, redirected: { value: false, configurable: true } });
            delivered = true;
            resolve(response);
            if (noBody) void dispose();
          } catch { stop(new TorCurlTransportFailure('delivery_unknown')); }
        });
        if (request.signal.aborted) onAbort();
        else curl.perform();
      } catch { stop(new TorCurlTransportFailure('setup_unknown')); }
    });
  }

  async function scopedFetch(resource, init) {
    if (closed) throw new TorCurlTransportFailure('closed');
    let request;
    try { request = new Request(resource, init); }
    catch { throw new TorCurlTransportFailure('request'); }
    if (request.signal.aborted) throw new TorCurlTransportFailure('cancelled');
    for (let redirects = 0; ; redirects++) {
      let url;
      try { url = new URL(request.url); }
      catch { throw new TorCurlTransportFailure('request'); }
      check(url.protocol === 'https:' && !url.username && !url.password && !url.hash);
      const response = await transfer(request, url);
      if (![301, 302, 303, 307, 308].includes(response.status)) {
        if (redirects) Object.defineProperty(response, 'redirected', { value: true });
        return response;
      }
      const location = response.headers.get('location');
      if (request.redirect === 'manual' || !location) return response;
      await response.body?.cancel();
      if (url.origin === config.binding?.origin || request.redirect === 'error' || redirects >= 20
        || !['GET', 'HEAD'].includes(request.method)) throw new TorCurlTransportFailure('redirect');
      let next;
      try { next = new URL(location, url); }
      catch { throw new TorCurlTransportFailure('redirect'); }
      if (next.protocol !== 'https:' || next.username || next.password || next.origin === config.binding?.origin)
        throw new TorCurlTransportFailure('redirect');
      const headers = new Headers(request.headers);
      if (next.origin !== url.origin) for (const key of ['authorization', 'cookie', 'proxy-authorization', 'host']) headers.delete(key);
      request = new Request(next, { method: request.method, headers, signal: request.signal, redirect: request.redirect });
    }
  }
  function close() {
    if (closing) return closing;
    closed = true;
    const active = [...operations];
    for (const operation of active) operation.stop(new TorCurlTransportFailure('closed'));
    return closing = Promise.all(active.map(operation => operation.dispose())).then(results => {
      const failure = results.find(Boolean);
      if (failure) throw failure;
    });
  }
  return Object.freeze({ fetch: scopedFetch, close });
}
