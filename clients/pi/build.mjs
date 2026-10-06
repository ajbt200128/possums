// Invoked by build.sh from a fresh scratch source/install tree.
import { build, version as esbuildVersion } from 'esbuild';
import ts from 'typescript';
import { createHash } from 'node:crypto';
import { readFile, writeFile, mkdir, realpath, access } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = await realpath('.');
const source = path.resolve(root, '../..');
const out = process.env.PHASE02_OUT;
const sha = bytes => createHash('sha256').update(bytes).digest('hex');
const runtimePins = {};
try {
  for (const name of ['pi-ai', 'pi-coding-agent', 'pi-agent-core']) {
    const entry = fileURLToPath(import.meta.resolve(`@earendil-works/${name}`));
    const bytes = await readFile(path.join(path.dirname(entry), '..', 'package.json'));
    const metadata = JSON.parse(bytes);
    if (metadata.version !== '0.99.2') throw new Error();
    runtimePins[name] = { version: metadata.version, manifestSha256: sha(bytes) };
  }
} catch { throw new Error('unqualified Pi runtime'); }
let checkout = false;
try { await access(path.join(source, '.git')); checkout = true; } catch {}
if (checkout || !out || !path.isAbsolute(out)) throw new Error('scratch build and absolute PHASE02_OUT required');
if (process.version !== 'v24.13.0' || ts.version !== '5.9.3' || esbuildVersion !== '0.25.10') throw new Error('unqualified build toolchain');
const configuration = ts.readConfigFile('tsconfig.json', ts.sys.readFile);
if (configuration.error) throw new Error('invalid compiler configuration');
const parsed = ts.parseJsonConfigFileContent(configuration.config, ts.sys, root);
const program = ts.createProgram(parsed.fileNames, parsed.options);
const diagnostics = [...parsed.errors, ...ts.getPreEmitDiagnostics(program)];
if (diagnostics.length) {
  console.error(ts.formatDiagnosticsWithColorAndContext(diagnostics, {
    getCurrentDirectory: () => root, getCanonicalFileName: name => name, getNewLine: () => '\n',
  }));
  process.exit(1);
}
await mkdir(out, { recursive: true });
const buildReport = { node: process.version, pi: '0.99.2', runtimePins, typescript: ts.version, esbuild: esbuildVersion,
  lock: sha(await readFile('package-lock.json')), artifacts: {} };
const file = path.join(out, 'extension.mjs');
const result = await build({ entryPoints: ['index.ts'], outfile: file, bundle: true, platform: 'node',
  format: 'esm', target: 'node24', treeShaking: true, metafile: true,
  external: ['@earendil-works/*'], define: { __PHASE01_FIXTURE__: 'false' },
});
const modules = [];
for (const [name, details] of Object.entries(result.metafile.inputs).sort()) {
  modules.push({ name, sha256: sha(await readFile(name)), imports: details.imports });
}
buildReport.artifacts.extension = { sha256: sha(await readFile(file)), modules,
  external: Object.values(result.metafile.outputs).flatMap(value => value.imports).filter(value => value.external) };
await writeFile(path.join(out, 'package.json'), JSON.stringify({
  name: 'possums-pi', version: '0.2.0', private: true, type: 'module',
  pi: { extensions: ['./extension.mjs'] },
  peerDependencies: { '@earendil-works/pi-ai': '*', '@earendil-works/pi-coding-agent': '*' },
}, null, 2) + '\n');
await writeFile(path.join(out, 'build-report.json'), JSON.stringify(buildReport, null, 2) + '\n');
console.log(JSON.stringify({ output: out, pi: '0.99.2', artifacts: Object.keys(buildReport.artifacts) }));
