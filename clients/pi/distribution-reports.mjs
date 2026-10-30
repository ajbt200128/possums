// Build-only public projection. Reports are correspondence evidence, not signatures.
import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { isBuiltin } from 'node:module';
import { execFileSync } from 'node:child_process';
import { isDeepStrictEqual } from 'node:util';
import { NATIVE_TARGETS, MAX_NATIVE_BYTES, inspectNativeHeader } from './native-artifacts.mjs';

const messages = Object.freeze({
  input: 'Expected a bounded regular public build input, not a link or special file. Restore the exact candidate inputs.',
  missing: 'A required public build input is missing. Supply every required artifact; no target may be omitted.',
  unreadable: 'A public build input could not be read. Check local permissions without sharing paths or file contents.',
  read_unknown: 'Public build input reading failed; cause unknown. Check scratch storage and report only this code.',
  json: 'A public build report is not valid JSON. Rebuild it from the reviewed source; do not edit receipts to pass.',
  schema: 'A public report differs from the closed schema. Restore the reviewed producer; no raw fields may ship.',
  source: 'Reported public source or dependency bytes differ from the reviewed checkout. Rebuild all affected inputs together.',
  toolchain: 'Reported toolchain, SDK or target differs from the candidate contract. Use the exact pinned build environment.',
  artifact: 'Reported artifact bytes or executable identity differ. Discard this candidate and rebuild; do not bypass inspection.',
  external: 'The bundle declares an unapproved external import. Keep Pi host peers external and bundle the reviewed non-host closure.',
  inspection: 'Independent inspection evidence differs from the reported target/linkage contract. Rebuild with the pinned distribution shell; no execution qualification is claimed.',
  inspection_tool: 'The native inspection tool could not complete; its underlying cause is unknown. Provide pinned readelf and Apple lipo/otool; do not accept uninspected bytes.',
  inspection_timeout: 'Independent native inspection exceeded its local time bound. Check the pinned tools and exact input files; no artifact was accepted or executed.',
  inspection_format: 'Independent inspection rejected the executable format or architecture. Rebuild the exact approved target; a filename or manifest label is not evidence.',
  inspection_linkage: 'Independent inspection rejected runtime dependencies or search paths. Rebuild with static musl or approved Apple platform libraries; do not bundle a loader workaround.',
  inspection_deployment: 'Independent inspection could not establish the approved macOS 14.0 deployment metadata. Rebuild with the pinned SDK; inspection does not prove execution on that OS.',
  inspection_input: 'Independent inspection requires an approved target and regular executable bytes matching the validated input. Restore all four exact target inputs; no artifact was executed.',
  inspection_unknown: 'Independent native inspection failed; cause unknown. Check the pinned inspector and tools, keep this candidate private, and report only this code.',
  output: 'Distribution output must be a new directory; existing files will not be overwritten. Select a fresh scratch destination.',
  output_unknown: 'Distribution writing failed; cause unknown. Any partial scratch output is unaccepted. Check local storage before rebuilding; no publication was attempted.',
  output_unavailable: 'The scratch destination is not writable. Select a fresh writable local directory; any partial output is unaccepted and no publication was attempted.',
  output_storage: 'Local storage or quota cannot complete the candidate. Free scratch capacity before rebuilding; partial output is unaccepted and no publication was attempted.',
  source_unknown: 'The exact public source snapshot could not be established; cause unknown. Use the reviewed checkout and pinned Git tools; no release provenance is claimed.',
  assembly_unknown: 'Distribution validation failed; cause unknown. Keep the candidate private and check the pinned producer and public inputs; no publication was attempted.',
});
const authored = new WeakSet();
export class DistributionFailure extends Error {
  constructor(stage) {
    const selected = typeof stage === 'string' && Object.hasOwn(messages, stage) ? stage : 'read_unknown';
    super(`possums_pi_distribution_${selected}: Stage: distribution_${selected}. ${messages[selected]}`);
    delete this.stack;
    this.code = `possums_pi_distribution_${selected}`;
    this.stage = selected;
    authored.add(this);
    Object.freeze(this);
  }
}
export const requireDistribution = (condition, stage = 'schema') => { if (!condition) throw new DistributionFailure(stage); };
export const digest = bytes => createHash('sha256').update(bytes).digest('hex');
const hash = value => typeof value === 'string' && /^[a-f0-9]{64}$/.test(value);
const exact = (value, fields) => value !== null && typeof value === 'object' && !Array.isArray(value)
  && Object.keys(value).length === fields.length && fields.every(key => Object.hasOwn(value, key));
