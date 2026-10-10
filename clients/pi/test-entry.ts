// Explicit test entry only. build.mjs never emits or distributes this bundle.
export { default as extension } from './index.js';
export { PossumsProvider } from './provider.js';
export { connect, connectPublished } from './bootstrap.js';
export { ConnectionFailure, connectionFailure, catalogConnectionFailure, approvalSummary } from './diagnostics.js';
export { API_APPROVALS, PUBLISHER, qualifyPublished } from '../../examples/phase01/approval.js';
export { invocation } from './wire.js';
export { Channel } from '../../examples/phase01/transport.js';
export { ReferenceClient, BalanceFailure, CatalogFailure, GatewayError, consumeCompletion, validateModels } from '../../examples/phase01/client.js';
export { ChannelError, DiagnosticFailure, OperationFailure, Operation, LIMITS, JSONDepthError } from '../../examples/phase01/limits.js';
export { snapshotInvocation } from '../../examples/phase01/tools.js';
