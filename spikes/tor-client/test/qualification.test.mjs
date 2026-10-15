import test from 'node:test';
import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import { createRequire } from 'node:module';
import { cp, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { Socks5ProxyAgent } from 'undici';
import { TorClient, diagnostic, setupConnectionTest } from '../tor-client.mjs';
import { createExistingSocksRuntime } from '../existing-socks-runtime.mjs';
import { isQualifiedUndici, undiciDirectory, qualificationHashes, digestFile } from '../undici-qualification.mjs';
import { config, fixturePolicy, networkFixture, syntheticVerifier } from './fixtures.mjs';

const require = createRequire(import.meta.url);
const { Socks5Client } = require('undici/lib/core/socks5-client.js');

// Feed fixed bytes directly to the MAINTAINED UPSTREAM decoder. This is not a
// replacement SOCKS parser. Integration fixtures use socksv5-server's parser.
function decoder(options) {
  const socket = new EventEmitter();
  const writes = [];
  socket.write = bytes => writes.push(Buffer.from(bytes));
  socket.destroy = () => { socket.destroyed = true; };
  const client = new Socks5Client(socket, options);
  const faults = [];
  client.on('error', error => faults.push(error.code));
  return { socket, client, writes, faults };
}

test('exact qualified decoder offers only password and rejects forced no-auth, fragmented or combined', () => {
  for (const chunks of [[Buffer.from([5, 0])], [Buffer.from([5]), Buffer.from([0])]]) {
    const fixture = decoder({ username: 'synthetic', password: 'synthetic', requireAuth: true });
    let authenticated = false;
    fixture.client.on('authenticated', () => { authenticated = true; });
    fixture.client.handshake();
    assert.deepEqual(fixture.writes, [Buffer.from([5, 1, 2])]);
    for (const chunk of chunks) fixture.socket.emit('data', chunk);
    assert.equal(authenticated, false);
    assert.deepEqual(fixture.faults, ['UND_ERR_SOCKS5_AUTH_METHOD']);
    assert.equal(fixture.socket.destroyed, true);
    assert.equal(fixture.writes.length, 1); // No auth payload, CONNECT or destination.
  }
});

test('qualified upstream default remains backward compatible, strict options require credentials', async () => {
  const fixture = decoder({ username: 'synthetic', password: 'synthetic' });
  fixture.client.handshake();
  assert.deepEqual(fixture.writes[0], Buffer.from([5, 2, 2, 0]));
  fixture.socket.emit('data', Buffer.from([5, 0]));
  assert.equal(fixture.client.state, 'authenticated');
  for (const options of [{ requireAuth: true }, { requireAuth: true, username: 'synthetic' }, { requireAuth: 'true' }]) {
    assert.throws(() => new Socks5ProxyAgent('socks5://127.0.0.1:9050', options),
      error => error.code === 'UND_ERR_INVALID_ARG');
  }
  const agent = new Socks5ProxyAgent('socks5://127.0.0.1:9050', { username: 'synthetic', password: 'synthetic', requireAuth: true });
  await agent.destroy();
});

test('exact qualification rejects stock/unknown/missing bytes without changing installed dependencies', async t => {
  assert.equal(isQualifiedUndici(), true);
  const root = await mkdtemp(join(tmpdir(), 'tor-spike-qualification-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  await cp(undiciDirectory, root, { recursive: true });
  assert.equal(isQualifiedUndici(root), true);
  await writeFile(join(root, 'lib/core/socks5-client.js'), 'synthetic unknown source');
  assert.equal(isQualifiedUndici(root), false);
  await rm(join(root, 'lib/core/socks5-client.js'));
  assert.equal(isQualifiedUndici(root), false);
  assert.equal(isQualifiedUndici(), true);
  for (const [path, hashes] of Object.entries(qualificationHashes))
    assert.equal(digestFile(join(undiciDirectory, path)), hashes.qualified);
});

test('fresh offline patch applies exactly; stock or already-loaded stock decoder fails closed', async t => {
  const root = await mkdtemp(join(tmpdir(), 'tor-spike-patch-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  await cp(undiciDirectory, join(root, 'node_modules/undici'), { recursive: true });
  for (const name of ['tor-client.mjs', 'undici-qualification.mjs', 'qualify-undici.mjs', 'patches'])
    await cp(fileURLToPath(new URL(`../${name}`, import.meta.url)), join(root, name), { recursive: true });
  const patch = fileURLToPath(new URL('../patches/undici-8.11.2-require-auth.patch', import.meta.url));
  assert.equal(spawnSync('git', ['apply', '--reverse', '--directory=node_modules/undici', patch],
    { cwd: root, stdio: 'ignore' }).status, 0);
  assert.equal(isQualifiedUndici(join(root, 'node_modules/undici')), false);
  const gate = `
    import { TorClient, diagnostic } from './tor-client.mjs';
    PATCH
    const client = new TorClient({ mode: 'existing', host: '127.0.0.1', port: 9050,
      dns: 'proxy', torAssumptionsAcknowledged: true }, {
      runtime: { async waitReadiness() { return { progress: 100 }; } },
      verifyBackend: async () => { process.exit(3); },
    });
    try { await client.prepareConnection(); process.exit(2); }
    catch (error) { process.exit(diagnostic(error).code === 'possums_tor_connector_qualification' ? 0 : 4); }
  `;
  const run = code => spawnSync(process.execPath, ['--input-type=module', '-e', code],
    { cwd: root, stdio: 'ignore' }).status;
  assert.equal(run(gate.replace('PATCH', '')), 0); // Actual unpatched runtime, before any socket.
  assert.equal(run(gate.replace('PATCH', "await import('./qualify-undici.mjs');")), 0); // Disk patched; cached decoder remains stock.
  assert.equal(isQualifiedUndici(join(root, 'node_modules/undici')), true);
  assert.equal(run("import { Socks5ProxyAgent } from 'undici'; process.exit(Socks5ProxyAgent.supportsRequireAuth === true ? 0 : 1);"), 0);
  assert.equal(spawnSync(process.execPath, ['qualify-undici.mjs'], { cwd: root, stdio: 'ignore' }).status, 0); // Idempotent.
  for (const manifest of ['synthetic malformed JSON', '{}']) {
    await writeFile(join(root, 'patches/undici-8.11.2-hashes.json'), manifest);
    assert.equal(run(gate.replace('PATCH', '')), 0); // Closed preparation error, not raw import exception/vacuous qualification.
    assert.equal(spawnSync(process.execPath, ['qualify-undici.mjs'], { cwd: root, stdio: 'ignore' }).status, 1);
  }
});

test('real external adapter uses normal/Test path and does not access control or own a process', async t => {
  const fixture = await networkFixture(); t.after(() => fixture.close());
  const runtime = createExistingSocksRuntime();
  assert.deepEqual(Object.keys(runtime), ['waitReadiness']);
  const client = new TorClient(config(fixture.proxyPort, { readiness: 'connection' }), {
    runtime, verifyBackend: syntheticVerifier(fixture.port), fixturePolicy,
  });
  assert.deepEqual(client.status(), { state: 'offline' });
  assert.equal(fixture.destinations, 0);
  const connection = await client.prepareConnection();
  assert.equal(client.status().readiness, 'connection');
  assert.equal(client.status().authentication, 'username_password_required');
  await connection.close();
  assert.equal((await setupConnectionTest(client)).ok, true);
  assert.equal(fixture.destinations, 8); assert.equal(fixture.scopeCount, 2);
});

test('real external adapter refuses fabricated control evidence and owned mode', async () => {
  const runtime = createExistingSocksRuntime();
  for (const extra of [{}, { readiness: 'control_bootstrap' }]) {
    const client = new TorClient(config(9050, extra), { runtime, verifyBackend: async () => assert.fail('no verification') });
    await assert.rejects(client.prepareConnection(), error => diagnostic(error).code === 'possums_tor_config');
  }
  assert.throws(() => new TorClient(config(9050, { mode: 'child', readiness: 'connection',
    executable: '/synthetic/tor', stateDirectory: '/synthetic/guards', launchApproved: true }), { runtime }),
  error => diagnostic(error).code === 'possums_tor_config');
});

test('real external no-auth and cancellation failures keep setup/normal policy', async t => {
  const fixture = await networkFixture({ noAuth: true }); t.after(() => fixture.close());
  const client = new TorClient(config(fixture.proxyPort, { readiness: 'connection' }), {
    runtime: createExistingSocksRuntime(), verifyBackend: syntheticVerifier(fixture.port), fixturePolicy,
  });
  for (const prepare of [() => client.prepareConnection(), () => setupConnectionTest(client)])
    await assert.rejects(prepare(), error => diagnostic(error).code === 'possums_tor_socks_negotiation');
  const abort = new AbortController(); abort.abort();
  await assert.rejects(setupConnectionTest(client, { signal: abort.signal }),
    error => diagnostic(error).code === 'possums_tor_cancelled' && diagnostic(error).stage === 'readiness');
  assert.equal(fixture.destinations, 0);
});
