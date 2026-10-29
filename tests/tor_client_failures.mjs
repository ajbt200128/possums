// OFFLINE ONLY: synthetic native carriers and local HPKE, never sockets/helper/Tor.
// PRIVACY.md: data-handling boundaries, forbidden telemetry, typed interface
// failures and synthetic testing. No raw cause, request data or new persistence.
import assert from 'node:assert/strict';
import { mkdtemp, readFile, realpath, rm, symlink } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
const source = process.env.PHASE02_RELEASE_SOURCE;
assert(source && path.isAbsolute(source), 'select a matching scratch source with pinned dependencies');
assert.equal(process.version, 'v24.13.0');
for (const [name, version] of [['esbuild', '0.25.10'], ['typescript', '5.9.3'], ['@earendil-works/pi-ai', '1.3.0'], ['@earendil-works/pi-coding-agent', '1.3.0']]) {
  assert.equal(JSON.parse(await readFile(path.join(source, 'node_modules', name, 'package.json'))).version, version);
}
const { build } = await import(pathToFileURL(path.join(source, 'node_modules/esbuild/lib/main.js')));
const ehbp = await import(pathToFileURL(path.join(source, 'node_modules/ehbp/dist/esm/index.js')));
const out = await realpath(await mkdtemp(path.join(tmpdir(), 'possums-tor-client-failures-')));
const originalFetch = globalThis.fetch, enc = new TextEncoder();
const hostile = 'HOSTILE_SYNTHETIC_SECRET_URL_ONION_PROMPT';
let checks = 0, fetches = 0;
async function check(name, body) { await body(); checks++; console.log('PASS ' + name); }
const entry = { object: 'model', id: 'synthetic', context_tokens: '20', max_output_tokens: '20',
  input_microunits_per_million_tokens: '1000000', output_microunits_per_million_tokens: '1000000',
  maximum_reservation_microunits: '52', tool_protocol: 'openai-functions-v1' };
const messages = [{ role: 'user', content: 'Synthetic input' }];
const event = fields => 'data: ' + JSON.stringify({ object: 'chat.completion.chunk', model: entry.id, ...fields }) + '\n\n';
const delta = value => event({ choices: [{ index: 0, delta: value, finish_reason: null }] });
const start = delta({ role: 'assistant', content: 'Synthetic retained text',
  tool_calls: [{ index: 0, id: 'synthetic_call', type: 'function', function: { name: 'echo', arguments: '{"value":"ok"}' } }] });
const finish = event({ choices: [{ index: 0, delta: {}, finish_reason: 'tool_calls' }] });
const money = (outcome = 'settled') => event({ choices: [], usage: { prompt_tokens: 2, completion_tokens: 3, total_tokens: 5 },
  possums: { outcome, status: outcome === 'refunded' ? 'incomplete' : 'complete',
    charged_microunits: outcome === 'refunded' ? '0' : '7', refunded_microunits: outcome === 'refunded' ? '52' : '45' } });
