// Build-only assembly. No registry, installation, activation or inference operation.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { isDeepStrictEqual } from 'node:util';
import { readReleaseMetadata, candidateManifest, DISTRIBUTION_FILES, MetadataError } from './release-metadata.mjs';
import { NATIVE_TARGETS } from './native-artifacts.mjs';
import { DistributionFailure, requireDistribution as require, digest, publicBytes, publicJSON,
  clientReport, nativeReport, independentlyInspect, VERIFIER_FILES } from './distribution-reports.mjs';
import { inspectDistribution, PackError } from './pack.mjs';
const REPOSITORY = fileURLToPath(new URL('../../', import.meta.url));
const ASSEMBLY_SOURCES = Object.freeze(['clients/pi/assemble.mjs', 'clients/pi/distribution-reports.mjs',
  'clients/pi/pack.mjs', 'clients/pi/release-metadata.mjs', 'clients/pi/release.json',
  'clients/pi/PACKAGE-README.md', 'clients/pi/THIRD-PARTY-NOTICES']);
const knownFailure = error => error instanceof DistributionFailure || error instanceof MetadataError || error instanceof PackError;
function directory(value) {
  require(typeof value === 'string' && value.length < 4096 && !value.includes('\0') && path.isAbsolute(value), 'input');
  try {
    const stat = fs.lstatSync(value);
    require(stat.isDirectory() && !stat.isSymbolicLink(), 'input');
    return fs.realpathSync(value);
  } catch (error) {
    if (knownFailure(error)) throw error;
    if (error?.code === 'ENOENT') throw new DistributionFailure('missing');
    if (['EACCES', 'EPERM'].includes(error?.code)) throw new DistributionFailure('unreadable');
    throw new DistributionFailure('read_unknown');
  }
}
function snapshot(repository) {
  try {
    const options = { cwd: repository, encoding: 'utf8', maxBuffer: 1024 * 1024, stdio: ['ignore', 'pipe', 'pipe'],
      env: { PATH: process.env.PATH, LC_ALL: 'C', GIT_CONFIG_GLOBAL: '/dev/null', GIT_CONFIG_SYSTEM: '/dev/null', GIT_OPTIONAL_LOCKS: '0' } };
    const commit = execFileSync('git', ['rev-parse', 'HEAD'], options).trim();
    require(/^[a-f0-9]{40}$/.test(commit), 'source');
    const dirty = execFileSync('git', ['-c', 'core.fsmonitor=false', 'status', '--porcelain', '--untracked-files=all'], options).trim().length > 0;
    return { commit, dirty };
  } catch (error) { throw error instanceof DistributionFailure ? error : new DistributionFailure('source_unknown'); }
}
const json = value => Buffer.from(JSON.stringify(value, null, 2) + '\n');

