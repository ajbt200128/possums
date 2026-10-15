import assert from 'node:assert/strict';
import test from 'node:test';
import net from 'node:net';
import tls from 'node:tls';
import dns from 'node:dns';
import { once } from 'node:events';
import { createRequire, syncBuiltinESMExports } from 'node:module';
import { createHash, X509Certificate } from 'node:crypto';
import { readFileSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { createHook } from 'node:async_hooks';
import { createTorSocksTransport, TorSocksTransportFailure, torSocksTransportDiagnostic } from '../clients/pi/tor-socks-transport.mjs';

// Offline SYNTHETIC endpoints, credentials, certificates and application bytes.
// No live Tor, inference, telemetry, installed client or parent-file changes.
// Set POSSUMS_TOR_TEST_SOCKS to the scratch-installed socks package entry.
const require = createRequire(import.meta.url);
const entry = resolve(process.env.POSSUMS_TOR_TEST_SOCKS ?? 'spikes/tor-client/node_modules/socks/build/index.js');
const root = resolve(dirname(entry), '..');
assert.equal(JSON.parse(readFileSync(join(root, 'package.json'))).version, '2.8.10');
// Hash from the exact npm release, not a patched decoder or private import.
assert.equal(createHash('sha256').update(readFileSync(join(root, 'build/client/socksclient.js'))).digest('hex'),
  '4b473feca4862b5c66ec0efa39ea2d40d07c845886b6081813b07d489fabd897');
const sdk = require(entry);
const selection = port => ({ host: '127.0.0.1', port, timeoutMs: 1500 });
const remote = { host: 'fixture.invalid', port: 12345 };
const tick = () => new Promise(resolve => setImmediate(resolve));
const failureCode = kind => error => {
  assert.ok(error instanceof TorSocksTransportFailure);
  assert.equal(error.diagnostic.code, `possums_tor_socks_${kind}`);
  assert.equal(error.cause, undefined);
  assert.ok(!JSON.stringify(error.diagnostic).includes('synthetic-hostile'));
  return true;
};

// Bounded TEST-ONLY fixture adapted from tests/tor_transport.mjs.
// This server decodes synthetic requests only; it never forwards/resolves them.
async function proxyFixture({ stall, auth = false, rejectAuth = false, wrongCredentials = false, fragment = false,
  truncate, malformed, method, reply = 0, addressType = 1, reserved = 0,
  tlsOptions, replyBytes } = {}) {
  const reached = Object.fromEntries(['tcp', 'method', 'authentication', 'connect', 'tls', 'http', 'closed'].map(stage => [stage, Promise.withResolvers()]));
  const sockets = new Set();
  const timers = new Set();
  const observations = { connections: 0, http: 0, domains: [], methods: [], auth: 0 };
  const send = (socket, bytes) => {
    if (!fragment) { socket.write(bytes); return; }
    let index = 0;
    const next = () => {
      if (socket.destroyed || index === bytes.length) return;
      socket.write(bytes.subarray(index, ++index));
      if (index === bytes.length) return;
      const timer = setTimeout(() => { timers.delete(timer); next(); }, 1);
      timers.add(timer);
    };
    next();
  };
  const server = net.createServer(socket => {
    sockets.add(socket); observations.connections++;
    socket.on('error', () => {});
    socket.once('close', () => { sockets.delete(socket); reached.closed.resolve(); });
    reached.tcp.resolve();
    let state = 'method';
    let buffer = Buffer.alloc(0);
    const onData = chunk => {
      buffer = Buffer.concat([buffer, chunk]);
      assert.ok(buffer.length <= 1024, 'bounded synthetic handshake');
      if (state === 'method') {
        if (buffer.length < 2 || buffer.length < 2 + buffer[1]) return;
        observations.methods.push([...buffer.subarray(2, 2 + buffer[1])]);
        buffer = buffer.subarray(2 + buffer[1]);
        reached.method.resolve();
        if (stall === 'method') return;
        if (truncate === 'method') { socket.end(Buffer.from([5])); return; }
        send(socket, Buffer.from([malformed === 'method' ? 4 : 5, method ?? (auth ? 2 : 0)]));
        state = auth ? 'authentication' : 'connect';
      } else if (state === 'authentication') {
        if (buffer.length < 2 || buffer.length < 3 + buffer[1]) return;
        const length = 3 + buffer[1] + buffer[2 + buffer[1]];
        if (buffer.length < length) return;
        const user = buffer.subarray(2, 2 + buffer[1]).toString();
        const password = buffer.subarray(3 + buffer[1], length).toString();
        const credentialMismatch = wrongCredentials && (user !== 'synthetic-user' || password !== 'synthetic-password');
        buffer = buffer.subarray(length);
        observations.auth++; reached.authentication.resolve();
        if (stall === 'authentication') return;
        if (truncate === 'authentication') { socket.end(Buffer.from([1])); return; }
        send(socket, Buffer.from([malformed === 'authentication' ? 2 : 1, rejectAuth || credentialMismatch ? 1 : 0]));
        state = 'connect';
      } else if (state === 'connect') {
        if (buffer.length < 5) return;
        assert.equal(buffer[3], 3, 'destination names MUST be proxy-resolved');
        if (buffer.length < 7 + buffer[4]) return;
        observations.domains.push(buffer.subarray(5, 5 + buffer[4]).toString());
        reached.connect.resolve();
        if (stall === 'connect') return;
        const response = replyBytes ?? Buffer.from([malformed === 'connect' ? 4 : 5, reply, reserved, addressType, 127, 0, 0, 1, 0, 1]);
        if (truncate === 'connect') { socket.end(response.subarray(0, 7)); return; }
        send(socket, response);
        state = 'application'; buffer = Buffer.alloc(0);
        socket.removeListener('data', onData);
        if (stall === 'tls') {
          socket.on('data', () => reached.tls.resolve());
        } else if (tlsOptions) {
          const secure = new tls.TLSSocket(socket, { isServer: true, secureContext: tls.createSecureContext(tlsOptions) });
          sockets.add(secure); secure.on('error', () => {}); secure.once('close', () => sockets.delete(secure));
          secure.on('secure', () => reached.tls.resolve());
          secure.on('data', () => { observations.http++; reached.http.resolve(); });
        } else {
          socket.on('data', () => { observations.http++; reached.http.resolve(); });
        }
      }
    };
    socket.on('data', onData);
  });
  server.listen(0, '127.0.0.1'); await once(server, 'listening');
  return {
    port: server.address().port, observations,
    reached: stage => reached[stage].promise,
    lateSuccess() { for (const socket of sockets) socket.write(Buffer.from([5, 0, 0, 1, 127, 0, 0, 1, 0, 1])); },
    get socketCount() { return sockets.size; },
    waitClosed() { return Promise.all([...sockets].map(socket => once(socket, 'close'))); },
    async close() {
      for (const timer of timers) clearTimeout(timer);
      for (const socket of sockets) socket.destroy();
      await new Promise(resolve => server.close(resolve));
    },
  };
}

function observeTimers() {
  const timers = new Set();
  const hook = createHook({ init(_id, type, _trigger, resource) { if (type === 'Timeout') timers.add(resource); } });
  hook.enable();
  return {
    assertDisposed() {
      assert.equal([...timers].filter(timer => !timer._destroyed && timer.hasRef()).length, 0,
        'no referenced operation or SDK timeout survives disposal');
    },
    assertSDKRetention() {
      assert.ok([...timers].some(timer => !timer._destroyed && !timer.hasRef()),
        'SDK retains bounded unref timeout memory; do not claim instant zero owners');
    },
    close() { hook.disable(); },
  };
}

for (const config of [
  { host: 'localhost', port: 9050 }, { host: '192.0.2.1', port: 9050 },
  { host: '127.invalid', port: 9050 }, { host: '::ffff:127.0.0.1', port: 9050 },
  { host: '127.0.0.1', port: 0 }, { host: '127.0.0.1', port: 65536 },
  { ...selection(9050), username: 'synthetic' }, { ...selection(9050), username: '', password: '' },
  { ...selection(9050), username: 'x'.repeat(256), password: 'synthetic' },
  { ...selection(9050), timeoutMs: 0 }, { ...selection(9050), requestTls: { rejectUnauthorized: false } },
  { ...selection(9050), custom_auth_response_handler: () => true },
]) test('invalid proxy/credentials/options are closed', () => {
  assert.throws(() => createTorSocksTransport(config, { sdk }), failureCode('configuration'));
});

for (const target of [
  { host: '', port: 443 }, { host: 'x'.repeat(256), port: 443 }, { host: '雪.invalid', port: 443 },
  { host: 'https://fixture.invalid/', port: 443 }, { host: 'user:password@fixture.invalid', port: 443 },
  { host: 'fixture.invalid/path', port: 443 }, { host: 'fixture.invalid?x', port: 443 },
  { host: 'fixture.invalid\n', port: 443 }, { host: '127.1', port: 443 },
  { host: 'fixture.invalid', port: -1 }, { host: 'fixture.invalid', port: 65536 },
  { ...remote, lookup: () => {} },
]) test('malformed routing input rejects before any socket', async () => {
  const transport = createTorSocksTransport(selection(9050), { sdk });
  try { await assert.rejects(transport.connectSocket(target), failureCode('destination')); }
  finally { await transport.close(); }
});

test('unknown diagnostics discard hostile exceptions; no SDK message classification', () => {
  const diagnostic = torSocksTransportDiagnostic(new Error('synthetic-hostile-secret.invalid'));
  assert.equal(diagnostic.constraint, 'cause_unknown');
  assert.ok(!JSON.stringify(diagnostic).includes('synthetic-hostile'));
  assert.equal(new TorSocksTransportFailure('synthetic-hostile').diagnostic.code, 'possums_tor_socks_connection_unknown');
});

for (const options of [{}, { fragment: true }, { auth: true, fragment: true }, { credentials: true }]) {
  test('stock SDK public factory: no-auth, optional auth and fragmentation', { timeout: 4000 }, async () => {
    const fixture = await proxyFixture(options);
    let factorySocket;
    const publicSDK = { SocksClient: { createConnection(options) {
      factorySocket = options.existing_socket;
      assert.equal(factorySocket.connecting, false, 'factory receives CONNECTED owner socket');
      return sdk.SocksClient.createConnection(options);
    } } };
    const transport = createTorSocksTransport({ ...selection(fixture.port),
      ...(options.auth || options.credentials ? { username: 'synthetic-user', password: 'synthetic-password' } : {}) }, { sdk: publicSDK });
    const timers = observeTimers();
    try {
      const socket = await transport.connectSocket(remote);
      assert.equal(socket, factorySocket);
      assert.deepEqual(fixture.observations.domains, ['fixture.invalid']);
      assert.equal(fixture.observations.connections, 1);
      assert.deepEqual(fixture.observations.methods[0], options.auth || options.credentials ? [0, 2] : [0]);
      assert.equal(fixture.observations.auth, options.auth ? 1 : 0);
      assert.equal(socket.listenerCount('data'), 0, 'SDK handlers removed after establishment');
      assert.equal(fixture.observations.http, 0, 'socket connector sends no application bytes');
      await transport.close(); await fixture.reached('closed'); await tick();
      assert.equal(socket.destroyed, true);
      assert.equal(socket.listenerCount('data'), 0);
      assert.equal(socket.listenerCount('connect'), 0);
      // Node's public net.connect({signal}) retains its native stream-finished
      // onerror listener even after close. No SDK or scope error owner remains.
      assert.deepEqual(socket.listeners('error').map(listener => listener.name), ['onerror']);
      timers.assertDisposed(); timers.assertSDKRetention();
    } finally { timers.close(); await transport.close(); await fixture.close(); }
  });
}

for (const options of [
  { auth: true, rejectAuth: true }, { auth: true, wrongCredentials: true },
  { malformed: 'method' }, { method: 1 }, { method: 255 },
  { malformed: 'connect' }, ...Array.from({ length: 8 }, (_, index) => ({ reply: index + 1 })),
]) test(`stock SDK rejects handshake with unknown typed constraint: ${JSON.stringify(options)}`, { timeout: 4000 }, async () => {
  const fixture = await proxyFixture(options);
  const transport = createTorSocksTransport({ ...selection(fixture.port),
    ...(options.auth ? { username: options.wrongCredentials ? 'synthetic-wrong-user' : 'synthetic-user', password: 'synthetic-password' } : {}) }, { sdk });
  try {
    await assert.rejects(transport.connectSocket(remote), failureCode('handshake_unknown'));
    assert.equal(fixture.observations.http, 0);
  } finally { await transport.close(); await fixture.close(); }
});

for (const stage of ['method', 'authentication', 'connect']) {
  for (const action of ['abort', 'close', 'deadline']) {
    test(`actual ${stage} stall: ${action} closes owned socket and removes SDK listeners`, { timeout: 4000 }, async () => {
      const fixture = await proxyFixture({ stall: stage, auth: stage === 'authentication' });
      let socket;
      const publicSDK = { SocksClient: { createConnection(options) {
        socket = options.existing_socket;
        return sdk.SocksClient.createConnection(options);
      } } };
      const transport = createTorSocksTransport({ ...selection(fixture.port), timeoutMs: action === 'deadline' ? 80 : 1500,
        ...(stage === 'authentication' ? { username: 'synthetic-user', password: 'synthetic-password' } : {}) }, { sdk: publicSDK });
      const controller = new AbortController();
      const timers = observeTimers();
      try {
        const rejected = assert.rejects(transport.connectSocket(remote, controller.signal),
          failureCode(action === 'abort' ? 'cancelled' : action === 'close' ? 'closed' : 'deadline'));
        await fixture.reached(stage);
        if (action === 'abort') controller.abort(new Error('synthetic-hostile-reason'));
        if (action === 'close') await transport.close();
        await rejected; await fixture.reached('closed'); await tick();
        assert.equal(socket.destroyed, true);
        assert.equal(socket.listenerCount('data'), 0);
        assert.equal(socket.listenerCount('connect'), 0);
        assert.deepEqual(socket.listeners('error').map(listener => listener.name), ['onerror']);
        timers.assertDisposed();
        fixture.lateSuccess(); await tick();
        assert.equal(fixture.socketCount, 0);
        assert.equal(fixture.observations.http, 0);
      } finally { timers.close(); await transport.close(); await fixture.close(); }
    });
  }
}

for (const stage of ['method', 'authentication', 'connect']) {
  test(`truncated ${stage} EOF rejects promptly; subsequent abort retains primary`, { timeout: 4000 }, async () => {
    const fixture = await proxyFixture({ truncate: stage, auth: stage === 'authentication' });
    const transport = createTorSocksTransport({ ...selection(fixture.port),
      ...(stage === 'authentication' ? { username: 'synthetic-user', password: 'synthetic-password' } : {}) }, { sdk });
    const controller = new AbortController();
    const timers = observeTimers();
    try {
      const rejected = assert.rejects(transport.connectSocket(remote, controller.signal), failureCode('handshake_unknown'));
      await rejected; await fixture.reached('closed');
      controller.abort(); await transport.close(); await tick();
      timers.assertDisposed(); timers.assertSDKRetention();
      assert.equal(fixture.socketCount, 0);
      assert.equal(fixture.observations.http, 0);
    } finally { timers.close(); await transport.close(); await fixture.close(); }
  });
}

test('actual numeric-loopback TCP abort before connect: signal reaches Node socket, no SDK handshake', { timeout: 4000 }, async () => {
  const fixture = await proxyFixture();
  const realConnect = net.connect;
  const controller = new AbortController();
  let socket;
  let calls = 0;
  const publicSDK = { SocksClient: { createConnection(options) { calls++; return sdk.SocksClient.createConnection(options); } } };
  const transport = createTorSocksTransport(selection(fixture.port), { sdk: publicSDK });
  net.connect = options => {
    assert.ok(options.signal instanceof AbortSignal);
    socket = realConnect(options); // Real Node TCP; NOT a fabricated Socket.
    assert.equal(socket.connecting, true);
    controller.abort(new Error('synthetic-hostile-reason'));
    return socket;
  };
  syncBuiltinESMExports();
  const timers = observeTimers();
  try {
    await assert.rejects(transport.connectSocket(remote, controller.signal), failureCode('cancelled'));
    socket.emit('connect'); await tick();
    assert.equal(socket.destroyed, true);
    assert.equal(calls, 0, 'late TCP event must not start SDK');
    assert.equal(socket.listenerCount('data'), 0);
    timers.assertDisposed();
    assert.equal(fixture.observations.http, 0);
  } finally { timers.close(); net.connect = realConnect; syncBuiltinESMExports(); await transport.close(); await fixture.close(); }
});

test('abort wins delayed actual public-factory establishment result; no late socket delivery', { timeout: 4000 }, async () => {
  const fixture = await proxyFixture();
  const established = Promise.withResolvers();
  const release = Promise.withResolvers();
  let socket;
  const publicSDK = { SocksClient: { async createConnection(options) {
    const info = await sdk.SocksClient.createConnection(options);
    socket = info.socket; established.resolve();
    await release.promise;
    return info;
  } } };
  const transport = createTorSocksTransport(selection(fixture.port), { sdk: publicSDK });
  const controller = new AbortController();
  try {
    const rejected = assert.rejects(transport.connectSocket(remote, controller.signal), failureCode('cancelled'));
    await established.promise; controller.abort(); release.resolve();
    await rejected; await fixture.reached('closed'); await tick();
    assert.equal(socket.destroyed, true);
    assert.equal(socket.listenerCount('data'), 0);
    assert.equal(fixture.observations.http, 0);
  } finally { release.resolve(); await transport.close(); await fixture.close(); }
});

for (const action of ['abort', 'close']) test(`established socket remains owned through ${action}`, { timeout: 4000 }, async () => {
  const fixture = await proxyFixture();
  const transport = createTorSocksTransport(selection(fixture.port), { sdk });
  const controller = new AbortController();
  try {
    const socket = await transport.connectSocket(remote, controller.signal);
    if (action === 'abort') controller.abort(); else await transport.close();
    await fixture.reached('closed'); await tick();
    assert.equal(socket.destroyed, true);
    assert.equal(fixture.socketCount, 0);
  } finally { await transport.close(); await fixture.close(); }
});

test('one scope closes both established and pending sockets', { timeout: 4000 }, async () => {
  const complete = await proxyFixture();
  // A single proxy can serve successive connections; hold the second factory
  // result to exercise scope ownership without another network destination.
  const held = Promise.withResolvers();
  const release = Promise.withResolvers();
  let count = 0;
  const publicSDK = { SocksClient: { async createConnection(options) {
    const info = await sdk.SocksClient.createConnection(options);
    if (++count === 2) { held.resolve(); await release.promise; }
    return info;
  } } };
  const transport = createTorSocksTransport(selection(complete.port), { sdk: publicSDK });
  try {
    const socket = await transport.connectSocket(remote);
    const rejected = assert.rejects(transport.connectSocket(remote), failureCode('closed'));
    await held.promise;
    const remoteClosed = complete.waitClosed();
    await transport.close(); release.resolve(); await rejected; await remoteClosed; await tick();
    assert.equal(socket.destroyed, true);
    assert.equal(complete.socketCount, 0);
  } finally { release.resolve(); await transport.close(); await complete.close(); }
});

test('no local DNS or direct destination connection on success/proxy failure', { timeout: 4000 }, async () => {
  const fixture = await proxyFixture();
  const transport = createTorSocksTransport(selection(fixture.port), { sdk });
  const realConnect = net.connect;
  const realLookup = dns.lookup;
  const connects = [];
  let lookups = 0;
  net.connect = options => { connects.push([options.host, options.port]); return realConnect(options); };
  dns.lookup = () => { lookups++; throw new Error('synthetic forbidden local DNS'); };
  syncBuiltinESMExports();
  try {
    const socket = await transport.connectSocket(remote);
    socket.destroy(); await once(socket, 'close'); await fixture.close();
    await assert.rejects(transport.connectSocket(remote), failureCode('proxy'));
    assert.equal(lookups, 0);
    assert.deepEqual(connects, [['127.0.0.1', fixture.port], ['127.0.0.1', fixture.port]]);
  } finally { net.connect = realConnect; dns.lookup = realLookup; syncBuiltinESMExports(); await transport.close(); }
});

test('pre-abort and closed scope send nothing; close is idempotent', async () => {
  const fixture = await proxyFixture();
  const transport = createTorSocksTransport(selection(fixture.port), { sdk });
  try {
    await assert.rejects(transport.connectSocket(remote, AbortSignal.abort('synthetic-hostile')), failureCode('cancelled'));
    await transport.close(); await transport.close();
    await assert.rejects(transport.connectSocket(remote), failureCode('closed'));
    assert.equal(fixture.observations.connections, 0);
  } finally { await fixture.close(); }
});

test('native TLS layered on returned socket: stalled TLS abort destroys underlying channel', { timeout: 4000 }, async () => {
  const fixture = await proxyFixture({ stall: 'tls' });
  const transport = createTorSocksTransport(selection(fixture.port), { sdk });
  const controller = new AbortController();
  let secure;
  try {
    const socket = await transport.connectSocket(remote, controller.signal);
    secure = tls.connect({ socket, servername: remote.host });
    secure.on('error', () => {});
    const finished = new Promise(resolve => secure.once('close', resolve));
    await fixture.reached('tls'); controller.abort();
    await finished; await fixture.reached('closed');
    assert.equal(socket.destroyed, true);
    assert.equal(secure.destroyed, true);
    assert.equal(fixture.observations.http, 0);
  } finally { secure?.destroy(); await transport.close(); await fixture.close(); }
});

for (const mode of ['untrusted', 'pin_match', 'pin_mismatch']) test(`native TLS proof: ${mode} before application bytes (parent owns integration)`, { timeout: 5000 }, async () => {
  const directory = mkdtempSync(join(tmpdir(), 'possums-synthetic-socks-tls-'));
  const generated = spawnSync('openssl', ['req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-days', '1',
    '-subj', '/CN=fixture.invalid', '-addext', 'subjectAltName=DNS:fixture.invalid',
    '-keyout', join(directory, 'key.pem'), '-out', join(directory, 'cert.pem')], { stdio: 'ignore' });
  assert.equal(generated.status, 0, 'synthetic certificate generation');
  const cert = readFileSync(join(directory, 'cert.pem'));
  const fixture = await proxyFixture({ tlsOptions: { key: readFileSync(join(directory, 'key.pem')), cert } });
  const transport = createTorSocksTransport(selection(fixture.port), { sdk });
  let secure;
  try {
    const socket = await transport.connectSocket(remote);
    // A synthetic CA is provided ONLY in this test. Production socket API has
    // no TLS override/callback. Reserved endpoint SPKI integration is parent's.
    secure = tls.connect({ socket, servername: remote.host, ...(mode === 'untrusted' ? {} : { ca: cert }) });
    secure.on('error', () => {});
    if (mode === 'untrusted') {
      await assert.rejects(once(secure, 'secureConnect'), error => error.code === 'DEPTH_ZERO_SELF_SIGNED_CERT');
    } else {
      await once(secure, 'secureConnect');
      assert.equal(secure.authorized, true);
      const received = new X509Certificate(secure.getPeerCertificate().raw).publicKey.export({ type: 'spki', format: 'der' });
      const expected = new X509Certificate(cert).publicKey.export({ type: 'spki', format: 'der' });
      assert.equal(fixture.observations.http, 0);
      const matches = received.equals(mode === 'pin_mismatch' ? Buffer.alloc(expected.length) : expected);
      if (matches) { secure.write('GET /synthetic HTTP/1.1\r\nHost: fixture.invalid\r\n\r\n'); await fixture.reached('http'); }
      else secure.destroy();
      assert.equal(fixture.observations.http, mode === 'pin_match' ? 1 : 0);
    }
    await transport.close();
    assert.equal(socket.destroyed, true);
    if (mode === 'untrusted') assert.equal(fixture.observations.http, 0);
  } finally { secure?.destroy(); await transport.close(); await fixture.close(); rmSync(directory, { recursive: true }); }
});

// Qualification gates MUST fail if the unmodified parser accepts malformed
// input. Do not turn these into passing bug-characterization assertions.
for (const [name, options] of [
  ['bad username/password subnegotiation version', { auth: true, malformed: 'authentication' }],
  ['unsupported CONNECT reply ATYP', { addressType: 2 }],
  ['nonzero CONNECT reply reserved byte', { reserved: 1 }],
  ['unoffered username/password method', { auth: true, omitCredentials: true }],
]) test(`QUALIFICATION GATE: reject ${name}`, { timeout: 4000 }, async () => {
  const fixture = await proxyFixture(options);
  const transport = createTorSocksTransport({ ...selection(fixture.port),
    ...(options.auth && !options.omitCredentials ? { username: 'synthetic-user', password: 'synthetic-password' } : {}) }, { sdk });
  try {
    await assert.rejects(transport.connectSocket(remote), failureCode('handshake_unknown'));
    assert.equal(fixture.observations.http, 0);
  } finally { await transport.close(); await fixture.close(); }
});
