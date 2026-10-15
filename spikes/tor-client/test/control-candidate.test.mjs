// REJECTION evidence only. tor-ctrl is a dev-only candidate, not an adapter.
// Fixed TCP response fragments/events are transport fixtures, NOT a new Tor
// protocol parser/server. No real Tor, real cookie or external network is used.
import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:net';
import { once } from 'node:events';
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { TorControl } from 'tor-ctrl';

async function responseFixture(t, respond) {
  const sockets = new Set();
  const server = createServer(socket => {
    sockets.add(socket); socket.on('error', () => {});
    socket.once('close', () => sockets.delete(socket));
    let responses = 0;
    // Deliberately do NOT parse received commands. Each data arrival elicits
    // fixed synthetic bytes. Sufficient to refute single-chunk reply handling.
    socket.on('data', bytes => respond(socket, responses++, bytes));
  });
  server.listen(0, '127.0.0.1'); await once(server, 'listening');
  t.after(async () => {
    for (const socket of sockets) socket.destroy();
    await new Promise(resolve => server.close(resolve));
  });
  return { port: server.address().port, destroy: () => { for (const socket of sockets) socket.destroy(); } };
}

test('rejected tor-ctrl 1.0.0 resolves an incomplete fragmented bootstrap snapshot', async t => {
  const fixture = await responseFixture(t, (socket, index) => {
    if (index === 0) socket.write('250-status/bootstrap-phase=NOTICE BOOTSTRAP PROGRESS=');
    else socket.write('250 OK\r\n');
  });
  const controller = new TorControl({ host: '127.0.0.1', port: fixture.port });
  await controller.connect(); // Also sends no mandatory NULL AUTHENTICATE.
  const result = await controller.getInfo('status/bootstrap-phase');
  assert.equal(result.message, 'status/bootstrap-phase=NOTICE BOOTSTRAP PROGRESS=');
  assert.equal(controller.state, 'connected');
  assert.equal(typeof controller.on, 'undefined'); // No structured event subscription.
  await controller.disconnect();
});

test('rejected tor-ctrl treats interleaved STATUS_CLIENT event as command failure', async t => {
  const fixture = await responseFixture(t, (socket, index) => socket.write(index === 0
    ? '650 STATUS_CLIENT NOTICE BOOTSTRAP PROGRESS=100\r\n250-status/bootstrap-phase=NOTICE BOOTSTRAP PROGRESS=0\r\n250 OK\r\n'
    : '250 OK\r\n'));
  const controller = new TorControl({ host: '127.0.0.1', port: fixture.port });
  await controller.connect();
  await assert.rejects(controller.getInfo('status/bootstrap-phase'));
  // Never print/retain this raw library error; the candidate is not integrated.
  await controller.disconnect();
});

test('rejected tor-ctrl quotes hexadecimal cookie as ASCII instead of sending cookie bytes', async t => {
  const root = await mkdtemp(join(tmpdir(), 'tor-control-candidate-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const cookie = Buffer.alloc(32, 0x42); // Fixed synthetic bytes, never real Tor state.
  const cookiePath = join(root, 'synthetic-cookie');
  await writeFile(cookiePath, cookie, { mode: 0o600 });
  let outbound;
  const fixture = await responseFixture(t, (socket, index, bytes) => {
    if (index === 0) outbound = Buffer.from(bytes);
    socket.write('250 OK\r\n'); // Not acceptance: actual Tor would reject the wrong token.
  });
  const controller = new TorControl({ host: '127.0.0.1', port: fixture.port, cookiePath });
  await controller.connect();
  assert.deepEqual(outbound, Buffer.from(`AUTHENTICATE "${cookie.toString('hex')}"\r\n`));
  assert.notDeepEqual(outbound, Buffer.from(`AUTHENTICATE ${cookie.toString('hex')}\r\n`));
  outbound.fill(0); outbound = undefined; cookie.fill(0);
  await controller.disconnect();
});

test('rejected tor-ctrl pending command does not reject on peer close', async t => {
  const fixture = await responseFixture(t, socket => socket.destroy());
  const controller = new TorControl({ host: '127.0.0.1', port: fixture.port });
  await controller.connect();
  const outcome = await Promise.race([
    controller.getInfo('status/bootstrap-phase').then(() => 'resolved', () => 'rejected'),
    new Promise(resolve => setTimeout(() => resolve('still_pending'), 30)),
  ]);
  assert.equal(outcome, 'still_pending');
  fixture.destroy();
  await controller.disconnect();
});
