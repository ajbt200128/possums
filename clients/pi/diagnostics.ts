import { CatalogFailure, CompletionFailure, GatewayError, gatewayDetailLabel, safeTorNativeDiagnostic, type Receipt } from '../../examples/phase01/client.js';
import { IntakeFailure, intakeDescription, type IntakeDiagnostic } from '../../examples/phase01/intake-diagnostics.js';

export function settlementDescription(receipt: Readonly<Receipt>): string {
  return `Authenticated gateway receipt (${receipt.complete === false ? 'incomplete' : 'complete'}): outcome ${receipt.outcome ?? 'unavailable'}; charged ${receipt.chargedMicrounits} microunits; refunded ${receipt.refundedMicrounits ?? 'unavailable'} microunits.${receipt.deliveryComplete === false ? ' Delivery incomplete; this does not establish successful completion.' : ''} Settlement is not tool-execution authorization; a new submission may incur another charge.`;
}
import { DiagnosticFailure } from '../../examples/phase01/limits.js';
import { VerifierFailure } from './verifier.mjs';
import { TorNativeTransportFailure, TorOnionFailure,
  TorSetupFailure, TorSetupBackendFailure, TorEndorsementFailure, isTorConnectionFailure } from './tor-connection.js';
type TorFailure = TorNativeTransportFailure | TorOnionFailure | TorSetupFailure | TorSetupBackendFailure | TorEndorsementFailure;

function torDescription(error: TorFailure): string {
  if (error instanceof TorNativeTransportFailure) {
    const safe = safeTorNativeDiagnostic(error);
    return `[${safe.code}] Stage: ${safe.stage}; constraint: ${safe.constraint}. ${safe.action} ${safe.outcome}`;
  }
  if (error instanceof TorOnionFailure) {
    return '[possums_tor_onion_invalid] Stage: configuration; constraint: canonical_v3_address_checksum. Select the approved canonical v3 host; do not send credentials or prompts.';
  }
  // These immutable closed carriers author code/action locally. Reconstruct
  // anyway so neither Error.message nor a prototype-shaped impostor is used.
  const safe = error instanceof TorSetupBackendFailure ? new TorSetupBackendFailure(error.stage, error.constraint, error.status)
    : error instanceof TorSetupFailure ? new TorSetupFailure(error.stage, error.constraint)
    : new TorEndorsementFailure(error.stage, error.constraint);
  return `[${safe.code}] Stage: ${safe.stage}; constraint: ${safe.constraint}. ${safe.action}` +
    (safe.constraint.endsWith('unknown') ? ' The underlying cause is unknown.' : '') +
    (safe instanceof TorSetupBackendFailure
      ? ` UNAUTHENTICATED public backend diagnostic.${safe.status === undefined ? '' : ` Observed acquisition HTTP status: ${safe.status} (not necessarily gateway response status).`} Backend authenticity, service availability and billing are not established.` : '');
}

