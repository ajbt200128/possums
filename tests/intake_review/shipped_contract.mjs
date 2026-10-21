// Execute only hash-pinned shipped login/balance methods and their exact scalar
// decoders. Never import/initialize the installed provider or its network stack.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';

const artifacts = [
  ['pi-v0.0.30-forwarding-4a2a41884907', '4a2a4188490785efaac408dd7d67e9735464ff81b67cdc9b059a8f15551fab7c'],
  ['pi-graceful-warnings-5e7c451e7259', '5e7c451e7259b3778e9d35b2fafda59dd35ea9596f9dfb8a3a4404c6f04e8db3'],
];

async function review() {
  for (const [name, hash] of artifacts) {
    const source = await readFile(`/Users/mb5/.local/share/possums/pi/${name}/extension.mjs`, 'utf8');
    assert.equal(createHash('sha256').update(source).digest('hex'), hash);
    const between = (start, end) => {
      assert.equal(source.split(start).length, 2);
      const from = source.indexOf(start);
      const to = source.indexOf(end, from + start.length);
      assert(to > from);
      return source.slice(from, to);
    };
    const functions = ['requireThat', 'keys', 'amount'].map(name =>
      between(`function ${name}(`, '\n}') + '\n}').join('\n');
    const token = between('var token = ', '\n');
    const login = between('async login(credential, signal)', '\n  async models(');
    const balance = between('async balance(signal)', '\n  async chat(');
    // Only transport/deadline/error carriers are synthetic; method and decoder
    // bytes are unchanged. Scope is schema acceptance, not SDK/TLS qualification.
    const Client = new Function(`
      class ChannelError extends Error {}
      class DiagnosticFailure extends Error {}
      class CatalogFailure extends Error {}
      class GatewayError extends Error {}
      class OperationFailure extends Error {}
      class BalanceFailure extends Error {}
      class Operation { controller = new AbortController(); check() {} close() {} }
      const LIMITS = {operationMs: 1000};
      ${functions}\n${token}
      return class {
        #bearer; #authExpiresAt;
        constructor(channel) { this.channel = channel; }
        ${login}\n${balance}
      };
    `)();
    const fakeToken = 'A'.repeat(43);
    let expires = 43200;
    let snapshot = { available_microunits: '100', in_flight: 0, completed_requests: '0' };
    let calls = 0;
    const client = new Client({
      async challenge() { calls++; return { challenge: fakeToken, expires_in: 600 }; },
      async control() { calls++; return { token: fakeToken, token_type: 'Bearer', expires_in: expires }; },
      async balance() { calls++; return snapshot; },
    });
    await client.login('synthetic-only');
    assert.equal((await client.balance()).completedRequests, '0');
    expires = 3600;
    await assert.rejects(client.login('synthetic-only'));
    expires = 43200;
    await client.login('synthetic-only');
    snapshot = { available_microunits: '100', in_flight: 0 };
    await assert.rejects(client.balance());
    snapshot = { available_microunits: '100', in_flight: 0, completed_requests: null };
    await assert.rejects(client.balance());
    assert.equal(calls, 9);
    console.log(`${name}: pinned login/balance compatibility obstruction confirmed`);
  }
}
review().catch(() => {
  console.error('INTAKE_REVIEW_SHIPPED_CONTRACT_UNAVAILABLE: source/schema check did not complete; cause unknown');
  process.exitCode = 1;
});
