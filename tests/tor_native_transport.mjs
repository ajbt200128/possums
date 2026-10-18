import test from 'node:test';
import assert from 'node:assert/strict';
import net from 'node:net';
import tls from 'node:tls';
import http from 'node:http';
import https from 'node:https';
import dns from 'node:dns';
import childProcess from 'node:child_process';
import { once } from 'node:events';
import { syncBuiltinESMExports } from 'node:module';
import { X509Certificate, createHash } from 'node:crypto';
import { mkdtempSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { Duplex } from 'node:stream';
import { createHook } from 'node:async_hooks';
import { gzipSync, createGunzip } from 'node:zlib';

// PRIVACY.md synthetic-testing exception + forbidden raw diagnostics + typed
// interface failures. OFFLINE: local fake SOCKS and disposable TLS fixtures.
// Actual compiled official SDK in every handshake, never a helper/SDK stub.
// No Tor, public upstream, inference, bootstrap, account or real key access.
const manifest = resolve('clients/pi/tor-native/Cargo.toml');
const target = resolve(process.env.POSSUMS_TOR_NATIVE_TEST_TARGET ?? '/tmp/possums-tor-native-target');
if (!process.env.POSSUMS_TOR_NATIVE_TEST_HELPER) {
  const built = childProcess.spawnSync('cargo', ['+1.92.0', 'build', '--offline', '--locked', '--manifest-path', manifest, '--target-dir', target], { stdio: 'pipe' });
  assert.equal(built.status, 0, 'locked Rust1.92 offline build must pass; raw compiler output withheld');
}
const helperPath = process.env.POSSUMS_TOR_NATIVE_TEST_HELPER
  ? resolve(process.env.POSSUMS_TOR_NATIVE_TEST_HELPER) : join(target, 'debug/possums-tor-native');
const children = new Set();
const spawn = childProcess.spawn;
let observeSpawn;
childProcess.spawn = function(path, args, options) {
  const child = spawn.call(this, path, args, options);
  if (path === helperPath) {
    assert.deepEqual(args, [], 'no configuration or credentials in argv');
    assert.deepEqual(options.env, {}, 'no inherited private/debug environment');
    assert.equal(options.shell, false);
    assert.equal(options.stdio[2], 'ignore', 'stderr discarded, no panic payload capture');
    children.add(child);
    child.once('close', () => children.delete(child));
    observeSpawn?.(child);
  }
  return child;
};
let dnsCalls = 0;
const lookup = dns.lookup;
dns.lookup = function(...args) { dnsCalls++; return lookup.apply(this, args); };
syncBuiltinESMExports();
const { createTorNativeTransport, TorNativeTransportFailure, torNativeTransportDiagnostic } = await import('../clients/pi/tor-native-transport.mjs');
const remote = { host: 'destination.fixture.invalid', port: 443 };
const scratch = mkdtempSync(join(tmpdir(), 'possums-tor-native-offline-'));
function openssl(...args) {
  const p = childProcess.spawnSync('openssl', args, { cwd: scratch, stdio: 'pipe' });
  assert.equal(p.status, 0, 'synthetic certificate generation must pass (output withheld)');
}
function material(name, hostname = remote.host, { signer, expired = false } = {}) {
  openssl('req', '-new', '-newkey', 'rsa:2048', '-nodes', '-keyout', `${name}.key`, '-out', `${name}.csr`, '-subj', `/CN=${name}.fixture.invalid`);
  writeFileSync(join(scratch, `${name}.ext`), `subjectAltName=DNS:${hostname}\nbasicConstraints=critical,CA:${signer ? 'FALSE' : 'TRUE'}\n`);
  if (expired) {
    writeFileSync(join(scratch, 'index'), ''); writeFileSync(join(scratch, 'serial'), '1000\n');
    writeFileSync(join(scratch, 'ca.conf'), '[ca]\ndefault_ca=test\n[test]\ndatabase=index\nserial=serial\nnew_certs_dir=.\ncertificate=valid.pem\nprivate_key=valid.key\ndefault_md=sha256\npolicy=policy\n[policy]\ncommonName=supplied\n');
    openssl('ca', '-batch', '-config', 'ca.conf', '-in', `${name}.csr`, '-out', `${name}.pem`, '-notext',
      '-startdate', '20200101000000Z', '-enddate', '20200102000000Z', '-extfile', `${name}.ext`);
  } else openssl('x509', '-req', '-in', `${name}.csr`, '-out', `${name}.pem`, '-days', '2', '-extfile', `${name}.ext`,
    ...(signer ? ['-CA', `${signer}.pem`, '-CAkey', `${signer}.key`, '-CAcreateserial'] : ['-signkey', `${name}.key`]));
  const cert = readFileSync(join(scratch, `${name}.pem`));
  return { cert, key: readFileSync(join(scratch, `${name}.key`)),
    pin: createHash('sha256').update(new X509Certificate(cert).publicKey.export({ type: 'spki', format: 'der' })).digest('hex') };
}
const valid = material('valid');
const wrongHost = material('wrong-host', 'other.fixture.invalid');
const expired = material('expired', undefined, { expired: true });
const other = material('other', undefined, { signer: 'valid' });
const untrusted = material('untrusted');

async function until(predicate) {
  const end = performance.now() + 2500;
  while (!predicate() && performance.now() < end) await new Promise(resolve => setTimeout(resolve, 5));
  assert.ok(predicate(), 'local owned resources must close within fixture deadline');
}
function failure(kind) {
  return error => {
    assert.ok(error instanceof TorNativeTransportFailure);
    if (kind) assert.equal(error.diagnostic.code, `possums_tor_native_${kind}`);
    assert.equal(error.cause, undefined); assert.equal(error.stack, undefined);
    assert.ok(!JSON.stringify(error).includes('hostile-canary'));
    assert.match(error.diagnostic.outcome, /unknown.*No request was replayed/);
    return true;
  };
}

// Only the synthetic SERVER decodes fixture control bytes. Production uses SDK.
async function fixture({ auth = false, rejectAuth = false, malformed, stall, truncate, fragment = false,
  status = 0, replyType = 1, rsv = 0, mode = 'echo', served = valid, handler } = {}) {
  const sockets = new Set(), timers = new Set();
  const stages = Object.fromEntries(['tcp', 'method', 'auth', 'connect', 'tls', 'http'].map(s => [s, Promise.withResolvers()]));
  const seen = { connections: 0, domains: [], ports: [], methods: [], auth: [], http: 0, application: 0, sni: [] };
  function own(socket) { sockets.add(socket); socket.on('error', () => {}); socket.once('close', () => sockets.delete(socket)); }
  const app = http.createServer((req, res) => {
    seen.http++; stages.http.resolve();
    if (handler) handler(req, res);
    else res.end('synthetic-body');
  });
  function send(socket, bytes) {
    if (!fragment) return socket.write(bytes);
    let i = 0;
    function next() {
      if (socket.destroyed || i === bytes.length) return;
      socket.write(bytes.subarray(i, ++i));
      if (i < bytes.length) {
        const timer = setTimeout(() => { timers.delete(timer); next(); }, 1); timers.add(timer);
      }
    }
    next();
  }
  const server = net.createServer(socket => {
    own(socket); seen.connections++; stages.tcp.resolve();
    let state = 'method', buffer = Buffer.alloc(0);
    function onData(chunk) {
      buffer = Buffer.concat([buffer, chunk]); assert.ok(buffer.length <= 1024);
      if (state === 'method') {
        if (buffer.length < 2 || buffer.length < 2 + buffer[1]) return;
        seen.methods.push([...buffer.subarray(2, 2 + buffer[1])]); buffer = Buffer.alloc(0); stages.method.resolve();
        if (stall === 'method' || (stall === 'third_method' && seen.connections === 3)) return;
        if (truncate === 'method') { socket.end(Buffer.from([5])); return; }
        state = auth || malformed === 'unoffered' ? 'auth' : 'connect';
        send(socket, Buffer.from([malformed === 'method' ? 4 : 5,
          malformed === 'unoffered' ? 2 : malformed === 'unsupported' ? 0x80 : auth ? 2 : 0]));
      } else if (state === 'auth') {
        if (buffer.length < 3 || buffer.length < 3 + buffer[1]) return;
        const length = 3 + buffer[1] + buffer[2 + buffer[1]];
        if (buffer.length < length) return;
        seen.auth.push([buffer.subarray(2, 2 + buffer[1]).toString(), buffer.subarray(3 + buffer[1], length).toString()]);
        buffer = Buffer.alloc(0); stages.auth.resolve();
        if (stall === 'auth') return;
        if (truncate === 'auth') { socket.end(Buffer.from([1])); return; }
        state = 'connect'; send(socket, Buffer.from([malformed === 'auth' ? 2 : 1, rejectAuth ? 1 : 0]));
      } else if (state === 'connect') {
        if (buffer.length < 5 || buffer.length < 7 + buffer[4]) return;
        assert.equal(buffer[3], 3, 'official request uses hostname, no local destination DNS');
        seen.domains.push(buffer.subarray(5, 5 + buffer[4]).toString());
        seen.ports.push(buffer.readUInt16BE(5 + buffer[4])); stages.connect.resolve();
        if (stall === 'connect') return;
        const addr = replyType === 4 ? [...new Uint8Array(16), 0, 1]
          : replyType === 3 ? [3, 97, 98, 99, 0, 1] : [127, 0, 0, 1, 0, 1];
        const reply = Buffer.from([malformed === 'connect' ? 4 : 5, status, rsv, malformed === 'atyp' ? 9 : replyType, ...addr]);
        if (truncate === 'connect') { socket.end(reply.subarray(0, 7)); return; }
        socket.removeListener('data', onData); buffer = Buffer.alloc(0); state = 'application';
        send(socket, reply);
        if (status || malformed) return;
        if (stall === 'tls') { socket.on('data', () => stages.tls.resolve()); return; }
        if (mode === 'tls') {
          const secure = new tls.TLSSocket(socket, { isServer: true, secureContext: tls.createSecureContext(served) });
          own(secure);
          secure.once('secure', () => { seen.sni.push(secure.servername); stages.tls.resolve(); app.emit('connection', secure); });
          secure.on('data', bytes => { seen.application += bytes.length; });
        } else if (mode === 'disconnect') socket.end('synthetic-final');
        else if (mode === 'sink') { socket.on('data', chunk => { seen.application += chunk.length; }); }
        else if (mode === 'blocked') socket.pause();
        else if (mode === 'flood') {
          function flood() { while (!socket.destroyed && socket.write(Buffer.alloc(16 * 1024, 65))) {} }
          socket.on('drain', flood); flood();
        } else socket.on('data', bytes => { seen.application += bytes.length; socket.write(bytes); });
      }
    }
    socket.on('data', onData);
  });
  server.listen(0, '127.0.0.1'); await once(server, 'listening');
  return { port: server.address().port, seen, sockets, reached: stage => stages[stage].promise,
    async close() { for (const t of timers) clearTimeout(t); for (const s of sockets) s.destroy();
      app.closeAllConnections(); await new Promise(resolve => server.close(resolve)); } };
}
function transport(proxy, extras = {}) {
  return createTorNativeTransport({ helperPath, host: '127.0.0.1', port: proxy.port, timeoutMs: 1500, ...extras });
}
async function withFixture(options, callback) {
  const proxy = await fixture(options);
  try { await callback(proxy); } finally { await proxy.close(); }
  await until(() => children.size === 0);
}
async function echo(socket, payload = 'synthetic-bytes') {
  const data = once(socket, 'data'); socket.write(payload); assert.equal((await data)[0].toString(), payload);
}

for (const selection of [
  { host: 'localhost' }, { host: '192.0.2.1' }, { host: '::ffff:127.0.0.1' }, { port: 0 },
  { username: 'hostile-canary' }, { timeoutMs: 300001 }, { timeoutMs: 0 }, { helperPath: 'relative' },
  { tls: { rejectUnauthorized: false } }, { password: 'hostile-canary' }, { username: 'x'.repeat(256), password: 'y' },
]) test('invalid configuration makes no IO', () => {
  const before = children.size;
  assert.throws(() => createTorNativeTransport({ helperPath, host: '127.0.0.1', port: 9050, ...selection }), failure('configuration'));
  assert.equal(children.size, before);
});
for (const host of ['https://host.invalid', 'host.invalid/path', 'host.invalid@evil.invalid', '', 'x'.repeat(256), 'a\0b', 'é.invalid'])
  test('invalid destination makes no IO', async () => {
    const scope = createTorNativeTransport({ helperPath, host: '127.0.0.1', port: 9050 });
    await assert.rejects(scope.connectSocket({ host, port: 443 }), failure('destination')); await scope.close();
  });
test('closed diagnostics discard hostile cause, kind and caller reason', () => {
  assert.equal(torNativeTransportDiagnostic(new Error('hostile-canary')).constraint, 'cause_unknown');
  assert.ok(!JSON.stringify(new TorNativeTransportFailure('hostile-canary')).includes('hostile-canary'));
});
for (const options of [{}, { fragment: true }, { auth: true }, { auth: true, fragment: true }, { replyType: 3 }, { replyType: 4 }])
  test(`compiled official SDK transparent roundtrip ${JSON.stringify(options)}`, async () => {
    await withFixture(options, async proxy => {
      const scope = transport(proxy, options.auth ? { username: 'synthetic-user', password: 'synthetic-pass' } : {});
      try {
        const beforeDNS = dnsCalls;
        const socket = await scope.connectSocket(remote);
        assert.ok(socket instanceof Duplex); await echo(socket);
        assert.equal(dnsCalls, beforeDNS);
        assert.deepEqual(proxy.seen.domains, [remote.host]); assert.deepEqual(proxy.seen.ports, [443]);
        assert.deepEqual(proxy.seen.methods, [options.auth ? [2, 0] : [0]]);
        if (options.auth) assert.deepEqual(proxy.seen.auth, [['synthetic-user', 'synthetic-pass']]);
        assert.equal(proxy.seen.connections, 1);
      } finally { await scope.close(); }
      await until(() => proxy.sockets.size === 0);
    });
  });
test('optional credentials preserve SDK no-auth selection without isolation promise', async () => {
  await withFixture({}, async proxy => {
    const scope = transport(proxy, { username: 'synthetic-user', password: 'synthetic-pass' });
    const socket = await scope.connectSocket(remote); await echo(socket); assert.deepEqual(proxy.seen.auth, []); await scope.close();
  });
});
test('LIMITATION official SDK accepts nonzero RSV; raw success does NOT authorize channel', async () => {
  await withFixture({ rsv: 173 }, async proxy => {
    const scope = transport(proxy); const socket = await scope.connectSocket(remote);
    assert.equal(proxy.seen.application, 0, 'no credentials or HTTP automatically sent');
    assert.equal(scope.fetch, undefined); assert.equal(socket.authorized, undefined);
    await scope.close();
  });
});
for (const [options, kind] of [
  [{ malformed: 'method' }, 'protocol'], [{ malformed: 'unoffered' }, 'protocol'],
  [{ malformed: 'unsupported' }, 'unsupported'], [{ auth: true, malformed: 'auth' }, 'protocol'],
  [{ auth: true, rejectAuth: true }, 'authentication'], [{ malformed: 'connect' }, 'protocol'],
  [{ malformed: 'atyp' }, 'protocol'],
  ...['method', 'auth', 'connect'].map(stage => [{ truncate: stage, auth: stage === 'auth' }, 'truncated']),
  ...Array.from({ length: 8 }, (_, i) => [{ status: i + 1 }, 'tunnel']),
]) test(`SDK rejects ${JSON.stringify(options)} without raw values`, async () => {
  await withFixture(options, async proxy => {
    const scope = transport(proxy, options.auth ? { username: 'synthetic-user', password: 'synthetic-pass' } : {});
    await assert.rejects(scope.connectSocket(remote), failure(kind));
    assert.equal(proxy.seen.application, 0); await scope.close(); await until(() => proxy.sockets.size === 0);
  });
});
for (const stage of ['method', 'auth', 'connect']) {
  test(`readiness deadline at ${stage} kills owned helper; no late success`, async () => {
    await withFixture({ stall: stage, auth: stage === 'auth' }, async proxy => {
      const scope = transport(proxy, { timeoutMs: 120, ...(stage === 'auth' ? { username: 'u', password: 'p' } : {}) });
      await assert.rejects(scope.connectSocket(remote), failure('deadline'));
      await scope.close(); await until(() => proxy.sockets.size === 0);
    });
  });
  test(`abort at ${stage} preserves primary and closes owned helper`, async () => {
    await withFixture({ stall: stage, auth: stage === 'auth' }, async proxy => {
      const scope = transport(proxy, stage === 'auth' ? { username: 'u', password: 'p' } : {});
      const controller = new AbortController();
      const result = scope.connectSocket(remote, controller.signal);
      const rejected = assert.rejects(result, failure('cancelled'));
      await proxy.reached(stage); controller.abort(new Error('hostile-canary')); await scope.close(); await rejected;
      await until(() => proxy.sockets.size === 0);
    });
  });
}
test('already aborted does not spawn', async () => {
  const scope = createTorNativeTransport({ helperPath, host: '127.0.0.1', port: 9050 });
  const controller = new AbortController(); controller.abort('hostile-canary');
  await assert.rejects(scope.connectSocket(remote, controller.signal), failure('cancelled')); assert.equal(children.size, 0); await scope.close();
});
test('immediate abort during proxy connect retains child until exit', async () => {
  await withFixture({}, async proxy => {
    const scope = transport(proxy); const controller = new AbortController();
    const result = scope.connectSocket(remote, controller.signal); controller.abort('hostile-canary');
    await assert.rejects(result, failure('cancelled')); assert.equal(children.size, 0); await scope.close();
  });
});
test('scope close while pending is idempotent and disallows new IO', async () => {
  await withFixture({ stall: 'connect' }, async proxy => {
    const scope = transport(proxy); const result = scope.connectSocket(remote);
    const rejected = assert.rejects(result, failure('closed'));
    await proxy.reached('connect'); const closing = scope.close(); assert.equal(scope.close(), closing); await closing; await rejected;
    await assert.rejects(scope.connectSocket(remote), failure('closed')); assert.equal(children.size, 0);
  });
});
test('one scope owns multiple pending AND established helpers', async () => {
  await withFixture({ stall: 'third_method' }, async proxy => {
    const scope = transport(proxy); await Promise.all([scope.connectSocket(remote), scope.connectSocket(remote)]);
    const result = scope.connectSocket(remote); const rejection = assert.rejects(result, failure('closed'));
    await until(() => proxy.seen.methods.length === 3);
    assert.equal(children.size, 3); await scope.close(); await rejection;
    assert.equal(children.size, 0); await until(() => proxy.sockets.size === 0);
  });
});
test('delayed private READY EOF cannot escape connection deadline', async () => {
  await withFixture({}, async proxy => {
    let child;
    observeSpawn = value => { child = value; child.stdio[4].pause(); };
    const scope = transport(proxy, { timeoutMs: 150 });
    try {
      await assert.rejects(scope.connectSocket(remote), failure('deadline'));
      assert.ok(child); assert.notEqual(child.signalCode, null);
      child.stdio[4].resume(); assert.equal(children.size, 0); await until(() => proxy.sockets.size === 0);
    } finally { observeSpawn = undefined; await scope.close(); }
  });
});
test('abort while actual native READY is withheld cannot resolve late', async () => {
  await withFixture({}, async proxy => {
    let child; observeSpawn = value => { child = value; child.stdio[4].pause(); };
    const scope = transport(proxy); const controller = new AbortController();
    try {
      const result = scope.connectSocket(remote, controller.signal); const rejected = assert.rejects(result, failure('cancelled'));
      await proxy.reached('connect'); controller.abort('hostile-canary'); await rejected;
      child.stdio[4].resume(); assert.equal(children.size, 0); await until(() => proxy.sockets.size === 0);
    } finally { observeSpawn = undefined; await scope.close(); }
  });
});
test('native read/write failure is not masked as successful EOF or caller cancellation', async () => {
  await withFixture({}, async proxy => {
    const scope = transport(proxy); const socket = await scope.connectSocket(remote); await echo(socket);
    const error = once(socket, 'error');
    for (const peer of proxy.sockets) peer.resetAndDestroy();
    failure('bridge_unknown')((await error)[0]); await scope.close(); assert.equal(children.size, 0);
  });
});
test('native stdout pipe failure closes TCP and retains stage-specific fallback', async () => {
  await withFixture({ mode: 'flood' }, async proxy => {
    let child; observeSpawn = value => { child = value; };
    const scope = transport(proxy);
    try {
      const socket = await scope.connectSocket(remote); const error = once(socket, 'error');
      child.stdout.destroy(); failure('bridge_unknown')((await error)[0]);
      await scope.close(); await until(() => proxy.sockets.size === 0);
    } finally { observeSpawn = undefined; await scope.close(); }
  });
});
test('connection timer is cleared at READY, not a generation-age abort', async () => {
  await withFixture({}, async proxy => {
    const scope = transport(proxy, { timeoutMs: 100 }); const socket = await scope.connectSocket(remote);
    await new Promise(resolve => setTimeout(resolve, 200)); await echo(socket); await scope.close();
  });
});
test('socket destroy aborts established child', async () => {
  await withFixture({}, async proxy => {
    const scope = transport(proxy); const socket = await scope.connectSocket(remote); socket.destroy();
    await until(() => children.size === 0 && proxy.sockets.size === 0); await scope.close();
  });
});
test('signal abort still owns established socket', async () => {
  await withFixture({}, async proxy => {
    const scope = transport(proxy); const controller = new AbortController(); const socket = await scope.connectSocket(remote, controller.signal);
    const rejected = once(socket, 'error'); controller.abort('hostile-canary'); failure('cancelled')((await rejected)[0]);
    await scope.close(); await until(() => proxy.sockets.size === 0);
  });
});
test('native stdin EOF closes BOTH halves on established connection', async () => {
  await withFixture({}, async proxy => {
    const scope = transport(proxy); const socket = await scope.connectSocket(remote); socket.end();
    await until(() => children.size === 0 && proxy.sockets.size === 0); await scope.close();
  });
});
test('server disconnect drains trailing application bytes and releases child', async () => {
  await withFixture({ mode: 'disconnect' }, async proxy => {
    const scope = transport(proxy); const socket = await scope.connectSocket(remote);
    const chunks = []; for await (const chunk of socket) chunks.push(chunk);
    assert.equal(Buffer.concat(chunks).toString(), 'synthetic-final'); await scope.close(); assert.equal(children.size, 0);
  });
});
for (const mode of ['flood', 'blocked']) test(`backpressure ${mode} is not disconnect; abort releases both copies`, async () => {
  await withFixture({ mode }, async proxy => {
    const scope = transport(proxy, { timeoutMs: 100 }); const controller = new AbortController();
    const socket = await scope.connectSocket(remote, controller.signal);
    if (mode === 'blocked') { socket.write(Buffer.alloc(4 * 1024 * 1024)); }
    else socket.pause();
    await new Promise(resolve => setTimeout(resolve, 220)); assert.equal(socket.destroyed, false); assert.equal(children.size, 1);
    controller.abort(); await scope.close(); assert.equal(children.size, 0);
    // The intentionally paused fixture cannot observe TCP EOF until it reads.
    // Resume only AFTER native exit is confirmed; drain pending synthetic data.
    if (mode === 'blocked') for (const peer of proxy.sockets) { peer.on('data', () => {}); peer.resume(); }
    await until(() => proxy.sockets.size === 0);
  });
});
test('large streaming upload uses bounded byte buffers with backpressure', async () => {
  await withFixture({ mode: 'sink' }, async proxy => {
    const scope = transport(proxy); const socket = await scope.connectSocket(remote);
    for (let i = 0; i < 64; i++) if (!socket.write(Buffer.alloc(16 * 1024))) await once(socket, 'drain');
    await until(() => proxy.seen.application === 1024 * 1024); await scope.close();
  });
});
test('native stdin EOF cancels pending SDK handshake', async () => {
  await withFixture({ stall: 'method' }, async proxy => {
    let child; observeSpawn = value => { child = value; };
    const scope = transport(proxy); const result = scope.connectSocket(remote);
    const rejected = assert.rejects(result, failure('cancelled'));
    try { await proxy.reached('method'); child.stdin.end(); await rejected; await scope.close(); }
    finally { observeSpawn = undefined; await scope.close(); }
    await until(() => proxy.sockets.size === 0);
  });
});
test('helper unavailable is safe and does not fall back', async () => {
  const scope = createTorNativeTransport({ helperPath: '/nonexistent/possums-tor-native', host: '127.0.0.1', port: 9050 });
  await assert.rejects(scope.connectSocket(remote), failure('helper')); await scope.close();
});
test('loopback listener refusal keeps proxy stage', async () => {
  const proxy = await fixture(); const scope = transport(proxy); await proxy.close();
  await assert.rejects(scope.connectSocket(remote), failure('proxy')); await scope.close();
});

// Mirror only the EXISTING v3 certificate policy, unchanged in parent v3.ts:
// exact SDK-endorsed SPKI + SAN hostname + validity; self-signed is permitted.
// This fixture does not claim integration with pinnedRequest or authorize work.
async function secure(scope, { pin = valid.pin, signal, publicTrust = false } = {}) {
  const raw = await scope.connectSocket(remote, signal);
  const socket = tls.connect({ socket: raw, servername: remote.host, rejectUnauthorized: publicTrust, ALPNProtocols: ['http/1.1'] });
  socket.on('error', () => {});
  await new Promise((resolve, reject) => {
    socket.once('error', () => reject(new Error('synthetic-tls-verification')));
    socket.once('secureConnect', () => {
      try {
        const peer = new X509Certificate(socket.getPeerCertificate().raw);
        assert.equal(socket.isSessionReused(), false);
        assert.equal(createHash('sha256').update(peer.publicKey.export({ type: 'spki', format: 'der' })).digest('hex'), pin);
        assert.notEqual(peer.checkHost(remote.host, { subject: 'never' }), undefined);
        assert.ok(Date.now() >= Date.parse(peer.validFrom) && Date.now() < Date.parse(peer.validTo));
        resolve();
      } catch { socket.destroy(); reject(new Error('synthetic-tls-verification')); }
    });
  });
  return socket;
}
async function nativeHTTP(socket, signal) {
  const agent = new https.Agent({ keepAlive: false, maxCachedSessions: 0 });
  agent.createConnection = () => socket;
  try {
    return await new Promise((resolve, reject) => {
      const req = https.request(`https://${remote.host}/synthetic`, { method: 'POST', agent, signal,
        headers: { Authorization: 'Bearer synthetic-credential' } }, resolve);
      req.once('error', () => reject(new Error('synthetic-http-interrupted'))); req.end('synthetic-upload');
    });
  } catch (error) { agent.destroy(); throw error; }
}
test('native TLS possession + exact v3 policy validates BEFORE maintained HTTP', async () => {
  await withFixture({ mode: 'tls', fragment: true }, async proxy => {
    const scope = transport(proxy);
    try {
      const socket = await secure(scope); assert.equal(proxy.seen.application, 0); assert.equal(proxy.seen.http, 0);
      const incoming = await nativeHTTP(socket); const chunks = []; for await (const chunk of incoming) chunks.push(chunk);
      assert.equal(Buffer.concat(chunks).toString(), 'synthetic-body'); assert.equal(proxy.seen.http, 1); assert.deepEqual(proxy.seen.sni, [remote.host]);
    } finally { await scope.close(); }
  });
});
for (const [name, served, options] of [
  ['wrong SAN hostname', wrongHost, { pin: wrongHost.pin }], ['expired', expired, { pin: expired.pin }],
  ['wrong exact SPKI', other, {}], ['untrusted public certificate', untrusted, { pin: untrusted.pin, publicTrust: true }],
]) test(`${name} rejects before any credentials/header/body/HTTP`, async () => {
  await withFixture({ mode: 'tls', served }, async proxy => {
    const scope = transport(proxy); await assert.rejects(secure(scope, options));
    assert.equal(proxy.seen.application, 0); assert.equal(proxy.seen.http, 0); await scope.close();
  });
});
test('maintained native HTTPS compression works over generic byte Duplex', async () => {
  await withFixture({ mode: 'tls', handler(_req, res) { res.writeHead(200, { 'content-encoding': 'gzip' }); res.end(gzipSync('synthetic-compressed')); } }, async proxy => {
    const scope = transport(proxy); const incoming = await nativeHTTP(await secure(scope));
    const chunks = []; for await (const chunk of incoming.pipe(createGunzip())) chunks.push(chunk);
    assert.equal(Buffer.concat(chunks).toString(), 'synthetic-compressed'); await scope.close();
  });
});
for (const stage of ['tls', 'http', 'body']) test(`native TLS/HTTP abort at ${stage} closes helper without replay`, async () => {
  await withFixture(stage === 'tls' ? { stall: 'tls' } : { mode: 'tls', handler(_req, res) {
    if (stage === 'body') { res.writeHead(200); res.write('synthetic-first'); }
  } }, async proxy => {
    const scope = transport(proxy); const controller = new AbortController();
    const handshake = secure(scope, { signal: controller.signal });
    if (stage === 'tls') {
      const rejected = assert.rejects(handshake); await proxy.reached('tls'); controller.abort('hostile-canary'); await rejected;
    } else {
      const socket = await handshake; const result = nativeHTTP(socket, controller.signal);
      if (stage === 'http') {
        const rejected = assert.rejects(result); await proxy.reached('http'); controller.abort('hostile-canary'); await rejected;
      } else {
        const incoming = await result; const failed = once(incoming, 'error'); incoming.resume(); controller.abort('hostile-canary'); await failed;
      }
    }
    await scope.close(); await until(() => proxy.sockets.size === 0); assert.equal(proxy.seen.connections, 1);
  });
});
test('connection readiness leaves no live owned deadline timer', async () => {
  const timeouts = new Map();
  const hook = createHook({ init(id, type, _trigger, resource) { if (type === 'Timeout') timeouts.set(id, resource); } }); hook.enable();
  try {
    await withFixture({}, async proxy => {
      const scope = transport(proxy, { timeoutMs: 299999 }); const socket = await scope.connectSocket(remote);
      assert.ok([...timeouts.values()].filter(t => t._idleTimeout === 299999).every(t => t._destroyed));
      await echo(socket); await scope.close();
    });
  } finally { hook.disable(); }
});

// Direct compiled helper IPC boundaries, including config EOF/size/closed enum.
async function direct(configBytes, { inputEOF = false, configEOF = true, inputBytes, inputReady } = {}) {
  const child = spawn(helperPath, [], { env: {}, cwd: '/', shell: false, stdio: ['pipe', 'pipe', 'pipe', 'pipe', 'pipe'] });
  const status = [], stdout = [], stderr = [];
  for (const stream of [child.stdin, child.stdio[3]]) stream.on('error', () => {});
  child.stdio[4].on('data', chunk => status.push(chunk)); child.stdout.on('data', chunk => stdout.push(chunk)); child.stderr.on('data', chunk => stderr.push(chunk));
  const exit = once(child, 'close');
  const timer = setTimeout(() => child.kill('SIGKILL'), 2500);
  child.stdio[3].write(configBytes); if (configEOF) child.stdio[3].end(); if (inputEOF) child.stdin.end();
  let code, signal;
  try {
    if (inputBytes) { await inputReady?.(); child.stdin.write(inputBytes); }
    [code, signal] = await exit;
  } finally { clearTimeout(timer); }
  return { code, signal, status: Buffer.concat(status).toString(), stdout: Buffer.concat(stdout).toString(), stderr: Buffer.concat(stderr).toString() };
}
for (const bytes of [Buffer.from('{hostile-canary'), Buffer.alloc(4096, 65), Buffer.from(JSON.stringify({ hostile: 'hostile-canary' }))])
  test('native private config parsing is bounded and never echoed', async () => {
    const result = await direct(bytes); assert.equal(result.code, 1); assert.equal(result.signal, null);
    assert.deepEqual(JSON.parse(result.status), { failure: 'configuration' }); assert.equal(result.stdout, ''); assert.equal(result.stderr, '');
  });
test('native stdin EOF cancels stalled private config without panic or raw diagnostics', async () => {
  const result = await direct(Buffer.from('{hostile-canary'), { inputEOF: true, configEOF: false });
  assert.equal(result.code, 1); assert.deepEqual(JSON.parse(result.status), { failure: 'cancelled' }); assert.equal(result.stderr, ''); assert.equal(result.stdout, '');
});
test('native data before READY is an IPC ordering failure, not cancellation', async () => {
  const result = await direct(Buffer.from('{synthetic'), { configEOF: false, inputBytes: Buffer.from([1]) });
  assert.equal(result.code, 1); assert.equal(result.signal, null);
  assert.deepEqual(JSON.parse(result.status), { failure: 'early_data' });
  assert.equal(result.stderr, ''); assert.equal(result.stdout, '');
});
test('native pre-READY byte during stalled SOCKS closes the owned socket with ordering category', async () => {
  await withFixture({ stall: 'connect' }, async proxy => {
    const result = await direct(Buffer.from(JSON.stringify({ proxy_host: '127.0.0.1', proxy_port: proxy.port,
      ...remote, timeout_ms: 2000 })), { inputBytes: Buffer.from([1]), inputReady: () => proxy.reached('connect') });
    assert.equal(result.code, 1); assert.equal(result.signal, null);
    assert.deepEqual(JSON.parse(result.status), { failure: 'early_data' });
    assert.equal(result.stderr, ''); assert.equal(result.stdout, '');
    await until(() => proxy.sockets.size === 0);
  });
});
test('final ownership has no helpers; synthetic secrets are removed from scratch', async () => {
  await until(() => children.size === 0); rmSync(scratch, { recursive: true, force: true });
  childProcess.spawn = spawn; dns.lookup = lookup; syncBuiltinESMExports();
});
