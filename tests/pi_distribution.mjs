// Offline synthetic correspondence/assembly tests only. Header fixtures are never executed.
// No native linkage, compiled provenance, execution or universal-artifact qualification is established.
// Privacy scope: PRIVACY.md data-handling boundaries, forbidden telemetry and typed failures.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import childProcess from 'node:child_process';
import { syncBuiltinESMExports } from 'node:module';
import { fileURLToPath, pathToFileURL } from 'node:url';

// argv[2] selects MODULE SOURCE DIRECTORY, never a repository/build/artifact root.
const sourceDirectory = process.argv[2] ? path.resolve(process.argv[2])
  : fileURLToPath(new URL('../clients/pi/', import.meta.url));
const load = name => import(pathToFileURL(path.join(sourceDirectory, name)).href);
const { assembleDistribution } = await load('assemble.mjs');
const { DistributionFailure, digest, publicBytes, publicJSON, nativeReport, verifierReport, clientReport,
  independentlyInspect, NATIVE_SOURCES, VERIFIER_SOURCES, RECIPE_FILES, VERIFIER_FILES } = await load('distribution-reports.mjs');
const { NATIVE_TARGETS } = await load('native-artifacts.mjs');
const { DISTRIBUTION_FILES, candidateManifest, readReleaseMetadata, MetadataError } = await load('release-metadata.mjs');
const { PackError } = await load('pack.mjs');
const releaseBytes = fs.readFileSync(path.join(sourceDirectory, 'release.json'));
const canary = 'PRIVATE_SYNTHETIC_CANARY';
const commit = 'a'.repeat(40);
const zeroHash = '0'.repeat(64);
const clone = value => structuredClone(value);
const write = (file, bytes, mode = 0o644) => {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, bytes, { mode });
  fs.chmodSync(file, mode);
};
const writeJSON = (file, value) => write(file, JSON.stringify(value));
const hashes = (repository, names) => Object.fromEntries(names.map(name => [name, digest(fs.readFileSync(path.join(repository, name)))]));
const targetKeys = Object.keys(NATIVE_TARGETS);
const firstKey = targetKeys[0];
function header(key) {
  const definition = NATIVE_TARGETS[key], bytes = Buffer.alloc(64);
  if (definition.format === 'macho') {
    bytes.writeUInt32LE(0xfeedfacf, 0); bytes.writeUInt32LE(definition.cpu, 4); bytes.writeUInt32LE(2, 12);
  } else {
    Buffer.from([0x7f, 0x45, 0x4c, 0x46, 2, 1]).copy(bytes);
    bytes.writeUInt16LE(2, 16); bytes.writeUInt16LE(definition.cpu, 18);
  }
  return bytes;
}
function fixture(t) {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'possums-synthetic-assembly-')));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const repository = path.join(root, 'repository'), nativeRoot = path.join(root, 'native-inputs');
  const sourcePackage = path.join(root, 'prepared/package'), workspace = path.join(root, 'prepared/source/clients/pi');
  const output = path.join(root, 'candidate');
  for (const directory of [repository, nativeRoot, sourcePackage, workspace]) fs.mkdirSync(directory, { recursive: true });
  const names = new Set([...NATIVE_SOURCES, ...VERIFIER_SOURCES, ...RECIPE_FILES.map(name => 'clients/pi/' + name),
    ...['assemble.mjs', 'pack.mjs', 'PACKAGE-README.md', 'THIRD-PARTY-NOTICES', 'index.ts'].map(name => 'clients/pi/' + name)]);
  for (const name of names) write(path.join(repository, name), `PUBLIC SYNTHETIC SOURCE MARKER: ${name}\n`);
  write(path.join(repository, 'clients/pi/release.json'), releaseBytes);
  writeJSON(path.join(repository, 'clients/pi/package-lock.json'), { packages: { '': {}, 'node_modules/synthetic-dependency': { version: '1.0.0' } } });
  // Source/import canaries are fake, private-to-fixture inputs, not public report fields.
  write(path.join(repository, 'clients/pi/index.ts'), `// PUBLIC SYNTHETIC SOURCE ${canary}\n`);
  const dependency = 'node_modules/synthetic-dependency/index.mjs';
  write(path.join(workspace, dependency), '// PUBLIC SYNTHETIC DEPENDENCY\n');
  const metadata = readReleaseMetadata(path.join(repository, 'clients/pi'));
  const manifest = candidateManifest(metadata);
  writeJSON(path.join(sourcePackage, 'package.json'), manifest);
  for (const name of VERIFIER_FILES) {
    const repoFile = path.join(repository, 'clients/pi', name);
    write(path.join(sourcePackage, name), fs.existsSync(repoFile) ? fs.readFileSync(repoFile) : `PUBLIC SYNTHETIC VERIFIER ARTIFACT: ${name}\n`);
  }
  write(path.join(sourcePackage, 'extension.mjs'), '// PUBLIC SYNTHETIC BUNDLE; NOT EXECUTED\n');
  const verifier = {
    go: '1.27.2', node: 'v24.13.0', sdk: 'v0.17.0-rc.1.0.20261009012920-31c57af7d7b4',
    sources: hashes(repository, VERIFIER_SOURCES),
    artifacts: Object.fromEntries([...VERIFIER_FILES.map(name => [name, { sha256: digest(fs.readFileSync(path.join(sourcePackage, name))) }]),
      ['possums-attestation', { sha256: digest('PUBLIC SYNTHETIC UNSHIPPED ARTIFACT') }]]),
  };
  writeJSON(path.join(sourcePackage, 'verifier-build-report.json'), verifier);
  const native = {};
  for (const key of targetKeys) {
    const definition = NATIVE_TARGETS[key], [platform, arch] = key.split('-'), apple = platform === 'darwin';
    const bytes = header(key);
    native[key] = {
      rust: '1.92.0', tor: '0.47.0', platform, arch, target: definition.target, distributionTarget: true,
      rustHost: apple ? definition.target : `${arch === 'arm64' ? 'aarch64' : 'x86_64'}-unknown-linux-gnu`,
      rustCommit: 'b'.repeat(40), targetStdSha256: digest('PUBLIC SYNTHETIC TARGET STANDARD LIBRARY'),
      linker: apple ? { kind: 'apple-clang', version: '17.0.0', sdk: '14.0', deploymentTarget: '14.0', abi: 'apple' }
        : { kind: 'rust-lld', abi: 'musl', selfContained: true },
      portability: apple ? { format: 'macho64', linkage: 'platform_only', deploymentTarget: '14.0', execution: 'not_tested' }
        : { format: 'elf64', linkage: 'static', execution: 'not_tested' },
      sources: hashes(repository, NATIVE_SOURCES), artifacts: { 'possums-tor-native': { sha256: digest(bytes), size: bytes.length } },
    };
    write(path.join(nativeRoot, key, 'possums-tor-native'), bytes, 0o755);
    writeJSON(path.join(nativeRoot, key, 'tor-native-build-report.json'), native[key]);
  }
  const definition = NATIVE_TARGETS[firstKey], bytes = header(firstKey);
  write(path.join(sourcePackage, definition.path), bytes, 0o755);
  writeJSON(path.join(sourcePackage, 'native-manifest.json'), { schema: 1, targets: {
    [firstKey]: { target: definition.target, path: definition.path, size: bytes.length, sha256: digest(bytes) },
  } });
  const client = {
    node: 'v24.13.0', pi: '1.3.0', runtimePins: Object.fromEntries(['pi-ai', 'pi-coding-agent', 'pi-agent-core'].map(name =>
      [name, { version: '1.3.0', manifestSha256: digest(`PUBLIC SYNTHETIC PIN: ${name}`) }])),
    typescript: '5.9.3', esbuild: '0.25.10', lock: digest(fs.readFileSync(path.join(repository, 'clients/pi/package-lock.json'))),
    recipe: { entry: 'index.ts', fixture: false, sources: hashes(repository, RECIPE_FILES.map(name => 'clients/pi/' + name)) },
    verifier: clone(verifier), torNative: clone(native[firstKey]),
    artifacts: { ...clone(verifier.artifacts), [definition.path]: { sha256: digest(bytes) },
      'native-manifest.json': { sha256: digest(fs.readFileSync(path.join(sourcePackage, 'native-manifest.json'))) },
      extension: { sha256: digest(fs.readFileSync(path.join(sourcePackage, 'extension.mjs'))),
        modules: [
          { name: 'index.ts', sha256: digest(fs.readFileSync(path.join(repository, 'clients/pi/index.ts'))), imports: [
            { path: dependency, kind: 'import-statement', original: './' + canary },
            { path: 'node:fs', kind: 'import-statement', external: true },
          ] },
          { name: dependency, sha256: digest(fs.readFileSync(path.join(workspace, dependency))), imports: [] },
        ],
        external: ['node:fs', '@earendil-works/pi-ai', '@earendil-works/pi-coding-agent/subpath'].map(name =>
          ({ path: name, kind: 'import-statement', external: true })),
      },
    },
  };
  delete client.artifacts['possums-attestation'];
  const saveClient = () => writeJSON(path.join(sourcePackage, 'build-report.json'), client);
  const saveNative = key => writeJSON(path.join(nativeRoot, key, 'tor-native-build-report.json'), native[key]);
  saveClient();
  // These must not be copied or imported, even though present beside public inputs.
  for (const name of ['node_modules/cache', 'source.ts', 'tools/compiler', 'tests/fixture', '.cache/private', 'possums-attestation']) {
    write(path.join(sourcePackage, name), canary);
  }
  return { root, repository, nativeRoot, sourcePackage, workspace, output, metadata, manifest, verifier, native, client,
    saveClient, saveNative, options: { repository, nativeRoot, sourcePackage, output } };
}
function withCommands(f, operation, behavior = {}) {
  const original = childProcess.execFileSync, calls = [];
  childProcess.execFileSync = (command, args, options) => {
    calls.push({ command, args: [...args] });
    assert.equal(options.cwd, f.repository);
    assert.deepEqual(options.stdio, ['ignore', 'pipe', 'pipe']);
    if (command === 'git') {
      assert.deepEqual(options.env, { PATH: process.env.PATH, LC_ALL: 'C', GIT_CONFIG_GLOBAL: '/dev/null',
        GIT_CONFIG_SYSTEM: '/dev/null', GIT_OPTIONAL_LOCKS: '0' });
      assert.ok(args.join(' ') === 'rev-parse HEAD' || args.join(' ') === '-c core.fsmonitor=false status --porcelain --untracked-files=all');
      const gitCalls = calls.filter(call => call.command === 'git');
      if (behavior.git) return behavior.git(args, gitCalls.length);
      return args[0] === 'rev-parse' ? commit + '\n' : '';
    }
    // No fallback to the real subprocess API: native fixtures can never execute.
    assert.equal(command, 'python3', 'only mocked Git and static Python inspection are permitted');
    assert.equal(args[0], path.join(f.repository, 'scripts/tor_native_portability.py'));
    assert.equal(args[1], '--json');
    assert.equal(args.length, 6);
    assert.equal(args[4], '--sha256'); assert.match(args[5], /^[a-f0-9]{64}$/);
    assert.equal(options.timeout, 180_000); assert.equal(options.killSignal, 'SIGKILL');
    assert.deepEqual(options.env, { PATH: process.env.PATH, LC_ALL: 'C' });
    const key = targetKeys.find(key => NATIVE_TARGETS[key].target === args[2]);
    assert.ok(key, 'static inspector receives an approved target');
    const number = calls.filter(call => call.command === 'python3').length;
    const expectedFile = number <= 4 ? path.join(f.nativeRoot, key, 'possums-tor-native') : path.join(f.output, NATIVE_TARGETS[key].path);
    assert.equal(args[3], expectedFile);
    assert.deepEqual(fs.readFileSync(args[3]).subarray(0, 32), header(key).subarray(0, 32), 'static inspection receives the expected synthetic CPU header');
    assert.equal(fs.statSync(args[3]).mode & 0o777, 0o755);
    if (behavior.inspect) return behavior.inspect({ key, file: args[3], number, sha256: args[5] });
    assert.equal(args[5], digest(fs.readFileSync(args[3])));
    return JSON.stringify({ ok: true, evidence: f.native[key].portability });
  };
  syncBuiltinESMExports();
  try { return operation(calls); }
  finally { childProcess.execFileSync = original; syncBuiltinESMExports(); }
}
function rejects(operation, stage, Class = DistributionFailure) {
  assert.throws(operation, error => {
    assert.ok(error instanceof Class, 'known typed failure must survive the boundary');
    assert.equal(error.stage, stage);
    assert.equal(error.stack, undefined);
    assert.equal(error.cause, undefined);
    assert.ok(Object.isFrozen(error));
    const prefix = Class === MetadataError ? 'possums_release_metadata_' : Class === PackError ? 'possums_pi_pack_' : 'possums_pi_distribution_';
    assert.equal(error.code, prefix + stage);
    assert.ok(!`${error.message}${JSON.stringify(error)}`.includes(canary));
    assert.ok(!error.message.includes(os.tmpdir()));
    return true;
  });
}
const checkClient = f => clientReport(f.client, f.sourcePackage, f.workspace, f.repository);
const checkNative = (f, key = firstKey) => nativeReport(f.native[key], key, fs.readFileSync(path.join(f.nativeRoot, key, 'possums-tor-native')), f.repository);
const assemble = (f, behavior) => withCommands(f, () => assembleDistribution(f.options), behavior);
function inventory(directory, relative = '') {
  return fs.readdirSync(path.join(directory, relative), { withFileTypes: true }).flatMap(entry => {
    const name = relative ? `${relative}/${entry.name}` : entry.name;
    assert.ok(!entry.isSymbolicLink());
    return entry.isDirectory() ? inventory(directory, name) : [name];
  }).sort();
}

