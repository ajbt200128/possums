// Explicit test entry only. build.mjs never emits or distributes this bundle.
export { default as extension } from './index.js';
export { PossumsProvider } from './provider.js';
export { ReplayGuard } from './replay.js';
export { invocation } from './wire.js';
export { Channel } from '../../examples/phase01/transport.js';
export { ReferenceClient, GatewayError, consumeCompletion, validateModels } from '../../examples/phase01/client.js';
export { ChannelError } from '../../examples/phase01/limits.js';
