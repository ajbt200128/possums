import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, readFile, rm, stat } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createServer, connect } from 'node:net';
import { once } from 'node:events';
import dns from 'node:dns';
import { Agent, fetch as undiciFetch, errors } from 'undici';
import { TorClient, TorFailure, diagnostic, validateConfig, childLaunchPlan, setupConnectionTest, backendTrustRejected } from '../tor-client.mjs';
import { config, fixturePolicy, networkFixture, syntheticVerifier, externalRuntime, stalledProxy, childRuntime } from './fixtures.mjs';

const rejectsCode = (promise, code, stage) => assert.rejects(promise, error => {
  const value = diagnostic(error);
  assert.equal(value.code, `possums_tor_${code}`);
  if (stage) assert.equal(value.stage, stage);
  return true;
});
const clientFor = (fixture, options = {}) => new TorClient(config(fixture.proxyPort), {
  runtime: externalRuntime(), verifyBackend: syntheticVerifier(fixture.port), fixturePolicy, ...options,
});

test('strict malformed configuration rejects before lifecycle or network', () => {
  for (const bad of [null, [], {}, config(0), config(65536), config(80, { host: 'localhost' }),
    config(80, { host: '192.0.2.1' }), config(80, { host: undefined }), config(80, { host: '127.0.0.1; echo injected' }),
    config(80, { host: '127.1' }), config(80, { dns: 'local' }), config(80, { torAssumptionsAcknowledged: false }),
    config(80, { password: 'synthetic-forbidden' }), config(80, { timeoutMs: NaN }), config(80, { mode: 'direct' }),
    config(80, { mode: 'child', executable: 'tor', stateDirectory: '/synthetic-state', launchApproved: true }),
    config(80, { mode: 'child', executable: '/tor\nLog notice stdout', stateDirectory: '/synthetic-state', launchApproved: true }),
    config(80, { mode: 'child', executable: '/tor', stateDirectory: '/synthetic-state', launchApproved: false })]) {
    assert.throws(() => validateConfig(bad), error => diagnostic(error).code === 'possums_tor_config');
  }
  assert.equal(validateConfig(config(9050, { host: '::1' })).host, '::1');
  const value = config(9050); const validated = validateConfig(value); value.dns = 'local';
  assert.equal(validated.dns, 'proxy'); assert.ok(Object.isFrozen(validated));
});

test('offline status performs no work', () => {
  const runtime = { waitReadiness() { assert.fail('status initiated bootstrap'); } };
  const client = new TorClient(config(9050), { runtime, verifyBackend() { assert.fail('status initiated verification'); } });
  assert.deepEqual(client.status(), { state: 'offline' });
  assert.deepEqual(client.status(), { state: 'offline' });
});

test('normal/setup share preparation, bootstrap, connector, all acquisition and diagnostics', async t => {
  const fixture = await networkFixture(); t.after(() => fixture.close());
  const runtime = externalRuntime(); const visits = [];
  const verifier = syntheticVerifier(fixture.port, visits);
  const client = clientFor(fixture, { runtime, verifyBackend: verifier });
  const originalPrepare = client.prepareConnection.bind(client);
  let calls = 0;
  client.prepareConnection = options => { calls++; return originalPrepare(options); };
  const connection = await client.prepareConnection();
  assert.equal(client.status().state, 'prepared');
  assert.deepEqual(visits, ['gateway.invalid', 'github.invalid', 'assets.invalid', 'amd.invalid']);
  // Proposed Channel renewal/recovery uses the exact SAME returned transport.
  for (const route of ['/renewal', '/recovery']) {
    const response = await connection.transport.fetch(new Request(`http://gateway.invalid:${fixture.port}${route}`));
    assert.equal(await response.text(), 'synthetic-public-evidence');
  }
  await connection.close();
  const result = await setupConnectionTest(client);
  assert.equal(result.ok, true); assert.equal(result.qualification, 'point_in_time_only');
  assert.equal(calls, 2); assert.equal(runtime.calls, 2);
  assert.deepEqual(visits.slice(4), visits.slice(0, 4));
  assert.equal(fixture.destinations, 10); assert.equal(fixture.scopeCount, 2);
  assert.equal(client.status().state, 'offline');
});

