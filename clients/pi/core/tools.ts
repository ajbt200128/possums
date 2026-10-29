import type { ReasoningOptions } from './reasoning.js';
import { DiagnosticFailure, mapData, requireThat } from './limits.js';

export type JSONObject = { [key: string]: JSONValue };
export type JSONValue = null | boolean | number | string | JSONValue[] | JSONObject;
export type Tool = { type: 'function'; function: { name: string; description?: string; parameters: JSONObject } };
export type ToolChoice = 'auto' | 'none' | 'required' | { type: 'function'; function: { name: string } };
export type ToolCall = { id: string; type: 'function'; function: { name: string; arguments: string } };
// Reviewed Tinfoil vision profile; catalog membership still gates dispatch.
export const supportsImageInput = (model: string): boolean =>
  ['deepseek-v4-1-flash', 'kimi-k3', 'gemma4-31b'].includes(model);
export type UserContent = string | ({ type: 'text'; text: string } | { type: 'image_url'; image_url: { url: string; detail?: 'auto' | 'low' | 'high' } })[];
export type Message = { role: 'system'; content: string } | { role: 'user'; content: UserContent } |
  { role: 'assistant'; content: string | null; tool_calls?: ToolCall[] } |
  { role: 'tool'; content: string; tool_call_id: string };
export type Invocation = ReasoningOptions & { model: string; stream: true; messages: Message[]; tools?: Tool[]; tool_choice?: ToolChoice };
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
export function freezeJSON<T>(value: T): T {
  if (value !== null && typeof value === 'object') {
    for (const child of Object.values(value)) freezeJSON(child);
    Object.freeze(value);
  }
  return value;
}
// Only the dispatch envelope is client authority. Transcript/tool semantics are
// owned by Pi (or the reference caller) and the gateway, not admitted again here.
export function admitInvocation(value: unknown, withSubmission = false): any {
  const chat = fields(value, withSubmission ? ['model', 'stream', 'submission', 'messages'] : ['model', 'stream', 'messages'], ['tools', 'tool_choice', 'reasoning_effort', 'chat_template_kwargs']);
  requireThat(chat.stream === true && typeof chat.model === 'string' && /^[A-Za-z0-9._:/-]{1,128}$/.test(chat.model));
  if (withSubmission) requireThat(typeof chat.submission === 'string' && /^[A-Za-z0-9_-]{43}$/.test(chat.submission));
  requireThat(Array.isArray(chat.messages), 'history');
  return chat;
}
export function snapshotInvocation(value: unknown): Invocation {
  // Hooks receive JSON data, never credentials/submission or mutable dispatch
  // authority. Recheck the encoded envelope so a nested toJSON cannot change it.
  try {
    const envelope = fields(value, ['model', 'stream', 'messages'], ['tools', 'tool_choice', 'reasoning_effort', 'chat_template_kwargs']);
    return freezeJSON(admitInvocation(JSON.parse(JSON.stringify(envelope))));
  } catch (error) {
    throw error instanceof DiagnosticFailure ? error : new DiagnosticFailure('request', 'encoding');
  }
}
