// Synthetic/local only. Run after build.sh:
// PHASE02_RELEASE_SOURCE=/tmp/possums-pi-build-XXXXXX/source/clients/pi node tests/phase02_release.mjs
// Builds private test bundles, never installs into Pi. AMD hardware verification,
// DSSE/Rekor crypto and X509 parsing are mocked ONLY in the isolated test bundle.
// Actual SDK verifyBundle orchestration, signed-subject/tag/measurement checks,
// SAN decoding, Sigstore policy helpers, expiry, SHA-256, gzip bounds and EHBP
// public-config decoding remain real. The production build has no mock hooks.
import assert from 'node:assert/strict';
import { mkdir, readFile, writeFile, rm } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { gzipSync } from 'node:zlib';
import { pathToFileURL } from 'node:url';
import path from 'node:path';
const source = process.env.PHASE02_RELEASE_SOURCE;
assert(source && path.isAbsolute(source) && /possums-pi-build-[^/]+\/source\/clients\/pi$/.test(source));
assert.equal(process.version, 'v24.13.0');
const { build } = await import(pathToFileURL(path.join(source, 'node_modules/esbuild/lib/main.js')));
const output = path.resolve(source, '../../../checks/release');
await mkdir(output, { recursive: true });
const entry = `
export { connect, connectPublished, recoverPublished, recoverCompiled } from './bootstrap.ts';
export { evidenceDirectory } from './evidence-cache.ts';
export { Channel, ReferenceClient, API_APPROVALS } from '../../examples/phase01/transport.ts';
export { PUBLISHER, qualifyPublished, requireReleaseTag } from '../../examples/phase01/approval.ts';
export { X509Certificate, SigstoreVerifier } from '@freedomofpress/sigstore-browser';
export { CertificateChain } from './node_modules/@tinfoilsh/verifier/dist/sev/cert-chain.js';
export { ReportSigner } from './node_modules/@tinfoilsh/verifier/dist/sev/constants.js';
`;
for (const fixture of [false, true]) {
  await build({ stdin: { contents: entry, resolveDir: source, loader: 'ts' },
    outfile: path.join(output, fixture ? 'synthetic.mjs' : 'production.mjs'),
    bundle: true, platform: 'node', format: 'esm', target: 'node24',
    define: { __PHASE01_FIXTURE__: 'false' }, // Even synthetic release tests cannot use Channel.fixture.
    plugins: fixture ? [{ name: 'isolated-hardware-fixture', setup(build) {
      build.onResolve({ filter: /^\.\/attestation\.js$/ }, args => {
        if (args.importer.endsWith('/@tinfoilsh/verifier/dist/client.js')) return { path: 'hardware', namespace: 'release-test' };
      });
      build.onLoad({ filter: /.*/, namespace: 'release-test' }, () => ({
        contents: 'export async function verifyAttestation(...args) { return globalThis.__possumsReleaseTestHardware(...args); }', loader: 'js',
      }));
    } }] : [],
  });
}
const prod = await import(pathToFileURL(path.join(output, 'production.mjs')));
const test = await import(pathToFileURL(path.join(output, 'synthetic.mjs')));
assert(!(await readFile(path.join(output, 'production.mjs'), 'utf8')).includes('__possumsReleaseTestHardware'));
const bytes = value => new TextEncoder().encode(JSON.stringify(value));
const sha = value => createHash('sha256').update(value).digest('hex');
const b64 = value => Buffer.from(value).toString('base64');
const host = 'possum-phase0.possums.containers.tinfoil.dev';
const repo = 'ajbt200128/possums';
const origin = `https://${host}`;
const tag = 'v0.0.99';
const workflow = `https://github.com/${repo}/.github/workflows/tinfoil-release-publish.yml@refs/tags/${tag}`;
const key = new Uint8Array([0, 0, 32, ...Array(32).fill(7), 0, 4, 0, 1, 0, 2]);
const config = Buffer.from(`cvm-version: 0.14.12\ncontainers:\n  - name: gateway\n    image: ghcr.io/${repo}-gateway@sha256:${'a'.repeat(64)}\n`);
const manifest = { hashes: { version: 'v0.14.12' }, config: b64(config),
  cmdline: `readonly=on tinfoil-config-hash=${sha(config)}`, snp_measurement: 'a'.repeat(96) };
