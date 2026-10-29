// PRIVACY.md data handling, typed failures and synthetic-testing exception.
// OFFLINE ONLY: actual official-SDK helper, fake SOCKS, native local TLS/HTTP.
// Synthetic appraisal contexts are test-build substitutions, NOT hardware proof.
import assert from 'node:assert/strict';
import net from 'node:net';
import tls from 'node:tls';
import dns from 'node:dns';
import https from 'node:https';
import { once } from 'node:events';
import { syncBuiltinESMExports } from 'node:module';
import { createHash, X509Certificate, generateKeyPairSync, sign } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { gzipSync } from 'node:zlib';
const source = process.env.PHASE02_RELEASE_SOURCE;
assert(source && path.isAbsolute(source), 'select a scratch copy with pinned dependencies');
const helperPath = process.env.POSSUMS_TOR_NATIVE_HELPER ?? '/tmp/possums-tor-native-target/debug/possums-tor-native';
const { build } = await import(pathToFileURL(path.join(source, 'node_modules/esbuild/lib/main.js')));
const { base32 } = await import(pathToFileURL(path.join(source, 'node_modules/@scure/base/index.js')));
const out = await mkdtemp(path.join(tmpdir(), 'possums-tor-channel-check-'));
const previousCache = process.env.XDG_CACHE_HOME;
process.env.XDG_CACHE_HOME = path.join(out, 'cache');
const sha = value => createHash('sha256').update(value).digest('hex');
const bytes = value => Buffer.from(JSON.stringify(value));
const rawB = Buffer.alloc(32, 9), otherB = Buffer.alloc(32, 8);
function onion(key) {
  const checksum = createHash('sha3-256').update('.onion checksum').update(key).update(Buffer.from([3])).digest().subarray(0, 2);
  return base32.encode(Buffer.concat([key, checksum, Buffer.from([3])])).toLowerCase() + '.onion';
}
function generateCertificate(name) {
  execFileSync('/usr/bin/openssl', ['req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-keyout', path.join(out, name + '.key'),
    '-out', path.join(out, name + '.pem'), '-days', '1', '-subj', '/CN=gateway.possums.dev',
    '-addext', 'subjectAltName=DNS:gateway.possums.dev,DNS:api.github.com,DNS:github.com,DNS:release-assets.githubusercontent.com,DNS:localhost'], { stdio: 'ignore' });
}
generateCertificate('valid'); generateCertificate('wrong');
const cert = await readFile(path.join(out, 'valid.pem'), 'utf8');
const wrongCert = await readFile(path.join(out, 'wrong.pem'), 'utf8');
const pin = sha(new X509Certificate(cert).publicKey.export({ type: 'spki', format: 'der' }));
const config = new Uint8Array([0, 0, 32, ...Array(32).fill(7), 0, 4, 0, 1, 0, 2]);
const yaml = Buffer.from('cvm-version: 0.14.12\ncontainers: []\n');
const manifest = bytes({ hashes: { version: 'v0.14.12' }, config: yaml.toString('base64'), cmdline: `tinfoil-config-hash=${sha(yaml)}` });
const { publicKey, privateKey } = generateKeyPairSync('ed25519');
const A = Buffer.from(publicKey.export({ format: 'jwk' }).x, 'base64url');
const facts = { tls_key_sha256: pin, hpke_key: '07'.repeat(32), release_tag: 'v0.0.99', release_digest: sha(manifest),
  freshness_expires_at_unix: Math.floor(Date.now() / 1000) + 3600, tor_endorsement_key: A.toString('hex') };
