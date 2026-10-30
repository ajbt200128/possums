import { constants, lstatSync, realpathSync, existsSync, readdirSync, opendirSync, openSync, closeSync, fstatSync, fchmodSync, read, readSync, writeSync, mkdirSync, mkdtempSync, rmSync, writeFileSync, createReadStream } from 'node:fs';
import { join, resolve, dirname, basename, delimiter } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';
import { createHash } from 'node:crypto';
import { isDeepStrictEqual } from 'node:util';
import { execFileSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { Transform } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import { createInflateRaw, crc32 } from 'node:zlib';
import { DISTRIBUTION_FILES, candidateManifest, readReleaseMetadata } from './release-metadata.mjs';

const SOURCE = dirname(fileURLToPath(import.meta.url));
const MAX_FILE = 64 * 1024 * 1024;
const MAX_TOTAL = 512 * 1024 * 1024;
const MAX_ARCHIVE = MAX_TOTAL + 1024 * 1024;
const TAR_BLOCK_BYTES = 512;
const GZIP_HEADER_BYTES = 10;
const GZIP_FOOTER_BYTES = 8;
const FILES = new Set(DISTRIBUTION_FILES);
const DIRECTORIES = new Set(DISTRIBUTION_FILES.flatMap((path) => {
  const parts = path.split('/');
  return parts.slice(0, -1).map((_, i) => parts.slice(0, i + 1).join('/'));
}));
const MESSAGES = Object.freeze({
  input: 'Input requires local directory or archive paths. Supply the reviewed build-only paths.',
  schema: 'Inventory requires the exact bounded distribution entries. Regenerate the clean inventory.',
  files: 'Files require exactly the allowlisted regular files, sizes and modes without links or extra directories. Rebuild a clean distribution.',
  metadata: 'Manifest requires the exact private candidate metadata without additional settings. Restore the authored candidate manifest; no publication rights are granted.',
  archive: 'Archive validation failed or its cause is unknown; require exactly the canonical regular entries with matching bytes, sizes and modes. Discard this tarball and rebuild locally.',
  tool: 'Packing-tool validation or execution failed; any underlying execution cause is unknown. Require Node 24.13.0, npm 11.6.2, tar 7.5.22 and a valid offline pack result. Check the pinned tools; no installation or publication was attempted.',
  io: 'Local packing I/O failed; cause unknown. Check local permissions and storage, then remove any incomplete output before rebuilding.',
});
export class PackError extends Error {
  constructor(stage) {
    stage = typeof stage === 'string' && Object.hasOwn(MESSAGES, stage) ? stage : 'io';
    const code = `possums_pi_pack_${stage}`;
    super(`${code}: ${MESSAGES[stage]}`);
    delete this.stack;
    this.code = code;
    this.stage = stage;
    Object.freeze(this);
  }
}
const reject = (stage) => { throw new PackError(stage); };
function fail(error, stage) { if (error instanceof PackError) throw error; reject(stage); }
function localPath(value) { if (typeof value !== 'string' || !value || value.includes('\0')) reject('input'); return resolve(value); }
const modeFor = (path) => path.endsWith('/possums-tor-native') ? 0o755 : 0o644;
function limitFor(path) {
  if (path === 'package.json' || path === 'native-manifest.json') return 16 * 1024;
  if (path.endsWith('.json')) return 2 * 1024 * 1024;
  return MAX_FILE;
}

// No links are followed. Hash/copy fixed-size chunks rather than allocating from file sizes.
function readRegular(file, path, destination, consume) {
  let fd;
  let output;
  let result;
  let primary;
  try {
    const before = lstatSync(file);
    if (!before.isFile() || before.nlink !== 1 || before.size > limitFor(path) || (before.mode & 0o7777) !== modeFor(path)) reject('files');
    fd = openSync(file, constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK);
    const stat = fstatSync(fd);
    if (!stat.isFile() || stat.dev !== before.dev || stat.ino !== before.ino || stat.size !== before.size || stat.nlink !== 1) reject('files');
    if (destination) {
      output = openSync(destination, constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL, modeFor(path));
      fchmodSync(output, modeFor(path));
    }
    const hash = createHash('sha256');
    const buffer = Buffer.alloc(64 * 1024);
    let size = 0;
    for (;;) {
      const length = readSync(fd, buffer, 0, buffer.length, null);
      if (!length) break;
      size += length;
      if (size > limitFor(path)) reject('files');
      hash.update(buffer.subarray(0, length));
      consume?.(buffer.subarray(0, length));
      if (output !== undefined) {
        let written = 0;
        while (written < length) {
          const count = writeSync(output, buffer, written, length - written);
          if (!count) reject('io');
          written += count;
        }
      }
    }
    const after = fstatSync(fd);
    if (size !== stat.size || after.size !== stat.size || after.mtimeMs !== stat.mtimeMs || after.ctimeMs !== stat.ctimeMs) reject('files');
    result = { path, size, sha256: hash.digest('hex'), mode: modeFor(path) };
  } catch (error) { primary = error instanceof PackError ? error : new PackError('io'); }
  finally {
    // Closing must not replace a known primary failure.
    for (const descriptor of [fd, output]) {
      if (descriptor !== undefined) { try { closeSync(descriptor); } catch { primary ??= new PackError('io'); } }
    }
  }
  if (primary) throw primary;
  return result;
}

/** Build-only byte inventory, not native-format, linkage, report or trust acceptance. */
export function inspectDistribution(directory) {
  try {
    directory = localPath(directory);
    if (!lstatSync(directory).isDirectory() || lstatSync(directory).isSymbolicLink()) reject('files');
    const inventory = [];
    let total = 0;
    let visited = 0;
    let manifestText = '';
    function visit(relative) {
      const dir = opendirSync(join(directory, relative));
      let primary;
      try {
        for (let entry; (entry = dir.readSync()) !== null;) {
          if (++visited > FILES.size + DIRECTORIES.size) reject('files');
          const path = relative ? `${relative}/${entry.name}` : entry.name;
          if (entry.isDirectory()) {
            if (!DIRECTORIES.has(path)) reject('files');
            visit(path);
          } else {
            if (!entry.isFile() || !FILES.has(path)) reject('files');
            const item = readRegular(join(directory, path), path, undefined,
              path === 'package.json' ? (chunk) => { manifestText += chunk.toString('utf8'); } : undefined);
            total += item.size;
            if (total > MAX_TOTAL) reject('files');
            inventory.push(item);
          }
        }
      } catch (error) { primary = error instanceof PackError ? error : new PackError('io'); }
      finally { try { dir.closeSync(); } catch { primary ??= new PackError('io'); } }
      if (primary) throw primary;
    }
    visit('');
    inventory.sort((a, b) => {
      if (a.path === b.path) return 0;
      return a.path < b.path ? -1 : 1;
    });
    if (inventory.length !== FILES.size || inventory.some((item, i) => item.path !== DISTRIBUTION_FILES[i])) reject('files');
    let manifest;
    try { manifest = JSON.parse(manifestText); }
    catch { reject('metadata'); }
    try {
      if (!isDeepStrictEqual(manifest, candidateManifest(readReleaseMetadata(SOURCE)))) reject('metadata');
    } catch { reject('metadata'); }
    return inventory;
  } catch (error) { fail(error, 'io'); }
}

function checkedInventory(inventory) {
  if (!Array.isArray(inventory) || inventory.length !== FILES.size) reject('schema');
  let total = 0;
  for (let i = 0; i < inventory.length; i++) {
    const item = inventory[i];
    if (!item || !isDeepStrictEqual(Object.keys(item).sort(), ['mode', 'path', 'sha256', 'size']) ||
        item.path !== DISTRIBUTION_FILES[i] || !Number.isSafeInteger(item.size) || item.size < 0 || item.size > limitFor(item.path) ||
        item.mode !== modeFor(item.path) || typeof item.sha256 !== 'string' || !/^[a-f0-9]{64}$/.test(item.sha256)) reject('schema');
    total += item.size;
  }
  if (total > MAX_TOTAL) reject('schema');
  return new Map(inventory.map((item) => [`package/${item.path}`, item]));
}

function sameFile(before, after) {
  return after.isFile() && ['dev', 'ino', 'mode', 'nlink', 'size', 'mtimeNs', 'ctimeNs'].every((key) => before[key] === after[key]);
}

function readEnvelopeBytes(fd, length, position) {
  const buffer = Buffer.alloc(length);
  let offset = 0;
  while (offset < length) {
    const count = readSync(fd, buffer, offset, length - offset, position + offset);
    if (!count) reject('archive');
    offset += count;
  }
  return buffer;
}

// Own one nonblocking descriptor for validation AND the compressed-byte receipt.
async function readTarball(file, inventory) {
  let fd;
  let result;
  let primary;
  try {
    file = localPath(file);
    const expected = checkedInventory(inventory);
    const before = lstatSync(file, { bigint: true });
    if (!before.isFile() || before.nlink !== 1n || before.size < 20n || before.size > MAX_ARCHIVE) reject('archive');
    fd = openSync(file, constants.O_RDONLY | constants.O_NONBLOCK | constants.O_NOFOLLOW);
    const stat = fstatSync(fd, { bigint: true });
    if (!sameFile(before, stat)) reject('archive');
    const archiveSize = Number(stat.size);
    const header = readEnvelopeBytes(fd, GZIP_HEADER_BYTES, 0);
    const footer = readEnvelopeBytes(fd, GZIP_FOOTER_BYTES, archiveSize - GZIP_FOOTER_BYTES);
    // No optional text, timestamp, or unknown encoder/platform metadata may hide here.
    if (!header.subarray(0, 4).equals(Buffer.from([0x1f, 0x8b, 8, 0])) || header.readUInt32LE(4) !== 0 ||
        ![0, 2, 4].includes(header[8]) || ![3, 255].includes(header[9])) reject('archive');
    let tar;
    try {
      const require = createRequire(import.meta.url);
      if (require('tar/package.json').version !== '7.5.22') reject('tool');
      tar = await import('tar');
    } catch (error) { fail(error, 'tool'); }
    const seen = new Set();
    // No PAX records are allowed; npm emits one header per file and two EOF blocks.
    // A parser may otherwise ignore unlisted bytes after the TAR end marker.
    const canonicalSize = inventory.reduce((size, item) => size + (1 + Math.ceil(item.size / TAR_BLOCK_BYTES)) * TAR_BLOCK_BYTES, 2 * TAR_BLOCK_BYTES);
    let problem = false;
    let expanded = 0;
    let checksum = 0;
    const expandedHash = createHash('sha256');
    const canonicalHash = createHash('sha256');
    let size = header.length;
    const sha256 = createHash('sha256').update(header);
    const sha512 = createHash('sha512').update(header);
    const compressed = new Transform({ transform(chunk, _encoding, callback) {
      size += chunk.length;
      if (size > MAX_ARCHIVE || BigInt(size) > stat.size) { callback(new PackError('archive')); return; }
      sha256.update(chunk); sha512.update(chunk);
      callback(null, chunk);
    } });
    const bounds = new Transform({ transform(chunk, _encoding, callback) {
      expanded += chunk.length;
      if (expanded > MAX_ARCHIVE) { callback(new PackError('archive')); return; }
      checksum = crc32(chunk, checksum);
      expandedHash.update(chunk);
      callback(null, chunk);
    } });
    const listing = tar.t({ strict: true, maxMetaEntrySize: 16 * 1024, onReadEntry(entry) {
      const item = expected.get(entry.path);
      if (!item || seen.has(entry.path) || entry.type !== 'File' || entry.linkpath || entry.size !== item.size ||
          (entry.mode & 0o7777) !== item.mode || seen.size >= FILES.size
          || ![null, undefined, 0].includes(entry.uid) || ![null, undefined, 0].includes(entry.gid)
          || ![null, undefined, ''].includes(entry.uname) || ![null, undefined, ''].includes(entry.gname)) {
        problem = true;
        listing.abort(new PackError('archive'));
        entry.resume();
        return;
      }
      // The parser deliberately ignores padding and bytes after field terminators.
      // Re-encode only accepted metadata, using the pinned library's public API.
      const canonicalHeader = new tar.Header({ path: entry.path, type: 'File', mode: item.mode,
        size: item.size, mtime: entry.mtime, uid: entry.uid, gid: entry.gid,
        uname: '', gname: '', linkpath: '' });
      if (canonicalHeader.encode()) { problem = true; listing.abort(new PackError('archive')); entry.resume(); return; }
      canonicalHash.update(canonicalHeader.block);
      seen.add(entry.path);
      const hash = createHash('sha256');
      let size = 0;
      entry.on('data', (chunk) => {
        size += chunk.length;
        if (size > item.size) { problem = true; listing.abort(new PackError('archive')); return; }
        hash.update(chunk);
        canonicalHash.update(chunk);
        // Finish padding with the final payload chunk, before another entry can emit.
        if (size === item.size) canonicalHash.update(Buffer.alloc((TAR_BLOCK_BYTES - size % TAR_BLOCK_BYTES) % TAR_BLOCK_BYTES));
      });
      entry.on('end', () => { if (size !== item.size || hash.digest('hex') !== item.sha256) problem = true; });
    } });
    // The pinned parser reports unsupported entries and over-limit metadata separately.
    listing.on('ignoredEntry', () => { problem = true; listing.abort(new PackError('archive')); });
    listing.on('meta', () => { problem = true; listing.abort(new PackError('archive')); });
    const inflater = createInflateRaw();
    try {
      // pipeline destroys streams on failure even with autoClose:false. Let the
      // ReadStream drain pending reads, but leave the actual close to finally.
      const stream = createReadStream(null, { fd, autoClose: false, start: GZIP_HEADER_BYTES,
        end: archiveSize - GZIP_FOOTER_BYTES - 1,
        fs: { read, close(_fd, callback) { callback(null); } } });
      await pipeline(stream, compressed, inflater, bounds, listing);
    } catch { reject('archive'); }
    size += footer.length;
    sha256.update(footer); sha512.update(footer);
    canonicalHash.update(Buffer.alloc(2 * TAR_BLOCK_BYTES));
    // InflateRaw can ignore trailing input; require consumption of the entire
    // segment, not just a valid first stream or canonical expanded TAR geometry.
    if (inflater.bytesWritten !== archiveSize - GZIP_HEADER_BYTES - GZIP_FOOTER_BYTES ||
        checksum !== footer.readUInt32LE(0) || expanded !== footer.readUInt32LE(4) ||
        problem || seen.size !== FILES.size || expanded !== canonicalSize || BigInt(size) !== stat.size ||
        expandedHash.digest('hex') !== canonicalHash.digest('hex')) reject('archive');
    if (!sameFile(stat, fstatSync(fd, { bigint: true })) || !sameFile(stat, lstatSync(file, { bigint: true }))) reject('archive');
    result = { size, sha256: sha256.digest('hex'), integrity: `sha512-${sha512.digest('base64')}`,
      files: inventory.map((item) => ({ ...item })) };
  } catch (error) { primary = error instanceof PackError ? error : new PackError('archive'); }
  finally {
    if (fd !== undefined) { try { closeSync(fd); } catch { primary ??= new PackError('io'); } }
  }
  if (primary) throw primary;
  return result;
}

/** Independently list with pinned tar's supported API; never extract an archive. */
export async function validateTarball(file, inventory) {
  const { files } = await readTarball(file, inventory);
  return files;
}

function runNpm(npm, args, cwd, env) {
  try {
    return execFileSync(process.execPath, [npm, ...args], { cwd, env, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'], maxBuffer: 1024 * 1024, timeout: 120_000 });
  } catch { reject('tool'); }
}

/** Private local packing only. Does not execute helpers, install, publish or grant rights. */
export async function packDistribution(directory, outputDirectory) {
  let scratch;
  let output;
  let ownsOutput = false;
  let primary;
  let result;
  try {
    directory = localPath(directory);
    output = localPath(outputDirectory);
    if (process.versions.node !== '24.13.0') reject('tool');
    const inventory = inspectDistribution(directory);
    const metadata = readReleaseMetadata(SOURCE);
    const filename = `${metadata.name.slice(1).replace('/', '-')}-${metadata.version}.tgz`;
    directory = realpathSync(directory);
    output = join(realpathSync(dirname(output)), basename(output));
    scratch = realpathSync(mkdtempSync(join(tmpdir(), 'possums-pi-pack-')));
    const staged = join(scratch, 'distribution');
    mkdirSync(staged);
    for (const path of DIRECTORIES) mkdirSync(join(staged, path), { recursive: true });
    for (const item of inventory) {
      const copied = readRegular(join(directory, item.path), item.path, join(staged, item.path));
      if (!isDeepStrictEqual(copied, item)) reject('files');
    }
    if (!isDeepStrictEqual(inspectDistribution(staged), inventory)) reject('files');
    for (const path of ['home', 'cache', 'tmp']) mkdirSync(join(scratch, path));
    const userconfig = join(scratch, 'user.npmrc');
    const globalconfig = join(scratch, 'global.npmrc');
    writeFileSync(userconfig, '');
    writeFileSync(globalconfig, '');
    // Intentionally no inherited npm/auth, proxy, NODE_OPTIONS or personal environment.
    const env = {
      PATH: dirname(process.execPath), HOME: join(scratch, 'home'), TMPDIR: join(scratch, 'tmp'),
      npm_config_userconfig: userconfig, npm_config_globalconfig: globalconfig,
      npm_config_cache: join(scratch, 'cache'), npm_config_offline: 'true',
      npm_config_workspaces: 'false', npm_config_ignore_scripts: 'true',
      npm_config_update_notifier: 'false', npm_config_audit: 'false', npm_config_fund: 'false', npm_config_logs_max: '0',
    };
    // Nix can resolve execPath to nodejs-slim, while argv0 retains the paired npm location.
    const toolDirectories = [dirname(resolve(process.argv0)), ...(process.env.PATH ?? '').split(delimiter).filter(Boolean)];
    const npmPath = toolDirectories.map((path) => join(path, 'npm')).find((path) => existsSync(path));
    if (!npmPath) reject('tool');
    const npm = realpathSync(npmPath);
    if (runNpm(npm, ['--version'], staged, env).trim() !== '11.6.2') reject('tool');
    mkdirSync(output, { mode: 0o700 });
    ownsOutput = true;
    const text = runNpm(npm, ['pack', '--json', '--ignore-scripts', '--workspaces=false', '--offline', '--pack-destination', output], staged, env);
    let report;
    try { report = JSON.parse(text); } catch { reject('tool'); }
    if (!Array.isArray(report) || report.length !== 1 || report[0]?.name !== metadata.name ||
        report[0]?.version !== metadata.version || report[0]?.filename !== filename) reject('tool');
    if (!isDeepStrictEqual(readdirSync(output), [filename])) reject('archive');
    const archive = join(output, filename);
    const validated = await readTarball(archive, inventory);
    result = { filename, ...validated, unpackedSize: inventory.reduce((sum, item) => sum + item.size, 0) };
  } catch (error) { primary = error instanceof PackError ? error : new PackError('io'); }
  finally {
    if (scratch) { try { rmSync(scratch, { recursive: true, force: true }); } catch { primary ??= new PackError('io'); } }
    if (primary && ownsOutput) { try { rmSync(output, { recursive: true, force: true }); } catch {} }
  }
  if (primary) throw primary;
  return result;
}

if (import.meta.main) {
  try {
    if (process.argv.length !== 4) reject('input');
    console.log(JSON.stringify(await packDistribution(process.argv[2], process.argv[3])));
  } catch (error) {
    console.error(error instanceof PackError ? error.message : new PackError('io').message);
    process.exitCode = 1;
  }
}
