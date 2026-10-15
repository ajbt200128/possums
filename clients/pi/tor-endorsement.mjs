// SOURCE ONLY: no address parsing, connection, secret submission or inference.
import { createPublicKey, verify } from 'node:crypto';
import { VerifierFailure, verifyTorV3 } from './verifier.mjs';

const DOMAIN = Buffer.from('possums/tor-onion-endorsement\0', 'ascii');
const constraints = Object.freeze({
  input: ['challenge', 'onion_identity', 'payload', 'signature', 'application_key', 'cause_unknown'],
  payload: ['domain', 'version', 'challenge', 'onion_identity'],
  signature: ['rejected', 'cause_unknown'],
  verification: ['cause_unknown'],
  freshness: ['expired', 'cause_unknown'],
  authorization: ['interrupted', 'cause_unknown'],
});
const actions = Object.freeze({
  input: 'Supply exact raw32 identities/challenge, raw95 payload and raw64 signature; do not use this connection.',
  payload: 'Obtain an endorsement for the same challenge and selected parsed onion identity; do not use this connection.',
  signature: 'The endorsement signature was not established. Do not send credentials or prompts.',
  verification: 'Backend authenticity was not established. Check the approved verifier; do not bypass appraisal.',
  freshness: 'Acquire fresh evidence and an endorsement for one new setup challenge before authorizing transport.',
  authorization: 'No transport was authorized. Check the setup operation before a user-controlled new attempt.',
});
export class TorEndorsementFailure extends Error {
  constructor(stage = 'signature', constraint = 'cause_unknown') {
    if (typeof stage !== 'string' || typeof constraint !== 'string' ||
        !Object.hasOwn(constraints, stage) || !constraints[stage].includes(constraint)) {
      stage = 'signature'; constraint = 'cause_unknown';
    }
    const code = `possums_tor_endorsement_${stage}_${constraint}`;
    super(`[${code}] Stage: ${stage}; constraint: ${constraint}. ${constraint.endsWith('cause_unknown') ? 'The underlying cause is unknown. ' : ''}${actions[stage]}`);
    delete this.stack;
    this.code = code; this.stage = stage; this.constraint = constraint;
    this.action = actions[stage];
    Object.freeze(this);
  }
}
function bytes(value, length, constraint) {
  if (!(value instanceof Uint8Array) || value.length !== length)
    throw new TorEndorsementFailure('input', constraint);
  return Buffer.from(value);
}
function requestBytes(challenge, selectedOnionIdentity, payload, signature) {
  try {
    return {
      challenge: bytes(challenge, 32, 'challenge'),
      onion: bytes(selectedOnionIdentity, 32, 'onion_identity'),
      payload: bytes(payload, 95, 'payload'),
      signature: bytes(signature, 64, 'signature'),
    };
  } catch (error) {
    throw error instanceof TorEndorsementFailure ? error : new TorEndorsementFailure('input', 'cause_unknown');
  }
}
async function checkSignature(applicationKey, request) {
  try {
    const key = bytes(applicationKey, 32, 'application_key');
    const { challenge, onion, payload, signature } = request;
    if (!payload.subarray(0, 30).equals(DOMAIN)) throw new TorEndorsementFailure('payload', 'domain');
    if (payload[30] !== 1) throw new TorEndorsementFailure('payload', 'version');
    if (!payload.subarray(31, 63).equals(challenge)) throw new TorEndorsementFailure('payload', 'challenge');
    if (!payload.subarray(63).equals(onion)) throw new TorEndorsementFailure('payload', 'onion_identity');
    const publicKey = createPublicKey({ key: { kty: 'OKP', crv: 'Ed25519', x: key.toString('base64url') }, format: 'jwk' });
    const valid = await new Promise((resolve, reject) => {
      verify(null, payload, publicKey, signature, (error, valid) => {
        if (error) reject(new TorEndorsementFailure('signature', 'cause_unknown'));
        else resolve(valid);
      });
    });
    if (!valid) throw new TorEndorsementFailure('signature', 'rejected');
  } catch (error) {
    throw error instanceof TorEndorsementFailure ? error : new TorEndorsementFailure('signature', 'cause_unknown');
  }
}

// Pure cryptographic compatibility check, NOT appraisal or authorization. Even
// fabricated A/challenge/B can pass: the result explicitly authorizes NO transport.
export async function checkTorEndorsementSignature(applicationKey, challenge, selectedOnionIdentity, payload, signature) {
  await checkSignature(applicationKey, requestBytes(challenge, selectedOnionIdentity, payload, signature));
  return Object.freeze({ signature_valid: true, transport_authorized: false });
}

// A is obtained ONLY through the existing production v3 appraisal. B must come
// from a future supported onion parser for the user-selected address. These
// readonly facts do NOT establish hostname/checksum, custody or a connection.
// Future transport must enforce expiry and Tor's authenticated connection to
// that same user-selected address before sending any secrets.
export async function authorizeTorEndorsement(evidence, challenge, manifest, selectedOnionIdentity, payload, signature,
    repository = 'ajbt200128/possums', signal) {
  // Snapshot once before yielding: the SDK nonce and signed challenge cannot
  // diverge through caller mutation while appraisal/crypto runs asynchronously.
  const request = requestBytes(challenge, selectedOnionIdentity, payload, signature);
  let facts;
  try {
    facts = await verifyTorV3(evidence, request.challenge, manifest, repository, signal);
  } catch (error) {
    throw error instanceof VerifierFailure ? error : new TorEndorsementFailure('verification', 'cause_unknown');
  }
  try {
    await checkSignature(Buffer.from(facts.tor_endorsement_key, 'hex'), request);
  } catch (error) {
    throw error instanceof TorEndorsementFailure ? error : new TorEndorsementFailure('signature', 'cause_unknown');
  }
  // Crypto is asynchronous: neither a previously fresh appraisal nor an earlier
  // abort check authorizes returning facts after expiry/cancellation.
  let identityFacts;
  try {
    identityFacts = Object.freeze({ ...facts, onion_identity: request.onion.toString('hex'), transport_authorized: false });
    if (signal?.aborted) throw new TorEndorsementFailure('authorization', 'interrupted');
  } catch (error) {
    throw error instanceof TorEndorsementFailure ? error : new TorEndorsementFailure('authorization', 'cause_unknown');
  }
  try {
    if (Date.now() >= facts.freshness_expires_at_unix * 1000)
      throw new TorEndorsementFailure('freshness', 'expired');
  } catch (error) {
    throw error instanceof TorEndorsementFailure ? error : new TorEndorsementFailure('freshness', 'cause_unknown');
  }
  return identityFacts;
}
