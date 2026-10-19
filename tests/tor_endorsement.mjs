// Offline only. Rust signs with PUBLIC RFC 8032 test material; Node verifies.
// Synthetic worker facts test orchestration, NEVER trusted hardware acceptance.
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { mkdtemp, copyFile, writeFile, rm } from 'node:fs/promises';
import { syncBuiltinESMExports } from 'node:module';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { Worker } from 'node:worker_threads';

const build = process.argv[2];
assert(build, 'scratch real WASM verifier build directory required');
const output = execFileSync('cargo', ['test', '--offline', '--locked', '--lib',
  'tor_endorsement::tests::node_interop_fixture', '--', '--exact', '--nocapture'], {
  encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'], timeout: 120_000,
});
const line = output.split('\n').find(line => line.startsWith('TOR_ENDORSEMENT_FIXTURE='));
assert(line, 'actual Rust public fixture output required');
const fixture = JSON.parse(line.slice('TOR_ENDORSEMENT_FIXTURE='.length));
const a = Buffer.from(fixture.application_key), challenge = Buffer.from(fixture.challenge);
const b = Buffer.from(fixture.onion_identity), payload = Buffer.from(fixture.payload);
const signature = Buffer.from(fixture.signature);
const encode = value => new TextEncoder().encode(value);
let checks = 0;
async function rejects(operation, code) {
  await assert.rejects(operation, error => {
    assert.equal(error.code, code);
    assert.equal(typeof error.stage, 'string'); assert.equal(typeof error.constraint, 'string');
    assert.equal(typeof error.action, 'string'); assert(error.action.length > 10);
    assert(!error.message.includes('hostile-secret'));
    assert(!JSON.stringify(error).includes('hostile-secret'));
    assert.equal(error.stack, undefined);
    return true;
  }); checks++;
}
const failure = (stage, constraint) => `possums_tor_endorsement_${stage}_${constraint}`;
const mutate = (bytes, index = 0) => { const result = Buffer.from(bytes); result[index] ^= 1; return result; };
const scratch = await mkdtemp(join(tmpdir(), 'possums-tor-endorsement-'));
try {
  for (const file of ['verifier.mjs', 'verifier-worker.mjs', 'verifier.wasm', 'wasm_exec.cjs'])
    await copyFile(join(build, file), join(scratch, file));
  await copyFile(new URL('../clients/pi/tor-endorsement.mjs', import.meta.url), join(scratch, 'tor-endorsement.mjs'));
  const core = await import(pathToFileURL(join(scratch, 'tor-endorsement.mjs')));
  const check = (key = a, nonce = challenge, onion = b, message = payload, sig = signature) =>
    core.checkTorEndorsementSignature(key, nonce, onion, message, sig);
  const authorize = (evidence = encode('{}'), nonce = challenge, manifest = encode('{}'),
    onion = b, message = payload, sig = signature, signal) =>
    core.authorizeTorEndorsement(evidence, nonce, manifest, onion, message, sig, undefined, signal);

  assert.deepEqual(await check(), { signature_valid: true, transport_authorized: false }); checks++;
  assert(Object.isFrozen(await check())); checks++;
  // Independently reproduce Rust's signature via supported Node JWK import.
  // PUBLIC RFC 8032 seed, not a usable runtime key or attested A.
  const publicTestKey = crypto.createPrivateKey({ format: 'jwk', key: {
    kty: 'OKP', crv: 'Ed25519', x: a.toString('base64url'),
    d: Buffer.from('9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60', 'hex').toString('base64url'),
  } });
  assert.deepEqual(crypto.sign(null, payload, publicTestKey), signature); checks++;
  const otherA = Buffer.from('3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c', 'hex');
  await rejects(() => check(otherA), failure('signature', 'rejected'));
  await rejects(() => check(a, mutate(challenge)), failure('payload', 'challenge'));
  await rejects(() => check(a, challenge, mutate(b)), failure('payload', 'onion_identity'));
  await rejects(() => check(a, challenge, b, payload, mutate(signature)), failure('signature', 'rejected'));
  for (const [index, constraint] of [[0, 'domain'], [29, 'domain'], [30, 'version'], [31, 'challenge'], [63, 'onion_identity']]) {
    const message = mutate(payload, index);
    // Even a valid signature over a noncanonical/wrong target message rejects.
    await rejects(() => check(a, challenge, b, message, crypto.sign(null, message, publicTestKey)), failure('payload', constraint));
  }
  const otherMessage = mutate(payload, 31);
  await rejects(() => check(a, challenge, b, payload, crypto.sign(null, otherMessage, publicTestKey)), failure('signature', 'rejected'));
  for (const [position, size, constraint] of [[0, 32, 'application_key'], [1, 32, 'challenge'], [2, 32, 'onion_identity'], [3, 95, 'payload'], [4, 64, 'signature']]) {
    for (const invalid of [undefined, 'hostile-secret', new Uint8Array(size - 1), new Uint8Array(size + 1)]) {
      const args = [a, challenge, b, payload, signature]; args[position] = invalid;
      await rejects(() => core.checkTorEndorsementSignature(...args), failure('input', constraint));
    }
  }
  const jsonMessage = encode(JSON.stringify({ challenge: [...challenge], onion: [...b] }));
  await rejects(() => check(a, challenge, b, jsonMessage, crypto.sign(null, jsonMessage, publicTestKey)), failure('input', 'payload'));

  // Real unmodified WASM bridge: a valid Rust signature cannot bypass appraisal.
  for (const evidence of [encode('{}'), encode('{'), encode('{"format":"hostile-secret"}')])
    await rejects(() => authorize(evidence), 'possums_v3_envelope_rejected');
  await rejects(() => core.authorizeTorEndorsement(encode('{}'), challenge, undefined, b, payload, signature), 'possums_v3_publisher_manifest');
  const preAborted = new AbortController(); preAborted.abort();
  await rejects(() => authorize(encode('{}'), challenge, encode('{}'), b, payload, signature, preAborted.signal), 'possums_v3_bridge_interrupted');

  // From here onward ONLY synthetic worker projection, not hardware acceptance.
  const facts = {
    release_digest: 'a'.repeat(64), release_tag: 'v0.0.41', tls_key_sha256: 'b'.repeat(64), hpke_key: 'c'.repeat(64),
    freshness_expires_at_unix: Math.floor(Date.now() / 1000) + 3600, tor_endorsement_key: a.toString('hex'),
  };
  const worker = source => writeFile(join(scratch, 'verifier-worker.mjs'), source);
  const emit = value => worker(`import { parentPort } from 'node:worker_threads'; parentPort.postMessage(${JSON.stringify(value)});`);
  await worker(`import { parentPort, workerData } from 'node:worker_threads';
    parentPort.postMessage(workerData.action === 'tor_endorsement_key_v1'
      && workerData.nonce === '${challenge.toString('base64')}' && workerData.manifest === 'e30='
      ? { tor_result: ${JSON.stringify(facts)} }
      : { failure: { code: 'possums_v3_input_schema', stage: 'input', constraint: 'schema' } });`);
  const mutable = [challenge, b, payload, signature].map(value => Buffer.from(value));
  const pending = authorize(encode('{}'), mutable[0], encode('{}'), mutable[1], mutable[2], mutable[3]);
  mutable.forEach(value => value.fill(9));
  const result = await pending;
  assert.deepEqual(result, { ...facts, onion_identity: b.toString('hex'), transport_authorized: false });
  assert(Object.isFrozen(result)); checks++;
  assert.throws(() => { result.onion_identity = 'hostile-secret'; }, TypeError); checks++;
  await emit({ tor_result: { ...facts, tor_endorsement_key: otherA.toString('hex') } });
  await rejects(() => authorize(), failure('signature', 'rejected'));
  await emit({ tor_result: { ...facts, freshness_expires_at_unix: Math.floor(Date.now() / 1000) } });
  await rejects(() => authorize(), 'possums_v3_freshness_expired');
  await emit({ tor_result: facts });
  await rejects(() => authorize(encode('{}'), mutate(challenge)), failure('payload', 'challenge'));
  await rejects(() => authorize(encode('{}'), challenge, encode('{}'), mutate(b)), failure('payload', 'onion_identity'));

  // Exercise asynchronous computation boundary using REAL Node verification.
  // No clock/trust/verdict parameters are injected into the production API.
  const realVerify = crypto.verify, realCreatePublicKey = crypto.createPublicKey;
  try {
    const controller = new AbortController();
    crypto.verify = (...args) => {
      const callback = args.pop();
      return realVerify(...args, (error, valid) => { controller.abort(); callback(error, valid); });
    };
    syncBuiltinESMExports();
    await rejects(() => authorize(encode('{}'), challenge, encode('{}'), b, payload, signature, controller.signal), failure('authorization', 'interrupted'));
    const primary = new AbortController();
    crypto.verify = (...args) => {
      const callback = args.pop();
      return realVerify(...args, (error, valid) => { primary.abort(); callback(error, valid); });
    };
    syncBuiltinESMExports();
    await rejects(() => authorize(encode('{}'), challenge, encode('{}'), b, payload, mutate(signature), primary.signal), failure('signature', 'rejected'));
    const brokenSignal = new AbortController();
    crypto.verify = (...args) => {
      const callback = args.pop();
      return realVerify(...args, (error, valid) => {
        Object.defineProperty(brokenSignal.signal, 'aborted', { get() { throw new Error('hostile-secret'); } });
        callback(error, valid);
      });
    };
    syncBuiltinESMExports();
    await rejects(() => authorize(encode('{}'), challenge, encode('{}'), b, payload, signature, brokenSignal.signal), failure('authorization', 'cause_unknown'));
    const expiry = Math.floor(Date.now() / 1000) + 2;
    await emit({ tor_result: { ...facts, freshness_expires_at_unix: expiry } });
    crypto.verify = (...args) => {
      const callback = args.pop();
      return realVerify(...args, (error, valid) => {
        setTimeout(() => callback(error, valid), Math.max(0, expiry * 1000 - Date.now() + 10));
      });
    };
    syncBuiltinESMExports();
    await rejects(() => authorize(), failure('freshness', 'expired'));
    await emit({ tor_result: facts });
    crypto.verify = (...args) => { args.at(-1)(new Error('hostile-secret'), false); };
    syncBuiltinESMExports();
    await rejects(() => authorize(), failure('signature', 'cause_unknown'));
    crypto.verify = () => { throw new Error('hostile-secret'); };
    syncBuiltinESMExports();
    await rejects(() => check(), failure('signature', 'cause_unknown'));
    crypto.verify = realVerify;
    crypto.createPublicKey = () => { throw new Error('hostile-secret'); };
    syncBuiltinESMExports();
    await rejects(() => check(), failure('signature', 'cause_unknown'));
  } finally {
    crypto.verify = realVerify; crypto.createPublicKey = realCreatePublicKey;
    syncBuiltinESMExports();
  }
  const terminate = Worker.prototype.terminate;
  try {
    Worker.prototype.terminate = async function() { await terminate.call(this); throw new Error('hostile-secret'); };
    await emit({ failure: { code: 'possums_v3_keys_tor_endorsement_spki', stage: 'keys', constraint: 'tor_endorsement_spki' } });
    await rejects(() => authorize(), 'possums_v3_keys_tor_endorsement_spki');
    await emit({ tor_result: facts });
    await rejects(() => authorize(), 'possums_v3_bridge_cleanup_cause_unknown');
  } finally { Worker.prototype.terminate = terminate; }
  const unknown = new core.TorEndorsementFailure('hostile-secret', 'hostile-secret');
  assert.equal(unknown.code, failure('signature', 'cause_unknown'));
  assert(!unknown.message.includes('hostile-secret')); checks++;
} finally { await rm(scratch, { recursive: true, force: true }); }
console.log(JSON.stringify({ checks, scope: 'offline Rust-sign/Node-verify public RFC fixture, real WASM wrapper rejection, synthetic host races only; no positive hardware/app-key chain or transport authorization' }));
