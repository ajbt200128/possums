// Offline synthetic host diagnostics; invoked by tor_selection.mjs.
// PRIVACY.md data handling, forbidden telemetry and typed-interface failures.
import assert from 'node:assert/strict';
import { generateKeyPairSync, sign } from 'node:crypto';

export async function torDiagnosticChecks(m, check) {
  const hostile = 'HOSTILE_SECRET_URL_ONION_PROMPT';
  await check('native categories preserve safe stage/action/outcome without fatal projection', async () => {
    for (const kind of ['configuration', 'destination', 'closed', 'cancelled', 'deadline', 'proxy', 'protocol', 'authentication',
      'unsupported', 'handshake_bound', 'truncated', 'handshake_io', 'handshake_unknown', 'tunnel', 'helper', 'early_data', 'ipc_unknown', 'bridge_unknown']) {
      const error = new m.TorNativeTransportFailure(kind);
      error.message = hostile;
      const safe = m.torNativeTransportDiagnostic(error);
      const projected = m.connectionFailure(error);
      assert.equal(projected.code, kind === 'configuration' ? 'tor_selection_invalid' : 'tor_transport_diagnostic');
      for (const value of [safe.code, safe.stage, safe.constraint, safe.action, safe.outcome]) assert(projected.message.includes(value));
      assert(!projected.message.includes(hostile)); assert.equal(projected.cause, undefined);
      assert.equal(m.catalogConnectionFailure(error).message, projected.message);
    }
    const changed = new m.TorNativeTransportFailure('proxy');
    changed.diagnostic = { code: hostile, stage: hostile, constraint: hostile, action: hostile, outcome: hostile };
    const safe = m.connectionFailure(changed);
    assert(safe.message.includes('possums_tor_native_ipc_unknown')); assert(!safe.message.includes(hostile));
    Object.defineProperty(changed, 'diagnostic', { get() { throw new Error(hostile); } });
    assert.match(m.connectionFailure(changed).message, /possums_tor_native_ipc_unknown/);
  });
  await check('mutable onion errors never provide message/code authority', async () => {
    const error = new m.TorOnionFailure();
    error.message = error.code = error.stage = error.constraint = hostile;
    const safe = m.connectionFailure(error);
    assert.equal(safe.code, 'tor_selection_invalid');
    assert(safe.message.includes('canonical_v3_address_checksum')); assert(!safe.message.includes(hostile));
  });
  await check('closed public setup diagnostics preserve unauthenticated status and real trust constraints', async () => {
    const backend = new m.TorSetupBackendFailure('acquisition', 'http_status', 503);
    const projected = m.connectionFailure(backend);
    assert.equal(projected.code, 'tor_transport_diagnostic');
    for (const value of [backend.code, backend.stage, backend.constraint, backend.action, '503', 'UNAUTHENTICATED']) assert(projected.message.includes(value));
    assert.match(projected.message, /billing are not established/);
    for (const error of [new m.TorEndorsementFailure('signature', 'rejected'), new m.TorEndorsementFailure('payload', 'onion_identity'),
      new m.TorSetupFailure('response', 'schema'), new m.TorSetupFailure('verification', 'cause_unknown'),
      new m.TorSetupBackendFailure('grant', 'derived_mounted_appraised_A_equality'), new m.VerifierFailure('publisher', 'identity')]) {
      assert.equal(m.connectionFailure(error).code, 'verification_failed');
      assert(m.connectionFailure(error).message.includes(error.code));
      assert(Object.isFrozen(error));
    }
    for (const error of [new m.TorSetupFailure(hostile, hostile), new m.TorEndorsementFailure(hostile, hostile),
      new m.TorSetupBackendFailure(hostile, hostile, hostile)]) {
      const safe = m.connectionFailure(error); assert(!safe.message.includes(hostile)); assert.match(safe.message, /cause.*unknown/);
    }
    assert.equal(m.connectionFailure(new m.TorEndorsementFailure('authorization', 'interrupted')).code, 'tor_transport_diagnostic');
    assert.match(m.connectionFailure(new Error(hostile)).message, /Stage: trust; constraint: unexpected/);
    assert.match(m.connectionFailure(new Error(hostile)).message, /cause is unknown/);
    assert.match(m.catalogConnectionFailure(new Error(hostile)).message, /catalog-stage.*cause is unknown/);
  });
  await check('actual provider host stops for transport/config but latches signature/identity failures', async () => {
    const { publicKey, privateKey } = generateKeyPairSync('ed25519');
    const A = Buffer.from(publicKey.export({ format: 'jwk' }).x, 'base64url');
    const nonce = Buffer.alloc(32, 1), B = Buffer.alloc(32, 2);
    const payload = Buffer.concat([Buffer.from('possums/tor-onion-endorsement\0'), Buffer.from([1]), nonce, B]);
    const signature = sign(null, payload, privateKey);
    assert.equal((await m.checkTorEndorsementSignature(A, nonce, B, payload, signature)).transport_authorized, false);
    const invalidSignature = Buffer.from(signature); invalidSignature[0] ^= 1;
    async function actualFailure(onionIdentity, proof) {
      try { await m.checkTorEndorsementSignature(A, nonce, onionIdentity, payload, proof); }
      catch (error) { return error; }
      assert.fail('expected a real cryptographic/binding rejection');
    }
    const signatureFailure = await actualFailure(B, invalidSignature);
    const identityFailure = await actualFailure(Buffer.alloc(32, 3), signature);
    assert.equal(signatureFailure.constraint, 'rejected'); assert.equal(identityFailure.constraint, 'onion_identity');
    for (const [error, fatal] of [
      [new m.TorNativeTransportFailure('proxy'), false], [new m.TorNativeTransportFailure('handshake_unknown'), false],
      [new m.ConnectionFailure('tor_selection_invalid'), false], [new m.ConnectionFailure('route_selection_changed'), false],
      [new m.TorSetupBackendFailure('acquisition', 'http_status', 503), false],
      [signatureFailure, true], [identityFailure, true],
      [new m.DiagnosticFailure('trust', 'endpoint_binding'), true],
    ]) {
      let calls = 0;
      const provider = new m.PossumsProvider(async () => { calls++; throw error; });
      provider.newSession();
      await assert.rejects(provider.verifySession());
      for (let turn = 0; turn < 2; turn++) {
        const events = [];
        const stream = provider.streamSimple({ id: 'synthetic', provider: 'possums', api: 'possums-verified' }, { messages: [] });
        for await (const event of stream) events.push(event);
        const result = await stream.result();
        assert.equal(result.stopReason, fatal ? 'error' : 'stop');
        assert.equal(events.at(-1).type, fatal ? 'error' : 'done');
        const warning = result.diagnostics?.find(item => item.type === 'possums_turn_warning');
        assert.equal(Boolean(warning), !fatal);
        if (warning) { assert.equal(warning.details.toolsAuthorized, false); assert.equal(warning.details.backendGenerationComplete, false); }
        assert(!JSON.stringify(result).includes(hostile));
      }
      assert.equal(calls, 1, 'warnings/status must never retry inference or trust');
      provider.shutdown();
    }
  });
}
