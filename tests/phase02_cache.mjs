// Real bounded child processes; all evidence and verifiers here are synthetic.
import assert from 'node:assert/strict';
import { fork } from 'node:child_process';
import { mkdir, mkdtemp, readFile, writeFile, rm, readdir, utimes, chmod } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { pathToFileURL, fileURLToPath } from 'node:url';
const bundle = process.env.PHASE02_TEST_BUILD;
assert(bundle && path.isAbsolute(bundle));
const { publicEvidence, Operation, ConnectionFailure, EvidenceObservation, LIMITS } = await import(pathToFileURL(bundle));
const hint = 'a'.repeat(64), changed = 'b'.repeat(64);
const entry = { tag: 'v1.2.3', manifest: new TextEncoder().encode('public manifest'), provenance: new TextEncoder().encode('public provenance'), vcek: new Uint8Array([1, 2, 3]) };
const hostile = 'HOSTILE credential URL prompt history stack';
if (process.env.CACHE_CHILD) {
  const op = new Operation(2000);
  try {
    await publicEvidence(process.env.CACHE_HINT, op, async legacy => {
      if (!legacy) process.send({ event: 'download' });
      else assert.deepEqual(legacy, { tag: entry.tag, manifest: entry.manifest, provenance: entry.provenance });
      process.send({ event: 'amd' });
      await new Promise(resolve => setTimeout(resolve, process.env.CACHE_CRASH ? 10000 : 75));
      return entry;
    }, async value => { assert.deepEqual(value, entry); process.send({ event: 'verify' }); return true; }, process.env.CACHE_DIRECTORY);
    process.send({ event: 'done' });
  } catch (error) { process.send({ event: 'failure', code: error.code, message: error.message }); process.exitCode = 1; }
  finally { op.close(); process.disconnect(); }
} else {
  const root = await mkdtemp(path.join(tmpdir(), 'possums-public-cache-test-'));
  let passed = 0;
  const directory = path.join(root, 'cache');
  function child(h = hint) {
    return new Promise((resolve, reject) => {
      const events = [];
      const worker = fork(fileURLToPath(import.meta.url), [], { env: { PHASE02_TEST_BUILD: bundle, CACHE_CHILD: '1', CACHE_HINT: h, CACHE_DIRECTORY: directory }, stdio: ['ignore', 'ignore', 'pipe', 'ipc'] });
      let stderr = ''; worker.stderr.on('data', data => { stderr += data; });
      const timer = setTimeout(() => { worker.kill(); reject(new Error('bounded cache fixture exceeded deadline')); }, 5000);
      worker.on('message', event => events.push(event));
      worker.on('error', reject);
      worker.on('exit', code => { clearTimeout(timer); code === 0 ? resolve(events) : reject(new Error('cache child fixture failed: ' + stderr)); });
    });
  }
  async function attempt(dir = directory, acquire = async () => entry, verify = async value => { assert.deepEqual(value, entry); return true; }, signal) {
    const op = new Operation(2000, signal);
    try { return await publicEvidence(hint, op, acquire, verify, dir); } finally { op.close(); }
  }
  try {
    const runs = (await Promise.all(Array.from({ length: 6 }, () => child()))).flat();
    assert.equal(runs.filter(e => e.event === 'download').length, 1);
    assert.equal(runs.filter(e => e.event === 'amd').length, 1);
    assert.equal(runs.filter(e => e.event === 'verify').length, 6); passed++;
    const warm = await child();
    assert.equal(warm.filter(e => e.event === 'download' || e.event === 'amd').length, 0); passed++;
    const changedRuns = (await Promise.all(Array.from({ length: 4 }, () => child(changed)))).flat();
    assert.equal(changedRuns.filter(e => e.event === 'download').length, 1);
    assert.equal(changedRuns.filter(e => e.event === 'amd').length, 1);
    assert.equal(changedRuns.filter(e => e.event === 'verify').length, 4); passed++;
    const record = JSON.parse(await readFile(path.join(directory, 'entry.json'), 'utf8'));
    assert.equal(record.hint, changed); assert.equal(record.version, 2);
    assert.equal(record.vcek, Buffer.from(entry.vcek).toString('base64'));
    assert(!JSON.stringify(record).includes('verified')); passed++;
    const { vcek: _vcek, ...withoutVcek } = record;
    const legacy = { ...withoutVcek, hint, version: 1 };
    await writeFile(path.join(directory, 'entry.json'), JSON.stringify(legacy));
    const migrated = (await Promise.all(Array.from({ length: 6 }, () => child()))).flat();
    assert.equal(migrated.filter(e => e.event === 'download').length, 0);
    assert.equal(migrated.filter(e => e.event === 'amd').length, 1);
    assert.equal(migrated.filter(e => e.event === 'verify').length, 6);
    assert.equal(JSON.parse(await readFile(path.join(directory, 'entry.json'))).version, 2); passed++;
    assert.equal((await child()).filter(e => e.event === 'amd').length, 0); passed++;
    for (const bad of ['{', '{}', JSON.stringify({ ...record, hint, digest: '0'.repeat(64) }), JSON.stringify({ ...record, hint, repository: hostile }), JSON.stringify({ ...record, hint, credentials: hostile }),
      JSON.stringify({ ...withoutVcek, hint }), JSON.stringify({ ...record, hint, vcek: '' }),
      JSON.stringify({ ...record, hint, vcek: hostile }), JSON.stringify({ ...record, hint, vcek: Buffer.alloc(LIMITS.certificate + 1).toString('base64') }),
      JSON.stringify({ ...record, hint, version: 1 }), JSON.stringify({ ...record, hint, version: 3 })]) {
      await writeFile(path.join(directory, 'entry.json'), bad);
      let downloads = 0; await attempt(directory, async () => { downloads++; return entry; });
      assert.equal(downloads, 1); passed++;
    }
    // Cached valid-looking bytes must still reach this process's verifier.
    await assert.rejects(attempt(directory, async () => { throw new Error('must not download'); }, async () => {
      throw new ConnectionFailure('verification_failed');
    }), e => e.code === 'verification_failed'); passed++;
    // Structurally valid wrong certificate bytes are verifier input, not a miss.
    await writeFile(path.join(directory, 'entry.json'), JSON.stringify({ ...record, hint, vcek: 'BA==' }));
    let fallback = 0;
    await assert.rejects(attempt(directory, async () => { fallback++; return entry; }, async value => {
      assert.deepEqual(value.vcek, new Uint8Array([4])); throw new ConnectionFailure('verification_failed');
    }), e => e.code === 'verification_failed');
    assert.equal(fallback, 0); passed++;
    await writeFile(path.join(directory, 'entry.json'), JSON.stringify(legacy));
    await assert.rejects(attempt(directory, async reused => {
      assert.equal(reused.tag, entry.tag);
      throw new ConnectionFailure('evidence_unavailable', new EvidenceObservation('amd_certificate', 'rate_limited', 429));
    }), e => e.message.includes('Stage: amd_certificate') && e.message.includes('HTTP status: 429'));
    assert.deepEqual(JSON.parse(await readFile(path.join(directory, 'entry.json'))), legacy); passed++;
    await rm(path.join(directory, 'entry.json'));
    for (const bad of [{ ...entry, vcek: new Uint8Array() }, { ...entry, vcek: new Uint8Array(LIMITS.certificate + 1) },
      { ...entry, vcek: hostile }, { ...entry, tag: hostile }, { ...entry, manifest: new Uint8Array(LIMITS.provenance + 1) },
      { ...entry, provenance: hostile }, { ...entry, credentials: hostile }]) {
      let verified = 0;
      await assert.rejects(attempt(directory, async () => bad, async () => { verified++; }), e => !e.message.includes(hostile));
      assert.equal(verified, 0); assert.deepEqual(await readdir(directory), []); passed++;
    }
    const lock = path.join(directory, 'acquisition.lock');
    await new Promise((resolve, reject) => {
      const worker = fork(fileURLToPath(import.meta.url), [], { env: { PHASE02_TEST_BUILD: bundle, CACHE_CHILD: '1', CACHE_CRASH: '1', CACHE_HINT: hint, CACHE_DIRECTORY: directory }, stdio: ['ignore', 'ignore', 'ignore', 'ipc'] });
      const timer = setTimeout(() => { worker.kill('SIGKILL'); reject(new Error('crash fixture deadline')); }, 5000);
      worker.on('message', event => { if (event.event === 'download') worker.kill('SIGKILL'); });
      worker.on('exit', (_code, signal) => { clearTimeout(timer); if (signal === 'SIGKILL') resolve(); else reject(new Error('crash fixture did not reach owner')); });
    });
    const recovered = (await Promise.all(Array.from({ length: 4 }, () => child()))).flat();
    assert.equal(recovered.filter(e => e.event === 'download').length, 1); passed++;
    await rm(path.join(directory, 'entry.json'));
    await mkdir(path.join(lock, `2147483647.${Date.now() + LIMITS.bootstrapMs}.00000000-0000-0000-0000-000000000001`), { recursive: true });
    await attempt(); assert.deepEqual(await readdir(directory), ['entry.json']); passed++;
    await rm(path.join(directory, 'entry.json'));
    await mkdir(path.join(lock, `${process.pid}.${Date.now() - 1}.00000000-0000-0000-0000-000000000002`), { recursive: true });
    await attempt(); passed++;
    await rm(path.join(directory, 'entry.json')); await mkdir(lock);
    const past = new Date(Date.now() - LIMITS.bootstrapMs - 1000); await utimes(lock, past, past);
    await attempt(); passed++;
    const controller = new AbortController(); controller.abort();
    let calls = 0;
    await assert.rejects(attempt(directory, async () => { calls++; return entry; }, async () => { calls++; }, controller.signal), e => e.message.includes('constraint: interrupted'));
    assert.equal(calls, 0); passed++;
    await rm(path.join(directory, 'entry.json'));
    const during = new AbortController();
    await assert.rejects(attempt(directory, async () => { during.abort(); return entry; }, async () => true, during.signal), e => !e.message.includes(hostile));
    assert.deepEqual(await readdir(directory), []); passed++;
    await mkdir(path.join(lock, `${process.pid}.${Date.now() + LIMITS.bootstrapMs}.00000000-0000-0000-0000-000000000003`), { recursive: true });
    const waiting = new AbortController();
    const timer = setTimeout(() => waiting.abort(), 75);
    await assert.rejects(attempt(directory, async () => { calls++; return entry; }, async () => { calls++; }, waiting.signal), e => e.message.includes('constraint: interrupted'));
    clearTimeout(timer); assert.equal(calls, 0); await rm(lock, { recursive: true }); passed++;
    await assert.rejects(attempt(directory, async () => entry, async () => {
      await rm(lock, { recursive: true });
      await mkdir(path.join(lock, `${process.pid}.${Date.now() + LIMITS.bootstrapMs}.00000000-0000-0000-0000-000000000004`), { recursive: true });
    }), e => e.message.includes('public_evidence_cache'));
    assert.deepEqual(await readdir(directory), ['acquisition.lock']); await rm(lock, { recursive: true }); passed++;
    await chmod(directory, 0o500);
    try { await assert.rejects(attempt(), e => e.message.includes('constraint: storage') && !e.message.includes(root)); }
    finally { await chmod(directory, 0o700); }
    passed++;
    // A failure never publishes an acquisition; primary observed status survives.
    await assert.rejects(attempt(directory, async () => { throw new ConnectionFailure('evidence_unavailable', new EvidenceObservation('release_discovery', 'rate_limited', 403)); }), e => e.message.includes('HTTP status: 403'));
    assert.deepEqual(await readdir(directory), []); passed++;
    const blocked = path.join(root, 'not-directory'); await writeFile(blocked, hostile);
    await assert.rejects(attempt(blocked), e => e.message.includes('public_evidence_cache') && e.message.includes('constraint: storage') && !e.message.includes(hostile) && e.cause === undefined); passed++;
    // Write failure after validation: no raw filesystem error and no partial entry.
    await mkdir(path.join(directory, 'entry.json'));
    await assert.rejects(attempt(), e => e.message.includes('public_evidence_cache') && !e.message.includes('EISDIR') && !e.message.includes(root)); passed++;
    console.log(JSON.stringify({ passed, scope: 'real cross-process synthetic acquisition cache', inference: 0, credentials: 0 }));
  } finally { await rm(root, { recursive: true, force: true }); }
}
