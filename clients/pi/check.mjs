// Run only from the scratch source tree printed by build.sh.
import { build } from 'esbuild';
import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import { fileURLToPath, pathToFileURL } from 'node:url';
import path from 'node:path';
import assert from 'node:assert/strict';
const packageSource = path.dirname(fileURLToPath(import.meta.url));
const scratch = path.resolve(packageSource, '../../..');
const testSource = path.join(scratch, 'checks/source');
const output = path.join(scratch, 'checks/build');
const piRoot = process.env.POSSUMS_PI_ROOT;
assert(piRoot && path.isAbsolute(piRoot), 'absolute pinned Pi release directory required');
assert(process.version === 'v24.13.0' && path.basename(scratch).startsWith('possums-pi-build-'), 'run from fresh build scratch with Node 24.13.0');
await mkdir(output, { recursive: true });
const common = { bundle: true, platform: 'node', format: 'esm', target: 'node24' };
for (const [name, fixture] of [['channel-node', false], ['fixture-node', true]]) {
  await build({ ...common, entryPoints: [path.resolve(packageSource, '../../examples/phase01/transport.ts')],
    outfile: path.join(output, name + '.mjs'), define: { __PHASE01_FIXTURE__: String(fixture) } });
}
await build({ ...common, entryPoints: [path.join(packageSource, 'test-entry.ts')], outfile: path.join(output, 'pi-test.mjs'),
  external: ['@earendil-works/*'], define: { __PHASE01_FIXTURE__: 'true' } });
await mkdir(path.join(output, 'node_modules/@earendil-works'), { recursive: true });
const { symlink } = await import('node:fs/promises');
for (const name of ['pi-ai', 'pi-coding-agent', 'pi-agent-core']) {
  try { await symlink(path.join(piRoot, 'node_modules/@earendil-works', name), path.join(output, 'node_modules/@earendil-works', name)); }
  catch (error) { if (error.code !== 'EEXIST') throw error; }
}
function run(name, options) {
  try { execFileSync(process.execPath, [path.join(testSource, name)], { stdio: 'inherit',
    env: { HOME: path.join(scratch, 'home'), TMPDIR: path.join(scratch, 'tmp'), PATH: '/usr/bin:/bin', ...options } }); }
  catch { throw new Error('local check failed: ' + name); }
}
run('phase01_client.mjs', { PHASE01_BUILD: output });
run('phase02_client.mjs', { PHASE02_CLIENT_BUILD: output, PHASE02_CLIENT_INSTALL: packageSource });
run('phase02_release.mjs', { PHASE02_RELEASE_SOURCE: packageSource });
run('phase02_cache.mjs', { PHASE02_TEST_BUILD: path.join(output, 'pi-test.mjs') });
run('phase02_pi.mjs', { PHASE02_TEST_BUILD: path.join(output, 'pi-test.mjs'), PHASE02_SCRATCH: path.join(scratch, 'checks/pi-cases'), POSSUMS_PI_ROOT: piRoot });
const text = await readFile(path.join(testSource, 'phase01_transport.mjs'), 'utf8');
const start = text.indexOf('async function admissionRegressions('), end = text.indexOf('\nconst admissionNode =', start);
assert(start >= 0 && end > start, 'shared admission test body missing');
const admission = path.join(output, 'admission.mjs');
await writeFile(admission, 'export ' + text.slice(start, end) + '\n');
const { admissionRegressions } = await import(pathToFileURL(admission));
const config = Buffer.from(new Uint8Array([0, 0, 32, ...Array(32).fill(7), 0, 4, 0, 1, 0, 2])).toString('hex');
const result = await admissionRegressions({ productionURL: pathToFileURL(path.join(output, 'channel-node.mjs')).href,
  fixtureURL: pathToFileURL(path.join(output, 'fixture-node.mjs')).href,
  meta: { origin: 'https://localhost:18443', config, key: '07'.repeat(32) } });
await writeFile(path.join(output, 'admission-results.json'), JSON.stringify(result, null, 2) + '\n');
console.log(JSON.stringify(result));
console.log('Local checks passed. No live tool qualification, production attestation or end-to-end claim.');