test('SYNTHETIC SUCCESS: clean private 21-file assembly inspects all four headers before AND after copy', t => {
  const f = fixture(t);
  withCommands(f, calls => {
    const receipt = assembleDistribution(f.options);
    assert.equal(receipt.private, true);
    assert.equal(receipt.publication, 'not_attempted');
    assert.equal(receipt.qualification, 'not_established_by_assembly');
    assert.deepEqual(receipt.nativeTargets, targetKeys);
    assert.equal(receipt.files.length, 21);
    assert.deepEqual(inventory(f.output), [...DISTRIBUTION_FILES]);
    assert.equal(fs.statSync(f.output).mode & 0o777, 0o700);
    assert.deepEqual(calls.filter(call => call.command === 'python3').map(call => call.args[2]),
      [...targetKeys, ...targetKeys].map(key => NATIVE_TARGETS[key].target));
    assert.equal(calls.filter(call => call.command === 'git').length, 4);
    const report = publicJSON(path.join(f.output, 'build-report.json'));
    assert.deepEqual(Object.keys(report).sort(), ['schema', 'release', 'sourceCommit', 'sourceState', 'node', 'pi', 'runtimePins',
      'typescript', 'esbuild', 'lock', 'recipe', 'modules', 'external', 'assemblySources', 'verifier', 'torNative',
      'nativeInspection', 'qualification', 'artifacts'].sort());
    assert.equal(report.sourceCommit, commit); assert.equal(report.sourceState, 'exact_commit');
    assert.deepEqual(report.release, { name: f.metadata.name, version: f.metadata.version, private: true, redistributionApproved: false });
    assert.equal(report.qualification, 'not_established_by_assembly');
    assert.deepEqual(Object.keys(report.torNative), targetKeys);
    assert.deepEqual(Object.keys(report.artifacts).sort(), DISTRIBUTION_FILES.filter(name => name !== 'build-report.json'));
    for (const item of receipt.files) {
      const bytes = fs.readFileSync(path.join(f.output, item.path));
      const mode = item.path.endsWith('/possums-tor-native') ? 0o755 : 0o644;
      assert.deepEqual(item, { path: item.path, size: bytes.length, sha256: digest(bytes), mode });
      assert.equal(fs.statSync(path.join(f.output, item.path)).mode & 0o7777, mode);
      if (item.path !== 'build-report.json') assert.deepEqual(report.artifacts[item.path], { size: item.size, sha256: item.sha256 });
      if (item.path.endsWith('.json')) {
        const text = bytes.toString();
        for (const forbidden of [canary, f.root, sourceDirectory, '"imports"', '"original"']) assert.ok(!text.includes(forbidden));
      }
    }
    assert.deepEqual(publicJSON(path.join(f.output, 'package.json')), f.manifest);
    assert.equal(f.manifest.private, true);
    assert.equal(Object.hasOwn(f.manifest, 'scripts'), false);
    const manifest = publicJSON(path.join(f.output, 'native-manifest.json'));
    assert.deepEqual(Object.keys(manifest.targets), targetKeys);
    for (const key of targetKeys) {
      assert.deepEqual(manifest.targets[key], { target: NATIVE_TARGETS[key].target, path: NATIVE_TARGETS[key].path,
        size: 64, sha256: digest(header(key)) });
      assert.deepEqual(report.nativeInspection[key], f.native[key].portability);
      assert.equal(report.torNative[key].portability.execution, 'not_tested');
    }
  });
});

