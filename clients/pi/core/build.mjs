// Run from a fresh scratch copy AFTER npm ci --ignore-scripts. Never installs.
import { build, version as esbuildVersion } from 'esbuild';
import { createHash } from 'node:crypto';
import { readFile, writeFile, mkdir, realpath } from 'node:fs/promises';
import path from 'node:path';
const root = await realpath('.');
const out = process.env.PHASE01_OUT;
if (!out || !path.isAbsolute(out) || root.includes('/projects/possums')) throw new Error('scratch build required');
if (process.version !== 'v24.13.0' || esbuildVersion !== '0.25.10') throw new Error('unqualified toolchain');
await mkdir(out, { recursive: true });
const sha = b => createHash('sha256').update(b).digest('hex');
const tools = {};
for (const file of ['node_modules/typescript/lib/_tsc.js', 'node_modules/esbuild/lib/main.js',
  `node_modules/@esbuild/${process.platform}-${process.arch}/bin/esbuild`,
  'node_modules/playwright/index.mjs', 'node_modules/playwright-core/browsers.json']) {
  tools[file] = sha(await readFile(file));
}
const manifest = { node: { version: process.version, executable: await realpath(process.execPath), sha256: sha(await readFile(process.execPath)) }, tools, lock: sha(await readFile('package-lock.json')), builds: {} };
for (const platform of ['node', 'browser']) {
  for (const fixture of [false, true]) {
    const name = `${fixture ? 'fixture' : 'channel'}-${platform}`;
    const file = path.join(out, name + '.mjs');
    const result = await build({ entryPoints: ['transport.ts'], outfile: file,
      bundle: true, platform, format: 'esm', target: 'es2022', metafile: true,
      define: { __PHASE01_FIXTURE__: String(fixture) }, treeShaking: true,
      plugins: platform === 'browser' ? [{ name: 'reject-legacy-zlib', setup(b) {
        // The retained browser reference factory cannot run the Node/WASM v3
        // adapter. Keep legacy factories buildable; v3 itself fails closed.
        b.onResolve({ filter: /\/clients\/pi\/v3\.js$/ }, () => ({ path: 'v3', namespace: 'unavailable' }));
        b.onResolve({ filter: /^zlib$/ }, () => ({ path: 'zlib', namespace: 'unavailable' }));
        b.onLoad({ filter: /.*/, namespace: 'unavailable' }, args => ({ resolveDir: root, contents: args.path === 'v3'
          ? `import { DiagnosticFailure } from ${JSON.stringify(path.join(root, 'limits.ts'))}; export const V3_EVIDENCE_LIMIT = 16 * 1024 * 1024; export async function qualifyV3(){throw new DiagnosticFailure('trust','policy');}`
          : 'export function gunzipSync(){throw new Error("DecompressionStream required")}' }));
      } }] : [],
    });
    const modules = [];
    for (const [name, details] of Object.entries(result.metafile.inputs).sort()) {
      modules.push({ name, sha256: name.startsWith('unavailable:') ? null : sha(await readFile(name)), imports: details.imports });
    }
    manifest.builds[name] = { sha256: sha(await readFile(file)), modules,
      external: Object.values(result.metafile.outputs).flatMap(o => o.imports).filter(i => i.external) };
  }
}
await writeFile(path.join(out, 'resolved-modules.json'), JSON.stringify(manifest, null, 2) + '\n');
console.log(JSON.stringify({ builds: Object.fromEntries(Object.entries(manifest.builds).map(([k,v]) => [k,{sha256:v.sha256,modules:v.modules.length}])) }));