test('proxy-side hostname DNS: no local lookup; all four sources traverse proxy', async t => {
  const fixture = await networkFixture(); t.after(() => fixture.close());
  const original = dns.lookup;
  let lookups = 0;
  dns.lookup = () => { lookups++; assert.fail('destination DNS leaked locally'); };
  t.after(() => { dns.lookup = original; });
  const connection = await clientFor(fixture).prepareConnection();
  t.after(() => connection.close());
  assert.equal(lookups, 0);
  assert.deepEqual(fixture.requested, ['gateway.invalid', 'github.invalid', 'assets.invalid', 'amd.invalid']);
  assert.equal(fixture.authentications, 4); assert.equal(fixture.scopeCount, 1);
});

test('bootstrap readiness gates every destination; structured summaries are ignored', async t => {
  const fixture = await networkFixture(); t.after(() => fixture.close());
  for (const progress of [0, 99, '100', NaN, undefined]) {
    const client = clientFor(fixture, { runtime: { async waitReadiness() { return { progress }; } } });
    await rejectsCode(setupConnectionTest(client), 'bootstrap');
    assert.equal(fixture.destinations, 0); assert.equal(fixture.authentications, 0);
  }
  const runtime = { async waitReadiness() { return { progress: 100, summary: 'hostile bridge configuration' }; } };
  assert.equal((await setupConnectionTest(clientFor(fixture, { runtime }))).ok, true);
});

test('setup/normal fail with identical closed diagnostics on SOCKS rejection', async t => {
  const fixture = await networkFixture({ rejectAuth: true }); t.after(() => fixture.close());
  const client = clientFor(fixture);
  await rejectsCode(client.prepareConnection(), 'socks_negotiation');
  const normal = client.status().diagnostic;
  await rejectsCode(setupConnectionTest(client), 'socks_negotiation');
  assert.deepEqual(client.status().diagnostic, normal);
  assert.equal(fixture.destinations, 0);
});

test('proxy absent is unavailable, not backend distrust; no direct fallback', async t => {
  const listener = createServer(); listener.listen(0, '127.0.0.1'); await once(listener, 'listening');
  const port = listener.address().port; await new Promise(resolve => listener.close(resolve));
  const fixture = await networkFixture(); t.after(() => fixture.close());
  const client = new TorClient(config(port), { runtime: externalRuntime(), verifyBackend: syntheticVerifier(fixture.port), fixturePolicy });
  await rejectsCode(client.prepareConnection(), 'socks_unavailable');
  assert.equal(client.status().diagnostic.disposition, 'graceful_stop'); assert.equal(fixture.destinations, 0);
});

test('SOCKS authentication rejection retains negotiation category', async t => {
  const fixture = await networkFixture({ rejectAuth: true }); t.after(() => fixture.close());
  await rejectsCode(clientFor(fixture).prepareConnection(), 'socks_negotiation');
  assert.equal(fixture.destinations, 0);
});

test('qualified patch: no-auth-only listener fails closed before destination traffic', async t => {
  const fixture = await networkFixture({ noAuth: true }); t.after(() => fixture.close());
  await rejectsCode(setupConnectionTest(clientFor(fixture)), 'socks_negotiation');
  assert.equal(fixture.authentications, 0); assert.equal(fixture.destinations, 0);
  assert.equal(fixture.requested.length, 0);
});

test('previous direct pool is not reused; proxy loss cannot fall back', async t => {
  const fixture = await networkFixture(); t.after(() => fixture.close());
  const direct = new Agent({ connect(_options, callback) {
    const socket = connect({ host: '127.0.0.1', port: fixture.port });
    socket.once('connect', () => callback(null, socket)); socket.once('error', error => callback(error, null));
  } });
  t.after(() => direct.destroy());
  const url = `http://gateway.invalid:${fixture.port}/evidence`;
  assert.equal(await (await undiciFetch(url, { dispatcher: direct })).text(), 'synthetic-public-evidence');
  const client = clientFor(fixture); const connection = await client.prepareConnection();
  t.after(() => connection.close());
  const previous = fixture.destinations;
  fixture.loseProxy();
  await assert.rejects(connection.transport.fetch(new Request(url)), error =>
    ['possums_tor_socks_unavailable', 'possums_tor_transport_interrupted'].includes(diagnostic(error).code));
  assert.equal(fixture.destinations, previous);
  assert.equal(await (await undiciFetch(url, { dispatcher: direct })).text(), 'synthetic-public-evidence');
});

