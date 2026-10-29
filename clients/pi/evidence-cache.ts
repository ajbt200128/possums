import { constants } from 'node:fs';
import { Buffer } from 'node:buffer';
import process from 'node:process';
import { mkdir, open, readdir, rename, rmdir, stat, unlink } from 'node:fs/promises';
import { homedir } from 'node:os';
import { isAbsolute, join } from 'node:path';
import { randomUUID } from 'node:crypto';
import { PUBLISHER, requireReleaseTag } from './core/approval.js';
import { LIMITS, DiagnosticFailure, Operation, OperationFailure, base64, digest, parseJSON, requireThat } from './core/limits.js';
import { VerifierFailure } from './verifier.mjs';
import { isTorConnectionFailure } from './tor-connection.js';
import { ConnectionFailure, EvidenceObservation } from './diagnostics.js';

// Public acquisition entries, never histories, verification verdicts or clients.
export type PublicEvidence = { tag: string; manifest: Uint8Array<ArrayBuffer>; provenance: Uint8Array<ArrayBuffer>; vcek?: Uint8Array<ArrayBuffer> };
export type CompletePublicEvidence = PublicEvidence & { vcek: Uint8Array<ArrayBuffer> };
const CAP = (2 * Math.ceil(LIMITS.provenance / 3) + Math.ceil(LIMITS.certificate / 3)) * 4 + 1024;
const HASH = /^[0-9a-f]{64}$/;
export function evidenceDirectory(): string {
  const root = process.env.XDG_CACHE_HOME;
  return join(root && isAbsolute(root) ? root : join(homedir(), '.cache'), 'possums', 'pi-public-evidence-v1');
}
function errno(error: unknown, code: string): boolean {
  return error instanceof Error && 'code' in error && error.code === code;
}
function cacheFailure(error: unknown): ConnectionFailure {
  return error instanceof ConnectionFailure ? error : new ConnectionFailure('evidence_unavailable',
    new EvidenceObservation('public_evidence_cache', error instanceof OperationFailure ? error.constraint : 'storage'));
}
async function readEntry(file: string, hint: string): Promise<PublicEvidence | undefined> {
  let handle;
  try {
    handle = await open(file, constants.O_RDONLY | constants.O_NOFOLLOW);
    const info = await handle.stat();
    if (!info.isFile() || info.size > CAP) return;
    const bytes = new Uint8Array(CAP + 1);
    const { bytesRead } = await handle.read(bytes, 0, bytes.length, 0);
    if (bytesRead !== info.size || bytesRead > CAP) return;
    // Malformed/partial entries are misses. Never copy arbitrary cache fields.
    try {
      const entry = parseJSON(bytes.slice(0, bytesRead), CAP);
      requireThat(entry.version === 1 || entry.version === 2);
      const fields = 'digest,hint,manifest,origin,provenance,repository,tag,' + (entry.version === 2 ? 'vcek,' : '') + 'version';
      requireThat(Object.keys(entry).sort().join(',') === fields);
      requireThat(entry.origin === PUBLISHER.origin && entry.repository === PUBLISHER.repository);
      requireThat(HASH.test(entry.hint) && entry.hint === hint && HASH.test(entry.digest));
      requireReleaseTag(entry.tag);
      const manifest = base64(entry.manifest, LIMITS.provenance), provenance = base64(entry.provenance, LIMITS.provenance);
      requireThat(await digest(manifest) === entry.digest);
      if (entry.version === 1) return { tag: entry.tag, manifest, provenance };
      const vcek = base64(entry.vcek, LIMITS.certificate);
      requireThat(vcek.length > 0);
      return { tag: entry.tag, manifest, provenance, vcek };
    } catch { return; }
  } catch (error) {
    if (errno(error, 'ENOENT') || errno(error, 'ELOOP')) return;
    throw cacheFailure(error);
  } finally { await handle?.close(); }
}
function alive(pid: number): boolean {
  try { process.kill(pid, 0); return true; }
  catch (error) { return !errno(error, 'ESRCH'); }
}
async function removeOwner(lock: string, owner: string): Promise<void> {
  // Only the winner removing this unique child may remove its parent. A second
  // reaper/releasing owner cannot remove a replacement's nonempty lock.
  try { await rmdir(join(lock, owner)); } catch (error) { if (errno(error, 'ENOENT')) return; throw error; }
  try { await rmdir(lock); } catch (error) { if (!errno(error, 'ENOENT') && !errno(error, 'ENOTEMPTY')) throw error; }
}
async function takeLock(lock: string, op: Operation): Promise<string> {
  for (;;) {
    op.check();
    const owner = `${process.pid}.${Date.now() + LIMITS.bootstrapMs}.${randomUUID()}`;
    try {
      await mkdir(lock, { mode: 0o700 });
      try { await mkdir(join(lock, owner), { mode: 0o700 }); op.check(); return owner; }
      catch (error) { await removeOwner(lock, owner).catch(() => {}); throw error; }
    } catch (error) { if (!errno(error, 'EEXIST')) throw error; }
    try {
      const owners = await readdir(lock);
      if (owners.length === 0) {
        // Crash between the two mkdir calls; never reap a normally initializing owner.
        if (Date.now() - (await stat(lock)).mtimeMs > LIMITS.bootstrapMs) await rmdir(lock);
      } else {
        requireThat(owners.length === 1);
        const match = /^([1-9][0-9]*)\.([0-9]+)\.([0-9a-f-]{36})$/.exec(owners[0]);
        requireThat(match && Number.isSafeInteger(Number(match[1])) && Number.isSafeInteger(Number(match[2])));
        if (!alive(Number(match[1])) || Date.now() >= Number(match[2])) await removeOwner(lock, owners[0]);
      }
    } catch (error) { if (!errno(error, 'ENOENT') && !errno(error, 'ENOTEMPTY')) throw error; }
    await op.wait(new Promise<void>(resolve => setTimeout(resolve, 25)), null);
  }
}

