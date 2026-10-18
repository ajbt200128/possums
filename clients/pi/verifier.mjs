// Offline public-evidence bridge; this module does not initialize a provider,
// fetch evidence, authenticate, retain verdicts or authorize inference.
import { Worker } from 'node:worker_threads';

const MAX_EVIDENCE = 16 * 1024 * 1024;
const DEADLINE_MS = 30_000;
const constraints = Object.freeze({
  input: ['size', 'nonce', 'workload', 'schema', 'cause_unknown'],
  envelope: ['rejected', 'cause_unknown'],
  verification: ['rejected', 'result', 'cause_unknown'],
  publisher: ['schema', 'certificate', 'identity', 'artifact', 'manifest', 'cause_unknown'],
  freshness: ['expired'], keys: ['tls', 'hpke', 'cause_unknown'],
  bridge: ['cause_unknown', 'cleanup_cause_unknown', 'interrupted', 'deadline', 'schema', 'artifact_unavailable'],
});
const actions = Object.freeze({
  input: 'Use the fixed approved workload and a fresh 32-byte challenge.',
  envelope: 'Acquire fresh v3 evidence for the same challenge; do not downgrade or bypass verification.',
  verification: 'The SDK did not establish authentic hardware/release evidence. Do not send credentials or prompts.',
  publisher: 'Obtain the matching approved Possums release; do not relax publisher policy.',
  freshness: 'Fresh verification is required before authorizing new work; unchanged witnesses do not extend expiry.',
  keys: 'The required endorsed key was not qualified. Do not use this connection.',
  bridge: 'Check the matching verifier package and retry verification only through an authorized setup operation.',
});
export class VerifierFailure extends Error {
  constructor(stage = 'bridge', constraint = 'cause_unknown') {
    if (typeof stage !== 'string' || typeof constraint !== 'string' || !Object.hasOwn(constraints, stage) || !constraints[stage].includes(constraint)) { stage = 'bridge'; constraint = 'cause_unknown'; }
    const code = `possums_v3_${stage}_${constraint}`;
    super(`[${code}] Stage: ${stage}; constraint: ${constraint}. ${constraint.endsWith('cause_unknown') ? 'The underlying cause is unknown. ' : ''}${actions[stage]}`);
    delete this.stack;
    this.code = code; this.stage = stage; this.constraint = constraint;
    this.action = actions[stage];
    Object.freeze(this);
  }
}
function qualified(message) {
  if (!message || typeof message !== 'object' || Array.isArray(message)) throw new VerifierFailure('bridge', 'schema');
  if (Object.keys(message).join(',') === 'failure') {
    const failure = message.failure;
    if (!failure || Object.keys(failure).sort().join(',') !== 'code,constraint,stage' ||
      typeof failure.stage !== 'string' || typeof failure.constraint !== 'string' ||
      !Object.hasOwn(constraints, failure.stage) || !constraints[failure.stage].includes(failure.constraint) ||
      failure.code !== `possums_v3_${failure.stage}_${failure.constraint}`) throw new VerifierFailure('bridge', 'schema');
    throw new VerifierFailure(failure.stage, failure.constraint);
  }
  if (Object.keys(message).join(',') !== 'result') throw new VerifierFailure('bridge', 'schema');
  const result = message.result;
  const hash = value => typeof value === 'string' && /^[0-9a-f]{64}$/.test(value);
  const fields = result && Object.keys(result).sort().join(',');
  if (!result || !['freshness_expires_at_unix,hpke_key,release_digest,release_tag,tls_key_sha256', 'freshness_expires_at_unix,release_digest,release_tag,tls_key_sha256'].includes(fields) ||
    !hash(result.release_digest) || !hash(result.tls_key_sha256) ||
    typeof result.release_tag !== 'string' || result.release_tag.length > 64 || !/^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$/.test(result.release_tag) ||
    !Number.isSafeInteger(result.freshness_expires_at_unix)) throw new VerifierFailure('bridge', 'schema');
  if (!hash(result.hpke_key)) throw new VerifierFailure('keys', 'hpke');
  if (result.freshness_expires_at_unix <= Math.floor(Date.now() / 1000)) throw new VerifierFailure('freshness', 'expired');
  return Object.freeze({ ...result });
}

// Returns facts, not a connection. The consuming channel must enforce expiry,
// exact EHBP configuration equality and its actual TLS/HPKE transport binding.
// A new worker is terminated on every outcome; no public evidence is persisted.
export async function verifyV3(evidence, nonce, repository = 'ajbt200128/possums', signal, manifest) {
  return appraise(evidence, nonce, repository, signal, manifest);
}

async function appraise(evidence, nonce, repository, signal, manifest) {
  if (!(evidence instanceof Uint8Array) || !evidence.length || evidence.length > MAX_EVIDENCE) throw new VerifierFailure('input', 'size');
  if (!(nonce instanceof Uint8Array) || nonce.length !== 32) throw new VerifierFailure('input', 'nonce');
  if (typeof repository !== 'string' || repository.length > 256) throw new VerifierFailure('input', 'workload');
  if (signal !== undefined && !(signal instanceof AbortSignal)) throw new VerifierFailure('input', 'schema');
  if (manifest !== undefined && (!(manifest instanceof Uint8Array) || !manifest.length || manifest.length > 512 * 1024)) throw new VerifierFailure('publisher', 'manifest');
  if (signal?.aborted) throw new VerifierFailure('bridge', 'interrupted');
  let worker;
  try {
    worker = new Worker(new URL('./verifier-worker.mjs', import.meta.url), {
      workerData: { evidence: Buffer.from(evidence).toString('base64'), nonce: Buffer.from(nonce).toString('base64'), repository,
        ...(manifest === undefined ? {} : { manifest: Buffer.from(manifest).toString('base64') }) },
      env: {}, execArgv: [], stdout: true, stderr: true,
    });
  } catch { throw new VerifierFailure('bridge', 'artifact_unavailable'); }
  // Host/runtime failures are not diagnostic payloads, even on worker startup.
  worker.stdout.resume(); worker.stderr.resume();
  let timer, abort, result, failed = false;
  try {
    result = await new Promise((resolve, reject) => {
      let settled = false;
      const finish = (error, result) => {
        if (settled) return;
        settled = true;
        if (error) reject(error);
        else resolve(result);
      };
      abort = () => finish(new VerifierFailure('bridge', 'interrupted'));
      signal?.addEventListener('abort', abort, { once: true });
      worker.once('message', message => {
        if (signal?.aborted) return abort();
        try { finish(undefined, qualified(message)); }
        catch (error) { finish(error instanceof VerifierFailure ? error : new VerifierFailure()); }
      });
      worker.once('error', () => finish(new VerifierFailure('bridge', 'artifact_unavailable')));
      worker.once('exit', () => finish(new VerifierFailure()));
      timer = setTimeout(() => finish(new VerifierFailure('bridge', 'deadline')), DEADLINE_MS);
      if (signal?.aborted) abort();
    });
  } catch (error) {
    failed = true;
    throw error instanceof VerifierFailure ? error : new VerifierFailure();
  } finally {
    clearTimeout(timer);
    signal?.removeEventListener('abort', abort);
    // Cleanup failure cannot replace an already observed verification failure.
    try { await worker.terminate(); }
    catch { if (!failed) throw new VerifierFailure('bridge', 'cleanup_cause_unknown'); }
  }
  if (signal?.aborted) throw new VerifierFailure('bridge', 'interrupted');
  if (result.freshness_expires_at_unix <= Math.floor(Date.now() / 1000)) throw new VerifierFailure('freshness', 'expired');
  return result;
}
