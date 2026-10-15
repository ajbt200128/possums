// Address validation only. No network, key custody, appraisal or authorization.
import { createHash, timingSafeEqual } from 'node:crypto';
import { base32 } from '@scure/base';

export class TorOnionFailure extends Error {
  constructor() {
    super('[possums_tor_onion_invalid] Stage: configuration; constraint: canonical_v3_address_checksum. Select the approved v3 endpoint; do not send credentials or prompts.');
    delete this.stack;
    this.code = 'possums_tor_onion_invalid';
    this.stage = 'configuration';
    this.constraint = 'canonical_v3_address_checksum';
  }
}

export function parseTorOnionAddress(address) {
  try {
    if (typeof address !== 'string' || !/^[a-z2-7]{56}\.onion$/.test(address)) throw new TorOnionFailure();
    const bytes = base32.decode(address.slice(0, 56).toUpperCase());
    if (bytes.length !== 35 || bytes[34] !== 3) throw new TorOnionFailure();
    // Tor v3 address framing; maintained Base32 and native SHA3 do the algorithms.
    const checksum = createHash('sha3-256').update('.onion checksum', 'ascii')
      .update(bytes.subarray(0, 32)).update(bytes.subarray(34)).digest().subarray(0, 2);
    if (!timingSafeEqual(checksum, bytes.subarray(32, 34))) throw new TorOnionFailure();
    return Object.freeze({ host: address, identity: Buffer.from(bytes.subarray(0, 32)), transport_authorized: false });
  } catch { throw new TorOnionFailure(); }
}
