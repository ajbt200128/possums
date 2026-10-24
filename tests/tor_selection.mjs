// OFFLINE ONLY. Pinned scratch source/dependencies, test-build substitutions;
// no production hooks, helper process, Tor, credentials, installation or traffic.
// PRIVACY.md: data handling, forbidden telemetry, typed failures, synthetic tests.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtemp, rm, readFile, symlink, realpath } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { torDiagnosticChecks } from './tor_diagnostics.mjs';
const source = process.env.PHASE02_RELEASE_SOURCE;
assert(source && path.isAbsolute(source), 'select a scratch copy with pinned dependencies');
assert.equal(process.version, 'v24.13.0');
for (const [name, version] of [['esbuild', '0.25.10'], ['typescript', '5.9.3'], ['@earendil-works/pi-ai', '1.3.0'], ['@earendil-works/pi-coding-agent', '1.3.0']]) {
  assert.equal(JSON.parse(await readFile(path.join(source, 'node_modules', name, 'package.json'))).version, version);
}
const { build } = await import(pathToFileURL(path.join(source, 'node_modules/esbuild/lib/main.js')));
const { base32 } = await import(pathToFileURL(path.join(source, 'node_modules/@scure/base/index.js')));
const bytes = Buffer.alloc(32, 9);
const checksum = createHash('sha3-256').update('.onion checksum').update(bytes).update(Buffer.from([3])).digest().subarray(0, 2);
const onion = base32.encode(Buffer.concat([bytes, checksum, Buffer.from([3])])).toLowerCase() + '.onion';
const hostile = 'HOSTILE_SYNTHETIC_SECRET';
const out = await realpath(await mkdtemp(path.join(tmpdir(), 'possums-tor-selection-check-')));
const originalEnv = process.env, originalFetch = globalThis.fetch;
let envReads = 0, prepares = 0, scopeId = 0, connects = [], scopes = [], holdClose, failClose, fixture;
let checks = 0;
async function check(name, body) { await body(); checks++; console.log('PASS ' + name); }
function deferred() { let resolve; const promise = new Promise(r => { resolve = r; }); return { promise, resolve }; }
const proxyEnv = values => {
  process.env = new Proxy({ ...originalEnv, POSSUMS_TOR_PROXY_USERNAME: undefined, POSSUMS_TOR_PROXY_PASSWORD: undefined, ...values }, {
    get(target, key) { if (key === 'POSSUMS_TOR_PROXY_USERNAME' || key === 'POSSUMS_TOR_PROXY_PASSWORD') envReads++; return target[key]; },
  });
};
try {
  globalThis.fetch = async () => { assert.fail('no network permitted'); };
  globalThis.__selectionPrepare = proxy => {
    prepares++; assert.equal(proxy.helperPath, path.join(out, 'possums-tor-native'));
  };
  globalThis.__selectionScope = owned => {
    const item = { id: ++scopeId, closed: false, close: async () => {
      item.closed = true; await owned.close();
      await holdClose?.promise;
      if (failClose) throw new Error(hostile);
    } };
    scopes.push(item); return item;
  };
  globalThis.__selectionConnect = async (route, scope, signal, previous) => {
    assert(!signal?.aborted); assert(!scope?.closed);
    connects.push({ route, scope, signal, previous });
    return fixture;
  };
  await symlink(path.join(source, 'node_modules'), path.join(out, 'node_modules'));
  const realTor = path.join(source, 'tor-connection.ts');
  await build({ stdin: { contents: `export { default as extension } from './index.ts';
    export * from './tor-selection.ts'; export * from './diagnostics.ts';
    export * from './tor-connection.ts'; export { VerifierFailure } from './verifier.mjs';
    export { checkTorEndorsementSignature } from './tor-endorsement.mjs';
    export { PossumsProvider } from './provider.ts';
    export { Channel, ReferenceClient } from '../../examples/phase01/transport.ts';
    export { DiagnosticFailure } from '../../examples/phase01/limits.ts';`, resolveDir: source, loader: 'ts' },
    outfile: path.join(out, 'check.mjs'), bundle: true, platform: 'node', format: 'esm', target: 'node24',
    external: ['@earendil-works/*'], define: { __PHASE01_FIXTURE__: 'true' }, plugins: [{ name: 'offline-selection-only', setup(b) {
      b.onResolve({ filter: /^\.\/bootstrap\.js$/ }, args => args.importer.endsWith('/index.ts') ? { path: 'bootstrap', namespace: 'synthetic' } : undefined);
      b.onResolve({ filter: /^\.\/tor-connection\.js$/ }, args => args.importer.endsWith('/index.ts') ? { path: 'scope', namespace: 'synthetic' } : undefined);
      b.onLoad({ filter: /.*/, namespace: 'synthetic' }, args => ({ contents: args.path === 'bootstrap' ? `
        export const connectPublished = signal => globalThis.__selectionConnect('clearnet', undefined, signal);
        export const connect = (_manifest, signal) => globalThis.__selectionConnect('manifest', undefined, signal);
        export const connectTorPublished = (scope, signal) => globalThis.__selectionConnect('tor', scope, signal);
        export const recoverPublished = (previous, signal) => globalThis.__selectionConnect('clearnet', undefined, signal, previous);
        export const recoverCompiled = (previous, _manifest, signal) => globalThis.__selectionConnect('manifest', undefined, signal, previous);
        export const recoverTorPublished = (previous, scope, signal) => globalThis.__selectionConnect('tor', scope, signal, previous);
      ` : `import { prepareTorConnection as actual } from ${JSON.stringify(realTor)};
        export function prepareTorConnection(proxy, onion) {
          globalThis.__selectionPrepare(proxy);
          return globalThis.__selectionScope(actual(proxy, onion));
        }`, loader: 'js', resolveDir: source }));
    } }] });
  const m = await import(pathToFileURL(path.join(out, 'check.mjs')));
  const key = new Uint8Array([0, 0, 32, ...Array(32).fill(7), 0, 4, 0, 1, 0, 2]);
  fixture = new m.ReferenceClient(await m.Channel.fixture('https://localhost:18443', key, '07'.repeat(32)));
  function extension(initial = {}) {
    const flags = new Map(), handlers = new Map(), commands = new Map(), warnings = [];
    let provider, refreshes = 0;
    m.extension({ registerFlag(name) { flags.set(name, undefined); }, getFlag: name => flags.get(name),
      registerProvider(value) { provider = value; }, on(name, handler) { handlers.set(name, handler); },
      registerCommand(name, command) { commands.set(name, command); }, setActiveTools() {} });
    // Pi assigns CLI flags only AFTER the extension factory returns.
    for (const [name, value] of Object.entries(initial)) flags.set(name, value);
    const ctx = { hasUI: true, ui: { notify: text => warnings.push(text) }, modelRegistry: { refresh: async () => { refreshes++; } } };
    return { flags, provider, warnings, start: () => handlers.get('session_start')({}, ctx),
      shutdown: () => handlers.get('session_shutdown')({}, ctx), status: () => commands.get('possums-status').handler('', ctx),
      message: message => handlers.get('message_end')({ message }, ctx), refreshes: () => refreshes };
  }
  await check('pure parser requires explicit onion and bounded numeric loopback authority', async () => {
    const input = { manifest: undefined, onion: undefined, proxy: undefined };
    assert.deepEqual(m.parseTorSelection(input), { route: 'clearnet', manifest: undefined });
    for (const [proxy, host, port] of [[undefined, '127.0.0.1', 19050], ['127.0.0.2:9050', '127.0.0.2', 9050], ['[::1]:19050', '::1', 19050]]) {
      const selected = m.parseTorSelection({ ...input, onion, proxy });
      assert.deepEqual(selected, { route: 'tor', onion, host, port }); assert(Object.isFrozen(selected));
    }
    for (const proxy of ['localhost:9050', '192.0.2.1:9050', '[::2]:9050', '127.0.0.1', '127.0.0.1:0', '127.0.0.1:65536',
      '127.0.0.1:bad', '127.0.0.1:9050/', '127.0.0.1:9050?', '127.0.0.1:9050#', 'user:pass@127.0.0.1:9050',
      '127.0.0.1:9050/path', 'socks5h://127.0.0.1:9050', '127.0.0.1:9050\n', hostile]) {
      assert.throws(() => m.parseTorSelection({ ...input, onion, proxy }), error => error.code === 'tor_selection_invalid' && !error.message.includes(hostile));
    }
    for (const bad of [{ ...input, proxy: '127.0.0.1:19050' }, { ...input, onion, manifest: '/synthetic' },
      { ...input, onion: onion.toUpperCase() }, { ...input, onion: 'http://' + onion }, { ...input, onion: hostile }]) {
      assert.throws(() => m.parseTorSelection(bad), error => error.code === 'tor_selection_invalid' && !error.message.includes(hostile));
    }
  });
  await check('selection factory locks all flags synchronously including rejected selections', async () => {
    let input = { manifest: undefined, onion: undefined, proxy: undefined };
    const select = m.createTorSelection(() => input);
    input = { ...input, onion }; const selected = select(); assert.equal(selected.route, 'tor');
    for (const field of ['manifest', 'onion', 'proxy']) {
      const saved = input; input = { ...input, [field]: hostile };
      assert.throws(select, error => error.code === 'route_selection_changed' && !error.message.includes(hostile)); input = saved;
    }
    assert.equal(select(), selected);
    let manifest;
    const direct = m.createTorSelection(() => ({ manifest, onion: undefined, proxy: undefined }));
    assert.equal(direct().route, 'clearnet');
    manifest = '/synthetic-manifest';
    assert.throws(direct, error => error.code === 'manifest_mismatch');
    let rejected = { manifest: '/synthetic', onion, proxy: undefined };
    const invalid = m.createTorSelection(() => rejected);
    assert.throws(invalid, error => error.code === 'tor_selection_invalid'); rejected = { ...rejected, manifest: undefined };
    assert.throws(invalid, error => error.code === 'route_selection_changed');
  });
  await check('default extension factory/status do not read Tor secrets, create scopes or connect', async () => {
    proxyEnv({ POSSUMS_TOR_PROXY_USERNAME: hostile });
    const before = connects.length, helpers = prepares, reads = envReads;
    const ext = extension(); await ext.status();
    assert.equal(connects.length, before); assert.equal(prepares, helpers); assert.equal(envReads, reads);
    await ext.start(); assert.equal(connects.at(-1).route, 'clearnet');
    await ext.status(); await ext.shutdown();
    assert.equal(prepares, helpers); assert.equal(envReads, reads);
  });
  await check('manifest plus Tor and proxy without onion reject before secret reads or traffic', async () => {
    for (const flags of [{ 'possums-tor-onion': onion, 'possums-manifest': '/synthetic' }, { 'possums-tor-proxy': '127.0.0.1:19050' }]) {
      const before = connects.length, reads = envReads, helpers = prepares;
      const ext = extension(flags); await ext.start();
      assert.equal(ext.provider.currentFailure.code, 'tor_selection_invalid'); assert.equal(connects.length, before);
      assert.equal(envReads, reads); assert.equal(prepares, helpers); assert(!ext.warnings.join('').includes(hostile)); await ext.shutdown();
    }
  });
  await check('Tor credentials are optional, paired and UTF-8 bounded by native validator', async () => {
    for (const [values, valid] of [[{}, true], [{ POSSUMS_TOR_PROXY_USERNAME: 'synthetic', POSSUMS_TOR_PROXY_PASSWORD: 'synthetic' }, true],
      [{ POSSUMS_TOR_PROXY_USERNAME: hostile }, false], [{ POSSUMS_TOR_PROXY_PASSWORD: hostile }, false],
      [{ POSSUMS_TOR_PROXY_USERNAME: '', POSSUMS_TOR_PROXY_PASSWORD: hostile }, false],
      [{ POSSUMS_TOR_PROXY_USERNAME: 'é'.repeat(128), POSSUMS_TOR_PROXY_PASSWORD: hostile }, false]]) {
      proxyEnv(values); const before = connects.length;
      const ext = extension({ 'possums-tor-onion': onion }); await ext.status(); assert.equal(connects.length, before);
      await ext.start();
      if (valid) { assert.equal(connects.length, before + 1); assert.equal(connects.at(-1).route, 'tor'); }
      else { assert.equal(connects.length, before); assert.equal(ext.provider.currentFailure.code, 'tor_selection_invalid'); }
      assert(!ext.warnings.join('').includes(hostile)); await ext.shutdown();
    }
  });
  await check('host route/manifest mutation cannot silently change subsequent sessions', async () => {
    proxyEnv({});
    for (const initial of [{}, { 'possums-tor-onion': onion }]) {
      for (const flag of ['possums-manifest', 'possums-tor-onion', 'possums-tor-proxy']) {
        const ext = extension(initial); await ext.start(); const before = connects.length;
        ext.flags.set(flag, hostile); await ext.start();
        const expected = initial['possums-tor-onion'] === undefined && flag === 'possums-manifest'
          ? 'manifest_mismatch' : 'route_selection_changed';
        assert.equal(ext.provider.currentFailure.code, expected); assert.equal(connects.length, before);
        await ext.status(); assert(!ext.warnings.join('').includes(hostile)); await ext.shutdown();
      }
    }
  });
  await check('shutdown aborts provider before waiting for scope exit; new session gets fresh locked scope', async () => {
    proxyEnv({}); const ext = extension({ 'possums-tor-onion': onion }); await ext.start();
    const old = connects.at(-1), oldScope = old.scope;
    holdClose = deferred(); const shutdown = ext.shutdown();
    assert(old.signal.aborted); assert(oldScope.closed);
    const before = connects.length; const start = ext.start();
    await Promise.resolve(); assert.equal(connects.length, before, 'no new trust while old exit is pending');
    holdClose.resolve(); await shutdown; await start; holdClose = undefined;
    const fresh = connects.at(-1); assert.notEqual(fresh.scope, oldScope); assert(!fresh.scope.closed); assert.equal(fresh.route, 'tor');
    await ext.shutdown(); assert(fresh.scope.closed);
  });
  await check('cleanup unknown faults report safely before UI removal and prevent scope reuse', async () => {
    const ext = extension({ 'possums-tor-onion': onion }); await ext.start(); const before = connects.length;
    failClose = true; await ext.shutdown(); failClose = false;
    assert.match(ext.warnings.at(-1), /possums_tor_cleanup_unknown/); assert(!ext.warnings.join('').includes(hostile));
    await ext.start(); assert.equal(connects.length, before); assert.equal(ext.provider.currentFailure.code, 'tor_cleanup_unknown');
    await ext.status(); await ext.shutdown();
  });
  await torDiagnosticChecks(m, check);
  await check('actual host warning notification is safe, transient and initiates no work', async () => {
    const ext = extension(); const before = connects.length;
    const provider = new m.PossumsProvider(async () => { throw new m.TorNativeTransportFailure('proxy'); });
    provider.newSession(); await assert.rejects(provider.verifySession());
    const message = await provider.streamSimple({ id: 'synthetic', provider: 'possums', api: 'possums-verified' }, { messages: [] }).result();
    ext.message(message); assert.match(ext.warnings.at(-2), /possums_tor_native_proxy/);
    assert(!ext.warnings.join('').includes(hostile)); assert.equal(connects.length, before);
    provider.shutdown(); await ext.shutdown();
  });
  console.log(`Tor selection/diagnostics: ${checks} offline checks passed; no live Tor or inference.`);
} finally {
  process.env = originalEnv; globalThis.fetch = originalFetch;
  for (const name of ['__selectionPrepare', '__selectionScope', '__selectionConnect']) delete globalThis[name];
  await rm(out, { recursive: true, force: true });
}
