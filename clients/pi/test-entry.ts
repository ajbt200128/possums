// Explicit test entry only. build.mjs never emits or distributes this bundle.
export { default as extension } from './index.js';
export { PossumsProvider } from './provider.js';
export { connect, connectPublished, recoverCompiled } from './bootstrap.js';
export { publicEvidence, evidenceDirectory } from './evidence-cache.js';
export { ConnectionFailure, EvidenceObservation, connectionFailure, catalogConnectionFailure, approvalSummary } from './diagnostics.js';
export { API_APPROVALS, PUBLISHER, qualifyPublished } from '../../examples/phase01/approval.js';
export { invocation } from './wire.js';
export { thinkingMetadata, simpleThinking } from './thinking.js';
export { reasoningOptions, reasoningParams, admitReasoning } from '../../examples/phase01/reasoning.js';
export { Channel } from '../../examples/phase01/transport.js';
export { pinnedRequest } from './v3.js';
export { ReferenceClient, CatalogFailure, GatewayError, gatewayError, CompletionFailure, consumeCompletion, validateModels } from '../../examples/phase01/client.js';
export { IntakeFailure, intakeDiagnostic, intakeDescription } from '../../examples/phase01/intake-diagnostics.js';
export { Identity } from 'ehbp';
export { ChannelError, DiagnosticFailure, OperationFailure, Operation, LIMITS, JSONDepthError } from '../../examples/phase01/limits.js';
export { snapshotInvocation } from '../../examples/phase01/tools.js';
