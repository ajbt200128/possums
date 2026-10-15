// Offline opt-in primitive tests. Real built WASM rejection is separate from
// synthetic host projection; neither establishes a valid hardware/app-key chain.
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, copyFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { Worker } from 'node:worker_threads';
const out = process.argv[2];
assert(out, 'scratch verifier build directory required');
const { verifyTorV3 } = await import(pathToFileURL(join(out, 'verifier.mjs')));
const encode = value => new TextEncoder().encode(value);
const nonce = new Uint8Array(32), manifest = encode('{}');
let checks = 0;
async function rejects(operation, stage, constraint) {
  await assert.rejects(operation, error => {
    assert.equal(error.code, `possums_v3_${stage}_${constraint}`);
    assert.equal(error.stage, stage); assert.equal(error.constraint, constraint);
    assert(!error.message.includes('hostile-secret'));
    assert(!JSON.stringify(error).includes('hostile-secret'));
    return true;
  }); checks++;
}
for (const value of [undefined, new Uint8Array(), new Uint8Array(512 * 1024 + 1), 'hostile-secret'])
  await rejects(() => verifyTorV3(encode('{}'), nonce, value), 'publisher', 'manifest');
await rejects(() => verifyTorV3(new Uint8Array(), nonce, manifest), 'input', 'size');
await rejects(() => verifyTorV3(encode('{}'), nonce.subarray(1), manifest), 'input', 'nonce');
await rejects(() => verifyTorV3(encode('{}'), nonce, manifest, 'hostile-secret/private'), 'input', 'workload');
for (const raw of ['{', '{}', '{"format":"hostile-secret"}'])
  await rejects(() => verifyTorV3(encode(raw), nonce, manifest), 'envelope', 'rejected');
const aborted = new AbortController(); aborted.abort();
await rejects(() => verifyTorV3(encode('{}'), nonce, manifest, undefined, aborted.signal), 'bridge', 'interrupted');

// Exercise the built worker/Go ABI directly: unsupported trust controls reject,
// and the separate action cannot fall back to default manifest-free appraisal.
async function rawAppraise(request) {
  const worker = new Worker(pathToFileURL(join(out, 'verifier-worker.mjs')), {
    workerData: request, env: {}, execArgv: [], stdout: true, stderr: true,
  });
  worker.stdout.resume(); worker.stderr.resume();
  try {
    return await new Promise((resolve, reject) => {
      worker.once('message', resolve);
      worker.once('error', () => reject(new Error('test_worker_unavailable')));
      worker.once('exit', () => reject(new Error('test_worker_interrupted')));
    });
  } finally { await worker.terminate(); }
}
for (const request of [
  { action: 'hostile-secret' }, { action: 'tor_endorsement_key_v1', key_id: 'hostile-secret' },
  { action: 'tor_endorsement_key_v1', verified: true },
]) {
  assert.deepEqual(await rawAppraise(request), { failure: { code: 'possums_v3_input_schema', stage: 'input', constraint: 'schema' } }); checks++;
}
assert.deepEqual(await rawAppraise({ action: 'tor_endorsement_key_v1' }), {
  failure: { code: 'possums_v3_publisher_manifest', stage: 'publisher', constraint: 'manifest' },
}); checks++;

const scratch = await mkdtemp(join(tmpdir(), 'possums-tor-key-bridge-'));
try {
  await copyFile(join(out, 'verifier.mjs'), join(scratch, 'verifier.mjs'));
  const bridge = await import(pathToFileURL(join(scratch, 'verifier.mjs')));
  const worker = source => writeFile(join(scratch, 'verifier-worker.mjs'), source);
  const emit = value => worker(`import { parentPort } from 'node:worker_threads'; parentPort.postMessage(${JSON.stringify(value)});`);
  const result = { release_digest: 'a'.repeat(64), release_tag: 'v0.0.41', tls_key_sha256: 'b'.repeat(64), hpke_key: 'c'.repeat(64),
    freshness_expires_at_unix: Math.floor(Date.now()/1000) + 3600,
    tor_endorsement_key: 'd75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a' };
  await worker(`import { parentPort, workerData } from 'node:worker_threads';
    parentPort.postMessage(workerData.action === 'tor_endorsement_key_v1' && workerData.manifest === 'e30='
      && !Object.hasOwn(workerData, 'key_id') ? { tor_result: ${JSON.stringify(result)} }
      : { failure: { code: 'possums_v3_input_schema', stage: 'input', constraint: 'schema' } });`);
  const copiedManifest = encode('{}');
  const pending = bridge.verifyTorV3(encode('{}'), nonce, copiedManifest);
  copiedManifest.fill(9);
  assert.deepEqual(await pending, result); checks++;
  await emit({ tor_result: result });
  await rejects(() => bridge.verifyV3(encode('{}'), nonce), 'bridge', 'schema');
  const { tor_endorsement_key: _key, ...defaultResult } = result;
  await emit({ result: defaultResult });
  assert.deepEqual(await bridge.verifyV3(encode('{}'), nonce), defaultResult); checks++;
  await rejects(() => bridge.verifyTorV3(encode('{}'), nonce, manifest), 'bridge', 'schema');
  for (const key of [undefined, 'a'.repeat(62), 'A'.repeat(64), 'hostile-secret']) {
    const invalid = { ...result, tor_endorsement_key: key };
    if (key === undefined) delete invalid.tor_endorsement_key;
    await emit({ tor_result: invalid });
    await rejects(() => bridge.verifyTorV3(encode('{}'), nonce, manifest), 'bridge', 'schema');
  }
  await emit({ tor_result: { ...result, raw: 'hostile-secret' } });
  await rejects(() => bridge.verifyTorV3(encode('{}'), nonce, manifest), 'bridge', 'schema');
  await emit({ tor_result: { ...result, freshness_expires_at_unix: Math.floor(Date.now()/1000) } });
  await rejects(() => bridge.verifyTorV3(encode('{}'), nonce, manifest), 'freshness', 'expired');
  for (const constraint of ['tor_endorsement_entry', 'tor_endorsement_format', 'tor_endorsement_spki', 'tor_endorsement_algorithm']) {
    await emit({ failure: { code: `possums_v3_keys_${constraint}`, stage: 'keys', constraint } });
    await rejects(() => bridge.verifyTorV3(encode('{}'), nonce, manifest), 'keys', constraint);
  }
  const terminate = Worker.prototype.terminate;
  try {
    Worker.prototype.terminate = async function() { await terminate.call(this); throw new Error('hostile-secret'); };
    await emit({ failure: { code: 'possums_v3_keys_tor_endorsement_format', stage: 'keys', constraint: 'tor_endorsement_format' } });
    await rejects(() => bridge.verifyTorV3(encode('{}'), nonce, manifest), 'keys', 'tor_endorsement_format');
    await emit({ tor_result: result });
    await rejects(() => bridge.verifyTorV3(encode('{}'), nonce, manifest), 'bridge', 'cleanup_cause_unknown');
  } finally { Worker.prototype.terminate = terminate; }
} finally { await rm(scratch, { recursive: true, force: true }); }
console.log(JSON.stringify({ checks, scope: 'offline opt-in built WASM rejection and synthetic host projection only; no positive hardware/app-key chain or live operations' }));
