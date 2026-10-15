// SOURCE ONLY: public response decoding/appraisal, never a connector or dispatch gate.
import { authorizeTorEndorsement, TorEndorsementFailure } from './tor-endorsement.mjs';
import { VerifierFailure } from './verifier.mjs';

const MAX_EVIDENCE = 16 * 1024 * 1024;
const MAX_RESPONSE = MAX_EVIDENCE + 2 * 1024;
const MAX_MANIFEST = 512 * 1024;
const constraints = Object.freeze({
  response: ['bytes', 'size', 'utf8', 'json', 'schema', 'payload', 'signature', 'evidence_size', 'cause_unknown'],
  input: ['cause_unknown'],
  verification: ['cause_unknown'],
  authorization: ['interrupted', 'cause_unknown'],
});
const actions = Object.freeze({
  response: 'Obtain the bounded three-field public setup response from the approved backend; do not send credentials or prompts.',
  input: 'Supply a 32-byte challenge, selected raw32 onion identity and matching bounded release manifest; do not authorize transport.',
  verification: 'Backend authenticity was not established. Check the approved verifier package; do not bypass appraisal.',
  authorization: 'No transport was authorized. Check setup before a user-controlled new public verification attempt.',
});
export class TorSetupFailure extends Error {
  constructor(stage = 'response', constraint = 'cause_unknown') {
    if (typeof stage !== 'string' || typeof constraint !== 'string' ||
        !Object.hasOwn(constraints, stage) || !constraints[stage].includes(constraint)) {
      stage = 'response'; constraint = 'cause_unknown';
    }
    const code = `possums_tor_setup_${stage}_${constraint}`;
    super(`[${code}] Stage: ${stage}; constraint: ${constraint}. ${constraint === 'cause_unknown' ? 'The underlying cause is unknown. ' : ''}${actions[stage]}`);
    delete this.stack;
    this.code = code; this.stage = stage; this.constraint = constraint;
    this.action = actions[stage];
    Object.freeze(this);
  }
}
// Closed public-backend vocabulary, separate from authenticated SDK appraisal.
function backendCode(stage, constraint) {
  if (typeof stage !== 'string' || typeof constraint !== 'string') return undefined;
  if (stage === 'acquisition' && ['cause_unknown', 'transport_unknown', 'http_status', 'body_read_unknown', 'size'].includes(constraint))
    return `possums_v3_acquisition_${constraint}`;
  const local = {
    'appraisal/verifier_support': 'tor_appraisal_unsupported',
    'helper/bounded_helper_completion': 'tor_helper_unknown',
    'helper/bounded_closed_response': 'tor_helper_invalid',
    'freshness/current_evidence': 'tor_appraisal_expired',
    'grant/derived_mounted_appraised_A_equality': 'tor_grant_mismatch',
    'input/challenge_get_empty_body': 'tor_nonce_refused',
    'response/bounded_json': 'tor_response_encoding_limit',
    'response/cause_unknown': 'tor_response_encoding_unknown',
  };
  const key = `${stage}/${constraint}`;
  if (Object.hasOwn(local, key)) return local[key];
  if (!['input', 'envelope', 'verification', 'publisher', 'keys', 'freshness'].includes(stage) ||
      (stage === 'input' && constraint === 'schema')) return undefined;
  const sdk = new VerifierFailure(stage, constraint);
  return sdk.stage === stage && sdk.constraint === constraint ? sdk.code : undefined;
}
function acquisitionStatus(status) {
  return Number.isInteger(status) && status >= 100 && status <= 999;
}
export class TorSetupBackendFailure extends Error {
  constructor(stage = 'backend', constraint = 'cause_unknown', status) {
    let code = backendCode(stage, constraint);
    if (!code || (status !== undefined &&
        (stage !== 'acquisition' || constraint !== 'http_status' || !acquisitionStatus(status)))) {
      code = 'possums_tor_setup_backend_cause_unknown'; stage = 'backend'; constraint = 'cause_unknown'; status = undefined;
    }
    let action = 'Check approved public setup evidence and backend offline before a deliberate new setup attempt; do not bypass appraisal.';
    if (code === 'tor_nonce_refused')
      action = 'Use public GET with exactly one lowercase-hex 32-byte nonce and an empty body; omit targets and extra fields.';
    else if (stage === 'grant')
      action = 'Check the current boot grant offline; do not bypass key equality.';
    else if (stage === 'freshness')
      action = 'Obtain fresh public evidence before a deliberate new setup attempt; do not bypass appraisal.';
    super(`[${code}] Stage: ${stage}; constraint: ${constraint}. Unverified public backend diagnostic only; ${constraint.endsWith('unknown') ? 'the underlying cause is unknown; ' : ''}${status === undefined ? '' : `observed acquisition HTTP status: ${status} (not necessarily the gateway response status); `}this establishes no backend authenticity, service-liveness, transport or accounting outcome. ${action}`);
    delete this.stack;
    this.code = code; this.stage = stage; this.constraint = constraint; this.action = action;
    if (status !== undefined) this.status = status;
    this.metadata_authenticated = false;
    this.transport_authorized = false;
    Object.freeze(this);
  }
}
function backendFailure(response) {
  if (!plainRecord(response.error)) throw new TorSetupFailure('response', 'schema');
  const error = response.error;
  const fields = Object.keys(error).sort().join(',');
  if (!['action,code,constraint,message,stage', 'action,code,constraint,message,stage,status'].includes(fields) ||
      typeof error.code !== 'string' || typeof error.message !== 'string' || typeof error.action !== 'string' ||
      backendCode(error.stage, error.constraint) !== error.code ||
      (Object.hasOwn(error, 'status') && (error.stage !== 'acquisition' || error.constraint !== 'http_status' || !acquisitionStatus(error.status))))
    throw new TorSetupFailure('response', 'schema');
  // Never retain/display upstream message/action, even for a recognized category.
  throw new TorSetupBackendFailure(error.stage, error.constraint, error.status);
}
function lowerHex(value, length, constraint) {
  if (typeof value !== 'string' || value.length !== length * 2 || !/^[0-9a-f]+$/.test(value))
    throw new TorSetupFailure('response', constraint);
  return Buffer.from(value, 'hex');
}
function plainRecord(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value) &&
    Object.getPrototypeOf(value) === Object.prototype;
}