const version = value => typeof value === 'string' && /^[0-9]+\.[0-9]+(?:\.[0-9]+)?$/.test(value) && value.length < 32;
export function publicBytes(file, limit = MAX_NATIVE_BYTES) {
  let descriptor, result, failure;
  try {
    const before = fs.lstatSync(file);
    requireDistribution(before.isFile() && !before.isSymbolicLink() && before.nlink === 1, 'input');
    descriptor = fs.openSync(file, fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW | fs.constants.O_NONBLOCK);
    const metadata = fs.fstatSync(descriptor);
    requireDistribution(metadata.isFile() && metadata.nlink === 1 && metadata.dev === before.dev && metadata.ino === before.ino
      && metadata.size <= limit, 'input');
    const buffer = Buffer.alloc(metadata.size + 1);
    let size = 0;
    while (size < buffer.length) {
      const count = fs.readSync(descriptor, buffer, size, buffer.length - size, null);
      if (!count) break;
      size += count;
    }
    requireDistribution(size === metadata.size, 'input');
    result = buffer.subarray(0, size);
  } catch (error) {
    if (authored.has(error)) failure = error;
    else {
      let code;
      try { code = Object.getOwnPropertyDescriptor(error, 'code')?.value; } catch { /* Keep unknown causes unknown. */ }
      failure = new DistributionFailure(code === 'ENOENT' || code === 'ENOTDIR' ? 'missing'
        : code === 'EACCES' || code === 'EPERM' ? 'unreadable' : 'read_unknown');
    }
  } finally {
    if (descriptor !== undefined) {
      try { fs.closeSync(descriptor); }
      catch { failure ??= new DistributionFailure('read_unknown'); }
    }
  }
  if (failure) throw failure;
  return result;
}
export function publicJSON(file) {
  const bytes = publicBytes(file, 2 * 1024 * 1024);
  try { return JSON.parse(bytes.toString('utf8')); }
  catch { throw new DistributionFailure('json'); }
}
function sources(value, expected, repository) {
  requireDistribution(exact(value, expected));
  for (const name of expected) {
    requireDistribution(hash(value[name]) && digest(publicBytes(path.join(repository, name))) === value[name], 'source');
  }
  return Object.fromEntries(expected.map(name => [name, value[name]]));
}
export const NATIVE_SOURCES = Object.freeze([
  'clients/pi/tor-native/Cargo.toml', 'clients/pi/tor-native/Cargo.lock', 'clients/pi/tor-native/src/main.rs',
  'scripts/build-tor-native.sh', 'scripts/tor_native_build.py', 'scripts/tor_native_portability.py',
]);
export const VERIFIER_SOURCES = Object.freeze([
  'attestation-helper/cmd/verifier-wasm/main_js.go', 'attestation-helper/go.mod', 'attestation-helper/go.sum',
  'attestation-helper/main.go', 'attestation-helper/main_test.go', 'attestation-helper/readiness_test.go',
  'attestation-helper/verifier/tor_test.go', 'attestation-helper/verifier/verifier.go',
  'attestation-helper/verifier/verifier_test.go', 'attestation-helper/verifier/wire.go',
  'clients/pi/verifier-worker.mjs', 'clients/pi/verifier.d.mts', 'clients/pi/verifier.mjs',
  'scripts/build-v3-verifier.sh', 'clients/pi/GO-LICENSE', 'tests/v3_verifier_wasm.mjs',
]);
export const VERIFIER_FILES = Object.freeze(['GO-LICENSE', 'verifier-worker.mjs', 'verifier.d.mts', 'verifier.mjs', 'verifier.wasm', 'wasm_exec.cjs']);
export const RECIPE_FILES = Object.freeze(['build.mjs', 'build.sh', 'package.json', 'package-lock.json', 'release.json',
  'release-metadata.mjs', 'distribution-reports.mjs', 'tsconfig.json', 'core/tsconfig.json', 'native-artifacts.d.mts']);
