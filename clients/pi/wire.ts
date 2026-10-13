import { getCurrentTools, type Model, type TranscriptContext, type Tool } from '@earendil-works/pi-ai';
import { convertMessages } from '@earendil-works/pi-ai/api/openai-completions';
import type { Chat } from '../../examples/phase01/transport.js';
import { DiagnosticFailure, requireThat } from '../../examples/phase01/limits.js';

// Reuse Pi's supported conversion, but not its weaker completion/usage parser.
// No inferred finish, cache markers, reasoning replay or synthetic image fallback.
const compat: Parameters<typeof convertMessages>[2] = {
  supportsStore: false, supportsDeveloperRole: false, supportsReasoningEffort: false,
  supportsUsageInStreaming: true, supportsFinishReason: true, maxTokensField: 'max_tokens',
  requiresToolResultName: false, requiresAssistantAfterToolResult: false,
  requiresThinkingAsText: false, requiresReasoningContentOnAssistantMessages: false,
  thinkingFormat: 'openai', chatTemplateKwargs: {}, chatTemplateArgs: {},
  openRouterRouting: {}, vercelGatewayRouting: {}, zaiToolStream: false,
  supportsOpenAIGrammarTools: false, supportsStrictMode: false,
  supportsMidConvoSystemMessages: false, supportsMidConvoToolAdditions: false,
  sendSessionAffinityHeaders: false, sessionAffinityFormat: 'openai', supportsLongCacheRetention: false,
};

export type Invocation = Omit<Chat, 'stream' | 'submission'>;

export function invocation(model: Model<'openai-completions'>, context: TranscriptContext): Invocation {
  requireThat(Array.isArray(context?.messages), 'history');
  for (const message of context.messages) {
    if (message.role === 'assistant' && (message.provider !== 'possums' || message.api !== 'openai-completions')) {
      throw new Error('possums_mixed_provider_history');
    }
    if (message.role === 'user' || message.role === 'toolResult') {
      if (Array.isArray(message.content) && message.content.some((block: { type: string }) => block.type !== 'text')) {
        throw new Error('possums_images_unsupported');
      }
    }
    if (message.role === 'assistant' && message.content.some((block: { type: string; namespace?: string }) => block.type === 'toolCall' && block.namespace !== undefined)) {
      throw new Error('possums_custom_tools_unsupported');
    }
  }
  let tools: Tool[];
  try { tools = getCurrentTools(context.messages); }
  catch { throw new DiagnosticFailure('request', 'conversion'); }
  for (const tool of tools) {
    if ('namespace' in tool && tool.namespace !== undefined) throw new Error('possums_custom_tools_unsupported');
    if (tool.constrainedSampling && (tool.constrainedSampling.type !== 'json_schema' || tool.constrainedSampling.strict === 'require')) {
      throw new Error('possums_constrained_sampling_unsupported');
    }
  }
  let converted: ReturnType<typeof convertMessages>;
  try { converted = convertMessages(model, context, compat); }
  catch { throw new DiagnosticFailure('request', 'conversion'); }
  // Pi represents text-only user blocks as an array. Collapse only those; do not
  // stringify multimodal/structured content into something the model can execute.
  const messages = converted.map((convertedMessage: ReturnType<typeof convertMessages>[number]) => {
    // Thinking is display-only, including signed native history.
    const { reasoning: _reasoning, reasoning_content: _reasoningContent,
      reasoning_text: _reasoningText, reasoning_details: _reasoningDetails, ...message } = convertedMessage as typeof convertedMessage & {
        reasoning?: unknown; reasoning_content?: unknown; reasoning_text?: unknown; reasoning_details?: unknown };
    if (Array.isArray(message.content)) {
      if (message.content.some((block: { type: string }) => block.type !== 'text')) throw new Error('possums_images_unsupported');
      return { ...message, content: message.content.map((block: { type: string; text?: string }) => block.text ?? '').join('') };
    }
    // OpenAI permits null here, but the upstream tokenizer rejects tool-only
    // assistant history with null content. Empty text preserves the same turn.
    if (message.role === 'assistant' && message.content === null && message.tool_calls?.length) {
      return { ...message, content: '' };
    }
    return message;
  });
  return {
    model: model.id, stream: true, messages,
    ...(tools.length ? { tools: tools.map((tool: Tool) => ({ type: 'function', function: {
      name: tool.name, parameters: tool.parameters,
      ...(tool.description === undefined ? {} : { description: tool.description }),
    } })) } : {}),
  } as Invocation;
}
