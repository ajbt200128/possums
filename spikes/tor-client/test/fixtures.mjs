import { createServer as createHTTPServer } from 'node:http';
import { createServer as createTCPServer, connect } from 'node:net';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { fileURLToPath } from 'node:url';
import { createServer, auth } from 'socksv5-server';
import { TorFailure } from '../tor-client.mjs';

export const fixturePolicy = url => url.protocol === 'http:' && url.hostname.endsWith('.invalid');
export function config(port, extra = {}) {
  return { mode: 'existing', host: '127.0.0.1', port, dns: 'proxy',
    torAssumptionsAcknowledged: true, timeoutMs: 1000, ...extra };
}
export function externalRuntime(progress = 100) {
  return { calls: 0, async waitReadiness() { this.calls++; return { progress }; } };
}
export function syntheticVerifier(port, visits = []) {
  // NOT a production trust verifier. Tests only verify that ALL logical sources
  // traverse the same injected public transport. No credentials or inference.
  return async (transport, signal) => {
    for (const host of ['gateway.invalid', 'github.invalid', 'assets.invalid', 'amd.invalid']) {
      const response = await transport.acquire(new Request(`http://${host}:${port}/evidence`, { signal }));
      if (await response.text() !== 'synthetic-public-evidence') throw new TorFailure('backend_trust');
      visits.push(host);
    }
    return { verified: true };
  };
}
export async function networkFixture({ deny = false, noAuth = false, rejectAuth = false } = {}) {
  let destinationCount = 0, authCount = 0;
  const requested = [];
  const scopes = new Set(); // Synthetic, transient only. Cleared by fixture close.
  const sockets = new Set();
  const track = socket => {
    sockets.add(socket);
    socket.on('error', () => {});
    socket.once('close', () => sockets.delete(socket));
    return socket;
  };
  const http = createHTTPServer((request, response) => {
    destinationCount++;
    if (request.url === '/cut') {
      response.writeHead(200, { 'Content-Type': 'text/plain', 'Content-Length': '100' });
      response.write('synthetic');
      setTimeout(() => request.socket.destroy(), 10);
    } else if (request.url === '/wait') {
      // Controlled by cancellation; no response.
    } else if (request.url === '/redirect') {
      response.writeHead(302, { Location: `http://assets.invalid:${http.address().port}/evidence` }); response.end();
    } else {
      response.end('synthetic-public-evidence');
    }
  });
  http.on('connection', track);
  http.listen(0, '127.0.0.1'); await once(http, 'listening');
  const server = createServer((info, accept, refuse) => {
    // Never use accept()'s automatic external forwarding or system DNS.
    requested.push(info.dstAddr);
    if (deny || !info.dstAddr.endsWith('.invalid') || info.dstPort !== http.address().port) { refuse(); return; }
    const destination = track(connect({ host: '127.0.0.1', port: http.address().port }));
    destination.on('connect', () => {
      const tunnel = accept(true);
      if (!tunnel) { destination.destroy(); return; }
      track(tunnel);
      tunnel.pipe(destination).pipe(tunnel);
      destination.on('close', () => tunnel.destroy());
      tunnel.on('close', () => destination.destroy());
    });
  });
  // Observe all raw sockets for teardown, without retaining payloads.
  server._srv.on('connection', track); // test-library fixture-only ownership
  server.on('error', () => {});
  if (noAuth) server.useAuth(auth.NoneAuth());
  else server.useAuth(auth.UserPasswordAuth((user, password, done) => {
    authCount++;
    if (/^[0-9a-f]{64}$/.test(user) && /^[0-9a-f]{64}$/.test(password)) scopes.add(user + password);
    done(!rejectAuth);
  }));
  server.listen(0, '127.0.0.1'); await once(server, 'listening');
  return {
    port: http.address().port, proxyPort: server.address().port,
    requested,
    get destinations() { return destinationCount; },
    get authentications() { return authCount; },
    get scopeCount() { return scopes.size; },
    loseProxy() { for (const socket of sockets) socket.destroy(); server.close(); },
    async close() {
      scopes.clear(); requested.length = 0;
      for (const socket of sockets) socket.destroy();
      await Promise.all([new Promise(resolve => server.close(resolve)), new Promise(resolve => http.close(resolve))]);
    },
  };
}
export async function stalledProxy() {
  const sockets = new Set();
  const server = createTCPServer(socket => {
    sockets.add(socket); socket.on('error', () => {}); socket.once('close', () => sockets.delete(socket));
    socket.resume(); // Discard input, allowing EOF to close the fixture socket.
    // Intentionally sends no protocol bytes; no handwritten SOCKS parser.
  });
  server.listen(0, '127.0.0.1'); await once(server, 'listening');
  return { port: server.address().port, get socketCount() { return sockets.size; },
    async waitForSocketClose() { await Promise.all([...sockets].map(socket => once(socket, 'close'))); },
    async close() { for (const socket of sockets) socket.destroy(); await new Promise(resolve => server.close(resolve)); } };
}
export function childRuntime(runtimeDirectory, { behavior = 'ready', cleanupFailure = false } = {}) {
  const runtime = {
    runtimeDirectory, launches: 0, stops: 0, child: undefined,
    launch(plan) {
      runtime.launches++;
      if (plan.shell !== false || plan.stdio !== 'ignore') throw new TorFailure('child_launch');
      // Deliberately ignore plan.executable/args: this spike CANNOT launch Tor.
      const child = spawn(process.execPath, [fileURLToPath(new URL('./fake-child.mjs', import.meta.url))], {
        shell: false, detached: false, stdio: ['ignore', 'ignore', 'ignore', 'ipc'],
        env: { FIXTURE_BEHAVIOR: behavior },
      });
      runtime.child = child;
      const exited = new Promise(resolve => child.once('close', resolve));
      let progress = 0;
      child.on('message', event => { if (Number.isInteger(event?.progress)) progress = event.progress; });
      child.on('error', () => {});
      let stopping;
      return {
        async waitReadiness(signal) {
          if (progress === 100) return { progress };
          return new Promise((resolve, reject) => {
            const clear = () => {
              child.off('message', message); child.off('exit', exit);
              signal.removeEventListener('abort', abort);
            };
            const message = event => { if (event?.progress === 100) { clear(); resolve({ progress: 100 }); } };
            const exit = () => { clear(); reject(new TorFailure('bootstrap')); };
            const abort = () => { clear(); reject(new TorFailure('cancelled', 'bootstrap')); };
            if (signal.aborted) { abort(); return; }
            child.on('message', message); child.once('exit', exit); signal.addEventListener('abort', abort, { once: true });
          });
        },
        stop() {
          return stopping ??= (async () => {
            runtime.stops++;
            if (child.exitCode === null && child.signalCode === null) child.kill('SIGTERM');
            await exited;
            if (cleanupFailure) throw new Error('hostile cleanup fixture text');
          })();
        },
      };
    },
  };
  return runtime;
}