const done = 'data: [DONE]\n\n';
const gateway = 'data: ' + JSON.stringify({ error: { code: 'generation_failed', detail: 'generation_http_failed', billing: 'refunded', message: hostile } }) + '\n\n';
const fixtures = new Map();
let nextPort = 18443;
try {
  await symlink(path.join(source, 'node_modules'), path.join(out, 'node_modules'));
  await build({ stdin: { contents: `export { Channel } from '../../clients/pi/core/transport.ts';
    export * from '../../clients/pi/core/client.ts'; export { DiagnosticFailure } from '../../clients/pi/core/limits.ts';
    export { PossumsProvider } from './provider.ts'; export * from './diagnostics.ts';
    export { TorNativeTransportFailure } from './tor-native-transport.mjs';`, resolveDir: source, loader: 'ts' },
    outfile: path.join(out, 'check.mjs'), bundle: true, platform: 'node', format: 'esm', target: 'node24',
    external: ['@earendil-works/*'], define: { __PHASE01_FIXTURE__: 'true' } });
  const m = await import(pathToFileURL(path.join(out, 'check.mjs')));
  const native = (kind = 'bridge_unknown') => new m.TorNativeTransportFailure(kind);
  // Same maintained SDK as the client. No custom wire decoder or network.
  globalThis.fetch = async req => {
    fetches++;
    const url = new URL(req.url), f = fixtures.get(url.origin);
    assert(f, 'network forbidden: only registered synthetic fixture origins');
    const route = url.pathname;
    f.requests.push(route);
    const fault = f.fault?.route === route ? f.fault : undefined;
    if (fault?.when === 'send') throw fault.error;
    if (route === '/v1/auth/challenge') return Response.json({ challenge: 'c'.repeat(43), expires_in: 600 });
    if (route === '/v1/models') return Response.json({ object: 'list', data: [entry] });
    const encapsulated = ehbp.hexToBytes(req.headers.get('Ehbp-Encapsulated-Key'));
    const recipient = await f.server.suite.SetupRecipient(f.server.getPrivateKey(), encapsulated, { info: enc.encode(ehbp.HPKE_REQUEST_INFO) });
    const encrypted = new Uint8Array(await req.arrayBuffer());
    await recipient.Open(encrypted.slice(4));
    const text = route === '/v1/sessions' ? JSON.stringify({ token: 'b'.repeat(43), token_type: 'Bearer', expires_in: 43200 }) :
      route === '/v1/submissions' ? JSON.stringify({ submission: 's'.repeat(43) }) : f.text;
    const nonce = crypto.getRandomValues(new Uint8Array(32));
    const secret = new Uint8Array(await recipient.Export(enc.encode(ehbp.EXPORT_LABEL), ehbp.EXPORT_LENGTH));
    const keys = await ehbp.deriveResponseKeys(secret, encapsulated, nonce);
    const cipher = await ehbp.encryptChunk(keys, 0, enc.encode(text));
    if (fault?.when === 'corrupt') cipher[cipher.length - 1] ^= 1;
    const frame = new Uint8Array(4 + cipher.length);
    new DataView(frame.buffer).setUint32(0, cipher.length, false); frame.set(cipher, 4);
    let sent = false;
    const body = new ReadableStream({ pull(c) {
      if (fault?.when === 'headers' || (sent && fault?.when === 'late')) { c.error(fault.error); return; }
      if (sent) { c.close(); return; }
      sent = true; c.enqueue(frame);
    } }, { highWaterMark: 0 });
    return new Response(body, { headers: { 'Ehbp-Response-Nonce': ehbp.bytesToHex(nonce),
      'Content-Type': route === '/v1/chat/completions' ? 'text/event-stream' : 'application/json' } });
  };
  async function fixture() {
    const server = await ehbp.Identity.generate(), origin = 'https://localhost:' + nextPort++;
    const channel = await m.Channel.fixture(origin, await server.marshalConfig(), await server.getPublicKeyHex());
    const f = { server, channel, client: new m.ReferenceClient(channel), requests: [], text: start + finish + money() + done };
    fixtures.set(origin, f); return f;
  }
  async function caught(action) { try { await action(); } catch (error) { return error; } assert.fail('expected classified failure'); }
  function projection(error, stage, kind = 'bridge_unknown') {
    assert(error instanceof m.CompletionFailure);
    assert.equal(error.stage, stage);
    assert.equal(error.nativeDiagnostic.code, 'possums_tor_native_' + kind);
    assert(Object.isFrozen(error)); assert(Object.isFrozen(error.nativeDiagnostic));
    assert.equal(error.cause, undefined); assert.equal(error.nativeDiagnostic.stack, undefined);
    const text = m.diagnosticDescription(error);
    assert.match(text, error.settlement ? /Remote completion is unknown.*authenticated billing evidence is retained/ : /Remote completion and billing are unknown/);
    if (error.settlement) assert(!text.includes('billing are unknown'));
    assert.match(text, /No request was replayed/); assert.match(text, /new submission may incur another charge/);
    assert(!text.includes(hostile)); assert(!text.includes('No inference request was sent'));
    return text;
  }
  const login = f => f.client.login('synthetic_not_a_usable_credential');
  async function provider(f) {
    const p = new m.PossumsProvider(async () => f.client);
    p.newSession();
    await p.auth.apiKey.login({ signal: new AbortController().signal, prompt: async () => 'synthetic_not_a_usable_credential', notify() {} });
    p.beginRun(); return p;
  }
  async function turn(p, options = {}) {
    const stream = p.streamSimple(p.getModels()[0], { messages: [{ ...messages[0], timestamp: 1 }],
      tools: [{ name: 'echo', description: 'Synthetic fixture', parameters: { type: 'object', properties: { value: { type: 'string' } } } }] }, options);
    const events = []; for await (const item of stream) events.push(item);
    return { result: await stream.result(), events };
  }
  function warning(result, code) {
    assert.equal(result.stopReason, 'stop'); assert.equal(result.errorMessage, undefined);
    const w = result.diagnostics.find(item => item.type === 'possums_turn_warning').details;
    assert.equal(w.backendGenerationComplete, false); assert.equal(w.toolsAuthorized, false);
    assert(w.message.includes(code)); assert(!JSON.stringify(result).includes(hostile));
    assert(!result.content.some(item => item.type === 'toolCall'));
    return w.message;
  }
  function body(text, error, cancelError) {
    let sent = false;
    return new ReadableStream({ pull(c) {
      if (!sent && text) { sent = true; c.enqueue(enc.encode(text)); }
      else if (error) c.error(error); else c.close();
    }, cancel() { if (cancelError) throw cancelError; } }, { highWaterMark: 0 });
  }
  await check('actual ReferenceClient challenge/login/catalog/submission/send preserve native categories', async () => {
    for (const [route, stage, method] of [
      ['/v1/auth/challenge', 'challenge', 'login'], ['/v1/sessions', 'authentication', 'login'],
      ['/v1/models', 'catalog', 'models'],
      ['/v1/submissions', 'submission', 'chat'], ['/v1/chat/completions', 'transport', 'chat'],
    ]) {
      const f = await fixture();
      if (method !== 'login') await login(f);
      f.fault = { route, when: 'send', error: native('proxy') };
      const error = await caught(() => method === 'login' ? login(f) : method === 'chat' ? f.client.chat(entry.id, messages, () => {}) : f.client[method]());
      projection(error, stage, 'proxy');
      assert.equal(f.requests.filter(item => item === route).length, 1);
      if (route === '/v1/submissions') assert(!f.requests.includes('/v1/chat/completions'));
    }
  });
  await check('control body native failures exposed by Tor collector survive reference wrappers', async () => {
    // Substitute the channel's already-classified collector result, not SDK wire
    // decoding. Genuine encrypted body/read failures are exercised below.
    for (const target of ['challenge', 'authentication', 'catalog', 'submission']) {
      const f = await fixture(); if (!['challenge', 'authentication'].includes(target)) await login(f);
      if (target === 'challenge') f.channel.challenge = async () => { throw native('truncated'); };
      else if (target === 'authentication' || target === 'submission') f.channel.control = async () => { throw native('truncated'); };
      else f.channel.models = async () => { throw native('truncated'); };
      const error = await caught(() => ['challenge', 'authentication'].includes(target) ? login(f) : target === 'catalog' ? f.client.models() : f.client.chat(entry.id, messages, () => {}));
      projection(error, target, 'truncated');
    }
  });
  await check('actual encrypted chat after headers/late body/receipt plus DONE without EOF preserve closed native and money', async () => {
    for (const [when, text, hasMoney] of [['headers', start, false], ['late', start, false],
      ['late', start + finish + money(), true], ['late', start + finish + money() + done, true]]) {
      const f = await fixture(); await login(f); f.text = text;
      f.fault = { route: '/v1/chat/completions', when, error: native() };
      const error = await caught(() => f.client.chat(entry.id, messages, () => {}));
      const description = projection(error, text.endsWith(done) && when === 'late' ? 'stream' : 'transport');
      assert.equal(error.status, 200); assert.match(description, /Observed HTTP status: 200/);
      assert.equal(Boolean(error.settlement), hasMoney);
      if (hasMoney) { assert.equal(error.settlement.chargedMicrounits, '7'); assert.equal(error.settlement.refundedMicrounits, '45'); assert.equal(error.settlement.deliveryComplete, false); }
      assert.equal(f.requests.filter(route => route === '/v1/chat/completions').length, 1);
    }
  });
  await check('consumeCompletion native interruption retains settled/refunded evidence but never terminal authority', async () => {
    for (const outcome of ['settled', 'refunded']) {
      const error = await caught(() => m.consumeCompletion(body(start + finish + money(outcome) + done, native()), entry, () => {}));
      projection(error, 'stream');
      assert.equal(error.settlement.outcome, outcome); assert.equal(error.settlement.deliveryComplete, false);
      assert.equal(error.settlement.chargedMicrounits, outcome === 'refunded' ? '0' : '7');
    }
    for (const kind of ['configuration', 'destination', 'closed', 'cancelled', 'deadline', 'proxy', 'protocol',
      'authentication', 'unsupported', 'handshake_bound', 'truncated', 'handshake_io', 'handshake_unknown',
      'tunnel', 'helper', 'ipc_unknown', 'bridge_unknown']) {
      projection(await caught(() => m.consumeCompletion(body('', native(kind)), entry, () => {})), 'transport', kind);
    }
  });
  await check('existing gateway primary survives late native read/cleanup; bare refund still needs authenticated EOF', async () => {
    for (const text of [gateway, start + money() + gateway + done]) {
      const error = await caught(() => m.consumeCompletion(body(text, native(), native('cancelled')), entry, () => {}));
      assert(error instanceof m.GatewayError); assert.equal(error.reason, 'generation_failed'); assert.equal(error.detail, 'generation_http_failed');
      assert.equal(error.billing, text === gateway ? 'unknown' : 'settled');
      if (text !== gateway) { assert.equal(error.settlement.chargedMicrounits, '7'); assert.equal(error.settlement.deliveryComplete, false); }
      assert(!error.message.includes(hostile));
    }
    for (const text of [gateway, start + money() + gateway]) {
      let cancellations = 0;
      const interrupted = new ReadableStream({ start(c) { c.enqueue(enc.encode(text + 'invalid trailing line\n')); },
        cancel() { cancellations++; throw native('cancelled'); } }, { highWaterMark: 0 });
      const error = await caught(() => m.consumeCompletion(interrupted, entry, () => {}));
      assert.equal(cancellations, 1, 'exercise a real failing underlying cancel, not cancel of an errored/closed stream');
      assert(error instanceof m.GatewayError); assert.equal(error.detail, 'generation_http_failed');
      assert.equal(error.billing, text === gateway ? 'unknown' : 'settled');
      if (text !== gateway) assert.equal(error.settlement.deliveryComplete, false);
    }
    const error = await caught(() => m.consumeCompletion(body(gateway, undefined, native('cancelled')), entry, () => {}));
    assert.equal(error.billing, 'refunded');
  });
  await check('later real integrity failure overrides operational error without erasing receipt', async () => {
    const error = await caught(() => m.consumeCompletion(body(start + money() + gateway + done,
      new m.DiagnosticFailure('transport', 'decryption', 'uncertain')), entry, () => {}));
    assert(error instanceof m.CompletionFailure); assert.equal(error.constraint, 'decryption');
    assert.equal(error.settlement.chargedMicrounits, '7'); assert.equal(error.settlement.deliveryComplete, false);
    assert.equal(error.nativeDiagnostic, undefined);
  });
  await check('mutable diagnostic replacements/getters and hostile unknown errors cannot escape or become native authority', async () => {
    for (const mode of ['fields', 'code', 'getter']) {
      const f = await fixture(); await login(f); const error = native('proxy'); error.message = hostile;
      error.diagnostic = { code: mode === 'fields' ? 'possums_tor_native_proxy' : hostile,
        stage: hostile, constraint: hostile, action: hostile, outcome: hostile };
      if (mode === 'getter') Object.defineProperty(error, 'diagnostic', { get() { throw Error(hostile); } });
      f.fault = { route: '/v1/chat/completions', when: 'send', error };
      const wrapped = await caught(() => f.client.chat(entry.id, messages, () => {}));
      projection(wrapped, 'transport', mode === 'fields' ? 'proxy' : 'ipc_unknown');
      if (mode === 'fields') error.diagnostic.code = hostile;
      assert(!JSON.stringify(wrapped.nativeDiagnostic).includes(hostile));
    }
    const fake = new Error(hostile); fake.diagnostic = native().diagnostic;
    const unknown = await caught(() => m.consumeCompletion(body('', fake), entry, () => {}));
    assert(unknown instanceof m.CompletionFailure); assert.equal(unknown.nativeDiagnostic, undefined);
    assert.equal(unknown.constraint, 'fetch'); assert(!m.diagnosticDescription(unknown).includes(hostile));
    const forged = new m.CompletionFailure('transport', 'fetch', undefined, undefined,
      { code: 'possums_tor_native_proxy', stage: hostile, constraint: hostile, action: hostile, outcome: hostile });
    assert(!m.diagnosticDescription(forged).includes(hostile));
  });
  await check('user hooks throwing native objects stay hook failures, not transport diagnostics', async () => {
    for (const hook of ['onPayload', 'onResponse', 'onEvent', 'onDelta']) {
      const f = await fixture(); await login(f);
      const throws = () => { throw native('proxy'); };
      const error = await caught(() => f.client.chat(entry.id, messages, hook === 'onDelta' ? throws : () => {}, false,
        hook === 'onDelta' ? {} : { [hook]: throws }));
      assert.equal(error.stage, 'hook'); assert.equal(error.constraint, 'unexpected');
      assert.equal(error.nativeDiagnostic, undefined); assert(!m.diagnosticDescription(error).includes('possums_tor_native'));
    }
    const f = await fixture(); await login(f);
    const prepared = await f.client.prepareChat(entry.id, messages, () => {});
    try {
      const error = await caught(() => f.client.chatPrepared(prepared, () => { throw native('proxy'); }));
      assert.equal(error.stage, 'transport'); assert.equal(error.constraint, 'fetch'); assert.equal(error.nativeDiagnostic, undefined);
      assert.equal(prepared.dispatched, false); assert(!f.requests.includes('/v1/chat/completions'));
    } finally { prepared.close(); }
  });
  await check('provider native warnings suppress provisional tools, retain money, never replay, and permit deliberate next turn', async () => {
    for (const outcome of ['settled', 'refunded']) {
      const f = await fixture(), p = await provider(f);
      try {
        f.text = start + finish + money(outcome) + done;
        f.fault = { route: '/v1/chat/completions', when: 'late', error: native() };
        const { result, events } = await turn(p);
        const text = warning(result, 'possums_tor_native_bridge_unknown');
        assert.match(text, /Delivery incomplete/); assert(!text.includes('No inference request was sent'));
        const receipt = result.diagnostics.find(item => item.type === 'possums_settled_receipt').details;
        assert.equal(receipt.chargedMicrounits, outcome === 'settled' ? '7' : '0');
        assert.equal(receipt.refundedMicrounits, outcome === 'settled' ? '45' : '52'); assert.equal(receipt.deliveryComplete, false);
        assert.equal(events.at(-1).type, 'done'); assert(!events.some(item => item.type === 'toolcall_end'));
        assert.equal(f.requests.filter(route => route === '/v1/chat/completions').length, 1);
        f.fault = undefined; f.text = delta({ role: 'assistant', content: 'Synthetic follow-up' }) +
          event({ choices: [{ index: 0, delta: {}, finish_reason: 'stop' }] }) + money() + done;
        const next = await turn(p); assert.equal(next.result.stopReason, 'stop');
        assert(!next.result.diagnostics.some(item => item.type === 'possums_turn_warning'));
        assert.equal(f.requests.filter(route => route === '/v1/chat/completions').length, 2);
      } finally { p.shutdown(); }
    }
  });
  await check('provider send/header failures warn unknown billing and known gateway primary remains primary', async () => {
    for (const mode of ['send', 'headers', 'primary']) {
      const f = await fixture(), p = await provider(f);
      try {
        f.text = gateway; f.fault = { route: '/v1/chat/completions', when: mode === 'primary' ? 'late' : mode, error: native('proxy') };
        const { result } = await turn(p);
        const text = warning(result, mode === 'primary' ? 'generation_http_failed' : 'possums_tor_native_proxy');
        assert.match(text, /Charge unknown/); assert(!text.includes('Reservation refunded')); assert(!text.includes('No inference request was sent'));
        if (mode === 'primary') assert(!text.includes('possums_tor_native'));
        assert.equal(f.requests.filter(route => route === '/v1/chat/completions').length, 1);
      } finally { p.shutdown(); }
    }
  });
  await check('provider gateway primary plus later native failure keeps received charged/refunded money', async () => {
    for (const outcome of ['settled', 'refunded']) {
      const f = await fixture(), p = await provider(f);
      try {
        f.text = start + money(outcome) + gateway + done;
        f.fault = { route: '/v1/chat/completions', when: 'late', error: native() };
        const { result } = await turn(p);
        const text = warning(result, 'generation_http_failed'); assert(!text.includes('possums_tor_native'));
        const receipt = result.diagnostics.find(item => item.type === 'possums_settled_receipt').details;
        assert.equal(receipt.chargedMicrounits, outcome === 'settled' ? '7' : '0');
        assert.equal(receipt.refundedMicrounits, outcome === 'settled' ? '45' : '52'); assert.equal(receipt.deliveryComplete, false);
        assert.equal(f.requests.filter(route => route === '/v1/chat/completions').length, 1);
      } finally { p.shutdown(); }
    }
  });
  await check('provider actual decryption failure remains fatal and blocks another dispatch', async () => {
    const f = await fixture(), p = await provider(f);
    try {
      f.fault = { route: '/v1/chat/completions', when: 'corrupt' };
      const first = await turn(p); assert.equal(first.result.stopReason, 'error');
      assert.match(first.result.errorMessage, /decryption/); assert(!first.result.diagnostics.some(item => item.type === 'possums_turn_warning'));
      const before = f.requests.length; assert.equal((await turn(p)).result.stopReason, 'error'); assert.equal(f.requests.length, before);
    } finally { p.shutdown(); }
  });
  await check('normal clearnet completion is unchanged and diagnostic formatting initiates no work', async () => {
    const f = await fixture(); await login(f);
    const receipt = await f.client.chat(entry.id, messages, () => {});
    assert.equal(receipt.finish, 'tool_calls'); assert.equal(receipt.deliveryComplete, true); assert.equal(receipt.chargedMicrounits, '7');
    const before = fetches;
    const error = new m.CompletionFailure('transport', 'fetch', undefined, 200, m.safeTorNativeDiagnostic(native('proxy')));
    for (let i = 0; i < 3; i++) projection(error, 'transport', 'proxy');
    assert.equal(fetches, before);
    assert.equal(m.diagnosticDescription(new m.DiagnosticFailure('transport', 'fetch', 'uncertain')),
      '[possums_transport_fetch] Stage: transport; constraint: fetch. The fetch constraint was not satisfied. Check connectivity; share only this code/stage/constraint if the problem persists.');
  });
  console.log(`Passed ${checks} offline client failure groups. No live/installed/runtime-rendering claim.`);
} finally { globalThis.fetch = originalFetch; await rm(out, { recursive: true, force: true }); }
