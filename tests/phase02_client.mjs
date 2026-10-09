// Offline only. Build/copy dependencies in the approved scratch directory first.
import assert from 'node:assert/strict';
import { pathToFileURL } from 'node:url';
import path from 'node:path';
const build = process.env.PHASE02_CLIENT_BUILD ?? process.env.PHASE01_BUILD;
const install = process.env.PHASE02_CLIENT_INSTALL;
assert.ok(build && install, 'scratch build and install required');
const m = await import(pathToFileURL(path.join(build, 'channel-node.mjs')));
const f = await import(pathToFileURL(path.join(build, 'fixture-node.mjs')));
const ehbp = await import(pathToFileURL(path.join(install, 'node_modules/ehbp/dist/esm/index.js')));
let checks = 0;
const check = value => { assert.ok(value); checks++; };
const bad = action => { assert.throws(action); checks++; };
const rejects = async (action, code = 'uncertain') => { await assert.rejects(action, e => e.code === code); checks++; };
const enc = new TextEncoder(), dec = new TextDecoder();
const model = { object: 'model', id: 'fixture', context_tokens: '20', max_output_tokens: '20',
  input_microunits_per_million_tokens: '1000000', output_microunits_per_million_tokens: '1000000',
  maximum_reservation_microunits: '52', tool_protocol: 'openai-functions-v1' };
const tool = { type: 'function', function: { name: 'lookup', description: 'Synthetic fixture',
  parameters: { type: 'object', properties: { n: { type: 'number', minimum: -1.5, maximum: 10 } }, additionalProperties: false } } };
