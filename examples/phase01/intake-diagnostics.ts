// Opt-in nonauthorizing metadata only. Never parse Error.message or server text.
import { DiagnosticFailure } from './limits.js';
const codes = [
  'INTAKE_AUTHENTICATION_UNCONFIRMED', 'INTAKE_MFA_UNSUPPORTED', 'INTAKE_BINDING_CHANGED',
  'INTAKE_INPUT_INVALID', 'INTAKE_ACCOUNT_UNAVAILABLE', 'INTAKE_CREDIT_INSUFFICIENT',
  'INTAKE_ADMISSION_DISABLED', 'INTAKE_REQUEST_CONFLICT', 'INTAKE_EVIDENCE_UNAVAILABLE',
  'INTAKE_REMOTE_UNAVAILABLE', 'INTAKE_PROTOCOL_UNCONFIRMED', 'INTAKE_REQUEST_UNAVAILABLE',
  'INTAKE_HOLD_WORK_DEADLINE', 'INTAKE_HOLD_UNUSABLE', 'INTAKE_HOLD_EXPIRED', 'INTAKE_TERMINAL_MISMATCH',
] as const;
const operations = ['intake_configuration', 'intake_login', 'intake_refresh', 'intake_auth_methods',
  'intake_reserve', 'intake_renew', 'intake_settle', 'intake_refund', 'intake_lookup', 'intake_balance', 'generation'] as const;
const stages = ['generation', 'transport', 'decode', 'auth_methods', 'token_metadata', 'binding', 'receipt',
  'remote', 'validation', 'account', 'admission', 'renewal', 'key', 'arithmetic', 'settlement', 'lookup', 'writer_gate',
  'transaction_begin', 'transaction_work', 'transaction_completion', 'reservation_identity', 'state_validation',
  'readiness', 'shutdown', 'startup', 'init', 'routing', 'request_read', 'request_dispatch', 'response_encoding'] as const;
const constraints = ['work_deadline', 'usable_hold', 'immutable_hold_expiry', 'first_terminal_wins',
  'input', 'transport', 'required_schema', 'password_method', 'mfa', 'token_metadata', 'original_expiry',
  'principal_binding', 'reservation_binding', 'reservation_state', 'account', 'credit', 'admission',
  'idempotency', 'charge', 'key_window', 'evidence', 'remote_cause_unknown', 'key_syntax', 'money_overflow',
  'immutable_deadline', 'writer_wait', 'work_budget', 'transaction_started', 'usable_storage', 'unique_identifier',
  'consistent_state', 'commit_evidence', 'open_storage', 'storage_closure', 'original_storage', 'existing_state',
  'empty_directory', 'supported_schema', 'state_path', 'routes', 'query', 'content_encoding', 'content_type',
  'singleton_headers', 'idempotency_header', 'complete_body', 'closed_response',
  'configuration', 'encoding_limit', 'encoding_unknown', 'client_unknown', 'request_unknown',
  'connect_unknown', 'transport_unknown', 'deadline', 'response_encoding', 'response_limit',
  'response_read_unknown', 'response_json', 'active_reservation',
  'expected_not_after_authoritative_deadline', 'strictly_later_deadline', 'renewal_observation'] as const;
export type IntakeDiagnostic = Readonly<{ code: typeof codes[number]; operation: typeof operations[number];
  stage: typeof stages[number]; constraint: typeof constraints[number]; status: number | null }>;
