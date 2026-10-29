// Local package integrity only, not gateway trust or package authentication.
// No I/O occurs on import; call only after valid explicit Tor selection.
import { constants, lstatSync, openSync, fstatSync, readSync, closeSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

export const MAX_NATIVE_BYTES = 64 * 1024 * 1024;
export const NATIVE_TARGETS = Object.freeze(Object.fromEntries([
  ['darwin-arm64', 'aarch64-apple-darwin', 'macho', 0x0100000c],
  ['darwin-x64', 'x86_64-apple-darwin', 'macho', 0x01000007],
  ['linux-arm64', 'aarch64-unknown-linux-musl', 'elf', 183],
  ['linux-x64', 'x86_64-unknown-linux-musl', 'elf', 62],
].map(([key, target, format, cpu]) => [key, Object.freeze({ target, format, cpu, path: `native/${key}/possums-tor-native` })])));
const definitions = Object.freeze({
  platform_unsupported: ['unsupported_platform', 'Tor is supported only on macOS/Linux arm64/x64. Use a supported platform; no direct fallback occurred.'],
  artifact_missing: ['artifact_missing', 'The selected Tor helper or manifest is missing. Restore the exact approved package; do not download or substitute a helper manually.'],
  artifact_unreadable: ['artifact_unreadable', 'The selected Tor package files could not be read. Check installation permissions and restore the approved package if needed.'],
  artifact_mismatch: ['artifact_mismatch', 'Tor package metadata, executable format or integrity differs from the package contract. Restore the exact approved package; do not bypass verification.'],
  inspection_unknown: ['cause_unknown', 'The local Tor package inspection did not complete; the underlying cause is unknown. Check installation permissions and share only this content-free code/stage/constraint.'],
});
const failures = new WeakSet();
export class NativeArtifactFailure extends Error {
  constructor(kind) {
    const selected = typeof kind === 'string' && Object.hasOwn(definitions, kind) ? kind : 'inspection_unknown';
    const [constraint, action] = definitions[selected];
    const code = `possums_tor_native_${selected}`;
    super(`${code}: Stage: helper_start; constraint: ${constraint}. ${action}`);
    delete this.stack;
    this.name = 'NativeArtifactFailure';
    this.kind = selected;
    this.diagnostic = Object.freeze({ code, stage: 'helper_start', constraint, action });
    failures.add(this);
    Object.freeze(this);
  }
}
export function nativeArtifactDiagnostic(error) {
  return (failures.has(error) ? error : new NativeArtifactFailure('inspection_unknown')).diagnostic;
}
const requireMatch = condition => { if (!condition) throw new NativeArtifactFailure('artifact_mismatch'); };
const record = value => value !== null && typeof value === 'object' && !Array.isArray(value);
const keys = (value, expected) => record(value) && Object.keys(value).length === expected.length && expected.every(key => Object.hasOwn(value, key));

export function validateNativeManifest(value, requireAll = false) {
  requireMatch(keys(value, ['schema', 'targets']) && value.schema === 1 && record(value.targets));
  const selected = Object.keys(value.targets);
  requireMatch(selected.length > 0 && selected.length <= 4 && (!requireAll || selected.length === 4));
  for (const key of selected) {
    requireMatch(Object.hasOwn(NATIVE_TARGETS, key));
    const entry = value.targets[key], expected = NATIVE_TARGETS[key];
    requireMatch(keys(entry, ['target', 'path', 'size', 'sha256']) && entry.target === expected.target && entry.path === expected.path
      && Number.isSafeInteger(entry.size) && entry.size >= 64 && entry.size <= MAX_NATIVE_BYTES
      && typeof entry.sha256 === 'string' && /^[a-f0-9]{64}$/.test(entry.sha256));
  }
  return value;
}
export function inspectNativeHeader(bytes, key) {
  requireMatch(Object.hasOwn(NATIVE_TARGETS, key) && bytes.length >= 64);
  const expected = NATIVE_TARGETS[key];
  if (expected.format === 'elf') {
    requireMatch(bytes.subarray(0, 4).equals(Buffer.from([0x7f, 0x45, 0x4c, 0x46]))
      && bytes[4] === 2 && bytes[5] === 1 && bytes.readUInt16LE(18) === expected.cpu
      && [2, 3].includes(bytes.readUInt16LE(16)));
  } else {
    requireMatch(bytes.readUInt32LE(0) === 0xfeedfacf && bytes.readUInt32LE(4) === expected.cpu && bytes.readUInt32LE(12) === 2);
  }
}
function ioFailure(error) {
  if (failures.has(error)) return error;
  let code;
  try { code = Object.getOwnPropertyDescriptor(error, 'code')?.value; } catch { /* Unknown cause stays unknown. */ }
  if (code === 'ENOENT' || code === 'ENOTDIR') return new NativeArtifactFailure('artifact_missing');
  if (code === 'EACCES' || code === 'EPERM') return new NativeArtifactFailure('artifact_unreadable');
  if (code === 'ELOOP') return new NativeArtifactFailure('artifact_mismatch');
  return new NativeArtifactFailure('inspection_unknown');
}
function localRead(file, limit, directory = false) {
  let descriptor, result, failure;
  try {
    const before = lstatSync(file);
    requireMatch(!before.isSymbolicLink() && (directory ? before.isDirectory() : before.isFile()));
    if (directory) return;
    descriptor = openSync(file, constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK);
    const metadata = fstatSync(descriptor);
    requireMatch(metadata.isFile() && metadata.dev === before.dev && metadata.ino === before.ino && metadata.size <= limit);
    // Fixed allocation also bounds reads if an installation changes concurrently.
    const buffer = Buffer.alloc(metadata.size + 1);
    let size = 0;
    while (size < buffer.length) {
      const count = readSync(descriptor, buffer, size, buffer.length - size, null);
      if (count === 0) break;
      size += count;
    }
    requireMatch(size === metadata.size);
    result = { metadata, bytes: buffer.subarray(0, size) };
  } catch (error) { failure = ioFailure(error); }
  finally {
    if (descriptor !== undefined) {
      try { closeSync(descriptor); }
      catch { failure ??= new NativeArtifactFailure('inspection_unknown'); }
    }
  }
  if (failure) throw failure;
  return result;
}
export function resolveNativeHelper(base = new URL('.', import.meta.url), platform = process.platform, arch = process.arch) {
  try { return resolve(base, platform, arch); }
  catch (error) { throw failures.has(error) ? error : new NativeArtifactFailure('inspection_unknown'); }
}
function resolve(base, platform, arch) {
  const key = `${platform}-${arch}`;
  if (!Object.hasOwn(NATIVE_TARGETS, key)) throw new NativeArtifactFailure('platform_unsupported');
  const root = fileURLToPath(base);
  const manifest = localRead(path.join(root, 'native-manifest.json'), 16 * 1024);
  let parsed;
  try { parsed = JSON.parse(manifest.bytes.toString('utf8')); }
  catch { throw new NativeArtifactFailure('artifact_mismatch'); }
  const entry = validateNativeManifest(parsed).targets[key];
  if (!entry) throw new NativeArtifactFailure('artifact_missing');
  localRead(path.join(root, 'native'), 0, true);
  localRead(path.join(root, 'native', key), 0, true);
  const helper = path.join(root, NATIVE_TARGETS[key].path);
  const file = localRead(helper, MAX_NATIVE_BYTES);
  requireMatch(file.metadata.mode & 0o111 && file.metadata.size === entry.size && file.bytes.length === entry.size
    && createHash('sha256').update(file.bytes).digest('hex') === entry.sha256);
  inspectNativeHeader(file.bytes, key);
  return helper;
}
