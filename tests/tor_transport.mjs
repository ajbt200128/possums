import assert from 'node:assert/strict';
import test from 'node:test';
import net from 'node:net';
import tls from 'node:tls';
import dns from 'node:dns';
import { once } from 'node:events';
import { createRequire, syncBuiltinESMExports } from 'node:module';
import { createHash } from 'node:crypto';
import { readFileSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { createHook } from 'node:async_hooks';
import { createTorTransport, TorTransportFailure, torTransportDiagnostic } from '../clients/pi/tor-transport.mjs';

// All endpoints, payloads, credentials and keys below are isolated synthetic fixtures.
// No Tor, Internet destination, inference, telemetry or installed-client changes.
const require = createRequire(import.meta.url);
const entry = resolve(process.env.POSSUMS_TOR_TEST_UNDICI ?? 'spikes/tor-client/node_modules/undici/index.js');
const root = dirname(entry);
assert.equal(JSON.parse(readFileSync(join(root, 'package.json'))).version, '8.12.1', 'qualification requires unmodified Undici 8.12.1');
for (const [file, expected] of Object.entries({
  'lib/core/socks5-client.js': '5ee3d782919a74b4c0fc0aac0da1d39abd759f7c017f7b3d4b932ccafddd1be7',
  'lib/dispatcher/socks5-proxy-agent.js': 'fa8a243c5b9eb4b5de4e73ddbefe63426afc8a02743830a4f1e2142767817578',
})) assert.equal(createHash('sha256').update(readFileSync(join(root, file))).digest('hex'), expected, 'unmodified decoder/agent required');
const sdk = require(entry);
const selection = port => ({ host: '127.0.0.1', port, timeoutMs: 1500 });
const syntheticURL = 'http://fixture.invalid:12345/public';
const failureCode = kind => error => {
  assert.ok(error instanceof TorTransportFailure);
  assert.equal(error.diagnostic.code, `possums_tor_transport_${kind}`);
  assert.equal(error.cause, undefined);
  return true;
};
const tick = () => new Promise(resolve => setImmediate(resolve));

// TEST-ONLY SOCKS protocol ownership. It never resolves or forwards a destination.
async function proxyFixture({ stall, auth = false, rejectAuth = false, fragment = false,
  truncate, malformed, reply = 0, body = 'complete', tlsOptions } = {}) {
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
      const timer = setTimeout(() => { timers.delete(timer); next(); }, 1);
      timers.add(timer);
    };
    next();
  };
  const handleHTTP = socket => {
    socket.on('data', () => {
      observations.http++;
      reached.http.resolve();
      if (body === 'stall') socket.write('HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nsynthetic');
      else if (body === 'cut') { socket.write('HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nsynthetic'); socket.end(); }
      else if (body === 'redirect') socket.end('HTTP/1.1 302 Found\r\nLocation: https://never-follow.invalid/\r\nContent-Length: 0\r\n\r\n');
      else socket.end('HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\nsynthetic');
    });
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
      if (state === 'method') {
        if (buffer.length < 2 || buffer.length < 2 + buffer[1]) return;
        observations.methods.push([...buffer.subarray(2, 2 + buffer[1])]);
        buffer = buffer.subarray(2 + buffer[1]);
        reached.method.resolve();
        if (stall === 'method') return;
        if (truncate === 'method') { socket.end(Buffer.from([5])); return; }
        send(socket, Buffer.from([malformed === 'method' ? 4 : 5, auth ? 2 : 0]));
        state = auth ? 'authentication' : 'connect';
      } else if (state === 'authentication') {
        if (buffer.length < 2 || buffer.length < 3 + buffer[1]) return;
        const length = 3 + buffer[1] + buffer[2 + buffer[1]];
        if (buffer.length < length) return;
        buffer = buffer.subarray(length);
        observations.auth++; reached.authentication.resolve();
        if (stall === 'authentication') return;
        if (truncate === 'authentication') { socket.end(Buffer.from([1])); return; }
        send(socket, Buffer.from([malformed === 'authentication' ? 2 : 1, rejectAuth ? 1 : 0]));
        state = 'connect';
      } else if (state === 'connect') {
        if (buffer.length < 5) return;
        assert.equal(buffer[3], 3, 'destination MUST be proxy-resolved domain');
        if (buffer.length < 7 + buffer[4]) return;
        observations.domains.push(buffer.subarray(5, 5 + buffer[4]).toString());
        reached.connect.resolve();
        if (stall === 'connect') return;
        const response = Buffer.from([malformed === 'connect' ? 4 : 5, reply, 0, 1, 127, 0, 0, 1, 0, 1]);
        if (truncate === 'connect') { socket.end(response.subarray(0, 7)); return; }
        send(socket, response);
        state = 'application'; buffer = Buffer.alloc(0);
        socket.removeListener('data', onData);
        if (stall === 'tls') {
          socket.on('data', () => reached.tls.resolve());
        } else if (tlsOptions) {
          const secure = new tls.TLSSocket(socket, { isServer: true, secureContext: tls.createSecureContext(tlsOptions) });
          sockets.add(secure); secure.on('error', () => {}); secure.once('close', () => sockets.delete(secure));
          secure.on('secure', () => reached.tls.resolve()); handleHTTP(secure);
        } else handleHTTP(socket);
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
    // Undici's shared, unreferenced 499ms fast-timer wheel is not an operation timer.
    assertDisposed() { assert.equal([...timers].filter(timer => !timer._destroyed && timer.hasRef()).length, 0, 'no referenced request/handshake deadline survives disposal'); },
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
]) test('invalid proxy/credential/TLS selection is closed', () => {
  assert.throws(() => createTorTransport(config, { sdk }), failureCode('configuration'));
});

