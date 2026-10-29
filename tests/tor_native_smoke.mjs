// Clean glibc/musl userspace launch check. Synthetic loopback only, no Tor,
// openssl/compiler dependency, credentials, public DNS, attestation or inference.
import assert from 'node:assert/strict';
import test from 'node:test';
import net from 'node:net';
import dns from 'node:dns';
import { once } from 'node:events';
import { existsSync } from 'node:fs';
import { syncBuiltinESMExports } from 'node:module';
import { createTorNativeTransport } from '../clients/pi/tor-native-transport.mjs';

assert.equal(process.versions.node, '24.13.0', 'use the pinned fixture runtime');
const helperPath = process.env.POSSUMS_TOR_NATIVE_TEST_HELPER;
assert.ok(helperPath?.startsWith('/'), 'an explicit prebuilt helper is required; this fixture never builds');
if (process.env.POSSUMS_TOR_NATIVE_TEST_ARCH) {
  assert.equal(process.platform, 'linux');
  assert.equal(process.arch, process.env.POSSUMS_TOR_NATIVE_TEST_ARCH);
  assert.equal(existsSync('/nix'), false, 'the clean root must not contain Nix');
  const libc = process.env.POSSUMS_TOR_NATIVE_TEST_LIBC;
  assert.ok(['glibc', 'musl'].includes(libc));
  const paths = {
    arm64: { glibc: '/lib/aarch64-linux-gnu/libc.so.6', musl: '/lib/ld-musl-aarch64.so.1' },
    x64: { glibc: '/lib/x86_64-linux-gnu/libc.so.6', musl: '/lib/ld-musl-x86_64.so.1' },
  };
  assert.ok(existsSync(paths[process.arch]?.[libc] ?? '/missing-fixture-libc'), 'the selected userspace libc must exist');
}
const lookup = dns.lookup;
dns.lookup = function(host, ...args) {
  // Node calls lookup even for a numeric listener; this uses no DNS query.
  assert.equal(host, '127.0.0.1', 'synthetic smoke must not resolve names');
  return lookup.call(this, host, ...args);
};
syncBuiltinESMExports();

test('prebuilt native helper completes a local SOCKS round trip', { timeout: 15000 }, async () => {
  const sockets = new Set();
  const destination = 'destination.fixture.invalid';
  const server = net.createServer(socket => {
    sockets.add(socket);
    socket.on('error', () => {});
    socket.on('close', () => sockets.delete(socket));
    let buffer = Buffer.alloc(0), state = 'method';
    socket.on('data', chunk => {
      buffer = Buffer.concat([buffer, chunk]);
      if (state === 'method') {
        if (buffer.length < 2 || buffer.length < 2 + buffer[1]) return;
        assert.equal(buffer[0], 5);
        assert.ok(buffer.subarray(2, 2 + buffer[1]).includes(0));
        buffer = buffer.subarray(2 + buffer[1]);
        state = 'connect';
        socket.write(Buffer.from([5, 0]));
      }
      if (state === 'connect') {
        if (buffer.length < 5 || buffer.length < 7 + buffer[4]) return;
        assert.deepEqual([...buffer.subarray(0, 4)], [5, 1, 0, 3]);
        assert.equal(buffer.subarray(5, 5 + buffer[4]).toString(), destination);
        assert.equal(buffer.readUInt16BE(5 + buffer[4]), 443);
        buffer = buffer.subarray(7 + buffer[4]);
        state = 'echo';
        socket.write(Buffer.from([5, 0, 0, 1, 127, 0, 0, 1, 0, 1]));
      }
      if (state === 'echo' && buffer.length) {
        socket.write(buffer);
        buffer = Buffer.alloc(0);
      }
    });
  });
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  const address = server.address();
  assert.ok(address && typeof address !== 'string');
  const scope = createTorNativeTransport({ helperPath, host: '127.0.0.1', port: address.port, timeoutMs: 2500 });
  try {
    const stream = await scope.connectSocket({ host: destination, port: 443 });
    const reply = once(stream, 'data');
    stream.write('synthetic-clean-userspace');
    assert.equal((await reply)[0].toString(), 'synthetic-clean-userspace');
  } finally {
    await scope.close();
    for (const socket of sockets) socket.destroy();
    await new Promise(resolve => server.close(resolve));
  }
  console.log(JSON.stringify({ suite: 'tor-native-clean-smoke', node: process.versions.node,
    platform: process.platform, arch: process.arch, roundTrip: true, network: 'synthetic-loopback-only' }));
});
