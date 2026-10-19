// Offline only: PUBLIC RFC 8032 material, real production WASM rejection.
// No synthetic positive SDK verdict, connector, Tor, credentials or inference.
import assert from 'node:assert/strict';
import { readFile, mkdtemp, copyFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import workerThreads from 'node:worker_threads';
import { syncBuiltinESMExports } from 'node:module';
const sourceURL = process.env.PHASE02_RELEASE_SOURCE
  ? pathToFileURL(join(process.env.PHASE02_RELEASE_SOURCE, '/')) : new URL('../clients/pi/', import.meta.url);
const { decodeTorSetupResponse, TorSetupFailure, TorSetupBackendFailure, verifyTorSetupResponse } = await import(new URL('tor-setup.mjs', sourceURL));
const { checkTorEndorsementSignature, TorEndorsementFailure } = await import(new URL('tor-endorsement.mjs', sourceURL));
const { VerifierFailure } = await import(new URL('verifier.mjs', sourceURL));

assert.equal(process.version, 'v24.13.0', 'use the pinned offline Node environment');
const build = process.argv[2];
assert(build, 'an existing production WASM verifier scratch build is required');
const fixture = JSON.parse(await readFile(new URL('./fixtures/tor_grant_rfc8032.json', import.meta.url), 'utf8'));
// The fixture generator defines this exact PUBLIC message; its JSON stores the
// signature/key, not payload bytes. Do not generate or use runtime key material.
const challenge = Buffer.alloc(32, 0x23), onion = Buffer.alloc(32, 0x42);
const payload = Buffer.concat([Buffer.from('possums/tor-onion-endorsement\0', 'ascii'), Buffer.from([1]), challenge, onion]);
const signature = Buffer.from(fixture.signature, 'hex');
const wire = { evidence: {}, payload: payload.toString('hex'), signature: fixture.signature };
const encode = value => Buffer.from(value, 'utf8');
const response = value => encode(JSON.stringify(value));
const MAX_EVIDENCE = 16 * 1024 * 1024, MAX_RESPONSE = MAX_EVIDENCE + 2048;
const canary = 'hostile-secret-input-only';
let checks = 0, fetchCalls = 0;
const realFetch = globalThis.fetch;
globalThis.fetch = async () => { fetchCalls++; throw new Error(canary); };
function diagnostic(error, code) {
  assert.equal(error.code, code);
  for (const field of ['stage', 'constraint', 'action']) assert.equal(typeof error[field], 'string');
  assert(error.action.length > 10);
  assert.equal(error.stack, undefined);
  assert.equal(error.cause, undefined);
  assert(Object.isFrozen(error));
  assert(!String(error).includes(canary));
  assert(!JSON.stringify(error).includes(canary));
  return true;
}
function rejected(bytes, constraint) {
  assert.throws(() => decodeTorSetupResponse(bytes), error => diagnostic(error, `possums_tor_setup_response_${constraint}`));
  checks++;
}
async function rejects(operation, code) {
  await assert.rejects(operation, error => diagnostic(error, code)); checks++;
}
const mutate = bytes => { const copy = Buffer.from(bytes); copy[0] ^= 1; return copy; };
const scratch = await mkdtemp(join(tmpdir(), 'possums-tor-setup-'));
const RealWorker = workerThreads.Worker;
try {
  const raw = response(wire), decoded = decodeTorSetupResponse(raw);
  assert.deepEqual(Object.keys(decoded).sort(), ['evidence', 'payload', 'signature', 'transport_authorized']);
  assert.equal(decoded.evidence.toString(), '{}');
  assert.deepEqual(decoded.payload, payload); assert.deepEqual(decoded.signature, signature);
  assert.equal(decoded.transport_authorized, false); assert(Object.isFrozen(decoded)); checks++;
  assert.throws(() => { decoded.transport_authorized = true; }, TypeError); checks++;
  raw.fill(0);
  assert.deepEqual(decoded.payload, payload); assert.deepEqual(decoded.signature, signature); checks++;
  const second = decodeTorSetupResponse(response(wire));
  decoded.payload.fill(0); decoded.signature.fill(0); decoded.evidence.fill(0);
  assert.deepEqual(second.payload, payload); assert.deepEqual(second.signature, signature);
  assert.equal(second.evidence.toString(), '{}'); checks++;

  for (const invalid of [undefined, null, {}, [], canary]) rejected(invalid, 'bytes');
  for (const text of ['', '{', JSON.stringify(wire).slice(0, -1), `${JSON.stringify(wire)} ${canary}`,
    `${JSON.stringify(wire)}{}`, '{"evidence":NaN}', '{"evidence":{},}']) rejected(encode(text), 'json');
  rejected(Buffer.from([0xff]), 'utf8');
  for (const invalid of [null, [], [wire], true, 1, canary]) rejected(response(invalid), 'schema');
  for (const invalid of [null, [], canary, 1, false]) rejected(response({ ...wire, evidence: invalid }), 'schema');
  for (const key of ['evidence', 'payload', 'signature']) {
    const missing = { ...wire }; delete missing[key]; rejected(response(missing), 'schema');
  }
  for (const key of ['extra', '__proto__', 'constructor', 'prototype'])
    rejected(encode(`{"${key}":"${canary}",${JSON.stringify(wire).slice(1)}`), 'schema');
  for (const key of ['__proto__', 'constructor', 'prototype', '\\u005f_proto__'])
    rejected(encode(JSON.stringify(wire).replace('"evidence":{}', `"evidence":{"nested":{"${key}":"${canary}"}}`)), 'schema');
  for (const [field, length] of [['payload', 190], ['signature', 128]]) {
    for (const invalid of [null, 1, [], {}, '', '0'.repeat(length - 1), '0'.repeat(length + 1),
      'A'.repeat(length), 'g'.repeat(length), `${'0'.repeat(length - 1)} `,
      `${'0'.repeat(length - canary.length)}${canary}`, `0x${'0'.repeat(length - 2)}`])
      rejected(response({ ...wire, [field]: invalid }), field);
  }
  rejected(Buffer.alloc(MAX_RESPONSE + 1), 'size');
  const padded = Buffer.alloc(MAX_RESPONSE, 0x20); response(wire).copy(padded);
  assert.equal(decodeTorSetupResponse(padded).transport_authorized, false); checks++;
  const evidenceAtLimit = { padding: 'x'.repeat(MAX_EVIDENCE - Buffer.byteLength('{"padding":""}')) };
  assert.equal(decodeTorSetupResponse(response({ ...wire, evidence: evidenceAtLimit })).evidence.length, MAX_EVIDENCE); checks++;
  rejected(response({ ...wire, evidence: { padding: `${evidenceAtLimit.padding}x` } }), 'evidence_size');
  // Explicit native JSON limitation, NOT acceptance/appraisal: duplicate names
  // collapse to the last value. No new duplicate scanner is claimed or shipped.
  assert.equal(decodeTorSetupResponse(encode(`{"evidence":{"unused":1},${JSON.stringify(wire).slice(1)}`)).evidence.toString(), '{}'); checks++;

  const backend = (code, stage, constraint, extra = {}) => ({ error: { code, stage, constraint, message: canary, action: canary, ...extra } });
  const backendCases = [
    ['possums_v3_keys_tor_endorsement_spki', 'keys', 'tor_endorsement_spki'],
    ['possums_v3_verification_rejected', 'verification', 'rejected'],
    ['possums_v3_publisher_identity', 'publisher', 'identity'],
    ['possums_v3_acquisition_transport_unknown', 'acquisition', 'transport_unknown'],
    ['possums_v3_acquisition_http_status', 'acquisition', 'http_status'],
    ['tor_grant_mismatch', 'grant', 'derived_mounted_appraised_A_equality'],
    ['tor_helper_unknown', 'helper', 'bounded_helper_completion'],
    ['tor_helper_invalid', 'helper', 'bounded_closed_response'],
    ['tor_appraisal_unsupported', 'appraisal', 'verifier_support'],
    ['tor_appraisal_expired', 'freshness', 'current_evidence'],
    ['tor_nonce_refused', 'input', 'challenge_get_empty_body'],
    ['tor_response_encoding_limit', 'response', 'bounded_json'],
    ['tor_response_encoding_unknown', 'response', 'cause_unknown'],
  ];
  for (const [code, stage, constraint] of backendCases) {
    const bytes = response(backend(code, stage, constraint));
    assert.throws(() => decodeTorSetupResponse(bytes), error => {
      assert(error instanceof TorSetupBackendFailure);
      assert.equal(error.stage, stage); assert.equal(error.constraint, constraint);
      assert.equal(error.metadata_authenticated, false); assert.equal(error.transport_authorized, false);
      assert.equal(error.status, undefined);
      assert(String(error).includes('Unverified public backend diagnostic'));
      assert(!String(error).includes('no service launched'));
      if (code === 'tor_nonce_refused') assert(error.action.includes('GET') && error.action.includes('32-byte nonce') && error.action.includes('empty body'));
      if (code === 'tor_grant_mismatch') assert(error.action.includes('key equality'));
      if (stage === 'freshness') assert(error.action.includes('fresh public evidence'));
      return diagnostic(error, code);
    }); checks++;
    await rejects(() => verifyTorSetupResponse(bytes, challenge, encode('{}'), onion), code);
  }
  for (const status of [100, 503, 999]) {
    assert.throws(() => decodeTorSetupResponse(response(backend('possums_v3_acquisition_http_status', 'acquisition', 'http_status', { status }))), error => {
      assert.equal(error.status, status);
      assert(String(error).includes('not necessarily the gateway response status'));
      assert(String(error).includes('no backend authenticity'));
      return diagnostic(error, 'possums_v3_acquisition_http_status');
    }); checks++;
  }
  for (const status of [null, false, '503', 99, 1000, 503.5, {}, []])
    rejected(response(backend('possums_v3_acquisition_http_status', 'acquisition', 'http_status', { status })), 'schema');
  rejected(response(backend('possums_v3_keys_tor_endorsement_spki', 'keys', 'tor_endorsement_spki', { status: 503 })), 'schema');
  const known = backend(...backendCases[0]);
  for (const field of ['code', 'stage', 'constraint']) {
    for (const value of [canary, null, {}, [], false, 503])
      rejected(response({ error: { ...known.error, [field]: value } }), 'schema');
  }
  for (const field of ['message', 'action']) {
    for (const value of [null, {}, [], false, 503])
      rejected(response({ error: { ...known.error, [field]: value } }), 'schema');
  }
  for (const field of Object.keys(known.error)) {
    const missing = { ...known.error }; delete missing[field]; rejected(response({ error: missing }), 'schema');
  }
  for (const value of [null, [], false, 503, canary]) rejected(response({ error: value }), 'schema');
  rejected(response({ ...known, evidence: {} }), 'schema');
  rejected(response({ error: { ...known.error, extra: canary } }), 'schema');
  for (const key of ['__proto__', 'constructor', 'prototype'])
    rejected(encode(`{"error":{"${key}":"${canary}",${JSON.stringify(known.error).slice(1)}}`), 'schema');
  for (const [code, stage, constraint] of [
    ['possums_v3_bridge_schema', 'bridge', 'schema'],
    ['possums_v3_input_schema', 'input', 'schema'],
    ['possums_v3_keys_tor_endorsement_spki', 'keys', 'tls'],
  ]) rejected(response(backend(code, stage, constraint)), 'schema');
  diagnostic(new TorSetupBackendFailure(canary, canary), 'possums_tor_setup_backend_cause_unknown'); checks++;
  diagnostic(new TorSetupBackendFailure('acquisition', 'http_status', canary), 'possums_tor_setup_backend_cause_unknown'); checks++;

  const publicKey = Buffer.from(fixture.application_key, 'hex');
  const signatureFacts = await checkTorEndorsementSignature(publicKey, challenge, onion, second.payload, second.signature);
  assert.deepEqual(signatureFacts, { signature_valid: true, transport_authorized: false }); checks++;
  await rejects(() => checkTorEndorsementSignature(publicKey, mutate(challenge), onion, second.payload, second.signature),
    'possums_tor_endorsement_payload_challenge');
  await rejects(() => checkTorEndorsementSignature(publicKey, challenge, mutate(onion), second.payload, second.signature),
    'possums_tor_endorsement_payload_onion_identity');
  await rejects(() => checkTorEndorsementSignature(publicKey, challenge, onion, second.payload, mutate(second.signature)),
    'possums_tor_endorsement_signature_rejected');

  const preAbort = new AbortController(); preAbort.abort(new Error(canary));
  await rejects(() => verifyTorSetupResponse(undefined, undefined, undefined, undefined, undefined, preAbort.signal),
    'possums_tor_setup_authorization_interrupted');
  await rejects(() => verifyTorSetupResponse(response(wire), challenge, encode('{}'), onion, undefined, {}), 'possums_v3_input_schema');
  for (const manifest of [undefined, encode(''), encode(canary.repeat(60000))])
    await rejects(() => verifyTorSetupResponse(response(wire), challenge, manifest, onion), 'possums_v3_publisher_manifest');
  await rejects(() => verifyTorSetupResponse(response(wire), challenge.subarray(1), encode('{}'), onion), 'possums_tor_endorsement_input_challenge');
  await rejects(() => verifyTorSetupResponse(response(wire), challenge, encode('{}'), onion.subarray(1)), 'possums_tor_endorsement_input_onion_identity');
  const brokenSignal = new AbortController().signal;
  Object.defineProperty(brokenSignal, 'aborted', { get() { throw new Error(canary); } });
  await rejects(() => verifyTorSetupResponse(response(wire), challenge, encode('{}'), onion, undefined, brokenSignal),
    'possums_tor_setup_authorization_cause_unknown');
  const failure = new TorSetupFailure(canary, canary);
  diagnostic(failure, 'possums_tor_setup_response_cause_unknown'); checks++;
  // Preserve typed primary failures by identity, including externally thrown
  // boundary faults; never classify their text or retain a raw cause.
  for (const primary of [new VerifierFailure('publisher', 'manifest'), new TorEndorsementFailure('payload', 'challenge')]) {
    const signal = new AbortController().signal;
    Object.defineProperty(signal, 'aborted', { get() { throw primary; } });
    await assert.rejects(() => verifyTorSetupResponse(response(wire), challenge, encode('{}'), onion, undefined, signal), error => error === primary);
    checks++;
  }
  const brokenBytes = new Proxy(new Uint8Array(), { get() { throw new Error(canary); } });
  rejected(brokenBytes, 'cause_unknown');
  await rejects(() => verifyTorSetupResponse(response(wire), brokenBytes, encode('{}'), onion),
    'possums_tor_setup_input_cause_unknown');

  // Production source only. Actual appraisal has no caller-injected verifier.
  for (const file of ['verifier.wasm', 'wasm_exec.cjs']) await copyFile(join(build, file), join(scratch, file));
  for (const file of ['verifier.mjs', 'verifier-worker.mjs', 'tor-endorsement.mjs', 'tor-setup.mjs'])
    await copyFile(new URL(file, sourceURL), join(scratch, file));
  let observed;
  workerThreads.Worker = class extends RealWorker {
    constructor(url, options) { super(url, options); observed = options.workerData; }
  };
  syncBuiltinESMExports();
  const production = await import(pathToFileURL(join(scratch, 'tor-setup.mjs')));
  for (const [code, stage, constraint] of backendCases)
    await rejects(() => production.verifyTorSetupResponse(response(backend(code, stage, constraint)), challenge, encode('{}'), onion), code);
  assert.equal(observed, undefined, 'backend diagnostics must stop before any SDK worker/appraisal'); checks++;
  for (const evidence of [{}, { format: canary }])
    await rejects(() => production.verifyTorSetupResponse(response({ ...wire, evidence }), challenge, encode('{}'), onion),
      'possums_v3_envelope_rejected');
  assert.equal(observed.action, 'tor_endorsement_key_v1');
  assert.equal(observed.nonce, challenge.toString('base64')); assert.equal(observed.manifest, 'e30='); checks++;
  const mutable = [response(wire), Buffer.from(challenge), encode('{}'), Buffer.from(onion)];
  const pending = production.verifyTorSetupResponse(...mutable);
  mutable.forEach(bytes => bytes.fill(9));
  await rejects(() => pending, 'possums_v3_envelope_rejected');
  assert.equal(observed.evidence, 'e30='); assert.equal(observed.nonce, challenge.toString('base64'));
  assert.equal(observed.manifest, 'e30='); checks++;
  const abort = new AbortController();
  const interrupted = production.verifyTorSetupResponse(response(wire), challenge, encode('{}'), onion, undefined, abort.signal);
  abort.abort(new Error(canary));
  await rejects(() => interrupted, 'possums_v3_bridge_interrupted');
  // Inspect the delegation seam instead of making up a positive chain to reach
  // signature/expiry checks. Those remain the existing helper's responsibility.
  const source = await readFile(new URL('tor-setup.mjs', sourceURL), 'utf8');
  assert(source.includes("import { authorizeTorEndorsement, TorEndorsementFailure } from './tor-endorsement.mjs'"));
  assert(source.includes('authorizeTorEndorsement(decoded.evidence, nonce, manifestSnapshot, onion,'));
  assert(source.includes('decoded.payload, decoded.signature, repository, signal)'));
  assert(source.includes('return Buffer.from(value)')); assert(source.includes('Buffer.from(manifest)'));
  assert(!source.includes('node:crypto')); assert(!source.includes('verifyTorV3(')); checks++;
  assert.equal(fetchCalls, 0); checks++;
} finally {
  workerThreads.Worker = RealWorker; syncBuiltinESMExports();
  globalThis.fetch = realFetch;
  await rm(scratch, { recursive: true, force: true });
}
console.log(JSON.stringify({ checks, scope: 'offline bounded decoding, public RFC signature and production WASM rejection only; no positive app-key chain or transport authorization' }));
