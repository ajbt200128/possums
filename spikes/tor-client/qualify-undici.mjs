// Explicit offline development qualification only. Never run on client startup.
import { spawnSync } from 'node:child_process';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { undiciDirectory, qualificationHashes, digestFile, isQualifiedUndici } from './undici-qualification.mjs';

function qualify() {
  if (isQualifiedUndici()) return true;
  if (!Object.entries(qualificationHashes).every(([path, hashes]) =>
    digestFile(join(undiciDirectory, path)) === hashes.original)) return false;
  const root = dirname(fileURLToPath(import.meta.url));
  const result = spawnSync('git', ['apply', '--directory=node_modules/undici',
    'patches/undici-8.11.2-require-auth.patch'], { cwd: root, stdio: 'ignore', shell: false });
  return result.status === 0 && isQualifiedUndici();
}
try {
  if (!qualify()) throw new Error();
} catch {
  process.stderr.write('tor-spike-qualification: exact Undici 8.11.2 patch unavailable; restore spike lock dependencies and qualify again\n');
  process.exitCode = 1;
}
