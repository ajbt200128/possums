import assert from 'node:assert/strict';
import { pathToFileURL } from 'node:url';
import path from 'node:path';
const build = process.env.PHASE01_BUILD;
assert.ok(build, 'scratch bundle directory required');
const moduleURL = pathToFileURL(path.join(build, 'channel-node.mjs')).href;

// The same streaming/catalog cases execute in Node and real Chromium.
export async function clientCases(moduleURL) {
  const m = await import(moduleURL);
  let count = 0;
  const check = ok => { if (!ok) throw new Error('client contract check'); count++; };
  const requestFailure = new m.BalanceFailure('request');
  const validationFailure = new m.BalanceFailure('validation');
  check(requestFailure instanceof m.BalanceFailure && requestFailure.code === 'rejected' && requestFailure.message === 'possums_balance_request_failed');
  check(validationFailure.code === 'rejected' && validationFailure.message === 'possums_balance_validation_failed');
  const model = (id, context = '20', price = '1000000', quote = '52') => ({ object: 'model', id,
    context_tokens: context, max_output_tokens: context, input_microunits_per_million_tokens: price,
    output_microunits_per_million_tokens: price, maximum_reservation_microunits: quote });
  const catalog = { object: 'list', data: [model('org/model:v1'), model('kimi-k2.5', '262144', '15000000', '10223616')] };
  check(m.validateModels(structuredClone(catalog)).length === 2);
  for (const mutate of [
    c => { c.data.push(c.data[0]); }, c => { c.data = []; }, c => { c.data[0].context_tokens = '0'; },
    c => { c.data[0].maximum_reservation_microunits = '51'; }, c => { c.data[0].context_tokens = '9007199254740993x'; },
    c => { c.data[0].input_microunits_per_million_tokens = '0'; }, c => { c.data[0].tools = true; },
  ]) {
    let rejected = false;
    try { const c = structuredClone(catalog); mutate(c); m.validateModels(c); } catch { rejected = true; }
    check(rejected);
  }
  const event = fields => 'data: ' + JSON.stringify({ object: 'chat.completion.chunk', model: 'org/model:v1', ...fields }) + '\n\n';
  const role = event({ choices: [{ index: 0, delta: { role: 'assistant' }, finish_reason: null }] });
  const delta = event({ choices: [{ index: 0, delta: { content: 'synthetic🐾<script>' }, finish_reason: null }] });
  const finish = event({ choices: [{ index: 0, delta: {}, finish_reason: 'length' }] });
  const usage = event({ choices: [], usage: { prompt_tokens: 2, completion_tokens: 3, total_tokens: 5 },
    possums: { outcome: 'settled', charged_microunits: '7', refunded_microunits: '45' } });
  const done = 'data: [DONE]\n\n';
  const valid = role + delta + finish + usage + done;
  function stream(text, size) {
    const bytes = typeof text === 'string' ? new TextEncoder().encode(text) : text;
    let offset = 0;
    return new ReadableStream({ pull(c) { if (offset === bytes.length) c.close(); else { c.enqueue(bytes.slice(offset, offset + size)); offset += Math.min(size, bytes.length - offset); } } }, { highWaterMark: 0 });
  }
  for (const size of [1, 7, 4096]) {
    let text = '';
    const receipt = await m.consumeCompletion(stream(valid, size), 'org/model:v1', value => { text += value; });
    check(text === 'synthetic🐾<script>' && receipt.finish === 'length' && receipt.totalTokens === 5 && receipt.chargedMicrounits === '7');
  }
  const setTimer = globalThis.setTimeout;
  const deadlines = [];
  globalThis.setTimeout = (callback, ms, ...args) => {
    deadlines.push(ms);
    return setTimer(callback, ms, ...args);
  };
  try { await m.consumeCompletion(stream(valid, 4096), 'org/model:v1', () => {}); }
  finally { globalThis.setTimeout = setTimer; }
  check(m.LIMITS.idleMs === 600000 && deadlines.includes(m.LIMITS.idleMs) &&
    deadlines.every(ms => Number.isFinite(ms) && ms <= m.LIMITS.idleMs));
  // Heartbeats and incomplete SSE lines are read activity, not model events.
  globalThis.setTimeout = (fn, ms, ...args) => setTimer(fn,
    ms === 600000 ? 80 : ms === 300000 ? 40 : ms, ...args);
  try {
    let part = 0;
    const fragments = [': heartbeat', '\n\n', ...Array(8).fill(': progress\n\n'), valid];
    const progressing = new ReadableStream({ async pull(c) {
      await new Promise(resolve => setTimer(resolve, 20));
      if (part === fragments.length) c.close(); else c.enqueue(new TextEncoder().encode(fragments[part++]));
    } }, { highWaterMark: 0 });
    check((await m.consumeCompletion(progressing, 'org/model:v1', () => {})).totalTokens === 5);
    let closed = 0, rejected = false;
    try { await m.consumeCompletion(new ReadableStream({ cancel() { closed++; } }), 'org/model:v1', () => {}); }
    catch (error) { rejected = error.code === 'uncertain'; }
    check(rejected && closed === 1);
  } finally { globalThis.setTimeout = setTimer; }
  for (const bad of [
    delta + finish + usage + done, role + usage + done, role + delta, role + delta + finish + done,
    role + finish + usage, role + finish + usage + done + done, role + finish + usage + done + delta,
    role + finish + usage.replace('"total_tokens":5', '"total_tokens":6') + done,
    role + finish + usage.replace('"charged_microunits":"7"', '"charged_microunits":"7","charged_microunits":"0"') + done,
    valid.replaceAll('org/model:v1', 'different'), valid.replace('"length"', '"tool_calls"'),
    valid.replace('"prompt_tokens":2', '"prompt_tokens":9007199254740993'),
    role + 'data: {"error":{"code":"generation_failed"}}\n\n',
    new Uint8Array([0xff]), 'data: ' + 'x'.repeat(m.LIMITS.sseEvent + 1),
  ]) {
    let rejected = false;
    try { await m.consumeCompletion(stream(bad, 7), 'org/model:v1', () => {}); }
    catch (error) { rejected = error.code === 'uncertain'; }
    check(rejected);
  }
  let cancelled = 0;
  const broken = new ReadableStream({ start(c) { c.enqueue(new TextEncoder().encode(role + delta)); }, cancel() { cancelled++; } });
  try { await m.consumeCompletion(broken, 'org/model:v1', () => { throw new Error('private callback'); }); } catch (error) { check(error.code === 'uncertain'); }
  check(cancelled === 1);
  const network = globalThis.fetch;
  let sends = 0;
  globalThis.fetch = () => { sends++; throw new Error('unexpected network'); };
  try {
    let rejected = false;
    try { await m.ReferenceClient.verified(new Uint8Array(), new Uint8Array(), new Uint8Array()); } catch { rejected = true; }
    check(rejected && sends === 0);
    rejected = false;
    try { new m.ReferenceClient({ challenge: () => fetch('https://unexpected.invalid') }); } catch { rejected = true; }
    check(rejected && sends === 0);
  } finally { globalThis.fetch = network; }
  return count;
}

