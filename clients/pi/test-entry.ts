// Explicit test entry only. build.mjs never emits or distributes this bundle.
export { default as extension } from './index.js';
export { PossumsProvider } from './provider.js';
export { connect, connectPublished, recoverCompiled } from './bootstrap.js';
export { publicEvidence, publicManifest, evidenceDirectory } from './evidence-cache.js';
export { ConnectionFailure, EvidenceObservation, connectionFailure, catalogConnectionFailure, approvalSummary } from './diagnostics.js';
export { API_APPROVALS, PUBLISHER, qualifyPublished } from './core/approval.js';
export { invocation } from './wire.js';
export { thinkingMetadata, simpleThinking } from './thinking.js';
export { reasoningOptions, reasoningParams, admitReasoning } from './core/reasoning.js';
export { Channel } from './core/transport.js';
export { pinnedRequest } from './v3.js';
export { ReferenceClient, CatalogFailure, GatewayError, gatewayError, CompletionFailure, consumeCompletion, validateModels } from './core/client.js';
export { IntakeFailure, intakeDiagnostic, intakeDescription } from './core/intake-diagnostics.js';
export { Identity } from 'ehbp';
export { ChannelError, DiagnosticFailure, OperationFailure, Operation, LIMITS, JSONDepthError } from './core/limits.js';
export { snapshotInvocation } from './core/tools.js';