test('HTTP redirect is explicit and next asset acquisition uses same proxy', async t => {
  const fixture = await networkFixture(); t.after(() => fixture.close());
  const connection = await clientFor(fixture).prepareConnection(); t.after(() => connection.close());
  const response = await connection.transport.acquire(new Request(`http://github.invalid:${fixture.port}/redirect`));
  assert.equal(response.status, 302); assert.equal(response.redirected, false);
  assert.equal(response.url, `http://github.invalid:${fixture.port}/redirect`);
  const target = response.headers.get('location'); await response.body.cancel();
  assert.equal(fixture.destinations, 5);
  assert.equal(await (await connection.transport.acquire(new Request(target))).text(), 'synthetic-public-evidence');
  assert.equal(fixture.destinations, 6);
  assert.ok(fixture.requested.includes('assets.invalid')); // Existing proxied asset tunnel may be reused.
});

test('backend verification has public-only authority and cannot dispatch credentials', async t => {
  const fixture = await networkFixture(); t.after(() => fixture.close());
  for (const verifier of [
    transport => transport.fetch(new Request(`http://gateway.invalid:${fixture.port}/evidence`)),
    transport => transport.acquire(new Request(`http://gateway.invalid:${fixture.port}/evidence`, { headers: { Authorization: 'synthetic-only-not-retained' } })),
    transport => transport.acquire(new Request(`http://gateway.invalid:${fixture.port}/evidence`, { method: 'POST', body: 'synthetic-only' })),
  ]) {
    const client = clientFor(fixture, { verifyBackend: verifier });
    await assert.rejects(client.prepareConnection(), error => ['possums_tor_config', 'possums_tor_backend_trust'].includes(diagnostic(error).code));
    assert.equal(fixture.destinations, 0);
  }
});

test('closed connection cannot be reused even after deliberate new preparation', async t => {
  const fixture = await networkFixture(); t.after(() => fixture.close());
  const client = clientFor(fixture);
  const old = await client.prepareConnection(); await old.close(); await old.close();
  const next = await client.prepareConnection(); t.after(() => next.close());
  await old.close();
  assert.equal(client.status().state, 'prepared'); // Stale cleanup cannot clear the replacement.
  const previous = fixture.destinations;
  await rejectsCode(old.transport.fetch(new Request(`http://gateway.invalid:${fixture.port}/evidence`)), 'closed');
  assert.equal(fixture.destinations, previous);
});

test('transport interrupted body carries closed category, never raw SDK text', async t => {
  const fixture = await networkFixture(); t.after(() => fixture.close());
  const connection = await clientFor(fixture).prepareConnection(); t.after(() => connection.close());
  const response = await connection.transport.acquire(new Request(`http://gateway.invalid:${fixture.port}/cut`));
  await rejectsCode(response.text(), 'transport_interrupted');
});

test('pre-cancellation, bootstrap cancellation and deadline send no destination requests', async t => {
  const fixture = await networkFixture(); t.after(() => fixture.close());
  for (const mode of ['pre', 'during', 'timeout']) {
    const abort = new AbortController();
    const runtime = { async waitReadiness() { return new Promise(() => {}); } };
    const client = new TorClient(config(fixture.proxyPort, { timeoutMs: 30 }), { runtime, verifyBackend: syntheticVerifier(fixture.port), fixturePolicy });
    if (mode === 'pre') abort.abort('hostile reason');
    const attempt = client.prepareConnection({ signal: abort.signal });
    if (mode === 'during') abort.abort('hostile reason');
    await rejectsCode(attempt, mode === 'timeout' ? 'timeout' : 'cancelled', 'bootstrap');
    assert.equal(fixture.destinations, 0);
  }
});

