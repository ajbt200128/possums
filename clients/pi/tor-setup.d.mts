import type { TorV3Facts } from './verifier.mjs';

/** Decoded bytes are owned copies, not an appraisal or connection verdict. */
export type DecodedTorSetupResponse = Readonly<{
  evidence: Uint8Array;
  payload: Uint8Array;
  signature: Uint8Array;
  transport_authorized: false;
}>;
export type TorSetupFacts = TorV3Facts & Readonly<{
  onion_identity: string;
  transport_authorized: false;
}>;
export class TorSetupFailure extends Error {
  readonly code: string;
  readonly stage: string;
  readonly constraint: string;
  readonly action: string;
  constructor(stage?: string, constraint?: string);
}
/** Unauthenticated public diagnostics only, never appraisal/dispatch evidence. */
export class TorSetupBackendFailure extends Error {
  readonly code: string;
  readonly stage: string;
  readonly constraint: string;
  readonly action: string;
  /** Acquisition status, not necessarily the gateway response status. */
  readonly status?: number;
  readonly metadata_authenticated: false;
  readonly transport_authorized: false;
  constructor(stage?: string, constraint?: string, status?: number);
}
export function decodeTorSetupResponse(responseBytes: Uint8Array): DecodedTorSetupResponse;
/** Public appraisal only; selectedOnionIdentity requires a future qualified parser. */
export function verifyTorSetupResponse(responseBytes: Uint8Array, challenge: Uint8Array,
  manifest: Uint8Array, selectedOnionIdentity: Uint8Array, repository?: string,
  signal?: AbortSignal): Promise<TorSetupFacts>;