const MANIFEST_CAP = 4 * Math.ceil(LIMITS.provenance / 3) + 1024;
async function readManifest(file: string, tag: string, expectedDigest: string): Promise<Uint8Array<ArrayBuffer> | undefined> {
  let handle;
  try {
    handle = await open(file, constants.O_RDONLY | constants.O_NOFOLLOW);
    const info = await handle.stat();
    if (!info.isFile() || info.size > MANIFEST_CAP) return;
    const bytes = new Uint8Array(MANIFEST_CAP + 1);
    const { bytesRead } = await handle.read(bytes, 0, bytes.length, 0);
    if (bytesRead !== info.size || bytesRead > MANIFEST_CAP) return;
    try {
      const entry = parseJSON(bytes.slice(0, bytesRead), MANIFEST_CAP);
      requireThat(Object.keys(entry).sort().join(',') === 'digest,manifest,origin,repository,tag,version');
      requireThat(entry.version === 1 && entry.origin === PUBLISHER.origin && entry.repository === PUBLISHER.repository
        && entry.tag === tag && entry.digest === expectedDigest);
      const manifest = base64(entry.manifest, LIMITS.provenance);
      requireThat(manifest.length > 0 && await digest(manifest) === expectedDigest);
      return manifest;
    } catch { return; }
  } catch (error) {
    if (errno(error, 'ENOENT') || errno(error, 'ELOOP')) return;
    throw error;
  } finally { await handle?.close(); }
}

