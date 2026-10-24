// Offline synthetic qualification; no inference, credentials or production sockets.
import assert from 'node:assert/strict';
import { readFile, writeFile, mkdir, mkdtemp, rm } from 'node:fs/promises';
import { createHash, X509Certificate } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { createServer } from 'node:https';
import { gzipSync } from 'node:zlib';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
const source = process.env.PHASE02_RELEASE_SOURCE;
assert(source && path.isAbsolute(source));
const { build } = await import(pathToFileURL(path.join(source, 'node_modules/esbuild/lib/main.js')));
const out = path.resolve(source, '../../../checks/v3');
await mkdir(out, { recursive: true });
const cacheRoot = await mkdtemp(path.join(out, 'cache-'));
const previousCache = process.env.XDG_CACHE_HOME;
process.env.XDG_CACHE_HOME = cacheRoot;
execFileSync('/usr/bin/openssl', ['req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-keyout', path.join(out, 'key.pem'),
  '-out', path.join(out, 'cert.pem'), '-days', '1', '-subj', '/CN=gateway.possums.dev',
  '-addext', 'subjectAltName=DNS:gateway.possums.dev,DNS:localhost'], { stdio: 'ignore' });
const certPEM = await readFile(path.join(out, 'cert.pem'), 'utf8');
const sha = value => createHash('sha256').update(value).digest('hex');
const tls = sha(new X509Certificate(certPEM).publicKey.export({ type: 'spki', format: 'der' }));
const bytes = value => new TextEncoder().encode(JSON.stringify(value));
const config = new Uint8Array([0, 0, 32, ...Array(32).fill(7), 0, 4, 0, 1, 0, 2]);
const yaml = Buffer.from('cvm-version: 0.14.16\ncontainers: []\n');
const manifest = bytes({ hashes: { version: 'v0.14.16' }, config: yaml.toString('base64'), cmdline: `tinfoil-config-hash=${sha(yaml)}` });
const facts = { tls_key_sha256: tls, hpke_key: '07'.repeat(32), release_tag: 'v0.0.99', release_digest: sha(manifest), freshness_expires_at_unix: Math.floor(Date.now() / 1000) + 3600 };
const input = { evidence: bytes({ nonce: 'synthetic' }), nonce: new Uint8Array(32).fill(1), manifest, tag: facts.release_tag, certificate: certPEM, config };
let result = facts, calls = 0, passed = 0;
const production = await readFile(path.resolve(source, '../../../package/extension.mjs'), 'utf8');
assert(production.includes('pi-public-manifest-v3')); passed++;
globalThis.__v3Appraise = async (evidence, nonce, repository, signal, actualManifest) => {
  calls++;
  if (actualManifest === undefined) assert.equal(repository, 'ajbt200128/possums');
  else {
    assert.equal(repository, `ajbt200128/possums@${input.tag}@sha256:${sha(actualManifest)}`);
    assert(actualManifest instanceof Uint8Array && actualManifest !== manifest);
  }
  assert.equal(nonce.length, 32);
  if (signal?.aborted) throw new Error('HOSTILE cancelled');
  return { ...result };
};
await build({ stdin: { contents: `export { Channel, ReferenceClient } from '../../examples/phase01/transport.ts';
export { connectPublished, recoverPublished } from './bootstrap.ts';
export { pinnedRequest, sameV3Endpoint } from './v3.ts';
export { Operation, DiagnosticFailure } from '../../examples/phase01/limits.ts';`, resolveDir: source, loader: 'ts' },
  outfile: path.join(out, 'synthetic.mjs'), bundle: true, platform: 'node', format: 'esm', target: 'node24',
  define: { __PHASE01_FIXTURE__: 'false' }, plugins: [{ name: 'synthetic-v3-appraisal-only', setup(build) {
    build.onResolve({ filter: /^\.\/verifier\.mjs$/ }, () => ({ path: 'bridge', namespace: 'synthetic' }));
    build.onLoad({ filter: /.*/, namespace: 'synthetic' }, () => ({ contents:
      `export class VerifierFailure extends Error {};
       export const verifyV3 = (...args) => globalThis.__v3Appraise(...args);
       export const verifyTorV3 = () => { throw new Error('unexpected_tor_appraisal_in_direct_fixture'); };`, loader: 'js' }));
  } }] });
