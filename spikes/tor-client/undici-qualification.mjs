import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';

const require = createRequire(import.meta.url);
export const undiciDirectory = dirname(require.resolve('undici/package.json'));
export const qualificationHashes = (() => {
  try { return JSON.parse(readFileSync(new URL('./patches/undici-8.11.2-hashes.json', import.meta.url))); }
  catch { return undefined; } // Preparation owns the closed qualification failure.
})();
export function digestFile(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex');
}
// Unknown/stock bytes must never silently ignore requireAuth. No import-time
// patching, downloads, dependency exception text or cached authorization.
export function isQualifiedUndici(directory = undiciDirectory) {
  try {
    if (JSON.parse(readFileSync(join(directory, 'package.json'))).version !== '8.11.2') return false;
    const entries = Object.entries(qualificationHashes);
    return entries.length === 4 && entries.every(([path, hashes]) =>
      digestFile(join(directory, path)) === hashes.qualified);
  } catch { return false; }
}
