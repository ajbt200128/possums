export class TorOnionFailure extends Error {
  readonly code: 'possums_tor_onion_invalid';
  readonly stage: 'configuration';
  readonly constraint: 'canonical_v3_address_checksum';
}
export function parseTorOnionAddress(address: unknown): Readonly<{
  host: string;
  identity: Buffer;
  transport_authorized: false;
}>;