const call = (id = 'call_1', name = 'lookup', args = '{"n":1}') => ({ id, type: 'function', function: { name, arguments: args } });
const chat = { model: 'fixture', stream: true, submission: 's'.repeat(43), messages: [{ role: 'user', content: 'hello' }], tools: [tool] };
const batch = [{ role: 'user', content: 'hello' }, { role: 'assistant', content: null, tool_calls: [call()] }, { role: 'tool', tool_call_id: 'call_1', content: 'result' }];
const encoded = value => JSON.parse(dec.decode(m.encodeChat(value)));
check(encoded(chat).tools[0].function.parameters.properties.n.minimum === -1.5);
for (const choice of ['auto', 'none', 'required', { type: 'function', function: { name: 'lookup' } }]) check(encoded({ ...chat, tool_choice: choice }).tool_choice !== undefined);
let nestedParameters={type:'object',properties:{value:{type:'string',enum:['synthetic']}}};
for(let i=0;i<6;i++)nestedParameters={type:'object',properties:{nested:nestedParameters}};
const nestedTool={...tool,function:{...tool.function,parameters:nestedParameters}};
assert.deepEqual(encoded({...chat,tools:[nestedTool]}).tools[0].function.parameters,nestedParameters);checks++;
check(encoded({ ...chat, messages: batch }).messages.length === 3);
check(encoded({ ...chat, messages: [...batch, { role: 'user', content: 'next' }] }).messages.length === 4);
for (const mutate of [
  c => { c.tools = []; }, c => { c.tools = [tool, tool]; }, c => { c.tools = Array(65).fill(tool); },
  c => { c.tools[0].function.name = 'n'.repeat(65); }, c => { c.tools[0].function.name = 'bad name'; },
  c => { c.tools[0].function.parameters.type = 'array'; }, c => { c.tools[0].function.parameters.$ref = 'https://invalid.example/schema'; },
  c => { c.tools[0].function.parameters.$ref = 'relative.json#/x'; }, c => { c.tools[0].function.parameters.$id = 'https://invalid.example/schema'; },
  c => { c.tool_choice = 'grammar'; }, c => { c.tool_choice = { type: 'function', function: { name: 'unknown' } }; },
  c => { c.messages = []; }, c => { c.messages[0].content = ''; }, c => { c.messages[0].content = ' \n'; },
  c => { c.messages[0].content = [{ type: 'text', text: 'no multimodal' }]; }, c => { c.messages[0].role = 'developer'; },
  c => { c.messages = [{ role: 'assistant', content: 'unfinished' }]; }, c => { c.messages = [{ role: 'assistant', content: null }]; },
  c => { c.messages = [{ role: 'tool', tool_call_id: 'orphan', content: 'bad' }]; },
  c => { c.temperature = 1; }, c => { c.max_tokens = 1; }, c => { c.response_format = {}; },
]) { const value = structuredClone(chat); mutate(value); bad(() => encoded(value)); }
for (const mutate of [
  c => { c.messages.pop(); }, c => { c.messages[2].tool_call_id = 'unknown'; }, c => { c.messages.push(c.messages[2]); },
  c => { c.messages.splice(2, 0, { role: 'user', content: 'interruption' }); },
  c => { c.messages[1].tool_calls.push(call()); }, c => { c.messages[1].tool_calls = []; },
  c => { c.messages[1].tool_calls[0].id = 'i'.repeat(129); }, c => { c.messages[1].tool_calls[0].function.name = 'unknown'; },
  c => { c.messages[1].tool_calls[0].function.arguments = '[]'; }, c => { c.messages[1].tool_calls[0].function.arguments = '{"x":1,"x":2}'; },
  c => { c.messages[1].tool_calls[0].function.arguments = '{"x":1e999}'; },
]) { const value = structuredClone({ ...chat, messages: batch }); mutate(value); bad(() => encoded(value)); }
const fullArgs = '{"x":"' + 'a'.repeat(m.LIMITS.toolArguments - 8) + '"}';
check(enc.encode(fullArgs).length === m.LIMITS.toolArguments);
const history = calls => [{ role: 'assistant', content: null, tool_calls: calls }, ...calls.map(c => ({ role: 'tool', tool_call_id: c.id, content: '' }))];
check(encoded({ ...chat, messages: history([call('a', 'lookup', fullArgs)]) }).messages.length === 2);
bad(() => encoded({ ...chat, messages: history([call('a', 'lookup', fullArgs + ' ')]) }));
check(encoded({ ...chat, messages: history(Array.from({ length: 4 }, (_, i) => call('c' + i, 'lookup', fullArgs))) }).messages.length === 5);
bad(() => encoded({ ...chat, messages: history(Array.from({ length: 5 }, (_, i) => call('c' + i, 'lookup', fullArgs))) }));
check(encoded({ ...chat, messages: history(Array.from({ length: 64 }, (_, i) => call('c' + i))) }).messages.length === 65);
bad(() => encoded({ ...chat, messages: history(Array.from({ length: 65 }, (_, i) => call('c' + i))) }));
let getters = 0;
for (const value of [
  { ...chat, tools: [Object.defineProperty({ ...tool }, 'function', { get() { getters++; return tool.function; } })] },
  { ...chat, tools: [{ ...tool, function: { ...tool.function, parameters: Object.defineProperty({ type: 'object' }, 'x', { enumerable: true, get() { getters++; return 1; } }) } }] },
  { ...chat, messages: new Array(1) }, { ...chat, tools: new Array(1) },
  { ...chat, messages: [new (class { role = 'user'; content = 'hello'; })()] },
  { ...chat, tools: Object.assign([tool], { extra: true }) },
]) bad(() => encoded(value));
check(getters === 0);
const marker = Object.defineProperty({ type: 'object' }, '~kind', { value: 'Object' });
check(!Object.hasOwn(encoded({ ...chat, tools: [{ ...tool, function: { ...tool.function, parameters: marker } }] }).tools[0].function.parameters, '~kind'));
for (const descriptor of [{ get() { getters++; return 'Object'; } }, { value: { toJSON() { getters++; return 'Object'; } } }, { value: 'Object', enumerable: true }]) {
  const parameters = Object.defineProperty({ type: 'object' }, '~kind', descriptor);
  if (descriptor.enumerable) continue; // Enumerable annotations are ordinary JSON, not the inert TypeBox marker.
  bad(() => encoded({ ...chat, tools: [{ ...tool, function: { ...tool.function, parameters } }] }));
}
check(getters === 0);
for (const n of [-0, 0, 1, -1.5, 1e-7, 1e308]) {
  const value = { n, b: true, s: '\ud800😀\n' }, size = enc.encode(JSON.stringify(value)).length;
  check(m.serialize(value, size).length === size); bad(() => m.serialize(value, size - 1));
}
for (const n of [NaN, Infinity, -Infinity, 1n]) bad(() => m.serialize({ n }, 100));
check(m.validateModels({ object: 'list', data: [model] })[0].tool_protocol === 'openai-functions-v1');
for (const protocol of [null, true, 'openai-functions-v2', '']) bad(() => m.validateModels({ object: 'list', data: [{ ...model, tool_protocol: protocol }] }));

