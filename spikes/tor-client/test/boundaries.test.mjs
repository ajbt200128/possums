import test from 'node:test';
import assert from 'node:assert/strict';
import { TorClient, diagnostic, setupConnectionTest } from '../tor-client.mjs';
import { config, fixturePolicy, networkFixture, externalRuntime } from './fixtures.mjs';

// Tests intentionally expose SDK/fixture limits rather than claiming full Tor
// qualification from a synthetic success. All sockets remain numeric loopback.
test('truncated SOCKS denial cannot fabricate backend rejection; deadline stays operational', async t => {
  const fixture = await networkFixture({ deny: true }); t.after(() => fixture.close());
  const client = new TorClient(config(fixture.proxyPort, { timeoutMs: 80 }), {
    runtime: externalRuntime(), fixturePolicy,
    verifyBackend: async transport => {
      await transport.acquire(new Request(`http://gateway.invalid:${fixture.port}/evidence`));
      return { verified: true };
    },
  });
  await assert.rejects(setupConnectionTest(client), error => {
    assert.equal(diagnostic(error).code, 'possums_tor_timeout');
    assert.equal(diagnostic(error).stage, 'socks_negotiation');
    assert.equal(diagnostic(error).disposition, 'graceful_stop');
    return true;
  });
  assert.equal(fixture.destinations, 0);
});

test('default request policy rejects plaintext fixture URLs before opening SOCKS', async t => {
  const fixture = await networkFixture(); t.after(() => fixture.close());
  const client = new TorClient(config(fixture.proxyPort), {
    runtime: externalRuntime(),
    verifyBackend: async transport => {
      await transport.acquire(new Request(`http://gateway.invalid:${fixture.port}/evidence`));
      return { verified: true };
    },
  });
  await assert.rejects(client.prepareConnection(), error => diagnostic(error).code === 'possums_tor_config');
  assert.equal(fixture.authentications, 0); assert.equal(fixture.destinations, 0);
});

test('unqualified verifier result does not authorize subsequent transport', async () => {
  for (const result of [undefined, false, { verified: false }]) {
    const client = new TorClient(config(9050), { runtime: externalRuntime(), verifyBackend: async () => result });
    await assert.rejects(client.prepareConnection(), error => {
      assert.equal(diagnostic(error).code, 'possums_tor_backend_trust');
      assert.equal(diagnostic(error).disposition, 'block_backend');
      return true;
    });
  }
});

test('known dependency network debugging is rejected without changing host settings', () => {
  const previous = process.env.NODE_DEBUG;
  process.env.NODE_DEBUG = 'undici:socks5-proxy';
  try {
    assert.throws(() => new TorClient(config(9050), { runtime: externalRuntime(), verifyBackend: async () => ({ verified: true }) }),
      error => diagnostic(error).code === 'possums_tor_config');
  } finally {
    if (previous === undefined) delete process.env.NODE_DEBUG;
    else process.env.NODE_DEBUG = previous;
  }
});
