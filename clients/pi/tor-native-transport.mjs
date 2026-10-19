// Unwired qualification candidate. Raw sockets convey NO authorization.
// PRIVACY.md: data-handling boundaries, forbidden raw diagnostics and typed
// failures. No logs, telemetry, fallback, resolver, TLS or HTTP implementation.
import { spawn } from 'node:child_process';
import { isIP } from 'node:net';
import { isAbsolute } from 'node:path';
import { Duplex } from 'node:stream';

const failures = new WeakSet();
const definitions = Object.freeze({
  configuration: ['configuration', 'invalid_selection', 'Select an absolute helper path, numeric loopback proxy and optional proxy-only credential pair.'],
  destination: ['destination', 'invalid_hostname', 'Supply only a bounded ASCII hostname and valid port, not a URL or Possums credentials.'],
  closed: ['operation', 'scope_closed', 'Create a new scope only for a deliberate new operation.'],
  cancelled: ['operation', 'caller_cancelled', 'Local socket closure was requested; check authoritative remote state before another submission.'],
  deadline: ['connection', 'deadline_elapsed', 'Check the selected local proxy before a deliberate new submission.'],
  proxy: ['proxy_tcp', 'connection_unavailable', 'Check the numeric loopback SOCKS listener. No direct fallback was attempted.'],
  protocol: ['socks_handshake', 'sdk_protocol_rejected', 'Check the proxy supports SOCKS5 and the offered authentication methods.'],
  authentication: ['socks_handshake', 'authentication_rejected', 'Check the optional proxy credentials locally; do not use Possums credentials.'],
  unsupported: ['socks_handshake', 'sdk_feature_unsupported', 'Check the proxy selects a supported offered authentication method.'],
  handshake_bound: ['socks_handshake', 'sdk_message_bound', 'Check the selected proxy emits bounded SOCKS5 replies.'],
  truncated: ['socks_handshake', 'unexpected_eof', 'The proxy closed an incomplete handshake. Check it locally before another submission.'],
  handshake_io: ['socks_handshake', 'transport_unavailable', 'The handshake transport failed. Check the local proxy.'],
  handshake_unknown: ['socks_handshake', 'cause_unknown', 'The SDK handshake cause is unknown. Share only this code and stage.'],
  tunnel: ['socks_connect', 'proxy_rejected_destination', 'The proxy refused the destination. Check endpoint availability; no fallback was attempted.'],
  helper: ['helper_start', 'artifact_unavailable', 'Build and select the matching native helper for this platform.'],
  early_data: ['helper_ipc', 'application_bytes_before_ready', 'Use the matching connector/helper; application bytes may be sent only after the private READY record. No direct fallback was attempted.'],
  ipc_unknown: ['helper_ipc', 'cause_unknown', 'The private readiness exchange was invalid or interrupted; share only this code and stage.'],
  bridge_unknown: ['byte_bridge', 'cause_unknown', 'The owned byte pipe was interrupted; remote completion is unknown.'],
});
export class TorNativeTransportFailure extends Error {
  constructor(kind) {
    const key = typeof kind === 'string' && Object.hasOwn(definitions, kind) ? kind : 'ipc_unknown';
    const [stage, constraint, action] = definitions[key];
    super(`possums_tor_native_${key}`);
    delete this.stack;
    this.name = 'TorNativeTransportFailure';
    this.diagnostic = Object.freeze({ code: this.message, stage, constraint, action,
      outcome: 'Remote completion and billing are unknown. No request was replayed; a deliberate new submission may incur another charge.' });
    failures.add(this);
  }
}
export function torNativeTransportDiagnostic(error) {
  return (failures.has(error) ? error : new TorNativeTransportFailure('ipc_unknown')).diagnostic;
}
function check(condition, kind = 'configuration') {
  if (!condition) throw new TorNativeTransportFailure(kind);
}
function configuration(input) {
  check(input && typeof input === 'object' && !Array.isArray(input));
  check(Object.keys(input).every(key => ['helperPath', 'host', 'port', 'username', 'password', 'timeoutMs'].includes(key)));
  const { helperPath, host, port, username, password, timeoutMs = 30_000 } = input;
  check(typeof helperPath === 'string' && isAbsolute(helperPath) && !helperPath.includes('\0'));
  check(typeof host === 'string' && isIP(host) !== 0 && (host === '::1' || host.startsWith('127.')));
  check(Number.isInteger(port) && port > 0 && port <= 65535);
  check(Number.isInteger(timeoutMs) && timeoutMs > 0 && timeoutMs <= 300_000);
  check((username === undefined && password === undefined) || [username, password].every(
    value => typeof value === 'string' && Buffer.byteLength(value) > 0 && Buffer.byteLength(value) <= 255));
  // Native child receives an empty env. Parent Node debug/subscribers are an
  // independent host boundary; refuse the supported debug logging switches.
  check(!process.env.NODE_DEBUG && !process.env.NODE_DEBUG_NATIVE);
  return { helperPath, host, port, username, password, timeoutMs };
}
function destination(input) {
  check(input && typeof input === 'object' && !Array.isArray(input), 'destination');
  check(Object.keys(input).every(key => ['host', 'port'].includes(key)), 'destination');
  const { host, port } = input;
  check(typeof host === 'string' && host.length > 0 && host.length <= 255
    && /^[a-zA-Z0-9.-]+$/.test(host) && host !== '.' && !host.includes('..'), 'destination');
  check(Number.isInteger(port) && port > 0 && port <= 65535, 'destination');
  return { host, port };
}