test('SOCKS stall cancellation/deadline closes only fixture sockets', async t => {
  const fixture = await stalledProxy(); t.after(() => fixture.close());
  for (const cancel of [true, false]) {
    const abort = new AbortController();
    const client = new TorClient(config(fixture.port, { timeoutMs: 60 }), {
      runtime: externalRuntime(), fixturePolicy,
      verifyBackend: async transport => { await transport.acquire(new Request('http://gateway.invalid:8080/evidence')); return { verified: true }; },
    });
    const attempt = client.prepareConnection({ signal: abort.signal });
    if (cancel) setTimeout(() => abort.abort(), 20);
    await rejectsCode(attempt, cancel ? 'cancelled' : 'timeout');
    await fixture.waitForSocketClose();
    assert.equal(fixture.socketCount, 0);
  }
});

test('request cancellation after preparation stops operation without inference replay', async t => {
  const fixture = await networkFixture(); t.after(() => fixture.close());
  const connection = await clientFor(fixture).prepareConnection(); t.after(() => connection.close());
  const abort = new AbortController();
  const attempt = connection.transport.fetch(new Request(`http://gateway.invalid:${fixture.port}/wait`, { signal: abort.signal }));
  setTimeout(() => abort.abort('hostile cancellation text'), 20);
  await rejectsCode(attempt, 'cancelled');
});

test('child plan: private scope, persistent guards, shell false, literal hostile path', () => {
  const value = config(9050, { mode: 'child', executable: '/synthetic/tor; touch NOT_EXECUTED', stateDirectory: '/synthetic/guards', launchApproved: true });
  const plan = childLaunchPlan(value, '/synthetic/runtime');
  assert.equal(plan.shell, false); assert.equal(plan.stdio, 'ignore'); assert.equal(plan.detached, false);
  assert.equal(plan.executable, value.executable); assert.equal(plan.preserveDirectory, '/synthetic/guards');
  assert.match(plan.torrc, /IsolateSOCKSAuth/); assert.match(plan.torrc, /CookieAuthentication 1/);
  assert.equal(plan.args.filter(arg => arg.includes('touch')).length, 0);
  assert.throws(() => childLaunchPlan(value, '/synthetic/guards/runtime'), TorFailure);
  assert.throws(() => childLaunchPlan(value, '/synthetic'), TorFailure);
  assert.throws(() => childLaunchPlan(value, '/synthetic/runtime/../guards'), TorFailure);
});

test('own-child lifecycle only; guard fixture survives cleanup and new circuit scope', async t => {
  const root = await mkdtemp(join(tmpdir(), 'possums-tor-client-fixture-')); t.after(() => rm(root, { recursive: true, force: true }));
  await mkdir(join(root, 'guards'), { mode: 0o700 });
  await writeFile(join(root, 'guards', 'synthetic-guard-state'), 'synthetic guard selection only', { mode: 0o600 });
  assert.equal((await stat(root)).mode & 0o777, 0o700);
  const unrelatedRuntime = childRuntime(join(root, 'unrelated'));
  const unrelated = unrelatedRuntime.launch({ shell: false, stdio: 'ignore' }); t.after(() => unrelated.stop());
  const fixture = await networkFixture(); t.after(() => fixture.close());
  const runtime = childRuntime(join(root, 'runtime'));
  const client = new TorClient(config(fixture.proxyPort, { mode: 'child', executable: process.execPath, stateDirectory: join(root, 'guards'), launchApproved: true }), {
    runtime, verifyBackend: syntheticVerifier(fixture.port), fixturePolicy,
  });
  assert.equal((await setupConnectionTest(client)).ok, true);
  assert.equal(runtime.launches, 1); assert.equal(runtime.stops, 1); assert.notEqual(runtime.child.signalCode, null);
  assert.equal(await readFile(join(root, 'guards', 'synthetic-guard-state'), 'utf8'), 'synthetic guard selection only');
  assert.equal((await setupConnectionTest(client)).ok, true);
  assert.equal(runtime.stops, 2); assert.equal(fixture.scopeCount, 2);
  assert.equal(unrelatedRuntime.stops, 0);
  assert.equal(unrelatedRuntime.child.exitCode, null);
  assert.equal(unrelatedRuntime.child.signalCode, null);
});

