// Isolated synthetic upstream -> production Rust server -> pinned shim/EHBP.
// Native HTTP trailers are transport-protected, outside EHBP body encryption.
import assert from 'node:assert/strict';
import https from 'node:https';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';
import path from 'node:path';
const dir = process.env.PHASE01_FIXTURE_DIR;
const meta = JSON.parse(await readFile(path.join(dir, 'fixture.json'), 'utf8'));
const require = createRequire(path.join(process.env.PHASE01_INSTALL, 'package.json'));
const { Identity } = await import(pathToFileURL(require.resolve('ehbp')));
const identity = await Identity.unmarshalPublicConfig(Uint8Array.from(Buffer.from(meta.config, 'hex')));
const controls = 'http://127.0.0.1:18445';
const stats = async () => (await fetch(controls + '/fixture/stats')).json();
let token;
async function request(route, payload, { optIn = false, streaming = false, negotiate = true, abort = false } = {}) {
  const headers = {};
  if (payload !== undefined) headers['content-type'] = 'application/json';
  if (token) headers.authorization = 'Bearer ' + token;
  if (optIn) headers['X-Tinfoil-Request-Usage-Metrics'] = 'true';
  if (streaming && negotiate) headers.te = 'trailers'; // HTTP/1.1 trailer negotiation, forwarded by Go ReverseProxy.
  const encrypted = await identity.encryptRequestWithContext(new Request(meta.origin + route, {
    method: payload === undefined ? 'GET' : 'POST', headers,
    ...(payload === undefined ? {} : { body: JSON.stringify(payload) }),
  }));
  const body = new Uint8Array(await encrypted.request.arrayBuffer());
  let aborted = false;
  let release = Promise.resolve();
  const response = await new Promise((resolve, reject) => {
    const req = https.request(encrypted.request.url, { method: encrypted.request.method, headers: Object.fromEntries(encrypted.request.headers), agent: false }, res => {
      const chunks = []; let size = 0; let progress = false;
      res.on('data', chunk => {
        size += chunk.length;
        if (size > 64 * 1024) { req.destroy(); reject(new Error('fixture_transport_bound')); return; }
        chunks.push(chunk);
        if (streaming && !progress) {
          progress = true;
          if (abort) {
            aborted = true;
            req.destroy();
            resolve({ aborted: true, trailers: res.trailers });
          } else {
            release = fetch(controls + '/fixture/release').then(r => assert.equal(r.status, 204));
          }
        }
      });
      res.on('end', () => resolve({ status: res.statusCode, headers: res.headers, trailers: res.trailers, bytes: Buffer.concat(chunks), progress }));
      res.on('error', () => { if (!aborted) reject(new Error('fixture_response_transport')); });
    });
    req.on('error', () => { if (!aborted) reject(new Error('fixture_request_transport')); });
    req.end(body);
  });
  if (response.aborted) return response;
  await release;
  assert.equal(response.status, 200);
  if (payload === undefined) {
    assert.equal(route, '/v1/auth/challenge');
    assert.equal(encrypted.context, null); // SDK-specified bodyless TLS-only response.
    return { ...response, text: response.bytes.toString('utf8') };
  }
  assert(encrypted.context);
  const decrypted = await identity.decryptResponseWithContext(new Response(response.bytes, { headers: response.headers }), encrypted.context);
  return { ...response, text: await decrypted.text() }; // Authenticated body EOF.
}
const baseline = await stats();
const challenge = JSON.parse((await request('/v1/auth/challenge')).text).challenge;
token = JSON.parse((await request('/v1/sessions', { challenge, credential: Buffer.alloc(32, 8).toString('base64url') })).text).token;
assert.equal(typeof token, 'string');
for (const { optIn, negotiate } of [{ optIn: false, negotiate: true }, { optIn: true, negotiate: false }, { optIn: true, negotiate: true }]) {
  const submission = JSON.parse((await request('/v1/submissions', { model: 'org/model:v1', new_conversation: true })).text).submission;
  const result = await request('/v1/chat/completions', { model: 'org/model:v1', stream: true, submission, messages: [{ role: 'user', content: 'synthetic native gateway qualification' }] }, { optIn, negotiate, streaming: true });
  assert(result.progress);
  assert.equal(result.headers.trailer?.toLowerCase(), optIn ? 'x-tinfoil-usage-metrics' : undefined);
  assert.equal(result.trailers['x-tinfoil-usage-metrics'], optIn && negotiate ? 'prompt=2,completion=3,total=5,model=org/model:v1,cached_prompt_tokens=0,uncached_prompt_tokens=2,cost_usd=999.00,other_cost_usd=1.20,web_search_calls=2' : undefined);
  assert(result.text.endsWith('data: [DONE]\n\n'));
  assert(!result.text.includes('private-header-marker'));
  const receipt = result.text.split('\n').filter(line => line.startsWith('data: {')).map(line => JSON.parse(line.slice(6))).find(event => event.possums)?.possums;
  assert.equal(receipt.status, 'complete');
  assert.equal(receipt.evidence, 'final_metrics');
  assert.equal(receipt.charged_microunits, '7'); // Quoted marked-up debit, NOT native provider cost.
  assert.equal(receipt.provider_cost_usd, '999.00');
}
// Opted-in disconnect never drains the held upstream to obtain a trailer.
const submission = JSON.parse((await request('/v1/submissions', { model: 'org/model:v1', new_conversation: true })).text).submission;
const aborted = await request('/v1/chat/completions', { model: 'org/model:v1', stream: true, submission, messages: [{ role: 'user', content: 'synthetic opted-in cancellation' }] }, { optIn: true, streaming: true, abort: true });
assert(aborted.aborted);
assert.equal(aborted.trailers['x-tinfoil-usage-metrics'], undefined);
const deadline = Date.now() + 5000;
let final;
do {
  final = await stats();
  if (final.cancelled === baseline.cancelled + 1 && final.in_flight === 0) break;
  assert(Date.now() < deadline, 'fixture_cancellation_deadline');
  await new Promise(resolve => setTimeout(resolve, 10));
} while (true);
assert.equal(final.generations - baseline.generations, 4);
assert.equal(final.tokenizations - baseline.tokenizations, 4);
assert.equal(final.completed - baseline.completed, 3);
assert.equal(final.completed_requests - baseline.completed_requests, 4);
assert.equal(BigInt(baseline.balance) - BigInt(final.balance), 25n); // 7 + 7 + 7 + last running snapshot 4.
await fetch(meta.origin + '/fixture/stop');
console.log('PASS production Rust -> pinned shim/EHBP native downstream trailers: absent/opt-in, allowlist, native cost separate from debit, authenticated body EOF, HTTP/1.1 TE negotiation, opted-in cancellation without drain; exactly 4 synthetic generations');
