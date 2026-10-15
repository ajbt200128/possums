import test from 'node:test';
import assert from 'node:assert/strict';
import net from 'node:net';
import tls from 'node:tls';
import http from 'node:http';
import dns from 'node:dns';
import { once } from 'node:events';
import { createRequire, syncBuiltinESMExports } from 'node:module';
import { X509Certificate, createHash } from 'node:crypto';
import { mkdtempSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { createHook } from 'node:async_hooks';
import { gzipSync } from 'node:zlib';
import { createTorCurlTransport, TorCurlTransportFailure, torCurlTransportDiagnostic } from '../clients/pi/tor-curl-transport.mjs';

// PRIVACY.md synthetic exception / forbidden raw diagnostics / typed failures.
// Offline ONLY. Fixture decodes SOCKS for a local synthetic server, no resolver,
// Tor, inference, native patching, direct fallback, deployment or telemetry.
// Native qualification: node-libcurl 5.1.2, darwin-arm64, Node 24.20.0 ABI137
// / N-API10. npm prebuild: static curl8.17/OpenSSL3.5.6 (system dylibs only).
// Supported source build: pinned Nix curl8.22/OpenSSL3.6.4, dynamic libcurl.4
// at /nix/store/6c7nrda2k1s1qq8c97rckhm3iy61hkz9-curl-8.22.0/lib/libcurl.4.dylib.
// Both also imported on the existing host Node24.13.0/ABI137; no Pi install.
// No supported JS SSL_CTX callback is exposed; CAINFO_BLOB + native verification
// + PINNEDPUBLICKEY is used, never socket adoption or disabled validation.
// EXPECTED RESEARCH FAILURES remain real failing tests: bad auth-version and
// nonzero reserved byte accepted by both native versions. No integration gate.
const require = createRequire(import.meta.url);
const entry = resolve(process.env.POSSUMS_TOR_TEST_CURL ?? '/tmp/possums-curl-qualification/node_modules/node-libcurl');
const sdk = require(entry);
assert.equal(require(join(entry, 'package.json')).version, '5.1.2');
assert.equal(createHash('sha256').update(readFileSync(join(entry, 'dist/Curl.js'))).digest('hex'),
  '1338445f8ba3a6a07a392c955ae78e04df05c0a75282f659ba43de0ea13c9014');
assert.match(sdk.Curl.getVersion(), /^libcurl\/(8\.17\.0 OpenSSL\/3\.5\.6|8\.22\.0 OpenSSL\/3\.6\.4)/);
const origin = 'https://gateway.fixture.invalid';
const onion = `${'b'.repeat(56)}.onion`;
const scratch = mkdtempSync(join(tmpdir(), 'possums-curl-offline-'));
function openssl(...args) {
  const result = spawnSync(process.env.POSSUMS_TOR_TEST_OPENSSL ?? 'openssl', args, { cwd: scratch, stdio: 'pipe' });
  assert.equal(result.status, 0, 'synthetic certificate generation must succeed (raw output withheld)');
}
function cert(name, hostname = 'gateway.fixture.invalid', { signer, expired = false, future = false } = {}) {
  openssl('req', '-new', '-newkey', 'rsa:2048', '-nodes', '-keyout', `${name}.key`, '-out', `${name}.csr`, '-subj', `/CN=${name}.fixture.invalid`);
  writeFileSync(join(scratch, `${name}.ext`), `subjectAltName=DNS:${hostname}\nbasicConstraints=critical,CA:${signer ? 'FALSE' : 'TRUE'}\n`);
  if (expired || future) {
    writeFileSync(join(scratch, 'index'), '');
    writeFileSync(join(scratch, 'serial'), '1000\n');
    writeFileSync(join(scratch, 'ca.conf'), '[ca]\ndefault_ca=test\n[test]\ndatabase=index\nserial=serial\nnew_certs_dir=.\ncertificate=valid.pem\nprivate_key=valid.key\ndefault_md=sha256\npolicy=policy\n[policy]\ncommonName=supplied\n');
    openssl('ca', '-batch', '-config', 'ca.conf', '-in', `${name}.csr`, '-out', `${name}.pem`, '-notext',
      '-startdate', future ? '20990101000000Z' : '20200101000000Z', '-enddate', future ? '20990102000000Z' : '20200102000000Z', '-extfile', `${name}.ext`);
  } else {
    openssl('x509', '-req', '-in', `${name}.csr`, '-out', `${name}.pem`, '-days', '2', '-extfile', `${name}.ext`,
      ...(signer ? ['-CA', `${signer}.pem`, '-CAkey', `${signer}.key`, '-CAcreateserial'] : ['-signkey', `${name}.key`]));
  }
  const pem = readFileSync(join(scratch, `${name}.pem`), 'utf8');
  return { key: readFileSync(join(scratch, `${name}.key`)), cert: pem,
    spki: createHash('sha256').update(new X509Certificate(pem).publicKey.export({ type: 'spki', format: 'der' })).digest('base64') };
}
const valid = cert('valid');
const wrong = cert('wrong', 'other.fixture.invalid');
const expired = cert('expired', undefined, { expired: true });
const future = cert('future', undefined, { future: true });
const other = cert('other', undefined, { signer: 'valid' });
cert('issuer', 'issuer.fixture.invalid');
const issuedLeaf = cert('issued', undefined, { signer: 'issuer' });
const publicLeaf = cert('public', 'public.fixture.invalid,DNS:second.fixture.invalid');
const publicSecond = cert('second', 'second.fixture.invalid', { signer: 'public' });
async function until(predicate) {
  const end = performance.now() + 1800;
  while (!predicate() && performance.now() < end) await new Promise(resolve => setTimeout(resolve, 10));
  assert.ok(predicate(), 'owned local fixture resources must close within bounded test deadline');
}
function failure(kind) {
  return error => {
    assert.ok(error instanceof TorCurlTransportFailure);
    if (kind) assert.equal(error.diagnostic.code, `possums_tor_curl_${kind}`);
    assert.equal(error.cause, undefined);
    assert.ok(!error.stack.includes('/') && !error.stack.includes('hostile-canary'));
    const projection = JSON.stringify(error.diagnostic);
    assert.ok(!projection.includes('hostile-canary'));
    assert.match(error.diagnostic.outcome, /unknown.*No request was replayed/);
    return true;
  };
}
async function fixture({ stall, truncate, malformed, auth = false, rejectAuth = false, fragment = false,
  credentials, material = valid, handler, rawReply, blind = false } = {}) {
  const sockets = new Set(), timers = new Set();
  const stages = Object.fromEntries(['tcp', 'method', 'auth', 'connect', 'tls', 'http'].map(key => [key, Promise.withResolvers()]));
  const seen = { connections: 0, http: 0, domains: [], ports: [], methods: [], hosts: [], sni: [], uploads: [], auth: 0, bytes: 0 };
  const app = http.createServer((request, response) => {
    seen.http++; seen.hosts.push(request.headers.host); stages.http.resolve();
    const chunks = [];
    request.on('data', chunk => chunks.push(chunk));
    request.on('end', () => seen.uploads.push(Buffer.concat(chunks).toString()));
    if (handler) handler(request, response, seen);
    else if (stall !== 'http' && stall !== 'body') response.end('synthetic-body');
    else if (stall === 'body') { response.writeHead(200); response.write('synthetic-first'); }
  });
  function own(socket) {
    sockets.add(socket); socket.on('error', () => {});
    socket.once('close', () => sockets.delete(socket));
  }
  let shim, bridge;
  if (blind) {
    shim = tls.createServer(material, secure => {
      own(secure); seen.sni.push(secure.servername); stages.tls.resolve();
      secure.on('data', chunk => { seen.bytes += chunk.length; });
      app.emit('connection', secure);
    });
    shim.on('connection', own); shim.on('tlsClientError', () => {});
    shim.listen(0, '127.0.0.1'); await once(shim, 'listening');
    bridge = net.createServer(socket => {
      own(socket);
      const upstream = net.connect({ host: '127.0.0.1', port: shim.address().port }); own(upstream);
      socket.on('data', chunk => {
        seen.blindFirstByte ??= chunk[0];
        seen.blindBytes = (seen.blindBytes ?? 0) + chunk.length;
        seen.blindPlaintextLeak ||= chunk.includes('synthetic-bridge-private');
      });
      socket.pipe(upstream); upstream.pipe(socket);
      socket.once('close', () => upstream.destroy()); upstream.once('close', () => socket.destroy());
    });
    bridge.listen(0, '127.0.0.1'); await once(bridge, 'listening');
  }
  function send(socket, data) {
    if (!fragment) { socket.write(data); return; }
    let index = 0;
    function next() {
      if (socket.destroyed || index === data.length) return;
      socket.write(data.subarray(index, ++index));
      if (index < data.length) {
        const timer = setTimeout(() => { timers.delete(timer); next(); }, 1); timers.add(timer);
      }
    }
    next();
  }
  const server = net.createServer(socket => {
    own(socket); seen.connections++; stages.tcp.resolve();
    let state = 'method', buffer = Buffer.alloc(0);
    function onData(chunk) {
      buffer = Buffer.concat([buffer, chunk]);
      assert.ok(buffer.length <= 1024, 'bounded synthetic SOCKS control fixture');
      if (state === 'method') {
        if (buffer.length < 2 || buffer.length < 2 + buffer[1]) return;
        seen.methods.push([...buffer.subarray(2, 2 + buffer[1])]); buffer = Buffer.alloc(0);
        stages.method.resolve();
        if (stall === 'method') return;
        if (truncate === 'method') { socket.end(Buffer.from([5])); return; }
        send(socket, Buffer.from([malformed === 'method' ? 4 : 5, malformed === 'unoffered' ? 2 : auth ? 2 : 0]));
        state = auth || malformed === 'unoffered' ? 'auth' : 'connect';
      } else if (state === 'auth') {
        if (buffer.length < 2 || buffer.length < 3 + buffer[1]) return;
        const count = 3 + buffer[1] + buffer[2 + buffer[1]];
        if (buffer.length < count) return;
        const user = buffer.subarray(2, 2 + buffer[1]).toString();
        const pass = buffer.subarray(3 + buffer[1], count).toString();
        if (credentials) assert.deepEqual([user, pass], credentials);
        buffer = Buffer.alloc(0); seen.auth++; stages.auth.resolve();
        if (stall === 'auth') return;
        if (truncate === 'auth') { socket.end(Buffer.from([1])); return; }
        send(socket, Buffer.from([malformed === 'auth' ? 2 : 1, rejectAuth ? 1 : 0])); state = 'connect';
      } else if (state === 'connect') {
        if (buffer.length < 5 || buffer.length < 7 + buffer[4]) return;
        assert.equal(buffer[3], 3, 'SOCKS5_HOSTNAME must delegate DNS');
        seen.domains.push(buffer.subarray(5, 5 + buffer[4]).toString());
        seen.ports.push(buffer.readUInt16BE(5 + buffer[4])); stages.connect.resolve();
        if (stall === 'connect') return;
        const reply = rawReply ?? Buffer.from([malformed === 'connect' ? 4 : 5, 0,
          malformed === 'reserved' ? 1 : 0, malformed === 'atyp' ? 9 : 1, 127, 0, 0, 1, 0, 1]);
        if (truncate === 'connect') { socket.end(reply.subarray(0, 7)); return; }
        send(socket, reply); socket.removeListener('data', onData);
        if (blind) {
          socket.pause();
          const tunnel = net.connect({ host: '127.0.0.1', port: bridge.address().port }); own(tunnel);
          tunnel.once('connect', () => { socket.pipe(tunnel); tunnel.pipe(socket); });
          socket.once('close', () => tunnel.destroy()); tunnel.once('close', () => socket.destroy());
          state = 'application'; buffer = Buffer.alloc(0); return;
        }
        if (stall === 'tls') { socket.on('data', () => stages.tls.resolve()); return; }
        const secure = new tls.TLSSocket(socket, { isServer: true, secureContext: tls.createSecureContext(material) });
        own(secure);
        secure.on('data', chunk => { seen.bytes += chunk.length; });
        secure.on('secure', () => {
          seen.sni.push(secure.servername); stages.tls.resolve();
          app.emit('connection', secure);
        });
        state = 'application'; buffer = Buffer.alloc(0);
      }
    }
    socket.on('data', onData);
  });
  server.listen(0, '127.0.0.1'); await once(server, 'listening');
  return { port: server.address().port, seen, sockets, reached: stage => stages[stage].promise,
    async close() {
      for (const timer of timers) clearTimeout(timer);
      for (const socket of sockets) socket.destroy();
      app.closeAllConnections();
      await Promise.all([server, bridge, shim].filter(Boolean).map(listener => new Promise(resolve => listener.close(resolve))));
    } };
}
function transport(proxy, material = valid, extras = {}) {
  return createTorCurlTransport({ host: '127.0.0.1', port: proxy.port, timeoutMs: 2000,
    gateway: { origin, onion, expectedLeafPem: material.cert, expectedSpkiSha256: material.spki }, ...extras }, { sdk });
}
async function withFixture(options, callback) {
  const proxy = await fixture(options);
  try { await callback(proxy); } finally { await proxy.close(); }
}

for (const selection of [
  { host: 'localhost', port: 9050 }, { host: '192.0.2.1', port: 9050 },
  { host: '::ffff:127.0.0.1', port: 9050 }, { host: '127.0.0.1', port: 0 },
  { host: '127.0.0.1', port: 9050, username: 'hostile-canary' },
  { host: '127.0.0.1', port: 9050, tls: { rejectUnauthorized: false } },
]) test('configuration rejects unsafe selection without IO', () => {
  assert.throws(() => createTorCurlTransport(selection, { sdk }), failure('configuration'));
});

test('diagnostics discard hostile SDK exception, cause and caller reason', () => {
  const error = new Error('hostile-canary', { cause: new Error('hostile-canary') });
  assert.equal(torCurlTransportDiagnostic(error).constraint, 'cause_unknown');
  assert.ok(!JSON.stringify(torCurlTransportDiagnostic(error)).includes('hostile-canary'));
  assert.ok(!JSON.stringify(new TorCurlTransportFailure('hostile-canary')).includes('hostile-canary'));
});

for (const material of [valid, issuedLeaf]) test(`native gateway preHTTP binding passes ${material === valid ? 'self-signed' : 'non-self-signed leaf anchor'}`, async () => {
  await withFixture({ material, fragment: true }, async proxy => {
    const scope = transport(proxy, material);
    try {
      const response = await scope.fetch(`${origin}/synthetic`, { method: 'POST', body: 'synthetic-upload', headers: { Authorization: 'Bearer synthetic-credential' } });
      assert.equal(await response.text(), 'synthetic-body');
      assert.deepEqual(proxy.seen.domains, [onion]); assert.deepEqual(proxy.seen.ports, [80]);
      assert.deepEqual(proxy.seen.hosts, ['gateway.fixture.invalid']);
      assert.deepEqual(proxy.seen.sni, ['gateway.fixture.invalid']);
      await until(() => proxy.seen.uploads.length === 1);
      assert.deepEqual(proxy.seen.uploads, ['synthetic-upload']);
      assert.equal(proxy.seen.connections, 1);
    } finally { await scope.close(); }
    await until(() => proxy.sockets.size === 0);
  });
});

for (const [name, served, anchor, kind] of [
  ['wrong hostname', wrong, wrong, 'tls'], ['expired', expired, expired, 'tls'], ['not yet valid', future, future, 'tls'],
  ['wrong SPKI with trusted issuer', other, valid, 'pin'],
  ['untrusted replacement', other, issuedLeaf, 'tls'],
]) test(`native ${name} rejects BEFORE credential/header/body HTTP bytes`, async () => {
  await withFixture({ material: served }, async proxy => {
    const scope = transport(proxy, anchor);
    try {
      await assert.rejects(scope.fetch(`${origin}/hostile-canary`, { method: 'POST', body: 'hostile-canary', headers: { authorization: 'Bearer hostile-canary' } }), failure(kind));
    } finally { await scope.close(); }
    assert.equal(proxy.seen.bytes, 0); assert.equal(proxy.seen.http, 0);
    await until(() => proxy.sockets.size === 0);
  });
});

for (const options of [
  { auth: false }, { auth: false, credentials: ['synthetic-user', 'synthetic-password'] },
  { auth: true, credentials: ['synthetic-user', 'synthetic-password'] },
]) test('optional independent proxy auth and no-auth both pass', async () => {
  await withFixture(options, async proxy => {
    const scope = transport(proxy, valid, options.credentials ? { username: options.credentials[0], password: options.credentials[1] } : {});
    try { assert.equal(await (await scope.fetch(origin)).text(), 'synthetic-body'); }
    finally { await scope.close(); }
    assert.equal(proxy.seen.http, 1);
  });
});

for (const options of [
  { malformed: 'method' }, { malformed: 'unoffered' }, { malformed: 'auth', auth: true },
  { rejectAuth: true, auth: true }, { malformed: 'connect' }, { malformed: 'atyp' },
  { malformed: 'reserved' }, { truncate: 'method' }, { truncate: 'auth', auth: true }, { truncate: 'connect' },
]) test(`stock native SOCKS rejects ${JSON.stringify(options)}`, async () => {
  await withFixture(options, async proxy => {
    const scope = transport(proxy, valid, options.auth ? { username: 'synthetic-user', password: 'synthetic-password' } : {});
    try { await assert.rejects(scope.fetch(origin), failure()); }
    finally { await scope.close(); }
    assert.equal(proxy.seen.http, 0);
    await until(() => proxy.sockets.size === 0);
  });
});

for (const stage of ['method', 'auth', 'connect', 'tls', 'http', 'body']) {
  for (const stop of ['abort', 'close']) test(`${stop} during ${stage} releases native operation without replay`, async () => {
    await withFixture({ stall: stage, auth: stage === 'auth' }, async proxy => {
      const scope = transport(proxy, valid, stage === 'auth' ? { username: 'synthetic-user', password: 'synthetic-password' } : {});
      const controller = new AbortController();
      const pending = scope.fetch(origin, { signal: controller.signal });
      let rejected;
      // Attach rejection before triggering cancellation (no unhandled diagnostics).
      const outcome = pending.catch(error => { rejected = error; });
      await proxy.reached(stage === 'body' ? 'http' : stage);
      let reader;
      if (stage === 'body') { reader = (await pending).body.getReader(); await reader.read(); }
      if (stop === 'abort') controller.abort(new Error('hostile-canary')); else await scope.close();
      if (stage === 'body') await assert.rejects(reader.read(), failure(stop === 'abort' ? 'cancelled' : 'closed'))
      else { await outcome; failure(stop === 'abort' ? 'cancelled' : 'closed')(rejected); }
      await scope.close();
      await until(() => proxy.sockets.size === 0);
      assert.equal(proxy.seen.connections, 1);
    });
  });
}

test('native response-body cancel closes paused stream promptly', async () => {
  await withFixture({ stall: 'body' }, async proxy => {
    const scope = transport(proxy);
    try {
      const response = await scope.fetch(origin);
      await response.body.cancel(new Error('hostile-canary'));
      await until(() => proxy.sockets.size === 0);
    } finally { await scope.close(); }
  });
});

test('native truncated content-length reply preserves partial code', async () => {
  await withFixture({ handler(_request, response) { response.writeHead(200, { 'content-length': '100' }); response.write('short'); response.socket.end(); } }, async proxy => {
    const scope = transport(proxy);
    try {
      const response = await scope.fetch(origin);
      await assert.rejects(response.text(), failure('partial'));
    } finally { await scope.close(); }
  });
});

test('header control memory is bounded and closed diagnostic survives callback', async () => {
  await withFixture({ handler(_request, response) { for (let i = 0; i < 100; i++) response.setHeader(`x-synthetic-${i}`, 'x'.repeat(1000)); response.end('short'); } }, async proxy => {
    const scope = transport(proxy);
    try { await assert.rejects(scope.fetch(origin), failure('headers')); }
    finally { await scope.close(); }
  });
});

test('scope forbids env NO_PROXY/direct bypass and destination DNS', async () => {
  const old = { no_proxy: process.env.no_proxy, NO_PROXY: process.env.NO_PROXY, HTTPS_PROXY: process.env.HTTPS_PROXY };
  const lookup = dns.lookup;
  let lookups = 0;
  process.env.NO_PROXY = '*'; process.env.no_proxy = '*'; process.env.HTTPS_PROXY = 'http://hostile-canary.invalid:1';
  dns.lookup = (...args) => { if (args[0] !== '127.0.0.1') lookups++; return lookup(...args); }; syncBuiltinESMExports();
  try {
    await withFixture({}, async proxy => {
      const scope = transport(proxy);
      try { assert.equal(await (await scope.fetch(origin)).text(), 'synthetic-body'); }
      finally { await scope.close(); }
      assert.equal(proxy.seen.connections, 1); assert.equal(lookups, 0);
    });
  } finally {
    dns.lookup = lookup; syncBuiltinESMExports();
    for (const [key, value] of Object.entries(old)) { if (value === undefined) delete process.env[key]; else process.env[key] = value; }
  }
});

test('public native verification, proxied acquisition redirects and gateway-anchor isolation', async () => {
  // Real native Curl subclass configures a synthetic public root through supported
  // setOpt in the offline fixture only. No mocked transport/decoder or production
  // TLS override API: factory public transfers retain their native default roots.
  class FixtureTrustCurl extends sdk.Curl {
    constructor() {
      super();
      this.setOpt('CAINFO_BLOB', publicLeaf.cert);
      this.setOpt('CAPATH', null);
    }
  }
  const nativeFixtureSDK = { ...sdk, Curl: FixtureTrustCurl };
  const publicScope = proxy => createTorCurlTransport({ host: '127.0.0.1', port: proxy.port, timeoutMs: 2000,
    gateway: { origin, onion, expectedLeafPem: valid.cert, expectedSpkiSha256: valid.spki } }, { sdk: nativeFixtureSDK });
  await withFixture({ material: publicLeaf, handler(_request, response, seen) {
    if (seen.http === 1) response.writeHead(302, { location: 'https://second.fixture.invalid/final' }).end();
    else response.end('public-final');
  } }, async proxy => {
    const scope = publicScope(proxy);
    try {
      const response = await scope.fetch('https://public.fixture.invalid');
      assert.equal(await response.text(), 'public-final');
      assert.deepEqual(proxy.seen.domains, ['public.fixture.invalid', 'second.fixture.invalid']);
      assert.equal(proxy.seen.connections, 2);
    } finally { await scope.close(); }
  });
  await withFixture({ material: publicSecond }, async proxy => {
    const scope = publicScope(proxy);
    try { assert.equal(await (await scope.fetch('https://second.fixture.invalid')).text(), 'synthetic-body'); }
    finally { await scope.close(); }
  });
  await withFixture({}, async proxy => {
    const scope = publicScope(proxy);
    try { await assert.rejects(scope.fetch('https://public.fixture.invalid'), failure('tls')); }
    finally { await scope.close(); }
    assert.equal(proxy.seen.bytes, 0);
  });
});

test('native EOF keeps unread buffered bytes and supports compression', async () => {
  await withFixture({ handler(_request, response) {
    response.setHeader('content-encoding', 'gzip'); response.end(gzipSync('synthetic-compressed'));
  } }, async proxy => {
    const scope = transport(proxy);
    try {
      const response = await scope.fetch(origin);
      await new Promise(resolve => setTimeout(resolve, 50));
      assert.equal(await response.text(), 'synthetic-compressed');
    } finally { await scope.close(); }
  });
});

test('native upload cancellation owns a stalled Web request body', async () => {
  await withFixture({ stall: 'http' }, async proxy => {
    const scope = transport(proxy);
    const controller = new AbortController(); let cancelled = false;
    const body = new ReadableStream({ start(stream) { stream.enqueue(new TextEncoder().encode('synthetic-initial')); }, cancel() { cancelled = true; } });
    const pending = scope.fetch(origin, { method: 'POST', body, duplex: 'half', signal: controller.signal });
    const rejected = assert.rejects(pending, failure('cancelled'));
    await proxy.reached('http'); controller.abort(new Error('hostile-canary'));
    await rejected; await scope.close();
    await until(() => proxy.sockets.size === 0 && cancelled);
    assert.equal(proxy.seen.connections, 1);
  });
});

test('truncation disposal clears referenced SDK timers and owned native handles', async () => {
  await withFixture({ truncate: 'connect' }, async proxy => {
    const handles = [], multis = [], timers = [];
    class ObservedCurl extends sdk.Curl { constructor() { super(); handles.push(this); } }
    class ObservedMulti extends sdk.Multi { constructor() { super(); multis.push(this); } }
    const hook = createHook({ init(_id, type, _trigger, resource) { if (type === 'Timeout') timers.push(resource); } });
    const scope = createTorCurlTransport({ host: '127.0.0.1', port: proxy.port, timeoutMs: 2000 }, { sdk: { ...sdk, Curl: ObservedCurl, Multi: ObservedMulti } });
    hook.enable();
    try {
      await assert.rejects(scope.fetch(origin), failure()); await scope.close();
      await until(() => proxy.sockets.size === 0);
      assert.equal(handles.length, 1); assert.equal(handles[0].isOpen, false);
      assert.equal(timers.filter(timer => !timer._destroyed && timer.hasRef()).length, 0);
      assert.throws(() => multis[0].getCount(), 'native Multi is closed');
    } finally { hook.disable(); await scope.close(); }
  });
});

test('native SDK backpressure pauses without buffering a complete large response', async () => {
  const total = 16 * 1024 * 1024; let written = 0, nativeStream, handle;
  await withFixture({ handler(_request, response) {
    const chunk = Buffer.alloc(16_384, 97);
    function write() {
      while (!response.destroyed && written < total) { written += chunk.length; if (!response.write(chunk)) { response.once('drain', write); return; } }
      if (!response.destroyed) response.end();
    }
    write();
  } }, async proxy => {
    class ObservedCurl extends sdk.Curl {
      constructor() { super(); handle = this; this.on('stream', stream => { nativeStream = stream; }); }
    }
    const scope = createTorCurlTransport({ host: '127.0.0.1', port: proxy.port, timeoutMs: 2000,
      gateway: { origin, onion, expectedLeafPem: valid.cert, expectedSpkiSha256: valid.spki } }, { sdk: { ...sdk, Curl: ObservedCurl } });
    try {
      const response = await scope.fetch(origin);
      await until(() => handle.handle.isPausedRecv);
      assert.ok(nativeStream.readableLength <= 32_768, 'native Node readable has bounded transport buffer');
      assert.ok(written < total, 'connected consumer stall propagates to synthetic sender');
      const reader = response.body.getReader(); let received = 0;
      for (;;) { const { done, value } = await reader.read(); if (done) break; received += value.length; }
      assert.equal(received, total);
    } finally { await scope.close(); }
  });
});

for (const status of [1, 2, 3, 4, 5, 6, 7, 8]) test(`native SOCKS reply ${status} retains closed handshake diagnostic`, async () => {
  await withFixture({ rawReply: Buffer.from([5, status, 0, 1, 127, 0, 0, 1, 0, 1]) }, async proxy => {
    const scope = transport(proxy);
    try { await assert.rejects(scope.fetch(origin), failure('socks')); }
    finally { await scope.close(); }
    assert.equal(proxy.seen.http, 0);
  });
});

for (const stage of ['method', 'tls', 'http']) test(`deadline disposes stalled ${stage} with no replay`, async () => {
  await withFixture({ stall: stage }, async proxy => {
    const scope = transport(proxy, valid, { timeoutMs: 75 });
    try { await assert.rejects(scope.fetch(origin), failure('deadline')); }
    finally { await scope.close(); }
    await until(() => proxy.sockets.size === 0); assert.equal(proxy.seen.connections, 1);
  });
});

test('late response and simultaneous abort/close never escape closed primary', async () => {
  await withFixture({ handler(_request, response) {
    response.write('synthetic-first');
    const timer = setTimeout(() => { if (!response.destroyed) response.end('hostile-canary'); }, 30);
    response.once('close', () => clearTimeout(timer));
  } }, async proxy => {
    const scope = transport(proxy); const controller = new AbortController();
    const response = await scope.fetch(origin, { signal: controller.signal });
    const reader = response.body.getReader(); await reader.read();
    controller.abort(new Error('hostile-canary')); const closing = scope.close();
    await assert.rejects(reader.read(), failure('cancelled'));
    await closing; assert.equal(scope.close(), closing);
    await until(() => proxy.sockets.size === 0); assert.equal(proxy.seen.connections, 1);
  });
});

const wrapperRoot = resolve(process.env.POSSUMS_TOR_TEST_SDK_ROOT ?? '/tmp/possums-curl-qualification/node_modules');
const wrapperRequire = createRequire(join(wrapperRoot, 'fixture.cjs'));

test('released OpenAI 7.19.0 SDK progressively decodes native SSE without inference/replay', async () => {
  assert.equal(JSON.parse(readFileSync(join(wrapperRoot, 'openai/package.json'), 'utf8')).version, '7.19.0');
  const OpenAI = wrapperRequire('openai').default;
  const finish = Promise.withResolvers();
  const event = content => `data: ${JSON.stringify({ id: 'synthetic', object: 'chat.completion.chunk', created: 1, model: 'synthetic', choices: [{ index: 0, delta: { content }, finish_reason: null }] })}\n\n`;
  await withFixture({ blind: true, handler(_request, response) {
    response.setHeader('content-type', 'text/event-stream'); response.write(event('synthetic-first'));
    void finish.promise.then(() => { if (!response.destroyed) response.end(`${event('synthetic-last')}data: [DONE]\n\n`); });
  } }, async proxy => {
    const scope = transport(proxy);
    try {
      const client = new OpenAI({ apiKey: 'synthetic-key', baseURL: `${origin}/v1`, maxRetries: 0, fetch: scope.fetch });
      const result = await client.chat.completions.create({ model: 'synthetic', messages: [{ role: 'user', content: 'synthetic' }], stream: true });
      const iterator = result[Symbol.asyncIterator]();
      assert.equal((await iterator.next()).value.choices[0].delta.content, 'synthetic-first');
      finish.resolve();
      assert.equal((await iterator.next()).value.choices[0].delta.content, 'synthetic-last');
      assert.equal((await iterator.next()).done, true);
      assert.equal(proxy.seen.connections, 1);
    } finally { finish.resolve(); await scope.close(); }
  });
});

test('released EHBP 0.3.2 SDK decrypts native incremental authenticated frames', async () => {
  assert.equal(JSON.parse(readFileSync(join(wrapperRoot, 'ehbp/package.json'), 'utf8')).version, '0.3.2');
  const ehbp = wrapperRequire('ehbp');
  const identity = await ehbp.Identity.generate();
  const encrypted = await identity.encryptRequestWithContext(new Request(origin, { method: 'POST', body: 'synthetic-plaintext' }));
  const token = await ehbp.extractSessionRecoveryToken(encrypted.context);
  const nonce = Buffer.alloc(32, 7);
  const keys = await ehbp.deriveResponseKeys(token.exportedSecret, token.requestEnc, nonce);
  const frame = async (seq, text) => {
    const cipher = await ehbp.encryptChunk(keys, seq, new TextEncoder().encode(text));
    const prefix = Buffer.alloc(4); prefix.writeUInt32BE(cipher.length);
    return Buffer.concat([prefix, cipher]);
  };
  const first = await frame(0, 'synthetic-first'), last = await frame(1, 'synthetic-last');
  const finish = Promise.withResolvers();
  await withFixture({ blind: true, handler(_request, response) {
    response.setHeader('ehbp-response-nonce', nonce.toString('hex')); response.write(first);
    void finish.promise.then(() => { if (!response.destroyed) response.end(last); });
  } }, async proxy => {
    const scope = transport(proxy);
    try {
      const nativeResponse = await scope.fetch(encrypted.request);
      const decrypted = await identity.decryptResponseWithContext(nativeResponse, encrypted.context);
      const reader = decrypted.body.getReader();
      assert.equal(new TextDecoder().decode((await reader.read()).value), 'synthetic-first');
      finish.resolve();
      assert.equal(new TextDecoder().decode((await reader.read()).value), 'synthetic-last');
      assert.equal((await reader.read()).done, true);
      assert.equal(proxy.seen.connections, 1);
      await until(() => proxy.seen.uploads.length === 1);
      assert.ok(!proxy.seen.uploads[0].includes('synthetic-plaintext'));
    } finally { finish.resolve(); await scope.close(); }
  });
});

test('native paused-body cancellation closes both handles and stream', async () => {
  let handle;
  await withFixture({ handler(_request, response) {
    const chunk = Buffer.alloc(16_384);
    function write() {
      while (!response.destroyed) if (!response.write(chunk)) { response.once('drain', write); return; }
    }
    write();
  } }, async proxy => {
    class ObservedCurl extends sdk.Curl { constructor() { super(); handle = this; } }
    const scope = createTorCurlTransport({ host: '127.0.0.1', port: proxy.port, timeoutMs: 2000,
      gateway: { origin, onion, expectedLeafPem: valid.cert, expectedSpkiSha256: valid.spki } }, { sdk: { ...sdk, Curl: ObservedCurl } });
    try {
      const response = await scope.fetch(origin);
      await until(() => handle.handle.isPausedRecv);
      await response.body.cancel();
      await scope.close(); assert.equal(handle.isOpen, false);
      await until(() => proxy.sockets.size === 0);
    } finally { await scope.close(); }
  });
});

test('closed scope and pre-aborted caller cannot start native work', async () => {
  await withFixture({}, async proxy => {
    const scope = transport(proxy); const controller = new AbortController();
    controller.abort(new Error('hostile-canary'));
    await assert.rejects(scope.fetch(origin, { signal: controller.signal }), failure('cancelled'));
    await scope.close(); await assert.rejects(scope.fetch(origin), failure('closed'));
    assert.equal(proxy.seen.connections, 0);
  });
});

test('original gateway Host cannot be replaced by request header', async () => {
  await withFixture({}, async proxy => {
    const scope = transport(proxy);
    try { assert.equal(await (await scope.fetch(origin, { headers: { host: 'hostile-canary.invalid' } })).text(), 'synthetic-body'); }
    finally { await scope.close(); }
    assert.deepEqual(proxy.seen.hosts, ['gateway.fixture.invalid']);
  });
});

test('refused numeric loopback proxy preserves numeric connection failure', async () => {
  const server = net.createServer(); server.listen(0, '127.0.0.1'); await once(server, 'listening');
  const port = server.address().port; await new Promise(resolve => server.close(resolve));
  const scope = transport({ port });
  try { await assert.rejects(scope.fetch(origin), failure('proxy')); }
  finally { await scope.close(); }
});

test('abort immediately after starting native transfer does not send HTTP', async () => {
  await withFixture({}, async proxy => {
    const scope = transport(proxy); const controller = new AbortController();
    const pending = scope.fetch(origin, { signal: controller.signal });
    const rejected = assert.rejects(pending, failure('cancelled'));
    controller.abort(new Error('hostile-canary')); await rejected; await scope.close();
    await until(() => proxy.sockets.size === 0); assert.equal(proxy.seen.http, 0);
  });
});

test('native TLS EOF retains handshake stage without claiming certificate cause', async () => {
  await withFixture({ stall: 'tls' }, async proxy => {
    const scope = transport(proxy); const pending = scope.fetch(origin);
    const rejected = assert.rejects(pending, failure('tls_handshake'));
    await proxy.reached('tls'); for (const socket of proxy.sockets) socket.end();
    await rejected; await scope.close(); assert.equal(proxy.seen.http, 0);
  });
});

test('gateway redirect is never followed or replayed', async () => {
  await withFixture({ handler(_request, response) { response.writeHead(307, { location: 'https://hostile-canary.invalid/' }).end(); } }, async proxy => {
    const scope = transport(proxy);
    try { await assert.rejects(scope.fetch(origin, { method: 'POST', body: 'synthetic' }), failure('redirect')); }
    finally { await scope.close(); }
    assert.equal(proxy.seen.connections, 1); assert.equal(proxy.seen.http, 1);
  });
});

test('native B.onion:80 route crosses synthetic blind bridge before public TLS shim', async () => {
  await withFixture({ blind: true }, async proxy => {
    const scope = transport(proxy);
    try {
      const response = await scope.fetch(`${origin}/synthetic`, { method: 'POST', body: 'synthetic-bridge-private', headers: { authorization: 'Bearer synthetic-bridge-private' } });
      assert.equal(await response.text(), 'synthetic-body');
      assert.deepEqual(proxy.seen.domains, [onion]); assert.deepEqual(proxy.seen.ports, [80]);
      assert.deepEqual(proxy.seen.sni, ['gateway.fixture.invalid']);
      assert.deepEqual(proxy.seen.hosts, ['gateway.fixture.invalid']);
      assert.equal(proxy.seen.blindFirstByte, 22); assert.ok(proxy.seen.blindBytes > 0);
      assert.equal(proxy.seen.blindPlaintextLeak, false);
    } finally { await scope.close(); }
    await until(() => proxy.sockets.size === 0);
  });
});

test.after(() => rmSync(scratch, { recursive: true, force: true }));