test('SYNTHETIC SUCCESS: report projection strips detailed import graphs and preserves only public names/hashes', t => {
  const f = fixture(t), projection = checkClient(f), text = JSON.stringify(projection);
  assert.equal(projection.nativeKey, firstKey);
  assert.deepEqual(Object.keys(projection.modules).sort(), ['clients/pi/index.ts', 'node_modules/synthetic-dependency/index.mjs']);
  assert.deepEqual(projection.external, ['@earendil-works/pi-ai', '@earendil-works/pi-coding-agent/subpath', 'node:fs']);
  for (const forbidden of [canary, f.root, '"imports"', '"original"']) assert.ok(!text.includes(forbidden));
  assert.deepEqual(verifierReport(f.verifier, f.sourcePackage, f.repository), f.verifier);
  for (const key of targetKeys) assert.deepEqual(checkNative(f, key), f.native[key]);
});

test('dirty synthetic snapshots never claim an exact source commit', t => {
  const f = fixture(t);
  assemble(f, { git: args => args[0] === 'rev-parse' ? commit : ' M synthetic-file\n' });
  const report = publicJSON(path.join(f.output, 'build-report.json'));
  assert.equal(report.sourceCommit, null); assert.equal(report.sourceState, 'uncommitted');
});
for (const key of targetKeys) test(`omitted mandatory synthetic target fails before output: ${key}`, t => {
  const f = fixture(t); fs.rmSync(path.join(f.nativeRoot, key), { recursive: true });
  rejects(() => assemble(f), 'missing'); assert.equal(fs.existsSync(f.output), false);
});
for (const [label, mutate, stage] of [
  ['GNU target', r => { r.target = 'x86_64-unknown-linux-gnu'; }, 'toolchain'],
  ['wrong CPU report', r => { r.arch = 'x64'; }, 'toolchain'],
  ['Rust pin', r => { r.rust = '1.91.0'; }, 'toolchain'],
  ['Tor pin', r => { r.tor = '0.46.0'; }, 'toolchain'],
  ['standard-library hash shape', r => { r.targetStdSha256 = canary; }, 'toolchain'],
  ['runtime-linkage declaration', r => { r.portability.linkage = 'gnu'; }, 'artifact'],
  ['execution claim', r => { r.portability.execution = 'passed'; }, 'artifact'],
  ['source hash', r => { r.sources[NATIVE_SOURCES[0]] = zeroHash; }, 'source'],
  ['artifact hash', r => { r.artifacts['possums-tor-native'].sha256 = zeroHash; }, 'artifact'],
  ['artifact size', r => { r.artifacts['possums-tor-native'].size++; }, 'artifact'],
  ['unknown top-level field', r => { r[canary] = canary; }, 'schema'],
  ['unknown nested linker field', r => { r.linker[canary] = canary; }, 'toolchain'],
  ['missing source', r => { delete r.sources[NATIVE_SOURCES[0]]; }, 'schema'],
]) test(`closed native report rejects ${label}`, t => {
  const f = fixture(t); mutate(f.native[firstKey]); rejects(() => checkNative(f), stage);
});
test('self-consistent artifact hash cannot conceal a wrong native CPU header', t => {
  const f = fixture(t), bytes = header('darwin-x64');
  f.native[firstKey].artifacts['possums-tor-native'].sha256 = digest(bytes);
  rejects(() => nativeReport(f.native[firstKey], firstKey, bytes, f.repository), 'artifact');
});
test('assembler preserves native report artifact rejection', t => {
  const f = fixture(t); f.native['linux-x64'].target = 'x86_64-unknown-linux-gnu'; f.saveNative('linux-x64');
  rejects(() => assemble(f), 'toolchain'); assert.equal(fs.existsSync(f.output), false);
});
for (const [label, mutate, stage] of [
  ['Go pin', r => { r.go = '1.26.0'; }, 'toolchain'],
  ['SDK pin', r => { r.sdk = canary; }, 'toolchain'],
  ['source hash', r => { r.sources[VERIFIER_SOURCES[0]] = zeroHash; }, 'source'],
  ['artifact hash', r => { r.artifacts['verifier.wasm'].sha256 = zeroHash; }, 'artifact'],
  ['extra artifact', r => { r.artifacts[canary] = { sha256: zeroHash }; }, 'schema'],
  ['hostile field', r => { r[canary] = canary; }, 'schema'],
]) test(`closed verifier report rejects ${label}`, t => {
  const f = fixture(t); mutate(f.verifier);
  rejects(() => verifierReport(f.verifier, f.sourcePackage, f.repository), stage);
});
for (const [label, mutate, stage] of [
  ['Node pin', r => { r.node = 'v24.12.0'; }, 'toolchain'],
  ['host runtime pin', r => { r.runtimePins['pi-ai'].version = '1.2.0'; }, 'toolchain'],
  ['lock hash', r => { r.lock = zeroHash; }, 'source'],
  ['recipe hash', r => { r.recipe.sources['clients/pi/build.mjs'] = zeroHash; }, 'source'],
  ['fixture recipe', r => { r.recipe.fixture = true; }, 'source'],
  ['unknown recipe field', r => { r.recipe[canary] = canary; }, 'source'],
  ['module hash', r => { r.artifacts.extension.modules[0].sha256 = zeroHash; }, 'source'],
  ['unlocked dependency', r => { r.artifacts.extension.modules[1].name = 'node_modules/unapproved/index.mjs'; }, 'source'],
  ['duplicate module', r => { r.artifacts.extension.modules.push(clone(r.artifacts.extension.modules[0])); }, 'source'],
  ['raw absolute module path', r => { r.artifacts.extension.modules[0].name = '/' + canary; }, 'source'],
  ['extension artifact hash', r => { r.artifacts.extension.sha256 = zeroHash; }, 'artifact'],
  ['unexpected module field', r => { r.artifacts.extension.modules[0][canary] = canary; }, 'source'],
  ['unapproved external package', r => { r.artifacts.extension.external[0].path = 'unapproved-package'; }, 'external'],
  ['peer prefix impersonation', r => { r.artifacts.extension.external[0].path = '@earendil-works/pi-ai-hostile'; }, 'external'],
  ['hostile external path', r => { r.artifacts.extension.external[0].path = '/' + canary; }, 'external'],
  ['unapproved internal import', r => { r.artifacts.extension.modules[0].imports[0].path = canary; }, 'external'],
  ['unapproved graph external', r => { r.artifacts.extension.modules[0].imports[1].path = 'unapproved-package'; }, 'external'],
  ['unexpected import field', r => { r.artifacts.extension.modules[0].imports[0][canary] = canary; }, 'external'],
  ['hostile original import', r => { r.artifacts.extension.modules[0].imports[0].original = '/' + canary; }, 'schema'],
  ['unexpected top-level field', r => { r[canary] = canary; }, 'schema'],
]) test(`closed client report rejects ${label}`, t => {
  const f = fixture(t); mutate(f.client); rejects(() => checkClient(f), stage);
});
test('nested verifier report must match the separate receipt exactly', t => {
  const f = fixture(t); f.client.verifier.artifacts['possums-attestation'].sha256 = zeroHash;
  rejects(() => checkClient(f), 'artifact');
});
test('source, dependency and recipe bytes are checked rather than trusting report hashes', t => {
  const f = fixture(t);
  for (const file of [path.join(f.repository, NATIVE_SOURCES[0]), path.join(f.repository, 'clients/pi/build.sh'),
    path.join(f.workspace, 'node_modules/synthetic-dependency/index.mjs')]) {
    const original = fs.readFileSync(file); fs.appendFileSync(file, canary);
    rejects(() => file.endsWith(NATIVE_SOURCES[0]) ? checkNative(f) : checkClient(f), 'source');
    fs.writeFileSync(file, original);
  }
});
test('host native artifact must agree with the separately supplied target artifact', t => {
  const f = fixture(t), file = path.join(f.nativeRoot, firstKey, 'possums-tor-native'), bytes = header(firstKey);
  bytes[63] = 1; write(file, bytes, 0o755); f.native[firstKey].artifacts['possums-tor-native'].sha256 = digest(bytes); f.saveNative(firstKey);
  // Static inspection mock still receives the unchanged valid CPU header with one harmless marker byte.
  withCommands(f, () => rejects(() => assembleDistribution(f.options), 'artifact'), {
    git: args => args[0] === 'rev-parse' ? commit : '',
    inspect: () => JSON.stringify({ ok: true, evidence: f.native[firstKey].portability }),
  });
});