test('unknown diagnostics discard hostile exception text and cause', () => {
  const diagnostic = torTransportDiagnostic(new Error('synthetic-hostile-secret.invalid'));
  assert.equal(diagnostic.constraint, 'cause_unknown');
  assert.ok(!JSON.stringify(diagnostic).includes('synthetic-hostile'));
  assert.equal(new TorTransportFailure('synthetic-hostile').diagnostic.code, 'possums_tor_transport_connection_unknown');
});

for (const options of [{}, { fragment: true }, { auth: true, fragment: true }, { body: 'redirect' }]) {
  test('actual SDK: no-auth/optional auth, fragmented replies, manual redirect', { timeout: 4000 }, async () => {
    const fixture = await proxyFixture(options);
    const transport = createTorTransport({ ...selection(fixture.port), ...(options.auth ? { username: 'synthetic-user', password: 'synthetic-password' } : {}) }, { sdk });
    try {
      const response = await transport.fetch(syntheticURL);
      assert.equal(response.status, options.body === 'redirect' ? 302 : 200);
      assert.equal(await response.text(), options.body === 'redirect' ? '' : 'synthetic');
      assert.deepEqual(fixture.observations.domains, ['fixture.invalid']);
      assert.equal(fixture.observations.connections, 1);
      assert.deepEqual(fixture.observations.methods[0], options.auth ? [2, 0] : [0]);
      assert.equal(fixture.observations.auth, options.auth ? 1 : 0);
    } finally { await transport.close(); await fixture.close(); }
  });
}

test('credentials still allow normal no-auth selection; no strict-auth patch', async () => {
  const fixture = await proxyFixture();
  const transport = createTorTransport({ ...selection(fixture.port), username: 'synthetic-user', password: 'synthetic-password' }, { sdk });
  try {
    assert.equal(await (await transport.fetch(syntheticURL)).text(), 'synthetic');
    assert.deepEqual(fixture.observations.methods[0], [2, 0]);
    assert.equal(fixture.observations.auth, 0);
  } finally { await transport.close(); await fixture.close(); }
});

