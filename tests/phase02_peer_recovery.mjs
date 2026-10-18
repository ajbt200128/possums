// Actual loopback TLS + provider/reference setup boundary; no public network,
// real credentials, hardware appraisal or inference. Candidate trust is synthetic.
import assert from 'node:assert/strict';
import { createHash, X509Certificate } from 'node:crypto';
import { createServer } from 'node:https';
import { execFileSync } from 'node:child_process';
import { mkdir, readFile } from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
const { PHASE02_TEST_BUILD: bundle, PHASE02_SCRATCH: root, POSSUMS_PI_ROOT: piRoot } = process.env;
assert(bundle && root && piRoot);
const m = await import(pathToFileURL(bundle));
const ai = await import(pathToFileURL(path.join(piRoot, 'node_modules/@earendil-works/pi-ai/dist/index.js')));
await mkdir(root, { recursive: true });
const leaves = {};
for (const name of ['A', 'B']) {
  const key = path.join(root, name + '-key.pem'), cert = path.join(root, name + '-cert.pem');
  execFileSync('/usr/bin/openssl', ['req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-keyout', key,
    '-out', cert, '-days', '1', '-subj', '/CN=localhost', '-addext', 'subjectAltName=DNS:localhost'], { stdio: 'ignore' });
  const certificate = await readFile(cert);
  leaves[name] = { key: await readFile(key), cert: certificate,
    pin: createHash('sha256').update(new X509Certificate(certificate).publicKey.export({ type: 'spki', format: 'der' })).digest('hex') };
}
const entry = { object: 'model', id: 'synthetic', context_tokens: '20', max_output_tokens: '20',
  input_microunits_per_million_tokens: '1000000', output_microunits_per_million_tokens: '1000000', maximum_reservation_microunits: '52' };
const catalog = { object: 'list', data: [entry] };
const frame = value => 'data: ' + JSON.stringify(value) + '\n\n';
const completion = frame({ object: 'chat.completion.chunk', model: entry.id,
  choices: [{ index: 0, delta: { role: 'assistant', content: 'Synthetic answer' }, finish_reason: null }] }) +
  frame({ object: 'chat.completion.chunk', model: entry.id, choices: [{ index: 0, delta: {}, finish_reason: 'stop' }] }) +
  frame({ object: 'chat.completion.chunk', model: entry.id, choices: [], usage: { prompt_tokens: 1, completion_tokens: 1, total_tokens: 2 },
    possums: { outcome: 'settled', charged_microunits: '2', refunded_microunits: '50' } }) + 'data: [DONE]\n\n';
