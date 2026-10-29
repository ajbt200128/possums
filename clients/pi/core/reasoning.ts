import { fields } from './tools.js';
import { DiagnosticFailure, requireThat } from './limits.js';

export type ReasoningOptions = { reasoning_effort?: string; chat_template_kwargs?: Record<string, boolean | string> };
export type ReasoningParams = { effort_map?: Partial<Record<'low' | 'medium' | 'high', string>>;
  params: { '/v1/chat/completions': { enable?: ReasoningOptions; disable?: ReasoningOptions } } };
const efforts = new Set(['none', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max']);
export function reasoningOptions(value: unknown, placeholder = false): ReasoningOptions {
  try { return supportedOptions(value, placeholder); }
  catch { throw new DiagnosticFailure('request', 'reasoning'); }
}
function supportedOptions(value: unknown, placeholder: boolean): ReasoningOptions {
  const options = fields(value, [], ['reasoning_effort', 'chat_template_kwargs']);
  const effort = (value: unknown) => typeof value === 'string' && (efforts.has(value) || (placeholder && value === '$EFFORT'));
  if (Object.hasOwn(options, 'reasoning_effort')) requireThat(effort(options.reasoning_effort));
  if (Object.hasOwn(options, 'chat_template_kwargs')) {
    const kwargs = fields(options.chat_template_kwargs, [], ['thinking', 'enable_thinking', 'reasoning_effort']);
    requireThat(Object.keys(kwargs).length > 0);
    for (const [key, value] of Object.entries(kwargs)) requireThat(key === 'reasoning_effort' ? effort(value) : typeof value === 'boolean');
    options.chat_template_kwargs = kwargs;
  }
  return options;
}
// Unrecognized metadata disables controls, not ordinary default inference.
export function reasoningParams(value: unknown): ReasoningParams | undefined {
  try {
    const metadata = fields(value, ['params'], ['effort_map']);
    const params = fields(metadata.params, ['/v1/chat/completions']);
    const templates = fields(params['/v1/chat/completions'], [], ['enable', 'disable']);
    requireThat(Object.keys(templates).length > 0);
    for (const key of ['enable', 'disable']) if (Object.hasOwn(templates, key)) {
      templates[key] = reasoningOptions(templates[key], key === 'enable');
      requireThat(Object.keys(templates[key]).length > 0);
    }
    if (Object.hasOwn(metadata, 'effort_map')) {
      metadata.effort_map = fields(metadata.effort_map, [], ['low', 'medium', 'high']);
      requireThat(Object.keys(metadata.effort_map).length > 0 && Object.values(metadata.effort_map).every(value => typeof value === 'string' && efforts.has(value)));
    }
    return { ...metadata, params: { '/v1/chat/completions': templates } };
  } catch { return undefined; }
}
export function resolveReasoning(template: ReasoningOptions | undefined, effort?: string): ReasoningOptions | undefined {
  if (!template) return undefined;
  const options = structuredClone(template);
  if (options.reasoning_effort === '$EFFORT') {
    if (effort === undefined) return undefined;
    options.reasoning_effort = effort;
  }
  if (options.chat_template_kwargs?.reasoning_effort === '$EFFORT') {
    if (effort === undefined) return undefined;
    options.chat_template_kwargs.reasoning_effort = effort;
  }
  return options;
}
function sameOptions(a: ReasoningOptions, b: ReasoningOptions): boolean {
  if (a.reasoning_effort !== b.reasoning_effort) return false;
  const left = a.chat_template_kwargs, right = b.chat_template_kwargs;
  if (!left || !right) return left === right;
  return Object.keys(left).length === Object.keys(right).length && Object.entries(left).every(([key, value]) => Object.hasOwn(right, key) && right[key] === value);
}
export function admitReasoning(model: { reasoning?: boolean; reasoning_params?: ReasoningParams }, value: ReasoningOptions): void {
  const options = reasoningOptions(value);
  if (Object.keys(options).length === 0) return;
  const templates = model.reasoning_params?.params['/v1/chat/completions'];
  requireThat(model.reasoning === true && templates, 'reasoning');
  const candidates = [templates.enable, templates.disable].flatMap(template => [resolveReasoning(template),
    ...Object.values(model.reasoning_params?.effort_map ?? {}).map(effort => resolveReasoning(template, effort))]);
  requireThat(candidates.some(candidate => candidate && sameOptions(candidate, options)), 'reasoning');
}
