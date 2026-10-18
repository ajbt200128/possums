// Offline synthetic qualification; no inference, credentials or production sockets.
import assert from 'node:assert/strict';
import { readFile, writeFile, mkdir } from 'node:fs/promises';
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
execFileSync('/usr/bin/openssl', ['req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-keyout', path.join(out, 'key.pem'),
  '-out', path.join(out, 'cert.pem'), '-days', '1', '-subj', '/CN=gateway.possums.dev',
  '-addext', 'subjectAltName=DNS:gateway.possums.dev,DNS:localhost'], { stdio: 'ignore' });
const certPEM = await readFile(path.join(out, 'cert.pem'), 'utf8');
const sha = value => createHash('sha256').update(value).digest('hex');
const tls = sha(new X509Certificate(certPEM).publicKey.export({ type: 'spki', format: 'der' }));
const bytes = value => new TextEncoder().encode(JSON.stringify(value));
const config = new Uint8Array([0, 0, 32, ...Array(32).fill(7), 0, 4, 0, 1, 0, 2]);
const yaml = Buffer.from('cvm-version: 0.14.12\ncontainers: []\n');
const manifest = bytes({ hashes: { version: 'v0.14.12' }, config: yaml.toString('base64'), cmdline: `tinfoil-config-hash=${sha(yaml)}` });
const facts = { tls_key_sha256: tls, hpke_key: '07'.repeat(32), release_tag: 'v0.0.99', release_digest: sha(manifest), freshness_expires_at_unix: Math.floor(Date.now() / 1000) + 3600 };
const input = { evidence: bytes({ nonce: 'synthetic' }), nonce: new Uint8Array(32).fill(1), manifest, tag: facts.release_tag, certificate: certPEM, config };
let result = facts, calls = 0, passed = 0;
const production = await readFile(path.resolve(source, '../../../package/extension.mjs'), 'utf8');
assert(!production.includes('pi-public-evidence-v1')); passed++;
globalThis.__v3Appraise = async (evidence, nonce, repository, signal, actualManifest) => {
  calls++;
  assert.equal(repository, `ajbt200128/possums@${input.tag}@sha256:${sha(actualManifest)}`);
  assert(actualManifest instanceof Uint8Array && actualManifest !== manifest);
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
// Real bootstrap routing/nonce ownership, synthetic appraisal only. No disk cache.
const fetch = globalThis.fetch, urls = [];
let certificateBody = { certificate: certPEM };
globalThis.fetch = async request => {
  const url = request.url; urls.push(url);
  assert.equal(request.credentials, 'omit');
  let body, status = 200, headers = {};
  if (url.includes('/releases/latest')) body = bytes({ tag_name: input.tag });
  else if (url.includes('/releases/download/')) { status = 302; headers.location = 'https://release-assets.githubusercontent.com/synthetic'; }
  else if (url === 'https://release-assets.githubusercontent.com/synthetic') body = manifest;
  else if (url.includes('/tinfoil-attestation?nonce=')) { assert.match(url.split('nonce=')[1], /^[0-9a-f]{64}$/); body = input.evidence; }
  else if (url.endsWith('/tinfoil-certificate')) body = bytes(certificateBody);
  else if (url.endsWith('/hpke-keys')) body = config;
  else throw new Error('HOSTILE unexpected fetch');
  const response = new Response(body, { status, headers }); Object.defineProperty(response, 'url', { value: url }); return response;
};
try {
  const first = await test.connectPublished();
  await rejects(() => test.recoverPublished(first));
  const nonces = urls.filter(url => url.includes('?nonce='));
  assert.equal(new Set(nonces).size, 2); assert(!urls.some(url => /amd|attestations\/sha256/.test(url))); passed++;
  result = { ...facts, freshness_expires_at_unix: facts.freshness_expires_at_unix + 100 };
  const freshWitness = await test.recoverPublished(first);
  assert(test.sameV3Endpoint(first.channel.endpointIdentity, freshWitness.channel.endpointIdentity));
  assert(freshWitness.channel.endpointIdentity.freshness > first.channel.endpointIdentity.freshness); passed++;
  await rejects(() => test.recoverPublished(freshWitness));
  result = facts;
  certificateBody = null;
  await assert.rejects(() => test.connectPublished(), error => error.observation?.stage === 'gateway_certificate' &&
    error.observation?.constraint === 'schema' && !error.message.includes('HOSTILE')); passed++;
} finally { globalThis.fetch = fetch; }
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
  await rejects(() => send('/', '00'.repeat(32)), 'certificate');
  assert.equal(requests, 0); passed++;
  const blocked = new test.Operation(5000);
  try {
    await rejects(() => test.pinnedRequest(new Request(url + '/', { method: 'POST', body: 'SYNTHETIC_PRIVATE_BODY',
      headers: { authorization: 'Bearer synthetic' } }), blocked, '00'.repeat(32), () => {}), 'certificate');
    assert.equal(requests, 0); passed++;
  } finally { blocked.close(); }
  await assert.rejects(() => send('/encoding'), error => error.constraint === 'body' && error.status === 503 && !error.message.includes('HOSTILE')); passed++;
  const redirectOp = new test.Operation(5000), beforeRedirect = requests;
  try {
    const response = await test.pinnedRequest(new Request(url + '/redirect'), redirectOp, tls, () => {});
    assert.equal(response.status, 302); await response.body.cancel(); assert.equal(requests, beforeRedirect + 1); passed++;
  } finally { redirectOp.close(); }
  assert.deepEqual(await send('/'), { ok: true }); passed++;
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