const event = fields => 'data: ' + JSON.stringify({ object: 'chat.completion.chunk', model: 'fixture', ...fields }) + '\n\n';
const delta = value => event({ choices: [{ index: 0, delta: value, finish_reason: null }] });
const role = delta({ role: 'assistant' });
const end = reason => event({ choices: [{ index: 0, delta: {}, finish_reason: reason }] });
const rates = { quoted_input_microunits_per_million_tokens: '1000000', quoted_output_microunits_per_million_tokens: '1000000' };
const usage = (extra = {}) => event({ choices: [], usage: { prompt_tokens: 2, completion_tokens: 3, total_tokens: 5 },
  possums: { outcome: 'settled', charged_microunits: '7', refunded_microunits: '45', ...extra } });
const done = 'data: [DONE]\n\n';
const failure = error => 'data: ' + JSON.stringify({ error }) + '\n\n';
const initial = (index = 0, id = 'call_1', args = '{"n":') => ({ index, ...call(id, 'lookup', args) });
const continuation = args => ({ index: 0, function: { arguments: args } });
const toolDeltas = delta({ tool_calls: [initial()] }) + delta({ tool_calls: [continuation('1}')] });
const valid = role + toolDeltas + end('tool_calls') + usage(rates) + done;
const stopCalls = role + toolDeltas + end('stop') + usage(rates) + done;
const partialLength = role + delta({ tool_calls: [initial()] }) + end('length') + usage(rates) + done;
function stream(text, size = 1024) {
  const bytes = enc.encode(text); let offset = 0;
  return new ReadableStream({ pull(c) { if (offset === bytes.length) c.close(); else { c.enqueue(bytes.slice(offset, offset + size)); offset = Math.min(offset + size, bytes.length); } } }, { highWaterMark: 0 });
}
const consume = (text, options = {}, entry = model) => m.consumeCompletion(stream(text), entry, () => {}, { tools: [tool], ...options });
for (const size of [1, 7, 4096]) {
  let events = 0;
  const receipt = await m.consumeCompletion(stream(valid, size), model, () => assert.fail('no text'), { tools: [tool], onEvent: async value => {
    check(Object.isFrozen(value) && Object.isFrozen(value.choices)); events++;
    if (value.choices[0]) check(Object.isFrozen(value.choices[0].delta));
  } });
  check(receipt.finish === 'tool_calls' && receipt.quotedInputMicrounitsPerMillion === '1000000' && receipt.chargedMicrounits === '7');
  check(events === 5);
}
check((await consume(stopCalls)).finish === 'stop');
check((await consume(partialLength)).finish === 'length');
for (const text of [partialLength.slice(0, -done.length), role + delta({ tool_calls: [initial()] }) + end('length') + done,
  role + delta({ tool_calls: [initial()] }) + end('length') + usage(rates)]) await rejects(() => consume(text));
