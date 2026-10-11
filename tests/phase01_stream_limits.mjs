// Offline scratch bundles only; lazy synthetic frames, real published EHBP crypto.
import assert from 'node:assert/strict';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
const build = process.env.PHASE01_BUILD, install = process.env.PHASE02_CLIENT_INSTALL;
assert.ok(build && install, 'scratch build/install required');
const m = await import(pathToFileURL(path.join(build, 'fixture-node.mjs')));
const ehbp = await import(pathToFileURL(path.join(install, 'node_modules/ehbp/dist/esm/index.js')));
const enc = new TextEncoder();
const event = fields => enc.encode('data: ' + JSON.stringify({ object: 'chat.completion.chunk', model: 'fixture', ...fields }) + '\n\n');
const delta = content => event({ choices: [{ index: 0, delta: { content }, finish_reason: null }] });
const role = event({ choices: [{ index: 0, delta: { role: 'assistant' }, finish_reason: null }] });
const finish = event({ choices: [{ index: 0, delta: {}, finish_reason: 'stop' }] });
const receipt = event({ choices: [], usage: { prompt_tokens: 2, completion_tokens: 3, total_tokens: 5 },
  possums: { outcome: 'settled', charged_microunits: '7', refunded_microunits: '45' } });
const done = enc.encode('data: [DONE]\n\n');
const text = 'x'.repeat(1024), repeated = delta(text), repetitions = 65537;
assert.ok(repeated.length * repetitions > 64 * 1024 * 1024);
function* long() { yield role; for (let i = 0; i < repetitions; i++) yield repeated; yield finish; yield receipt; yield done; }
function lazy(parts, failEOF = false) {
  const iterator = parts[Symbol.iterator]();
  return new ReadableStream({ pull(c) {
    const next = iterator.next();
    if (!next.done) c.enqueue(next.value);
    else if (failEOF) c.error(new Error('synthetic EOF failure'));
    else c.close();
  } }, { highWaterMark: 0 });
}
let displayed = 0;
assert.equal((await m.consumeCompletion(lazy(long()), 'fixture', value => { displayed += value.length; })).finish, 'stop');
assert.equal(displayed, text.length * repetitions);

const server = await ehbp.Identity.generate();
const channel = await m.Channel.fixture('https://localhost:18443', await server.marshalConfig(), await server.getPublicKeyHex());
const chat = { model: 'fixture', stream: true, submission: 's'.repeat(43), messages: [{ role: 'user', content: 'synthetic' }] };
const originalFetch = globalThis.fetch;
let parts, mode, failEOF = false, emittedFrames = 0, encryptedBytes = 0, longFrames, longBytes;
globalThis.fetch = async req => {
  assert.equal(new URL(req.url).origin, 'https://localhost:18443');
  const encapsulated = ehbp.hexToBytes(req.headers.get('Ehbp-Encapsulated-Key'));
  const recipient = await server.suite.SetupRecipient(server.getPrivateKey(), encapsulated, { info: enc.encode(ehbp.HPKE_REQUEST_INFO) });
  // Authenticate the synthetic request with the same HPKE context.
  await recipient.Open(new Uint8Array(await req.arrayBuffer()).slice(4));
  const nonce = crypto.getRandomValues(new Uint8Array(32));
  const secret = new Uint8Array(await recipient.Export(enc.encode(ehbp.EXPORT_LABEL), ehbp.EXPORT_LENGTH));
  const keys = await ehbp.deriveResponseKeys(secret, encapsulated, nonce);
  const iterator = parts[Symbol.iterator]();
  let seq = 0, frame, offset = 0;
  return new Response(new ReadableStream({ async pull(c) {
    if (!frame || offset === frame.length) {
      const next = iterator.next();
      if (next.done) { if (failEOF) c.error(new Error('synthetic EOF failure')); else c.close(); return; }
      if (mode === 'raw') frame = next.value;
      else {
        const cipher = await ehbp.encryptChunk(keys, seq++, next.value);
        if (mode === 'corrupt') cipher[cipher.length - 1] ^= 1;
        frame = new Uint8Array(cipher.length + 4);
        new DataView(frame.buffer).setUint32(0, cipher.length, false); frame.set(cipher, 4);
        emittedFrames++; encryptedBytes += frame.length;
      }
      offset = 0;
    }
    const end = Math.min(frame.length, offset + (mode === 'large-chunk' ? 65537 : m.LIMITS.chunk));
    c.enqueue(frame.slice(offset, end)); offset = end;
  } }, { highWaterMark: 0 }), { headers: { 'Ehbp-Response-Nonce': ehbp.bytesToHex(nonce), 'Content-Type': 'text/event-stream' } });
};
const consume = async () => m.consumeCompletion(await channel.chat(chat, 'b'.repeat(43)), 'fixture', () => {});
const negative = async (expected, action = consume) => assert.rejects(action, error =>
  error.code === 'uncertain' && error.constraint === expected);
try {
  parts = long();
  assert.equal((await consume()).chargedMicrounits, '7');
  assert.ok(emittedFrames > 65536 && encryptedBytes > 64 * 1024 * 1024);
  longFrames = emittedFrames; longBytes = encryptedBytes;
  for (const [values, expected] of [
    [[role, repeated], 'finish_missing'], [[role, finish, done], 'usage_missing'],
    [[role, finish, receipt], 'done_missing'], [[role, finish, receipt, done, repeated], 'envelope'],
    [[enc.encode('data: {hostile}\n\n')], 'json'], [[new Uint8Array([255])], 'utf8'],
    [[enc.encode('data: ' + 'x'.repeat(m.LIMITS.sseEvent) + '\n\n')], 'envelope'],
  ]) { parts = values; await negative(expected); }
  parts = [role, finish, receipt, done]; failEOF = true; await negative('fetch'); failEOF = false;
  parts = [role, finish, receipt, done]; mode = 'corrupt'; await negative('decryption');
  mode = 'raw';
  for (const raw of [new Uint8Array(), new Uint8Array([0, 0]), new Uint8Array([0, 0, 0, 16, 1]),
    new Uint8Array([0, 0, 0, 0]), new Uint8Array([0, 4, 0, 1])]) {
    parts = raw.length ? [raw] : []; await negative('frames');
  }
  // Single ciphertext frame and HTTP chunk ceilings still precede SDK allocation.
  mode = undefined; parts = [new Uint8Array(m.LIMITS.frame)]; await negative('frames');
  mode = 'large-chunk'; parts = [new Uint8Array(65536)]; await negative('frames');
  await negative('envelope', () => m.consumeCompletion(lazy([new Uint8Array(m.LIMITS.frame + 1)]), 'fixture', () => {}));
  await negative('eof', () => m.consumeCompletion(lazy([role, finish, receipt, done], true), 'fixture', () => {}));
} finally { globalThis.fetch = originalFetch; }
console.log(JSON.stringify({ suite: 'phase01_stream_limits', lazyEvents: repetitions + 4, emittedFrames: longFrames, encryptedBytes: longBytes,
  cumulativeBytesAndCounts: 'passed', retainedCeilingsAndTerminalFailures: 'passed' }));
