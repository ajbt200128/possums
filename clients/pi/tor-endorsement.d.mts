export class TorEndorsementFailure extends Error {
  readonly code: string;
  readonly stage: string;
  readonly constraint: string;
  readonly action: string;
  constructor(stage?: string, constraint?: string);
}
/** Signature compatibility only; cannot authorize a connection. */
export function checkTorEndorsementSignature(applicationKey: Uint8Array, challenge: Uint8Array,
  selectedOnionIdentity: Uint8Array, payload: Uint8Array, signature: Uint8Array): Promise<Readonly<{
    signature_valid: true; transport_authorized: false;
  }>>;