const doc = { format: 'https://tinfoil.sh/predicate/sev-snp-guest/v2', body: b64(gzipSync(Buffer.alloc(1184))) };
const base = {
  domain: host, releaseTag: tag, digest: sha(bytes(manifest)), enclaveAttestationReport: doc,
  enclaveCert: 'isolated fixture certificate', vcek: b64(Buffer.from([1])),
  sigstoreBundle: { dsseEnvelope: { payloadType: 'application/vnd.in-toto+json',
    payload: b64(bytes({ _type: 'https://in-toto.io/Statement/v1', subject: [{ name: 'tinfoil-deployment.json', digest: { sha256: sha(bytes(manifest)) } }],
      predicateType: 'https://tinfoil.sh/predicate/snp-tdx-multiplatform/v1', predicate: manifest })),
    signatures: [{ sig: 'fixture-signature' }] },
    verificationMaterial: { certificate: { rawBytes: 'AQ==' }, tlogEntries: [{ inclusionProof: { hashes: [] } }] } },
};
const publicKey = new Uint8Array([1, 2, 3]);
let now = Date.parse('2030-10-12T00:00:00Z');
const NativeDate = Date;
globalThis.Date = class extends NativeDate { constructor(...args) { super(...(args.length ? args : [now])); } static now() { return now; } };
let state, hardwareCalls = 0, dsseCalls = 0, network = 0, passed = 0;
const originalFetch = globalThis.fetch;
const originalParse = test.X509Certificate.parse;
const originalDSSE = test.SigstoreVerifier.prototype.verifyDsse;
const forbidden = async () => { network++; throw new Error('HOSTILE network forbidden'); };
globalThis.fetch = forbidden;
// Fixture-only certificate construction. The SDK still parses ASN.1 SAN bytes
// and decodes its dcode fields; this is not production certificate parsing.
function base32(input) {
  let buffer = 0, bits = 0, result = '';
  for (const byte of input) { buffer = (buffer << 8) | byte; bits += 8;
    while (bits >= 5) { bits -= 5; result += 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567'[(buffer >>> bits) & 31]; }
  }
  if (bits) result += 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567'[(buffer << (5 - bits)) & 31];
  return result;
}
function domains(value, prefix) {
  return base32(value).match(/.{1,50}/g).map((chunk, i) => String(i).padStart(2, '0') + chunk + '.' + prefix + '.fixture');
}
function der(tag, value) {
  const size = value.length;
  return Buffer.concat([Buffer.from([tag, ...(size < 128 ? [size] : [0x82, size >> 8, size & 255])]), value]);
}
function reset() {
  const signer = {
    extSubjectAltName: { uri: workflow, otherName: () => undefined },
    extFulcioIssuerV1: { issuer: 'https://token.actions.githubusercontent.com' },
    extGitHubWorkflowRepository: { workflowRepository: repo }, extGitHubWorkflowRef: { workflowRef: `refs/tags/${tag}` },
    extGitHubWorkflowSHA: { workflowSHA: 'b'.repeat(40) },
    extSourceRepositoryURI: { sourceRepositoryURI: `https://github.com/${repo}` },
    extSourceRepositoryDigest: { sourceRepositoryDigest: 'b'.repeat(40) },
    extSourceRepositoryRef: { sourceRepositoryRef: `refs/tags/${tag}` },
    extBuildConfigURI: { buildConfigURI: workflow }, extBuildConfigDigest: { buildConfigDigest: 'b'.repeat(40) },
    extBuildSignerURI: { buildSignerURI: workflow }, extBuildSignerDigest: { buildSignerDigest: 'b'.repeat(40) },
    extRunInvocationURI: { runInvocationURI: `https://github.com/${repo}/actions/runs/123/attempts/1` },
  };
  state = { signer, notAfter: now + 24 * 60 * 60 * 1000, notBefore: now - 1000, hardwareSignature: true,
    measurement: manifest.snp_measurement, hpkeKey: '07'.repeat(32), tlsFingerprint: sha(publicKey),
    sanDomain: host, sanKey: Buffer.alloc(32, 7), sanHash: sha(doc.format + doc.body) };
}
reset();
globalThis.__possumsReleaseTestHardware = async (report, vcek) => {
  hardwareCalls++;
  assert.equal(report.format, doc.format); assert.equal(report.body, doc.body);
  if (vcek !== base.vcek) throw new Error('HOSTILE wrong VCEK DER');
  if (!state.hardwareSignature) throw new Error('HOSTILE hardware signature');
  return { measurement: { type: doc.format, registers: [state.measurement] }, hpkePublicKey: state.hpkeKey, tlsPublicKeyFingerprint: state.tlsFingerprint };
};
test.X509Certificate.parse = value => {
  if (typeof value !== 'string') return state.signer;
  return { publicKey, notBefore: new Date(state.notBefore), notAfter: new Date(state.notAfter),
    validForDate: test.X509Certificate.prototype.validForDate,
    extension(oid) {
      assert.equal(oid, '2.5.29.17');
      const sans = [state.sanDomain, ...domains(state.sanKey, 'hpke'), ...domains(Buffer.from(state.sanHash), 'hatt')];
      return { value: der(0x30, Buffer.concat(sans.map(san => der(0x82, Buffer.from(san))))) };
    },
  };
};
test.SigstoreVerifier.prototype.verifyDsse = async function(bundle, policy) {
  dsseCalls++;
  if (bundle.dsseEnvelope.signatures[0].sig !== 'fixture-signature') throw new Error('HOSTILE DSSE/Rekor signature');
  await policy.verify(state.signer); // Actual library repository/tag policy, never a verified boolean.
  return { payloadType: bundle.dsseEnvelope.payloadType, payload: Buffer.from(bundle.dsseEnvelope.payload, 'base64') };
};
function statement(bundle, mutate) {
  const value = JSON.parse(Buffer.from(bundle.sigstoreBundle.dsseEnvelope.payload, 'base64'));
  mutate(value); bundle.sigstoreBundle.dsseEnvelope.payload = b64(bytes(value));
}
async function rejects(name, fn, code = 'rejected') {
  await assert.rejects(fn, error => {
    assert.equal(error.code, code, name); assert(!error.message.includes('HOSTILE'), name);
    assert.equal(error.cause, undefined, name); return true;
  }, name); passed++;
}
const qualify = (b = base, m = bytes(manifest), k = key) => test.Channel.published(bytes(b), m, k);
// Actual pinned CertificateChain methods, with synthetic certificate objects.
// Signature results are controlled: these are NOT real AMD cryptography tests.
async function certificateChainChecks() {
  const fixtureParse = test.X509Certificate.parse;
  const hwid = new Uint8Array(64).fill(7);
  const tcb = { blSpl: 1, teeSpl: 2, snpSpl: 3, ucodeSpl: 4 };
  const extensions = new Map([
    ['1.3.6.1.4.1.3704.1.4', hwid],
    ['1.3.6.1.4.1.3704.1.2', new Uint8Array([0x16, 5, ...Buffer.from('Genoa')])],
    ...['1', '2', '3', '8'].map((suffix, i) => [`1.3.6.1.4.1.3704.1.3.${suffix}`, new Uint8Array([2, 1, i + 1])]),
  ]);
  const location = new Map([['C', 'US'], ['L', 'Santa Clara'], ['ST', 'CA'], ['O', 'Advanced Micro Devices'], ['OU', 'Engineering']]);
  const edges = [];
  function certificate(name) {
    return { version: 'v3', issuerDN: location, subjectDN: new Map([...location, ['CN', name]]),
      notBefore: new Date(now - 1000), notAfter: new Date(now + 1000),
      validForDate: test.X509Certificate.prototype.validForDate, signatureValid: true,
      async verify(issuer) { edges.push([this, issuer]); return this.signatureValid; },
      extension(oid) { const value = extensions.get(oid); return value && { value }; },
      root: { subs: [{ subs: Array.from({ length: 7 }, (_, i) => i === 6
        ? { subs: [{ subs: [{ toOID: () => '1.2.840.10045.2.1' }, { toOID: () => '1.3.132.0.34' }] }] } : {}) },
        { subs: [{ toOID: () => '1.2.840.113549.1.1.10' }] }] },
    };
  }
  const certs = [certificate('ARK-Genoa'), certificate('SEV-Genoa'), certificate('SEV-VCEK')];
  const report = { productName: 'Genoa', signerInfoParsed: { signingKey: test.ReportSigner.VcekReportSigner } };
  const supplied = new Uint8Array([1, 2, 3]), parsed = [];
  try {
    test.X509Certificate.parse = value => { parsed.push(value); return certs[parsed.length - 1]; };
    const chain = await test.CertificateChain.fromReport(report, supplied);
    assert.equal(parsed.length, 3); assert.equal(parsed[2], supplied);
    assert(parsed.slice(0, 2).every(value => typeof value === 'string' && value.startsWith('-----BEGIN CERTIFICATE-----')));
    assert.equal(await chain.verifyChain(), true);
    assert.deepEqual(edges, [[certs[0], undefined], [certs[1], certs[0]], [certs[2], certs[1]]]); passed++;
    for (const cert of certs) {
      for (const field of ['notBefore', 'notAfter']) {
        const saved = cert[field]; cert[field] = new Date(field === 'notBefore' ? now + 1 : now - 1);
        await assert.rejects(() => chain.verifyChain()); cert[field] = saved; passed++;
      }
      cert.signatureValid = false; await assert.rejects(() => chain.verifyChain()); cert.signatureValid = true; passed++;
    }
    chain.validateVcekTcb(tcb); chain.validateVcekHwid(hwid.slice()); passed++;
    for (const field of Object.keys(tcb)) {
      assert.throws(() => chain.validateVcekTcb({ ...tcb, [field]: 99 })); passed++;
    }
    for (const oid of ['1', '2', '3', '8'].map(suffix => `1.3.6.1.4.1.3704.1.3.${suffix}`)) {
      const saved = extensions.get(oid); extensions.delete(oid);
      assert.throws(() => chain.validateVcekTcb(tcb)); extensions.set(oid, saved); passed++;
    }
    assert.throws(() => chain.validateVcekHwid(new Uint8Array(64).fill(8))); passed++;
    extensions.delete('1.3.6.1.4.1.3704.1.4');
    assert.throws(() => chain.validateVcekHwid(hwid)); assert.throws(() => chain.validateVcekFormat()); passed++;
    extensions.set('1.3.6.1.4.1.3704.1.4', new Uint8Array(63));
    assert.throws(() => chain.validateVcekFormat()); passed++;
    await assert.rejects(() => test.CertificateChain.fromReport({ ...report, productName: 'Milan' }, supplied));
    await assert.rejects(() => test.CertificateChain.fromReport({ ...report, signerInfoParsed: { signingKey: test.ReportSigner.VlekReportSigner } }, supplied));
    assert.equal(parsed.length, 3); passed++;
    // Real X509 parser and embedded ARK/ASK, but deliberately invalid VCEK DER.
    test.X509Certificate.parse = originalParse;
    await assert.rejects(() => test.CertificateChain.fromReport(report, new Uint8Array([0]))); passed++;
  } finally { test.X509Certificate.parse = fixtureParse; }
}
try {
  await certificateChainChecks();
  assert.equal(prod.API_APPROVALS[0].expires, NativeDate.parse('2026-10-11T00:00:00Z'));
  await rejects('compiled admin expiry unchanged', () => prod.Channel.api(bytes(base), bytes(manifest), key));
  await rejects('real hardware crypto rejects synthetic report', () => prod.Channel.published(bytes(base), bytes(manifest), key));
  await rejects('production fixture disabled', () => prod.Channel.fixture('https://localhost:18443', key, '07'.repeat(32)));
  assert.throws(() => new prod.Channel(Symbol(), origin, {})); passed++;
  assert.throws(() => new prod.ReferenceClient({ verified: true, release: { tag, expires: Infinity } })); passed++;
  for (const bad of ['v01.2.3', 'main', 'v1.2.3/../../x', 'https://HOSTILE', 'v1.2.3\n', 'v1.2.3-rc.1', null]) {
    assert.throws(() => prod.requireReleaseTag(bad)); passed++;
  }
  const calls = network;
  let channel = await qualify();
  assert.equal(network, calls); assert(hardwareCalls > 0 && dsseCalls > 0);
  assert.deepEqual(channel.release, { tag, expires: now + 43200000 });
  assert(Object.isFrozen(channel.release));
  assert.throws(() => { channel.release.expires = Infinity; });
  assert.throws(() => { channel.release = { tag, expires: Infinity }; });
  const client = new test.ReferenceClient(channel), fresh = client.freshSession();
  assert.notEqual(client, fresh); assert.equal(client.channel, fresh.channel); assert.equal(client.release, fresh.release);
  await rejects('fresh auth wrapper has no bearer', () => fresh.models());
  passed++;
  // Snapshots are made before the first await, including the EHBP key bytes.
  const bb = bytes(base), mm = bytes(manifest), kk = key.slice();
  const pending = test.Channel.published(bb, mm, kk);
  bb.fill(0); mm.fill(0); kk.fill(0);
  assert.equal((await pending).release.tag, tag); passed++;
  for (const [name, mutate] of [
    ['raw manifest hash', b => { b.digest = '0'.repeat(64); }],
    ['domain', b => { b.domain = 'HOSTILE.invalid'; }],
    ['tag', b => { b.releaseTag = 'v0.0.100'; }],
    ['report format', b => { b.enclaveAttestationReport.format = 'https://tinfoil.sh/predicate/dummy/v2'; }],
    ['gzip bomb', b => { b.enclaveAttestationReport.body = b64(gzipSync(Buffer.alloc(1024 * 1024))); }],
    ['signature', b => { b.sigstoreBundle.dsseEnvelope.signatures[0].sig = 'HOSTILE'; }],
    ['subject digest', b => statement(b, s => { s.subject[0].digest.sha256 = '0'.repeat(64); })],
    ['subject name', b => statement(b, s => { s.subject[0].name = 'HOSTILE'; })],
    ['predicate', b => statement(b, s => { s.predicate.config = 'HOSTILE'; })],
    ['predicate type', b => statement(b, s => { s.predicateType = 'HOSTILE'; })],
    ['payload type', b => { b.sigstoreBundle.dsseEnvelope.payloadType = 'HOSTILE'; }],
    ['extra subject', b => statement(b, s => { s.subject.push(s.subject[0]); })],
    ['multiple signatures', b => { b.sigstoreBundle.dsseEnvelope.signatures.push({ sig: 'fixture-signature' }); }],
    ['oversized proof', b => { b.sigstoreBundle.verificationMaterial.tlogEntries[0].inclusionProof.hashes = Array(65).fill('x'); }],
  ]) { reset(); const bad = structuredClone(base); mutate(bad); await rejects(name, () => qualify(bad)); }
  await rejects('exact raw bytes, not JSON equivalence', () => qualify(base, Buffer.from(JSON.stringify(manifest, null, 2))));
  for (const [name, change] of [
    ['config hash binding', m => { m.cmdline = 'tinfoil-config-hash=' + '0'.repeat(64); }],
    ['duplicate config hash', m => { m.cmdline += ' tinfoil-config-hash=' + sha(config); }],
    ['CVM version', m => { m.hashes.version = 'v0.99.0'; }],
  ]) {
    const m = structuredClone(manifest); change(m); const b = structuredClone(base); b.digest = sha(bytes(m));
    statement(b, s => { s.predicate = m; s.subject[0].digest.sha256 = b.digest; });
    await rejects(name, () => qualify(b, bytes(m)));
  }
  for (const [name, mutate] of [
    ['hardware signature', s => { s.hardwareSignature = false; }],
    ['measurement', s => { s.measurement = 'c'.repeat(96); }],
    ['TLS key', s => { s.tlsFingerprint = '0'.repeat(64); }],
    ['HPKE endorsement', s => { s.hpkeKey = '08'.repeat(32); }],
    ['certificate SAN domain', s => { s.sanDomain = 'HOSTILE.invalid'; }],
    ['certificate SAN key', s => { s.sanKey = Buffer.alloc(32, 8); }],
    ['certificate SAN report', s => { s.sanHash = '0'.repeat(64); }],
    ['certificate not yet valid', s => { s.notBefore = now + 1000; }],
    ['certificate expired', s => { s.notAfter = now - 1; }],
    ['certificate nonfinite expiry', s => { s.notAfter = NaN; }],
  ]) { reset(); mutate(state); await rejects(name, () => qualify()); }
  // Every required identity extension must be present AND coherent. Policies are
  // real sigstore-browser helpers, although the certificate parsing is mocked.
  for (const [extension, field] of [
    ['extGitHubWorkflowRepository', 'workflowRepository'], ['extGitHubWorkflowRef', 'workflowRef'],
    ['extGitHubWorkflowSHA', 'workflowSHA'], ['extSourceRepositoryURI', 'sourceRepositoryURI'],
    ['extSourceRepositoryDigest', 'sourceRepositoryDigest'], ['extSourceRepositoryRef', 'sourceRepositoryRef'],
    ['extBuildConfigURI', 'buildConfigURI'], ['extBuildConfigDigest', 'buildConfigDigest'],
    ['extBuildSignerURI', 'buildSignerURI'], ['extBuildSignerDigest', 'buildSignerDigest'],
    ['extRunInvocationURI', 'runInvocationURI'], ['extFulcioIssuerV1', 'issuer'],
  ]) {
    reset(); delete state.signer[extension]; await rejects('missing ' + extension, () => qualify());
    reset(); state.signer[extension][field] = 'HOSTILE'; await rejects('wrong ' + extension, () => qualify());
  }
  reset(); state.signer.extSubjectAltName.uri = workflow.replace('tinfoil-release-publish.yml', 'other.yml');
  await rejects('exact publisher workflow SAN', () => qualify());
  reset(); state.signer.extSourceRepositoryDigest.sourceRepositoryDigest = 'c'.repeat(40);
  await rejects('well formed but incoherent source SHA', () => qualify());
  for (const invocation of [`https://github.com/other/repo/actions/runs/123/attempts/1`,
    `https://githubXcom/${repo}/actions/runs/123/attempts/1`, `https://github.com/${repo}/actions/runs/123/attempts/1?x=1`]) {
    reset(); state.signer.extRunInvocationURI.runInvocationURI = invocation; await rejects('invocation authority/shape', () => qualify());
  }
  reset(); const mixed = key.slice(); mixed[3] ^= 1; await rejects('mixed channel key config', () => qualify(base, bytes(manifest), mixed));
  reset(); state.notAfter = now + 1000;
  channel = await qualify(); assert.equal(channel.release.expires, now + 1000); passed++;
  const before = network; now += 1000;
  await assert.rejects(channel.challenge(), error => error.stage === 'trust' && error.constraint === 'expired' && error.code === 'rejected'); passed++;
  assert.equal(network, before);
  reset();
  // Acquisition uses controlled in-memory responses, not any real public server.
  const seen = [];
  let latestTag = tag, payload = structuredClone(base), manifestBody = bytes(manifest), mode = 'ok';
  const discovery = `https://api.github.com/repos/${repo}/releases/latest`;
  const asset = `https://github.com/${repo}/releases/download/${tag}/tinfoil-deployment.json`;
  const cdn = 'https://release-assets.githubusercontent.com/synthetic-manifest';
  let assetLocation = cdn, assetStatus = 302, liveKey = key, provenanceBundles;
  globalThis.fetch = async request => {
    network++; seen.push(request.url);
    assert.equal(request.method, 'GET'); assert.equal(request.body, null);
    assert.equal(request.headers.has('authorization'), false); assert.equal(request.headers.has('cookie'), false);
    assert.equal(request.credentials, 'omit'); assert.equal(request.redirect, request.url === asset ? 'manual' : 'error'); assert.equal(request.cache, 'no-store');
    assert.equal(request.referrerPolicy, 'no-referrer');
    if (mode === 'network') throw new Error('HOSTILE transport');
    let body;
    if (request.url === discovery) body = bytes({ tag_name: latestTag, target_commitish: 'HOSTILE', url: 'https://HOSTILE',
      assets: [{ name: 'tinfoil-deployment.json', browser_download_url: 'https://HOSTILE' }] });
    else if (request.url === asset) body = new Uint8Array();
    else if (request.url === cdn) body = manifestBody;
    else if (request.url === origin + '/.well-known/tinfoil-attestation') body = bytes(payload.enclaveAttestationReport);
    else if (request.url.startsWith('https://kdsintf.amd.com/vcek/v1/Genoa/')) body = new Uint8Array([1]);
    else if (request.url === origin + '/.well-known/tinfoil-certificate') body = bytes({ certificate: payload.enclaveCert });
    else if (request.url === `https://api.github.com/repos/${repo}/attestations/sha256:${sha(manifestBody)}`) body = bytes({ attestations: (provenanceBundles ?? [payload.sigstoreBundle]).map(bundle => ({ bundle })) });
    else if (request.url === origin + '/.well-known/hpke-keys') body = liveKey;
    else throw new Error('HOSTILE unapproved request');
    const response = new Response(body, { status: mode === 'redirect' || (mode === 'cdn-redirect' && request.url === cdn) ? 302 : request.url === asset ? assetStatus : 200,
      headers: request.url === asset ? { location: assetLocation } : {} });
    Object.defineProperty(response, 'url', { value: mode === 'wrong-url' ? 'https://HOSTILE' : request.url });
    return response;
  };
  const connected = await test.connectPublished();
  assert.equal(connected.release.tag, tag); assert.equal(seen.length, 8);
  assert.equal(seen[0], origin + '/.well-known/tinfoil-attestation');
  assert.equal(new Set(seen).size, seen.length); assert(!seen.some(url => url.includes('HOSTILE'))); passed++;
  const attempts = network;
  assert.equal(connected.freshSession().release, connected.release); assert.equal(network, attempts);
  const unchangedStart=seen.length;
  await rejects('unchanged public report and keys',()=>test.recoverPublished(connected.freshSession()),'evidence_unchanged');
  assert.deepEqual(seen.slice(unchangedStart),[origin+'/.well-known/tinfoil-attestation',origin+'/.well-known/hpke-keys']);
  const cacheFile = path.join(test.evidenceDirectory(), 'entry.json');
  const record = JSON.parse(await readFile(cacheFile, 'utf8'));
  assert.deepEqual(Object.keys(record).sort(), ['digest', 'hint', 'manifest', 'origin', 'provenance', 'repository', 'tag', 'vcek', 'version']);
  assert.equal(record.version, 2); assert.equal(record.vcek, base.vcek);
  assert.equal(Buffer.from(record.manifest, 'base64').toString(), Buffer.from(manifestBody).toString());
  const verifyBefore = [hardwareCalls, dsseCalls], hitsBefore = seen.length;
  await test.connectPublished();
  assert.deepEqual(seen.slice(hitsBefore).map(url => new URL(url).hostname), [host, host, host]);
  assert.equal(hardwareCalls, verifyBefore[0] + 1); assert.equal(dsseCalls, verifyBefore[1] + 1); passed++;
  const { vcek: _vcek, ...legacyFields } = record;
  await writeFile(cacheFile, JSON.stringify({ ...legacyFields, version: 1 }));
  const migrationStart = seen.length, migrationHardware = hardwareCalls;
  await test.connectPublished();
  assert.deepEqual(seen.slice(migrationStart).map(url => new URL(url).hostname), [host, 'kdsintf.amd.com', host, host]);
  assert.equal(hardwareCalls, migrationHardware + 1);
  assert.deepEqual(JSON.parse(await readFile(cacheFile)), record); passed++;
  for (const wrong of ['Ag==', b64(Buffer.from('HOSTILE certificate chip URL credential'))]) {
    await writeFile(cacheFile, JSON.stringify({ ...record, vcek: wrong }));
    const before = seen.length;
    await rejects('cached wrong/corrupt VCEK DER', () => test.connectPublished(), 'verification_failed');
    assert(!seen.slice(before).some(url => url.includes('github.com') || url.includes('kdsintf.amd.com')));
    assert.equal(JSON.parse(await readFile(cacheFile)).vcek, wrong);
  }
  await writeFile(cacheFile, JSON.stringify(record));
  for (const [name, mutate] of [
    ['cached hardware signature', s => { s.hardwareSignature = false; }],
    ['cached certificate expiry', s => { s.notAfter = now - 1; }],
    ['cached certificate not yet valid', s => { s.notBefore = now + 1000; }],
    ['cached certificate report binding', s => { s.sanHash = '0'.repeat(64); }],
    ['cached live endpoint key', s => { s.sanKey = Buffer.alloc(32, 8); }],
    ['cached measurement', s => { s.measurement = 'c'.repeat(96); }],
    ['cached endpoint key', s => { s.hpkeKey = '08'.repeat(32); }],
    ['cached publisher', s => { s.signer.extBuildSignerDigest.buildSignerDigest = 'c'.repeat(40); }],
  ]) {
    reset(); mutate(state);
    const before = seen.length;
    await rejects(name, () => test.connectPublished(), 'verification_failed');
    assert(!seen.slice(before).some(url => url.includes('github.com') || url.includes('kdsintf.amd.com')), 'no fallback after failed verification');
  }
  reset();
  const wrongAssociation = JSON.parse(Buffer.from(record.provenance, 'base64'));
  wrongAssociation.attestations[0].bundle.dsseEnvelope.payload = b64(bytes({ _type: 'https://in-toto.io/Statement/v1', subject: [{ name: 'wrong-artifact' }], predicate: manifest }));
  await writeFile(cacheFile, JSON.stringify({ ...record, provenance: b64(bytes(wrongAssociation)) }));
  await rejects('cached wrong artifact association', () => test.connectPublished(), 'verification_failed');
  await writeFile(cacheFile, JSON.stringify(record));
  payload.enclaveAttestationReport.extra = 'public serialization change';
  const changedStart = seen.length;
  const recovered=await test.recoverPublished(connected.freshSession());
  assert.equal(seen.length - changedStart, 9);
  assert.equal(recovered.release.tag, connected.release.tag,'same tag may carry new fully verified report');
  await rejects('verified replacement becomes new baseline',()=>test.recoverPublished(recovered.freshSession()),'evidence_unchanged');
  passed++;
  payload = structuredClone(base);
  // Same report, manifest, release and measurement: a fully verified key
  // change still qualifies. The public key hint alone never authorizes it.
  liveKey=new Uint8Array([0,0,32,...Array(32).fill(8),0,4,0,1,0,2]);
  state.hpkeKey='08'.repeat(32);state.sanKey=Buffer.alloc(32,8);
  const keyStart=seen.length;
  const changedKey=await test.recoverPublished(connected.freshSession());
  assert.equal(changedKey.release.tag,connected.release.tag);
  assert.deepEqual(seen.slice(keyStart, keyStart+2),[origin+'/.well-known/tinfoil-attestation',origin+'/.well-known/hpke-keys']);
  await rejects('verified key becomes new baseline',()=>test.recoverPublished(changedKey.freshSession()),'evidence_unchanged');
  liveKey=key;
  await rejects('mismatched key candidate cannot replace trust',()=>test.recoverPublished(changedKey.freshSession()),'verification_failed');
  reset();passed++;
  const cold = async () => { await rm(test.evidenceDirectory(), { recursive: true, force: true }); return test.connectPublished(); };
  const fixtureParse = test.X509Certificate.parse;
  test.X509Certificate.parse = value => {
    if (typeof value !== 'string' && value.length === 1 && value[0] === 2) {
      return { ...state.signer, extSubjectAltName: { ...state.signer.extSubjectAltName,
        uri: workflow.replace(tag, 'v0.0.98') } };
    }
    return fixtureParse(value);
  };
  const older = structuredClone(payload.sigstoreBundle);
  older.verificationMaterial.certificate.rawBytes = 'Ag==';
  provenanceBundles = [older, payload.sigstoreBundle];
  assert.equal((await cold()).release.tag, tag); passed++;
  assert.equal((await test.connectPublished()).release.tag, tag); passed++;
  provenanceBundles = [payload.sigstoreBundle, payload.sigstoreBundle];
  await rejects('ambiguous current-tag provenance hints', cold, 'verification_failed');
  provenanceBundles = [older];
  await rejects('only another tag provenance', cold, 'verification_failed');
  const forged = structuredClone(payload.sigstoreBundle);
  forged.dsseEnvelope.signatures[0].sig = 'HOSTILE';
  provenanceBundles = [older, forged];
  await rejects('matching identity hint cannot bypass DSSE verification', cold, 'verification_failed');
  provenanceBundles = Array(31).fill(older);
  await rejects('bounded provenance candidate count', cold, 'verification_failed');
  provenanceBundles = undefined; test.X509Certificate.parse = fixtureParse;
  await rm(test.evidenceDirectory(), { recursive: true, force: true });
  latestTag = 'v1.2.3/../../HOSTILE';
  await rejects('discovery path injection', () => test.connectPublished(), 'verification_failed');
  latestTag = tag;
  payload.sigstoreBundle.dsseEnvelope.signatures[0].sig = 'HOSTILE';
  await rejects('discovery cannot authorize release', () => test.connectPublished(), 'verification_failed');
  payload = structuredClone(base); manifestBody = Buffer.from(JSON.stringify(manifest, null, 2));
  await rejects('download must hash to signed subject', () => test.connectPublished(), 'verification_failed');
  manifestBody = bytes(manifest);
  for (mode of ['network', 'redirect', 'wrong-url']) {
    const start = network;
    await rejects('bounded acquisition ' + mode, () => test.connectPublished(), 'evidence_unavailable');
    assert.equal(network, start + 1, 'no acquisition retries');
  }
  mode = 'ok';
  for (assetLocation of ['https://HOSTILE/manifest', 'http://release-assets.githubusercontent.com/manifest',
    'https://release-assets.githubusercontent.com.HOSTILE/manifest', 'https://user:password@release-assets.githubusercontent.com/manifest',
    'https://release-assets.githubusercontent.com:8443/manifest', cdn + '#fragment', 'relative-manifest', '']) {
    const start = network;
    await rejects('manifest redirect authority', () => test.connectPublished(), 'evidence_unavailable');
    assert.equal(network, start + 3, 'unapproved redirect is never followed');
  }
  assetLocation = cdn;
  for (assetStatus of [200, 301, 303, 307, 308]) await rejects('manifest redirect status', () => test.connectPublished(), 'evidence_unavailable');
  assetStatus = 302; mode = 'cdn-redirect';
  const beforeLoop = network;
  await rejects('no second asset redirect', () => test.connectPublished(), 'evidence_unavailable');
  assert.equal(network, beforeLoop + 4);
  mode = 'ok';
  const acquisitionFetch = globalThis.fetch;
  for (const [stage, matches] of [
    ['release_discovery', url => url === discovery], ['manifest_redirect', url => url === asset],
    ['manifest_download', url => url === cdn], ['gateway_attestation', url => url.endsWith('/.well-known/tinfoil-attestation')],
    ['amd_certificate', url => url.startsWith('https://kdsintf.amd.com/')],
    ['gateway_certificate', url => url.endsWith('/.well-known/tinfoil-certificate')],
    ['release_provenance', url => url.includes('/attestations/sha256:')], ['gateway_keys', url => url.endsWith('/.well-known/hpke-keys')],
  ]) {
    for (const failure of ['http', 'rate_limited', 'request', ...(stage === 'manifest_redirect' ? [] : ['body'])]) {
      globalThis.fetch = async request => {
        if (!matches(request.url)) return acquisitionFetch(request);
        if (failure === 'request') throw new Error('HOSTILE credential prompt history stack URL');
        const response = new Response(failure === 'body' ? new Uint8Array(1024 * 1024) : 'HOSTILE credential prompt history stack URL',
          { status: failure === 'body' ? 200 : failure === 'rate_limited' ? 429 : 503 });
        Object.defineProperty(response, 'url', { value: request.url }); return response;
      };
      await assert.rejects(cold(), error => {
        assert(error.message.includes(`Stage: ${stage}`), error.message);
        assert(error.message.includes(`constraint: ${failure}`) || error.message.includes(`constraint: ${failure === 'http' && stage === 'manifest_redirect' ? 'redirect' : failure}`), error.message);
        if (failure === 'http' || failure === 'rate_limited') assert(error.message.includes(`Observed HTTP status: ${failure === 'http' ? 503 : 429}`));
        assert(!error.message.includes('HOSTILE')); assert.equal(error.cause, undefined); return true;
      });
      await assert.rejects(readFile(cacheFile), error => error.code === 'ENOENT'); passed++;
    }
  }
  globalThis.fetch = acquisitionFetch;
  const controller = new AbortController(); controller.abort();
  const start = network;
  await rejects('cancel before acquisition', () => test.connectPublished(controller.signal), 'evidence_unavailable');
  assert.equal(network, start);
  console.log(JSON.stringify({ passed, scope: 'synthetic/local release policy only',
    actual: ['SDK orchestration', 'Sigstore identity policies', 'subject/tag/measurement matching', 'SAN decoding', 'SHA-256', 'gzip bounds', 'expiry', 'EHBP config'],
    mocked: ['AMD hardware verification', 'DSSE/Rekor crypto', 'X509 parsing', 'public network responses'], inference: 0, credentials: 0 }));
} finally {
  globalThis.fetch = originalFetch; globalThis.Date = NativeDate;
  test.X509Certificate.parse = originalParse; test.SigstoreVerifier.prototype.verifyDsse = originalDSSE;
  delete globalThis.__possumsReleaseTestHardware;
}