let currentFacts = facts, verifyCalls = 0, expectedNonce, deferAppraisal;
globalThis.__torChannelAppraise = async (evidence, nonce, actualManifest, repository, signal) => {
  verifyCalls++;
  assert.equal(repository, 'ajbt200128/possums');
  assert.equal(sha(actualManifest), sha(manifest));
  assert.deepEqual(Buffer.from(nonce), expectedNonce);
  assert.equal(JSON.parse(Buffer.from(evidence)).nonce, Buffer.from(nonce).toString('hex'));
  if (deferAppraisal) await deferAppraisal;
  assert(!signal?.aborted);
  return { ...currentFacts };
};
globalThis.__directChannelAppraise = async (evidence, nonce, repository, signal, actualManifest) => {
  assert.equal(repository, 'ajbt200128/possums'); assert.equal(actualManifest, undefined);
  assert.deepEqual(Buffer.from(nonce), expectedNonce);
  assert.equal(JSON.parse(Buffer.from(evidence)).nonce, Buffer.from(nonce).toString('hex'));
  assert(!signal?.aborted); return { ...facts };
};
let checks = 0;
async function rejects(action, constraint) {
  await assert.rejects(action, error => {
    assert(!error.message.includes('HOSTILE')); assert.equal(error.cause, undefined);
    if (constraint) assert.equal(error.constraint ?? error.diagnostic?.constraint, constraint);
    return true;
  }); checks++;
}
await build({ stdin: { contents: `export { Channel, ReferenceClient } from '../../clients/pi/core/transport.ts';
export { connectTorPublished, recoverTorPublished, connectPublished, recoverPublished } from './bootstrap.ts';
export { TorConnection, prepareTorConnection, TorNativeTransportFailure, TorSetupBackendFailure } from './tor-connection.ts';
export { Operation, DiagnosticFailure } from '../../clients/pi/core/limits.ts';
export { pinnedRequest, sameV3Endpoint } from './v3.ts';`, resolveDir: source, loader: 'ts' },
  outfile: path.join(out, 'check.mjs'), bundle: true, platform: 'node', format: 'esm', target: 'node24',
  define: { __PHASE01_FIXTURE__: 'false' }, plugins: [{ name: 'synthetic-appraisal-only', setup(b) {
    b.onResolve({ filter: /^\.\/verifier\.mjs$/ }, () => ({ path: 'appraise', namespace: 'test-only' }));
    b.onLoad({ filter: /.*/, namespace: 'test-only' }, () => ({ contents: `export class VerifierFailure extends Error {}
      export const verifyTorV3 = (...args) => globalThis.__torChannelAppraise(...args);
      export const verifyV3 = (e, n, r, s, m) => globalThis.__directChannelAppraise(e, n, r, s, m);`, loader: 'js' }));
  } }] });
