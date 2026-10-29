import { readFileSync } from 'node:fs';
import { join } from 'node:path';

// Candidate only: no project LICENSE grant exists. Adding one requires reviewed source changes.
export const DISTRIBUTION_FILES = Object.freeze([
  'GO-LICENSE',
  'README.md',
  'THIRD-PARTY-NOTICES',
  'build-report.json',
  'extension.mjs',
  'native-manifest.json',
  'native/darwin-arm64/possums-tor-native',
  'native/darwin-arm64/tor-native-build-report.json',
  'native/darwin-x64/possums-tor-native',
  'native/darwin-x64/tor-native-build-report.json',
  'native/linux-arm64/possums-tor-native',
  'native/linux-arm64/tor-native-build-report.json',
  'native/linux-x64/possums-tor-native',
  'native/linux-x64/tor-native-build-report.json',
  'package.json',
  'verifier-build-report.json',
  'verifier-worker.mjs',
  'verifier.d.mts',
  'verifier.mjs',
  'verifier.wasm',
  'wasm_exec.cjs',
]);

const MESSAGES = Object.freeze({
  read: 'Metadata read failed; cause unknown. Check the local release.json file and permissions.',
  parse: 'Metadata parse requires valid JSON. Correct the authored release.json file.',
  shape: 'Metadata validation requires the exact authored keys and plain data fields. Restore the reviewed schema.',
  candidate: 'Metadata validation requires a bounded prerelease semver. Correct the candidate version.',
  pins: 'Metadata validation requires the reviewed package, repository and runtime pins. Restore the reviewed constants.',
  approval: 'Metadata approval is unsupported: license must remain null and redistribution unapproved. A release requires separately reviewed source changes.',
  unknown: 'Metadata validation failed; cause unknown. Restore plain authored candidate metadata.',
});
class MetadataError extends Error {
  constructor(stage) {
    const code = `possums_release_metadata_${stage}`;
    super(`${code}: ${MESSAGES[stage]}`);
    delete this.stack;
    this.code = code;
    this.stage = stage;
    Object.freeze(this);
  }
}
const reject = (stage) => { throw new MetadataError(stage); };
function exactRecord(value, keys) {
  if (!value || typeof value !== 'object' ||
      ![Object.prototype, null].includes(Object.getPrototypeOf(value))) reject('shape');
  const fields = Object.getOwnPropertyDescriptors(value);
  if (Reflect.ownKeys(fields).length !== keys.length ||
      keys.some((key) => !Object.hasOwn(fields, key) || !Object.hasOwn(fields[key], 'value'))) reject('shape');
  return Object.fromEntries(keys.map((key) => [key, fields[key].value]));
}
function validate(metadata) {
  try {
    metadata = exactRecord(metadata, ['name', 'version', 'description', 'nodeVersion', 'piVersion', 'repository', 'license', 'redistributionApproved']);
    metadata.repository = exactRecord(metadata.repository, ['type', 'url', 'directory']);
    if (metadata.license !== null || metadata.redistributionApproved !== false) reject('approval');
    // Numeric prerelease identifiers cannot have leading zeros; stable releases are not candidates.
    const numeric = '(?:0|[1-9][0-9]*)';
    const identifier = `(?:${numeric}|[0-9]*[A-Za-z-][0-9A-Za-z-]*)`;
    const semver = new RegExp(`^${numeric}\\.${numeric}\\.${numeric}-${identifier}(?:\\.${identifier})*(?:\\+[0-9A-Za-z-]+(?:\\.[0-9A-Za-z-]+)*)?$`);
    if (typeof metadata.version !== 'string' || metadata.version.length > 128 ||
        !semver.test(metadata.version)) reject('candidate');
    if (metadata.name !== '@possums/pi' || metadata.nodeVersion !== '24.13.0' || metadata.piVersion !== '1.3.0' ||
        metadata.description !== 'Possums verified API client extension for Pi' ||
        metadata.repository.type !== 'git' || metadata.repository.url !== 'https://github.com/ajbt200128/possums' ||
        metadata.repository.directory !== 'clients/pi') reject('pins');
    return metadata;
  } catch (error) {
    if (error instanceof MetadataError) throw error;
    reject('unknown');
  }
}

// Synchronous, build-only read of <directory>/release.json; no network or runtime client imports.
export function readReleaseMetadata(directory) {
  let text;
  try { text = readFileSync(join(directory, 'release.json'), 'utf8'); }
  catch { reject('read'); }
  let metadata;
  try { metadata = JSON.parse(text); }
  catch { reject('parse'); }
  return validate(metadata);
}

export function candidateManifest(metadata) {
  const approvedShape = validate(metadata);
  return {
    name: approvedShape.name, version: approvedShape.version, private: true, type: 'module',
    description: approvedShape.description,
    pi: { extensions: ['./extension.mjs'] }, keywords: ['pi-package'],
    engines: { node: approvedShape.nodeVersion },
    peerDependencies: { '@earendil-works/pi-ai': '*', '@earendil-works/pi-coding-agent': '*' },
    repository: { ...approvedShape.repository }, files: [...DISTRIBUTION_FILES],
    // Future public settings do not authorize publication or override private:true.
    publishConfig: { access: 'public', provenance: true },
  };
}