const fields = ['code', 'operation', 'stage', 'constraint', 'status'];
const controlDiagnostics = new WeakMap<IntakeFailure, IntakeDiagnostic>();
// Keep the original HTTP failure category and recovery eligibility. In particular,
// a control rejection must not become a new GatewayError recovery trigger.
export class IntakeFailure extends DiagnosticFailure {
  constructor(stage: 'challenge' | 'authentication' | 'submission', status: number | undefined,
    diagnostic: IntakeDiagnostic, code: 'rejected' | 'uncertain' = 'uncertain') {
    super(stage, 'http', code, status);
    const closed = intakeDiagnostic(diagnostic);
    if (closed) controlDiagnostics.set(this, closed);
  }
  get intakeDiagnostic(): IntakeDiagnostic | undefined { return controlDiagnostics.get(this); }
}
function member<T extends string>(values: readonly T[], value: unknown): value is T {
  return typeof value === 'string' && value.length <= 64 && values.includes(value as T);
}
export function intakeDiagnostic(value: unknown): IntakeDiagnostic | undefined {
  try {
    if (!value || typeof value !== 'object' || Array.isArray(value)) return undefined;
    const keys = Object.keys(value);
    if (keys.length !== fields.length || !fields.every(key => Object.hasOwn(value, key))) return undefined;
    const { code, operation, stage, constraint, status } = value as Record<string, unknown>;
    if (!member(codes, code) || !member(operations, operation) || !member(stages, stage) || !member(constraints, constraint) ||
      !(status === null || typeof status === 'number' && Number.isInteger(status) && status >= 100 && status <= 599)) return undefined;
    return Object.freeze({ code, operation, stage, constraint, status });
  } catch { return undefined; }
}
export function intakeDescription(value: IntakeDiagnostic): string {
  // Revalidate even typed callers; only closed labels select local presentation.
  const diagnostic = intakeDiagnostic(value);
  if (!diagnostic) return '';
  const { code, operation, stage, constraint, status } = diagnostic;
  const actions: Partial<Record<IntakeDiagnostic['code'], string>> = {
    INTAKE_AUTHENTICATION_UNCONFIRMED: 'Native intake authentication is unconfirmed. Check access privately and log in deliberately; expiry is not assumed.',
    INTAKE_MFA_UNSUPPORTED: 'Native MFA is unsupported by this gateway. Ask the operator to check supported authentication; do not bypass MFA.',
    INTAKE_BINDING_CHANGED: 'The original customer binding no longer matches. Stop using this session and log in deliberately.',
    INTAKE_INPUT_INVALID: 'The intake request failed input validation. Check the expected request format privately.',
    INTAKE_CREDIT_INSUFFICIENT: 'The maximum hold exceeds available credit. Add credit before a deliberate new submission.',
    // Retain decoding for the earlier gateway candidate, not a current cutoff policy.
    INTAKE_HOLD_WORK_DEADLINE: 'The gateway reported its hold work budget elapsed. Compute termination is not established; check gateway/client version compatibility.',
    INTAKE_HOLD_UNUSABLE: 'No active, unexpired hold was confirmed at initial admission. Check intake availability privately.',
    INTAKE_HOLD_EXPIRED: 'Intake returned an expired hold, not the requested settlement. Check accounting privately.',
    INTAKE_TERMINAL_MISMATCH: 'The authoritative first terminal outcome differs from the requested terminal operation. Retain any received receipt and check accounting privately.',
  };
  const transportActions: Partial<Record<IntakeDiagnostic['constraint'], string>> = {
    configuration: 'Check the approved HTTPS origin and runtime key privately; do not share their values.',
    encoding_limit: 'The intake request exceeded its encoding bound. Check gateway/intake contract compatibility.',
    encoding_unknown: 'Intake request encoding did not complete; the underlying cause is unknown. Check gateway compatibility.',
    client_unknown: 'Intake client construction did not complete; the underlying cause is unknown. Check the gateway runtime.',
    request_unknown: 'Intake request construction did not complete; the underlying cause is unknown. Check gateway compatibility.',
    connect_unknown: 'A verified intake connection was not established; the connection cause is unknown. Check connectivity privately.',
    transport_unknown: 'Intake transport did not complete; the underlying cause is unknown. Check connectivity privately.',
    deadline: 'The intake attempt deadline elapsed. Check service availability; the remote outcome is not established.',
    response_encoding: 'Intake returned unsupported content encoding. Check gateway/intake compatibility.',
    response_limit: 'The intake response exceeded its bounded envelope. Check gateway/intake compatibility; no partial receipt is authoritative.',
    response_read_unknown: 'The intake response could not be read completely; the underlying cause is unknown. Check service availability.',
    response_json: 'The intake response failed JSON/schema decoding. Check gateway/intake compatibility.',
    active_reservation: 'The hold is already terminal and cannot be renewed. Retain the authoritative outcome; do not replay.',
    expected_not_after_authoritative_deadline: 'The renewal expectation is ahead of the authoritative lease. Check gateway/intake compatibility; do not replay.',
    strictly_later_deadline: 'Renewal did not establish a later lease. Check intake availability privately; no renewal retry was made.',
    renewal_observation: 'Renewal evidence did not satisfy the lease-time constraints. Check gateway/intake compatibility; no renewal retry was made.',
  };
  const action = transportActions[constraint] ?? (constraint === 'writer_wait' ? 'The intake writer gate wait did not complete. Check intake availability privately.' :
    constraint === 'commit_evidence' ? 'Transaction completion evidence is unavailable; commit outcome is unknown. Check accounting privately; do not replay the operation.' :
    constraint === 'original_expiry' ? 'Original intake authority expired. Log in deliberately; this session cannot extend its authority.' :
    actions[code] ?? 'Intake evidence is unconfirmed; the underlying cause is unknown. Ask the operator to check the indicated constraint privately.');
  return `[${code}] Operation: ${operation}; stage: ${stage}; constraint: ${constraint}.` +
    (status === null ? ' Intake HTTP status: unobserved.' : ` Observed intake HTTP status: ${status}.`) +
    ` ${action} Diagnostic metadata does not establish completion, billing or tool authorization. Share only code/operation/stage/constraint/status for support.`;
}
