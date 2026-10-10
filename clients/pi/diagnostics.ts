import { CatalogFailure, GatewayError, gatewayDetailLabel } from '../../examples/phase01/client.js';
import { DiagnosticFailure } from '../../examples/phase01/limits.js';

export function diagnosticDescription(failure: DiagnosticFailure): string {
  const actions = {
    trust: 'Start a new Pi session to verify again; do not bypass verification. Re-entering credentials does not renew trust.',
    challenge: 'Check connectivity and try /login again; credential validity is not established.',
    authentication: 'For an HTTP 401/403 rejection, check your recovery credential and use /login; otherwise check connectivity. Do not share credentials.',
    catalog: 'Check connectivity and refresh models; do not substitute unverified catalog data.',
    request: 'Review supported request options and history structure; simplify tool schemas if needed.',
    submission: 'Check connectivity or use /login for an HTTP 401/403 rejection. Do not replay an uncertain submission.',
    transport: 'Check connectivity; share only this code/stage/constraint if the problem persists.',
    stream: 'The response is not authorized for tool execution. Share only this code/stage/constraint if the problem persists.',
    settlement: 'Receipt validation or local accounting display did not complete. Share only this code/stage/constraint for support.',
    provider: 'The client encountered an unexpected local failure; its cause is unknown. Share only this code/stage/constraint for support.',
    hook: 'A client request/response hook did not complete. Check custom hooks; do not share request data.',
    balance: 'This balance check sends no inference. Repeat it once Pi is idle; share only this code for support if it persists.',
    compaction: 'No checkpoint is authorized. Share only this code for support; a deliberate /compact may incur a new charge.',
  } as const;
  const description = failure.constraint === 'expired' ? 'Session-pinned trust validity has elapsed.' :
    failure.constraint === 'unexpected' ? 'The component did not complete; the underlying cause is unknown.' :
    failure.constraint === 'precision' ? 'A numeric value exceeds the supported safe-integer range for Pi metadata/display.' :
    `The ${failure.constraint.replaceAll('_', ' ')} constraint was not satisfied.`;
  return `[${failure.message}] Stage: ${failure.stage}; constraint: ${failure.constraint}. ${description}` +
    (failure.status === undefined ? '' : ` Observed HTTP status: ${failure.status}.`) + ` ${actions[failure.stage]}`;
}

const update = 'For an explicit pinned manifest, install the matching independently approved client/manifest pair. For normal gateway updates, start a new Pi session. Re-entering your credential will not fix verification.';
const catalogAction = 'Check connectivity and refresh models; if this persists, share this content-free code for support.';
const reasons = Object.freeze({
  trust_expired: 'The session-pinned gateway trust has expired. Start a new Pi session to verify again; do not bypass verification. This is not evidence that authentication expired.',
  credential_missing: 'No usable recovery credential was selected. Use /login or configure POSSUMS_RECOVERY_CREDENTIAL locally; never share its value.',
  credential_resolution_failed: 'Local credential selection did not complete; cause unknown. Check the local Pi credential configuration. Share only this code for support, not credentials.',
  credential_entry_failed: 'Credential entry did not complete. Try /login again; do not share credentials. Cause unknown unless you deliberately cancelled.',
  authentication_diagnostic: 'Gateway authentication did not complete.',
  connection_diagnostic: 'The verified connection did not complete.',
  approval_expired: 'The installed client approval has expired. ' + update,
  approval_unavailable: 'No usable local client approval is available. ' + update,
  manifest_unavailable: 'The public release manifest is missing, unreadable or exceeds local limits. ' + update,
  manifest_mismatch: 'The public release manifest does not match this extension’s approval. ' + update,
  evidence_unavailable: 'Public verification evidence could not be retrieved within connection limits. Check connectivity and start a new Pi session to verify again; do not change approval pins.',
  verification_failed: 'The published gateway release or serving channel could not be verified. Check connectivity and start a new Pi session to verify again. Do not bypass verification or replay an interrupted request.',
  session_unavailable: 'Gateway session setup (challenge/login) did not complete; the exact constraint is unknown. Start a new Pi session if none is active; otherwise check connectivity and try /login again. This does not establish that your credential is invalid.',
  catalog_unavailable: 'The session was authenticated, but an unexpected catalog-stage failure occurred; the underlying cause is unknown. Check connectivity and try refreshing models again; if this persists, share this content-free code for support.',
  catalog_request_failed: 'The catalog request failed before a response was available. ' + catalogAction,
  catalog_http_rejected: 'The catalog request received an HTTP rejection. ' + catalogAction,
  catalog_body_invalid: 'The catalog response body could not be read or decoded within client limits. ' + catalogAction,
  catalog_validation_failed: 'The catalog response failed schema or reservation-quote validation. ' + catalogAction,
  catalog_conversion_failed: 'The validated catalog could not be converted to Pi model metadata within supported limits. ' + catalogAction,
  service_quiescing: 'Gateway is quiescing. Wait for service availability before deliberately starting a new request; this diagnostic does not retry or establish prior billing.',
});
export type ConnectionFailureCode = keyof typeof reasons;

