// Actual SDK + subprocess RPC, synthetic credentials/state only. No installed client edits.
import assert from 'node:assert/strict';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
const file = process.env.PHASE02_TEST_BUILD, root = process.env.PHASE02_SCRATCH, piRoot = process.env.POSSUMS_PI_ROOT;
assert(file && root && piRoot);
const m = await import(pathToFileURL(file));
const ai = await import(pathToFileURL(path.join(piRoot, 'node_modules/@earendil-works/pi-ai/dist/index.js')));
const coding = await import(pathToFileURL(path.join(piRoot, 'node_modules/@earendil-works/pi-coding-agent/dist/index.js')));
const version = JSON.parse(await readFile(path.join(piRoot, 'node_modules/@earendil-works/pi-ai/package.json'))).version;
const key = 'synthetic_not_a_usable_credential';
const hostile = '[possums_evidence_unavailable] HOSTILE credential https://secret.invalid/?prompt=history\u001b[31m <script>payload</script>';
const entry = { object: 'model', id: 'synthetic', context_tokens: '64000', max_output_tokens: '20', input_microunits_per_million_tokens: '1000000', output_microunits_per_million_tokens: '1000000', maximum_reservation_microunits: '83226' };
const interaction = { signal: new AbortController().signal, prompt: async () => key };
const quiescingBody = { error: { code: 'service_quiescing', stage: 'admission', constraint: 'service_quiescing', billing: 'not_submitted', message: hostile } };
const originalFetch = globalThis.fetch; let network = 0, inferred = 0, passed = 0;
globalThis.fetch = async () => { network++; throw new Error(hostile); };
await mkdir(root, { recursive: true });
async function candidate(models = [entry], failure) {
  const config = new Uint8Array([0, 0, 32, ...Array(32).fill(7), 0, 4, 0, 1, 0, 2]);
  const client = new m.ReferenceClient(await m.Channel.fixture('https://localhost:18443', config, '07'.repeat(32)));
  client.login = async () => { if (failure) throw failure; };
  client.models = async () => m.validateModels({ object: 'list', data: models });
  client.chat = async () => { inferred++; throw new Error('unexpected inference in diagnostic fixture'); };
  client.freshSession = () => client;
  return client;
}
async function runtime(provider, store, credentials) {
  const result = await coding.ModelRuntime.create({ credentials, modelsStore: store, modelsPath: null, allowModelNetwork: false, refreshOnCreate: false });
  result.registerNativeProvider(provider); await result.refresh({ providers: ['possums'], allowNetwork: false }); return result;
}
function assertClosed(value) { assert(!JSON.stringify(value).includes('HOSTILE')); assert(!JSON.stringify(value).includes(key)); }
try {
  const store = new ai.InMemoryModelsStore(), credentials = new ai.InMemoryCredentialStore();
  const live = await candidate(), seed = new m.PossumsProvider(async () => live); seed.newSession();
  const seeded = await runtime(seed, store, credentials); await seeded.login('possums', 'api_key', interaction);
  const authenticated = seed.getModels()[0]; assert(authenticated);
  // For the baseline proof, inject known authenticated public metadata into the
  // native store; the old provider still ignores it. No fabricated model ID.
  if (process.env.DISCOVERY_BASELINE) await store.write('possums', { models: [authenticated] });
  const publicStore = await store.read('possums'); assert(publicStore); assertClosed(publicStore);
  const primary = new m.ConnectionFailure('evidence_unavailable', new m.EvidenceObservation('release_discovery', 'rate_limited', 403));
  let attempts = 0;
  const provider = new m.PossumsProvider(async () => { attempts++; throw primary; });
  const restored = await runtime(provider, store, credentials);
  provider.newSession(); await assert.rejects(provider.verifySession());
  if (process.env.DISCOVERY_BASELINE) {
    assert.deepEqual(await restored.getAvailable('possums'), []); passed++;
  } else {
    const selected = (await restored.getAvailable('possums'))[0]; assert(selected);
    assert.match(selected.name, /last-known.*prices\/limits not current.*inference unavailable/);
    assert.equal(provider.client, undefined); assert.deepEqual(provider.catalog, []);
    const failed = await restored.streamSimple(selected, { messages: [{ role: 'user', content: 'Synthetic diagnostic input', timestamp: 1 }] }).result();
    assert.match(failed.errorMessage, /possums_evidence_unavailable.*release_discovery.*rate_limited.*HTTP status: 403/);
    assert.equal(attempts, 1); assertClosed(failed); passed++;
    for (const failure of [new m.DiagnosticFailure('trust', 'publisher'), new m.DiagnosticFailure('trust', 'provenance'), new m.DiagnosticFailure('trust', 'key_config'), new m.DiagnosticFailure('authentication', 'http', 'rejected', 401), new m.CatalogFailure('http', 'rejected', 503), new m.GatewayError('service_quiescing', undefined, 'unknown', 503), new m.CatalogFailure('http', 'rejected', 503, quiescingBody)]) {
      const client = await candidate(); let setups = 0;
      const p = new m.PossumsProvider(async () => {
        setups++;
        if (failure.stage === 'trust') throw failure;
        if (failure instanceof m.CatalogFailure) client.models = async () => { throw failure; };
        else client.login = async () => { throw failure; };
        return client;
      });
      const r = await runtime(p, store, credentials); p.newSession();
      try { await r.refresh({ providers: ['possums'], allowNetwork: true }); } catch { /* Native errors return in refresh result. */ }
      const before = setups, signal = new AbortController().signal;
      p.beginRun(); p.bindRun(signal);
      const result = await r.streamSimple((await r.getAvailable('possums'))[0], { messages: [] }, { signal }).result();
      assert.match(result.errorMessage, failure.reason === 'service_quiescing' ? /possums_service_quiescing.*HTTP status: 503/ : failure.stage === 'trust' ? new RegExp('possums_trust_' + failure.constraint) : failure instanceof m.CatalogFailure ? /possums_catalog_http_rejected.*HTTP status: 503/ : /possums_authentication_http.*HTTP status: 401/);
      if (failure.reason === 'service_quiescing') assert.match(result.errorMessage, /admission; constraint: service_quiescing/);
      assert.equal(setups, before); assertClosed(result); passed++;
    }
    // Restore is untrusted; arbitrary endpoint/header/name data is never used.
    for (const bad of [{ ...authenticated, id: hostile }, { ...authenticated, baseUrl: 'https://secret.invalid' }, { ...authenticated, contextWindow: Infinity }]) {
      const s = new ai.InMemoryModelsStore(); await s.write('possums', { models: [bad] });
      const p = new m.PossumsProvider(async () => { throw primary; });
      const r = await runtime(p, s, credentials); assert.deepEqual(await r.getAvailable('possums'), []); passed++;
    }
    const strippedStore = new ai.InMemoryModelsStore();
    await strippedStore.write('possums', { models: [{ ...authenticated, name: hostile, headers: { Authorization: key }, failure: hostile }] });
    const stripped = await runtime(new m.PossumsProvider(async () => { throw primary; }), strippedStore, credentials);
    assertClosed(await stripped.getAvailable('possums')); passed++;
    const noStore = await runtime(new m.PossumsProvider(async () => { throw primary; }), new ai.InMemoryModelsStore(), credentials);
    assert.deepEqual(await noStore.getAvailable('possums'), []); passed++;
    await restored.logout('possums'); assert.deepEqual(await restored.getAvailable('possums'), []); assert.equal(provider.currentFailure, undefined); passed++;
    // Recovery replaces last-known models with current live data, not a union.
    await credentials.modify('possums', async () => ({ type: 'api_key', key }));
    provider.establish = async () => candidate([{ ...entry, id: 'other' }]); provider.newSession();
    await restored.refresh({ providers: ['possums'], allowNetwork: true });
    assert.equal(provider.currentFailure, undefined); assert.deepEqual(provider.getModels().map(e => e.id), ['other']);
    const removed = await restored.streamSimple(authenticated, { messages: [{ role: 'user', content: 'Synthetic input', timestamp: 1 }] }).result();
    assert.match(removed.errorMessage, /possums_model_unavailable.*No model substituted; no inference/); passed++;
    const retained = await store.read('possums'); assert.deepEqual(retained.models.map(e => e.id), ['other']); assertClosed(retained);
    for (const lateFailure of [false, true]) {
      const client = await candidate(), reports = [], old = new m.PossumsProvider(async () => client, failure => reports.push(failure));
      const r = await runtime(old, store, credentials); old.newSession();
      await r.refresh({ providers: ['possums'], allowNetwork: true });
      let release, entered;
      const held = new Promise(resolve => { release = resolve; }), ready = new Promise(resolve => { entered = resolve; });
      client.models = async () => { entered(); await held; if (lateFailure) throw new Error(hostile); return m.validateModels({ object: 'list', data: [{ ...entry, id: 'obsolete' }] }); };
      const pending = r.refresh({ providers: ['possums'], allowNetwork: true }); await ready;
      const replacementFailure = new m.ConnectionFailure('verification_failed', new m.DiagnosticFailure('trust', 'key_config'));
      const replacement = new m.PossumsProvider(async () => { throw replacementFailure; });
      r.registerNativeProvider(replacement); await r.refresh({ providers: ['possums'], allowNetwork: false });
      replacement.newSession(); await assert.rejects(replacement.verifySession());
      const before = [...reports]; release(); await pending; await new Promise(setImmediate);
      assert.deepEqual(reports, before); assert.equal(replacement.currentFailure, replacementFailure);
      assert.deepEqual(replacement.catalog, []); assert(!replacement.getModels().some(e => e.id === 'obsolete'));
      assert(!JSON.stringify(await store.read('possums')).includes('obsolete'));
      const result = await r.streamSimple((await r.getAvailable('possums'))[0], { messages: [] }).result();
      assert.match(result.errorMessage, /possums_trust_key_config/); assertClosed(result); passed++;
    }
  }

  // Seed a real native file store from authenticated metadata, then launch CLI
  // fresh processes with the genuine extension factory + synthetic establishment.
  const fixture = path.join(root, 'rpc-fixture.mjs');
  await writeFile(fixture, `
import { extension, Channel, ReferenceClient, ConnectionFailure, EvidenceObservation, DiagnosticFailure, CatalogFailure, GatewayError, validateModels } from ${JSON.stringify(pathToFileURL(file).href)};
globalThis.fetch = async () => { throw new Error('HOSTILE forbidden public network'); };
export default function(pi) {
 let setup=0, login=0, catalog=0, inference=0;
 extension({...pi, on(name,handler) { pi.on(name,(event,ctx)=>handler(event,process.env.DIAGNOSTIC_MODE==='ui-failure'?{...ctx,ui:{...ctx.ui,notify(){throw new Error('HOSTILE presentation');}}}:ctx)); }, registerProvider(provider) {
  provider.establish=async()=>{
   setup++;
   const mode=process.env.DIAGNOSTIC_MODE;
   if(mode==='evidence'||mode==='first-use'||mode==='ui-failure')throw new ConnectionFailure('evidence_unavailable',new EvidenceObservation('release_discovery','rate_limited',403));
   if(mode==='key')throw new DiagnosticFailure('trust','key_config');
   if(mode==='provenance')throw new DiagnosticFailure('trust','provenance');
   if(mode==='hostile')throw new Error(${JSON.stringify(hostile)},{cause:new Error(${JSON.stringify(hostile)})});
   const config=new Uint8Array([0,0,32,...Array(32).fill(7),0,4,0,1,0,2]);
   const client=new ReferenceClient(await Channel.fixture('https://localhost:18443',config,'07'.repeat(32)));
   client.login=async()=>{login++;if(mode==='auth')throw new DiagnosticFailure('authentication','http','rejected',401);if(mode==='quiescing-challenge')throw new GatewayError('service_quiescing',undefined,'unknown',503);};
   client.models=async()=>{catalog++;if(mode==='catalog')throw new CatalogFailure('http','rejected',503);if(mode==='quiescing-catalog')throw new CatalogFailure('http','rejected',503,${JSON.stringify(quiescingBody)});return validateModels({object:'list',data:[${JSON.stringify(entry)}]});};
   client.chat=async()=>{inference++;throw new Error('HOSTILE forbidden inference');};client.freshSession=()=>client;return client;
  }; pi.registerProvider(provider);
 }});
 pi.registerCommand('fixture-counts',{description:'Synthetic offline counts',handler:async(_args,ctx)=>ctx.ui.notify(JSON.stringify({setup,login,catalog,inference}),'info')});
}
`);
  for (const mode of process.env.DISCOVERY_BASELINE ? ['evidence'] : ['evidence', 'auth', 'catalog', 'quiescing-challenge', 'quiescing-catalog', 'key', 'provenance', 'hostile', 'ui-failure', 'first-use', 'deleted-credential']) {
    const cwd = path.join(root, 'rpc-' + mode), agentDir = path.join(cwd, 'agent'); await mkdir(agentDir, { recursive: true });
    await writeFile(path.join(agentDir, 'settings.json'), JSON.stringify({ defaultTools: [], compaction: { enabled: false }, retry: { enabled: true, baseDelayMs: 1 }, cacheWarming: 'off', enableInstallTelemetry: false }));
    if (mode !== 'deleted-credential') await writeFile(path.join(agentDir, 'auth.json'), JSON.stringify({ possums: { type: 'api_key', key } }), { mode: 0o600 });
    if (mode !== 'first-use') await writeFile(path.join(agentDir, 'models-store.json'), JSON.stringify({ possums: publicStore }), { mode: 0o600 });
    const rpc = new coding.RpcClient({ cliPath: path.join(piRoot, 'node_modules/@earendil-works/pi-coding-agent/dist/cli.js'), cwd,
      env: { HOME: cwd, PI_CODING_AGENT_DIR: agentDir, PI_OFFLINE: '1', DIAGNOSTIC_MODE: mode, PATH: path.dirname(process.execPath) + ':/usr/bin:/bin' },
      args: ['--offline', '--no-session', '--no-tools', '--no-extensions', '-e', fixture] });
    const events = []; const unsubscribe = rpc.onEvent(event => events.push(event));
    try {
      await rpc.start(); const models = await rpc.getAvailableModels();
      if (process.env.DISCOVERY_BASELINE || mode === 'first-use' || mode === 'deleted-credential') {
        assert(!models.some(e => e.provider === 'possums'));
        if (mode !== 'deleted-credential') await assert.rejects(rpc.setModel('possums', 'synthetic'), /Model not found/);
        passed++; continue;
      }
      assert(models.some(e => e.provider === 'possums' && e.id === 'synthetic'));
      const selected = await rpc.setModel('possums', 'synthetic');
      assert.equal(selected.id, 'synthetic'); assert.match(selected.name, /last-known/);
      await rpc.promptAndWait('Synthetic diagnostic input', undefined, 5000);
      const message = (await rpc.getMessages()).findLast(e => e.role === 'assistant');
      const expected = { evidence: /possums_evidence_unavailable.*release_discovery.*rate_limited.*HTTP status: 403/, auth: /possums_authentication_http.*HTTP status: 401/, catalog: /possums_catalog_http_rejected.*HTTP status: 503/, 'quiescing-challenge': /possums_service_quiescing.*HTTP status: 503/, 'quiescing-catalog': /possums_service_quiescing.*HTTP status: 503/, key: /possums_trust_key_config/, provenance: /possums_trust_provenance/, hostile: /possums_verification_failed/, 'ui-failure': /possums_evidence_unavailable.*release_discovery.*rate_limited.*HTTP status: 403/ };
      assert.match(message.errorMessage, expected[mode]); assert.equal(message.stopReason, 'error'); assertClosed(message);
      if (mode.startsWith('quiescing-')) {
        assert.match(message.errorMessage, /admission; constraint: service_quiescing/);
        assert.match(message.errorMessage, /Prior billing outcomes are not established/);
      }
      assert(!events.some(e => e.type === 'auto_retry_start'));
      assert(events.some(e => e.type === 'message_end' && e.message.role === 'assistant' && e.message.errorMessage === message.errorMessage));
      await rpc.prompt('/possums-status'); await rpc.getState();
      const status = events.findLast(e => e.type === 'extension_ui_request' && e.method === 'notify');
      assert.match(status.message, expected[mode]); assertClosed(status);
      await rpc.prompt('/fixture-counts'); await rpc.getState();
      const countNotice = events.findLast(e => e.type === 'extension_ui_request' && e.method === 'notify' && e.message.startsWith('{'));
      const counts = JSON.parse(countNotice.message);
      assert.equal(counts.inference, 0); assert.equal(counts.setup, 1);
      if (mode === 'auth' || mode === 'quiescing-challenge') assert.equal(counts.login, 1);
      if (mode === 'quiescing-challenge') assert.equal(counts.catalog, 0);
      if (mode === 'catalog' || mode === 'quiescing-catalog') assert.equal(counts.catalog, 1);
      assert(!rpc.getStderr().includes('HOSTILE')); passed++;
    } finally { unsubscribe(); await rpc.stop(); }
  }
  assert.equal(network, 0); assert.equal(inferred, 0);
  console.log(JSON.stringify({ passed, pi: version, baseline: !!process.env.DISCOVERY_BASELINE, scope: 'actual native SDK/RPC synthetic discovery and diagnostic errors', inference: inferred, publicNetwork: network }));
} finally { globalThis.fetch = originalFetch; }