function artifact(value, bytes) {
  requireDistribution(exact(value, ['sha256']) && hash(value.sha256));
  if (bytes) requireDistribution(digest(bytes) === value.sha256, 'artifact');
  return { sha256: value.sha256 };
}
export function nativeReport(value, key, bytes, repository) {
  requireDistribution(Object.hasOwn(NATIVE_TARGETS, key) && exact(value, ['rust', 'tor', 'platform', 'arch', 'target',
    'distributionTarget', 'rustHost', 'rustCommit', 'targetStdSha256', 'linker', 'portability', 'sources', 'artifacts']));
  const [platform, arch] = key.split('-'), target = NATIVE_TARGETS[key].target;
  requireDistribution(value.rust === '1.92.0' && value.tor === '0.47.0' && value.platform === platform && value.arch === arch
    && value.target === target && value.distributionTarget === true && typeof value.rustCommit === 'string'
    && /^[a-f0-9]{40}$/.test(value.rustCommit) && hash(value.targetStdSha256)
    && value.rustHost === (platform === 'darwin' ? target : `${arch === 'arm64' ? 'aarch64' : 'x86_64'}-unknown-linux-gnu`), 'toolchain');
  if (platform === 'darwin') {
    requireDistribution(exact(value.linker, ['kind', 'version', 'sdk', 'deploymentTarget', 'abi'])
      && value.linker.kind === 'apple-clang' && version(value.linker.version) && version(value.linker.sdk)
      && value.linker.deploymentTarget === '14.0' && value.linker.abi === 'apple', 'toolchain');
    requireDistribution(exact(value.portability, ['format', 'linkage', 'deploymentTarget', 'execution'])
      && value.portability.format === 'macho64' && value.portability.linkage === 'platform_only'
      && value.portability.deploymentTarget === '14.0' && value.portability.execution === 'not_tested', 'artifact');
  } else {
    requireDistribution(exact(value.linker, ['kind', 'abi', 'selfContained']) && value.linker.kind === 'rust-lld'
      && value.linker.abi === 'musl' && value.linker.selfContained === true, 'toolchain');
    requireDistribution(exact(value.portability, ['format', 'linkage', 'execution']) && value.portability.format === 'elf64'
      && value.portability.linkage === 'static' && value.portability.execution === 'not_tested', 'artifact');
  }
  const publicSources = sources(value.sources, NATIVE_SOURCES, repository);
  requireDistribution(exact(value.artifacts, ['possums-tor-native'])
    && exact(value.artifacts['possums-tor-native'], ['sha256', 'size']));
  const entry = value.artifacts['possums-tor-native'];
  requireDistribution(hash(entry.sha256) && Number.isSafeInteger(entry.size) && entry.size >= 64 && entry.size <= MAX_NATIVE_BYTES, 'artifact');
  if (bytes) {
    requireDistribution(entry.size === bytes.length && entry.sha256 === digest(bytes), 'artifact');
    try { inspectNativeHeader(bytes, key); } catch { throw new DistributionFailure('artifact'); }
  }
  return { ...value, sources: publicSources };
}
export function verifierReport(value, directory, repository) {
  requireDistribution(exact(value, ['go', 'node', 'sdk', 'sources', 'artifacts']));
  requireDistribution(value.go === '1.27.2' && value.node === 'v24.13.0'
    && value.sdk === 'v0.17.0-rc.1.0.20261009012920-31c57af7d7b4', 'toolchain');
  const publicSources = sources(value.sources, VERIFIER_SOURCES, repository);
  requireDistribution(exact(value.artifacts, [...VERIFIER_FILES, 'possums-attestation']));
  const artifacts = Object.fromEntries([...VERIFIER_FILES, 'possums-attestation'].map(name => [name,
    artifact(value.artifacts[name], name === 'possums-attestation' ? undefined : publicBytes(path.join(directory, name)))]));
  // possums-attestation is an upstream build output, never a shipped file.
  return { go: value.go, node: value.node, sdk: value.sdk, sources: publicSources, artifacts };
}
const publicModule = name => typeof name === 'string' && name.length < 256
  && /^[a-zA-Z0-9_@.\/-]+$/.test(name) && !name.startsWith('/') && !name.split('/').some(part => !part || part === '.' || part === '..');
