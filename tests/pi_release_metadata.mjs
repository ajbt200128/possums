import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
const source = process.argv[2] ?? fileURLToPath(new URL('../clients/pi/', import.meta.url));
const { readReleaseMetadata, candidateManifest, DISTRIBUTION_FILES } = await import(pathToFileURL(join(source, 'release-metadata.mjs')));

const canary = 'SYNTHETIC_PRIVATE_CANARY_do_not_echo';
const metadata = readReleaseMetadata(source);
const manifest = candidateManifest(metadata);
function rejects(action, stage) {
  assert.throws(action, (error) => {
    assert.equal(error.code, `possums_release_metadata_${stage}`);
    assert.equal(error.stage, stage);
    assert.ok(error.message.startsWith(`${error.code}: `));
    assert.ok(!String(error).includes(canary));
    assert.equal(error.stack, undefined);
    assert.equal(error.cause, undefined);
    assert.ok(Object.isFrozen(error));
    return true;
  });
}
const change = (patch) => ({ ...structuredClone(metadata), ...patch });
assert.deepEqual(metadata, {
  name: '@possums/pi', version: '0.3.0-rc.1',
  description: 'Possums verified API client extension for Pi',
  nodeVersion: '24.13.0', piVersion: '1.3.0',
  repository: { type: 'git', url: 'https://github.com/ajbt200128/possums', directory: 'clients/pi' },
  license: null, redistributionApproved: false,
});
assert.deepEqual(Object.keys(manifest).sort(), [
  'description', 'engines', 'files', 'keywords', 'name', 'peerDependencies', 'pi', 'private',
  'publishConfig', 'repository', 'type', 'version',
].sort());
assert.equal(manifest.name, '@possums/pi');
assert.equal(manifest.version, '0.3.0-rc.1');
assert.equal(manifest.description, metadata.description);
assert.equal(manifest.private, true);
assert.equal(manifest.type, 'module');
assert.deepEqual(manifest.engines, { node: '24.13.0' });
assert.deepEqual(manifest.pi, { extensions: ['./extension.mjs'] });
assert.deepEqual(manifest.keywords, ['pi-package']);
assert.deepEqual(manifest.peerDependencies, { '@earendil-works/pi-ai': '*', '@earendil-works/pi-coding-agent': '*' });
assert.deepEqual(manifest.publishConfig, { access: 'public', provenance: true });
assert.deepEqual(manifest.repository, metadata.repository);
assert.deepEqual(DISTRIBUTION_FILES, [
  'GO-LICENSE', 'README.md', 'THIRD-PARTY-NOTICES', 'build-report.json', 'extension.mjs',
  'native-manifest.json',
  'native/darwin-arm64/possums-tor-native', 'native/darwin-arm64/tor-native-build-report.json',
  'native/darwin-x64/possums-tor-native', 'native/darwin-x64/tor-native-build-report.json',
  'native/linux-arm64/possums-tor-native', 'native/linux-arm64/tor-native-build-report.json',
  'native/linux-x64/possums-tor-native', 'native/linux-x64/tor-native-build-report.json',
  'package.json', 'verifier-build-report.json', 'verifier-worker.mjs', 'verifier.d.mts',
  'verifier.mjs', 'verifier.wasm', 'wasm_exec.cjs',
]);
assert.deepEqual(DISTRIBUTION_FILES, [...DISTRIBUTION_FILES].sort());
assert.equal(new Set(DISTRIBUTION_FILES).size, DISTRIBUTION_FILES.length);
assert.ok(Object.isFrozen(DISTRIBUTION_FILES));
assert.deepEqual(manifest.files, DISTRIBUTION_FILES);
for (const forbidden of ['license', 'scripts', 'dependencies', 'devDependencies', 'registryURL', 'auth']) {
  assert.ok(!Object.hasOwn(manifest, forbidden));
  rejects(() => candidateManifest(change({ [forbidden]: canary })), forbidden === 'license' ? 'approval' : 'shape');
}
assert.ok(!DISTRIBUTION_FILES.includes('LICENSE')); // No project grant until separate approval.
assert.ok(DISTRIBUTION_FILES.every((path) => !/node_modules|fixture|cache|(^|\/)tests?\/|\.\.|^\//.test(path)));
for (const field of Object.keys(metadata)) {
  const incomplete = change({});
  delete incomplete[field];
  rejects(() => candidateManifest(incomplete), 'shape');
}
for (const bad of [null, [], true, canary, 12, { ...metadata, [canary]: true }]) {
  rejects(() => candidateManifest(bad), 'shape');
}
for (const version of [null, {}, 1, '', '0.3.0', 'v0.3.0-rc.1', '01.3.0-rc.1',
  '0.03.0-rc.1', '0.3.00-rc.1', '0.3.0-01', '0.3.0-rc.01', '0.3.0-rc..1',
  '0.3.0-rc.1/../../file', '0.3.0-rc.1\n', `0.3.0-${canary}/`, '0.3.0-' + 'a'.repeat(129)]) {
  rejects(() => candidateManifest(change({ version })), 'candidate');
}
for (const version of ['0.3.0-rc.2', '0.3.0-0', '0.3.0-01a', '0.3.0-rc.1+build.01']) {
  assert.equal(candidateManifest(change({ version })).version, version);
  assert.equal(candidateManifest(change({ version })).private, true);
}
for (const patch of [{ license: 'MIT' }, { license: false }, { redistributionApproved: true },
  { redistributionApproved: 'false' }, { license: 'Apache-2.0', redistributionApproved: true }]) {
  rejects(() => candidateManifest(change(patch)), 'approval');
}
for (const patch of [{ name: canary }, { description: canary }, { nodeVersion: '>=24.13.0 <25' },
  { nodeVersion: '24.14.0' }, { piVersion: '1.3.1' }]) {
  rejects(() => candidateManifest(change(patch)), 'pins');
}
for (const patch of [{ type: 'https' }, { url: `https://${canary}.invalid` },
  { directory: '../pi' }, { directory: '/clients/pi' }, { directory: `clients/pi/${canary}` }]) {
  rejects(() => candidateManifest(change({ repository: { ...metadata.repository, ...patch } })), 'pins');
}
for (const repository of [null, [], canary, { ...metadata.repository, auth: canary }]) {
  rejects(() => candidateManifest(change({ repository })), 'shape');
}
const accessor = change({});
Object.defineProperty(accessor, 'version', { get() { throw new Error(canary); } });
rejects(() => candidateManifest(accessor), 'shape');
const hostile = new Proxy({}, { ownKeys() { throw new Error(canary); } });
rejects(() => candidateManifest(hostile), 'unknown');
// Returned objects cannot mutate subsequent manifests or the authored input.
manifest.files.push(canary);
manifest.repository.directory = canary;
assert.deepEqual(candidateManifest(metadata).files, DISTRIBUTION_FILES);
assert.equal(metadata.repository.directory, 'clients/pi');

const temporary = mkdtempSync(join(tmpdir(), 'possums-release-metadata-'));
try {
  rejects(() => readReleaseMetadata(temporary), 'read');
  rejects(() => readReleaseMetadata(null), 'read');
  const file = join(temporary, 'release.json');
  for (const text of [`{"${canary}":`, canary, '{', '']) {
    writeFileSync(file, text);
    rejects(() => readReleaseMetadata(temporary), 'parse');
  }
  for (const value of [null, [canary], { [canary]: true }]) {
    writeFileSync(file, JSON.stringify(value));
    rejects(() => readReleaseMetadata(temporary), 'shape');
  }
  writeFileSync(file, JSON.stringify(change({ redistributionApproved: true })));
  rejects(() => readReleaseMetadata(temporary), 'approval');
  writeFileSync(file, JSON.stringify(metadata));
  assert.deepEqual(readReleaseMetadata(temporary), metadata);
} finally {
  rmSync(temporary, { recursive: true, force: true });
}
console.log('Pi release metadata: offline candidate/private/schema/privacy checks passed (no publication qualification).');
