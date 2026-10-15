export type V3Facts = Readonly<{
  tls_key_sha256: string; hpke_key: string; release_tag: string; release_digest: string;
  freshness_expires_at_unix: number;
}>;
export type TorV3Facts = V3Facts & Readonly<{ tor_endorsement_key: string }>;
export class VerifierFailure extends Error {
  readonly code: string;
  readonly stage: string;
  readonly constraint: string;
  readonly action: string;
  constructor(stage?: string, constraint?: string);
}
export function verifyV3(evidence: Uint8Array, nonce: Uint8Array, repository?: string,
  signal?: AbortSignal, manifest?: Uint8Array): Promise<V3Facts>;
export function verifyTorV3(evidence: Uint8Array, nonce: Uint8Array, manifest: Uint8Array,
  repository?: string, signal?: AbortSignal): Promise<TorV3Facts>;
