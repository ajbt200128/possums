// One public-evidence appraisal per isolated worker. No credential/prompt API.
import { parentPort, workerData } from 'node:worker_threads';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';

const fallback = { failure: { code: 'possums_v3_bridge_cause_unknown', stage: 'bridge', constraint: 'cause_unknown' } };
function reject() { parentPort.postMessage(fallback); }
function unavailable() { parentPort.postMessage({ failure: { code: 'possums_v3_bridge_artifact_unavailable', stage: 'bridge', constraint: 'artifact_unavailable' } }); }
try {
  createRequire(import.meta.url)('./wasm_exec.cjs');
  // Go's default JS filesystem shim prints and buffers writes. Discard them
  // inside this worker, including panic output; only the closed ABI may escape.
  globalThis.fs.writeSync = (_fd, bytes) => bytes.length;
  // The production verifier is offline. Fail closed if a dependency ever tries
  // to acquire evidence itself rather than using the caller's supplied bytes.
  globalThis.fetch = async () => { throw new Error('possums_v3_network_disabled'); };
  const go = new globalThis.Go();
  go.env = {};
  go.exit = () => {};
  globalThis.possumsV3Ready = () => {
    try {
      const output = globalThis.possumsVerifyV3(JSON.stringify(workerData));
      if (typeof output !== 'string' || output.length > 4096) return reject();
      parentPort.postMessage(JSON.parse(output));
    } catch { reject(); }
  };
  let bytes;
  try { bytes = await readFile(new URL('./verifier.wasm', import.meta.url)); }
  catch { unavailable(); process.exit(0); }
  const { instance } = await WebAssembly.instantiate(bytes, go.importObject);
  go.run(instance).catch(reject);
} catch { reject(); }