for (const [label, bytes, stage] of [
  ['malformed JSON', '{' + canary, 'json'],
  ['oversize JSON', ' '.repeat(2 * 1024 * 1024 + 1), 'input'],
]) test(`${label} is bounded and content-free`, t => {
  const f = fixture(t), file = path.join(f.sourcePackage, 'build-report.json'); write(file, bytes);
  rejects(() => assemble(f), stage); assert.equal(fs.existsSync(f.output), false);
});
for (const name of ['build-report.json', 'verifier.wasm', NATIVE_TARGETS[firstKey].path]) test(`public input leaf links are rejected: ${name}`, t => {
  const f = fixture(t), file = path.join(f.sourcePackage, name), saved = file + '.saved';
  fs.renameSync(file, saved); fs.symlinkSync(saved, file); rejects(() => assemble(f), 'input');
});
test('malformed manifest and mismatched native-manifest hashes fail closed', t => {
  const f = fixture(t), file = path.join(f.sourcePackage, 'native-manifest.json');
  writeJSON(file, { schema: 1, targets: {} }); rejects(() => checkClient(f), 'artifact');
  f.client.artifacts['native-manifest.json'].sha256 = digest(fs.readFileSync(file));
  rejects(() => checkClient(f), 'artifact');
});
test('nonregular inputs are rejected BEFORE open so FIFOs cannot block assembly', () => {
  const originalStat = fs.lstatSync, originalOpen = fs.openSync;
  let opened = false;
  fs.lstatSync = () => ({ isFile: () => false, isSymbolicLink: () => false, nlink: 1 });
  fs.openSync = () => { opened = true; throw new Error(canary); };
  try { rejects(() => publicBytes('/synthetic-never-opened'), 'input'); assert.equal(opened, false); }
  finally { fs.lstatSync = originalStat; fs.openSync = originalOpen; }
});
for (const replacement of ['fifo', 'regular']) test(`public input descriptor rejects post-stat ${replacement} replacement`, t => {
  const f = fixture(t), file = path.join(f.root, 'raced-public-input'); write(file, 'PUBLIC SYNTHETIC');
  const originalOpen = fs.openSync, originalClose = fs.closeSync;
  let swapped = false, closed = 0;
  fs.openSync = (candidate, flags, ...args) => {
    if (candidate === file && !swapped) {
      assert.equal(flags & fs.constants.O_NONBLOCK, fs.constants.O_NONBLOCK);
      swapped = true; fs.renameSync(file, file + '.original');
      if (replacement === 'fifo') childProcess.execFileSync('/usr/bin/mkfifo', [file], { stdio: 'ignore' });
      else fs.copyFileSync(file + '.original', file);
    }
    return originalOpen(candidate, flags, ...args);
  };
  fs.closeSync = descriptor => { closed++; return originalClose(descriptor); };
  try { rejects(() => publicBytes(file), 'input'); assert.equal(swapped, true); assert.equal(closed, 1); }
  finally { fs.openSync = originalOpen; fs.closeSync = originalClose; }
});
test('public input hard links and hostile exported error stages stay closed', t => {
  const f = fixture(t), original = path.join(f.root, 'original'), linked = path.join(f.root, 'linked');
  write(original, 'PUBLIC SYNTHETIC'); fs.linkSync(original, linked);
  rejects(() => publicBytes(linked), 'input');
  rejects(() => { throw new MetadataError(canary); }, 'unknown', MetadataError);
  rejects(() => { throw new PackError(canary); }, 'io', PackError);
});
test('filesystem missing, nonregular, over-limit and permission failures stay typed', t => {
  const f = fixture(t), absent = path.join(f.root, canary), file = path.join(f.root, 'public-input');
  rejects(() => publicBytes(absent), 'missing');
  write(file, 'SYNTHETIC'); rejects(() => publicBytes(path.join(file, 'child')), 'missing');
  rejects(() => publicBytes(f.repository), 'input'); rejects(() => publicBytes(file, 1), 'input');
  if (process.getuid?.() !== 0) {
    fs.chmodSync(file, 0);
    try { rejects(() => publicBytes(file), 'unreadable'); }
    finally { fs.chmodSync(file, 0o644); }
  }
});

