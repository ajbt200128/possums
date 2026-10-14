// Offline generated-artifact tests. Loads only the public-evidence bridge,
// never a Pi provider, saved credentials, application endpoint or inference.
import assert from 'node:assert/strict';
import { readFile, mkdtemp, writeFile, copyFile, rm } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { Worker } from 'node:worker_threads';
const out = process.argv[2];
assert(out, 'scratch verifier build directory required');
const { verifyV3, VerifierFailure } = await import(pathToFileURL(join(out, 'verifier.mjs')));
const encode = value => new TextEncoder().encode(value);
const nonce = new Uint8Array(32);
let checks = 0;
async function rejects(operation, stage, constraint) {
  await assert.rejects(operation, error => {
    assert(error instanceof Error);
    assert.equal(error.stage, stage); assert.equal(error.constraint, constraint);
    assert.equal(error.code, `possums_v3_${stage}_${constraint}`);
    assert(!JSON.stringify(error).includes('hostile-secret'));
    assert(!error.message.includes('hostile-secret'));
    return true;
  }); checks++;
}
const report = JSON.parse(await readFile(join(out, 'verifier-build-report.json'), 'utf8'));
for (const [name, expected] of Object.entries(report.artifacts)) {
  assert.equal(createHash('sha256').update(await readFile(join(out, name))).digest('hex'), expected.sha256); checks++;
}
assert.equal(report.go, '1.27.2'); checks++;
assert.equal(new VerifierFailure('__proto__', 'hostile-secret').code, 'possums_v3_bridge_cause_unknown'); checks++;
await rejects(() => verifyV3(new Uint8Array(), nonce), 'input', 'size');
await rejects(() => verifyV3(new Uint8Array(16*1024*1024+1), nonce), 'input', 'size');
await rejects(() => verifyV3(encode('{}'), new Uint8Array(31)), 'input', 'nonce');
await rejects(() => verifyV3(encode('{}'), nonce, 'hostile-secret/private'), 'input', 'workload');
await rejects(() => verifyV3(encode('{}'), nonce, undefined, {}), 'input', 'schema');
for (const raw of ['{', '{}', '{"format":"hostile-secret"}', '{"format":"https://tinfoil.sh/predicate/sev-snp-guest/v2","body":"hostile-secret"}']) {
  await rejects(() => verifyV3(encode(raw), nonce), 'envelope', 'rejected');
}
const before = new AbortController(); before.abort();
await rejects(() => verifyV3(encode('{}'), nonce, undefined, before.signal), 'bridge', 'interrupted');
const during = new AbortController();
const pending = verifyV3(encode('{}'), nonce, undefined, during.signal);
during.abort();
await rejects(() => pending, 'bridge', 'interrupted');

// Host-boundary tests use a synthetic worker, not a trusted verifier. They
// establish closed output projection, expiry checks and primary-error cleanup,
// not hardware/Sigstore acceptance or an executable inference channel.
const scratch = await mkdtemp(join(tmpdir(), 'possums-v3-bridge-tests-'));
try {
  await copyFile(join(out, 'verifier.mjs'), join(scratch, 'verifier.mjs'));
  const bridge = await import(pathToFileURL(join(scratch, 'verifier.mjs')));
  const worker = async source => writeFile(join(scratch, 'verifier-worker.mjs'), source);
  const emit = value => worker(`import { parentPort } from 'node:worker_threads'; parentPort.postMessage(${JSON.stringify(value)});`);
  await worker('throw new Error("hostile-secret");');
  await rejects(() => bridge.verifyV3(encode('{}'), nonce), 'bridge', 'artifact_unavailable');
  await emit({ failure: { code: 'hostile-secret', stage: '__proto__', constraint: 'unknown' } });
  await rejects(() => bridge.verifyV3(encode('{}'), nonce), 'bridge', 'schema');
  await emit({ failure: { code: 'possums_v3_input_schema', stage: ['input'], constraint: 'schema' } });
  await rejects(() => bridge.verifyV3(encode('{}'), nonce), 'bridge', 'schema');
  await emit({ failure: { code: 'possums_v3_publisher_identity', stage: 'publisher', constraint: 'identity', raw: 'hostile-secret' } });
  await rejects(() => bridge.verifyV3(encode('{}'), nonce), 'bridge', 'schema');
  await worker('import { parentPort } from "node:worker_threads"; parentPort.postMessage({failure:{code:"possums_v3_publisher_identity",stage:"publisher",constraint:"identity"}}); throw new Error("hostile-secret");');
  await rejects(() => bridge.verifyV3(encode('{}'), nonce), 'publisher', 'identity');
  const result = { release_digest: 'a'.repeat(64), release_tag: 'v0.0.41', tls_key_sha256: 'b'.repeat(64), hpke_key: 'c'.repeat(64), freshness_expires_at_unix: Math.floor(Date.now()/1000)+3600 };
  await emit({ result });
  assert.deepEqual(await bridge.verifyV3(encode('{}'), nonce), result); checks++;
  const terminate = Worker.prototype.terminate;
  const lateAbort = new AbortController();
  try {
    Worker.prototype.terminate = async function() { lateAbort.abort(); return await terminate.call(this); };
    await rejects(() => bridge.verifyV3(encode('{}'), nonce, undefined, lateAbort.signal), 'bridge', 'interrupted');
  } finally { Worker.prototype.terminate = terminate; }
  const noHPKE = { ...result }; delete noHPKE.hpke_key;
  await emit({ result: noHPKE });
  await rejects(() => bridge.verifyV3(encode('{}'), nonce), 'keys', 'hpke');
  await emit({ result });
  try {
    Worker.prototype.terminate = async function() { await terminate.call(this); throw new Error('hostile-secret'); };
    await rejects(() => bridge.verifyV3(encode('{}'), nonce), 'bridge', 'cleanup_cause_unknown');
    await emit({ failure: { code: 'possums_v3_publisher_identity', stage: 'publisher', constraint: 'identity' } });
    await rejects(() => bridge.verifyV3(encode('{}'), nonce), 'publisher', 'identity');
  } finally { Worker.prototype.terminate = terminate; }
  await emit({ result: { ...result, freshness_expires_at_unix: Math.floor(Date.now()/1000) } });
  await rejects(() => bridge.verifyV3(encode('{}'), nonce), 'freshness', 'expired');
  await emit({ result: { ...result, raw: 'hostile-secret' } });
  await rejects(() => bridge.verifyV3(encode('{}'), nonce), 'bridge', 'schema');
  await emit({ result, failure: { code: 'hostile-secret' } });
  await rejects(() => bridge.verifyV3(encode('{}'), nonce), 'bridge', 'schema');
  await worker('process.exit(0);');
  await rejects(() => bridge.verifyV3(encode('{}'), nonce), 'bridge', 'cause_unknown');
  await worker('setInterval(() => {}, 1000);');
  const schedule = globalThis.setTimeout;
  try {
    // Test-only accelerated deadline; the shipped bridge has no clock override.
    globalThis.setTimeout = (fn, delay, ...args) => schedule(fn, delay === 30_000 ? 1 : delay, ...args);
    await rejects(() => bridge.verifyV3(encode('{}'), nonce), 'bridge', 'deadline');
  } finally { globalThis.setTimeout = schedule; }
} finally { await rm(scratch, { recursive: true, force: true }); }
console.log(JSON.stringify({ checks, scope: 'offline native-Go WASM rejection and synthetic JS bridge projection; no live/authentication/inference or positive hardware acceptance' }));
