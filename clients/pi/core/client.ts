import { Channel, hasByteReadTimeout, type ResponseOptions } from './transport.js';
import { intakeDiagnostic, intakeDescription, type IntakeDiagnostic } from './intake-diagnostics.js';
import type { V3Input, TorV3Input } from '../v3.js';
import type { TorConnection } from '../tor-connection.js';
import { TorNativeTransportFailure, torNativeTransportDiagnostic, type TorNativeDiagnostic } from '../tor-native-transport.mjs';
import { ChannelError, DiagnosticFailure, LIMITS, Operation, OperationFailure, cleanup, parseJSON, requireThat, serialize,
  type FailureConstraint, type FailureStage } from './limits.js';
import { admitReasoning, reasoningParams, type ReasoningParams, type ReasoningOptions } from './reasoning.js';
import { fields, freezeJSON, snapshotInvocation,
  type Chat, type Invocation, type Tool, type ToolChoice } from './tools.js';

// Events are provisional display data, NEVER permission to execute a tool.
// Execution needs caller policy checks AND a settled receipt + DONE + authenticated EOF.
export type CompletionEvent = Readonly<{
  object: 'chat.completion.chunk'; model: string;
  choices: readonly Readonly<{ index: 0; delta: Readonly<{ role?: 'assistant' | null; content?: string | null;
    reasoning?: string | null; reasoning_content?: string | null;
    tool_calls?: readonly Readonly<{ index: number; id?: string | null; type?: 'function' | null;
      function?: Readonly<{ name?: string | null; arguments?: string | null }> | null }>[] | null }>;
    finish_reason?: Receipt['finish'] | null }>[];
  usage?: Readonly<{ prompt_tokens: number; completion_tokens: number; total_tokens: number }>;
  possums?: Readonly<{ outcome: 'settled' | 'refunded'; status?: 'complete' | 'incomplete'; charged_microunits: string; refunded_microunits: string;
    quoted_input_microunits_per_million_tokens?: string; quoted_output_microunits_per_million_tokens?: string }>;
}>;
export type CompletionOptions = { signal?: AbortSignal; tools?: Tool[];
  onEvent?: (event: CompletionEvent) => void | Promise<void> };
export type ChatOptions = ResponseOptions & CompletionOptions & ReasoningOptions & { tool_choice?: ToolChoice;
  onPayload?: (payload: Readonly<Invocation>) => unknown | Promise<unknown> };

export type LiveModel = { object: 'model'; id: string; name?: string; context_tokens: string; max_output_tokens: string;
  input_microunits_per_million_tokens: string; output_microunits_per_million_tokens: string; maximum_reservation_microunits: string;
  tool_protocol?: 'openai-functions-v1'; cached_input_microunits_per_million_tokens?: string | null;
  reasoning?: boolean; reasoning_params?: ReasoningParams };
export type Receipt = { finish?: 'stop' | 'length' | 'tool_calls'; inputTokens: number; outputTokens: number; totalTokens: number;
  chargedMicrounits: string; refundedMicrounits?: string;
  quotedInputMicrounitsPerMillion?: string; quotedOutputMicrounitsPerMillion?: string;
  complete?: boolean; outcome?: 'settled' | 'refunded'; reservedMicrounits?: string; cachedTokens?: number; quotedCachedInputMicrounitsPerMillion?: string | null; evidence?: string;
  usageAvailable?: boolean; deliveryComplete?: boolean };