console.log(JSON.stringify({ runtime: 'node', checks: await clientCases(moduleURL) }));
if (process.env.PHASE01_BROWSER) {
  const { createServer } = await import('node:http');
  const { readFile } = await import('node:fs/promises');
  const { chromium } = await import('../examples/phase01/node_modules/playwright/index.mjs');
  const source = await readFile(path.join(build, 'channel-browser.mjs'));
  const server = createServer((req, res) => { res.setHeader('Cache-Control', 'no-store'); res.setHeader('Content-Type', req.url === '/bundle.mjs' ? 'text/javascript' : 'text/html'); res.end(req.url === '/bundle.mjs' ? source : '<!doctype html><title>Transport spike</title>'); });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const origin = 'http://127.0.0.1:' + server.address().port;
  const browser = await chromium.launch({ executablePath: process.env.PHASE01_BROWSER, headless: true });
  try {
    const page = await browser.newPage();
    await page.goto(origin);
    const checks = await page.evaluate('(' + clientCases.toString() + ')(' + JSON.stringify(origin + '/bundle.mjs') + ')');
    console.log(JSON.stringify({ runtime: 'chromium', checks, physicalAndroid: 'not tested' }));
  } finally { await browser.close(); await new Promise(resolve => server.close(resolve)); }
}
