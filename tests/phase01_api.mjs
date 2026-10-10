// Actual reviewed EHBP/shim -> Rust API -> synthetic inference, no live accounts.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';
import path from 'node:path';
const { PHASE01_BUILD: build, PHASE01_FIXTURE_DIR: dir } = process.env;
assert.ok(build && dir);
const meta = JSON.parse(await readFile(path.join(dir, 'fixture.json'), 'utf8'));
const module = await import(pathToFileURL(path.join(build, 'fixture-node.mjs')));
const config = new Uint8Array(Buffer.from(meta.config, 'hex'));
const channel = await module.Channel.fixture(meta.origin, config, meta.key);
const client = new module.ReferenceClient(channel);
await client.login(Buffer.alloc(32, 8).toString('base64url'));
const models = await client.models();
assert.equal(models.length, 2);
assert.ok(models.some(model => model.id === 'kimi-k2.5' && BigInt(model.maximum_reservation_microunits) > 5_000_000n));
const beforeBalance = await client.balance();
assert.deepEqual(beforeBalance, { availableMicrounits: '5000000', inFlight: 0, completedRequests: '0' });
let progress = false;
let release;
const released = new Promise(resolve => { release = resolve; });
const completion = client.chat('org/model:v1', [{ role: 'user', content: 'synthetic API qualification' }], delta => {
  assert.ok(delta.length > 0);
  progress = true;
  fetch(meta.origin + '/fixture/release', { redirect: 'error', credentials: 'omit' }).then(() => release());
});
await released;
const receipt = await completion;
assert.ok(progress);
assert.equal(receipt.finish, 'stop');
assert.equal(receipt.chargedMicrounits, '7');
assert.equal(receipt.totalTokens, 5);
const afterBalance = await client.balance();
assert.equal(afterBalance.inFlight, 0);
assert.equal(BigInt(beforeBalance.availableMicrounits) - BigInt(afterBalance.availableMicrounits), BigInt(receipt.chargedMicrounits));
assert.equal(BigInt(afterBalance.completedRequests) - BigInt(beforeBalance.completedRequests), 1n);
const stats = async () => (await fetch(meta.origin + '/fixture/stats', { redirect: 'error', credentials: 'omit' })).json();
let observed = await stats();
assert.equal(observed.balance, '4999993');
assert.equal(observed.generations, 1);
assert.equal(observed.tokenizations, 1);
// A deliberately new conversation/model selection may reject for maximum credit,
// never substitute a cheaper model, cap output or transmit a tokenizer prompt.
await assert.rejects(() => client.chat('kimi-k2.5', [{ role: 'user', content: 'synthetic credit qualification' }], () => {}, true), error => error.reason === 'insufficient_credit');
observed = await stats();
assert.equal(observed.generations, 1);
assert.equal(observed.tokenizations, 1);
assert.equal(observed.balance, '4999993');
assert.deepEqual(await client.balance(), afterBalance);
if (process.env.PHASE01_BROWSER) {
  const { createHash, X509Certificate } = await import('node:crypto');
  const { chromium } = await import('../examples/phase01/node_modules/playwright/index.mjs');
  const certificate = new X509Certificate(await readFile(path.join(dir, 'ca.pem')));
  const pin = createHash('sha256').update(certificate.publicKey.export({ type: 'spki', format: 'der' })).digest('base64');
  const browser = await chromium.launch({ executablePath: process.env.PHASE01_BROWSER, headless: true, args: ['--ignore-certificate-errors-spki-list=' + pin] });
  try {
    const context = await browser.newContext({ serviceWorkers: 'block' });
    await context.grantPermissions(['local-network-access'], { origin: 'https://phase01.invalid' });
    const source = await readFile(path.join(build, 'fixture-browser.mjs'), 'utf8');
    await context.route('https://phase01.invalid/**', route => route.fulfill({ contentType: route.request().url().endsWith('.mjs') ? 'text/javascript' : 'text/html', body: route.request().url().endsWith('.mjs') ? source : '<!doctype html><pre id=answer></pre>' }));
    const page = await context.newPage(); await page.goto('https://phase01.invalid/');
    const result = await page.evaluate(async meta => {
      const m = await import('/fixture.mjs');
      const channel = await m.Channel.fixture(meta.origin, Uint8Array.from(meta.config.match(/../g), v => parseInt(v, 16)), meta.key);
      const client = new m.ReferenceClient(channel);
      await client.login(btoa(String.fromCharCode(...new Uint8Array(32).fill(8))).replaceAll('+', '-').replaceAll('/', '_').replaceAll('=', ''));
      const models = await client.models();
      let progress = false;
      const receipt = await client.chat('org/model:v1', [{ role: 'user', content: 'synthetic browser qualification' }], delta => {
        progress = true;
        document.getElementById('answer').append(document.createTextNode(delta));
        void fetch(meta.origin + '/fixture/release', { credentials: 'omit', redirect: 'error' });
      });
      return { models: models.length, progress, finish: receipt.finish, charge: receipt.chargedMicrounits,
        safeRendering: document.querySelector('script') === null && document.getElementById('answer').textContent.includes('<script>'),
        storage: localStorage.length + sessionStorage.length };
    }, meta);
    assert.deepEqual(result, { models: 2, progress: true, finish: 'stop', charge: '7', safeRendering: true, storage: 0 });
  } finally { await browser.close(); }
}
await fetch(meta.origin + '/fixture/stop', { redirect: 'error', credentials: 'omit' });
console.log(JSON.stringify({ api: 'pass', actualShim: true, protectedProgress: true, noCreditPromptCalls: true, source: 'synthetic', physicalAndroid: 'not tested' }));