export type Completion = Partial<Receipt> & { finish: NonNullable<Receipt['finish']> };
function closedTorNativeDiagnostic(observed?: Readonly<TorNativeDiagnostic>): Readonly<TorNativeDiagnostic> {
  // Only the allowlisted kind selects locally authored fields. Neither mutable
  // diagnostic replacements nor raw exception/cause/stack become retained data.
  try {
    const code = observed?.code;
    const kind = typeof code === 'string' && code.startsWith('possums_tor_native_')
      ? code.slice('possums_tor_native_'.length) : undefined;
    return torNativeTransportDiagnostic(new TorNativeTransportFailure(kind));
  } catch { return torNativeTransportDiagnostic(new TorNativeTransportFailure('ipc_unknown')); }
}
export function safeTorNativeDiagnostic(error: TorNativeTransportFailure): Readonly<TorNativeDiagnostic> {
  try { return closedTorNativeDiagnostic(torNativeTransportDiagnostic(error)); }
  catch { return closedTorNativeDiagnostic(); }
}
const interruptedReceipts = new WeakMap<CompletionFailure, Readonly<Receipt>>();
const nativeDiagnostics = new WeakMap<CompletionFailure, Readonly<TorNativeDiagnostic>>();
export class CompletionFailure extends DiagnosticFailure {
  constructor(stage: FailureStage, constraint: FailureConstraint, receipt?: Readonly<Receipt>, status?: number,
    native?: Readonly<TorNativeDiagnostic>) {
    super(stage, constraint, 'uncertain', status);
    if (receipt) interruptedReceipts.set(this, Object.freeze({ ...receipt, deliveryComplete: false }));
    if (native) nativeDiagnostics.set(this, closedTorNativeDiagnostic(native));
  }
  get settlement(): Readonly<Receipt> | undefined { return interruptedReceipts.get(this); }
  get nativeDiagnostic(): Readonly<TorNativeDiagnostic> | undefined { return nativeDiagnostics.get(this); }
}
function usageMetadata(value: any) {
  const input = count(value?.prompt_tokens), output = count(value?.completion_tokens), sum = count(value?.total_tokens);
  return { inputTokens: input ?? 0, outputTokens: output ?? 0, totalTokens: sum ?? 0,
    cachedTokens: count(value?.cached_tokens ?? value?.prompt_tokens_details?.cached_tokens),
    usageAvailable: input !== undefined && output !== undefined && sum !== undefined };
}
const token = (value: unknown): value is string => typeof value === 'string' && /^[A-Za-z0-9_-]{43}$/.test(value);
function keys(value: any, names: readonly string[]): void {
  requireThat(value && typeof value === 'object' && !Array.isArray(value));
  requireThat(names.every(key => Object.hasOwn(value, key)));
}
function amount(value: unknown): bigint {
  requireThat(typeof value === 'string' && /^(0|[1-9][0-9]{0,19})$/.test(value));
  const result = BigInt(value);
  requireThat(result <= (1n << 64n) - 1n);
  return result;
}
// Response fields are forward-compatible; request fields remain closed authority.
function responseFields(value: any, required: readonly string[], optional: readonly string[] = []): any {
  keys(value, required);
  return Object.fromEntries([...required, ...optional].filter(key => Object.hasOwn(value, key)).map(key => [key, value[key]]));
}
function optionalAmount(value: unknown): string | undefined {
  try { amount(value); return value as string; } catch { return undefined; }
}
const count = (value: unknown): number | undefined => Number.isSafeInteger(value) && (value as number) >= 0 ? value as number : undefined;
// Optional upstream display text is untrusted presentation data. Reject empty,
// oversized or control/escape/bidi-format strings; invalid names fall back to id.
const displayName = (value: unknown): value is string => typeof value === 'string' && value.length > 0 &&
  [...value].length <= 128 && !/[\u0000-\u001f\u007f-\u009f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/.test(value);
export function validateModels(value: any): readonly LiveModel[] {
  value = parseJSON(serialize(value, LIMITS.catalog), LIMITS.catalog);
  keys(value, ['object', 'data']);
  requireThat(value.object === 'list' && Array.isArray(value.data) && value.data.length > 0 && value.data.length <= LIMITS.models);
  const ids = new Set<string>();
  value.data = value.data.map((model: unknown) => responseFields(model, ['object', 'id', 'context_tokens', 'max_output_tokens', 'input_microunits_per_million_tokens', 'output_microunits_per_million_tokens', 'maximum_reservation_microunits'], ['name', 'tool_protocol', 'cached_input_microunits_per_million_tokens', 'reasoning', 'reasoning_params']));
  for (const model of value.data) {
    if (Object.hasOwn(model, 'name') && !displayName(model.name)) delete model.name;
    if (Object.hasOwn(model, 'tool_protocol')) requireThat(model.tool_protocol === 'openai-functions-v1');
    requireThat(model.object === 'model' && typeof model.id === 'string' && /^[A-Za-z0-9._:/-]{1,128}$/.test(model.id) && !ids.has(model.id));
    ids.add(model.id);
    const context = amount(model.context_tokens), output = amount(model.max_output_tokens);
    amount(model.input_microunits_per_million_tokens); amount(model.output_microunits_per_million_tokens);
    requireThat(context > 0n && output > 0n);
    if (model.cached_input_microunits_per_million_tokens != null) amount(model.cached_input_microunits_per_million_tokens);
    amount(model.maximum_reservation_microunits);
    if (typeof model.reasoning !== 'boolean') delete model.reasoning;
    const metadata = reasoningParams(model.reasoning_params);
    delete model.reasoning_params;
    if (metadata) model.reasoning_params = freezeJSON(metadata);
    Object.freeze(model);
  }
  return Object.freeze(value.data);
}

// This list is the closed gateway detail vocabulary. Never render server-provided
// messages or an unrecognized detail, even when it resembles an internal code.
const details = new Set(`images_unsupported reasoning_parameters_unsupported inference_unavailable upstream_response_invalid verification_failed catalog_failed request_encoding_failed
  tool_profile_unqualified tokenizer_send_failed tokenizer_http_failed tokenizer_response_invalid tokenizer_upload_incomplete
  generation_send_failed generation_http_failed endpoint_binding_failed stream_content_type_invalid stream_transport_failed
  stream_idle_timeout stream_deadline_exceeded sdk_stream_decode_failed upstream_error_event stream_event_schema_invalid
  stream_choice_invalid stream_delta_unsupported tool_index_invalid tool_identity_invalid tool_name_not_allowed
  tool_choice_violated tool_call_incomplete tool_arguments_too_large tool_state_too_large stream_finish_invalid stream_finish_missing
  stream_usage_invalid stream_usage_unexpected stream_usage_missing stream_output_after_finish settlement_failed`.split(/\s+/));
export function gatewayDetailLabel(detail: string | undefined): string | undefined {
  if (detail === 'images_unsupported') return '[images_unsupported] Image input is unsupported for this model; remove images or deliberately select an image-capable model';
  return detail !== undefined && details.has(detail) ? `[${detail}] ${detail.replaceAll('_', ' ')}` : undefined;
}
const gatewayReasons = new Set(['insufficient_credit', 'unauthorized', 'invalid_request', 'account_limit', 'duplicate_request', 'unavailable', 'generation_failed', 'service_quiescing']);
const claimedRefunds = new WeakSet<GatewayError>();
const confirmedRefunds = new WeakSet<GatewayError>();
const confirmedSettlements = new WeakMap<GatewayError, Readonly<Receipt>>();
const intakeDiagnostics = new WeakMap<GatewayError, IntakeDiagnostic>();
function gatewayDescription(reason: string, detail: string | undefined, refunded: boolean, receipt?: Readonly<Receipt>, intake?: IntakeDiagnostic): string {
  const reasons: Readonly<Record<string, string>> = {
    insufficient_credit: 'Insufficient credit for this model’s maximum reservation.',
    unauthorized: 'Gateway rejected authentication; use /login to authenticate again.',
    invalid_request: 'Request rejected by gateway.', account_limit: 'Account limit reached.',
    duplicate_request: 'Duplicate submission rejected.', unavailable: 'Gateway unavailable.',
    generation_failed: 'Generation failed.', service_quiescing: 'Gateway is quiescing; wait for service availability before making a new request.',
  };
  const label = gatewayDetailLabel(detail);
  const known = Object.hasOwn(reasons, reason);
  return `Possums: ${known ? `[${reason}] ${reasons[reason]}` : '[possums_gateway_unclassified] Gateway returned an unclassified category; cause unknown.'}` +
    (label ? ` ${label}.` : '') + (intake ? ` ${intakeDescription(intake)}` : '') + ` Stage: ${reason === 'service_quiescing' ? 'admission' : detail === 'images_unsupported' ? 'request' : 'gateway'}; constraint: ${label ? detail : known ? reason : 'unclassified'}. Share only this content-free code/stage/constraint for support.` + (receipt ? ` Authenticated gateway receipt (${receipt.complete === false ? 'incomplete' : 'complete'}): outcome ${receipt.outcome ?? 'unavailable'}; charged ${receipt.chargedMicrounits} microunits; refunded ${receipt.refundedMicrounits ?? 'unavailable'} microunits. Receipt alone does not establish successful generation or authorize tool execution. Not replayed. A new request may incur another charge.` : refunded ? ' Reservation refunded. Not replayed.' :
      ' Charge unknown. Not replayed. A new request may incur another charge.');
}
export class GatewayError extends ChannelError {
  readonly status?: number;
  readonly reason: string;
  readonly detail?: string;
  constructor(reason: string, detail?: string, _claimedBilling: 'refunded' | 'unknown' = 'unknown', status?: number, intake?: unknown) {
    super('uncertain');
    this.reason = gatewayReasons.has(reason) ? reason : 'unclassified';
    this.detail = detail === undefined ? undefined : details.has(detail) ? detail : 'unclassified';
    if (Number.isInteger(status) && status! >= 100 && status! <= 599) this.status = status;
    const diagnostic = intakeDiagnostic(intake);
    if (diagnostic) intakeDiagnostics.set(this, diagnostic);
    this.message = gatewayDescription(this.reason, this.detail, false, undefined, diagnostic);
  }
  get intakeDiagnostic(): IntakeDiagnostic | undefined { return intakeDiagnostics.get(this); }
  get billing(): 'refunded' | 'settled' | 'unknown' { const receipt = confirmedSettlements.get(this); return receipt ? receipt.outcome ?? 'unknown' : confirmedRefunds.has(this) ? 'refunded' : 'unknown'; }
  get settlement(): Readonly<Receipt> | undefined { return confirmedSettlements.get(this); }
}
export function gatewayError(value: any, status?: number): GatewayError {
  keys(value, ['error']);
  if (value.error?.code === 'service_quiescing') {
    // The pre-admission wire envelope is distinct from ordinary gateway errors.
    // Its billing field is not an accounting receipt.
    keys(value.error, ['code', 'stage', 'constraint', 'billing', 'message']);
    requireThat(status === 503 && value.error.stage === 'admission' &&
      value.error.constraint === 'service_quiescing' && value.error.billing === 'not_submitted' &&
      typeof value.error.message === 'string');
    return new GatewayError('service_quiescing', undefined, 'unknown', status);
  }
  responseFields(value.error, ['code'], ['detail', 'message', 'billing', 'outcome']);
  const intake = intakeDiagnostic(value.error.intake);
  requireThat(typeof value.error.code === 'string' &&
    (gatewayReasons.has(value.error.code) || intake !== undefined) && value.error.code !== 'service_quiescing');
  requireThat(value.error.detail === undefined || (typeof value.error.detail === 'string' && details.has(value.error.detail)));
  requireThat(value.error.message === undefined || typeof value.error.message === 'string');
  requireThat(value.error.billing === undefined || value.error.billing === 'refunded' || value.error.billing === 'unknown');
  requireThat(value.error.outcome === undefined || (value.error.code === 'duplicate_request' &&
    typeof value.error.outcome === 'string' && ['in_flight', 'settled', 'refunded'].includes(value.error.outcome)));
  const failure = new GatewayError(value.error.code, value.error.detail, 'unknown', status, intake);
  if (gatewayReasons.has(value.error.code) && value.error.billing === 'refunded') claimedRefunds.add(failure);
  return failure;
}

export type CatalogFailureStage = 'request' | 'http' | 'body' | 'validation' | 'conversion';
// Catalog diagnostics retain only closed stages and validated observations, never
// the response, original exception or inference billing claims.
export class CatalogFailure extends ChannelError {
  readonly status?: number;
  readonly reason?: string;
  readonly detail?: string;
  readonly intakeDiagnostic?: IntakeDiagnostic;
  constructor(readonly stage: CatalogFailureStage, code: ChannelError['code'] = 'rejected', status?: number, response?: unknown) {
    super(code);
    requireThat(['request', 'http', 'body', 'validation', 'conversion'].includes(stage));
    requireThat(code === 'rejected' || code === 'uncertain');
    this.message = `catalog_${stage}_failed`;
    if (stage === 'http' && Number.isInteger(status) && status! >= 100 && status! <= 599) {
      this.status = status;
      try {
        const failure = gatewayError(response, status);
        this.reason = failure.reason; this.detail = failure.detail;
        this.intakeDiagnostic = failure.intakeDiagnostic;
      } catch { /* An unreadable/unknown error body does not erase the HTTP status. */ }
    }
    Object.freeze(this);
  }
}

// Success requires the ordered, authenticated gateway receipt AND stream EOF.
// No answer is accumulated; interruption is uncertain and never causes a resend.
export async function consumeCompletion(body: ReadableStream<Uint8Array>, model: string | LiveModel,
  onDelta: (text: string) => void, options: CompletionOptions = {}, httpStatus?: number): Promise<Completion> {
  let reader: ReadableStreamDefaultReader<Uint8Array>, op: Operation;
  try { reader = body.getReader(); op = new Operation(null, options.signal); }
  catch { throw new DiagnosticFailure('stream', 'envelope', 'uncertain'); }
  const decoder = new TextDecoder('utf-8', { fatal: true });
  let pending = '', data: string | undefined, state: 'role' | 'text' | 'usage' | 'done' | 'eof' | 'error' = 'role';
  let finish: Receipt['finish'] | undefined, receipt: Receipt | undefined, sawEvent = false, controlError = false;
  let usage = usageMetadata(undefined);
  let failure: GatewayError | undefined, terminal = false;
  let stage: FailureStage = 'stream', constraint: FailureConstraint = 'envelope';
  let modelID: string;
  async function event(payload: string): Promise<void> {
    constraint = 'envelope';
    sawEvent = true;
    if (payload === '[DONE]') {
      constraint = state === 'role' || state === 'text' ? 'finish_missing' : 'usage_missing';
      requireThat(state === 'done' || (state === 'usage' && finish)); state = 'eof'; return;
    }
    requireThat(state !== 'eof' && state !== 'error');
    constraint = 'json';
    let value: any;
    try { value = JSON.parse(payload); }
    catch { throw new DiagnosticFailure('stream', 'json', 'uncertain'); }
    constraint = 'schema';
    requireThat(value && typeof value === 'object');
    if (Object.hasOwn(value, 'error')) { failure = gatewayError(value); state = receipt ? 'done' : 'error'; return; }
    requireThat(value.object === 'chat.completion.chunk' && value.model === modelID && Array.isArray(value.choices));
    let textDelta: string | undefined;
    value = responseFields(value, ['object', 'model', 'choices'], ['usage', 'possums']);
    if (value.possums !== undefined) {
      constraint = 'usage';
      requireThat(!receipt && value.choices.length === 0);
      const accounting = value.possums;
      usage = usageMetadata(accounting.usage ?? value.usage);
      constraint = 'receipt';
      requireThat(accounting && typeof accounting === 'object' && !Array.isArray(accounting));
      const charged = optionalAmount(accounting.charged_microunits);
      // Optional/unreadable metadata cannot erase readable authenticated money.
      if (charged !== undefined) {
        receipt = { finish, ...usage,
          chargedMicrounits: charged, refundedMicrounits: optionalAmount(accounting.refunded_microunits),
          complete: accounting.status === undefined || accounting.status === 'complete',
          outcome: accounting.outcome === 'settled' || accounting.outcome === 'refunded' ? accounting.outcome : undefined,
          reservedMicrounits: optionalAmount(accounting.reserved_microunits),
          quotedInputMicrounitsPerMillion: optionalAmount(accounting.quoted_input_microunits_per_million_tokens),
          quotedOutputMicrounitsPerMillion: optionalAmount(accounting.quoted_output_microunits_per_million_tokens),
          quotedCachedInputMicrounitsPerMillion: optionalAmount(accounting.quoted_cached_input_microunits_per_million_tokens),
          evidence: ['none', 'running_sse', 'final_sse', 'final_metrics'].includes(accounting.evidence) ? accounting.evidence : undefined };
      }
      // Only normalized, understood response data reaches hooks.
      value.possums = { outcome: receipt?.outcome, status: receipt?.complete ? 'complete' : 'incomplete',
        charged_microunits: receipt?.chargedMicrounits, refunded_microunits: receipt?.refundedMicrounits };
      delete value.usage;
      state = 'done';
    } else {
      constraint = 'choice';
      requireThat(value.choices.length <= 1 && !receipt && state !== 'done');
      if (value.usage != null) {
        usage = usageMetadata(value.usage);
        value.usage = { prompt_tokens: usage.inputTokens, completion_tokens: usage.outputTokens, total_tokens: usage.totalTokens };
      }
      if (value.choices.length === 0) { requireThat(value.usage != null); return; }
      constraint = 'choice';
      requireThat(state === 'role' || state === 'text');
      const choice = responseFields(value.choices[0], ['index', 'delta'], ['finish_reason']);
      value.choices = [choice];
      requireThat(choice.index === 0);
      constraint = 'delta';
      choice.delta = responseFields(choice.delta, [], ['role', 'content', 'tool_calls', 'reasoning', 'reasoning_content']);
      if (choice.delta.role != null) requireThat(choice.delta.role === 'assistant');
      state = 'text';
      if (choice.delta.content != null) { requireThat(typeof choice.delta.content === 'string'); textDelta = choice.delta.content; }
      for (const key of ['reasoning', 'reasoning_content']) requireThat(choice.delta[key] == null || typeof choice.delta[key] === 'string');
      if (Object.hasOwn(choice.delta, 'tool_calls') && choice.delta.tool_calls != null) {
        constraint = 'tool';
        requireThat(Array.isArray(choice.delta.tool_calls));
        choice.delta.tool_calls = choice.delta.tool_calls.map((delta: unknown) => responseFields(delta, ['index'], ['id', 'type', 'function']));
        const indexes = new Set<number>();
        for (const delta of choice.delta.tool_calls) {
          requireThat(delta && Number.isSafeInteger(delta.index) && delta.index >= 0 && !indexes.has(delta.index));
          indexes.add(delta.index);
          // Fragments are forwarded, not accumulated or authorized here.
          // Pi assembles calls and applies lookup/schema policy before execution.
          if (delta.type != null) requireThat(delta.type === 'function');
          if (delta.id != null) requireThat(typeof delta.id === 'string');
          if (delta.function != null) {
            delta.function = responseFields(delta.function, [], ['name', 'arguments']);
            if (delta.function.name != null) requireThat(typeof delta.function.name === 'string');
            if (delta.function.arguments != null) requireThat(typeof delta.function.arguments === 'string');
          }
        }
      }
      if (choice.finish_reason != null) {
        constraint = 'finish';
        requireThat(choice.finish_reason === 'tool_calls' || choice.finish_reason === 'stop' || choice.finish_reason === 'length');
        finish = choice.finish_reason; state = 'usage';
      }
    }
    // Validate the WHOLE event before exposing any of its deltas. Hooks cannot
    // mutate parser state, and awaiting them bounds queueing/backpressure.
    freezeJSON(value);
    op.check();
    if (textDelta !== undefined) {
      stage = 'hook'; constraint = 'unexpected';
      try { await op.wait(Promise.resolve().then(() => onDelta(textDelta)), LIMITS.idleMs); }
      catch (error) { if (error instanceof OperationFailure) throw error; throw new DiagnosticFailure('hook', 'unexpected', 'uncertain'); }
      stage = 'stream';
    }
    if (options.onEvent) {
      op.check(); stage = 'hook'; constraint = 'unexpected';
      try { await op.wait(Promise.resolve().then(() => options.onEvent!(value)), LIMITS.idleMs); }
      catch (error) { if (error instanceof OperationFailure) throw error; throw new DiagnosticFailure('hook', 'unexpected', 'uncertain'); }
      stage = 'stream';
    }
  }
  async function line(value: string): Promise<void> {
    if (value.endsWith('\r')) value = value.slice(0, -1);
    if (value.startsWith(':')) { requireThat(state !== 'error'); return; }
    if (!value) { if (data !== undefined) { const payload = data; data = undefined; await event(payload); } return; }
    requireThat(state !== 'eof' && state !== 'error' && data === undefined && value.startsWith('data: '));
    data = value.slice(6);
  }
  function atEOF(): boolean { return state === 'eof'; }
  try {
    op.check();
    stage = 'request'; constraint = 'schema';
    modelID = typeof model === 'string' ? model : validateModels({ object: 'list', data: [model] })[0].id;
    stage = 'stream'; constraint = 'envelope';
    for (;;) {
      op.check();
      // Channel bodies time HTTP bytes, not decrypted frames or visible text.
      let next: ReadableStreamReadResult<Uint8Array>;
      try { next = await op.wait(reader.read(), hasByteReadTimeout(body) ? null : LIMITS.idleMs); }
      catch (error) {
        if (error instanceof OperationFailure || error instanceof DiagnosticFailure) throw error;
        if (error instanceof TorNativeTransportFailure) throw new CompletionFailure(atEOF() ? 'stream' : 'transport',
          atEOF() ? 'eof' : 'fetch', undefined, httpStatus, safeTorNativeDiagnostic(error));
        throw new DiagnosticFailure(atEOF() ? 'stream' : 'transport', atEOF() ? 'eof' : 'fetch', 'uncertain');
      }
      if (next.done) {
        constraint = 'utf8'; requireThat(decoder.decode() === '');
        // Only a Channel-produced, SDK-decrypted body may carry HTTP admission status.
        if (controlError) { constraint = 'json'; failure = gatewayError(JSON.parse(pending), hasByteReadTimeout(body) ? httpStatus : undefined); }
        else { constraint = 'envelope'; requireThat(pending === '' && data === undefined); }
        if (receipt && (receipt.complete === false || receipt.outcome === 'refunded') && !failure) failure = new GatewayError('generation_failed', 'stream_finish_missing');
        if (failure) {
          terminal = true;
          if (receipt && atEOF()) {
            confirmedSettlements.set(failure, Object.freeze({ ...receipt, deliveryComplete: true }));
            failure.message = gatewayDescription(failure.reason, failure.detail, false, receipt, failure.intakeDiagnostic);
          } else if (claimedRefunds.has(failure)) {
            confirmedRefunds.add(failure);
            failure.message = gatewayDescription(failure.reason, failure.detail, true, undefined, failure.intakeDiagnostic);
          }
          throw failure;
        }
        constraint = !finish ? 'finish_missing' : !atEOF() ? 'done_missing' : 'eof';
        requireThat(atEOF() && finish && (!receipt || (receipt.complete !== false && receipt.outcome === 'settled')));
        return Object.freeze(receipt ? { ...receipt, finish, deliveryComplete: true } : { ...usage, finish });
      }
      constraint = 'envelope';
      constraint = 'utf8'; pending += decoder.decode(next.value, { stream: true });
      constraint = 'envelope';
      if (!sawEvent && !failure && pending.trimStart().startsWith('{')) controlError = true;
      if (controlError) continue;
      let end: number;
      while ((end = pending.indexOf('\n')) !== -1) {
        const current = pending.slice(0, end); pending = pending.slice(end + 1); await line(current);
      }
    }
  } catch (error) {
    if (terminal && error === failure) throw failure;
    // A later integrity failure must not be hidden by an earlier operational
    // error frame. Keep received money, but preserve the fatal trust category.
    if (error instanceof DiagnosticFailure && (error.stage === 'trust' || ['endpoint_binding', 'decryption', 'frames'].includes(error.constraint))) {
      throw new CompletionFailure(error.stage, error.constraint, receipt, error.status ?? httpStatus,
        error instanceof CompletionFailure ? error.nativeDiagnostic : undefined);
    }
    if (failure) {
      const interrupted = new GatewayError(failure.reason, failure.detail, 'unknown', failure.status, failure.intakeDiagnostic);
      if (receipt) confirmedSettlements.set(interrupted, Object.freeze({ ...receipt, deliveryComplete: false }));
      throw interrupted; // An error's refund claim alone still needs authenticated EOF.
    }
    if (error instanceof DiagnosticFailure) throw new CompletionFailure(error.stage, error.constraint, receipt, error.status ?? httpStatus,
      error instanceof CompletionFailure ? error.nativeDiagnostic : undefined);
    if (error instanceof OperationFailure) throw new CompletionFailure(stage, error.constraint, receipt, httpStatus);
    throw new CompletionFailure(stage, constraint, receipt, httpStatus);
  }
  finally { op.close(); await cleanup(reader.cancel()); }
}

type PreparedState = {
  payload?: Invocation; options?: ResponseOptions & Pick<CompletionOptions, 'onEvent'>; onDelta?: (text: string) => void;
  model: string; newConversation: boolean; dispatched: boolean; busy: boolean; closed: boolean;
};
const preparedStates = new WeakMap<PreparedChat, PreparedState>();
const preparedAuthority = Symbol('prepared chat authority');
// Only preparation creates this handle. Close it when recovery is abandoned;
// dispatch and ordinary chat close it automatically, releasing prompt references.
export class PreparedChat {
  constructor(authority: symbol, state: PreparedState) {
    requireThat(authority === preparedAuthority);
    preparedStates.set(this, state);
    Object.freeze(this);
  }
  get dispatched(): boolean { return preparedStates.get(this)?.dispatched ?? false; }
  close(): void {
    const state = preparedStates.get(this);
    if (state) { state.closed = true; state.payload = undefined; state.options = undefined; state.onDelta = undefined; }
  }
}

export class ReferenceClient {
  #bearer: string | undefined;
  #authExpiresAt: number | undefined;
  constructor(readonly channel: Channel) { Channel.requireVerified(channel); }
  static async verified(bundle: Uint8Array, manifest: Uint8Array<ArrayBuffer>, keyConfig: Uint8Array): Promise<ReferenceClient> {
    return new ReferenceClient(await Channel.api(bundle, manifest, keyConfig));
  }
  static async published(bundle: Uint8Array, manifest: Uint8Array<ArrayBuffer>, keyConfig: Uint8Array, signal?: AbortSignal): Promise<ReferenceClient> {
    return new ReferenceClient(await Channel.published(bundle, manifest, keyConfig, signal));
  }
  static async v3(input: V3Input | TorV3Input, signal?: AbortSignal, scope?: TorConnection): Promise<ReferenceClient> {
    return new ReferenceClient(await Channel.v3(input, signal, scope));
  }
  get release() { return this.channel.release; }
  get authExpiresAt(): number | undefined { return this.#authExpiresAt; }
  freshSession(): ReferenceClient { return new ReferenceClient(this.channel); }
  async login(credential: string, signal?: AbortSignal): Promise<void> {
    this.#bearer = undefined;
    this.#authExpiresAt = undefined;
    let op: Operation;
    try { op = new Operation(2 * LIMITS.operationMs, signal); }
    catch { throw new DiagnosticFailure('challenge', 'schema'); }
    let stage: 'challenge' | 'authentication' = 'challenge', constraint: FailureConstraint = 'fetch';
    let started = false;
    try {
      op.check(); started = true;
      const challenge: any = await this.channel.challenge(op.controller.signal);
      constraint = 'schema';
      keys(challenge, ['challenge', 'expires_in']); requireThat(token(challenge.challenge) && challenge.expires_in === 600);
      op.check(); stage = 'authentication'; constraint = 'fetch';
      // Conservative local bound: issuance cannot precede this send.
      const issuedAfter = Date.now();
      const session: any = await this.channel.control('/v1/sessions', { challenge: challenge.challenge, credential }, undefined, op.controller.signal);
      constraint = 'schema';
      keys(session, ['token', 'token_type', 'expires_in']);
      requireThat(token(session.token) && session.token_type === 'Bearer' && session.expires_in === 43200);
      op.check(); this.#bearer = session.token;
      this.#authExpiresAt = issuedAfter + session.expires_in * 1000;
    } catch (error) {
      if (error instanceof DiagnosticFailure || error instanceof CatalogFailure || error instanceof GatewayError) throw error;
      if (error instanceof TorNativeTransportFailure) throw new CompletionFailure(stage, constraint, undefined, undefined, safeTorNativeDiagnostic(error));
      if (error instanceof OperationFailure) throw new DiagnosticFailure(stage, error.constraint, started ? 'uncertain' : 'rejected');
      throw new DiagnosticFailure(stage, constraint, op.controller.signal.aborted || (error instanceof ChannelError && error.code === 'uncertain') ? 'uncertain' : 'rejected');
    } finally { op.close(); }
  }
  async models(signal?: AbortSignal): Promise<readonly LiveModel[]> {
    let value: unknown;
    try {
      requireThat(this.#bearer); value = await this.channel.models(this.#bearer, signal);
    } catch (error) {
      if (error instanceof CatalogFailure || error instanceof DiagnosticFailure) throw error;
      if (error instanceof TorNativeTransportFailure) throw new CompletionFailure('catalog', 'fetch', undefined, undefined, safeTorNativeDiagnostic(error));
      throw new CatalogFailure('request', error instanceof ChannelError ? error.code : 'rejected');
    }
    try { return validateModels(value); }
    catch { throw new CatalogFailure('validation'); }
  }
  async chat(model: string, messages: Chat['messages'], onDelta: (text: string) => void, newConversation = false,
    options: ChatOptions = {}): Promise<Completion> {
    const bearer = this.#bearer;
    const prepared = await this.prepareChat(model, messages, onDelta, newConversation, options);
    try { return await this.#chatPrepared(prepared, bearer); }
    finally { prepared.close(); }
  }
  async prepareChat(model: string, messages: Chat['messages'], onDelta: (text: string) => void, newConversation = false,
    options: ChatOptions = {}): Promise<PreparedChat> {
    let opts: ChatOptions, op: Operation;
    try {
      requireThat(this.#bearer && typeof newConversation === 'boolean' && typeof onDelta === 'function');
      // Copy option descriptors once. No arbitrary request options / fetch / headers.
      opts = fields(options, [], ['signal', 'tools', 'tool_choice', 'reasoning_effort', 'chat_template_kwargs', 'onPayload', 'onResponse', 'onEvent']);
      for (const key of ['onPayload', 'onResponse', 'onEvent'] as const) requireThat(opts[key] === undefined || typeof opts[key] === 'function');
      op = new Operation(null, opts.signal);
    } catch { throw new DiagnosticFailure('request', 'schema'); }
    let stage: FailureStage = 'request', constraint: FailureConstraint = 'encoding';
    try {
      op.check();
      let payload = snapshotInvocation({ model, messages, stream: true,
        ...(opts.tools !== undefined ? { tools: opts.tools } : {}), ...(opts.tool_choice !== undefined ? { tool_choice: opts.tool_choice } : {}),
        ...(opts.reasoning_effort !== undefined ? { reasoning_effort: opts.reasoning_effort } : {}),
        ...(opts.chat_template_kwargs !== undefined ? { chat_template_kwargs: opts.chat_template_kwargs } : {}) });
      if (opts.onPayload) {
        stage = 'hook'; constraint = 'unexpected';
        let replacement: unknown;
        try { replacement = await op.wait(Promise.resolve().then(() => opts.onPayload!(payload)), LIMITS.idleMs); }
        catch (error) { if (error instanceof OperationFailure) throw error; throw new DiagnosticFailure('hook', 'unexpected'); }
        stage = 'request'; constraint = 'encoding';
        // Returning undefined keeps the immutable original; replacements are
        // re-admitted with the same model and cannot smuggle transport options.
        if (replacement !== undefined) payload = snapshotInvocation(replacement);
        requireThat(payload.model === model);
      }
      op.check();
      return new PreparedChat(preparedAuthority, { payload,
        options: Object.freeze({ signal: opts.signal, onResponse: opts.onResponse, onEvent: opts.onEvent }), onDelta,
        model, newConversation, dispatched: false, busy: false, closed: false });
    } catch (error) {
      if (error instanceof DiagnosticFailure) throw error;
      if (error instanceof OperationFailure) throw new DiagnosticFailure(stage, error.constraint);
      throw new DiagnosticFailure(stage, constraint);
    } finally { op.close(); }
  }
  async chatPrepared(prepared: PreparedChat, beforeDispatch?: () => void): Promise<Completion> {
    return this.#chatPrepared(prepared, this.#bearer, beforeDispatch);
  }
  async #chatPrepared(prepared: PreparedChat, bearer: string | undefined, beforeDispatch?: () => void): Promise<Completion> {
    // Acquire synchronously, before any await. The state is module-owned rather
    // than a mutable caller-supplied boolean, including across client rotations.
    const state = preparedStates.get(prepared);
    if (!state || state.closed || state.dispatched || state.busy) throw new DiagnosticFailure('request', 'schema');
    state.busy = true;
    let op: Operation;
    try { requireThat(bearer); op = new Operation(null, state.options!.signal); }
    catch { state.busy = false; throw new DiagnosticFailure('request', 'schema'); }
    const { payload, options: opts, onDelta, model, newConversation } = state;
    let issued = false, hookThrown = false, guardThrown = false, stage: FailureStage = 'request', constraint: FailureConstraint = 'encoding';
    let status: number | undefined;
    try {
      op.check(); stage = 'catalog'; constraint = 'fetch';
      // Refresh, never cache a usable offline catalog or substitute a cheaper model.
      // One invocation uses one in-memory session even if another login overlaps.
      const catalog = await this.channel.models(bearer, op.controller.signal);
      constraint = 'schema';
      const entry = validateModels(catalog).find(entry => entry.id === model);
      requireThat(entry);
      stage = 'request'; constraint = 'schema';
      admitReasoning(entry, { ...(Object.hasOwn(payload!, 'reasoning_effort') ? { reasoning_effort: payload!.reasoning_effort } : {}),
        ...(Object.hasOwn(payload!, 'chat_template_kwargs') ? { chat_template_kwargs: payload!.chat_template_kwargs } : {}) });
      const usesTools = payload!.tools !== undefined || payload!.messages.some(message => message.role === 'tool' || (message.role === 'assistant' && message.tool_calls !== undefined));
      requireThat(!usesTools || entry.tool_protocol === 'openai-functions-v1');
      op.check(); requireThat(!state.closed); issued = true; stage = 'submission'; constraint = 'fetch';
      const issuance: any = await this.channel.control('/v1/submissions', { model, new_conversation: newConversation }, bearer, op.controller.signal);
      constraint = 'schema';
      keys(issuance, ['submission']); requireThat(token(issuance.submission));
      op.check(); requireThat(!state.closed); stage = 'transport'; constraint = 'fetch';
      // The owner checks session/account/run and cancellation synchronously at
      // the last possible boundary, after all awaited setup and hooks.
      try { beforeDispatch?.(); }
      catch (error) { guardThrown = true; throw error; }
      op.check(); requireThat(!state.closed);
      state.dispatched = true;
      const body = await this.channel.chat({ ...payload!, submission: issuance.submission }, bearer,
        { signal: op.controller.signal, onResponse: async info => {
          if (Number.isInteger(info.status) && info.status >= 100 && info.status <= 599) status = info.status;
          if (!opts!.onResponse) return;
          try { await opts!.onResponse!(info); }
          catch { hookThrown = true; throw new DiagnosticFailure('hook', 'unexpected', 'uncertain', status); }
        } });
      stage = 'stream';
      return await consumeCompletion(body, entry, onDelta!, { signal: op.controller.signal, onEvent: opts!.onEvent }, status);
    } catch (error) {
      if (error instanceof GatewayError) {
        // Preserve the same authenticated refund carrier; copying would lose its evidence.
        if (status !== undefined) Object.defineProperty(error, 'status', { value: status });
        throw error;
      }
      if (error instanceof CatalogFailure) throw error;
      if (hookThrown) throw new DiagnosticFailure('hook', 'unexpected', 'uncertain', status);
      if (error instanceof CompletionFailure) throw new CompletionFailure(error.stage, error.constraint, error.settlement, error.status ?? status, error.nativeDiagnostic);
      if (!guardThrown && error instanceof TorNativeTransportFailure) throw new CompletionFailure(stage, constraint, undefined, status, safeTorNativeDiagnostic(error));
      if (error instanceof DiagnosticFailure) {
        if ((issued && error.code !== 'uncertain') || (error.status === undefined && status !== undefined)) {
          throw new DiagnosticFailure(error.stage, error.constraint, issued ? 'uncertain' : error.code, error.status ?? status);
        }
        throw error;
      }
      if (error instanceof OperationFailure) throw new DiagnosticFailure(stage, error.constraint, issued ? 'uncertain' : 'rejected');
      throw new DiagnosticFailure(stage, constraint,
        issued || (error instanceof ChannelError && error.code === 'uncertain') ? 'uncertain' : 'rejected');
    } finally { op.close(); state.busy = false; if (state.dispatched) prepared.close(); }
  }
}