export function decodeTorSetupResponse(responseBytes) {
  try {
    if (!(responseBytes instanceof Uint8Array)) throw new TorSetupFailure('response', 'bytes');
    if (responseBytes.length > MAX_RESPONSE) throw new TorSetupFailure('response', 'size');
    const snapshot = Buffer.from(responseBytes);
    let text;
    try { text = new TextDecoder('utf-8', { fatal: true }).decode(snapshot); }
    catch (error) {
      throw new TorSetupFailure('response', error instanceof TypeError ? 'utf8' : 'cause_unknown');
    }
    let response;
    try {
      // Native strict JSON syntax, not a duplicate-key/depth preflight scanner.
      response = JSON.parse(text, (key, value) => {
        if (key === '__proto__' || key === 'constructor' || key === 'prototype')
          throw new TorSetupFailure('response', 'schema');
        return value;
      });
    } catch (error) {
      if (error instanceof TorSetupFailure) throw error;
      const constraint = error instanceof SyntaxError ? 'json' : 'cause_unknown';
      throw new TorSetupFailure('response', constraint);
    }
    if (plainRecord(response) && Object.keys(response).join(',') === 'error') backendFailure(response);
    if (!plainRecord(response) || Object.keys(response).sort().join(',') !== 'evidence,payload,signature' ||
        !plainRecord(response.evidence)) throw new TorSetupFailure('response', 'schema');
    const payload = lowerHex(response.payload, 95, 'payload');
    const signature = lowerHex(response.signature, 64, 'signature');
    const evidence = Buffer.from(JSON.stringify(response.evidence), 'utf8');
    if (evidence.length > MAX_EVIDENCE) throw new TorSetupFailure('response', 'evidence_size');
    return Object.freeze({ evidence, payload, signature, transport_authorized: false });
  } catch (error) {
    throw error instanceof TorSetupFailure || error instanceof TorSetupBackendFailure ? error : new TorSetupFailure('response', 'cause_unknown');
  }
}
function identitySnapshot(value, constraint) {
  if (!(value instanceof Uint8Array) || value.length !== 32)
    throw new TorEndorsementFailure('input', constraint);
  return Buffer.from(value);
}

// B is supplied by a FUTURE qualified parser, not parsed or authenticated here.
// Facts never prove an onion checksum, key custody, socket binding or transport.
export async function verifyTorSetupResponse(responseBytes, challenge, manifest, selectedOnionIdentity,
    repository = 'ajbt200128/possums', signal) {
  let stage = 'authorization';
  try {
    if (signal !== undefined && !(signal instanceof AbortSignal)) throw new VerifierFailure('input', 'schema');
    if (signal?.aborted) throw new TorSetupFailure('authorization', 'interrupted');
    stage = 'input';
    const nonce = identitySnapshot(challenge, 'challenge');
    const onion = identitySnapshot(selectedOnionIdentity, 'onion_identity');
    if (!(manifest instanceof Uint8Array) || !manifest.length || manifest.length > MAX_MANIFEST)
      throw new VerifierFailure('publisher', 'manifest');
    const manifestSnapshot = Buffer.from(manifest);
    const decoded = decodeTorSetupResponse(responseBytes);
    // All caller-owned bytes have been copied before this first async boundary.
    stage = 'verification';
    return await authorizeTorEndorsement(decoded.evidence, nonce, manifestSnapshot, onion,
      decoded.payload, decoded.signature, repository, signal);
  } catch (error) {
    if (error instanceof VerifierFailure || error instanceof TorEndorsementFailure || error instanceof TorSetupFailure || error instanceof TorSetupBackendFailure)
      throw error;
    throw new TorSetupFailure(stage, 'cause_unknown');
  }
}
