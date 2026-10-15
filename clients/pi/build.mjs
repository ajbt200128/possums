// Invoked by build.sh from a fresh scratch source/install tree.
import { build, version as esbuildVersion } from 'esbuild';
import ts from 'typescript';
import { createHash } from 'node:crypto';
import { readFile, writeFile, mkdir, realpath, access, symlink, chmod } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = await realpath('.');
const source = path.resolve(root, '../..');
const out = process.env.PHASE02_OUT;
const sha = bytes => createHash('sha256').update(bytes).digest('hex');
const runtimePins = {};
const runtimeRoots = {};
try {
  for (const name of ['pi-ai', 'pi-coding-agent', 'pi-agent-core']) {
    const entry = fileURLToPath(import.meta.resolve(`@earendil-works/${name}`));
    const bytes = await readFile(path.join(path.dirname(entry), '..', 'package.json'));
    const metadata = JSON.parse(bytes);
    if (metadata.version !== '1.0.4') throw new Error();
    runtimePins[name] = { version: metadata.version, manifestSha256: sha(bytes) };
    runtimeRoots[name] = path.join(path.dirname(entry), '..');
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
const verifierBuild = process.env.POSSUMS_VERIFIER_BUILD;
if (!verifierBuild || !path.isAbsolute(verifierBuild)) throw new Error('matching scratch verifier build required');
let verifierReport;
try { verifierReport = JSON.parse(await readFile(path.join(verifierBuild, 'verifier-build-report.json'), 'utf8')); }
catch { throw new Error('possums_build_verifier_report_invalid: verifier packaging report could not be read/decoded; cause unknown. Rebuild the matching scratch verifier.'); }
if (verifierReport.go !== '1.27.2' || verifierReport.node !== process.version ||
    verifierReport.sdk !== 'v0.17.0-rc.1.0.20261009012920-31c57af7d7b4') throw new Error('unqualified verifier toolchain');
await mkdir(out, { recursive: true });
const verifierArtifacts = {};
for (const name of ['verifier.wasm', 'wasm_exec.cjs', 'GO-LICENSE']) {
  const bytes = await readFile(path.join(verifierBuild, name));
  if (sha(bytes) !== verifierReport.artifacts[name]?.sha256) throw new Error('verifier artifact hash mismatch');
  await writeFile(path.join(out, name), bytes);
  verifierArtifacts[name] = { sha256: sha(bytes) };
}
for (const name of ['verifier.mjs', 'verifier-worker.mjs', 'verifier.d.mts']) {
  const bytes = await readFile(name);
  if (sha(bytes) !== verifierReport.sources['clients/pi/' + name]) throw new Error('verifier source mismatch');
  await writeFile(path.join(out, name), bytes);
  verifierArtifacts[name] = { sha256: sha(bytes) };
}
await writeFile(path.join(out, 'verifier-build-report.json'), JSON.stringify(verifierReport, null, 2) + '\n');
const nativeBuild = process.env.POSSUMS_TOR_NATIVE_BUILD;
if (!nativeBuild || !path.isAbsolute(nativeBuild)) throw new Error('possums_build_tor_native_missing: provide the matching native helper build before packaging.');
let nativeReport;
try { nativeReport = JSON.parse(await readFile(path.join(nativeBuild, 'tor-native-build-report.json'), 'utf8')); }
catch { throw new Error('possums_build_tor_native_report: native helper report could not be read/decoded; cause unknown. Rebuild the matching helper.'); }
if (!nativeReport || typeof nativeReport !== 'object' || Array.isArray(nativeReport))
  throw new Error('possums_build_tor_native_schema: helper report must be an object; rebuild the matching helper.');
if (nativeReport.rust !== '1.92.0' || nativeReport.tor !== '0.47.0' ||
    nativeReport.platform !== process.platform || nativeReport.arch !== process.arch)
  throw new Error('possums_build_tor_native_platform: helper toolchain/platform differs from this package; rebuild for the intended host.');
for (const name of ['Cargo.toml', 'Cargo.lock', 'src/main.rs']) {
  const key = 'clients/pi/tor-native/' + name;
  let bytes;
  try { bytes = await readFile(path.join(source, key)); }
  catch { throw new Error('possums_build_tor_native_source_read: helper source could not be read; cause unknown. Restore the matching source tree.'); }
  if (sha(bytes) !== nativeReport.sources?.[key]) throw new Error('possums_build_tor_native_source_hash: helper source changed; rebuild before packaging.');
}
let nativeBytes;
try { nativeBytes = await readFile(path.join(nativeBuild, 'possums-tor-native')); }
catch { throw new Error('possums_build_tor_native_artifact_read: helper artifact could not be read; cause unknown. Rebuild for this host.'); }
if (sha(nativeBytes) !== nativeReport.artifacts?.['possums-tor-native']?.sha256)
  throw new Error('possums_build_tor_native_artifact_hash: helper artifact differs from its report; rebuild before packaging.');
try {
  await writeFile(path.join(out, 'possums-tor-native'), nativeBytes);
  await chmod(path.join(out, 'possums-tor-native'), 0o755);
  await writeFile(path.join(out, 'tor-native-build-report.json'), JSON.stringify(nativeReport, null, 2) + '\n');
} catch { throw new Error('possums_build_tor_native_output: helper packaging failed; cause unknown. Check the scratch output directory before rebuilding.'); }
verifierArtifacts['possums-tor-native'] = { sha256: sha(nativeBytes) };
const peers = path.join(out, 'node_modules', '@earendil-works');
await mkdir(peers, { recursive: true });
for (const [name, directory] of Object.entries(runtimeRoots)) await symlink(directory, path.join(peers, name));
const buildReport = { node: process.version, pi: '1.0.4', runtimePins, typescript: ts.version, esbuild: esbuildVersion,
  lock: sha(await readFile('package-lock.json')), verifier: verifierReport, torNative: nativeReport,
  legacyBootstrapFixture: { revision: '1b909881b468a2d5035ff2180ed5027fd2d8b9f6', sha256: sha(await readFile('legacy-bootstrap.ts')) },
  artifacts: { ...verifierArtifacts } };
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
console.log(JSON.stringify({ output: out, pi: '1.0.4', artifacts: Object.keys(buildReport.artifacts) }));