const inspectionCodes = {
  possums_build_tor_inspection_tool: 'inspection_tool', possums_build_tor_format: 'inspection_format',
  possums_build_tor_linkage: 'inspection_linkage', possums_build_tor_deployment: 'inspection_deployment',
  possums_build_tor_inspection_target: 'inspection_input', possums_build_tor_artifact: 'inspection_input',
  possums_build_tor_inspection_input: 'inspection_input', possums_build_tor_inspection_unknown: 'inspection_unknown',
  possums_build_tor_inspection_timeout: 'inspection_timeout',
};
for (const [code, stage] of Object.entries(inspectionCodes)) test(`typed static inspection failure survives assembly: ${code}`, t => {
  const f = fixture(t);
  rejects(() => assemble(f, { inspect: () => { throw Object.assign(new Error(canary), { stdout: JSON.stringify({ ok: false, code }), stderr: canary }); } }), stage);
  assert.equal(fs.existsSync(f.output), false);
});
for (const [label, response, stage] of [
  ['malformed success', () => '{' + canary, 'inspection_unknown'],
  ['unexpected success fields', () => JSON.stringify({ ok: true, evidence: {}, [canary]: canary }), 'inspection'],
  ['wrong evidence', () => JSON.stringify({ ok: true, evidence: { linkage: canary } }), 'inspection'],
  ['unknown failure code', () => { throw { stdout: JSON.stringify({ ok: false, code: canary }) }; }, 'inspection_unknown'],
  ['hostile failure fields', () => { throw { stdout: JSON.stringify({ ok: false, code: 'possums_build_tor_linkage', [canary]: canary }) }; }, 'inspection_unknown'],
  ['malformed failure JSON', () => { throw { stdout: '{' + canary }; }, 'inspection_unknown'],
  ['raw unknown exception', () => { throw new Error(canary); }, 'inspection_unknown'],
  ['parent deadline', () => { throw Object.assign(new Error(canary), { code: 'ETIMEDOUT', stderr: canary }); }, 'inspection_timeout'],
  ['primary before parent deadline', () => { throw { code: 'ETIMEDOUT', stdout: JSON.stringify({ ok: false, code: 'possums_build_tor_linkage' }) }; }, 'inspection_linkage'],
  ['hostile deadline getter', () => { throw { get code() { throw new Error(canary); } }; }, 'inspection_unknown'],
  ['throwing stdout getter', () => { throw { get stdout() { throw new Error(canary); } }; }, 'inspection_unknown'],
]) test(`static inspection rejects ${label} without hostile diagnostic text`, t => {
  const f = fixture(t); rejects(() => assemble(f, { inspect: response }), stage);
});
test('direct static inspector accepts only its exact expected evidence', t => {
  const f = fixture(t);
  withCommands(f, () => assert.deepEqual(independentlyInspect(f.repository, NATIVE_TARGETS[firstKey].target,
    path.join(f.nativeRoot, firstKey, 'possums-tor-native'), f.native[firstKey].portability), f.native[firstKey].portability));
});
for (const [name, validatedReads, native = false] of [['extension.mjs', 1], ['verifier.wasm', 2], ['native input', 1, true]]) {
  test(`assembly rejects ${name} replaced after its validated read`, t => {
    const f = fixture(t), file = native ? path.join(f.nativeRoot, firstKey, 'possums-tor-native') : path.join(f.sourcePackage, name);
    const originalOpen = fs.openSync, originalClose = fs.closeSync;
    const descriptors = new Set(); let reads = 0, replaced = false, receipt;
    fs.openSync = (candidate, flags, ...args) => {
      const descriptor = originalOpen(candidate, flags, ...args);
      if (candidate === file && typeof flags === 'number' && !(flags & (fs.constants.O_WRONLY | fs.constants.O_RDWR))) descriptors.add(descriptor);
      return descriptor;
    };
    fs.closeSync = descriptor => {
      const tracked = descriptors.delete(descriptor); originalClose(descriptor);
      if (tracked && ++reads === validatedReads) {
        const replacement = native ? header(firstKey) : Buffer.from(canary);
        if (native) replacement[63] ^= 1;
        replaced = true; fs.writeFileSync(file, replacement);
      }
    };
    const behavior = native ? { inspect: ({ file, sha256 }) => {
      assert.equal(sha256, f.native[firstKey].artifacts['possums-tor-native'].sha256);
      assert.notEqual(sha256, digest(fs.readFileSync(file)));
      throw { stdout: JSON.stringify({ ok: false, code: 'possums_build_tor_artifact' }) };
    } } : undefined;
    try {
      rejects(() => { receipt = assemble(f, behavior); }, native ? 'inspection_input' : 'artifact');
      assert.equal(replaced, true); assert.equal(receipt, undefined); assert.equal(fs.existsSync(f.output), false);
    } finally { fs.openSync = originalOpen; fs.closeSync = originalClose; syncBuiltinESMExports(); }
  });
}
test('post-copy inspection failure leaves only an explicitly unaccepted partial candidate', t => {
  const f = fixture(t); let receipt;
  rejects(() => { receipt = assemble(f, { inspect: ({ key, number }) => {
    if (number === 5) throw { stdout: JSON.stringify({ ok: false, code: 'possums_build_tor_linkage' }) };
    return JSON.stringify({ ok: true, evidence: f.native[key].portability });
  } }); }, 'inspection_linkage');
  assert.equal(receipt, undefined); assert.equal(fs.existsSync(f.output), true);
});
test('existing output is untouched, including its private canary', t => {
  const f = fixture(t); write(path.join(f.output, 'sentinel'), canary);
  rejects(() => assemble(f), 'output');
  assert.deepEqual(fs.readdirSync(f.output), ['sentinel']); assert.equal(fs.readFileSync(path.join(f.output, 'sentinel'), 'utf8'), canary);
});
test('typed unwritable output failure has no accepted receipt', t => {
  const f = fixture(t);
  if (process.getuid?.() === 0) { t.skip('permission denial requires a non-root process'); return; }
  const parent = path.join(f.root, 'unwritable'); fs.mkdirSync(parent); fs.chmodSync(parent, 0o500);
  f.options.output = path.join(parent, 'candidate');
  try { rejects(() => assemble(f), 'output_unavailable'); assert.equal(fs.existsSync(f.options.output), false); }
  finally { fs.chmodSync(parent, 0o700); }
});
test('unexpected filesystem write constraint remains a cause-unknown output failure', t => {
  const f = fixture(t); f.options.output = path.join(f.root, 'x'.repeat(256));
  rejects(() => assemble(f), 'output_unknown');
});
test('exclusive output creation cannot overwrite an output created during inspection', t => {
  const f = fixture(t);
  rejects(() => assemble(f, { inspect: ({ key, number }) => {
    if (number === 4) write(path.join(f.output, 'sentinel'), canary);
    return JSON.stringify({ ok: true, evidence: f.native[key].portability });
  } }), 'output');
  assert.deepEqual(fs.readdirSync(f.output), ['sentinel']);
});
for (const [label, git, stage, partial = false] of [
  ['changed commit', (args, number) => args[0] === 'rev-parse' ? (number === 3 ? 'c'.repeat(40) : commit) : '', 'source', true],
  ['changed dirty state', (args, number) => args[0] === 'rev-parse' ? commit : number === 4 ? ' M synthetic' : '', 'source', true],
  ['malformed commit', () => canary, 'source'],
  ['unknown Git failure', () => { throw new Error(canary); }, 'source_unknown'],
]) test(`source snapshot rejects ${label} without exposing raw Git output`, t => {
  const f = fixture(t); let receipt;
  rejects(() => { receipt = assemble(f, { git }); }, stage);
  assert.equal(receipt, undefined); assert.equal(fs.existsSync(f.output), partial);
});
for (const [label, mutate, stage] of [
  ['malformed JSON', file => write(file, '{' + canary), 'parse'],
  ['unapproved redistribution', file => { const value = JSON.parse(releaseBytes); value.redistributionApproved = true; writeJSON(file, value); }, 'approval'],
]) test(`known metadata ${label} failure survives assembler boundary`, t => {
  const f = fixture(t); mutate(path.join(f.repository, 'clients/pi/release.json'));
  rejects(() => assemble(f), stage, MetadataError); assert.equal(fs.existsSync(f.output), false);
});
for (const [label, mutate, stage] of [
  ['extra file', f => write(path.join(f.output, 'unapproved-source.ts'), canary), 'files'],
  ['wrong mode', f => fs.chmodSync(path.join(f.output, 'extension.mjs'), 0o755), 'files'],
  ['wrong private manifest', f => { const value = clone(f.manifest); value.private = false; writeJSON(path.join(f.output, 'package.json'), value); }, 'metadata'],
]) test(`known pack ${label} failure survives assembler boundary; partial output is unaccepted`, t => {
  const f = fixture(t); let receipt;
  rejects(() => { receipt = assemble(f, { inspect: ({ key, number }) => {
    if (number === 8) mutate(f);
    return JSON.stringify({ ok: true, evidence: f.native[key].portability });
  } }); }, stage, PackError);
  assert.equal(receipt, undefined);
});