const old = await consume(role + end('stop') + usage() + done, { tools: undefined }, 'fixture');
check(old.finish === 'stop' && !Object.hasOwn(old, 'quotedInputMicrounitsPerMillion'));
for (const text of [
  role + end('tool_calls') + usage() + done,
  role + delta({ tool_calls: [initial()] }) + end('stop') + usage() + done,
  role + delta({ tool_calls: [initial(0, 'call_1', '[]')] }) + end('stop') + usage() + done,
  role + delta({ tool_calls: [initial(1)] }) + end('tool_calls') + usage() + done,
  role + delta({ tool_calls: [initial(), initial(0, 'another')] }),
  role + delta({ tool_calls: [initial(), initial(1)] }),
  role + delta({ tool_calls: [{ index: 0, function: { arguments: '{}' } }] }),
  role + delta({ tool_calls: [{ ...initial(), type: 'custom' }] }),
  role + delta({ tool_calls: [{ ...initial(), function: { name: 'unknown', arguments: '{}' } }] }),
  role + delta({ tool_calls: [initial()] }) + delta({ tool_calls: [initial()] }),
  role + delta({ tool_calls: [initial(0, 'call_1', '[]')] }) + end('tool_calls') + usage() + done,
  role + delta({ tool_calls: [initial(0, 'call_1', '{"n":1,"n":2}')] }) + end('tool_calls') + usage() + done,
  valid.slice(0, -done.length), valid + delta({ content: 'late' }), valid.replace('"settled"', '"pending"'),
]) await rejects(() => consume(text));
await rejects(() => consume(valid, {}, 'fixture'));
const textOnly = { ...model }; delete textOnly.tool_protocol;
await rejects(() => consume(valid, {}, textOnly));
await rejects(() => consume(valid, { tools: undefined }));
function argumentStream(count, argumentsText) {
  let text = role;
  for (let index = 0; index < count; index++) {
    text += delta({ tool_calls: [initial(index, 'id_' + index, argumentsText.slice(0, 32000))] });
    for (let offset = 32000; offset < argumentsText.length; offset += 32000) text += delta({ tool_calls: [{ index, function: { arguments: argumentsText.slice(offset, offset + 32000) } }] });
  }
  return text + end('tool_calls') + usage() + done;
}
check((await consume(argumentStream(4, fullArgs))).finish === 'tool_calls');
await rejects(() => consume(argumentStream(5, fullArgs)));
await rejects(() => consume(argumentStream(5, fullArgs).replace(end('tool_calls'), end('length'))));
await rejects(() => consume(argumentStream(1, fullArgs + ' ')));
await rejects(() => consume(argumentStream(1, fullArgs + ' ').replace(end('tool_calls'), end('length'))));
check((await consume(argumentStream(64, '{}'))).finish === 'tool_calls');
await rejects(() => consume(argumentStream(65, '{}')));
const unicodeArgs = delta({ tool_calls: [initial(0, 'unicode', '{"x":"\ud83d')] }) + delta({ tool_calls: [continuation('\ude00"}')] });
check((await consume(role + unicodeArgs + end('tool_calls') + usage() + done)).finish === 'tool_calls');
let delivered = 0;
await rejects(() => consume(role + delta({ tool_calls: [initial(), initial(1, 'call_1')] }), { onEvent() { delivered++; } }));
check(delivered === 1); // role only: invalid multi-delta event is atomic to observers.
for (const extra of [
  { ...rates, charged_microunits: '6' }, { quoted_input_microunits_per_million_tokens: '1' },
  { quoted_output_microunits_per_million_tokens: '1' }, { ...rates, quoted_input_microunits_per_million_tokens: '0' },
  { ...rates, quoted_input_microunits_per_million_tokens: '01' }, { ...rates, quoted_output_microunits_per_million_tokens: '18446744073709551616' },
]) await rejects(() => consume(role + end('stop') + usage(extra) + done));
const rounded = await consume(role + end('stop') + usage({ ...rates, quoted_input_microunits_per_million_tokens: '1', quoted_output_microunits_per_million_tokens: '1', charged_microunits: '1' }) + done);
check(rounded.chargedMicrounits === '1');
let eofController, settled = false;
const delayedEOF = new ReadableStream({ start(c) { eofController = c; c.enqueue(enc.encode(valid)); } });
const beforeEOF = m.consumeCompletion(delayedEOF, model, () => {}, { tools: [tool] }).then(value => { settled = true; return value; });
await new Promise(r => setTimeout(r, 10)); check(!settled); eofController.close(); await beforeEOF; check(settled);
const newError = { code: 'generation_failed', detail: 'stream_idle_timeout', message: 'SECRET_PROMPT_AND_KEY', billing: 'refunded' };
let errorEvents = 0, errorDeltas = 0;
let legitimate;
try { await m.consumeCompletion(stream(role + failure(newError)), model, () => { errorDeltas++; },
  { tools: [tool], onEvent(value) {
    assert(!Object.hasOwn(value, 'error'));
    assert(!JSON.stringify(value).includes('SECRET_PROMPT_AND_KEY'));
    errorEvents++;
  } }); assert.fail('expected gateway error'); }
catch (error) { legitimate = error; }
check(legitimate.reason === 'generation_failed' && legitimate.detail === 'stream_idle_timeout' &&
  legitimate.billing === 'refunded' && /Generation failed.*stream idle timeout.*Reservation refunded/.test(legitimate.message) &&
  !String(legitimate).includes('SECRET_PROMPT_AND_KEY'));
check(errorEvents === 1 && errorDeltas === 0); // Only the initial role reaches the hook.
await assert.rejects(() => consume(JSON.stringify({ error: newError })), error =>
  error.reason === 'generation_failed' && error.billing === 'refunded' &&
  /Reservation refunded/.test(error.message) && !error.message.includes('SECRET_PROMPT_AND_KEY')); checks++;