export function diagnosticDescription(failure: DiagnosticFailure): string {
  if (failure instanceof IntakeFailure && failure.intakeDiagnostic) {
    return intakeDescription(failure.intakeDiagnostic) + ' No request was replayed; billing is unknown unless an authenticated receipt was received. A new submission may incur another charge.';
  }
  if (failure instanceof CompletionFailure && failure.nativeDiagnostic) {
    const native = failure.nativeDiagnostic;
    const outcome = failure.settlement
      ? 'Remote completion is unknown. Received authenticated billing evidence is retained; delivery completion is not established. No request was replayed; a deliberate new submission may incur another charge.'
      : native.outcome;
    return `[${native.code}] Stage: ${failure.stage}; constraint: ${failure.constraint}. Native stage: ${native.stage}; constraint: ${native.constraint}.` +
      (failure.status === undefined ? '' : ` Observed HTTP status: ${failure.status}.`) + ` ${native.action} ${outcome}`;
  }
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
    compaction: 'No checkpoint is authorized. Share only this code for support; a deliberate /compact may incur a new charge.',
  } as const;
  const requestConstraints: Partial<Record<DiagnosticFailure['constraint'], string>> = {
    tool_name: 'Function names must be 1–64 ASCII letters, digits, underscores or hyphens; declarations must have unique names. Correct the tool declaration.',
    tool_id: 'Historical tool-call IDs must be 1–128 ASCII letters, digits, underscores or hyphens. Correct the call/result IDs together.',
    tool_schema: 'Function declarations must contain a name and JSON schema object with no explicit non-object root type; optional descriptions must be text. Keep schemas within parser limits and use an object schema or a local reference/combinator.',
    schema_reference: 'Only local fragment schema references are supported; resource IDs and dynamic/recursive references are unsupported. Inline external schemas without changing their constraints.',
    tool_choice: 'Tool choice must be auto, none, required, or a named currently declared function. Correct the choice or declare that function.',
    history: 'History must use supported text messages and function-call records within the request envelope. Correct the history structure; do not share its contents.',
    history_order: 'Tool calls/results must have unique call IDs and complete, matching result batches before the next turn. Correct the history ordering.',
    tool_arguments: 'Historical function arguments must be a JSON object with finite numbers and unique keys within parser limits. Correct the stored arguments.',
    reasoning: 'Reasoning controls must match a configuration in the current authenticated model catalog. Omit unsupported controls or refresh model metadata; no effort or disable configuration is inferred.',
    conversion: 'Pi SDK history/tool conversion did not complete; the underlying cause is unknown. Check custom context transformations; share only this code/stage/constraint for support.',
  };
  const requestDescription = failure.stage === 'request' ? requestConstraints[failure.constraint] : undefined;
  const description = requestDescription ?? (failure.constraint === 'expired' ? 'Session-pinned trust validity has elapsed.' :
    failure.constraint === 'unexpected' ? 'The component did not complete; the underlying cause is unknown.' :
    failure.constraint === 'precision' ? 'A numeric value exceeds the supported safe-integer range for Pi metadata/display.' :
    failure.constraint === 'peer_key_mismatch' ? 'The TLS peer key differs from the last verified identity; the underlying cause is unknown.' :
    `The ${failure.constraint.replaceAll('_', ' ')} constraint was not satisfied.`);
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
  tor_transport_diagnostic: 'The selected Tor transport did not complete. No direct fallback was attempted.',
  tor_selection_invalid: 'Local Tor selection is invalid. Select a canonical v3 host with --possums-tor-onion and a numeric loopback host:port with --possums-tor-proxy (default 127.0.0.1:19050). Do not combine Tor with --possums-manifest or select a proxy alone. Proxy-only username/password must both be absent or locally configured together, each nonempty and at most 255 UTF-8 bytes; do not share them.',
  route_selection_changed: 'Route or manifest flags changed after trust selection. Restore the original flags or restart Pi with the intended setup; no route was switched.',
  tor_cleanup_unknown: 'Native child exit could not be confirmed during Tor scope cleanup; cause unknown. Restart Pi before another setup attempt. Remote completion and prior billing are unknown.',
  approval_expired: 'The installed client approval has expired. ' + update,
  approval_unavailable: 'No usable local client approval is available. ' + update,
  manifest_unavailable: 'The public release manifest is missing, unreadable or exceeds local limits. ' + update,
  manifest_mismatch: 'The public release manifest does not match this extension’s approval. ' + update,
  evidence_unavailable: 'Public verification evidence could not be retrieved within connection limits. Check connectivity and start a new Pi session to verify again; do not change approval pins.',
  evidence_unchanged: 'Public evidence did not establish a different fully verified endpoint. No credentials were sent by this probe; the previous request was not replayed. Check authentication deliberately; earlier billing remains unknown.',
  verification_failed: 'The published gateway release or serving channel could not be verified. Check connectivity and start a new Pi session to verify again. Do not bypass verification or replay an interrupted request.',
  session_unavailable: 'Gateway session setup (challenge/login) did not complete; the exact constraint is unknown. Start a new Pi session if none is active; otherwise check connectivity and try /login again. This does not establish that your credential is invalid.',
  catalog_unavailable: 'The session was authenticated, but an unexpected catalog-stage failure occurred; the underlying cause is unknown. Check connectivity and try refreshing models again; if this persists, share this content-free code for support.',
  catalog_request_failed: 'The catalog request failed before a response was available. ' + catalogAction,
  catalog_http_rejected: 'The catalog request received an HTTP rejection. ' + catalogAction,
  catalog_body_invalid: 'The catalog response body could not be read or decoded within client limits. ' + catalogAction,
  catalog_validation_failed: 'The catalog response required fields were unreadable or unsupported. ' + catalogAction,
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
  readonly observation?: EvidenceObservation;
  readonly diagnostic?: DiagnosticFailure;
  readonly intakeDiagnostic?: IntakeDiagnostic;
  constructor(readonly code: ConnectionFailureCode, catalog?: CatalogFailure | GatewayError | DiagnosticFailure | EvidenceObservation | VerifierFailure | TorFailure) {
    if (!Object.hasOwn(reasons, code)) throw new Error('possums_connection_diagnostic_invalid');
    // The immutable carrier validates these fields; never use its error.message.
    const observed = catalog instanceof CatalogFailure && catalog.stage === 'http'
      ? ` Observed HTTP status: ${catalog.status ?? 'unavailable'}.${catalog.reason ? ` Gateway code: ${catalog.reason}.` : ''}${catalog.detail ? ` Gateway detail: ${gatewayDetailLabel(catalog.detail)}.` : ''}` : '';
    const intake = catalog instanceof CatalogFailure || catalog instanceof GatewayError || catalog instanceof IntakeFailure ? catalog.intakeDiagnostic : undefined;
    const evidence = intake ? ` ${intakeDescription(intake)}` : catalog instanceof EvidenceObservation
      ? ` Stage: ${catalog.stage} (${evidenceStages[catalog.stage]}); constraint: ${catalog.constraint} (${evidenceConstraints[catalog.constraint]}).${catalog.status === undefined ? '' : ` Observed HTTP status: ${catalog.status}.`}${catalog.constraint === 'rate_limited' ? ' Wait before starting a new Pi session; re-entering credentials will not fix this.' : ''}` :
      catalog instanceof GatewayError ? ` Stage: admission; constraint: service_quiescing.${catalog.status === undefined ? '' : ` Observed HTTP status: ${catalog.status}.`}` :
      code === 'tor_cleanup_unknown' ? ' Stage: tor_cleanup; constraint: cause_unknown.' :
      ` Stage: ${catalog instanceof CatalogFailure ? 'catalog' : code === 'tor_selection_invalid' || code === 'route_selection_changed' ? 'configuration' : 'connection'}; constraint: ${catalog instanceof CatalogFailure ? catalog.stage : code}.${catalog instanceof CatalogFailure && catalog.reason === 'service_quiescing' ? ' Gateway stage: admission; constraint: service_quiescing.' : ''}`;
    const compatibilityCode = code === 'verification_failed' && catalog instanceof DiagnosticFailure
      && catalog.stage === 'trust' && catalog.constraint === 'unexpected' ? '[possums_verification_failed] ' : '';
    const outcome = code === 'tor_cleanup_unknown'
      ? 'No request was replayed; a deliberate new submission may incur another charge.'
      : 'No inference request was sent by this connection attempt. Cached models do not authorize inference. Prior billing outcomes are not established by this check.';
    super(`${compatibilityCode}${catalog && isTorConnectionFailure(catalog) ? `[possums_${code}] ${reasons[code]} ${torDescription(catalog)}` : catalog instanceof VerifierFailure ? catalog.message : catalog instanceof DiagnosticFailure ? diagnosticDescription(catalog) : `[possums_${code}] ${reasons[code]}${observed}${evidence}`} ${outcome}`);
    if (catalog instanceof EvidenceObservation) this.observation = catalog;
    if (catalog instanceof DiagnosticFailure) this.diagnostic = catalog;
    if (intake) this.intakeDiagnostic = intake;
    Object.freeze(this);
  }
}
export function catalogConnectionFailure(error: unknown): ConnectionFailure {
  if (error instanceof ConnectionFailure) return error;
  if (error instanceof DiagnosticFailure || isTorConnectionFailure(error)) return connectionFailure(error);
  if (!(error instanceof CatalogFailure)) return new ConnectionFailure('catalog_unavailable');
  const codes = { request: 'catalog_request_failed', http: 'catalog_http_rejected', body: 'catalog_body_invalid',
    validation: 'catalog_validation_failed', conversion: 'catalog_conversion_failed' } as const;
  return new ConnectionFailure(error.reason === 'service_quiescing' ? 'service_quiescing' : codes[error.stage], error);
}
export function connectionFailure(error: unknown): ConnectionFailure {
  if (error instanceof ConnectionFailure) return error;
  if (error instanceof TorNativeTransportFailure) return new ConnectionFailure(
    safeTorNativeDiagnostic(error).code === 'possums_tor_native_configuration' ? 'tor_selection_invalid' : 'tor_transport_diagnostic', error);
  if (error instanceof TorOnionFailure) return new ConnectionFailure('tor_selection_invalid', error);
  if (error instanceof TorSetupBackendFailure || error instanceof TorSetupFailure || error instanceof TorEndorsementFailure) {
    // Operational acquisition/interruption is not distrust. Signature, binding,
    // appraisal and malformed public trust evidence still fail closed/fatal.
    const operational = error instanceof TorSetupBackendFailure && ['acquisition', 'backend', 'input', 'response'].includes(error.stage)
      || error.stage === 'authorization' && error.constraint === 'interrupted';
    return new ConnectionFailure(operational ? 'tor_transport_diagnostic' : 'verification_failed', error);
  }
  if (error instanceof GatewayError) return new ConnectionFailure(error.reason === 'service_quiescing' ? 'service_quiescing' : 'connection_diagnostic', error);
  if (error instanceof VerifierFailure) return new ConnectionFailure('verification_failed', error);
  if (error instanceof DiagnosticFailure) return new ConnectionFailure(error.stage === 'trust' ? error.constraint === 'expired' ? 'trust_expired' : 'verification_failed' : error.stage === 'authentication' || error.stage === 'challenge' ? 'authentication_diagnostic' : 'connection_diagnostic', error);
  return new ConnectionFailure('verification_failed', new DiagnosticFailure('trust', 'unexpected'));
}
