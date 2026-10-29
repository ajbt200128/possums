// Synthetic package-integrity fixtures only: no helper execution or gateway trust.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { createHash } from 'node:crypto';
import { syncBuiltinESMExports } from 'node:module';
import { execFileSync } from 'node:child_process';
const moduleURL = process.argv[2] ? pathToFileURL(path.join(process.argv[2], 'native-artifacts.mjs'))
  : new URL('../clients/pi/native-artifacts.mjs', import.meta.url);
const { NATIVE_TARGETS, MAX_NATIVE_BYTES, NativeArtifactFailure, nativeArtifactDiagnostic,
  validateNativeManifest, resolveNativeHelper } = await import(moduleURL);
const canary = 'hostile-private-value-never-display';
const sha = bytes => createHash('sha256').update(bytes).digest('hex');
function fixture(t, key = 'darwin-arm64') {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'possums-native-fixture-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const definition = NATIVE_TARGETS[key], helper = path.join(root, definition.path);
  fs.mkdirSync(path.dirname(helper), { recursive: true });
  const bytes = Buffer.alloc(64);
  if (definition.format === 'macho') {
    bytes.writeUInt32LE(0xfeedfacf, 0); bytes.writeUInt32LE(definition.cpu, 4); bytes.writeUInt32LE(2, 12);
  } else {
    Buffer.from([0x7f, 0x45, 0x4c, 0x46, 2, 1]).copy(bytes);
    bytes.writeUInt16LE(2, 16); bytes.writeUInt16LE(definition.cpu, 18);
  }
  fs.writeFileSync(helper, bytes, { mode: 0o755 });
  const manifest = { schema: 1, targets: { [key]: { target: definition.target, path: definition.path, size: bytes.length, sha256: sha(bytes) } } };
  const write = () => fs.writeFileSync(path.join(root, 'native-manifest.json'), JSON.stringify(manifest));
  write();
  const [platform, arch] = key.split('-');
  return { root, helper, manifest, write, resolve: () => resolveNativeHelper(pathToFileURL(root + '/'), platform, arch) };
}
function rejects(operation, kind) {
  assert.throws(operation, error => {
    assert.ok(error instanceof NativeArtifactFailure);
    assert.equal(error.kind, kind);
    assert.equal(error.stack, undefined);
    assert.equal(error.cause, undefined);
    assert.ok(Object.isFrozen(error));
    assert.match(error.message, /Stage: helper_start; constraint:/);
    assert.ok(!JSON.stringify(error).includes(canary));
    assert.ok(!error.message.includes(canary));
    return true;
  });
}
for (const key of Object.keys(NATIVE_TARGETS)) {
  test(`closed package-relative selection: ${key}`, t => {
    const f = fixture(t, key);
    assert.equal(f.resolve(), f.helper);
  });
}
test('import and unsupported-platform selection do no filesystem inspection', async () => {
  const original = fs.lstatSync;
  fs.lstatSync = () => { throw new Error(canary); };
  syncBuiltinESMExports();
  try {
    const module = await import(moduleURL.href + '?no-io-on-import');
    assert.equal(typeof module.resolveNativeHelper, 'function');
    rejects(() => resolveNativeHelper(new URL('file:///absent/'), 'win32', 'x64'), 'platform_unsupported');
    rejects(() => resolveNativeHelper(new URL('file:///absent/'), 'linux', 'riscv64'), 'platform_unsupported');
  } finally { fs.lstatSync = original; syncBuiltinESMExports(); }
});
test('missing selected asset is not substituted from another target', t => {
  const f = fixture(t);
  rejects(() => resolveNativeHelper(pathToFileURL(f.root + '/'), 'linux', 'x64'), 'artifact_missing');
});
for (const mutation of [
  m => { m.schema = 2; }, m => { m[canary] = true; },
  m => { m.targets['darwin-arm64'].target = canary; },
  m => { m.targets['darwin-arm64'].path = '../../' + canary; },
  m => { m.targets['darwin-arm64'].sha256 = canary; },
  m => { m.targets['darwin-arm64'].size = MAX_NATIVE_BYTES + 1; },
  m => { m.targets['darwin-arm64'].size = 63; },
  m => { m.targets['darwin-arm64'].extra = canary; },
  m => { m.targets[canary] = m.targets['darwin-arm64']; },
  m => { m.targets = {}; },
]) test('rejects a malformed, unrecognized or redirected manifest field', t => {
  const f = fixture(t); mutation(f.manifest); f.write(); rejects(f.resolve, 'artifact_mismatch');
});
test('GNU metadata cannot masquerade as a Linux musl distribution target', t => {
  const f = fixture(t, 'linux-x64'); f.manifest.targets['linux-x64'].target = 'x86_64-unknown-linux-gnu'; f.write();
  rejects(f.resolve, 'artifact_mismatch');
});
test('complete public manifest requires all four targets', t => {
  const f = fixture(t); rejects(() => validateNativeManifest(f.manifest, true), 'artifact_mismatch');
});
for (const asset of ['native-manifest.json', 'native/darwin-arm64/possums-tor-native']) test(`missing ${asset} is specific`, t => {
  const f = fixture(t); fs.unlinkSync(path.join(f.root, asset)); rejects(f.resolve, 'artifact_missing');
});
test('invalid and oversized manifest bytes do not leak input', t => {
  const f = fixture(t), file = path.join(f.root, 'native-manifest.json');
  fs.writeFileSync(file, `{${canary}`); rejects(f.resolve, 'artifact_mismatch');
  fs.writeFileSync(file, ' '.repeat(16 * 1024 + 1)); rejects(f.resolve, 'artifact_mismatch');
});
test('hash and executable permission checks are independent of metadata shape', t => {
  const f = fixture(t); fs.chmodSync(f.helper, 0o644); rejects(f.resolve, 'artifact_mismatch');
  fs.chmodSync(f.helper, 0o755); fs.appendFileSync(f.helper, canary); rejects(f.resolve, 'artifact_mismatch');
});
test('self-consistent hash cannot conceal the wrong executable architecture', t => {
  const f = fixture(t), bytes = fs.readFileSync(f.helper); bytes.writeUInt32LE(0x01000007, 4);
  fs.writeFileSync(f.helper, bytes); f.manifest.targets['darwin-arm64'].sha256 = sha(bytes); f.write();
  rejects(f.resolve, 'artifact_mismatch');
});
for (const asset of ['native-manifest.json', 'native/darwin-arm64/possums-tor-native', 'native/darwin-arm64', 'native']) test(`rejects symlinked ${asset}`, t => {
  const f = fixture(t), file = path.join(f.root, asset), saved = file + '-original';
  fs.renameSync(file, saved); fs.symlinkSync(saved, file); rejects(f.resolve, 'artifact_mismatch');
});
for (const [code, kind] of [['EACCES', 'artifact_unreadable'], ['EPERM', 'artifact_unreadable'], ['ENOENT', 'artifact_missing'], ['UNSUPPORTED', 'inspection_unknown']]) {
  test(`filesystem category remains content-free: ${code}`, t => {
    const f = fixture(t), original = fs.openSync;
    fs.openSync = () => { throw Object.assign(new Error(canary), { code }); }; syncBuiltinESMExports();
    try { rejects(f.resolve, kind); }
    finally { fs.openSync = original; syncBuiltinESMExports(); }
  });
}
for (const replacement of ['fifo', 'regular']) test(`descriptor rejects post-stat ${replacement} replacement without blocking`, t => {
  const f = fixture(t), originalOpen = fs.openSync, originalClose = fs.closeSync;
  let swapped = false, closed = 0;
  fs.openSync = (file, flags, ...args) => {
    if (file === f.helper && !swapped) {
      // Assert before making the FIFO: a missing flag fails quickly rather than hanging the suite.
      assert.equal(flags & fs.constants.O_NONBLOCK, fs.constants.O_NONBLOCK);
      swapped = true; fs.renameSync(file, file + '.original');
      if (replacement === 'fifo') execFileSync('/usr/bin/mkfifo', [file], { stdio: 'ignore' });
      else fs.copyFileSync(file + '.original', file);
    }
    return originalOpen(file, flags, ...args);
  };
  fs.closeSync = descriptor => { closed++; return originalClose(descriptor); };
  syncBuiltinESMExports();
  try { rejects(f.resolve, 'artifact_mismatch'); assert.equal(swapped, true); assert.ok(closed >= 2); }
  finally { fs.openSync = originalOpen; fs.closeSync = originalClose; syncBuiltinESMExports(); }
});
test('cleanup failure does not overwrite an observed primary filesystem category', t => {
  const f = fixture(t), originalRead = fs.readSync, originalClose = fs.closeSync;
  fs.readSync = () => { throw Object.assign(new Error(canary), { code: 'EACCES' }); };
  fs.closeSync = descriptor => { originalClose(descriptor); throw new Error(canary); }; syncBuiltinESMExports();
  try { rejects(f.resolve, 'artifact_unreadable'); }
  finally { fs.readSync = originalRead; fs.closeSync = originalClose; syncBuiltinESMExports(); }
});
test('forged diagnostics and unexpected local faults cannot supply display text', t => {
  const forged = Object.assign(Object.create(NativeArtifactFailure.prototype), { diagnostic: { action: canary } });
  assert.equal(nativeArtifactDiagnostic(forged).constraint, 'cause_unknown');
  const f = fixture(t), original = fs.openSync;
  fs.openSync = () => { throw { get code() { throw new Error(canary); } }; }; syncBuiltinESMExports();
  try { rejects(f.resolve, 'inspection_unknown'); }
  finally { fs.openSync = original; syncBuiltinESMExports(); }
  rejects(() => resolveNativeHelper(new URL('https://example.invalid/'), 'linux', 'x64'), 'inspection_unknown');
});
