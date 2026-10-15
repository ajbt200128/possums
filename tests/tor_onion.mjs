// Offline public synthetic vectors emitted by the maintained Arti HsId codec.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';
import { test } from 'node:test';

const { parseTorOnionAddress, TorOnionFailure } = await import(process.env.POSSUMS_TOR_ONION_TEST_MODULE
  ? pathToFileURL(process.env.POSSUMS_TOR_ONION_TEST_MODULE).href : '../clients/pi/tor-onion.mjs');
const vectors = JSON.parse(readFileSync(process.env.POSSUMS_TOR_ONION_TEST_VECTORS, 'utf8'));
assert.equal(vectors.length, 3);
for (const vector of vectors) {
  test('matches maintained Arti synthetic address and raw identity', () => {
    const parsed = parseTorOnionAddress(vector.address);
    assert.equal(parsed.host, vector.address);
    assert.equal(parsed.identity.toString('hex'), vector.identity);
    assert.equal(parsed.transport_authorized, false);
    parsed.identity.fill(0);
    assert.equal(parseTorOnionAddress(vector.address).identity.toString('hex'), vector.identity);
    assert.ok(Object.isFrozen(parsed));
  });
}
const valid = vectors[1].address;
for (const invalid of [undefined, null, 3, {}, '', valid.toUpperCase(), `https://${valid}`,
  `${valid}:80`, `${valid}/`, `sub.${valid}`, ` ${valid}`, `${valid}\n`,
  'HOSTILE_SECRET_DO_NOT_ECHO', valid.slice(1), valid.slice(0, 55) + 'a.onion',
  (valid[0] === 'a' ? 'b' : 'a') + valid.slice(1)]) {
  test('rejects invalid address with closed non-echoing diagnostics', () => {
    assert.throws(() => parseTorOnionAddress(invalid), error => {
      assert.ok(error instanceof TorOnionFailure);
      assert.equal(error.code, 'possums_tor_onion_invalid');
      assert.equal(error.stack, undefined);
      assert.ok(!error.message.includes('HOSTILE'));
      assert.ok(!error.message.includes(valid));
      return true;
    });
  });
}
