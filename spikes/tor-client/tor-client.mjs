import { randomBytes } from 'node:crypto';
import { isIP, connect } from 'node:net';
import { isAbsolute, join, resolve, sep } from 'node:path';
import { fetch as undiciFetch, Socks5ProxyAgent, errors } from 'undici';
import { isQualifiedUndici } from './undici-qualification.mjs';

// No raw causes, values, URLs or process output cross this projection.
const messages = Object.freeze({
  config: ['configuration', 'invalid', 'Use a numeric loopback SOCKS5 endpoint, proxy DNS and explicit Tor assumptions. Disable raw network debugging.'],
  connector_qualification: ['configuration', 'dependency_unqualified', 'Restore the spike-only pinned dependency and apply its exact qualified offline patch. Do not use stock no-auth negotiation.'],
  missing_exec: ['child_launch', 'executable_missing', 'Install Tor yourself, then select its absolute executable path.'],
  child_launch: ['child_launch', 'launch_rejected', 'Check the selected executable and private Tor directory permissions.'],
  bootstrap: ['bootstrap', 'not_ready', 'Wait for Tor bootstrap; check its configuration locally. No destination traffic was sent.'],
  socks_unavailable: ['socks_connect', 'unavailable', 'Check that your configured loopback SOCKS listener is running. No direct fallback was attempted.'],
  socks_negotiation: ['socks_negotiation', 'rejected', 'Check SOCKS5 and isolation support on the configured Tor listener.'],
  transport_interrupted: ['transport', 'interrupted', 'Delivery was interrupted. Check Tor, then deliberately start a new connection.'],
  backend_trust: ['backend_verification', 'rejected', 'Backend identity/authenticity was rejected. Do not bypass verification; establish fresh verified trust.'],
  cancelled: ['operation', 'cancelled', 'Connection preparation was deliberately stopped.'],
  timeout: ['operation', 'deadline', 'The connection deadline elapsed. Check Tor before deliberately testing again.'],
  closed: ['transport', 'closed', 'This connection scope is closed. Explicitly prepare a new connection.'],
});
const stages = new Set(['configuration', 'child_launch', 'bootstrap', 'readiness', 'socks_connect', 'socks_negotiation', 'transport', 'backend_verification', 'cleanup']);
const failures = new WeakMap();
export class TorFailure extends Error {
  constructor(kind, stage) {
    const known = typeof kind === 'string' && Object.hasOwn(messages, kind) ? messages[kind] : undefined;
    const safeStage = stages.has(stage) ? stage : known?.[0] ?? 'transport';
    const detail = known ?? [safeStage, 'cause_unknown', 'The underlying cause is unknown. Share only this code and stage; explicitly prepare a new connection.'];
    const diagnostic = Object.freeze({
      code: `possums_tor_${known ? kind : `${safeStage}_unknown`}`,
      stage: safeStage, constraint: detail[1], action: detail[2],
      disposition: kind === 'backend_trust' || (!known && safeStage === 'backend_verification') ? 'block_backend' : 'graceful_stop',
      outcome: 'Completion, cancellation and billing are not established by this diagnostic. This prototype did not replay inference; a deliberate new submission may incur another charge.',
    });
    super(diagnostic.code);
    failures.set(this, diagnostic);
  }
}
export function diagnostic(error, stage = 'transport') {
  return failures.get(error) ?? failures.get(new TorFailure('unknown', stage));
}
function safeFailure(error, stage) {
  return failures.has(error) ? error : new TorFailure('unknown', stage);
}
export function backendTrustRejected() { return new TorFailure('backend_trust'); }
function requireConfig(condition) { if (!condition) throw new TorFailure('config'); }
function path(value) {
  requireConfig(typeof value === 'string' && isAbsolute(value) && resolve(value) === value
    && !/[\u0000-\u001f\u007f]/.test(value));
  return value;
}
export function validateConfig(input) {
  requireConfig(input && typeof input === 'object' && !Array.isArray(input));
  const common = ['mode', 'host', 'port', 'dns', 'torAssumptionsAcknowledged', 'timeoutMs', 'readiness'];
  const keys = input.mode === 'child' ? [...common, 'executable', 'stateDirectory', 'launchApproved'] : common;
  requireConfig(Object.keys(input).every(key => keys.includes(key)));
  requireConfig(input.mode === 'existing' || input.mode === 'child');
  requireConfig(typeof input.host === 'string' && isIP(input.host) !== 0
    && (input.host === '::1' || /^127\./.test(input.host)));
  requireConfig(Number.isInteger(input.port) && input.port > 0 && input.port <= 65535);
  requireConfig(input.dns === 'proxy' && input.torAssumptionsAcknowledged === true);
  const timeoutMs = input.timeoutMs ?? 30_000;
  requireConfig(Number.isInteger(timeoutMs) && timeoutMs > 0 && timeoutMs <= 300_000);
  const readiness = input.readiness ?? 'control_bootstrap';
  requireConfig(readiness === 'control_bootstrap' || (input.mode === 'existing' && readiness === 'connection'));
  if (input.mode === 'child') {
    requireConfig(input.launchApproved === true);
    path(input.executable); path(input.stateDirectory);
  }
  return Object.freeze({ ...input, timeoutMs, readiness });
}