const peer = name => ['@earendil-works/pi-ai', '@earendil-works/pi-coding-agent'].some(value => name === value || name.startsWith(value + '/'));
export function clientReport(value, directory, workspace, repository) {
  requireDistribution(exact(value, ['node', 'pi', 'runtimePins', 'typescript', 'esbuild', 'lock', 'recipe', 'verifier', 'torNative', 'artifacts']));
  requireDistribution(value.node === 'v24.13.0' && value.pi === '1.3.0' && value.typescript === '5.9.3' && value.esbuild === '0.25.10', 'toolchain');
  requireDistribution(hash(value.lock) && value.lock === digest(publicBytes(path.join(repository, 'clients/pi/package-lock.json'))), 'source');
  requireDistribution(exact(value.runtimePins, ['pi-ai', 'pi-coding-agent', 'pi-agent-core']));
  for (const entry of Object.values(value.runtimePins)) requireDistribution(exact(entry, ['version', 'manifestSha256']) && entry.version === '1.3.0' && hash(entry.manifestSha256), 'toolchain');
  requireDistribution(exact(value.recipe, ['entry', 'fixture', 'sources']) && value.recipe.entry === 'index.ts' && value.recipe.fixture === false, 'source');
  const recipe = { entry: value.recipe.entry, fixture: false, sources: sources(value.recipe.sources, RECIPE_FILES.map(name => 'clients/pi/' + name), repository) };
  const verifier = verifierReport(value.verifier, directory, repository);
  requireDistribution(isDeepStrictEqual(value.verifier, publicJSON(path.join(directory, 'verifier-build-report.json'))), 'artifact');
  const nativeKey = `${value.torNative?.platform}-${value.torNative?.arch}`;
  requireDistribution(Object.hasOwn(NATIVE_TARGETS, nativeKey), 'artifact');
  const nativeBytes = publicBytes(path.join(directory, NATIVE_TARGETS[nativeKey].path));
  const torNative = nativeReport(value.torNative, nativeKey, nativeBytes, repository);
  requireDistribution(exact(value.artifacts, [...VERIFIER_FILES, NATIVE_TARGETS[nativeKey].path, 'native-manifest.json', 'extension']));
  const payload = Object.fromEntries(VERIFIER_FILES.map(name => [name, artifact(value.artifacts[name], publicBytes(path.join(directory, name)))]));
  artifact(value.artifacts[NATIVE_TARGETS[nativeKey].path], nativeBytes);
  artifact(value.artifacts['native-manifest.json'], publicBytes(path.join(directory, 'native-manifest.json')));
  const definition = NATIVE_TARGETS[nativeKey];
  requireDistribution(isDeepStrictEqual(publicJSON(path.join(directory, 'native-manifest.json')), { schema: 1, targets: {
    [nativeKey]: { target: definition.target, path: definition.path, size: nativeBytes.length, sha256: digest(nativeBytes) },
  } }), 'artifact');
  const extension = value.artifacts.extension;
  requireDistribution(exact(extension, ['sha256', 'modules', 'external']) && hash(extension.sha256)
    && extension.sha256 === digest(publicBytes(path.join(directory, 'extension.mjs'))) && Array.isArray(extension.modules)
    && extension.modules.length > 0 && extension.modules.length < 1024 && Array.isArray(extension.external) && extension.external.length < 128, 'artifact');
  const lock = publicJSON(path.join(repository, 'clients/pi/package-lock.json'));
  const modules = {}, seen = new Set();
  for (const entry of extension.modules) {
    requireDistribution(exact(entry, ['name', 'sha256', 'imports']) && publicModule(entry.name) && hash(entry.sha256)
      && !seen.has(entry.name) && Array.isArray(entry.imports) && entry.imports.length < 256, 'source');
    seen.add(entry.name);
    if (entry.name.startsWith('node_modules/')) {
      const segments = entry.name.split('/'), nested = segments.lastIndexOf('node_modules');
      const count = nested + (segments[nested + 1]?.startsWith('@') ? 3 : 2);
      requireDistribution(Object.hasOwn(lock.packages, segments.slice(0, count).join('/')), 'source');
      requireDistribution(entry.sha256 === digest(publicBytes(path.join(workspace, entry.name))), 'source');
    } else {
      requireDistribution(/^(?:core\/)?[a-zA-Z0-9_-]+\.(?:ts|mjs)$/.test(entry.name)
        && entry.sha256 === digest(publicBytes(path.join(repository, 'clients/pi', entry.name))), 'source');
    }
    modules[entry.name.startsWith('node_modules/') ? entry.name : 'clients/pi/' + entry.name] = entry.sha256;
  }
  const allowedExternal = entry => exact(entry, ['path', 'kind', 'external']) && typeof entry.path === 'string'
    && entry.path.length < 256 && (isBuiltin(entry.path) || (publicModule(entry.path) && peer(entry.path)))
    && ['import-statement', 'dynamic-import', 'require-call'].includes(entry.kind) && entry.external === true;
  requireDistribution(extension.external.every(allowedExternal), 'external');
  // Detailed import graphs are not public output. Still reject unexpected fields.
  for (const entry of extension.modules) for (const imported of entry.imports) {
    requireDistribution(imported && typeof imported === 'object' && !Array.isArray(imported)
      && Object.keys(imported).every(key => ['path', 'kind', 'external', 'original'].includes(key))
      && typeof imported.path === 'string' && ['import-statement', 'dynamic-import', 'require-call'].includes(imported.kind)
      && (imported.external === true ? allowedExternal({ path: imported.path, kind: imported.kind, external: true }) : seen.has(imported.path)), 'external');
    requireDistribution(!Object.hasOwn(imported, 'external') || typeof imported.external === 'boolean', 'schema');
    requireDistribution(!Object.hasOwn(imported, 'original') || (typeof imported.original === 'string'
      && imported.original.length < 256 && (isBuiltin(imported.original) || (/^[a-zA-Z0-9_@#.\/-]+$/.test(imported.original)
        && !imported.original.startsWith('/') && publicModule(path.posix.normalize(path.posix.join(path.posix.dirname(entry.name), imported.original)))))), 'schema');
  }
  return { node: value.node, pi: value.pi, runtimePins: value.runtimePins, typescript: value.typescript, esbuild: value.esbuild,
    lock: value.lock, recipe, verifier, torNative, nativeKey, artifacts: { ...payload, 'extension.mjs': { sha256: extension.sha256 } },
    modules, external: [...new Set(extension.external.map(entry => entry.path))].sort() };
}
const INSPECTION_FAILURES = Object.freeze({
  possums_build_tor_inspection_tool: 'inspection_tool', possums_build_tor_format: 'inspection_format',
  possums_build_tor_linkage: 'inspection_linkage', possums_build_tor_deployment: 'inspection_deployment',
  possums_build_tor_inspection_target: 'inspection_input', possums_build_tor_artifact: 'inspection_input',
  possums_build_tor_inspection_input: 'inspection_input', possums_build_tor_inspection_unknown: 'inspection_unknown',
  possums_build_tor_inspection_timeout: 'inspection_timeout',
});
const INSPECTION_TIMEOUT_MS = 180_000;
export function independentlyInspect(repository, target, file, expected, sha256 = digest(publicBytes(file))) {
  requireDistribution(hash(sha256), 'input');
  let text;
  try {
    text = execFileSync('python3', [path.join(repository, 'scripts/tor_native_portability.py'), '--json', target, file, '--sha256', sha256], {
      cwd: repository, env: { PATH: process.env.PATH, LC_ALL: 'C' }, encoding: 'utf8', maxBuffer: 64 * 1024,
      stdio: ['ignore', 'pipe', 'pipe'], timeout: INSPECTION_TIMEOUT_MS, killSignal: 'SIGKILL',
    });
  } catch (error) {
    let stage;
    try {
      const failure = JSON.parse(error.stdout);
      if (exact(failure, ['ok', 'code']) && failure.ok === false && typeof failure.code === 'string'
        && Object.hasOwn(INSPECTION_FAILURES, failure.code)) stage = INSPECTION_FAILURES[failure.code];
    } catch {}
    if (stage) throw new DistributionFailure(stage);
    let timedOut = false;
    try { timedOut = error?.code === 'ETIMEDOUT'; } catch {}
    throw new DistributionFailure(timedOut ? 'inspection_timeout' : 'inspection_unknown');
  }
  let receipt;
  try { receipt = JSON.parse(text); } catch { throw new DistributionFailure('inspection_unknown'); }
  requireDistribution(exact(receipt, ['ok', 'evidence']) && receipt.ok === true && isDeepStrictEqual(receipt.evidence, expected), 'inspection');
  return receipt.evidence;
}