test('external lifecycle never invokes stop or launch', async t => {
  const fixture = await networkFixture(); t.after(() => fixture.close());
  const runtime = { ...externalRuntime(), launch() { assert.fail('external process launched'); }, stop() { assert.fail('external process stopped'); } };
  assert.equal((await setupConnectionTest(clientFor(fixture, { runtime }))).ok, true);
});

test('child cancellation/exit stop own child; trust primary survives cleanup failure', async t => {
  const root = await mkdtemp(join(tmpdir(), 'possums-tor-client-fixture-')); t.after(() => rm(root, { recursive: true, force: true }));
  const fixture = await networkFixture(); t.after(() => fixture.close());
  for (const behavior of ['stall', 'exit', 'ready']) {
    const runtime = childRuntime(join(root, 'runtime'), { behavior, cleanupFailure: behavior === 'ready' });
    const client = new TorClient(config(fixture.proxyPort, { mode: 'child', timeoutMs: behavior === 'stall' ? 60 : 1000,
      executable: process.execPath, stateDirectory: join(root, 'guards'), launchApproved: true }), {
      runtime, verifyBackend: async () => { throw backendTrustRejected(); }, fixturePolicy,
    });
    await rejectsCode(client.prepareConnection(), behavior === 'stall' ? 'timeout' : behavior === 'exit' ? 'bootstrap' : 'backend_trust');
    assert.equal(runtime.stops, 1);
    if (behavior === 'ready') {
      assert.equal(client.status().diagnostic.disposition, 'block_backend');
      assert.equal(client.status().cleanup.code, 'possums_tor_cleanup_unknown');
    }
  }
});

test('closed expected errors and stage-specific unknown errors ignore hostile text', async () => {
  const hostile = 'synthetic bridge secret http://fixture.invalid/path username raw exception';
  for (const stage of ['configuration', 'child_launch', 'bootstrap', 'socks_connect', 'socks_negotiation', 'transport', 'backend_verification', 'cleanup']) {
    const value = diagnostic(new Error(hostile), stage);
    assert.equal(value.code, `possums_tor_${stage}_unknown`); assert.equal(value.constraint, 'cause_unknown');
    assert.equal(JSON.stringify(value).includes(hostile), false);
  }
  for (const kind of ['config', 'missing_exec', 'child_launch', 'bootstrap', 'socks_unavailable', 'socks_negotiation', 'transport_interrupted', 'backend_trust']) {
    const value = diagnostic(new TorFailure(kind)); assert.ok(value.action.length > 10);
    assert.equal(value.disposition, kind === 'backend_trust' ? 'block_backend' : 'graceful_stop');
  }
  assert.equal(diagnostic(new errors.Socks5ProxyError(hostile)).constraint, 'cause_unknown');
  for (const kind of ['toString', '__proto__', hostile]) {
    assert.equal(diagnostic(new TorFailure(kind)).code, 'possums_tor_transport_unknown');
  }
});

test('unknown lifecycle and verifier faults retain stage; absent executable is distinct', async t => {
  const root = await mkdtemp(join(tmpdir(), 'possums-tor-client-fixture-')); t.after(() => rm(root, { recursive: true, force: true }));
  const fixture = await networkFixture(); t.after(() => fixture.close());
  const childConfig = config(fixture.proxyPort, { mode: 'child', executable: '/synthetic/missing-tor', stateDirectory: join(root, 'guards'), launchApproved: true });
  for (const kind of ['missing_exec', 'child_launch', 'unknown']) {
    const runtime = { runtimeDirectory: join(root, 'runtime'), launch() {
      if (kind === 'unknown') throw new Error('hostile process arguments');
      throw new TorFailure(kind);
    } };
    await rejectsCode(new TorClient(childConfig, { runtime, verifyBackend: syntheticVerifier(fixture.port), fixturePolicy }).prepareConnection(), kind === 'unknown' ? 'child_launch_unknown' : kind);
  }
  await rejectsCode(clientFor(fixture, { runtime: { waitReadiness() { throw new Error('hostile bootstrap output'); } } }).prepareConnection(), 'bootstrap_unknown');
  await rejectsCode(clientFor(fixture, { verifyBackend() { throw new Error('hostile verifier text'); } }).prepareConnection(), 'backend_verification_unknown');
});
