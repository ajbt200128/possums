import type { Model, ThinkingLevelMap } from '@earendil-works/pi-ai';
import type { LiveModel } from './core/client.js';
import { resolveReasoning, type ReasoningOptions } from './core/reasoning.js';

const levels = ['off', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max'] as const;
export function thinkingMetadata(entry: LiveModel): Pick<Model<'openai-completions'>, 'reasoning' | 'thinkingLevelMap'> {
  const map: ThinkingLevelMap = Object.fromEntries(levels.map(level => [level, null]));
  const templates = entry.reasoning === true ? entry.reasoning_params?.params['/v1/chat/completions'] : undefined;
  if (resolveReasoning(templates?.disable)) map.off = 'off';
  if (templates?.enable) {
    for (const level of ['low', 'medium', 'high'] as const) {
      const effort = entry.reasoning_params?.effort_map?.[level];
      if (effort && resolveReasoning(templates.enable, effort)) map[level] = effort;
    }
    // A non-placeholder enable template is a toggle, not an effort scale.
    if (!Object.values(map).some(value => value !== null && value !== 'off') && resolveReasoning(templates.enable)) map.high = 'on';
  }
  return { reasoning: Object.values(map).some(value => value !== null && value !== 'off'), thinkingLevelMap: map };
}
export function simpleThinking(entry: LiveModel, level: unknown): ReasoningOptions {
  const templates = entry.reasoning === true ? entry.reasoning_params?.params['/v1/chat/completions'] : undefined;
  // Pi 1.0.4 Agent maps native off to undefined. This rule is deliberately
  // restricted to streamSimple; raw stream omission preserves upstream defaults.
  if (level === undefined) return resolveReasoning(templates?.disable) ?? {};
  const metadata = thinkingMetadata(entry);
  if (!metadata.reasoning) return {};
  if (typeof level !== 'string' || !levels.includes(level as typeof levels[number]) || level === 'off' ||
      metadata.thinkingLevelMap?.[level as keyof ThinkingLevelMap] == null) throw new Error('possums_request_options_unsupported');
  const effort = entry.reasoning_params?.effort_map?.[level as 'low' | 'medium' | 'high'];
  return resolveReasoning(templates?.enable, effort) ?? {};
}
