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
import { gzipSync, gunzipSync, deflateRawSync } from 'node:zlib';

const source = resolve(process.argv[2] ?? fileURLToPath(new URL('../clients/pi/pack.mjs', import.meta.url)));
const { PackError, inspectDistribution, packDistribution, validateTarball } = await import(pathToFileURL(source));
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
  // Substitute a real FIFO immediately before open. A blocking-open regression
  // must time out in a child rather than hang this test process.
  for (const kind of ['distribution', 'archive']) {
    const dir = fixture(`fifo-${kind}`);
    const target = kind === 'distribution' ? join(dir, 'README.md') : join(root, 'fifo.tgz');
    if (kind === 'archive') fs.copyFileSync(goodTar, target);
    childProcess.execFileSync(process.execPath, ['--input-type=module', '-e', `
      import assert from 'node:assert/strict';
      import fs from 'node:fs';
      import { execFileSync } from 'node:child_process';
      import { syncBuiltinESMExports } from 'node:module';
      import { pathToFileURL } from 'node:url';
      const [source, kind, target, dir, inventoryText] = process.argv.slice(1);
      const pack = await import(pathToFileURL(source));
      const open = fs.openSync, close = fs.closeSync, stat = fs.fstatSync;
      let swapped = false, descriptor, closed = 0;
      fs.openSync = (path, flags, ...args) => {
        if (path !== target) return open(path, flags, ...args);
        assert.equal(swapped, false);
        swapped = true;
        fs.unlinkSync(target);
        execFileSync('mkfifo', [target], { stdio: 'ignore', timeout: 2000 });
        descriptor = open(path, flags, ...args);
        assert(flags & fs.constants.O_NONBLOCK);
        assert(flags & fs.constants.O_NOFOLLOW);
        return descriptor;
      };
      fs.closeSync = (fd) => { close(fd); if (fd === descriptor) closed++; };
      syncBuiltinESMExports();
      const check = (error) => error.code === 'possums_pi_pack_' + (kind === 'distribution' ? 'files' : 'archive');
      if (kind === 'distribution') assert.throws(() => pack.inspectDistribution(dir), check);
      else await assert.rejects(pack.validateTarball(target, JSON.parse(inventoryText)), check);
      assert.equal(swapped, true);
      assert.equal(closed, 1);
      assert.throws(() => stat(descriptor), (error) => error.code === 'EBADF');
    `, source, kind, target, dir, JSON.stringify(inventory)], { timeout: 10_000, stdio: 'pipe' });
    count++;
  }

  // Observe the owned FD directly: the stream must not reopen a checked path,
  // and success, validation failure and close failure must all release it.
  async function descriptorCase(name, mutate, stage, closeFault = false, valid = true) {
    const file = join(root, `descriptor-${name}.tgz`);
    fs.writeFileSync(file, valid ? fs.readFileSync(goodTar) : Buffer.from('PRIVATE_CANARY invalid archive'));
    const originalOpen = fs.openSync, originalClose = fs.closeSync, originalStream = fs.createReadStream;
    let descriptor, opens = 0, closes = 0, streams = 0;
    fs.openSync = (path, flags, ...args) => {
      const fd = originalOpen(path, flags, ...args);
      if (path === file && typeof flags === 'number' && !(flags & (fs.constants.O_WRONLY | fs.constants.O_RDWR))) { descriptor = fd; opens++; }
      return fd;
    };
    fs.closeSync = (fd) => {
      originalClose(fd);
      if (fd === descriptor) {
        closes++;
        if (closeFault) throw new Error('PRIVATE_CANARY close password Bearer');
      }
    };
    fs.createReadStream = (path, options) => {
      assert.equal(path, null);
      assert.equal(options.fd, descriptor);
      assert.equal(options.autoClose, false);
      streams++;
      const stream = originalStream(path, options);
      if (mutate) stream.once('end', () => mutate(file));
      return stream;
    };
    syncBuiltinESMExports();
    try {
      if (stage) await assert.rejects(validateTarball(file, inventory), (error) => diagnostic(error, stage));
      else assert.deepEqual(await validateTarball(file, inventory), inventory);
      assert.equal(opens, 1);
      assert.equal(streams, valid ? 1 : 0);
      assert.equal(closes, 1);
      assert.throws(() => fs.fstatSync(descriptor), (error) => error.code === 'EBADF');
      count++;
    } finally {
      fs.openSync = originalOpen; fs.closeSync = originalClose; fs.createReadStream = originalStream;
      syncBuiltinESMExports();
    }
  }
  await descriptorCase('success');
  await descriptorCase('replacement', (file) => {
    const replacement = `${file}.replacement`;
    fs.copyFileSync(goodTar, replacement);
    fs.renameSync(replacement, file);
  }, 'archive');
  await descriptorCase('in-place', (file) => {
    // Even identical bytes are not a stable snapshot after an observed write.
    fs.writeFileSync(file, fs.readFileSync(goodTar));
    fs.utimesSync(file, 1, 1);
  }, 'archive');
  await descriptorCase('invalid', undefined, 'archive', false, false);
  await descriptorCase('primary-before-close', undefined, 'archive', true, false);
  await descriptorCase('close-only', undefined, 'io', true);

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
  // The supported Header decoder locates payload padding; only these hostile
  // fixtures manually alter ignored bytes and repair the header checksum.
  const first = new tar.Header(plain);
  const hidden = Buffer.from('PRIVATE_CANARY/private/path');
  assert.ok(first.size + hidden.length < 512);
  for (const [name, offset] of [
    ['file-padding', 512 + first.size], ['uname-tail', 266], ['gname-tail', 298],
    ['path-tail', Buffer.byteLength(first.path) + 1], ['linkpath-tail', 158],
    ['prefix-tail', 346], ['unused-header', 500],
  ]) {
    const bytes = Buffer.from(plain);
    const canary = offset === 500 ? hidden.subarray(0, 12) : hidden;
    canary.copy(bytes, offset);
    if (offset < 512) {
      bytes.fill(0x20, 148, 156);
      const sum = bytes.subarray(0, 512).reduce((total, value) => total + value, 0);
      bytes.write(sum.toString(8).padStart(6, '0') + '\0 ', 148, 8, 'ascii');
      assert.equal(new tar.Header(bytes).cksumValid, true);
    }
    const file = join(root, `ignored-${name}.tgz`);
    fs.writeFileSync(file, gzipSync(bytes));
    await assert.rejects(validateTarball(file, inventory), (error) => diagnostic(error, 'archive'));
    count++;
  }
  for (const length of [0, 512, 64 * 1024 + 17]) {
    const payload = Buffer.alloc(length, 0x5a);
    const file = await archive(`payload-${length}`, cwd => fs.writeFileSync(join(cwd, 'package/GO-LICENSE'), payload));
    const expected = inventory.map(item => item.path === 'GO-LICENSE'
      ? { ...item, size: payload.length, sha256: createHash('sha256').update(payload).digest('hex') } : item);
    assert.deepEqual(await validateTarball(file, expected), expected);
    count++;
  }
  const gzip = fs.readFileSync(goodTar);
  function optionalField(bytes, flag, field) {
    const header = Buffer.from(bytes.subarray(0, 10));
    header[3] = flag;
    return Buffer.concat([header, field, bytes.subarray(10)]);
  }
  const emptyMember = gzipSync(Buffer.alloc(0));
  const comment = Buffer.from('PRIVATE_CANARY password Bearer\0');
  const badEnvelope = [
    // These three reproduce acceptance by Gunzip despite unchanged TAR bytes.
    ['zero-padding', Buffer.concat([gzip, Buffer.alloc(64)]), true],
    ['empty-member', Buffer.concat([gzip, emptyMember]), true],
    ['comment-member', Buffer.concat([gzip, optionalField(emptyMember, 16, comment)]), true],
    ['first-filename', optionalField(gzip, 8, comment), true],
    ['first-comment', optionalField(gzip, 16, comment), true],
    ['first-extra', optionalField(gzip, 4, Buffer.concat([Buffer.from([comment.length, 0]), comment])), true],
    ['raw-zero-padding', Buffer.concat([gzip.subarray(0, -8), Buffer.alloc(64), gzip.subarray(-8)])],
    ['raw-extra-data', Buffer.concat([gzip.subarray(0, -8), comment, gzip.subarray(-8)])],
    ['raw-second-stream', Buffer.concat([gzip.subarray(0, -8), deflateRawSync(Buffer.alloc(0)), gzip.subarray(-8)])],
    ['raw-truncated', Buffer.concat([gzip.subarray(0, -9), gzip.subarray(-8)])],
    ['footer-truncated', gzip.subarray(0, -1)],
    ['trailing-data', Buffer.concat([gzip, comment])],
  ];
  for (const length of [0, 9, 10, 17, 18, 19]) badEnvelope.push([`truncated-${length}`, gzip.subarray(0, length)]);
  for (const [name, offset, value] of [
    ['magic', 0, 0], ['method', 2, 0], ['text-flag', 3, 1], ['reserved-flags', 3, 0xe0],
    ['timestamp', 4, 1], ['unknown-compression-flag', 8, 0x41], ['unknown-platform', 9, 0x41],
    ['raw-invalid', 10, 7], ['crc', gzip.length - 8, gzip[gzip.length - 8] ^ 1],
    ['isize', gzip.length - 4, gzip[gzip.length - 4] ^ 1],
  ]) {
    const bytes = Buffer.from(gzip);
    bytes[offset] = value;
    badEnvelope.push([name, bytes]);
  }
  for (const [name, bytes, gunzipAccepted] of badEnvelope) {
    if (gunzipAccepted) assert.deepEqual(gunzipSync(bytes), plain);
    const file = join(root, `envelope-${name}.tgz`);
    fs.writeFileSync(file, bytes);
    await assert.rejects(validateTarball(file, inventory), (error) => diagnostic(error, 'archive'));
    count++;
  }

  // Fixed envelope reads must tolerate short reads, reject EOF/errors at either
  // boundary, close the same descriptor, and preserve failure over cleanup.
  for (const fault of ['short', 'header-eof', 'footer-eof', 'header-throw', 'footer-throw']) {
    const originalOpen = fs.openSync, originalRead = fs.readSync, originalClose = fs.closeSync;
    let descriptor, opens = 0, closes = 0;
    fs.openSync = (path, ...args) => {
      const fd = originalOpen(path, ...args);
      if (path === goodTar) { descriptor = fd; opens++; }
      return fd;
    };
    fs.readSync = (fd, buffer, offset, length, position) => {
      assert.equal(fd, descriptor);
      const boundary = position >= gzip.length - 8 ? 'footer' : 'header';
      if (fault === `${boundary}-eof`) return 0;
      if (fault === `${boundary}-throw`) throw new Error('PRIVATE_CANARY password Bearer');
      return originalRead(fd, buffer, offset, Math.min(length, 2), position);
    };
    fs.closeSync = (fd) => {
      assert.equal(fd, descriptor);
      originalClose(fd); closes++;
      if (fault !== 'short') throw new Error('PRIVATE_CANARY close');
    };
    syncBuiltinESMExports();
    try {
      if (fault === 'short') assert.deepEqual(await validateTarball(goodTar, inventory), inventory);
      else await assert.rejects(validateTarball(goodTar, inventory), (error) => diagnostic(error, 'archive'));
      assert.equal(opens, 1); assert.equal(closes, 1);
      assert.throws(() => fs.fstatSync(descriptor), (error) => error.code === 'EBADF');
      count++;
    } finally {
      fs.openSync = originalOpen; fs.readSync = originalRead; fs.closeSync = originalClose;
      syncBuiltinESMExports();
    }
  }
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

  // Real entry-point failures are bounded and print only the authored diagnostic.
  for (const args of [[], ['PRIVATE_CANARY /private/path password Bearer'], ['PRIVATE_CANARY', 'PRIVATE_CANARY', 'PRIVATE_CANARY']]) {
    const cli = childProcess.spawnSync(process.execPath, [source, ...args], { encoding: 'utf8', timeout: 10_000 });
    assert.equal(cli.error, undefined);
    assert.equal(cli.signal, null);
    assert.equal(cli.status, 1);
    assert.equal(cli.stdout, '');
    assert.equal(cli.stderr, `${new PackError('input').message}\n`);
    assert.doesNotMatch(cli.stderr, /PRIVATE_CANARY|\/private\/|password|Bearer|\bat file:/);
    count++;
  }

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
  // Replace the path after validated bytes have flowed but before acceptance.
  // Packing must reject, not return a receipt obtained by hashing a reopened path.
  const originalPackStream = fs.createReadStream;
  const racedOutput = join(root, 'replaced-output');
  let archiveStreams = 0;
  fs.createReadStream = (path, options) => {
    archiveStreams++;
    const stream = originalPackStream(path, options);
    stream.once('end', () => {
      const archivePath = join(racedOutput, packed.filename);
      const replacement = `${archivePath}.replacement`;
      fs.writeFileSync(replacement, 'PRIVATE_CANARY replacement bytes');
      fs.renameSync(replacement, archivePath);
    });
    return stream;
  };
  syncBuiltinESMExports();
  try {
    await assert.rejects(packDistribution(clean, racedOutput), (error) => diagnostic(error, 'archive'));
    assert.equal(archiveStreams, 1);
    assert.equal(fs.existsSync(racedOutput), false);
    count++;
  } finally { fs.createReadStream = originalPackStream; syncBuiltinESMExports(); }

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