/** One private candidate: every native target is mandatory, with independent byte inspection. */
export function assembleDistribution({ sourcePackage, nativeRoot, output, repository = REPOSITORY } = {}) {
  let writing = false;
  try {
    require(process.version === 'v24.13.0', 'toolchain');
    repository = directory(repository); sourcePackage = directory(sourcePackage); nativeRoot = directory(nativeRoot);
    require(typeof output === 'string' && path.isAbsolute(output) && !fs.existsSync(output), 'output');
    output = path.join(directory(path.dirname(output)), path.basename(output));
    const before = snapshot(repository);
    const metadata = readReleaseMetadata(path.join(repository, 'clients/pi'));
    const manifest = candidateManifest(metadata);
    require(isDeepStrictEqual(publicJSON(path.join(sourcePackage, 'package.json')), manifest), 'schema');
    const workspace = path.resolve(sourcePackage, '../source/clients/pi');
    const client = clientReport(publicJSON(path.join(sourcePackage, 'build-report.json')), sourcePackage, workspace, repository);
    const files = new Map([...VERIFIER_FILES, 'extension.mjs'].map(name => {
      const bytes = publicBytes(path.join(sourcePackage, name));
      require(digest(bytes) === client.artifacts[name].sha256, 'artifact');
      return [name, bytes];
    }));
    files.set('verifier-build-report.json', json(client.verifier));
    files.set('package.json', json(manifest));
    files.set('README.md', publicBytes(path.join(repository, 'clients/pi/PACKAGE-README.md')));
    files.set('THIRD-PARTY-NOTICES', publicBytes(path.join(repository, 'clients/pi/THIRD-PARTY-NOTICES')));
    const reports = {}, inspections = {}, targets = {};
    for (const [key, definition] of Object.entries(NATIVE_TARGETS)) {
      const input = directory(path.join(nativeRoot, key));
      const helper = path.join(input, 'possums-tor-native');
      const bytes = publicBytes(helper);
      const report = nativeReport(publicJSON(path.join(input, 'tor-native-build-report.json')), key, bytes, repository);
      // Never trust a label, report boolean or matching hash as linkage evidence.
      independentlyInspect(repository, definition.target, helper, report.portability, digest(bytes));
      if (key === client.nativeKey) require(report.artifacts['possums-tor-native'].sha256 === client.torNative.artifacts['possums-tor-native'].sha256, 'artifact');
      reports[key] = report; inspections[key] = report.portability;
      targets[key] = { target: definition.target, path: definition.path, size: bytes.length, sha256: digest(bytes) };
      files.set(definition.path, bytes);
      files.set(`native/${key}/tor-native-build-report.json`, json(report));
    }
    files.set('native-manifest.json', json({ schema: 1, targets }));
    const report = {
      schema: 1, release: { name: metadata.name, version: metadata.version, private: true, redistributionApproved: false },
      sourceCommit: before.dirty ? null : before.commit, sourceState: before.dirty ? 'uncommitted' : 'exact_commit',
      node: client.node, pi: client.pi, runtimePins: client.runtimePins, typescript: client.typescript, esbuild: client.esbuild,
      lock: client.lock, recipe: client.recipe, modules: client.modules, external: client.external,
      assemblySources: Object.fromEntries(ASSEMBLY_SOURCES.map(name => [name, digest(publicBytes(path.join(repository, name)))])),
      verifier: client.verifier, torNative: reports, nativeInspection: inspections,
      qualification: 'not_established_by_assembly',
      artifacts: Object.fromEntries([...files].sort(([a], [b]) => a < b ? -1 : a > b ? 1 : 0).map(([name, bytes]) => [name, { size: bytes.length, sha256: digest(bytes) }])),
    };
    files.set('build-report.json', json(report));
    require(files.size === DISTRIBUTION_FILES.length && DISTRIBUTION_FILES.every(name => files.has(name)), 'schema');
    writing = true;
    fs.mkdirSync(output, { mode: 0o700 });
    for (const name of DISTRIBUTION_FILES) {
      const destination = path.join(output, name);
      fs.mkdirSync(path.dirname(destination), { recursive: true });
      const mode = name.endsWith('/possums-tor-native') ? 0o755 : 0o644;
      fs.writeFileSync(destination, files.get(name), { flag: 'wx', mode });
      fs.chmodSync(destination, mode);
    }
    // Inspection of copied bytes is mandatory too; copying never upgrades a report to proof.
    for (const [key, definition] of Object.entries(NATIVE_TARGETS)) {
      independentlyInspect(repository, definition.target, path.join(output, definition.path), inspections[key], targets[key].sha256);
    }
    const inventory = inspectDistribution(output);
    for (const entry of inventory) {
      const expected = files.get(entry.path);
      // This external inventory can hash the report, although the report cannot hash itself.
      require(entry.sha256 === digest(expected) && entry.size === expected.length, 'artifact');
    }
    const after = snapshot(repository);
    require(before.commit === after.commit && before.dirty === after.dirty, 'source');
    return { private: true, files: inventory, nativeTargets: Object.keys(NATIVE_TARGETS),
      qualification: 'not_established_by_assembly', publication: 'not_attempted' };
  } catch (error) {
    if (knownFailure(error)) throw error;
    if (writing && error?.code === 'EEXIST') throw new DistributionFailure('output');
    if (writing && ['EACCES', 'EPERM', 'EROFS'].includes(error?.code)) throw new DistributionFailure('output_unavailable');
    if (writing && ['ENOSPC', 'EDQUOT', 'EFBIG'].includes(error?.code)) throw new DistributionFailure('output_storage');
    throw new DistributionFailure(writing ? 'output_unknown' : 'assembly_unknown');
  }
}
if (import.meta.main) {
  try {
    require(process.argv.length === 5, 'input');
    const [sourcePackage, nativeRoot, output] = process.argv.slice(2);
    const receipt = assembleDistribution({ sourcePackage, nativeRoot, output });
    console.log(JSON.stringify({ private: receipt.private, files: receipt.files.length, nativeTargets: receipt.nativeTargets,
      qualification: receipt.qualification, publication: receipt.publication }));
  } catch (error) {
    console.error((knownFailure(error) ? error : new DistributionFailure('assembly_unknown')).message);
    process.exitCode = 1;
  }
}