// V3 keeps only one public manifest. Fresh nonce-bound evidence supplies the
// release hint; every hit still passes complete channel verification. The
// legacy directory/schema is deliberately independent, never silently migrated.
export async function publicManifest<T>(tag: string, expectedDigest: string, op: Operation,
  acquire: () => Promise<Uint8Array<ArrayBuffer>>, verify: (manifest: Uint8Array<ArrayBuffer>) => Promise<T>,
  directory = join(evidenceDirectory(), '..', 'pi-public-manifest-v3')): Promise<T> {
  const file = join(directory, 'entry.json'), lock = join(directory, 'acquisition.lock');
  let owner: string | undefined, temporary: string | undefined;
  try {
    requireReleaseTag(tag); requireThat(HASH.test(expectedDigest)); op.check();
    const cached = await readManifest(file, tag, expectedDigest);
    op.check();
    if (cached) { const result = await verify(cached); op.check(); return result; }
    await mkdir(directory, { recursive: true, mode: 0o700 });
    owner = await takeLock(lock, op);
    const shared = await readManifest(file, tag, expectedDigest);
    op.check();
    if (shared) { const result = await verify(shared); op.check(); return result; }
    const acquired = await acquire();
    op.check();
    if (!(acquired instanceof Uint8Array) || !acquired.length || acquired.length > LIMITS.provenance)
      throw new ConnectionFailure('verification_failed', new EvidenceObservation('manifest_download', 'binding'));
    const manifest = new Uint8Array(acquired);
    if (await digest(manifest) !== expectedDigest)
      throw new ConnectionFailure('verification_failed', new EvidenceObservation('manifest_download', 'binding'));
    const result = await verify(new Uint8Array(manifest));
    op.check();
    const record = JSON.stringify({ version: 1, origin: PUBLISHER.origin, repository: PUBLISHER.repository,
      tag, digest: expectedDigest, manifest: Buffer.from(manifest).toString('base64') });
    requireThat(Buffer.byteLength(record) <= MANIFEST_CAP);
    temporary = join(directory, `entry-${randomUUID()}.tmp`);
    const handle = await open(temporary, 'wx', 0o600);
    try { await handle.writeFile(record); } finally { await handle.close(); }
    op.check();
    const deadline = Number(owner.split('.')[1]);
    requireThat(Date.now() < deadline && (await readdir(lock)).includes(owner));
    op.check();
    await rename(temporary, file); temporary = undefined;
    op.check(); return result;
  } catch (error) {
    if (error instanceof DiagnosticFailure || error instanceof VerifierFailure || isTorConnectionFailure(error)) throw error;
    throw cacheFailure(error);
  } finally {
    if (temporary) await unlink(temporary).catch(() => {});
    if (owner) await removeOwner(lock, owner).catch(() => {});
  }
}

export async function publicEvidence<T>(hint: string, op: Operation,
  acquire: (legacy?: PublicEvidence) => Promise<CompletePublicEvidence>, verify: (entry: CompletePublicEvidence) => Promise<T>, directory = evidenceDirectory()): Promise<T> {
  const file = join(directory, 'entry.json'), lock = join(directory, 'acquisition.lock');
  let owner: string | undefined, temporary: string | undefined;
  try {
    requireThat(HASH.test(hint));
    op.check();
    const cached = await readEntry(file, hint);
    op.check();
    if (cached?.vcek) return await verify({ ...cached, vcek: cached.vcek }); // Verify in EVERY process.
    await mkdir(directory, { recursive: true, mode: 0o700 });
    owner = await takeLock(lock, op);
    const shared = await readEntry(file, hint);
    op.check();
    if (shared?.vcek) return await verify({ ...shared, vcek: shared.vcek });
    // A matching strict v1 entry saves GitHub acquisition during migration.
    const entry = await acquire(shared);
    op.check();
    requireThat(Object.keys(entry).sort().join(',') === 'manifest,provenance,tag,vcek');
    requireReleaseTag(entry.tag);
    for (const [bytes, cap] of [[entry.manifest, LIMITS.provenance], [entry.provenance, LIMITS.provenance], [entry.vcek, LIMITS.certificate]] as const) {
      requireThat(bytes instanceof Uint8Array && bytes.length <= cap);
    }
    requireThat(entry.vcek.length > 0);
    const result = await verify(entry); // Publish only after the complete serving-channel checks.
    op.check();
    const record = JSON.stringify({ version: 2, origin: PUBLISHER.origin, repository: PUBLISHER.repository,
      hint, tag: entry.tag, digest: await digest(entry.manifest),
      manifest: Buffer.from(entry.manifest).toString('base64'), provenance: Buffer.from(entry.provenance).toString('base64'),
      vcek: Buffer.from(entry.vcek).toString('base64') });
    requireThat(Buffer.byteLength(record) <= CAP);
    temporary = join(directory, `entry-${randomUUID()}.tmp`);
    const handle = await open(temporary, 'wx', 0o600);
    try { await handle.writeFile(record); } finally { await handle.close(); }
    op.check();
    // A cancelled/expired owner must not publish over its successor.
    const deadline = Number(owner.split('.')[1]);
    requireThat(Date.now() < deadline && (await readdir(lock)).includes(owner));
    op.check();
    await rename(temporary, file); temporary = undefined;
    op.check();
    return result;
  } catch (error) { throw cacheFailure(error); }
  finally {
    if (temporary) await unlink(temporary).catch(() => {});
    if (owner) await removeOwner(lock, owner).catch(() => {});
  }
}
