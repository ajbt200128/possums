// Offline synthetic public bytes only. Run against the pinned scratch test bundle.
// Privacy scope: PRIVACY.md, Data handling boundaries and Mandatory typed interface failures.
import assert from 'node:assert/strict';
import { fork } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdir, mkdtemp, readFile, writeFile, rm, readdir, stat, utimes, symlink } from 'node:fs/promises';
import { homedir, tmpdir } from 'node:os';
import path from 'node:path';
import { pathToFileURL, fileURLToPath } from 'node:url';

const bundle = process.env.PHASE02_TEST_BUILD;
assert(bundle && path.isAbsolute(bundle), 'absolute pinned PHASE02_TEST_BUILD required');
const { publicManifest, evidenceDirectory, Operation, LIMITS, ConnectionFailure, DiagnosticFailure } = await import(pathToFileURL(bundle));
assert.equal(typeof publicManifest, 'function', 'scratch test entry must export publicManifest');
const tag = 'v1.2.3', nextTag = 'v1.2.4';
const manifest = new TextEncoder().encode('{"synthetic":"public release manifest"}\n');
const changedManifest = new TextEncoder().encode('{"synthetic":"changed public release manifest"}\n');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const expectedDigest = hash(manifest), changedDigest = hash(changedManifest);
const cap = 4 * Math.ceil(LIMITS.provenance / 3) + 1024;
const hostile = 'SYNTHETIC_PRIVATE_credential_prompt_history_URL_stack';
const privateResult = Object.freeze({ nonce: 'SYNTHETIC_NONCE', report: 'SYNTHETIC_REPORT', verdict: 'SYNTHETIC_VERDICT', credential: hostile });
const origin = 'https://gateway.possums.dev', repository = 'ajbt200128/possums';
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
const recordFor = (bytes = manifest, release = tag) => ({ version: 1, origin, repository, tag: release, digest: hash(bytes), manifest: Buffer.from(bytes).toString('base64') });