// Reused from recovery branch ee80c8b, without its lifecycle/replay changes.
const evidenceStages = Object.freeze({
  release_discovery: 'GitHub release discovery', manifest_redirect: 'GitHub manifest redirect',
  manifest_download: 'public manifest download', gateway_attestation: 'gateway attestation',
  amd_certificate: 'AMD endorsement certificate', gateway_certificate: 'gateway certificate',
  release_provenance: 'GitHub release provenance', gateway_keys: 'gateway endpoint keys',
  public_evidence_cache: 'local public evidence cache',
});
const evidenceConstraints = Object.freeze({
  request: 'request transport did not complete before a response',
  interrupted: 'evidence operation interrupted', idle: 'evidence read wait elapsed', deadline: 'evidence operation deadline elapsed',
  http: 'HTTP response rejected', rate_limited: 'public evidence service rate limited this request',
  binding: 'response destination did not match the required endpoint',
  redirect: 'manifest redirect status or authority did not match policy',
  body: 'response body could not be read within transport/parser limits',
  schema: 'evidence format did not satisfy the required schema',
  storage: 'local public cache read, coordination or publication did not complete; check cache directory permissions',
});
export type EvidenceStage = keyof typeof evidenceStages;
export type EvidenceConstraint = keyof typeof evidenceConstraints;
export class EvidenceObservation {
  constructor(readonly stage: EvidenceStage, readonly constraint: EvidenceConstraint, readonly status?: number) {
    if (!Object.hasOwn(evidenceStages, stage) || !Object.hasOwn(evidenceConstraints, constraint)
      || (status !== undefined && (!Number.isInteger(status) || status < 100 || status > 599))) {
      throw new Error('possums_evidence_diagnostic_invalid');
    }
    Object.freeze(this);
  }
}

export function approvalSummary(release?: Readonly<{ tag: string; expires: number }>): string {
  return release
    ? `Session-pinned gateway: ${release.tag}; valid until ${new Date(release.expires).toISOString()}. Local session information only, not a current connection check.`
    : 'No gateway has been verified for this session.';
}

// Construct only locally authored messages; never retain the original error/cause.
export class ConnectionFailure extends Error {
  constructor(readonly code: ConnectionFailureCode, catalog?: CatalogFailure | GatewayError | DiagnosticFailure | EvidenceObservation) {
    if (!Object.hasOwn(reasons, code)) throw new Error('possums_connection_diagnostic_invalid');
    // The immutable carrier validates these fields; never use its error.message.
    const observed = catalog instanceof CatalogFailure && catalog.stage === 'http'
      ? ` Observed HTTP status: ${catalog.status ?? 'unavailable'}.${catalog.reason ? ` Gateway code: ${catalog.reason}.` : ''}${catalog.detail ? ` Gateway detail: ${gatewayDetailLabel(catalog.detail)}.` : ''}` : '';
    const evidence = catalog instanceof EvidenceObservation
      ? ` Stage: ${catalog.stage} (${evidenceStages[catalog.stage]}); constraint: ${catalog.constraint} (${evidenceConstraints[catalog.constraint]}).${catalog.status === undefined ? '' : ` Observed HTTP status: ${catalog.status}.`}${catalog.constraint === 'rate_limited' ? ' Wait before starting a new Pi session; re-entering credentials will not fix this.' : ''}` :
      catalog instanceof GatewayError ? ` Stage: admission; constraint: service_quiescing.${catalog.status === undefined ? '' : ` Observed HTTP status: ${catalog.status}.`}` :
      ` Stage: ${catalog instanceof CatalogFailure ? 'catalog' : 'connection'}; constraint: ${catalog instanceof CatalogFailure ? catalog.stage : code}.${catalog instanceof CatalogFailure && catalog.reason === 'service_quiescing' ? ' Gateway stage: admission; constraint: service_quiescing.' : ''}`;
    super(`${catalog instanceof DiagnosticFailure ? diagnosticDescription(catalog) : `[possums_${code}] ${reasons[code]}${observed}${evidence}`} No inference request was sent by this connection attempt. Cached models do not authorize inference. Prior billing outcomes are not established by this check.`);
    Object.freeze(this);
  }
}
export function catalogConnectionFailure(error: unknown): ConnectionFailure {
  if (error instanceof ConnectionFailure) return error;
  if (error instanceof DiagnosticFailure) return connectionFailure(error);
  if (!(error instanceof CatalogFailure)) return new ConnectionFailure('catalog_unavailable');
  const codes = { request: 'catalog_request_failed', http: 'catalog_http_rejected', body: 'catalog_body_invalid',
    validation: 'catalog_validation_failed', conversion: 'catalog_conversion_failed' } as const;
  return new ConnectionFailure(error.reason === 'service_quiescing' ? 'service_quiescing' : codes[error.stage], error);
}
export function connectionFailure(error: unknown): ConnectionFailure {
  if (error instanceof ConnectionFailure) return error;
  if (error instanceof GatewayError && error.reason === 'service_quiescing') return new ConnectionFailure('service_quiescing', error);
  if (error instanceof DiagnosticFailure) return new ConnectionFailure(error.stage === 'trust' ? error.constraint === 'expired' ? 'trust_expired' : 'verification_failed' : error.stage === 'authentication' || error.stage === 'challenge' ? 'authentication_diagnostic' : 'connection_diagnostic', error);
  return new ConnectionFailure('verification_failed');
}