// Test-only native trust root. Real tls.connect still validates hostname/dates/
// trust and possesses the peer key; production code has no CA override hook.
const tlsConnect = tls.connect, lookup = dns.lookup, directFetch = globalThis.fetch;
let dnsCalls = 0, failWebPKI = false, observeTLS = true, globalFetchCalls = 0, afterTLS;
tls.connect = function(options, ...args) {
  if (observeTLS) {
    assert(options.socket, 'Tor requests must supply the owned raw Duplex, never a direct host');
    assert.equal(options.host, undefined); assert.equal(options.port, undefined);
    assert.equal(options.ALPNProtocols.join(','), 'http/1.1');
    assert.equal(options.session, undefined, 'no supplied TLS resumption state');
  }
  const peer = tlsConnect.call(this, { ...options, ...(!failWebPKI && options.rejectUnauthorized ? { ca: cert } : {}) }, ...args);
  peer.once('secureConnect', () => afterTLS?.());
  return peer;
};
dns.lookup = function(...args) { dnsCalls++; return lookup.apply(this, args); };
syncBuiltinESMExports();
const test = await import(pathToFileURL(path.join(out, 'check.mjs')));
const seen = { destinations: [], ports: [], requests: [], sni: [], proxyTCP: 0 };
let responseB = rawB, responseCert = cert, responseConfig = config, setupError, setupStatus = 200, setupPayloadCorrupt = false;
let onHTTP, interruptBodyScope, useWrongPeer = false, handshakeReject = false, stallTLS = false;
const sockets = new Set();
const own = socket => { sockets.add(socket); socket.on('error', () => {}); socket.once('close', () => sockets.delete(socket)); };
const publicHost = 'gateway.possums.dev';
function setup(nonce) {
  expectedNonce = Buffer.from(nonce, 'hex');
  const payload = Buffer.concat([Buffer.from('possums/tor-onion-endorsement\0'), Buffer.from([1]), expectedNonce, responseB]);
  const signature = sign(null, payload, privateKey);
  if (setupPayloadCorrupt) payload[31] ^= 1;
  return bytes({ evidence: { nonce }, payload: payload.toString('hex'), signature: signature.toString('hex') });
}
function respond(req, res) {
  seen.requests.push({ host: req.headers.host, url: req.url, auth: req.headers.authorization, method: req.method,
    agent: req.headers['user-agent'] });
  req.resume(); onHTTP?.();
  if (interruptBodyScope && (req.url.startsWith('/attestation/tor?nonce=') || req.url === '/v1/models')) {
    const scope = interruptBodyScope;
    res.writeHead(200); res.write('{'); setTimeout(() => { void scope.close(); }, 20); return;
  }
  let body, headers = {}, status = 200;
  if (req.url.endsWith('/releases/latest')) { status = 403; headers['x-ratelimit-remaining'] = '0'; }
  else if (req.url.includes('/releases/download/')) { status = 302; headers.location = 'https://release-assets.githubusercontent.com/synthetic'; }
  else if (req.url === '/synthetic') body = manifest;
  else if (req.url.startsWith('/attestation/tor?nonce=')) {
    assert.match(req.url, /^\/attestation\/tor\?nonce=[0-9a-f]{64}$/);
    status = setupStatus;
    body = setupError ?? setup(req.url.split('nonce=')[1]);
  } else if (req.url.endsWith('/tinfoil-certificate')) body = bytes({ certificate: responseCert });
  else if (req.url.endsWith('/hpke-keys')) body = responseConfig;
  else if (req.url === '/v1/auth/challenge') body = bytes({ challenge: 'synthetic' });
  else if (req.url === '/v1/models') body = bytes({ synthetic: true });
  else if (req.url === '/stream') { res.writeHead(200); res.write('synthetic'); return; }
  else if (req.url === '/broken') { res.writeHead(200, { 'content-encoding': 'gzip' }); res.end('HOSTILE malformed compression'); return; }
  else if (req.url === '/truncated') { res.writeHead(200, { 'content-length': '100' }); res.write('short'); setTimeout(() => res.destroy(), 10); return; }
  else if (req.url === '/redirect') { status = 302; headers.location = 'https://unfollowed.fixture.invalid/'; }
  else if (req.url === '/no-body') { status = 204; }
  else body = bytes({ ok: true });
  if (body) { headers['content-encoding'] = 'gzip'; body = gzipSync(body); }
  res.writeHead(status, headers); res.end(body);
}
const server = https.createServer({ cert, key: await readFile(path.join(out, 'valid.key')) }, respond);
const wrongServer = https.createServer({ cert: wrongCert, key: await readFile(path.join(out, 'wrong.key')) }, respond);
for (const s of [server, wrongServer]) { s.on('connection', own); s.on('secureConnection', sock => seen.sni.push(sock.servername)); s.listen(0, '127.0.0.1'); await once(s, 'listening'); }
// Only this synthetic server parses SOCKS controls. Production uses the SDK.
const proxy = net.createServer(socket => {
  own(socket); seen.proxyTCP++;
  let state = 'greeting', buffer = Buffer.alloc(0);
  function onData(chunk) {
    buffer = Buffer.concat([buffer, chunk]); assert(buffer.length < 1024);
    if (state === 'greeting') {
      if (buffer.length < 2 + buffer[1]) return;
      buffer = Buffer.alloc(0); state = 'connect'; socket.write(Buffer.from([5, 0]));
    } else {
      if (buffer.length < 5 || buffer.length < 7 + buffer[4]) return;
      assert.equal(buffer[3], 3);
      seen.destinations.push(buffer.subarray(5, 5 + buffer[4]).toString('ascii'));
      seen.ports.push(buffer.readUInt16BE(5 + buffer[4]));
      socket.removeListener('data', onData);
      if (handshakeReject) { socket.end(Buffer.from([5, 5, 0, 1, 127, 0, 0, 1, 0, 1])); return; }
      if (stallTLS) { socket.write(Buffer.from([5, 0, 0, 1, 127, 0, 0, 1, 0, 1])); return; }
      const upstream = net.connect({ host: '127.0.0.1', port: (useWrongPeer ? wrongServer : server).address().port }); own(upstream);
      upstream.once('connect', () => { socket.write(Buffer.from([5, 0, 23, 1, 127, 0, 0, 1, 0, 1])); socket.pipe(upstream).pipe(socket); });
      socket.once('close', () => upstream.destroy()); upstream.once('close', () => socket.destroy());
    }
  }
  socket.on('data', onData);
});
proxy.listen(0, '127.0.0.1'); await once(proxy, 'listening');
const proxyConfig = { helperPath, host: '127.0.0.1', port: proxy.address().port, timeoutMs: 1500 };
const scopes = new Set();
const prepare = (address = onion(rawB), config = proxyConfig) => { const scope = test.prepareTorConnection(config, address); scopes.add(scope); return scope; };
async function publicRequest(scope, pathname, signal) {
  const op = new test.Operation(5000, signal);
  try {
    const response = await scope.publicFetch(new Request('https://' + publicHost + pathname, { signal: op.controller.signal, credentials: 'omit', redirect: 'error' }), op);
    return await response.json();
  } finally { op.close(); }
}
function torInput(nonce = Buffer.alloc(32, 3), selected = rawB) {
  const payload = Buffer.concat([Buffer.from('possums/tor-onion-endorsement\0'), Buffer.from([1]), nonce, selected]);
  expectedNonce = Buffer.from(nonce);
  return { evidence: bytes({ nonce: nonce.toString('hex') }), nonce, manifest, tag: facts.release_tag,
    certificate: cert, config, payload, signature: sign(null, payload, privateKey) };
}
try {
  const dnsBaseline = dnsCalls;
  globalThis.fetch = async () => { globalFetchCalls++; throw new Error('HOSTILE direct public fallback'); };
  await rejects(() => test.connectTorPublished({ publicFetch: () => {}, connectSocket: () => {} }), 'endpoint_binding');
  assert.throws(() => new test.TorConnection(Symbol(), proxyConfig, onion(rawB))); checks++;
  assert.throws(() => prepare('localhost'), error => error.code === 'possums_tor_onion_invalid'); checks++;
  const local = { ...proxyConfig };
  const scope = prepare(onion(rawB), local); local.port = 1;
  const exposedB = scope.onionIdentity; exposedB.fill(0);
  assert.deepEqual(Buffer.from(scope.onionIdentity), rawB); assert(Object.isFrozen(scope)); checks++;
  const first = await test.connectTorPublished(scope);
  assert.equal(first.channel.endpointIdentity.onionIdentity, rawB.toString('hex')); checks++;
  assert.equal(verifyCalls, 1, 'one full Tor projection after fresh public release appraisal'); checks++;
  assert.deepEqual(seen.destinations.slice(0, 5), [onion(rawB), 'github.com', 'release-assets.githubusercontent.com', onion(rawB), onion(rawB)]);
  assert.deepEqual(seen.ports.slice(0, 5), [80, 443, 443, 80, 80]);
  assert(!seen.destinations.includes('api.github.com')); checks++;
  assert(seen.sni.includes(publicHost) && !seen.sni.some(s => s.endsWith('.onion')));
  assert(seen.requests.every(r => !r.auth && r.method === 'GET' && r.agent === 'possums-pi'));
  assert(seen.requests.filter(r => r.url.startsWith('/attestation/tor')).every(r => r.host === publicHost)); checks++;
  assert.equal(globalFetchCalls, 0); assert.equal(dnsCalls, dnsBaseline); checks++;
  const optionalAuth = prepare(onion(rawB), { ...proxyConfig, username: 'synthetic-proxy-only', password: 'synthetic-proxy-only' });
  assert.deepEqual(await publicRequest(optionalAuth, '/'), { ok: true }); checks++;
  assert.deepEqual(await first.channel.challenge(), { challenge: 'synthetic' }); checks++;
  assert.deepEqual(await first.channel.models('a'.repeat(43)), { synthetic: true });
  assert.equal(seen.requests.at(-1).auth, 'Bearer ' + 'a'.repeat(43));
  assert.equal(seen.requests.at(-1).agent, undefined); checks++;
  await rejects(() => test.recoverTorPublished(first, scope));
  const nonces = seen.requests.filter(r => r.url.startsWith('/attestation/tor')).map(r => r.url);
  assert.equal(new Set(nonces).size, nonces.length); checks++;
  currentFacts = { ...facts, freshness_expires_at_unix: facts.freshness_expires_at_unix + 100 };
  const renewed = await test.recoverTorPublished(first, scope);
  assert(test.sameV3Endpoint(first.channel.endpointIdentity, renewed.channel.endpointIdentity)); checks++;
  await rejects(() => test.recoverTorPublished(renewed, prepare()));
  await rejects(() => test.recoverPublished(first));
  await rejects(() => test.recoverTorPublished(first, prepare(onion(otherB))));
  assert(!test.sameV3Endpoint(first.channel.endpointIdentity, { ...first.channel.endpointIdentity, onionIdentity: otherB.toString('hex') })); checks++;
  responseB = otherB;
  const beforeMismatch = seen.requests.length;
  await rejects(() => test.connectTorPublished(scope), 'onion_identity');
  assert(seen.requests.slice(beforeMismatch).every(r => !r.auth)); checks++;
  responseB = rawB; setupPayloadCorrupt = true;
  await rejects(() => test.connectTorPublished(scope), 'challenge'); setupPayloadCorrupt = false;
  responseCert = wrongCert; await rejects(() => test.connectTorPublished(scope)); responseCert = cert;
  responseConfig = new Uint8Array(config); responseConfig[3] ^= 1;
  await rejects(() => test.connectTorPublished(scope)); responseConfig = config;
  for (const update of [{ release_tag: 'v0.0.98' }, { release_digest: '00'.repeat(32) },
    { hpke_key: '08'.repeat(32) }, { tls_key_sha256: '00'.repeat(32) }, { freshness_expires_at_unix: 1 }]) {
    currentFacts = { ...facts, ...update }; await rejects(() => test.connectTorPublished(scope));
  }
  currentFacts = facts;
  setupError = bytes({ error: { code: 'possums_v3_acquisition_http_status', stage: 'acquisition', constraint: 'http_status',
    status: 503, message: 'HOSTILE secret', action: 'HOSTILE secret' } });
  await rejects(() => test.connectTorPublished(scope), 'http_status'); setupError = undefined;
  const backendErrors = [
    { code: 'tor_grant_mismatch', stage: 'grant', constraint: 'derived_mounted_appraised_A_equality' },
    { code: 'tor_appraisal_expired', stage: 'freshness', constraint: 'current_evidence' },
    { code: 'possums_v3_acquisition_http_status', stage: 'acquisition', constraint: 'http_status', status: 503 },
  ];
  const cancel = ReadableStream.prototype.cancel;
  let failCleanup = false, cleanupFaults = 0;
  ReadableStream.prototype.cancel = function(...args) {
    if (failCleanup) { cleanupFaults++; throw new Error('HOSTILE cleanup failure'); }
    return cancel.apply(this, args);
  };
  try {
    setupStatus = 502;
    for (const backend of backendErrors) {
      setupError = bytes({ error: { ...backend, message: 'HOSTILE backend message', action: 'HOSTILE backend action' } });
      const before = seen.requests.length, appraisals = verifyCalls;
      onHTTP = () => { if (seen.requests.at(-1).url.startsWith('/attestation/tor?')) failCleanup = true; };
      await assert.rejects(() => test.connectTorPublished(scope), error => {
        assert(error instanceof test.TorSetupBackendFailure);
        for (const key of ['code', 'stage', 'constraint', 'status']) assert.equal(error[key], backend[key]);
        assert.equal(error.metadata_authenticated, false); assert.equal(error.transport_authorized, false);
        assert(!error.message.includes('HOSTILE')); assert.equal(error.cause, undefined);
        return true;
      }); checks++;
      failCleanup = false; onHTTP = undefined;
      assert.equal(verifyCalls, appraisals, 'non2xx never reaches appraisal');
      assert.equal(seen.requests.length - before, 1, 'stop at setup, before cache/GitHub/certificate/keys/private requests'); checks++;
    }
    assert(cleanupFaults >= backendErrors.length, 'cleanup faults must not replace closed backend failures'); checks++;
    for (const body of [Buffer.from('HOSTILE malformed JSON'), bytes({ error: { code: 'HOSTILE unknown' } }),
      bytes({ error: { code: 'HOSTILE unknown', stage: 'HOSTILE', constraint: 'HOSTILE', message: 'HOSTILE', action: 'HOSTILE' } }),
      undefined]) { // undefined emits a valid, signed success-shaped setup body on non2xx.
      setupError = body;
      const before = seen.requests.length, appraisals = verifyCalls;
      await assert.rejects(() => test.connectTorPublished(scope), error => {
        assert.equal(error.code, 'evidence_unavailable');
        assert.equal(error.observation.stage, 'gateway_attestation');
        assert.equal(error.observation.constraint, 'http'); assert.equal(error.observation.status, 502);
        assert(!error.message.includes('HOSTILE')); assert.equal(error.cause, undefined);
        return true;
      }); checks++;
      assert.equal(verifyCalls, appraisals); assert.equal(seen.requests.length - before, 1); checks++;
    }
  } finally {
    ReadableStream.prototype.cancel = cancel; failCleanup = false; onHTTP = undefined;
    setupError = undefined; setupStatus = 200;
  }
  // The scope cannot be turned into a bearer-bearing public acquisition path.
  const beforePublicSecret = seen.proxyTCP, publicOp = new test.Operation(5000);
  try {
    await rejects(() => scope.publicFetch(new Request('https://' + publicHost, { credentials: 'omit', headers: { authorization: 'synthetic' } }), publicOp), 'encoding');
    await rejects(() => scope.publicFetch(new Request('https://' + publicHost, { method: 'POST', credentials: 'omit', body: 'synthetic' }), publicOp), 'encoding');
    assert.equal(seen.proxyTCP, beforePublicSecret); checks++;
  } finally { publicOp.close(); }
  // Wrong actual peer blocks ALL HTTP, including a private bearer GET.
  useWrongPeer = true;
  const beforeWrong = seen.requests.length;
  await rejects(() => first.channel.models('a'.repeat(43)));
  await rejects(() => first.channel.control('/v1/sessions', { challenge: 'b'.repeat(32), credential: 'c'.repeat(32) }));
  assert.equal(seen.requests.length, beforeWrong); checks++;
  useWrongPeer = false;
  // Never tolerate an untrusted public WebPKI handshake just to get a quote.
  failWebPKI = true;
  const beforeWebPKI = seen.requests.length;
  await rejects(() => publicRequest(scope, '/')); assert.equal(seen.requests.length, beforeWebPKI); checks++;
  failWebPKI = false;
  handshakeReject = true;
  await assert.rejects(() => test.connectTorPublished(scope), error => error instanceof test.TorNativeTransportFailure
    && error.diagnostic.code === 'possums_tor_native_tunnel' && error.diagnostic.stage === 'socks_connect'); checks++;
  await assert.rejects(() => first.channel.models('a'.repeat(43)), error => error instanceof test.TorNativeTransportFailure); checks++;
  handshakeReject = false;
  interruptBodyScope = prepare();
  try {
    await assert.rejects(() => test.connectTorPublished(interruptBodyScope), error => error instanceof test.TorNativeTransportFailure
      && error.diagnostic.code === 'possums_tor_native_closed'); checks++;
  } finally { interruptBodyScope = undefined; }
  const interruptedScope = prepare(), interruptedChannel = await test.Channel.v3(torInput(), undefined, interruptedScope);
  interruptBodyScope = interruptedScope;
  try {
    await assert.rejects(() => interruptedChannel.models('a'.repeat(43)), error => error instanceof test.TorNativeTransportFailure
      && error.diagnostic.code === 'possums_tor_native_closed'); checks++;
  } finally { interruptBodyScope = undefined; }
  // Snapshot every caller byte before appraisal yields.
  let releaseAppraisal;
  deferAppraisal = new Promise(resolve => { releaseAppraisal = resolve; });
  const original = torInput();
  const snapshotInput = { ...original, evidence: Buffer.from(original.evidence), nonce: Buffer.from(original.nonce),
    manifest: Buffer.from(original.manifest), config: new Uint8Array(original.config), payload: Buffer.from(original.payload), signature: Buffer.from(original.signature) };
  const pending = test.Channel.v3(snapshotInput, undefined, scope);
  for (const field of ['evidence', 'nonce', 'manifest', 'config', 'payload', 'signature']) snapshotInput[field].fill(0);
  releaseAppraisal(); const snapshotted = await pending; deferAppraisal = undefined;
  assert.equal(snapshotted.endpointIdentity.config, sha(config)); checks++;
  await rejects(() => test.Channel.v3(torInput(undefined, otherB), undefined, scope), 'onion_identity');
  const otherChannel = await test.Channel.v3(torInput(undefined, otherB), undefined, prepare(onion(otherB)));
  assert.throws(() => otherChannel.requireRenewal(first.channel), error => error.constraint === 'policy'); checks++;
  // Freshness after crypto and again at actual TLS/handoff.
  currentFacts = { ...facts, freshness_expires_at_unix: Math.floor(Date.now() / 1000) + 60 };
  const expiring = await test.Channel.v3(torInput(), undefined, scope);
  const dateNow = Date.now;
  Date.now = () => currentFacts.freshness_expires_at_unix * 1000;
  const beforeExpired = seen.proxyTCP;
  try { await rejects(() => expiring.challenge(), 'expired'); assert.equal(seen.proxyTCP, beforeExpired); checks++; }
  finally { Date.now = dateNow; }
  const expireClock = () => { Date.now = () => currentFacts.freshness_expires_at_unix * 1000; };
  afterTLS = expireClock;
  const beforeTLSExpired = seen.requests.length;
  try { await rejects(() => expiring.challenge(), 'expired'); assert.equal(seen.requests.length, beforeTLSExpired); checks++; }
  finally { afterTLS = undefined; Date.now = dateNow; }
  onHTTP = expireClock;
  const beforeHandoffExpired = seen.requests.length;
  try { await rejects(() => expiring.challenge(), 'expired'); assert.equal(seen.requests.length, beforeHandoffExpired + 1); checks++; }
  finally { onHTTP = undefined; Date.now = dateNow; currentFacts = facts; }
  const closedAtTLS = prepare(), closeAtTLSChannel = await test.Channel.v3(torInput(), undefined, closedAtTLS);
  let tlsClose;
  afterTLS = () => { tlsClose = closedAtTLS.close(); };
  const beforeCloseAtTLS = seen.requests.length;
  try {
    await assert.rejects(() => closeAtTLSChannel.challenge(), error => error instanceof test.TorNativeTransportFailure
      && error.diagnostic.code === 'possums_tor_native_closed'); checks++;
    await tlsClose; assert.equal(seen.requests.length, beforeCloseAtTLS); checks++;
  } finally { afterTLS = undefined; }
  // HTTP parser/compression/lifecycle remain native Node implementations.
  assert.deepEqual(await publicRequest(scope, '/'), { ok: true }); checks++;
  await rejects(() => publicRequest(scope, '/broken'), 'body');
  await rejects(() => publicRequest(scope, '/truncated'), 'eof');
  for (const pathname of ['/redirect', '/no-body']) {
    const op = new test.Operation(5000), before = seen.requests.length;
    try {
      const response = await scope.publicFetch(new Request('https://' + publicHost + pathname, { credentials: 'omit' }), op);
      assert.equal(response.url, 'https://' + publicHost + pathname);
      assert.equal(response.redirected, false);
      assert.equal(response.status, pathname === '/redirect' ? 302 : 204);
      if (response.body) await response.body.cancel();
      assert.equal(seen.requests.length, before + 1); checks++;
    } finally { op.close(); }
  }
  const streamOp = new test.Operation(5000);
  const response = await scope.publicFetch(new Request('https://' + publicHost + '/stream', { credentials: 'omit' }), streamOp);
  await response.body.cancel(); streamOp.close(); checks++;
  // Closing the owner terminates an established body; no bootstrap-owned close.
  const closing = prepare(), closeOp = new test.Operation(5000);
  const closeBody = await closing.publicFetch(new Request('https://' + publicHost + '/stream', { credentials: 'omit' }), closeOp);
  const closeReader = closeBody.body.getReader(); await closeReader.read();
  const readAfterClose = assert.rejects(closeReader.read(), error => error instanceof test.TorNativeTransportFailure);
  await closing.close(); await readAfterClose; closeOp.close(); checks++;
  await assert.rejects(() => closing.connectSocket({ host: 'api.github.com', port: 443 }), error => error.diagnostic.code === 'possums_tor_native_closed'); checks++;
  stallTLS = true;
  const abort = new AbortController(), abortRequest = publicRequest(scope, '/', abort.signal);
  setTimeout(() => abort.abort(), 20); await rejects(() => abortRequest); stallTLS = false;
  const aborted = new AbortController(); aborted.abort(); const beforeAborted = seen.proxyTCP;
  await rejects(() => test.connectTorPublished(scope, aborted.signal)); assert.equal(seen.proxyTCP, beforeAborted); checks++;
  // Unmodified direct default uses global public fetch and no Tor factory.
  observeTLS = false;
  let directNonce;
  globalThis.__directChannelAppraise = async (evidence, nonce, repository, signal, actualManifest) => {
    assert.equal(repository, actualManifest === undefined ? 'ajbt200128/possums'
      : `ajbt200128/possums@${facts.release_tag}@sha256:${sha(actualManifest)}`);
    assert.deepEqual(Buffer.from(nonce), directNonce);
    assert.equal(JSON.parse(Buffer.from(evidence)).nonce, directNonce.toString('hex'));
    assert(!signal?.aborted); return { ...facts };
  };
  const directURLs = [];
  globalThis.fetch = async req => {
    directURLs.push(req.url); assert.equal(req.credentials, 'omit');
    let body, status = 200, headers = {};
    if (req.url.endsWith('/releases/latest')) body = bytes({ tag_name: facts.release_tag });
    else if (req.url.includes('/releases/download/')) { status = 302; headers.location = 'https://release-assets.githubusercontent.com/synthetic'; }
    else if (req.url.endsWith('/synthetic')) body = manifest;
    else if (req.url.includes('/tinfoil-attestation?nonce=')) { directNonce = Buffer.from(req.url.split('nonce=')[1], 'hex'); body = bytes({ nonce: directNonce.toString('hex') }); }
    else if (req.url.endsWith('/tinfoil-certificate')) body = bytes({ certificate: cert });
    else if (req.url.endsWith('/hpke-keys')) body = config;
    else throw new Error('HOSTILE unexpected direct fetch');
    const response = new Response(body, { status, headers }); Object.defineProperty(response, 'url', { value: req.url }); return response;
  };
  const beforeDirect = seen.proxyTCP;
  const direct = await test.connectPublished();
  assert.equal(direct.channel.endpointIdentity.onionIdentity, undefined); assert.equal(seen.proxyTCP, beforeDirect); checks++;
  await rejects(() => test.recoverPublished(direct));
  assert(!directURLs.some(url => url.includes('/attestation/tor'))); checks++;
  const op = new test.Operation(5000);
  try {
    const response = await test.pinnedRequest(new Request(`https://localhost:${server.address().port}/`), op, pin, () => {});
    assert.deepEqual(await response.json(), { ok: true }); assert.equal(seen.proxyTCP, beforeDirect); checks++;
  } finally { op.close(); }
  assert(seen.requests.every(r => !r.url.includes('/v1/chat') && !r.url.includes('/v1/submissions'))); checks++;
  console.log(JSON.stringify({ suite: 'tor-channel', checks, scope: 'actual native helper/local TLS/HTTP; synthetic appraisal only; zero inference, live Tor or hardware-positive claim' }));
} finally {
  globalThis.fetch = directFetch; tls.connect = tlsConnect; dns.lookup = lookup; syncBuiltinESMExports();
  for (const scope of scopes) await scope.close();
  for (const socket of sockets) socket.destroy();
  for (const s of [proxy, server, wrongServer]) { s.closeAllConnections?.(); await new Promise(resolve => s.close(resolve)); }
  delete globalThis.__torChannelAppraise; delete globalThis.__directChannelAppraise;
  if (previousCache === undefined) delete process.env.XDG_CACHE_HOME; else process.env.XDG_CACHE_HOME = previousCache;
  await rm(out, { recursive: true, force: true });
}
