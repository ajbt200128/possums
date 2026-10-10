// Synthetic, offline diagnostics checks. Called by the phase02_pi.mjs harness with
// its fixture-capable build; no production credential or network is used.
import assert from 'node:assert/strict';

export async function diagnosticChecks(m, check) {
  const secret = 'PRIVATE_PROMPT_CREDENTIAL';
  const token = 'a'.repeat(43);
  const model = { object: 'model', id: 'synthetic', context_tokens: '20', max_output_tokens: '20',
    input_microunits_per_million_tokens: '1000000', output_microunits_per_million_tokens: '1000000',
    maximum_reservation_microunits: '52' };
  const role = { object: 'chat.completion.chunk', model: 'synthetic', choices: [{ index: 0, delta: { role: 'assistant' }, finish_reason: null }] };
  const finish = { object: 'chat.completion.chunk', model: 'synthetic', choices: [{ index: 0, delta: {}, finish_reason: 'stop' }] };
  const usage = { object: 'chat.completion.chunk', model: 'synthetic', choices: [],
    usage: { prompt_tokens: 2, completion_tokens: 3, total_tokens: 5 },
    possums: { outcome: 'settled', charged_microunits: '7', refunded_microunits: '45' } };
  const frame = value => `data: ${typeof value === 'string' ? value : JSON.stringify(value)}\n\n`;
  function stream(parts, fail = false) {
    return new ReadableStream({ start(controller) {
      for (const part of parts) controller.enqueue(typeof part === 'string' ? new TextEncoder().encode(part) : part);
      if (fail) controller.error(new Error(secret)); else controller.close();
    } });
  }
  async function rejected(parts, stage, constraint, options = {}, fail = false, onDelta = () => {}) {
    await assert.rejects(m.consumeCompletion(stream(parts, fail), 'synthetic', onDelta, options), error => {
      assert(error instanceof m.DiagnosticFailure);
      assert.equal(error.stage, stage); assert.equal(error.constraint, constraint);
      assert.equal(error.code, 'uncertain'); assert(!error.message.includes(secret));
      assert.equal(error.cause, undefined); return true;
    });
  }
  await check('diagnostics distinguish missing authenticated stream terminals', async () => {
    await rejected([frame(role)], 'stream', 'finish_missing');
    await rejected([frame(role), frame(finish)], 'stream', 'usage_missing');
    await rejected([frame(role), frame(finish), frame(usage)], 'stream', 'done_missing');
    await rejected([frame(role), frame(finish), frame('[DONE]')], 'stream', 'usage_missing');
  });
  await check('diagnostics distinguish hostile stream encoding, JSON, choice, tool and receipt', async () => {
    await rejected([new Uint8Array([0xff])], 'stream', 'utf8');
    await rejected([frame(`{${secret}`)], 'stream', 'json');
    await rejected([frame({ ...role, choices: [{ index: 1, delta: { role: 'assistant' }, finish_reason: null }] })], 'stream', 'choice');
    await rejected([frame(role), frame({ ...role, choices: [{ index: 0, delta: { tool_calls: [{ index: 0 }] }, finish_reason: null }] })], 'stream', 'tool');
    await rejected([frame(role), frame(finish), frame({ ...usage, possums: { ...usage.possums, charged_microunits: secret } })], 'stream', 'receipt');
    await rejected([frame({ ...role, model: secret })], 'stream', 'schema');
  });
  await check('read and hook failures stay closed, interrupted delivery does not confirm refund', async () => {
    await rejected([], 'transport', 'fetch', {}, true);
    const text = { ...role, choices: [{ index: 0, delta: { content: 'synthetic' }, finish_reason: null }] };
    await rejected([frame(role), frame(text)], 'hook', 'unexpected', {}, false, () => { throw new Error(secret); });
    await rejected([frame(role)], 'hook', 'unexpected', { onEvent: () => { throw new Error(secret); } });
    const abort = new AbortController(); abort.abort();
    await rejected([frame({ error: { code: 'generation_failed', billing: 'refunded', message: secret } })],
      'stream', 'interrupted', { signal: abort.signal });
    let pulled = false;
    const partial = new ReadableStream({ pull(controller) {
      if (!pulled) { pulled = true; controller.enqueue(new TextEncoder().encode(frame({ error: { code: 'generation_failed', billing: 'refunded', message: secret } }))); }
      else controller.error(new Error(secret));
    } }, { highWaterMark: 0 });
    await assert.rejects(m.consumeCompletion(partial, 'synthetic', () => {}), error => {
      assert(error instanceof m.GatewayError); assert.equal(error.billing, 'unknown');
      assert(!error.message.includes(secret)); return true;
    });
    await assert.rejects(m.consumeCompletion(stream([frame({ error: { code: 'generation_failed', billing: 'refunded', message: secret } })]), 'synthetic', () => {}), error => {
      assert(error instanceof m.GatewayError); assert.equal(error.billing, 'refunded'); return true;
    });
  });
  await check('local operations distinguish deadline, idle and parent interruption without changing limits', async () => {
    for (const constraint of ['idle', 'deadline', 'interrupted']) {
      const parent = new AbortController();
      const op = new m.Operation(constraint === 'deadline' ? 1 : 1000, parent.signal);
      const pending = op.wait(new Promise(() => {}), constraint === 'idle' ? 1 : 1000);
      if (constraint === 'interrupted') parent.abort(new Error(secret));
      try { await assert.rejects(pending, error => error instanceof m.OperationFailure && error.constraint === constraint); }
      finally { op.close(); }
    }
    const parent = new m.Operation(1), child = new m.Operation(1000, parent.controller.signal);
    try { await assert.rejects(child.wait(new Promise(() => {}), 1000), error => error instanceof m.OperationFailure && error.constraint === 'deadline'); }
    finally { child.close(); parent.close(); }
  });
  await check('missing authenticated EOF, invalid finish/usage/delta and closed carriers remain distinct', async () => {
    await rejected([frame(role), frame({ ...finish, choices: [{ ...finish.choices[0], finish_reason: secret }] })], 'stream', 'finish');
    await rejected([frame(role), frame(finish), frame({ ...usage, usage: { prompt_tokens: 2, completion_tokens: 3, total_tokens: 0 } })], 'stream', 'usage');
    await rejected([frame(role), frame({ ...role, choices: [{ index: 0, delta: { content: { value: secret } }, finish_reason: null }] })], 'stream', 'delta');
    let pulled = false;
    const body = new ReadableStream({ pull(controller) {
      if (!pulled) { pulled = true; controller.enqueue(new TextEncoder().encode([role, finish, usage, '[DONE]'].map(frame).join(''))); }
      else controller.error(new Error(secret));
    } }, { highWaterMark: 0 });
    await assert.rejects(m.consumeCompletion(body, 'synthetic', () => {}), error => error instanceof m.DiagnosticFailure && error.stage === 'stream' && error.constraint === 'eof');
    assert.throws(() => new m.DiagnosticFailure(secret, 'unexpected'));
    assert.throws(() => new m.DiagnosticFailure('provider', secret));
    const failure = new m.DiagnosticFailure('provider', 'unexpected', 'uncertain', secret);
    assert(Object.isFrozen(failure)); assert.equal(failure.status, undefined); assert(!failure.message.includes(secret));
  });
  await check('challenge HTTP versus credential rejection and transport failures stay content-free', async () => {
    const originalFetch = globalThis.fetch;
    const key = new Uint8Array([0, 0, 32, ...Array(32).fill(7), 0, 4, 0, 1, 0, 2]);
    const channel = await m.Channel.fixture('https://localhost:18443', key, '07'.repeat(32));
    try {
      globalThis.fetch = async () => new Response(secret, { status: 401 });
      await assert.rejects(channel.challenge(), error => error instanceof m.DiagnosticFailure && error.stage === 'challenge' && error.constraint === 'http' && error.status === 401 && !error.message.includes(secret));
      // An unbound encrypted response is not fabricated as credential expiry.
      await assert.rejects(channel.control('/v1/sessions', { challenge: token, credential: token }), error => error instanceof m.DiagnosticFailure && error.stage === 'authentication' && error.constraint === 'endpoint_binding' && error.status === 401);
      globalThis.fetch = async () => { throw new Error(secret); };
      await assert.rejects(channel.control('/v1/sessions', { challenge: token, credential: token }), error => error instanceof m.DiagnosticFailure && error.stage === 'authentication' && error.constraint === 'fetch' && error.status === undefined);
      globalThis.fetch = async () => Response.json({ challenge: token, expires_in: 600 });
      const client = new m.ReferenceClient(channel);
      channel.control = async () => { throw new m.DiagnosticFailure('authentication', 'http', 'rejected', 401); };
      await assert.rejects(client.login(token), error => error instanceof m.DiagnosticFailure && error.stage === 'authentication' && error.status === 401);
    } finally { globalThis.fetch = originalFetch; }
  });
  await check('quiescing wire shape is exact, not a refund, and stream errors need HTTP admission evidence', async () => {
    const wire = { error: { code: 'service_quiescing', stage: 'admission', constraint: 'service_quiescing',
      billing: 'not_submitted', message: secret } };
    const observation = new m.CatalogFailure('http', 'rejected', 503, wire);
    assert.equal(observation.reason, 'service_quiescing'); assert.equal(observation.status, 503);
    assert.equal(observation.detail, undefined); assert.equal(observation.billing, undefined);
    assert(!JSON.stringify(observation).includes(secret));
    for (const altered of [
      { ...wire, error: { ...wire.error, stage: 'stream' } },
      { ...wire, error: { ...wire.error, constraint: 'unknown' } },
      { ...wire, error: { ...wire.error, billing: 'refunded' } },
      { ...wire, error: { ...wire.error, detail: secret } },
      { ...wire, error: { ...wire.error, message: null } },
    ]) assert.equal(new m.CatalogFailure('http', 'rejected', 503, altered).reason, undefined);
    assert.equal(new m.CatalogFailure('http', 'rejected', 502, wire).reason, undefined);
    const fabricated = new m.GatewayError('service_quiescing', undefined, 'refunded', 503);
    assert.equal(fabricated.billing, 'unknown'); assert(!fabricated.message.includes('Reservation refunded'));
    await rejected([JSON.stringify(wire)], 'stream', 'json');
    await rejected([frame(wire)], 'stream', 'schema');
    await assert.rejects(m.consumeCompletion(stream([JSON.stringify(wire)], true), 'synthetic', () => {}, {}, 503), error =>
      !(error instanceof m.GatewayError) && error.billing !== 'refunded' && !String(error).includes(secret));
    for (const status of [undefined, 200, 502, 503])
      await assert.rejects(m.consumeCompletion(stream([JSON.stringify(wire)]), 'synthetic', () => {}, {}, status), error =>
        error instanceof m.DiagnosticFailure && error.billing !== 'refunded' && !String(error).includes(secret));
  });
  await check('gateway status is bounded to an observed HTTP status', async () => {
    assert.equal(new m.GatewayError('unavailable', undefined, 'unknown', 503).status, 503);
    for (const status of [0, 600, 503.5, Number.NaN, secret])
      assert.equal(new m.GatewayError('unavailable', undefined, 'unknown', status).status, undefined);
  });
  await check('login diagnostics and chat stages preserve closed failures without reissuing inference', async () => {
    const key = new Uint8Array([0, 0, 32, ...Array(32).fill(7), 0, 4, 0, 1, 0, 2]);
    const channel = await m.Channel.fixture('https://localhost:18443', key, '07'.repeat(32));
    const client = new m.ReferenceClient(channel);
    async function failure(run, stage, constraint, code) {
      await assert.rejects(run(), error => {
        assert(error instanceof m.DiagnosticFailure); assert.equal(error.stage, stage);
        assert.equal(error.constraint, constraint); assert.equal(error.code, code);
        assert(!error.message.includes(secret)); assert.equal(error.cause, undefined); return true;
      });
    }
    channel.challenge = async () => { throw new Error(secret); };
    await failure(() => client.login(secret), 'challenge', 'fetch', 'rejected');
    channel.challenge = async () => ({ challenge: token, expires_in: 0 });
    await failure(() => client.login(secret), 'challenge', 'schema', 'rejected');
    channel.challenge = async () => ({ challenge: token, expires_in: 600 });
    channel.control = async () => { throw new m.OperationFailure('deadline'); };
    await failure(() => client.login(secret), 'authentication', 'deadline', 'uncertain');
    channel.control = async path => path === '/v1/sessions' ? { token, token_type: 'Bearer', expires_in: 43200 } : { submission: token };
    await client.login(secret);
    let submissions = 0, sends = 0;
    channel.control = async path => { if (path === '/v1/submissions') submissions++; return { submission: token }; };
    channel.chat = async (_payload, _bearer, options) => {
      sends++; await options.onResponse?.({ status: 200, contentType: 'text/event-stream' });
      return stream([frame(role), frame(finish), frame(usage), frame('[DONE]')]);
    };
    channel.models = async () => { throw new m.CatalogFailure('http', 'rejected', 503); };
    await assert.rejects(client.chat('synthetic', [{ role: 'user', content: 'synthetic' }], () => {}), error =>
      error instanceof m.CatalogFailure && error.stage === 'http' && error.status === 503);
    channel.models = async () => ({ object: 'list', data: [model] });
    await failure(() => client.chat('synthetic', [{ role: 'user', content: 'synthetic' }], () => {}, false,
      { onPayload: () => { throw new Error(secret); } }), 'hook', 'unexpected', 'rejected');
    channel.control = async () => { submissions++; throw new Error(secret); };
    await failure(() => client.chat('synthetic', [{ role: 'user', content: 'synthetic' }], () => {}), 'submission', 'fetch', 'uncertain');
    assert.equal(submissions, 1); assert.equal(sends, 0);
    channel.control = async () => { submissions++; return { submission: token }; };
    await failure(() => client.chat('synthetic', [{ role: 'user', content: 'synthetic' }], () => {}, false,
      { onResponse: () => { throw new Error(secret); } }), 'hook', 'unexpected', 'uncertain');
    assert.equal(sends, 1);
    await failure(() => client.chat('synthetic', [{ role: 'user', content: 'synthetic' }], () => {}, false,
      { onEvent: () => { throw new Error(secret); } }), 'hook', 'unexpected', 'uncertain');
    await failure(() => client.chat('synthetic', [{ role: 'user', content: 'synthetic' }], () => {}, false,
      { onPayload: 12 }), 'request', 'schema', 'rejected');
  });
}