// Pure proposal, never launches Tor. The runtime adapter must create/secure/lock
// these directories and refuse conflicts, symlinks and insecure existing modes.
export function childLaunchPlan(config, runtimeDirectory) {
  config = validateConfig(config);
  requireConfig(config.mode === 'child');
  path(runtimeDirectory);
  requireConfig(runtimeDirectory !== config.stateDirectory
    && !runtimeDirectory.startsWith(config.stateDirectory + sep)
    && !config.stateDirectory.startsWith(runtimeDirectory + sep));
  const quote = value => JSON.stringify(value);
  const listener = config.host === '::1' ? `[::1]:${config.port}` : `${config.host}:${config.port}`;
  return Object.freeze({
    executable: config.executable,
    args: Object.freeze(['--defaults-torrc', join(runtimeDirectory, 'empty-defaults'), '-f', join(runtimeDirectory, 'torrc')]),
    shell: false, detached: false, stdio: 'ignore',
    runtimeDirectory,
    // State is independent of login/logout and random SOCKS circuit scopes.
    preserveDirectory: config.stateDirectory,
    torrc: [
      `DataDirectory ${quote(config.stateDirectory)}`,
      `SocksPort ${listener} IsolateSOCKSAuth`,
      'SafeSocks 1', 'ClientOnly 1', 'AvoidDiskWrites 1',
      'ControlPort auto',
      `ControlPortWriteToFile ${quote(join(runtimeDirectory, 'control-port'))}`,
      'CookieAuthentication 1',
      `CookieAuthFile ${quote(join(runtimeDirectory, 'control-cookie'))}`,
      'Log err /dev/null',
    ].join('\n') + '\n',
  });
}

function scope(signal, timeoutMs, stage) {
  const controller = new AbortController();
  let reason;
  const cancel = () => { reason ??= new TorFailure('cancelled', stage()); controller.abort(); };
  if (signal?.aborted) cancel();
  else signal?.addEventListener('abort', cancel, { once: true });
  const timer = setTimeout(() => { reason ??= new TorFailure('timeout', stage()); controller.abort(); }, timeoutMs);
  return {
    signal: controller.signal,
    check() { if (controller.signal.aborted) throw reason ?? new TorFailure('closed'); },
    async wait(promise) {
      this.check();
      let stop;
      const interrupted = new Promise((_, reject) => {
        stop = () => reject(reason ?? new TorFailure('closed'));
        controller.signal.addEventListener('abort', stop, { once: true });
      });
      try { const value = await Promise.race([promise, interrupted]); this.check(); return value; }
      finally { controller.signal.removeEventListener('abort', stop); }
    },
    prepared() { clearTimeout(timer); },
    close() { clearTimeout(timer); signal?.removeEventListener('abort', cancel); controller.abort(); },
  };
}