for (const [options, kind] of [
  [{ auth: true, rejectAuth: true }, 'authentication'],
  [{ malformed: 'method' }, 'method'],
  [{ auth: true, malformed: 'authentication' }, 'authentication'],
  [{ malformed: 'connect' }, 'tunnel'], [{ reply: 5 }, 'tunnel'],
  [{ truncate: 'method' }, 'deadline'], [{ auth: true, truncate: 'authentication' }, 'deadline'],
  [{ truncate: 'connect' }, 'deadline'],
]) test(`actual SDK malformed/rejected/truncated ${JSON.stringify(options)}`, { timeout: 4000 }, async () => {
  const fixture = await proxyFixture(options);
  const transport = createTorTransport({ ...selection(fixture.port), timeoutMs: 80,
    ...(options.auth ? { username: 'synthetic-user', password: 'synthetic-password' } : {}) }, { sdk });
  try {
    await assert.rejects(transport.fetch(syntheticURL), failureCode(kind));
    assert.equal(fixture.observations.http, 0);
  } finally { await transport.close(); await fixture.close(); }
});

for (const stage of ['method', 'authentication', 'connect', 'tls']) {
  test(`caller Request abort disposes actual SDK ${stage} handshake and timers`, { timeout: 4000 }, async () => {
    const fixture = await proxyFixture({ stall: stage, auth: stage === 'authentication' });
    const transport = createTorTransport({ ...selection(fixture.port),
      ...(stage === 'authentication' ? { username: 'synthetic-user', password: 'synthetic-password' } : {}) }, { sdk });
    const controller = new AbortController();
    const timers = observeTimers();
    try {
      const pending = transport.fetch(new Request(stage === 'tls' ? 'https://fixture.invalid/public' : syntheticURL, { signal: controller.signal }));
      const rejected = assert.rejects(pending, failureCode('cancelled'));
      await fixture.reached(stage);
      controller.abort(new Error('synthetic-hostile-abort-reason'));
      await rejected;
      await fixture.reached('closed');
      await tick(); timers.assertDisposed();
      fixture.lateSuccess(); await tick();
      assert.equal(fixture.socketCount, 0);
      assert.equal(fixture.observations.http, 0);
    } finally { timers.close(); await transport.close(); await fixture.close(); }
  });
}

for (const stage of ['method', 'authentication', 'connect']) {
  test(`abort after truncated ${stage} must clear SDK pending negotiation timer`, { timeout: 4000 }, async () => {
    const fixture = await proxyFixture({ truncate: stage, auth: stage === 'authentication' });
    const transport = createTorTransport({ ...selection(fixture.port),
      ...(stage === 'authentication' ? { username: 'synthetic-user', password: 'synthetic-password' } : {}) }, { sdk });
    const controller = new AbortController();
    const timers = observeTimers();
    try {
      const pending = transport.fetch(syntheticURL, { signal: controller.signal });
      const rejected = assert.rejects(pending, failureCode('cancelled'));
      await fixture.reached('closed');
      await tick(); // Observe EOF locally before cancelling, not just server FIN.
      controller.abort();
      await rejected;
      await transport.close();
      await tick();
      timers.assertDisposed();
      assert.equal(fixture.observations.http, 0);
    } finally { timers.close(); await transport.close(); await fixture.close(); }
  });
}

test('TCP abort suppresses a late connector success', { timeout: 4000 }, async () => {
  const realConnect = net.connect;
  let socket;
  net.connect = () => { socket = new net.Socket(); return socket; };
  syncBuiltinESMExports();
  const transport = createTorTransport(selection(9050), { sdk });
  const controller = new AbortController();
  const timers = observeTimers();
  try {
    const pending = transport.fetch(syntheticURL, { signal: controller.signal });
    const rejected = assert.rejects(pending, failureCode('cancelled'));
    // SDK fetch dispatch is synchronous here; no TCP protocol bytes are fabricated.
    assert.ok(socket);
    controller.abort();
    await rejected;
    socket.emit('connect');
    await tick();
    assert.equal(socket.destroyed, true);
    assert.equal(socket.listenerCount('data'), 0, 'late callback must not start SDK SOCKS handshake');
    timers.assertDisposed();
  } finally { timers.close(); net.connect = realConnect; syncBuiltinESMExports(); await transport.close(); }
});