const test = await import(pathToFileURL(path.join(out, 'synthetic.mjs')));
async function rejects(fn, constraint) {
  await assert.rejects(fn, error => { assert(!error.message.includes('HOSTILE')); if (constraint) assert.equal(error.constraint, constraint); return true; }); passed++;
}
const channel = await test.Channel.v3(input);
assert.equal(channel.release.expires, facts.freshness_expires_at_unix * 1000); passed++;
// The reviewed upgrade must not remove compatibility with the prior runtime.
const priorYaml = Buffer.from('cvm-version: 0.14.12\ncontainers: []\n');
const priorManifest = bytes({ hashes: { version: 'v0.14.12' }, config: priorYaml.toString('base64'),
  cmdline: `tinfoil-config-hash=${sha(priorYaml)}` });
result = { ...facts, release_digest: sha(priorManifest) };
assert.equal((await test.Channel.v3({ ...input, manifest: priorManifest })).release.tag, facts.release_tag); passed++;
result = facts;
for (const version of ['v0.14.11', 'v0.14.13', 'v0.14.15', 'v0.14.17', 'v0.99.0', '0.14.16',
  'v0.14.160', 'v0.14.16-rc.1', 'v0.14.16\n', 'HOSTILE credential history URL', null, 16, {}, ['v0.14.16'], undefined]) {
  const before = calls;
  await rejects(() => test.Channel.v3({ ...input, manifest: bytes({ ...JSON.parse(Buffer.from(manifest)), hashes: { version } }) }), 'manifest');
  assert.equal(calls, before, 'unsupported CVM versions fail before SDK appraisal');
}
await rejects(() => test.Channel.v3({ ...input, config: config.slice(0, 40) }), 'key_config');
const wrongConfig = new Uint8Array(config); wrongConfig[3]++;
await rejects(() => test.Channel.v3({ ...input, config: wrongConfig }), 'key_config');
for (const update of [{ hpke_key: '08'.repeat(32) }, { tls_key_sha256: '00'.repeat(32) },
  { release_tag: 'v0.0.98' }, { release_digest: '00'.repeat(32) }, { freshness_expires_at_unix: 1 }]) {
  result = { ...facts, ...update }; await rejects(() => test.Channel.v3(input));
}
result = facts;
for (const update of [{ hashes: { version: 'v0.14.11' } }, { cmdline: 'tinfoil-config-hash=wrong' },
  { cmdline: `tinfoil-config-hash=${sha(yaml)} tinfoil-config-hash=${sha(yaml)}` }]) {
  await rejects(() => test.Channel.v3({ ...input, manifest: bytes({ ...JSON.parse(Buffer.from(manifest)), ...update }) }), 'manifest');
}
await rejects(() => test.Channel.v3({ ...input, certificate: 'HOSTILE bad certificate' }), 'certificate');
await rejects(() => test.Channel.v3({ ...input, nonce: new Uint8Array(31) }), 'schema');
await rejects(() => test.Channel.v3({ ...input, evidence: new Uint8Array(16 * 1024 * 1024 + 1) }), 'schema');
const controller = new AbortController(); controller.abort();
await rejects(() => test.Channel.v3(input, controller.signal));
const same = await test.Channel.v3({ ...input, evidence: bytes({ nonce: 'different report bytes' }) });
assert(test.sameV3Endpoint(channel.endpointIdentity, same.endpointIdentity)); passed++;
assert.throws(() => same.requireRenewal(channel), error => error.constraint === 'policy'); passed++;
result = { ...facts, freshness_expires_at_unix: facts.freshness_expires_at_unix + 100 };
const renewed = await test.Channel.v3(input); renewed.requireRenewal(channel); passed++;
result = facts;
// Real bootstrap/cache/nonce ownership, synthetic appraisal only. GitHub's
// rate-limited API must never be used, on either cold or warm acquisition.
const fetch = globalThis.fetch, urls = [];
let certificateBody = { certificate: certPEM }, manifestStatus = 302;
globalThis.fetch = async request => {
  const url = request.url; urls.push(url);
  assert.equal(request.credentials, 'omit');
  let body, status = 200, headers = {};
  if (url.startsWith('https://api.github.com/')) { status = 403; headers['x-ratelimit-remaining'] = '0'; body = bytes({ message: 'HOSTILE rate limit' }); }
  else if (url.includes('/releases/download/')) {
    status = manifestStatus; headers.location = 'https://release-assets.githubusercontent.com/synthetic';
  }
  else if (url === 'https://release-assets.githubusercontent.com/synthetic') body = manifest;
  else if (url.includes('/tinfoil-attestation?nonce=')) { assert.match(url.split('nonce=')[1], /^[0-9a-f]{64}$/); body = input.evidence; }
  else if (url.endsWith('/tinfoil-certificate')) body = bytes(certificateBody);
  else if (url.endsWith('/hpke-keys')) body = config;
  else throw new Error('HOSTILE unexpected fetch');
  const response = new Response(body, { status, headers }); Object.defineProperty(response, 'url', { value: url }); return response;
};
try {
  const first = await test.connectPublished();
  assert.equal(urls.filter(url => url.includes('/releases/download/')).length, 1);
  manifestStatus = 429;
  // A fresh trust session independently appraises fresh evidence and the cached
  // manifest even while GitHub artifact acquisition would return HTTP 429.
  const second = await test.connectPublished();
  assert(test.sameV3Endpoint(first.channel.endpointIdentity, second.channel.endpointIdentity));
  assert.equal(urls.filter(url => url.includes('/releases/download/')).length, 1); passed++;
  await rejects(() => test.recoverPublished(first));
  const nonces = urls.filter(url => url.includes('?nonce='));
  assert.equal(new Set(nonces).size, 3); assert(!urls.some(url => /api\.github\.com|amd|attestations\/sha256/.test(url))); passed++;
  result = { ...facts, freshness_expires_at_unix: facts.freshness_expires_at_unix + 100 };
  const freshWitness = await test.recoverPublished(first);
  assert(test.sameV3Endpoint(first.channel.endpointIdentity, freshWitness.channel.endpointIdentity));
  assert(freshWitness.channel.endpointIdentity.freshness > first.channel.endpointIdentity.freshness); passed++;
  await rejects(() => test.recoverPublished(freshWitness));
  result = facts;
  certificateBody = null;
  await assert.rejects(() => test.connectPublished(), error => error.observation?.stage === 'gateway_certificate' &&
    error.observation?.constraint === 'schema' && !error.message.includes('HOSTILE')); passed++;
  certificateBody = { certificate: certPEM };
  // A cold cache still needs exact public bytes; rate limiting is classified,
  // never converted into a channel or an older-release fallback.
  await rm(path.join(cacheRoot, 'possums', 'pi-public-manifest-v3'), { recursive: true });
  for (const [status, constraint] of [[429, 'rate_limited'], [503, 'http'], [200, 'redirect'], [307, 'redirect']]) {
    manifestStatus = status;
    const before = calls;
    await assert.rejects(() => test.connectPublished(), error => error.observation?.stage === 'manifest_redirect' &&
      error.observation?.constraint === constraint && error.observation?.status === status);
    assert.equal(calls, before + 1, 'failed acquisition never reaches complete channel appraisal'); passed++;
  }
  assert(!urls.some(url => /api\.github\.com|amd/.test(url))); passed++;
} finally {
  globalThis.fetch = fetch;
  if (previousCache === undefined) delete process.env.XDG_CACHE_HOME; else process.env.XDG_CACHE_HOME = previousCache;
  await rm(cacheRoot, { recursive: true, force: true });
}
// Actual native TLS + HTTPS request, endorsed self-signed leaf, headers withheld
// on a wrong SPKI. This does not simulate hardware/Sigstore acceptance.
let requests = 0, streamClosed;
const closed = new Promise(resolve => { streamClosed = resolve; });
const server = createServer({ key: await readFile(path.join(out, 'key.pem')), cert: certPEM }, (req, res) => {
  requests++;
  if (req.url === '/slow') return;
  if (req.url === '/encoding') { res.writeHead(503, { 'content-encoding': 'HOSTILE_UNSUPPORTED' }); res.end(); return; }
  if (req.url === '/redirect') { res.writeHead(302, { location: '/not-followed' }); res.end(); return; }
  if (req.url === '/stream') { req.once('close', streamClosed); res.writeHead(200); res.write('synthetic'); return; }
  res.writeHead(200, { 'content-encoding': 'gzip', 'content-type': 'application/json' });
  res.end(gzipSync(Buffer.from('{"ok":true}')));
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
const url = `https://localhost:${server.address().port}`;
async function send(pathname, pin = tls, signal, check = () => {}) {
  const op = new test.Operation(5000, signal);
  try {
    const response = await test.pinnedRequest(new Request(url + pathname, { headers: { authorization: 'Bearer synthetic' } }), op, pin, check);
    return await response.json();
  } finally { op.close(); }
}
try {
  await rejects(() => send('/', '00'.repeat(32)), 'peer_key_mismatch');
  assert.equal(requests, 0); passed++;
  const blocked = new test.Operation(5000);
  try {
    await rejects(() => test.pinnedRequest(new Request(url + '/', { method: 'POST', body: 'SYNTHETIC_PRIVATE_BODY',
      headers: { authorization: 'Bearer synthetic' } }), blocked, '00'.repeat(32), () => {}), 'peer_key_mismatch');
    assert.equal(requests, 0); passed++;
  } finally { blocked.close(); }
  await assert.rejects(() => send('/encoding'), error => error.constraint === 'body' && error.status === 503 && !error.message.includes('HOSTILE')); passed++;
  const redirectOp = new test.Operation(5000), beforeRedirect = requests;
  try {
    const response = await test.pinnedRequest(new Request(url + '/redirect'), redirectOp, tls, () => {});
    assert.equal(response.status, 302); await response.body.cancel(); assert.equal(requests, beforeRedirect + 1); passed++;
  } finally { redirectOp.close(); }
  assert.deepEqual(await send('/'), { ok: true }); passed++;
  // Same address, new endpoint key: an old live trust context fails before
  // HTTP, while an independently supplied new pin can reach the same server.
  // The new pin here is synthetic, not evidence of public attestation success.
  execFileSync('/usr/bin/openssl', ['req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-keyout', path.join(out, 'rotated-key.pem'),
    '-out', path.join(out, 'rotated-cert.pem'), '-days', '1', '-subj', '/CN=localhost',
    '-addext', 'subjectAltName=DNS:localhost'], { stdio: 'ignore' });
  const rotatedCert = await readFile(path.join(out, 'rotated-cert.pem'), 'utf8');
  const rotatedPin = sha(new X509Certificate(rotatedCert).publicKey.export({ type: 'spki', format: 'der' }));
  assert.notEqual(rotatedPin, tls);
  server.setSecureContext({ key: await readFile(path.join(out, 'rotated-key.pem')), cert: rotatedCert });
  const beforeRotation = requests;
  await rejects(() => send('/'), 'peer_key_mismatch');
  assert.equal(requests, beforeRotation); passed++;
  // Invalid dates take precedence over a key mismatch, never nominate recovery.
  const now = Date.now, rotated = new X509Certificate(rotatedCert);
  try {
    for (const date of [Date.parse(rotated.validFrom) - 1, Date.parse(rotated.validTo)]) {
      Date.now = () => date;
      await rejects(() => send('/'), 'certificate');
      assert.equal(requests, beforeRotation); passed++;
    }
    Date.now = () => { throw new Error('HOSTILE date failure'); };
    await rejects(() => send('/'), 'certificate');
    assert.equal(requests, beforeRotation); passed++;
  } finally { Date.now = now; }
  assert.deepEqual(await send('/', rotatedPin), { ok: true });
  assert.equal(requests, beforeRotation + 1); passed++;
  execFileSync('/usr/bin/openssl', ['req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-keyout', path.join(out, 'wrong-host-key.pem'),
    '-out', path.join(out, 'wrong-host-cert.pem'), '-days', '1', '-subj', '/CN=wrong.invalid',
    '-addext', 'subjectAltName=DNS:wrong.invalid'], { stdio: 'ignore' });
  server.setSecureContext({ key: await readFile(path.join(out, 'wrong-host-key.pem')), cert: await readFile(path.join(out, 'wrong-host-cert.pem')) });
  const beforeWrongHost = requests;
  await rejects(() => send('/'), 'certificate');
  assert.equal(requests, beforeWrongHost); passed++;
  server.setSecureContext({ key: await readFile(path.join(out, 'key.pem')), cert: certPEM });
  assert.deepEqual(await send('/'), { ok: true }); passed++;
  const cancelled = new test.Operation(5000);
  const response = await test.pinnedRequest(new Request(url + '/stream'), cancelled, tls, () => {});
  await response.body.cancel(); await closed; cancelled.close(); passed++;
  const abort = new AbortController(); const pending = send('/slow', tls, abort.signal); setTimeout(() => abort.abort(), 20);
  await rejects(() => pending); const before = requests;
  await rejects(() => send('/', tls, undefined, () => { throw new test.DiagnosticFailure('trust', 'expired'); }), 'expired');
  assert.equal(requests, before); passed++;
} finally { server.closeAllConnections(); await new Promise(resolve => server.close(resolve)); }
console.log(JSON.stringify({ suite: 'pi-v3-synthetic', passed, appraisals: calls,
  scope: 'synthetic appraisal/nonce/bootstrap/renewal and real loopback TLS; not positive hardware or host/inference qualification' }));