// Undici handles HTTP/SOCKS/TLS parsing. This connector opens ONLY the configured
// numeric-loopback proxy. Tracking sockets also closes a handshake on cancellation.
function connector(endpoint, operation, sockets, onStage) {
  return (_options, callback) => {
    try { operation.check(); }
    catch (error) { callback(error, null); return; }
    onStage('socks_connect');
    const socket = connect({ host: endpoint.host, port: endpoint.port });
    sockets.add(socket);
    let delivered = false;
    const abort = () => socket.destroy(new TorFailure('cancelled', 'socks_connect'));
    operation.signal.addEventListener('abort', abort, { once: true });
    const onError = () => {
      if (!delivered) { delivered = true; callback(new TorFailure('socks_unavailable'), null); }
    };
    socket.on('error', onError);
    socket.once('close', () => {
      sockets.delete(socket);
      operation.signal.removeEventListener('abort', abort);
    });
    socket.once('connect', () => {
      if (!delivered) { delivered = true; onStage('socks_negotiation'); callback(null, socket); }
    });
  };
}
function transportFailure(error) {
  // fetch wraps typed SDK faults in a cause; do not classify formatted text.
  const cause = error?.cause;
  if (failures.has(error)) return error;
  if (failures.has(cause)) return cause;
  if ([cause, error].some(value => value instanceof errors.ConnectTimeoutError
    || value instanceof errors.HeadersTimeoutError || value instanceof errors.BodyTimeoutError))
    return new TorFailure('timeout', 'transport');
  if (cause instanceof errors.SocketError || error instanceof errors.SocketError
    || ['ECONNRESET', 'EPIPE', 'ETIMEDOUT'].includes(cause?.code ?? error?.code))
    return new TorFailure('transport_interrupted');
  // Socks5ProxyError inherits the base UndiciError hasInstance in this pin:
  // instanceof alone also matches unrelated SDK errors. Use reviewed SDK codes.
  const sdk = cause instanceof errors.UndiciError ? cause : error instanceof errors.UndiciError ? error : undefined;
  if (sdk && ['UND_ERR_SOCKS5', 'UND_ERR_SOCKS5_VERSION', 'UND_ERR_SOCKS5_AUTH_REJECTED',
    'UND_ERR_SOCKS5_AUTH_METHOD', 'UND_ERR_SOCKS5_AUTH_VERSION', 'UND_ERR_SOCKS5_AUTH_FAILED',
    'UND_ERR_SOCKS5_REPLY_VERSION', 'UND_ERR_SOCKS5_ADDR_TYPE',
    ...Array.from({ length: 8 }, (_, index) => `UND_ERR_SOCKS5_REPLY_${index + 1}`)].includes(sdk.code))
    return new TorFailure('socks_negotiation');
  return new TorFailure('unknown', 'transport');
}
function admitRequest(request, fixturePolicy, publicOnly) {
  requireConfig(request instanceof Request);
  let url;
  try { url = new URL(request.url); }
  catch { throw new TorFailure('config'); }
  requireConfig((url.protocol === 'https:' || fixturePolicy?.(url) === true)
    && !url.username && !url.password && !url.hash);
  if (publicOnly) {
    // The backend-verification callback has public-acquisition authority only.
    requireConfig(request.method === 'GET' && request.body === null
      && [...request.headers].length === 0);
  }
}

/**
 * Runtime contract: waitReadiness returns structured control {progress: 100},
 * or explicit external connection-readiness policy (no bootstrap claim). Owned
 * launch synchronously returns a waitReadiness/stop handle so abort cannot lose
 * ownership. Real external adapter exists; owned/control remains synthetic only.
 * Backend verifier is existing caller-owned trust logic, not a Tor replacement.
 */