for (const action of ['signal', 'body_cancel', 'close']) test(`response body ${action} disposes transport without raw abort reason`, { timeout: 4000 }, async () => {
  const fixture = await proxyFixture({ body: 'stall' });
  const transport = createTorTransport(selection(fixture.port), { sdk });
  const controller = new AbortController();
  try {
    const response = await transport.fetch(syntheticURL, { signal: controller.signal });
    const reader = response.body.getReader();
    assert.equal(new TextDecoder().decode((await reader.read()).value), 'synthetic');
    if (action === 'body_cancel') await reader.cancel('synthetic-hostile-cancel');
    else {
      const pending = reader.read();
      const rejected = assert.rejects(pending, failureCode(action === 'close' ? 'closed' : 'cancelled'));
      if (action === 'close') await transport.close();
      else controller.abort(new Error('synthetic-hostile-reason'));
      await rejected;
    }
    await fixture.reached('closed');
    assert.equal(fixture.socketCount, 0);
  } finally { await transport.close(); await fixture.close(); }
});

test('response truncation has a closed delivery failure, not raw SDK diagnostics', async () => {
  const fixture = await proxyFixture({ body: 'cut' });
  const transport = createTorTransport(selection(fixture.port), { sdk });
  try { await assert.rejects((await transport.fetch(syntheticURL)).text(), failureCode('delivery_unknown')); }
  finally { await transport.close(); await fixture.close(); }
});

test('HTTPS default rejects an untrusted certificate with no HTTP sent', { timeout: 4000 }, async () => {
  const directory = mkdtempSync(join(tmpdir(), 'possums-synthetic-tls-'));
  const generated = spawnSync('openssl', ['req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-days', '1',
    '-subj', '/CN=fixture.invalid', '-addext', 'subjectAltName=DNS:fixture.invalid',
    '-keyout', join(directory, 'key.pem'), '-out', join(directory, 'cert.pem')], { stdio: 'ignore' });
  assert.equal(generated.status, 0, 'synthetic TLS fixture generation');
  const fixture = await proxyFixture({ tlsOptions: { key: readFileSync(join(directory, 'key.pem')), cert: readFileSync(join(directory, 'cert.pem')) } });
  const transport = createTorTransport(selection(fixture.port), { sdk });
  try {
    await assert.rejects(transport.fetch('https://fixture.invalid/public'), failureCode('tls'));
    assert.equal(fixture.observations.http, 0);
  } finally { await transport.close(); await fixture.close(); rmSync(directory, { recursive: true }); }
});

test('zero local DNS/direct destination connections on success or proxy failure', { timeout: 4000 }, async () => {
  const fixture = await proxyFixture();
  const transport = createTorTransport(selection(fixture.port), { sdk });
  const realConnect = net.connect;
  const realLookup = dns.lookup;
  const connects = [];
  let lookups = 0;
  net.connect = options => { connects.push([options.host, options.port]); return realConnect(options); };
  dns.lookup = () => { lookups++; throw new Error('synthetic forbidden local DNS'); };
  syncBuiltinESMExports();
  try {
    assert.equal(await (await transport.fetch(syntheticURL)).text(), 'synthetic');
    await fixture.close();
    await assert.rejects(transport.fetch(syntheticURL), failureCode('proxy'));
    assert.equal(lookups, 0);
    assert.deepEqual(connects, [['127.0.0.1', fixture.port], ['127.0.0.1', fixture.port]]);
  } finally { net.connect = realConnect; dns.lookup = realLookup; syncBuiltinESMExports(); await transport.close(); }
});

test('pre-abort and closed scope send nothing; close is idempotent', async () => {
  const fixture = await proxyFixture();
  const transport = createTorTransport(selection(fixture.port), { sdk });
  try {
    await assert.rejects(transport.fetch(syntheticURL, { signal: AbortSignal.abort('synthetic') }), failureCode('cancelled'));
    await transport.close(); await transport.close();
    await assert.rejects(transport.fetch(syntheticURL), failureCode('closed'));
    assert.equal(fixture.observations.connections, 0);
  } finally { await fixture.close(); }
});