if (process.env.MANIFEST_CACHE_CHILD) {
  const op = new Operation(8000);
  const tell = event => process.send({ event });
  try {
    // The parent releases all six ready children together, rather than relying on
    // process startup timing to make the first acquisition genuinely contended.
    await new Promise(resolve => { process.once('message', message => { assert.equal(message, 'go'); resolve(); }); tell('ready'); });
    await publicManifest(tag, expectedDigest, op, async () => {
      tell('acquire');
      await delay(process.env.MANIFEST_CACHE_CRASH ? 20000 : 100);
      return manifest.slice();
    }, async value => {
      assert.deepEqual(value, manifest);
      assert.equal(hash(value), expectedDigest);
      tell('verify');
      return privateResult;
    }, process.env.MANIFEST_CACHE_DIRECTORY);
    tell('done');
  } catch {
    // Child stderr/IPC never carry a raw exception or local path.
    tell('failure'); process.exitCode = 1;
  } finally { op.close(); process.disconnect(); }
} else {
  const root = await mkdtemp(path.join(tmpdir(), 'possums-public-manifest-test-'));
  const directory = path.join(root, 'explicit-cache'), file = path.join(directory, 'entry.json');
  const lock = path.join(directory, 'acquisition.lock');
  const workers = new Set();
  let passed = 0;
  const savedXdg = process.env.XDG_CACHE_HOME;
  async function attempt({ dir = directory, release = tag, digest = expectedDigest, signal,
    acquire = async () => manifest.slice(), verify = async value => { assert.deepEqual(value, manifest); return privateResult; } } = {}) {
    const op = new Operation(4000, signal);
    try { return await publicManifest(release, digest, op, acquire, verify, dir); }
    finally { op.close(); }
  }
  async function reset() { await rm(directory, { recursive: true, force: true }); }
  async function empty() {
    assert.deepEqual(await readdir(directory).catch(error => { if (error.code === 'ENOENT') return []; throw error; }), []);
  }
  function storageFailure(error) {
    assert(error instanceof ConnectionFailure);
    assert.equal(error.code, 'evidence_unavailable');
    assert.equal(error.observation?.stage, 'public_evidence_cache');
    assert.equal(error.observation?.constraint, 'storage');
    assert.equal(error.cause, undefined);
    for (const forbidden of [hostile, root, 'EISDIR', 'ENOTDIR', 'EACCES', 'ENOENT']) assert(!error.message.includes(forbidden));
    return true;
  }
  function interrupted(error) {
    assert(error instanceof ConnectionFailure);
    assert.equal(error.code, 'evidence_unavailable');
    assert.equal(error.observation?.stage, 'public_evidence_cache');
    assert.equal(error.observation?.constraint, 'interrupted');
    assert.equal(error.cause, undefined);
    assert(!error.message.includes(root));
    return true;
  }
  async function inspect(dir = directory, bytes = manifest, release = tag) {
    // Parse with Node's independent JSON/base64/SHA-256 primitives, not production
    // cache helpers; equality admits ONLY public acquisition bytes and bindings.
    assert.deepEqual(await readdir(dir), ['entry.json']);
    const raw = await readFile(path.join(dir, 'entry.json'));
    assert(raw.length <= cap);
    const parsed = JSON.parse(raw.toString('utf8'));
    assert.deepEqual(Object.keys(parsed).sort(), ['digest', 'manifest', 'origin', 'repository', 'tag', 'version']);
    assert.deepEqual(parsed, recordFor(bytes, release));
    const decoded = Buffer.from(parsed.manifest, 'base64');
    assert.equal(decoded.toString('base64'), parsed.manifest);
    assert.deepEqual(decoded, Buffer.from(bytes));
    assert.equal(hash(decoded), parsed.digest);
    assert(decoded.length > 0 && decoded.length <= LIMITS.provenance);
    for (const sentinel of Object.values(privateResult)) assert(!raw.includes(Buffer.from(sentinel)));
    assert.equal((await stat(dir)).mode & 0o777, 0o700);
    assert.equal((await stat(path.join(dir, 'entry.json'))).mode & 0o777, 0o600);
    return raw;
  }
  function startChild(crash = false) {
    const events = [];
    const worker = fork(fileURLToPath(import.meta.url), [], {
      env: { PHASE02_TEST_BUILD: bundle, MANIFEST_CACHE_CHILD: '1', MANIFEST_CACHE_DIRECTORY: directory,
        ...(crash ? { MANIFEST_CACHE_CRASH: '1' } : {}) },
      stdio: ['ignore', 'ignore', 'ignore', 'ipc'],
    });
    workers.add(worker);
    let readyResolve, readyReject, ownerResolve, ownerReject;
    const ready = new Promise((resolve, reject) => { readyResolve = resolve; readyReject = reject; });
    const owner = new Promise((resolve, reject) => { ownerResolve = resolve; ownerReject = reject; });
    // Warm children do not acquire; attach a handler even if owner is unused.
    void owner.catch(() => {});
    let timeout = false;
    const timer = setTimeout(() => { timeout = true; worker.kill('SIGKILL'); }, 12000);
    const done = new Promise((resolve, reject) => {
      worker.on('message', message => {
        events.push(message.event);
        if (message.event === 'ready') readyResolve();
        if (message.event === 'acquire') ownerResolve();
      });
      worker.once('error', () => { readyReject(new Error('manifest child startup failed')); ownerReject(new Error('manifest child startup failed')); });
      worker.once('close', (code, signal) => {
        clearTimeout(timer); workers.delete(worker);
        readyReject(new Error('manifest child ended before ready'));
        ownerReject(new Error('manifest child ended before acquisition'));
        if (timeout || (crash ? signal !== 'SIGKILL' : code !== 0)) reject(new Error('bounded synthetic manifest child failed'));
        else resolve(events);
      });
    });
    // Observe failures immediately while the parent is still waiting for ready.
    void done.catch(() => {});
    return { worker, ready, owner, done };
  }
  async function concurrent(count) {
    const children = Array.from({ length: count }, () => startChild());
    await Promise.all(children.map(child => child.ready));
    for (const child of children) child.worker.send('go');
    return await Promise.all(children.map(child => child.done));
  }
  async function miss(raw) {
    await mkdir(directory, { recursive: true, mode: 0o700 });
    await writeFile(file, raw, { mode: 0o600 });
    let acquisitions = 0, verifications = 0;
    await attempt({ acquire: async () => { acquisitions++; return manifest.slice(); }, verify: async value => {
      verifications++; assert.deepEqual(value, manifest); return privateResult;
    } });
    assert.equal(acquisitions, 1); assert.equal(verifications, 1);
    await inspect(); passed++;
  }
  try {
    // Per-user default API: absolute XDG accepted, relative/empty XDG rejected.
    delete process.env.XDG_CACHE_HOME;
    assert.equal(evidenceDirectory(), path.join(homedir(), '.cache', 'possums', 'pi-public-evidence-v1'));
    for (const value of ['', 'relative-cache']) {
      process.env.XDG_CACHE_HOME = value;
      assert.equal(evidenceDirectory(), path.join(homedir(), '.cache', 'possums', 'pi-public-evidence-v1'));
    }
    process.env.XDG_CACHE_HOME = path.join(root, 'user-cache');
    const legacyDirectory = evidenceDirectory();
    assert.equal(legacyDirectory, path.join(root, 'user-cache', 'possums', 'pi-public-evidence-v1'));
    await mkdir(legacyDirectory, { recursive: true });
    const legacyFile = path.join(legacyDirectory, 'entry.json');
    await writeFile(legacyFile, hostile);
    const op = new Operation(4000);
    try {
      assert.strictEqual(await publicManifest(tag, expectedDigest, op, async () => manifest.slice(), async () => privateResult), privateResult);
    } finally { op.close(); }
    await inspect(path.join(legacyDirectory, '..', 'pi-public-manifest-v3'));
    assert.equal(await readFile(legacyFile, 'utf8'), hostile);
    passed++;
    await reset();

    const cold = await concurrent(6);
    assert.equal(cold.flat().filter(event => event === 'acquire').length, 1);
    for (const events of cold) {
      assert.equal(events.filter(event => event === 'verify').length, 1);
      assert.equal(events.filter(event => event === 'done').length, 1);
      assert(!events.includes('failure'));
    }
    const initial = await inspect(); passed++;
    const warm = await concurrent(6);
    assert.equal(warm.flat().filter(event => event === 'acquire').length, 0);
    assert.equal(warm.flat().filter(event => event === 'verify').length, 6);
    assert.deepEqual(await inspect(), initial); passed++;

    for (const [release, bytes] of [[nextTag, manifest], [nextTag, changedManifest]]) {
      let acquisitions = 0;
      await attempt({ release, digest: hash(bytes), acquire: async () => { acquisitions++; return bytes.slice(); },
        verify: async value => { assert.deepEqual(value, bytes); return privateResult; } });
      assert.equal(acquisitions, 1); await inspect(directory, bytes, release); passed++;
    }
    // Every malformed/unbound record is replaced, not passed to the verifier.
    const valid = recordFor();
    const malformed = [Buffer.from([0xff]), '', '{', '{}', 'null', '[]',
      JSON.stringify(valid).replace('"version":1', '"version":1,"version":1'),
      JSON.stringify({ ...valid, version: 2 }), JSON.stringify({ ...valid, tag: nextTag }),
      JSON.stringify({ ...valid, digest: changedDigest }), JSON.stringify({ ...valid, digest: expectedDigest.toUpperCase() }),
      JSON.stringify({ ...valid, origin: 'https://synthetic.invalid' }), JSON.stringify({ ...valid, repository: 'synthetic/wrong' }),
      JSON.stringify({ ...valid, manifest: '' }), JSON.stringify({ ...valid, manifest: hostile }),
      JSON.stringify({ ...valid, manifest: Buffer.from(changedManifest).toString('base64') }),
      JSON.stringify({ ...valid, manifest: Buffer.alloc(LIMITS.provenance + 1).toString('base64') }),
      JSON.stringify({ ...valid, manifest: null }), JSON.stringify({ ...valid, tag: { private: hostile } }),
      ' '.repeat(cap + 1), '['.repeat(LIMITS.depth + 1) + ']'.repeat(LIMITS.depth + 1)];
    for (const key of Object.keys(valid)) {
      const missing = { ...valid }; delete missing[key]; malformed.push(JSON.stringify(missing));
    }
    for (const key of ['credential', 'report', 'nonce', 'verdict', 'provenance', 'vcek', 'history', '__proto__'])
      malformed.push(JSON.stringify({ ...valid, [key]: hostile }));
    for (const raw of malformed) await miss(raw);

    // No symlink following or legacy-format migration.
    await rm(file); const target = path.join(root, 'symlink-target');
    await writeFile(target, JSON.stringify(valid)); await symlink(target, file);
    let downloads = 0;
    await attempt({ acquire: async () => { downloads++; return manifest.slice(); } });
    assert.equal(downloads, 1); await inspect();
    assert.equal(await readFile(target, 'utf8'), JSON.stringify(valid)); passed++;

    // Cache hits must still verify and may not fall back after verification fails.
    let fallback = 0;
    const denied = new DiagnosticFailure('trust', 'endpoint_binding');
    await assert.rejects(attempt({ acquire: async () => { fallback++; return manifest.slice(); }, verify: async () => { throw denied; } }), error => error === denied);
    assert.equal(fallback, 0); assert.deepEqual(await inspect(), initial); passed++;
    // A failed replacement must not publish or fall back to the older tag.
    let replacements = 0;
    await assert.rejects(attempt({ release: nextTag, acquire: async () => { replacements++; return manifest.slice(); },
      verify: async () => { throw denied; } }), error => error === denied);
    assert.equal(replacements, 1); assert.deepEqual(await inspect(), initial); passed++;
    for (const primary of [new DiagnosticFailure('trust', 'provenance', 'rejected', 403),
      new ConnectionFailure('verification_failed', new DiagnosticFailure('trust', 'certificate', 'rejected', 429))]) {
      for (const stage of ['acquire', 'verify', 'warm-verify']) {
        await reset();
        if (stage === 'warm-verify') await attempt();
        const before = stage === 'warm-verify' ? await readFile(file) : undefined;
        let acquisitions = 0, verifications = 0;
        await assert.rejects(attempt({ acquire: async () => {
          acquisitions++; if (stage === 'acquire') throw primary; return manifest.slice();
        }, verify: async () => { verifications++; throw primary; } }), error => error === primary);
        assert.equal(acquisitions, stage === 'warm-verify' ? 0 : 1);
        assert.equal(verifications, stage === 'acquire' ? 0 : 1);
        if (before) { assert.deepEqual(await readFile(file), before); await inspect(); } else await empty();
        passed++;
      }
    }
    await reset();
    for (const bad of [new Uint8Array(), new Uint8Array(LIMITS.provenance + 1), changedManifest, hostile, null]) {
      let verified = 0;
      await assert.rejects(attempt({ acquire: async () => bad, verify: async () => { verified++; } }), error => {
        assert(error instanceof ConnectionFailure); assert.equal(error.code, 'verification_failed');
        assert.equal(error.observation?.stage, 'manifest_download'); assert.equal(error.observation?.constraint, 'binding');
        assert(!error.message.includes(hostile)); return true;
      });
      assert.equal(verified, 0); await empty(); passed++;
    }
    // Invalid selectors are rejected before even attempting acquisition.
    for (const options of [{ release: hostile }, { digest: hostile }, { digest: expectedDigest.toUpperCase() }]) {
      let calls = 0;
      await assert.rejects(attempt({ ...options, acquire: async () => { calls++; return manifest.slice(); } }));
      assert.equal(calls, 0); await empty(); passed++;
    }

    // Cancellation while callbacks are pending: the callback settles explicitly;
    // this does not claim publicManifest can cancel an uncooperative callback.
    for (const stage of ['before', 'acquire', 'verify', 'warm-verify', 'waiting']) {
      await reset();
      if (stage === 'warm-verify') await attempt();
      const before = stage === 'warm-verify' ? await readFile(file) : undefined;
      if (stage === 'waiting') await mkdir(path.join(lock, `${process.pid}.${Date.now() + LIMITS.bootstrapMs}.00000000-0000-0000-0000-000000000001`), { recursive: true });
      const controller = new AbortController();
      if (stage === 'before') controller.abort();
      let acquisitions = 0, verifications = 0;
      async function cancelPending() { await Promise.resolve(); controller.abort(); await Promise.resolve(); }
      const timer = stage === 'waiting' ? setTimeout(() => controller.abort(), 50) : undefined;
      try {
        await assert.rejects(attempt({ signal: controller.signal, acquire: async () => {
          acquisitions++; if (stage === 'acquire') await cancelPending(); return manifest.slice();
        }, verify: async () => {
          verifications++; if (stage === 'verify' || stage === 'warm-verify') await cancelPending(); return privateResult;
        } }), interrupted);
      } finally { clearTimeout(timer); }
      assert.equal(acquisitions, stage === 'acquire' || stage === 'verify' ? 1 : 0);
      assert.equal(verifications, stage === 'verify' || stage === 'warm-verify' ? 1 : 0);
      if (stage === 'waiting') await rm(lock, { recursive: true });
      if (before) assert.deepEqual(await inspect(), before); else await empty();
      passed++;
    }

    await reset();
    const crashed = startChild(true);
    await crashed.ready; crashed.worker.send('go'); await crashed.owner;
    crashed.worker.kill('SIGKILL'); await crashed.done;
    assert.deepEqual(await readdir(directory), ['acquisition.lock']);
    const reclaimed = await concurrent(6);
    assert.equal(reclaimed.flat().filter(event => event === 'acquire').length, 1);
    assert.equal(reclaimed.flat().filter(event => event === 'verify').length, 6);
    await inspect(); passed++;
    // Both the expired-owner and crash-before-owner-mkdir paths are bounded.
    for (const state of ['expired', 'empty']) {
      await reset(); await mkdir(lock, { recursive: true, mode: 0o700 });
      if (state === 'expired') await mkdir(path.join(lock, `${process.pid}.${Date.now() - 1}.00000000-0000-0000-0000-000000000002`));
      else { const past = new Date(Date.now() - LIMITS.bootstrapMs - 1000); await utimes(lock, past, past); }
      await attempt(); await inspect(); passed++;
    }
    // A superseded owner cannot publish over a replacement owner.
    await reset();
    await assert.rejects(attempt({ verify: async () => {
      await rm(lock, { recursive: true });
      await mkdir(path.join(lock, `${process.pid}.${Date.now() + LIMITS.bootstrapMs}.00000000-0000-0000-0000-000000000003`), { recursive: true });
      return privateResult;
    } }), storageFailure);
    assert.deepEqual(await readdir(directory), ['acquisition.lock']); passed++;

    await reset();
    const blocked = path.join(root, 'not-a-directory'); await writeFile(blocked, hostile);
    let calls = 0;
    await assert.rejects(attempt({ dir: blocked, acquire: async () => { calls++; return manifest.slice(); } }), storageFailure);
    assert.equal(calls, 0); assert.equal(await readFile(blocked, 'utf8'), hostile); passed++;
    // Deterministic publication failure, including on privileged test runners.
    await mkdir(file, { recursive: true });
    await assert.rejects(attempt(), storageFailure);
    assert.deepEqual(await readdir(directory), ['entry.json']);
    assert((await stat(file)).isDirectory()); passed++;
    for (const stage of ['acquire', 'verify']) {
      await reset();
      await assert.rejects(attempt({ [stage]: async () => { throw new Error(`${hostile} ${root}`); } }), storageFailure);
      await empty(); passed++;
    }

    // The byte ceiling is inclusive and independently checked after publication.
    await reset(); const maximum = new Uint8Array(LIMITS.provenance).fill(49);
    await attempt({ digest: hash(maximum), acquire: async () => maximum.slice(), verify: async value => {
      assert.deepEqual(value, maximum); return privateResult;
    } });
    await inspect(directory, maximum); passed++;

    // Verifier-owned results and mutable views must not change the public bytes
    // persisted under an already checked digest. This is NOT a cached verdict.
    await reset(); const acquired = Object.assign(manifest.slice(), privateResult);
    assert.strictEqual(await attempt({ acquire: async () => acquired, verify: async value => {
      assert.deepEqual(value, manifest); value[0] ^= 1; return privateResult;
    } }), privateResult);
    await inspect();
    acquired.fill(0);
    await attempt({ acquire: async () => { throw new Error('warm mutation must not acquire'); }, verify: async value => {
      assert.deepEqual(value, manifest); value.fill(0); return privateResult;
    } });
    await inspect();
    await attempt({ acquire: async () => { throw new Error('second warm mutation must not acquire'); } });
    await inspect(); passed++;

    console.log(JSON.stringify({ passed, scope: 'offline synthetic v3 manifest cache; six real concurrent child processes', inference: 0, network: 0, credentials: 0 }));
  } finally {
    if (savedXdg === undefined) delete process.env.XDG_CACHE_HOME; else process.env.XDG_CACHE_HOME = savedXdg;
    await Promise.all([...workers].map(worker => new Promise(resolve => { worker.once('close', resolve); worker.kill('SIGKILL'); })));
    await rm(root, { recursive: true, force: true });
  }
}
