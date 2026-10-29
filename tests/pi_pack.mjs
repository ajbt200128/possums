// Offline synthetic packaging only: these bytes are not four-target native acceptance.
// Optional argv[2] is pack.mjs in a scratch source tree with its pinned dependencies.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { join, dirname, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { tmpdir } from 'node:os';
import { createHash } from 'node:crypto';
import { createRequire, syncBuiltinESMExports } from 'node:module';
import childProcess from 'node:child_process';
import { gzipSync, gunzipSync } from 'node:zlib';

const source = resolve(process.argv[2] ?? fileURLToPath(new URL('../clients/pi/pack.mjs', import.meta.url)));
const { inspectDistribution, packDistribution, validateTarball } = await import(pathToFileURL(source));
const { DISTRIBUTION_FILES, candidateManifest, readReleaseMetadata } = await import(pathToFileURL(join(dirname(source), 'release-metadata.mjs')));
const require = createRequire(pathToFileURL(source));
const tar = require('tar');
assert.equal(require('tar/package.json').version, '7.5.22');
assert.equal(process.versions.node, '24.13.0');
const root = fs.realpathSync(fs.mkdtempSync(join(tmpdir(), 'possums-pi-pack-test-')));
const manifest = candidateManifest(readReleaseMetadata(dirname(source)));
let count = 0;
const mode = (path) => path.endsWith('/possums-tor-native') ? 0o755 : 0o644;
function fixture(name) {
  const dir = join(root, name);
  fs.mkdirSync(dir);
  for (const path of DISTRIBUTION_FILES) {
    fs.mkdirSync(dirname(join(dir, path)), { recursive: true });
    // Never execute the synthetic helper, extension, Wasm or verifier bytes.
    fs.writeFileSync(join(dir, path), path === 'package.json' ? JSON.stringify(manifest) : `synthetic-not-executable:${path}\n`);
    fs.chmodSync(join(dir, path), mode(path));
  }
  return dir;
}
function diagnostic(error, stage) {
  assert.equal(error.code, `possums_pi_pack_${stage}`);
  assert.equal(error.stage, stage);
  assert.equal(Object.hasOwn(error, 'stack'), false);
  assert.equal(Object.hasOwn(error, 'cause'), false);
  assert.doesNotMatch(`${String(error)} ${JSON.stringify(error)}`, /PRIVATE_CANARY|\/private\/|\/tmp\/|password|Bearer/);
  return true;
}
function badDistribution(name, mutate, stage = 'files') {
  const dir = fixture(name);
  mutate(dir);
  assert.throws(() => inspectDistribution(dir), (error) => diagnostic(error, stage));
  count++;
}
async function archive(name, mutate = () => {}, paths = DISTRIBUTION_FILES.map((path) => `package/${path}`)) {
  const cwd = join(root, `archive-${name}`);
  fs.mkdirSync(cwd);
  const dir = fixture(`archive-data-${name}`);
  fs.renameSync(dir, join(cwd, 'package'));
  mutate(cwd);
  const file = join(root, `${name}.tgz`);
  await tar.c({ cwd, file, gzip: true, portable: true, noMtime: true, noDirRecurse: true, preservePaths: true }, paths);
  return file;
}
try {
  const clean = fixture('clean');
  const inventory = inspectDistribution(clean);
  assert.equal(inventory.length, 21);
  assert.deepEqual(inventory.map((item) => item.path), DISTRIBUTION_FILES);
  for (const item of inventory) {
    const bytes = fs.readFileSync(join(clean, item.path));
    assert.equal(item.size, bytes.length);
    assert.equal(item.sha256, createHash('sha256').update(bytes).digest('hex'));
    assert.equal(item.mode, mode(item.path));
  }
  count++;
  for (const path of ['extension.mjs', 'native/darwin-x64/possums-tor-native', 'native/linux-arm64/tor-native-build-report.json']) {
    badDistribution(`missing-${count}`, (dir) => fs.unlinkSync(join(dir, path)));
  }
  for (const path of ['.npmrc', 'node_modules', 'fixtures', '.cache', 'native/foreign', 'extra-empty']) {
    badDistribution(`extra-${count}`, (dir) => {
      if (path === '.npmrc') fs.writeFileSync(join(dir, path), 'PRIVATE_CANARY');
      else fs.mkdirSync(join(dir, path));
    });
  }
  badDistribution('symlink', (dir) => { fs.unlinkSync(join(dir, 'README.md')); fs.symlinkSync('GO-LICENSE', join(dir, 'README.md')); });
  badDistribution('directory-symlink', (dir) => { fs.rmSync(join(dir, 'native/linux-x64'), { recursive: true }); fs.symlinkSync('linux-arm64', join(dir, 'native/linux-x64')); });
  badDistribution('hardlink', (dir) => { fs.unlinkSync(join(dir, 'README.md')); fs.linkSync(join(dir, 'GO-LICENSE'), join(dir, 'README.md')); });
  badDistribution('mode', (dir) => fs.chmodSync(join(dir, 'extension.mjs'), 0o755));
  badDistribution('helper-mode', (dir) => fs.chmodSync(join(dir, 'native/linux-x64/possums-tor-native'), 0o644));
  badDistribution('report-bound', (dir) => fs.truncateSync(join(dir, 'build-report.json'), 2 * 1024 * 1024 + 1));
  badDistribution('runtime-bound', (dir) => fs.truncateSync(join(dir, 'verifier.wasm'), 64 * 1024 * 1024 + 1));
  badDistribution('manifest-bound', (dir) => fs.truncateSync(join(dir, 'package.json'), 16 * 1024 + 1));
  badDistribution('native-manifest-bound', (dir) => fs.truncateSync(join(dir, 'native-manifest.json'), 16 * 1024 + 1));
  for (const change of [
    { private: false }, { scripts: { prepack: 'PRIVATE_CANARY' } }, { dependencies: { foreign: '1.0.0' } },
    { _authToken: 'PRIVATE_CANARY' }, { publishConfig: { registry: 'PRIVATE_CANARY' } }, { version: 'file:PRIVATE_CANARY' },
    { engines: { node: '24' } }, { peerDependencies: {} }, { files: ['**/*'] }, { license: 'MIT' },
  ]) {
    badDistribution(`metadata-${count}`, (dir) => fs.writeFileSync(join(dir, 'package.json'), JSON.stringify({ ...manifest, ...change })), 'metadata');
  }
  badDistribution('malformed-json', (dir) => fs.writeFileSync(join(dir, 'package.json'), '{PRIVATE_CANARY'), 'metadata');
  assert.throws(() => inspectDistribution(null), (error) => diagnostic(error, 'input'));
  count++;

  // A hostile external exception is neither retained nor formatted into diagnostics.
  const originalRead = fs.readSync;
  fs.readSync = () => { throw new Error('PRIVATE_CANARY password Bearer'); };
  syncBuiltinESMExports();
  try { assert.throws(() => inspectDistribution(clean), (error) => diagnostic(error, 'io')); count++; }
  finally { fs.readSync = originalRead; syncBuiltinESMExports(); }

  const goodTar = await archive('good');
  assert.deepEqual(await validateTarball(goodTar, inventory), inventory);
  count++;
  const cases = [
    ['content', (cwd) => fs.writeFileSync(join(cwd, 'package/extension.mjs'), 'different bytes')],
    ['same-size-content', (cwd) => { const file = join(cwd, 'package/extension.mjs'); const bytes = fs.readFileSync(file); bytes[0] ^= 1; fs.writeFileSync(file, bytes); }],
    ['mode-mismatch', (cwd) => fs.chmodSync(join(cwd, 'package/extension.mjs'), 0o755)],
    ['symlink-entry', (cwd) => { fs.unlinkSync(join(cwd, 'package/README.md')); fs.symlinkSync('GO-LICENSE', join(cwd, 'package/README.md')); }],
    ['hardlink-entry', (cwd) => { fs.unlinkSync(join(cwd, 'package/README.md')); fs.linkSync(join(cwd, 'package/GO-LICENSE'), join(cwd, 'package/README.md')); }],
    ['extra-entry', (cwd) => fs.writeFileSync(join(cwd, 'package/extra'), 'PRIVATE_CANARY'), [...DISTRIBUTION_FILES.map((path) => `package/${path}`), 'package/extra']],
    ['duplicate-entry', () => {}, [...DISTRIBUTION_FILES.map((path) => `package/${path}`), 'package/README.md']],
    ['wrong-path', () => {}, DISTRIBUTION_FILES.map((path) => path === 'README.md' ? './package/README.md' : `package/${path}`)],
    ['missing-entry', () => {}, DISTRIBUTION_FILES.filter((path) => path !== 'README.md').map((path) => `package/${path}`)],
  ];
  for (const [name, mutate, paths] of cases) {
    const file = await archive(name, mutate, paths);
    await assert.rejects(validateTarball(file, inventory), (error) => diagnostic(error, 'archive'));
    count++;
  }
  // Use tar's supported Header encoder for an unsupported-entry negative fixture.
  const unsupported = join(root, 'unsupported.tgz');
  const header = new tar.Header({ path: 'package/extra', type: 'FIFO', size: 0, mode: 0o644 });
  header.encode();
  const plain = gunzipSync(fs.readFileSync(goodTar));
  fs.writeFileSync(unsupported, gzipSync(Buffer.concat([plain.subarray(0, -1024), header.block, Buffer.alloc(1024)])));
  await assert.rejects(validateTarball(unsupported, inventory), (error) => diagnostic(error, 'archive'));
  count++;
  const trailing = join(root, 'trailing.tgz');
  fs.writeFileSync(trailing, gzipSync(Buffer.concat([plain, Buffer.from('PRIVATE_CANARY trailing bytes')])));
  await assert.rejects(validateTarball(trailing, inventory), (error) => diagnostic(error, 'archive'));
  count++;
  const identifyingHeader = new tar.Header();
  identifyingHeader.decode(plain, 0);
  identifyingHeader.uid = 7; identifyingHeader.uname = 'PRIVATE_CANARY'; identifyingHeader.encode();
  const identifying = join(root, 'identifying.tgz');
  fs.writeFileSync(identifying, gzipSync(Buffer.concat([identifyingHeader.block, plain.subarray(512)])));
  await assert.rejects(validateTarball(identifying, inventory), (error) => diagnostic(error, 'archive'));
  count++;
  const malformed = join(root, 'malformed.tgz');
  fs.writeFileSync(malformed, 'PRIVATE_CANARY password Bearer');
  await assert.rejects(validateTarball(malformed, inventory), (error) => diagnostic(error, 'archive'));
  await assert.rejects(validateTarball(goodTar, []), (error) => diagnostic(error, 'schema'));
  count += 2;

  // The only tool operation is real offline npm pack of private synthetic files.
  // Hostile inherited configuration must not reach npm, and lifecycle scripts are absent.
  const keys = ['npm_config_registry', 'npm_config_userconfig', 'NODE_OPTIONS', 'HTTPS_PROXY'];
  const saved = keys.map((key) => process.env[key]);
  keys.forEach((key) => { process.env[key] = 'PRIVATE_CANARY'; });
  let packed;
  try { packed = await packDistribution(clean, join(root, 'output')); }
  finally { keys.forEach((key, i) => { if (saved[i] === undefined) delete process.env[key]; else process.env[key] = saved[i]; }); }
  assert.equal(packed.filename, 'possums-pi-0.3.0-rc.1.tgz');
  assert.deepEqual(packed.files, inventory);
  assert.equal(packed.unpackedSize, inventory.reduce((sum, item) => sum + item.size, 0));
  const bytes = fs.readFileSync(join(root, 'output', packed.filename));
  assert.equal(packed.size, bytes.length);
  assert.equal(packed.sha256, createHash('sha256').update(bytes).digest('hex'));
  assert.equal(packed.integrity, `sha512-${createHash('sha512').update(bytes).digest('base64')}`);
  assert.doesNotMatch(JSON.stringify(packed), /\/private\/|\/tmp\/|PRIVATE_CANARY/);
  count++;
  await assert.rejects(packDistribution(clean, join(root, 'output')), (error) => diagnostic(error, 'io'));
  assert.deepEqual(fs.readFileSync(join(root, 'output', packed.filename)), bytes);
  count++;

  // A new authored private candidate version drives the manifest AND archive name.
  const alternateSource = join(root, 'alternate-source'); fs.mkdirSync(alternateSource);
  for (const name of ['pack.mjs', 'release-metadata.mjs', 'release.json']) fs.copyFileSync(join(dirname(source), name), join(alternateSource, name));
  const alternateMetadata = { ...readReleaseMetadata(dirname(source)), version: '0.3.0-rc.2' };
  fs.writeFileSync(join(alternateSource, 'release.json'), JSON.stringify(alternateMetadata));
  fs.symlinkSync(fs.realpathSync(join(dirname(source), 'node_modules')), join(alternateSource, 'node_modules'));
  const alternateDistribution = fixture('alternate-version');
  fs.writeFileSync(join(alternateDistribution, 'package.json'), JSON.stringify(candidateManifest(alternateMetadata)));
  const alternate = await import(pathToFileURL(join(alternateSource, 'pack.mjs')));
  const alternateResult = await alternate.packDistribution(alternateDistribution, join(root, 'alternate-output'));
  assert.equal(alternateResult.filename, 'possums-pi-0.3.0-rc.2.tgz'); count++;

  const originalWrite = fs.writeSync;
  fs.writeSync = () => 0; syncBuiltinESMExports();
  try { await assert.rejects(packDistribution(clean, join(root, 'zero-write')), (error) => diagnostic(error, 'io')); count++; }
  finally { fs.writeSync = originalWrite; syncBuiltinESMExports(); }

  const originalExec = childProcess.execFileSync;
  const originalRemove = fs.rmSync;
  const deferredCleanup = [];
  childProcess.execFileSync = () => { throw new Error('PRIVATE_CANARY password Bearer'); };
  fs.rmSync = (path) => { deferredCleanup.push(path); throw new Error('PRIVATE_CANARY cleanup'); };
  syncBuiltinESMExports();
  try {
    // A later cleanup fault cannot replace the known primary tool failure.
    await assert.rejects(packDistribution(clean, join(root, 'tool-failure')), (error) => diagnostic(error, 'tool'));
    count++;
    // Importing the source again must not invoke npm or perform CLI work.
    await import(`${pathToFileURL(source)}?side-effect-check`);
    count++;
  } finally {
    childProcess.execFileSync = originalExec;
    fs.rmSync = originalRemove;
    syncBuiltinESMExports();
    for (const path of deferredCleanup) fs.rmSync(path, { recursive: true, force: true });
  }
  console.log(`pi pack: ${count} offline synthetic checks passed; no real distribution/native acceptance, execution, installation or publication.`);
} finally { fs.rmSync(root, { recursive: true, force: true }); }
