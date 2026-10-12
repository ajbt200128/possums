import { ChannelError, DiagnosticFailure, JSONDepthError, LIMITS, mapData, parseJSON, requireThat, serialize, utf8, type FailureConstraint } from './limits.js';

export type JSONObject = { [key: string]: JSONValue };
export type JSONValue = null | boolean | number | string | JSONValue[] | JSONObject;
export type Tool = { type: 'function'; function: { name: string; description?: string; parameters: JSONObject } };
export type ToolChoice = 'auto' | 'none' | 'required' | { type: 'function'; function: { name: string } };
export type ToolCall = { id: string; type: 'function'; function: { name: string; arguments: string } };
export type Message = { role: 'system' | 'user'; content: string } |
  { role: 'assistant'; content: string | null; tool_calls?: ToolCall[] } |
  { role: 'tool'; content: string; tool_call_id: string };
export type Invocation = { model: string; stream: true; messages: Message[]; tools?: Tool[]; tool_choice?: ToolChoice };
export type Chat = Invocation & { submission: string };

// Validate own descriptors before touching values, including optional fields.
export function fields(value: unknown, required: readonly string[], optional: readonly string[] = []): any {
  requireThat(!Array.isArray(value));
  const result = mapData(value, required.length + optional.length, (key, child) => {
    requireThat(required.includes(key) || optional.includes(key));
    return child;
  });
  for (const key of required) requireThat(Object.hasOwn(result, key));
  return result;
}
export function toolName(value: unknown, constraint?: FailureConstraint): asserts value is string {
  requireThat(typeof value === 'string' && /^[A-Za-z0-9_-]{1,64}$/.test(value), constraint);
}
export function toolID(value: unknown, constraint?: FailureConstraint): asserts value is string {
  requireThat(typeof value === 'string' && /^[A-Za-z0-9_-]{1,128}$/.test(value), constraint);
}
export function objectArguments(value: unknown, cap = LIMITS.chat): number {
  requireThat(typeof value === 'string' && value.length <= cap);
  const bytes = utf8.encode(value);
  const parsed = parseJSON(bytes, cap);
  requireThat(parsed !== null && typeof parsed === 'object' && !Array.isArray(parsed));
  // Reject JSON numeric overflow as well as duplicate keys / excessive structure.
  serialize(parsed, cap);
  return bytes.length;
}
export function freezeJSON<T>(value: T): T {
  if (value !== null && typeof value === 'object') {
    for (const child of Object.values(value)) freezeJSON(child);
    Object.freeze(value);
  }
  return value;
}
// Add context to plain rejections; preserve specific and unexpected failures
// for the owning boundary to classify.
function requestPart<T>(constraint: FailureConstraint, validate: () => T): T {
  try { return validate(); }
  catch (error) {
    if (error instanceof ChannelError && error.code === 'rejected' && !(error instanceof DiagnosticFailure) && !(error instanceof JSONDepthError)) {
      throw new DiagnosticFailure('request', constraint);
    }
    throw error;
  }
}
export function admitTools(value: unknown): Tool[] {
  requireThat(Array.isArray(value) && value.length > 0, 'tool_schema');
  const names = new Set<string>();
  let schemaBytes = 0;
  const result = requestPart('tool_schema', () => mapData(value, LIMITS.nodes, (_key, item) => {
    const tool = fields(item, ['type', 'function']);
    requireThat(tool.type === 'function', 'tool_schema');
    const fn = fields(tool.function, ['name', 'parameters'], ['description']);
    toolName(fn.name, 'tool_name'); requireThat(!names.has(fn.name), 'tool_name'); names.add(fn.name);
    if (Object.hasOwn(fn, 'description')) requireThat(typeof fn.description === 'string', 'tool_schema');
    // The descriptor-aware serializer snapshots the entire schema, not just its root.
    const encoded = serialize(fn.parameters, LIMITS.chat - schemaBytes);
    schemaBytes += encoded.length;
    const schema = parseJSON(encoded, LIMITS.chat);
    // Schemas rooted in a local $ref or combinator need not repeat type: object.
    requireThat(schema && typeof schema === 'object' && !Array.isArray(schema) &&
      (!Object.hasOwn(schema, 'type') || schema.type === 'object'), 'tool_schema');
    function refs(node: any): void {
      if (!node || typeof node !== 'object') return;
      if (Object.hasOwn(node, '$ref')) requireThat(typeof node.$ref === 'string' && node.$ref.startsWith('#'), 'schema_reference');
      // Resource rebasing/dynamic references would make a local-looking $ref external.
      requireThat(!Object.hasOwn(node, '$id') && !Object.hasOwn(node, '$dynamicRef') && !Object.hasOwn(node, '$recursiveRef'), 'schema_reference');
      for (const child of Object.values(node)) refs(child);
    }
    refs(schema); fn.parameters = schema; tool.function = fn;
    return tool;
  }));
  return result;
}
function namesOf(tools?: Tool[]): Set<string> | undefined {
  return tools && new Set(Array.from(tools, tool => tool.function.name));
}
// Also used before the payload hook: no submission or credentials enter a logical invocation.
export function admitInvocation(value: unknown, withSubmission = false): any {
  const chat = fields(value, withSubmission ? ['model', 'stream', 'submission', 'messages'] : ['model', 'stream', 'messages'], ['tools', 'tool_choice']);
  requireThat(chat.stream === true && typeof chat.model === 'string' && /^[A-Za-z0-9._:/-]{1,128}$/.test(chat.model));
  if (withSubmission) requireThat(typeof chat.submission === 'string' && /^[A-Za-z0-9_-]{43}$/.test(chat.submission));
  if (Object.hasOwn(chat, 'tools')) chat.tools = admitTools(chat.tools);
  const names = namesOf(chat.tools);
  if (Object.hasOwn(chat, 'tool_choice')) {
    requireThat(names, 'tool_choice');
    if (typeof chat.tool_choice === 'string') requireThat(['auto', 'none', 'required'].includes(chat.tool_choice), 'tool_choice');
    else {
      const choice = requestPart('tool_choice', () => fields(chat.tool_choice, ['type', 'function']));
      choice.function = requestPart('tool_choice', () => fields(choice.function, ['name']));
      requireThat(choice.type === 'function' && names.has(choice.function.name), 'tool_choice');
      chat.tool_choice = choice;
    }
  }
  requireThat(Array.isArray(chat.messages), 'history');
  const ids = new Set<string>(), pending = new Set<string>();
  let finalRole = '', finalContent = '';
  chat.messages = requestPart('history', () => mapData(chat.messages, LIMITS.nodes, (_key, value) => {
    const message = fields(value, ['role', 'content'], ['tool_calls', 'tool_call_id']);
    const role = message.role;
    requireThat(['system', 'user', 'assistant', 'tool'].includes(role), 'history');
    requireThat((typeof message.content === 'string' && message.content.length <= LIMITS.chat) ||
      (role === 'assistant' && message.content === null && Object.hasOwn(message, 'tool_calls')), 'history');
    requireThat(!Object.hasOwn(message, 'tool_calls') || role === 'assistant', 'history');
    requireThat(!Object.hasOwn(message, 'tool_call_id') || role === 'tool', 'history');
    if (role === 'tool') {
      toolID(message.tool_call_id, 'tool_id');
      requireThat(pending.delete(message.tool_call_id), 'history_order');
    } else {
      requireThat(pending.size === 0, 'history_order');
      if (Object.hasOwn(message, 'tool_calls')) {
        requireThat(Array.isArray(message.tool_calls) && message.tool_calls.length > 0, 'history');
        message.tool_calls = mapData(message.tool_calls, LIMITS.nodes, (_key, value) => {
          const call = fields(value, ['id', 'type', 'function']);
          toolID(call.id, 'tool_id'); requireThat(!ids.has(call.id), 'history_order');
          requireThat(call.type === 'function', 'history');
          ids.add(call.id); pending.add(call.id);
          call.function = fields(call.function, ['name', 'arguments']);
          // Historical calls are transcript data, not authorization to execute
          // a tool still present in the current loadout. Live calls are gated separately.
          toolName(call.function.name, 'tool_name');
          requestPart('tool_arguments', () => objectArguments(call.function.arguments, LIMITS.chat));
          return call;
        });
      }
    }
    finalRole = role; finalContent = message.content;
    return message;
  }));
  requireThat(pending.size === 0, 'history_order');
  requireThat((finalRole === 'user' && finalContent.trim().length > 0) || finalRole === 'tool', 'history');
  return chat;
}
export function snapshotInvocation(value: unknown): Invocation {
  return freezeJSON(parseJSON(serialize(admitInvocation(value), LIMITS.chat), LIMITS.chat));
}