const fabricated = new legitimate.constructor('generation_failed', 'stream_idle_timeout', 'refunded');
check(fabricated.billing === 'unknown' && /Generation failed.*Charge unknown/.test(fabricated.message) &&
  !fabricated.message.includes('SECRET_PROMPT_AND_KEY'));
const hostileConstructor = new legitimate.constructor('possums_private_key', 'possums_private_prompt', 'refunded');
check(hostileConstructor.billing === 'unknown' && /Gateway request failed.*Charge unknown/.test(hostileConstructor.message) &&
  !hostileConstructor.message.includes('possums_private'));
for (const injected of [fabricated, legitimate]) {
  await assert.rejects(() => m.consumeCompletion(stream(role + delta({ content: 'synthetic' })), model, () => {},
    { tools: [tool], onEvent() { throw injected; } }), error => error.code === 'uncertain' && error.billing !== 'refunded'); checks++;
}
let errorController, errorSettled = false;
const errorBody = new ReadableStream({ start(c) { errorController = c; c.enqueue(enc.encode(failure(newError))); } });
const pendingError = m.consumeCompletion(errorBody, model, () => {}, { tools: [tool] }).catch(error => { errorSettled = true; return error; });
await new Promise(r => setTimeout(r, 10)); check(!errorSettled);
errorController.error(new Error('post-error transport SECRET_PROMPT_AND_KEY'));
const incompleteError = await pendingError;
check(incompleteError.reason === 'generation_failed' && incompleteError.billing === 'unknown' &&
  /Generation failed.*Charge unknown/.test(incompleteError.message) && !String(incompleteError).includes('SECRET_PROMPT_AND_KEY'));
for (const extra of [delta({ tool_calls: [initial()] }), JSON.stringify({ error: newError })]) {
  await assert.rejects(() => consume(failure(newError) + extra), error =>
    error.reason === 'generation_failed' && error.billing === 'unknown'); checks++;
}
for (const hostile of [
  { ...newError, detail: 'possums_secret_key' }, { ...newError, code: 'possums_secret_key' },
  { ...newError, billing: 'possums_secret_key' },
]) await rejects(() => consume(failure(hostile)));
await assert.rejects(() => consume(failure({ code: 'unavailable' })), error =>
  error.reason === 'unavailable' && error.billing === 'unknown'); checks++;
let cancelled = 0, pulls = 0, entered;
const callbackEntered = new Promise(resolve => { entered = resolve; });
const abort = new AbortController();
const stalled = new ReadableStream({ pull(c) { pulls++; c.enqueue(enc.encode(role)); }, cancel() { cancelled++; return new Promise(() => {}); } }, { highWaterMark: 0 });
const waiting = m.consumeCompletion(stalled, model, () => {}, { signal: abort.signal, tools: [tool], onEvent() { entered(); return new Promise(() => {}); } });
await callbackEntered; check(pulls === 1); abort.abort(); await rejects(() => waiting); check(cancelled === 1);