/**
 * One helper owns one TCP socket. This generic Duplex owns only the child's
 * stdio streams, not a duplicated/adopted network FD. Suitable for tls.connect
 * ({ socket }); the caller MUST authenticate TLS before creating HTTP.
 * Official 0.47.0 ignores SOCKS RSV; success proves neither proxy authenticity,
 * circuit isolation, endpoint identity nor authorization to send credentials.
 * Optional proxy credentials permit the SDK's offered no-auth selection.
 * There is no generation-age timer, publicFetch override or integration with
 * pinnedRequest. Native packaging/installation and parent trust gates remain
 * separate. Host diagnostic subscribers/history are outside this boundary.
 */
export function createTorNativeTransport(input) {
  const config = configuration(input);
  const operations = new Set();
  let closed = false;
  let closing;

  async function connectSocket(inputDestination, signal) {
    if (closed) throw new TorNativeTransportFailure('closed');
    const remote = destination(inputDestination);
    check(signal === undefined || signal instanceof AbortSignal);
    if (signal?.aborted) throw new TorNativeTransportFailure('cancelled');
    const encoded = Buffer.from(JSON.stringify({ proxy_host: config.host, proxy_port: config.port,
      ...remote, username: config.username, password: config.password, timeout_ms: config.timeoutMs }));
    check(encoded.length < 4096);
    let child, socket, primary, timer, ready = false, settled = false;
    let resolveReady, rejectReady, resolveDisposed;
    const readiness = new Promise((resolve, reject) => { resolveReady = resolve; rejectReady = reject; });
    const disposed = new Promise(resolve => { resolveDisposed = resolve; });
    const operation = { stop, disposed };
    operations.add(operation);

    function stop(failure) {
      primary ??= failure;
      clearTimeout(timer);
      if (!settled) { settled = true; rejectReady(primary); }
      socket?.destroy(primary);
      if (child) {
        // Immediate kill avoids an unowned escalation timer or a hung reader.
        // Retain operation ownership until child close confirms exit + IPC EOF.
        for (const stream of child.stdio) stream?.destroy();
        child.kill('SIGKILL');
      }
    }
    function onAbort() { stop(new TorNativeTransportFailure('cancelled')); }
    function finish() {
      clearTimeout(timer);
      signal?.removeEventListener('abort', onAbort);
      operations.delete(operation);
      resolveDisposed();
    }
    try {
      child = spawn(config.helperPath, [], { env: {}, cwd: '/', shell: false,
        stdio: ['pipe', 'pipe', 'ignore', 'pipe', 'pipe'] });
    } catch {
      encoded.fill(0);
      finish();
      throw new TorNativeTransportFailure('helper');
    }
    // Wrapping supported streams preserves Node TLS backpressure/ownership.
    socket = new Duplex({ allowHalfOpen: false,
      read() { child.stdout.resume(); },
      write(chunk, encoding, callback) {
        child.stdin.write(chunk, encoding, error => callback(error ? primary ?? new TorNativeTransportFailure('bridge_unknown') : undefined));
      },
      final(callback) { child.stdin.end(callback); },
      destroy(error, callback) {
        const failure = failures.has(error) ? error : new TorNativeTransportFailure('cancelled');
        stop(failure);
        callback(error ? failure : null);
      },
    });
    // The scope observes closure even when a caller abandons the returned socket.
    socket.on('error', () => {});
    let outputEnded = false, eofDelivered = false;
    function deliverEOF() {
      // Pipe EOF alone cannot distinguish normal native EOF from a failed
      // copy/process exit. Do not let Duplex auto-destroy mask that failure.
      if (!primary && outputEnded && child.exitCode === 0 && !eofDelivered) {
        eofDelivered = true;
        socket.push(null);
      }
    }
    child.stdout.on('data', chunk => { if (!socket.push(chunk)) child.stdout.pause(); });
    child.stdout.once('end', () => { outputEnded = true; deliverEOF(); });
    for (const stream of [child.stdin, child.stdout, child.stdio[3], child.stdio[4]]) {
      stream.on('error', () => stop(new TorNativeTransportFailure(ready ? 'bridge_unknown' : 'ipc_unknown')));
    }
    child.once('error', () => stop(new TorNativeTransportFailure('helper')));
    child.once('exit', code => {
      if (ready && code !== 0) stop(new TorNativeTransportFailure('bridge_unknown'));
      else deliverEOF();
    });
    child.once('close', code => {
      if (!ready || code !== 0) stop(primary ?? new TorNativeTransportFailure(ready ? 'bridge_unknown' : 'ipc_unknown'));
      // EOF of a successful bridge drains readable bytes before auto-destroy.
      else child.stdin.end();
      finish();
    });
    let record = Buffer.alloc(0);
    child.stdio[4].on('data', chunk => {
      if (record.length + chunk.length > 256) return stop(new TorNativeTransportFailure('ipc_unknown'));
      record = Buffer.concat([record, chunk]);
    });
    child.stdio[4].once('end', () => {
      if (primary || settled) return;
      let message;
      try { message = JSON.parse(record.toString('utf8')); }
      catch { return stop(new TorNativeTransportFailure('ipc_unknown')); }
      if (!message || typeof message !== 'object' || Array.isArray(message)) return stop(new TorNativeTransportFailure('ipc_unknown'));
      if (Object.keys(message).join(',') === 'failure' && typeof message.failure === 'string'
        && Object.hasOwn(definitions, message.failure)) return stop(new TorNativeTransportFailure(message.failure));
      if (Object.keys(message).join(',') !== 'ready' || message.ready !== true
        || child.exitCode !== null || child.signalCode !== null || socket.destroyed)
        return stop(new TorNativeTransportFailure('ipc_unknown'));
      ready = true; settled = true; clearTimeout(timer);
      resolveReady(socket);
    });
    signal?.addEventListener('abort', onAbort, { once: true });
    timer = setTimeout(() => stop(new TorNativeTransportFailure('deadline')), config.timeoutMs);
    // Private config FD only; never argv, stdout, stderr or inherited env.
    child.stdio[3].end(encoded, () => encoded.fill(0));
    if (signal?.aborted) onAbort();
    try { return await readiness; }
    catch { await disposed; throw primary; }
  }

  function close() {
    if (closing) return closing;
    closed = true;
    const active = [...operations];
    for (const operation of active) operation.stop(new TorNativeTransportFailure('closed'));
    return closing = Promise.all(active.map(operation => operation.disposed)).then(() => {});
  }
  return Object.freeze({ connectSocket, close });
}
