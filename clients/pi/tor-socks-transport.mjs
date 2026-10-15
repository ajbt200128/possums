// UNQUALIFIED offline candidate: four malformed/unoffered-reply gates fail.
// Do not wire this module into the installed provider or send real requests.
import { connect, isIP } from 'node:net';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const failures = new WeakSet();
const definitions = Object.freeze({
  configuration: ['configuration', 'invalid_selection', 'Select a numeric loopback proxy, valid port and optional username/password pair.'],
  destination: ['destination', 'invalid_routing_input', 'Use a bounded ASCII URL hostname and valid port; do not supply a URL or credentials.'],
  dependency: ['configuration', 'dependency_unavailable', 'The candidate requires the unmodified socks 2.8.10 release; it is not qualified for integration.'],
  closed: ['operation', 'scope_closed', 'Explicitly create a new transport scope before a deliberate submission.'],
  cancelled: ['operation', 'caller_cancelled', 'The local socket was stopped; check authoritative request state before another submission.'],
  deadline: ['connection', 'deadline_elapsed', 'Check the selected proxy locally before a deliberate new submission.'],
  proxy: ['proxy_tcp', 'connection_unavailable', 'Check the selected loopback SOCKS listener. No direct fallback was attempted.'],
  handshake_unknown: ['socks_handshake', 'cause_unknown', 'The SDK does not expose a typed handshake constraint. Check proxy configuration locally; share only this code and stage.'],
  connection_unknown: ['connection', 'cause_unknown', 'The local connection cause is unknown. Check proxy configuration locally; share only this code and stage.'],
});

// PRIVACY.md: typed failures and forbidden raw diagnostics; no logs or telemetry.
export class TorSocksTransportFailure extends Error {
  constructor(kind) {
    const key = typeof kind === 'string' && Object.hasOwn(definitions, kind) ? kind : 'connection_unknown';
    const [stage, constraint, action] = definitions[key];
    super(`possums_tor_socks_${key}`);
    this.name = 'TorSocksTransportFailure';
    this.diagnostic = Object.freeze({
      code: this.message, stage, constraint, action,
      outcome: 'Remote completion and billing are unknown. No request was replayed; a deliberate new submission may incur another charge.',
    });
    failures.add(this);
  }
}

export function torSocksTransportDiagnostic(error) {
  return (failures.has(error) ? error : new TorSocksTransportFailure('connection_unknown')).diagnostic;
}

function check(condition, kind) {
  if (!condition) throw new TorSocksTransportFailure(kind);
}

function configuration(input) {
  check(input && typeof input === 'object' && !Array.isArray(input), 'configuration');
  check(Object.keys(input).every(key => ['host', 'port', 'username', 'password', 'timeoutMs'].includes(key)), 'configuration');
  const { host, port, username, password, timeoutMs = 30_000 } = input;
  check(typeof host === 'string' && isIP(host) !== 0 && (host === '::1' || host.startsWith('127.')), 'configuration');
  check(Number.isInteger(port) && port > 0 && port <= 65535, 'configuration');
  check(Number.isInteger(timeoutMs) && timeoutMs > 0 && timeoutMs <= 300_000, 'configuration');
  check((username === undefined && password === undefined)
    || [username, password].every(value => typeof value === 'string' && Buffer.byteLength(value) > 0 && Buffer.byteLength(value) <= 255), 'configuration');
  check(!process.env.NODE_DEBUG, 'configuration');
  return { host, port, username, password, timeoutMs };
}

function destination(input) {
  check(input && typeof input === 'object' && !Array.isArray(input), 'destination');
  check(Object.keys(input).every(key => ['host', 'port'].includes(key)), 'destination');
  const { host, port } = input;
  check(typeof host === 'string' && host.length > 0 && host.length <= 255 && /^[\x21-\x7e]+$/.test(host), 'destination');
  check(Number.isInteger(port) && port > 0 && port <= 65535, 'destination');
  // Validation only, never DNS. Reject URL syntax and normalization surprises.
  const literal = isIP(host);
  let parsed;
  try { parsed = new URL(`https://${literal === 6 ? `[${host}]` : host}:${port}/`); }
  catch { throw new TorSocksTransportFailure('destination'); }
  check(!parsed.username && !parsed.password && parsed.pathname === '/' && !parsed.search && !parsed.hash
    && parsed.hostname.toLowerCase() === (literal === 6 ? `[${host.toLowerCase()}]` : host.toLowerCase()), 'destination');
  return { host, port };
}