export class TorClient {
  #config; #runtime; #verify; #fixturePolicy; #status = Object.freeze({ state: 'offline' });
  #active; #preparing = false;
  constructor(config, { runtime, verifyBackend, fixturePolicy } = {}) {
    this.#config = validateConfig(config);
    // Supported dependency debug modes can print raw destinations/errors.
    requireConfig(!process.env.NODE_DEBUG);
    requireConfig(runtime && typeof verifyBackend === 'function');
    this.#runtime = runtime; this.#verify = verifyBackend; this.#fixturePolicy = fixturePolicy;
  }
  // A snapshot only: never launch, bootstrap, verify, renew or fetch here.
  status() { return this.#status; }
  async prepareConnection({ signal } = {}) {
    requireConfig(!this.#preparing && !this.#active);
    this.#preparing = true;
    let stage = this.#config.readiness === 'connection' ? 'readiness' : 'bootstrap';
    let child, agent;
    const sockets = new Set();
    const operation = scope(signal, this.#config.timeoutMs, () => stage);
    let cleanupPromise;
    const cleanup = () => cleanupPromise ??= (async () => {
      operation.close();
      for (const socket of sockets) socket.destroy();
      let failure;
      try { await agent?.destroy(); } catch { failure = new TorFailure('unknown', 'cleanup'); }
      // No PID lookup, kill-by-name, process group or external stop callback.
      try { await child?.stop(); } catch { failure ??= new TorFailure('unknown', 'cleanup'); }
      return failure;
    })();
    try {
      operation.check();
      this.#status = Object.freeze({ state: 'preparing' });
      let readiness;
      if (this.#config.mode === 'child') {
        stage = 'child_launch';
        try {
          const plan = childLaunchPlan(this.#config, this.#runtime.runtimeDirectory);
          child = this.#runtime.launch(plan);
          requireConfig(child && typeof child.stop === 'function' && typeof child.waitReadiness === 'function');
        } catch (error) { throw safeFailure(error, stage); }
        stage = 'bootstrap';
        readiness = await operation.wait(Promise.resolve().then(() => child.waitReadiness(operation.signal)));
      } else {
        readiness = await operation.wait(Promise.resolve().then(() => this.#runtime.waitReadiness(this.#config, operation.signal)));
      }
      // No destination traffic before control readiness where requested. External
      // connection mode deliberately defers readiness to SOCKS + backend checks.
      if (this.#config.readiness === 'control_bootstrap') {
        if (!readiness || readiness.progress !== 100) throw new TorFailure('bootstrap');
      } else if (readiness?.readiness !== 'connection') throw new TorFailure('unknown', 'readiness');
      operation.check();
      stage = 'transport';
      if (!isQualifiedUndici() || Socks5ProxyAgent.supportsRequireAuth !== true)
        throw new TorFailure('connector_qualification');
      const host = this.#config.host === '::1' ? '[::1]' : this.#config.host;
      const username = randomBytes(32).toString('hex');
      const password = randomBytes(32).toString('hex');
      agent = new Socks5ProxyAgent(`socks5://${host}:${this.#config.port}`, {
        username, password, requireAuth: true, connect: connector(this.#config, operation, sockets, value => { stage = value; }),
        connectTimeout: this.#config.timeoutMs,
      });
      let verified = false;
      const send = async (request, publicOnly) => {
        admitRequest(request, this.#fixturePolicy, publicOnly);
        operation.check();
        const signals = AbortSignal.any([operation.signal, request.signal]);
        let response;
        try {
          // Node's bundled Request and external Undici have distinct brands.
          // Pass supported RequestInit fields rather than stringify a Request.
          response = await undiciFetch(request.url, {
            method: request.method, headers: request.headers, body: request.body ?? undefined,
            duplex: 'half', dispatcher: agent, signal: signals, redirect: 'manual',
            credentials: 'omit', cache: 'no-store', referrerPolicy: 'no-referrer',
          });
        } catch (error) {
          operation.check();
          if (request.signal.aborted) throw new TorFailure('cancelled', 'transport');
          throw transportFailure(error);
        }
        stage = 'transport';
        if (!response.body) return response;
        const reader = response.body.getReader();
        const body = new ReadableStream({
          async pull(controller) {
            try {
              operation.check();
              const { done, value } = await reader.read();
              if (done) { stage = 'backend_verification'; controller.close(); }
              else controller.enqueue(value);
            } catch (error) {
              const failure = failures.has(error) ? error
                : request.signal.aborted || operation.signal.aborted ? new TorFailure('cancelled', 'transport') : transportFailure(error);
              try { await reader.cancel(); } catch { /* Preserve primary failure. */ }
              controller.error(failure);
            }
          },
          async cancel() { try { await reader.cancel(); } catch { /* No raw diagnostic. */ } },
        }, { highWaterMark: 0 });
        // Preserve fetch's URL/redirect binding evidence for existing acquisition.
        const wrapped = new Response(body, { status: response.status, statusText: response.statusText, headers: response.headers });
        Object.defineProperties(wrapped, { url: { value: response.url }, redirected: { value: response.redirected } });
        return wrapped;
      };
      const transport = Object.freeze({
        acquire: request => send(request, true),
        fetch: request => {
          if (!verified) throw backendTrustRejected();
          return send(request, false);
        },
      });
      stage = 'backend_verification';
      const result = await operation.wait(Promise.resolve().then(() => this.#verify(transport, operation.signal)));
      operation.check();
      if (result?.verified !== true) throw backendTrustRejected();
      verified = true;
      operation.prepared();
      const connection = Object.freeze({
        transport,
        close: async () => {
          const failure = await cleanup();
          if (this.#active === connection) {
            this.#active = undefined;
            this.#status = failure ? Object.freeze({ state: 'stopped', diagnostic: diagnostic(failure, 'cleanup') }) : Object.freeze({ state: 'offline' });
          }
          return failure ? diagnostic(failure, 'cleanup') : undefined;
        },
      });
      this.#active = connection;
      this.#status = Object.freeze({ state: 'prepared', qualification: 'point_in_time_only',
        readiness: this.#config.readiness, authentication: 'username_password_required' });
      return connection;
    } catch (error) {
      const failure = safeFailure(error, stage);
      const cleanupFailure = await cleanup();
      this.#status = Object.freeze({ state: 'stopped', diagnostic: diagnostic(failure, stage),
        ...(cleanupFailure ? { cleanup: diagnostic(cleanupFailure, 'cleanup') } : {}) });
      throw failure;
    } finally { this.#preparing = false; }
  }
}

// Exactly the normal preparation path; no echo/IP service or verifier shortcut.
export async function setupConnectionTest(client, options) {
  const connection = await client.prepareConnection(options);
  const cleanupFailure = await connection.close();
  if (cleanupFailure) return Object.freeze({ ok: false, diagnostic: cleanupFailure });
  return Object.freeze({ ok: true, qualification: 'point_in_time_only',
    message: 'Configured proxy, required SOCKS username/password negotiation and backend verification completed for this attempt. This does not prove Tor identity, actual circuit isolation, anonymity, ongoing health or earlier billing. No account credentials or inference were sent.' });
}