// Real local HPKE on both sides; the fetch replacement never opens a socket.
const server = await ehbp.Identity.generate();
const config = await server.marshalConfig();
const channel = await f.Channel.fixture('https://localhost:18443', config, await server.getPublicKeyHex());
const client = new f.ReferenceClient(channel);
const nativeFetch = globalThis.fetch;
let sent = [], decoded = [], responsePulls = 0, holdPath, held, catalogModel = model, chatText = valid, corrupt = false;
let observedAbort = false;
globalThis.fetch = async req => {
  check(req instanceof Request && req.credentials === 'omit' && req.redirect === 'error' && req.cache === 'no-store');
  check(!req.signal.aborted);
  const route = new URL(req.url).pathname; sent.push(route);
  if (route === holdPath) {
    held?.();
    return new Promise((_, reject) => req.signal.addEventListener('abort', () => { observedAbort = true; reject(new Error('fixture abort')); }, { once: true }));
  }
  if (route === '/v1/auth/challenge') return Response.json({ challenge: 'c'.repeat(43), expires_in: 600 });
  if (route === '/v1/models') return Response.json({ object: 'list', data: [catalogModel] });
  const encapsulated = ehbp.hexToBytes(req.headers.get('Ehbp-Encapsulated-Key'));
  const recipient = await server.suite.SetupRecipient(server.getPrivateKey(), encapsulated, { info: enc.encode(ehbp.HPKE_REQUEST_INFO) });
  const body = new Uint8Array(await req.arrayBuffer());
  const payload = JSON.parse(dec.decode(await recipient.Open(body.slice(4)))); decoded.push({ route, payload });
  const response = route === '/v1/sessions' ? JSON.stringify({ token: 'b'.repeat(43), token_type: 'Bearer', expires_in: 43200 }) :
    route === '/v1/submissions' ? JSON.stringify({ submission: 's'.repeat(43) }) : chatText;
  const nonce = crypto.getRandomValues(new Uint8Array(32));
  const secret = new Uint8Array(await recipient.Export(enc.encode(ehbp.EXPORT_LABEL), ehbp.EXPORT_LENGTH));
  const keys = await ehbp.deriveResponseKeys(secret, encapsulated, nonce);
  const cipher = await ehbp.encryptChunk(keys, 0, enc.encode(response));
  if (corrupt && route === '/v1/chat/completions') cipher[cipher.length - 1] ^= 1;
  const frame = new Uint8Array(4 + cipher.length); new DataView(frame.buffer).setUint32(0, cipher.length, false); frame.set(cipher, 4);
  let emitted = false;
  responsePulls = 0;
  return new Response(new ReadableStream({ pull(c) { responsePulls++; if (!emitted) { emitted = true; c.enqueue(frame); } else c.close(); } }, { highWaterMark: 0 }),
    { headers: { 'Ehbp-Response-Nonce': ehbp.bytesToHex(nonce), 'Content-Type': route === '/v1/chat/completions' ? 'text/event-stream; fixture=hidden' : 'application/json', 'X-Private': 'not-for-hook' } });
};
try {
  const pre = AbortSignal.abort();
  await rejects(() => client.login('c'.repeat(43), pre), 'rejected'); check(sent.length === 0);
  await client.login('c'.repeat(43)); check(sent.join(',') === '/v1/auth/challenge,/v1/sessions');
  let initial = sent.length;
  await rejects(() => client.models(pre), 'rejected'); check(sent.length === initial);
  await rejects(() => client.chat('fixture', chat.messages, () => {}, false, { signal: pre }), 'rejected'); check(sent.length === initial);
  check((await client.chat('fixture',chat.messages,()=>{},false,{tools:[nestedTool]})).finish==='tool_calls');
  assert.deepEqual(decoded.at(-1).payload.tools[0].function.parameters,nestedParameters);checks++;
  let tooDeep=nestedParameters;
  for(let i=0;i<8;i++)tooDeep={type:'object',properties:{nested:tooDeep}};
  initial=sent.length;
  await assert.rejects(()=>client.chat('fixture',chat.messages,()=>{},false,{
    tools:[{...tool,function:{...tool.function,parameters:tooDeep}}],
  }),error=>error.code==='rejected' && error.message==='possums_request_json_depth');checks++;
  check(sent.length===initial);
  let payloadHook = false, responseHook = false, eventHook = false;
  const receipt = await client.chat('fixture', chat.messages, () => {}, false, { tools: [tool],
    onPayload(payload) {
      payloadHook = true; check(sent.length === initial); check(Object.isFrozen(payload) && Object.isFrozen(payload.messages));
      check(!('submission' in payload) && !('bearer' in payload));
      return { ...payload, messages: [{ role: 'user', content: 'hook replacement' }] };
    },
    onResponse(info) { responseHook = true; check(payloadHook && !eventHook); check(responsePulls === 0); check(Object.isFrozen(info)); assert.deepEqual(info, { status: 200, contentType: 'text/event-stream' }); },
    async onEvent(value) { eventHook = true; check(responseHook && Object.isFrozen(value)); },
  });
  check(receipt.finish === 'tool_calls' && payloadHook && responseHook && eventHook);
  check(decoded.at(-1).payload.messages[0].content === 'hook replacement');
  check(decoded.at(-1).payload.submission === 's'.repeat(43));
  chatText = stopCalls;
  check((await client.chat('fixture', chat.messages, () => {}, false, { tools: [tool] })).finish === 'stop');
  chatText = partialLength;
  check((await client.chat('fixture', chat.messages, () => {}, false, { tools: [tool] })).finish === 'length');
  chatText = role + delta({ tool_calls: [{ index: 0, ...call('call_1', 'lookup', '{"n":') }] }) + end('stop') + usage(rates) + done;
  await rejects(() => client.chat('fixture', chat.messages, () => {}, false, { tools: [tool] }));
  const other = { ...tool, function: { ...tool.function, name: 'other' } };
  chatText = role + delta({ tool_calls: [{ index: 0, ...call('call_1', 'other', '{}') }] }) + end('stop') + usage(rates) + done;
  await rejects(() => client.chat('fixture', chat.messages, () => {}, false, { tools: [tool] }));
  await rejects(() => client.chat('fixture', chat.messages, () => {}, false, { tools: [tool, other], tool_choice: { type: 'function', function: { name: 'lookup' } } }));
  await rejects(() => client.chat('fixture', chat.messages, () => {}, false, { tools: [tool, other], tool_choice: 'none' }));
  chatText = valid;
  let depthFailure;try{f.serialize({tools:[{...tool,function:{...tool.function,parameters:tooDeep}}]},f.LIMITS.chat);}catch(error){depthFailure=error;}
  check(depthFailure?.message==='possums_request_json_depth');
  for (const injected of [fabricated, legitimate, depthFailure]) {
    initial = sent.length;
    await assert.rejects(() => client.chat('fixture', chat.messages, () => {}, false,
      { tools: [tool], onPayload() { throw injected; } }), error => error.code === 'rejected' && error.billing !== 'refunded'); checks++;
    check(sent.length === initial);
    await assert.rejects(() => client.chat('fixture', chat.messages, () => {}, false,
      { tools: [tool], onEvent() { throw injected; } }), error => error.code === 'uncertain' && error.billing !== 'refunded'); checks++;
  }
  for (const replacement of [p => ({ ...p, model: 'other' }), p => ({ ...p, submission: 's'.repeat(43) }), p => ({ ...p, headers: {} }), p => ({ ...p, tools: [] })]) {
    initial = sent.length;
    await rejects(() => client.chat('fixture', chat.messages, () => {}, false, { tools: [tool], onPayload: replacement }), 'rejected');
    check(sent.length === initial);
  }
  catalogModel = textOnly; initial = sent.length;
  await rejects(() => client.chat('fixture', chat.messages, () => {}, false, { tools: [tool] }), 'rejected');
  check(sent.slice(initial).join(',') === '/v1/models'); catalogModel = model;
  initial = sent.length;
  await rejects(() => client.chat('fixture', chat.messages, () => {}, false, { tools: [tool], onResponse() { throw new Error('private hook diagnostics'); } }));
  check(sent.slice(initial).join(',') === '/v1/models,/v1/submissions,/v1/chat/completions');
  corrupt = true;
  await rejects(() => client.chat('fixture', chat.messages, () => {}, false, { tools: [tool] })); corrupt = false;
  for (const route of ['/v1/auth/challenge', '/v1/sessions', '/v1/models', '/v1/submissions', '/v1/chat/completions']) {
    const controller = new AbortController(); holdPath = route; observedAbort = false;
    const reached = new Promise(resolve => { held = resolve; });
    const operation = route === '/v1/auth/challenge' || route === '/v1/sessions' ? client.login('c'.repeat(43), controller.signal) :
      route === '/v1/models' ? client.models(controller.signal) : client.chat('fixture', chat.messages, () => {}, false, { tools: [tool], signal: controller.signal });
    // Attach immediately so no rejected fixture promise is ever unhandled.
    const rejected = assert.rejects(operation, e => e.code === 'uncertain');
    await reached; controller.abort(); await rejected; check(observedAbort);
    holdPath = undefined;
    await client.login('c'.repeat(43));
  }
  const controller = new AbortController(); let payloadEntered;
  const reached = new Promise(resolve => { payloadEntered = resolve; }); initial = sent.length;
  const waitingHook = client.chat('fixture', chat.messages, () => {}, false, { signal: controller.signal, onPayload() { payloadEntered(); return new Promise(() => {}); } });
  await reached; controller.abort(); await rejects(() => waitingHook, 'rejected'); check(sent.length === initial);
} finally { globalThis.fetch = nativeFetch; }
console.log(JSON.stringify({ runtime: 'node', checks, network: 'offline fetch fixture only', productionApprovalsAdded: 0 }));