function loadSDK() {
  try {
    if (require('socks/package.json').version !== '2.8.10') throw new Error();
    return require('socks');
  } catch { throw new TorSocksTransportFailure('dependency'); }
}

/**
 * UNQUALIFIED research candidate: stock socks 2.8.10 accepts malformed auth
 * versions / unsupported reply ATYP (see tests/tor_socks_transport.mjs).
 * Unwired; not a substitute for application-channel authentication.
 *
 * Only public SocksClient.createConnection({ existing_socket }) is called.
 * Node owns proxy TCP AbortSignal; destroying that same socket stops SOCKS.
 * Optional credentials permit no-auth and imply NO circuit isolation guarantee.
 * Destination names remain proxy-resolved; no direct fallback or TLS override.
 * The parent owns native TLS and its pre-HTTP SPKI/WebPKI checks. Abort/scope
 * close retains ownership of the underlying socket after it is returned.
 *
 * The SDK's internal unref'd timeout cannot be cleared through its public API.
 * It retains handshake/options memory until the bounded timeout; disposal does
 * not promise instant memory erasure or zero whole-process owners. Node/host
 * diagnostics subscribers and Pi/T3 history are separate privacy boundaries.
 * SDK injection exists only for offline qualification, not runtime selection.
 */
export function createTorSocksTransport(input, { sdk = loadSDK() } = {}) {
  const config = configuration(input);
  const operations = new Set();
  let closed = false;
  let closing;

  async function connectSocket(inputDestination, signal) {
    if (closed) throw new TorSocksTransportFailure('closed');
    const remote = destination(inputDestination);
    check(signal === undefined || signal instanceof AbortSignal, 'configuration');
    if (signal?.aborted) throw new TorSocksTransportFailure('cancelled');
    const controller = new AbortController();
    let socket;
    let primary;
    let phase = 'proxy';
    let timer;
    let resolveClosed;
    const disposed = new Promise(resolve => { resolveClosed = resolve; });
    const operation = { stop, disposed };
    operations.add(operation);

    function stop(failure) {
      primary ??= failure;
      clearTimeout(timer);
      controller.abort(); // Never pass the caller's potentially hostile reason.
      socket?.destroy();
    }
    function onAbort() { stop(new TorSocksTransportFailure('cancelled')); }
    function onSocketError() { primary ??= new TorSocksTransportFailure(phase); }
    function onClose() {
      clearTimeout(timer);
      signal?.removeEventListener('abort', onAbort);
      socket.removeListener('error', onSocketError);
      operations.delete(operation);
      resolveClosed();
    }
    signal?.addEventListener('abort', onAbort, { once: true });
    timer = setTimeout(() => stop(new TorSocksTransportFailure('deadline')), config.timeoutMs);
    try {
      socket = connect({ host: config.host, port: config.port, signal: controller.signal });
      socket.on('error', onSocketError);
      socket.once('close', onClose);
      await new Promise((resolve, reject) => {
        function cleanup() {
          socket.removeListener('connect', onConnect);
          socket.removeListener('error', onError);
          socket.removeListener('close', onClosed);
        }
        function onConnect() { cleanup(); resolve(); }
        function onError() { cleanup(); reject(primary ?? new TorSocksTransportFailure('proxy')); }
        function onClosed() { cleanup(); reject(primary ?? new TorSocksTransportFailure('proxy')); }
        socket.once('connect', onConnect);
        socket.once('error', onError);
        socket.once('close', onClosed);
      });
      if (primary || socket.destroyed) throw primary ?? new TorSocksTransportFailure('proxy');
      phase = 'handshake_unknown';
      const info = await sdk.SocksClient.createConnection({
        command: 'connect',
        proxy: { host: config.host, port: config.port, type: 5, userId: config.username, password: config.password },
        destination: remote, existing_socket: socket, timeout: config.timeoutMs,
      });
      if (primary || socket.destroyed) throw primary ?? new TorSocksTransportFailure('handshake_unknown');
      check(info?.socket === socket, 'handshake_unknown');
      clearTimeout(timer);
      return socket;
    } catch (error) {
      stop(failures.has(error) ? error : new TorSocksTransportFailure(phase));
      if (socket) await disposed;
      else {
        signal?.removeEventListener('abort', onAbort);
        operations.delete(operation);
        resolveClosed();
      }
      throw primary;
    }
  }

  function close() {
    if (closing) return closing;
    closed = true;
    const active = [...operations];
    for (const operation of active) operation.stop(new TorSocksTransportFailure('closed'));
    return closing = Promise.all(active.map(operation => operation.disposed)).then(() => {});
  }
  return Object.freeze({ connectSocket, close });
}