const context = ai.normalizeContext({ messages: [{ role: 'user', content: 'Synthetic input', timestamp: 1 }] });
const credential = 'synthetic_not_a_usable_credential';
const interaction = () => ({ signal: new AbortController().signal, prompt: async () => credential, notify() {} });
const fetch = globalThis.fetch;
globalThis.fetch = async () => { throw new Error('HOSTILE unexpected public fetch'); };
let passed = 0;
async function check(name, body) { await body(); passed++; console.log('PASS ' + name); }
function deferred() { let resolve; const promise = new Promise(r => { resolve = r; }); return { promise, resolve }; }
async function fixture() {
  let active = 'A', rotateCatalog = false, rotateChat = false, probes = 0, hooks = 0;
  const counts = { challenge: 0, sessions: 0, models: 0, submissions: 0, chats: 0 };
  const prepared = [];
  const server = createServer(leaves.A, (req, res) => {
    req.resume();
    let body;
    switch (req.url) {
      case '/v1/auth/challenge': counts.challenge++; body = { challenge: 'c'.repeat(43), expires_in: 600 }; break;
      case '/v1/sessions': counts.sessions++; body = { token: 'b'.repeat(43), token_type: 'Bearer', expires_in: 43200 }; break;
      case '/v1/models': counts.models++; body = catalog; break;
      case '/v1/submissions': counts.submissions++; body = { submission: 's'.repeat(43) }; break;
      case '/v1/chat/completions': counts.chats++; break;
      default: assert.fail('unexpected loopback endpoint');
    }
    res.writeHead(200, { 'content-type': body ? 'application/json' : 'text/event-stream' });
    res.end(body ? JSON.stringify(body) : completion);
    if (rotateCatalog && req.url === '/v1/models') rotate();
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const origin = `https://localhost:${server.address().port}`;
  function rotate() { active = active === 'A' ? 'B' : 'A'; server.setSecureContext(leaves[active]); }
  async function candidate(identity) {
    const config = new Uint8Array([0, 0, 32, ...Array(32).fill(7), 0, 4, 0, 1, 0, 2]);
    const channel = await m.Channel.fixture('https://localhost:18443', config, '07'.repeat(32));
    async function request(route, signal, bearer, payload) {
      const op = new m.Operation(5000, signal);
      try {
        const response = await m.pinnedRequest(new Request(origin + route, {
          method: payload === undefined ? 'GET' : 'POST',
          headers: bearer ? { authorization: 'Bearer ' + bearer } : {},
          ...(payload === undefined ? {} : { body: JSON.stringify(payload) }),
        }), op, leaves[identity].pin, () => {});
        // Tiny synthetic response only; no production buffering change.
        return new Response(await response.arrayBuffer(), { status: response.status, headers: response.headers });
      } finally { op.close(); }
    }
    channel.challenge = async signal => (await request('/v1/auth/challenge', signal)).json();
    channel.models = async (bearer, signal) => (await request('/v1/models', signal, bearer)).json();
    channel.control = async (route, payload, bearer, signal) => (await request(route, signal, bearer, payload)).json();
    channel.chat = async (payload, bearer, options) => {
      if (rotateChat) rotate();
      const response = await request('/v1/chat/completions', options.signal, bearer, payload);
      await options.onResponse?.({ status: response.status, contentType: response.headers.get('content-type') });
      return response.body;
    };
    const template = new m.ReferenceClient(channel), freshSession = template.freshSession.bind(template);
    template.freshSession = () => {
      const client = freshSession(), prepare = client.prepareChat.bind(client);
      client.prepareChat = async (...args) => { const value = await prepare(...args); prepared.push(value); return value; };
      return client;
    };
    return template;
  }
  const provider = new m.PossumsProvider(() => candidate(active), () => {}, async () => { probes++; return candidate(active); });
  provider.newSession(); await provider.auth.apiKey.login(interaction());
  const selected = provider.getModels()[0];
  return { provider, counts, prepared, candidate, rotate,
    set rotateCatalog(value) { rotateCatalog = value; }, set rotateChat(value) { rotateChat = value; },
    get probes() { return probes; }, get hooks() { return hooks; },
    invoke(signal) { return provider.streamSimple(selected, context, { signal, onPayload() { hooks++; } }).result(); },
    async close() { provider.shutdown(); server.closeAllConnections(); await new Promise(resolve => server.close(resolve)); },
  };
}
try {
  for (const stage of ['catalog', 'submission']) await check('real TLS ' + stage + ' key mismatch recovers before HTTP and dispatches once', async () => {
    const f = await fixture();
    try {
      if (stage === 'catalog') f.rotate(); else f.rotateCatalog = true;
      let probes = 0;
      f.provider.recover = async () => { probes++; f.rotateCatalog = false; return f.candidate('B'); };
      const result = await f.invoke();
      assert.equal(result.stopReason, 'stop'); assert(!result.diagnostics.some(value => value.type === 'possums_turn_warning'));
      assert.deepEqual(f.counts, { challenge: 2, sessions: 2, models: stage === 'catalog' ? 3 : 4, submissions: 1, chats: 1 });
      assert.equal(probes, 1); assert.equal(f.hooks, 1); assert.equal(f.prepared.length, 1); assert.equal(f.prepared[0].dispatched, true);
      assert.equal(f.provider.recoveryBlocked, undefined); assert(!JSON.stringify(result).includes(credential));
    } finally { await f.close(); }
  });
  await check('healthy request adds no public recovery or authentication', async () => {
    const f = await fixture();
    try { assert.equal((await f.invoke()).stopReason, 'stop'); assert.equal(f.probes, 0); assert.equal(f.counts.sessions, 1); assert.equal(f.counts.chats, 1); }
    finally { await f.close(); }
  });
  for (const failure of ['unchanged', 'verifier', 'still_mismatched']) await check('failed ' + failure + ' recovery sends no replacement credential or inference', async () => {
    const f = await fixture();
    try {
      f.rotate(); let probes = 0;
      f.provider.recover = async () => {
        probes++;
        if (failure === 'unchanged') throw new m.ConnectionFailure('evidence_unchanged');
        if (failure === 'verifier') throw new m.DiagnosticFailure('trust', 'verification');
        return f.candidate('A');
      };
      const result = await f.invoke();
      assert.equal(probes, 1); assert.equal(f.counts.sessions, 1); assert.equal(f.counts.submissions, 0); assert.equal(f.counts.chats, 0);
      assert.equal(f.prepared[0].dispatched, false); assert(result.diagnostics.some(value => value.type === 'possums_billing_unknown'));
      assert.equal(result.stopReason, 'error'); assert(f.provider.recoveryBlocked);
      assert.match(result.errorMessage, /possums_trust_peer_key_mismatch/);
      assert.match(result.errorMessage, /Setup recovery also failed/);
      assert.match(result.errorMessage, new RegExp(failure === 'unchanged' ? 'possums_evidence_unchanged' :
        failure === 'verifier' ? 'possums_trust_verification' : 'possums_trust_peer_key_mismatch'));
      f.provider.settleRun(); f.provider.beginRun();
      const before = { ...f.counts }, later = await f.invoke();
      assert.equal(later.stopReason, 'error'); assert.match(later.errorMessage, /possums_trust_peer_key_mismatch/);
      assert.deepEqual(f.counts, before); assert.equal(probes, 1); assert.equal(f.counts.chats, 0);
    } finally { await f.close(); }
  });
  await check('three-attempt scope bounds repeated TLS rotations without inference or repeated payload hook', async () => {
    const f = await fixture();
    try {
      f.rotateCatalog = true;
      const result = await f.invoke();
      assert.equal(f.probes, 3); assert.equal(f.counts.sessions, 4); assert.equal(f.counts.chats, 0); assert.equal(f.counts.submissions, 0);
      assert.equal(f.hooks, 1); assert.equal(f.prepared[0].dispatched, false); assert.equal(result.stopReason, 'error');
      assert.match(result.errorMessage, /possums_trust_peer_key_mismatch/); assert(f.provider.recoveryBlocked);
      assert.match(result.errorMessage, /Setup recovery also failed.*possums_session_unavailable/);
      f.provider.settleRun(); f.provider.beginRun(); const before = { ...f.counts };
      assert.equal((await f.invoke()).stopReason, 'error'); assert.deepEqual(f.counts, before);
      assert.equal(f.probes, 3); assert.equal(f.counts.chats, 0);
    } finally { await f.close(); }
  });
  await check('TLS mismatch after dispatch latch never recovers, transmits HTTP or replays', async () => {
    const f = await fixture();
    try {
      f.rotateChat = true;
      const result = await f.invoke();
      assert.equal(result.stopReason, 'error'); assert.match(result.errorMessage, /possums_trust_peer_key_mismatch/);
      assert.match(result.errorMessage, /underlying cause is unknown/); assert.doesNotMatch(result.errorMessage, /Observed HTTP/);
      assert.equal(f.prepared[0].dispatched, true); assert(f.provider.recoveryBlocked);
      assert.equal(f.probes, 0); assert.equal(f.counts.submissions, 1); assert.equal(f.counts.chats, 0);
      assert(result.diagnostics.some(value => value.type === 'possums_billing_unknown'));
      await f.invoke(); assert.equal(f.probes, 0); assert.equal(f.counts.submissions, 1);
    } finally { await f.close(); }
  });
  for (const action of ['abort', 'logout', 'newSession']) await check(action + ' during peer recovery prevents late credential/prompt transmission', async () => {
    const f = await fixture(), entered = deferred(), release = deferred(), controller = new AbortController();
    try {
      f.rotate(); f.provider.beginRun(); f.provider.bindRun(controller.signal);
      f.provider.recover = async () => { entered.resolve(); await release.promise; return f.candidate('B'); };
      const pending = f.invoke(controller.signal); await entered.promise;
      if (action === 'abort') controller.abort(); else f.provider[action]();
      release.resolve(); await pending;
      // Let the synthetic verifier finish too; its obsolete candidate is inert.
      await new Promise(resolve => setImmediate(resolve));
      assert.equal(f.counts.sessions, 1); assert.equal(f.counts.submissions, 0); assert.equal(f.counts.chats, 0);
      assert.equal(f.provider.recoveryBlocked, undefined, 'obsolete recovery must not latch a replacement session');
    } finally { release.resolve(); await f.close(); }
  });
  for (const action of ['logout', 'newSession']) await check(action + ' during rejected recovery propagation cannot relatch replacement trust', async () => {
    // Sweep the short rejection chain, including replacement after the verifier
    // rejected but before the request owner's outer catch receives that error.
    for (let depth = 0; depth <= 6; depth++) {
      const f = await fixture(), changed = deferred();
      try {
        f.rotate();
        f.provider.recover = async () => {
          const schedule = remaining => queueMicrotask(() => {
            if (remaining) schedule(remaining - 1);
            else { f.provider[action](); changed.resolve(); }
          });
          schedule(depth);
          throw new m.DiagnosticFailure('trust', 'verification');
        };
        const pending = f.invoke(); await changed.promise; await pending;
        await new Promise(resolve => setImmediate(resolve));
        assert.equal(f.provider.recoveryBlocked, undefined, action + ' rejection depth ' + depth);
        assert.equal(f.counts.sessions, 1); assert.equal(f.counts.submissions, 0); assert.equal(f.counts.chats, 0);
      } finally { await f.close(); }
    }
  });
  for (const action of ['logout', 'newSession']) await check(action + ' before a nonrecoverable TLS failure cannot relatch replacement trust', async () => {
    const f = await fixture();
    try {
      f.provider.client.channel.models = async () => {
        queueMicrotask(() => f.provider[action]());
        throw new m.DiagnosticFailure('trust', 'certificate');
      };
      await f.invoke();
      assert.equal(f.provider.recoveryBlocked, undefined);
      assert.equal(f.counts.sessions, 1); assert.equal(f.counts.submissions, 0); assert.equal(f.counts.chats, 0);
    } finally { await f.close(); }
  });
  await check('concurrent peer mismatch waiters coalesce verification and keep one surviving request alive', async () => {
    const f = await fixture(), entered = deferred(), release = deferred(), canceled = new AbortController();
    try {
      f.rotate(); let probes = 0;
      f.provider.recover = async () => { probes++; entered.resolve(); await release.promise; return f.candidate('B'); };
      const first = f.invoke(canceled.signal); await entered.promise;
      const second = f.invoke();
      // Both requests join the same recovery owner before the canceled waiter leaves.
      const deadline = performance.now() + 5000;
      while (f.provider.runScope.waiters < 2) {
        assert(performance.now() < deadline, 'second recovery waiter did not join');
        await new Promise(resolve => setImmediate(resolve));
      }
      canceled.abort(); release.resolve();
      await first; assert.equal((await second).stopReason, 'stop');
      assert.equal(probes, 1); assert.equal(f.counts.sessions, 2); assert.equal(f.counts.chats, 1); assert.equal(f.hooks, 2);
    } finally { release.resolve(); await f.close(); }
  });
} finally { globalThis.fetch = fetch; }
console.log(JSON.stringify({ suite: 'pi-peer-key-recovery', passed, publicNetwork: 0, inference: 0, scope: 'real loopback TLS and actual provider/reference dispatch ownership; synthetic verification only' }));
