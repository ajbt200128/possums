import { API_APPROVALS } from '../../examples/phase01/approval.js';

const update = 'Install the independently approved Possums extension and its matching public manifest together, then run /reload. Re-entering your credential will not fix this verification problem.';
const reasons = Object.freeze({
  approval_expired: 'The installed client approval has expired. ' + update,
  approval_unavailable: 'No usable local client approval is available. ' + update,
  manifest_unavailable: 'The public release manifest is missing, unreadable or exceeds local limits. ' + update,
  manifest_mismatch: 'The public release manifest does not match this extension’s approval. ' + update,
  evidence_unavailable: 'Public verification evidence could not be retrieved within connection limits. Check connectivity and try refreshing again; do not change approval pins.',
  verification_failed: 'The gateway could not be verified against this client’s approval. If the gateway was updated, update the independently approved extension and matching public manifest, then run /reload. Re-entering your credential will not fix a verification failure.',
});
export type ConnectionFailureCode = keyof typeof reasons;

export function approvalSummary(): string {
  const approval = API_APPROVALS.length === 1 ? API_APPROVALS[0] : undefined;
  return approval
    ? `Client approval: ${approval.tag}; expires ${new Date(approval.expires).toISOString()}. Local approval information only, not a connection check.`
    : 'No single local client approval is configured.';
}

// Construct only locally authored messages; never retain the original error/cause.
export class ConnectionFailure extends Error {
  constructor(readonly code: ConnectionFailureCode) {
    super(`[possums_${code}] ${reasons[code]} ${approvalSummary()} No inference request was sent by this connection attempt. Cached models do not authorize inference.`);
  }
}
export function connectionFailure(error: unknown): ConnectionFailure {
  return error instanceof ConnectionFailure ? error : new ConnectionFailure('verification_failed');
}
