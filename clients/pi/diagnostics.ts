import { CatalogFailure, gatewayDetailLabel } from '../../examples/phase01/client.js';

const update = 'For an explicit pinned manifest, install the matching independently approved client/manifest pair. For normal gateway updates, start a new Pi session. Re-entering your credential will not fix verification.';
const catalogAction = 'Check connectivity and refresh models; if this persists, share this content-free code for support.';
const reasons = Object.freeze({
  approval_expired: 'The installed client approval has expired. ' + update,
  approval_unavailable: 'No usable local client approval is available. ' + update,
  manifest_unavailable: 'The public release manifest is missing, unreadable or exceeds local limits. ' + update,
  manifest_mismatch: 'The public release manifest does not match this extension’s approval. ' + update,
  evidence_unavailable: 'Public verification evidence could not be retrieved within connection limits. Check connectivity and start a new Pi session to verify again; do not change approval pins.',
  verification_failed: 'The published gateway release or serving channel could not be verified. Check connectivity and start a new Pi session to verify again. Do not bypass verification or replay an interrupted request.',
  session_unavailable: 'Gateway session setup (challenge/login) did not complete. Start a new Pi session if none is active; otherwise check connectivity and try /login again. This does not establish that your credential is invalid.',
  catalog_unavailable: 'The session was authenticated, but the model catalog could not be fetched, validated or converted for Pi. Check connectivity and try refreshing models again; if this persists, share this content-free code for support.',
  catalog_request_failed: 'The catalog request failed before a response was available. ' + catalogAction,
  catalog_http_rejected: 'The catalog request received an HTTP rejection. ' + catalogAction,
  catalog_body_invalid: 'The catalog response body could not be read or decoded within client limits. ' + catalogAction,
  catalog_validation_failed: 'The catalog response failed schema or reservation-quote validation. ' + catalogAction,
  catalog_conversion_failed: 'The validated catalog could not be converted to Pi model metadata within supported limits. ' + catalogAction,
});
export type ConnectionFailureCode = keyof typeof reasons;

export function approvalSummary(release?: Readonly<{ tag: string; expires: number }>): string {
  return release
    ? `Session-pinned gateway: ${release.tag}; valid until ${new Date(release.expires).toISOString()}. Local session information only, not a current connection check.`
    : 'No gateway has been verified for this session.';
}

// Construct only locally authored messages; never retain the original error/cause.
export class ConnectionFailure extends Error {
  constructor(readonly code: ConnectionFailureCode, catalog?: CatalogFailure) {
    // The immutable carrier validates these fields; never use its error.message.
    const observed = catalog instanceof CatalogFailure && catalog.stage === 'http'
      ? ` Observed HTTP status: ${catalog.status ?? 'unavailable'}.${catalog.reason ? ` Gateway code: ${catalog.reason}.` : ''}${catalog.detail ? ` Gateway detail: ${gatewayDetailLabel(catalog.detail)}.` : ''}` : '';
    super(`[possums_${code}] ${reasons[code]}${observed} No inference request was sent by this connection attempt. Cached models do not authorize inference.`);
  }
}
export function catalogConnectionFailure(error: unknown): ConnectionFailure {
  if (!(error instanceof CatalogFailure)) return new ConnectionFailure('catalog_unavailable');
  const codes = { request: 'catalog_request_failed', http: 'catalog_http_rejected', body: 'catalog_body_invalid',
    validation: 'catalog_validation_failed', conversion: 'catalog_conversion_failed' } as const;
  return new ConnectionFailure(codes[error.stage], error);
}
export function connectionFailure(error: unknown): ConnectionFailure {
  return error instanceof ConnectionFailure ? error : new ConnectionFailure('verification_failed');
}
