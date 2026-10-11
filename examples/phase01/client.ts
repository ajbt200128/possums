import { Channel, hasByteReadTimeout, type ResponseOptions } from './transport.js';
import { ChannelError, DiagnosticFailure, JSONDepthError, LIMITS, Operation, OperationFailure, cleanup, parseJSON, requireThat, serialize, utf8,
  type FailureConstraint, type FailureStage } from './limits.js';
import { admitTools, fields, freezeJSON, objectArguments, snapshotInvocation, toolID, toolName,
  type Chat, type Invocation, type Tool, type ToolChoice } from './tools.js';

// Events are provisional display data, NEVER permission to execute a tool.
// Execution needs caller policy checks AND a settled receipt + DONE + authenticated EOF.
export type CompletionEvent = Readonly<{
  object: 'chat.completion.chunk'; model: string;
  choices: readonly Readonly<{ index: 0; delta: Readonly<{ role?: 'assistant' | null; content?: string | null;
    tool_calls?: readonly Readonly<{ index: number; id?: string | null; type?: 'function' | null;
      function?: Readonly<{ name?: string | null; arguments?: string | null }> | null }>[] | null }>;
    finish_reason?: Receipt['finish'] | null }>[];
  usage?: Readonly<{ prompt_tokens: number; completion_tokens: number; total_tokens: number }>;
  possums?: Readonly<{ outcome: 'settled' | 'refunded'; status?: 'complete' | 'incomplete'; charged_microunits: string; refunded_microunits: string;
    quoted_input_microunits_per_million_tokens?: string; quoted_output_microunits_per_million_tokens?: string }>;
}>;
export type CompletionOptions = { signal?: AbortSignal; tools?: Tool[];
  onEvent?: (event: CompletionEvent) => void | Promise<void> };
export type ChatOptions = ResponseOptions & CompletionOptions & { tool_choice?: ToolChoice;
  onPayload?: (payload: Readonly<Invocation>) => unknown | Promise<unknown> };

export type LiveModel = { object: 'model'; id: string; context_tokens: string; max_output_tokens: string;
  input_microunits_per_million_tokens: string; output_microunits_per_million_tokens: string; maximum_reservation_microunits: string;
  tool_protocol?: 'openai-functions-v1'; cached_input_microunits_per_million_tokens?: string | null };
export type Receipt = { finish: 'stop' | 'length' | 'tool_calls'; inputTokens: number; outputTokens: number; totalTokens: number;
  chargedMicrounits: string; refundedMicrounits: string;
  quotedInputMicrounitsPerMillion?: string; quotedOutputMicrounitsPerMillion?: string;
  complete?: boolean; outcome?: 'settled' | 'refunded'; reservedMicrounits?: string; cachedTokens?: number; quotedCachedInputMicrounitsPerMillion?: string | null; evidence?: string };
export type BalanceSnapshot = Readonly<{ availableMicrounits: string; inFlight: number; completedRequests: string }>;
export class BalanceFailure extends ChannelError {
  constructor(readonly stage: 'request' | 'validation', code: ChannelError['code'] = 'rejected', readonly observation?: DiagnosticFailure) {
    super(code);
    this.message = `possums_balance_${stage}_failed`;
    Object.freeze(this);
  }
}
const token = (value: unknown): value is string => typeof value === 'string' && /^[A-Za-z0-9_-]{43}$/.test(value);
function keys(value: any, names: readonly string[]): void {
  requireThat(value && typeof value === 'object' && !Array.isArray(value));
  const actual = Object.keys(value);
  requireThat(actual.length === names.length && actual.every(key => names.includes(key)));
}
function amount(value: unknown): bigint {
  requireThat(typeof value === 'string' && /^(0|[1-9][0-9]{0,19})$/.test(value));
  const result = BigInt(value);
  requireThat(result <= (1n << 64n) - 1n);
  return result;
}
export function validateModels(value: any): readonly LiveModel[] {
  value = parseJSON(serialize(value, LIMITS.catalog), LIMITS.catalog);
  keys(value, ['object', 'data']);
  requireThat(value.object === 'list' && Array.isArray(value.data) && value.data.length > 0 && value.data.length <= LIMITS.models);
  const ids = new Set<string>();
  for (const model of value.data) {
    fields(model, ['object', 'id', 'context_tokens', 'max_output_tokens', 'input_microunits_per_million_tokens', 'output_microunits_per_million_tokens', 'maximum_reservation_microunits'], ['tool_protocol', 'cached_input_microunits_per_million_tokens']);
    if (Object.hasOwn(model, 'tool_protocol')) requireThat(model.tool_protocol === 'openai-functions-v1');
    requireThat(model.object === 'model' && typeof model.id === 'string' && /^[A-Za-z0-9._:/-]{1,128}$/.test(model.id) && !ids.has(model.id));
    ids.add(model.id);
    const context = amount(model.context_tokens), output = amount(model.max_output_tokens);
    const inputPrice = amount(model.input_microunits_per_million_tokens), outputPrice = amount(model.output_microunits_per_million_tokens);
    requireThat(context > 0n && output > 0n && output <= context && inputPrice > 0n && outputPrice > 0n);
    if (model.cached_input_microunits_per_million_tokens != null) requireThat(amount(model.cached_input_microunits_per_million_tokens) <= inputPrice);
    const quote = (130n * (context * inputPrice + output * outputPrice) + 100_000_000n - 1n) / 100_000_000n;
    requireThat(amount(model.maximum_reservation_microunits) === quote);
    Object.freeze(model);
  }
  return Object.freeze(value.data);
}

// This list is the closed gateway detail vocabulary. Never render server-provided
// messages or an unrecognized detail, even when it resembles an internal code.
const details = new Set(`inference_unavailable upstream_response_invalid verification_failed catalog_failed request_encoding_failed
  tool_profile_unqualified tokenizer_send_failed tokenizer_http_failed tokenizer_response_invalid tokenizer_upload_incomplete
  generation_send_failed generation_http_failed endpoint_binding_failed stream_content_type_invalid stream_transport_failed
  stream_idle_timeout stream_deadline_exceeded sdk_stream_decode_failed upstream_error_event stream_event_schema_invalid
  stream_choice_invalid stream_delta_unsupported tool_index_invalid tool_identity_invalid tool_name_not_allowed
  tool_choice_violated tool_call_incomplete tool_arguments_too_large tool_state_too_large stream_finish_invalid stream_finish_missing
  stream_usage_invalid stream_usage_unexpected stream_usage_missing stream_output_after_finish settlement_failed`.split(/\s+/));
export function gatewayDetailLabel(detail: string | undefined): string | undefined {
  return detail !== undefined && details.has(detail) ? `[${detail}] ${detail.replaceAll('_', ' ')}` : undefined;
}
const gatewayReasons = new Set(['insufficient_credit', 'unauthorized', 'invalid_request', 'account_limit', 'duplicate_request', 'unavailable', 'generation_failed', 'service_quiescing']);
const claimedRefunds = new WeakSet<GatewayError>();
const confirmedRefunds = new WeakSet<GatewayError>();
const confirmedSettlements = new WeakMap<GatewayError, Readonly<Receipt>>();
function gatewayDescription(reason: string, detail: string | undefined, refunded: boolean, receipt?: Readonly<Receipt>): string {
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
    (label ? ` ${label}.` : '') + ` Stage: ${reason === 'service_quiescing' ? 'admission' : 'gateway'}; constraint: ${label ? detail : known ? reason : 'unclassified'}. Share only this content-free code/stage/constraint for support.` + (receipt ? ` Authenticated ${receipt.complete === false ? 'incomplete' : 'complete'} settlement: charged ${receipt.chargedMicrounits} microunits; refunded ${receipt.refundedMicrounits} microunits. Settlement alone does not authorize tool execution. Not replayed. A new request may incur another charge.` : refunded ? ' Reservation refunded. Not replayed.' :
      ' Charge unknown. Not replayed. A new request may incur another charge.');
}
export class GatewayError extends ChannelError {
  readonly status?: number;
  readonly reason: string;
  readonly detail?: string;
  constructor(reason: string, detail?: string, _claimedBilling: 'refunded' | 'unknown' = 'unknown', status?: number) {
    super('uncertain');
    this.reason = gatewayReasons.has(reason) ? reason : 'unclassified';
    this.detail = detail === undefined ? undefined : details.has(detail) ? detail : 'unclassified';
    if (Number.isInteger(status) && status! >= 100 && status! <= 599) this.status = status;
    this.message = gatewayDescription(this.reason, this.detail, false);
  }
  get billing(): 'refunded' | 'settled' | 'unknown' { const receipt = confirmedSettlements.get(this); return receipt ? receipt.outcome ?? 'settled' : confirmedRefunds.has(this) ? 'refunded' : 'unknown'; }
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
  fields(value.error, ['code'], ['detail', 'message', 'billing', 'outcome']);
  requireThat(typeof value.error.code === 'string' &&
    gatewayReasons.has(value.error.code) && value.error.code !== 'service_quiescing');
  requireThat(value.error.detail === undefined || (typeof value.error.detail === 'string' && details.has(value.error.detail)));
  requireThat(value.error.message === undefined || typeof value.error.message === 'string');
  requireThat(value.error.billing === undefined || value.error.billing === 'refunded' || value.error.billing === 'unknown');
  requireThat(value.error.outcome === undefined || (value.error.code === 'duplicate_request' &&
    typeof value.error.outcome === 'string' && ['in_flight', 'settled', 'refunded'].includes(value.error.outcome)));
  const failure = new GatewayError(value.error.code, value.error.detail);
  if (value.error.billing === 'refunded') claimedRefunds.add(failure);
  return failure;
}

export type CatalogFailureStage = 'request' | 'http' | 'body' | 'validation' | 'conversion';
// Catalog diagnostics retain only closed stages and validated observations, never
// the response, original exception or inference billing claims.
export class CatalogFailure extends ChannelError {
  readonly status?: number;
  readonly reason?: string;
  readonly detail?: string;
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
      } catch { /* An unreadable/unknown error body does not erase the HTTP status. */ }
    }
    Object.freeze(this);
  }
}

// Success requires the ordered, authenticated gateway receipt AND stream EOF.
// No answer is accumulated; interruption is uncertain and never causes a resend.
export async function consumeCompletion(body: ReadableStream<Uint8Array>, model: string | LiveModel,
  onDelta: (text: string) => void, options: CompletionOptions = {}, httpStatus?: number): Promise<Receipt> {
  let reader: ReadableStreamDefaultReader<Uint8Array>, op: Operation;
  try { reader = body.getReader(); op = new Operation(null, options.signal); }
  catch { throw new DiagnosticFailure('stream', 'envelope', 'uncertain'); }
  const decoder = new TextDecoder('utf-8', { fatal: true });
  let pending = '', data: string | undefined, state: 'role' | 'text' | 'usage' | 'done' | 'eof' | 'error' = 'role';
  let finish: Receipt['finish'] | undefined, receipt: Receipt | undefined, sawEvent = false, controlError = false;
  let failure: GatewayError | undefined, terminal = false;
  let stage: FailureStage = 'stream', constraint: FailureConstraint = 'envelope';
  let modelID: string, names: Set<string> | undefined;
  const calls: { id: string; name: string; arguments: string }[] = [], ids = new Set<string>();
  // Shared encoded-state allowance, derived from the request envelope rather
  // than a tool count/per-call quota. Six bytes per UTF-16 unit cover worst-case
  // UTF-8 plus old/new string overlap; 256 covers records, slots and identity-set
  // bookkeeping conservatively. Engine/parser/callback memory is separate, so
  // this is not a whole-runtime RSS bound. Charge before retaining/concatenating.
  const toolRecordRetainedBytes = 256, toolStringRetainedBytesPerUnit = 6;
  let retainedBytes = 0;
  function retain(bytes: number): void {
    if (bytes > LIMITS.chat - retainedBytes) throw new DiagnosticFailure('stream', 'tool_state', 'uncertain');
    retainedBytes += bytes;
  }
  async function event(payload: string): Promise<void> {
    constraint = 'envelope';
    sawEvent = true;
    if (payload === '[DONE]') {
      constraint = state === 'role' || state === 'text' ? 'finish_missing' : 'usage_missing';
      requireThat(state === 'done'); state = 'eof'; return;
    }
    requireThat(state !== 'eof' && state !== 'error');
    constraint = 'json';
    const value = parseJSON(utf8.encode(payload), LIMITS.sseEvent);
    constraint = 'schema';
    requireThat(value && typeof value === 'object');
    if (Object.hasOwn(value, 'error')) { failure = gatewayError(value); state = receipt ? 'done' : 'error'; return; }
    requireThat(value.object === 'chat.completion.chunk' && value.model === modelID && Array.isArray(value.choices));
    let textDelta: string | undefined;
    fields(value, ['object', 'model', 'choices'], ['usage', 'possums']);
    if (value.possums !== undefined) {
      constraint = 'usage';
      requireThat(!receipt && value.choices.length === 0);
      const counts = value.possums.usage ?? value.usage ?? { prompt_tokens: 0, completion_tokens: 0, total_tokens: 0 };
      fields(counts, ['prompt_tokens', 'completion_tokens', 'total_tokens'], ['cached_tokens', 'prompt_tokens_details', 'completion_tokens_details', 'cost_usd']);
      const { prompt_tokens: input, completion_tokens: output, total_tokens: sum } = counts;
      requireThat([input, output, sum].every(n => Number.isSafeInteger(n) && n >= 0) && input + output === sum);
      constraint = 'receipt';
      fields(value.possums, ['outcome', 'charged_microunits', 'refunded_microunits'],
        ['quoted_input_microunits_per_million_tokens', 'quoted_output_microunits_per_million_tokens', 'quoted_cached_input_microunits_per_million_tokens', 'status', 'reserved_microunits', 'markup_percent', 'evidence', 'usage', 'provider_cost_usd']);
      requireThat(value.possums.outcome === 'settled' || value.possums.outcome === 'refunded');
      const complete = value.possums.status === undefined || value.possums.status === 'complete';
      requireThat(value.possums.status === undefined || complete || value.possums.status === 'incomplete');
      requireThat(!complete || (finish && value.possums.outcome === 'settled'));
      if (value.possums.status !== undefined) {
        requireThat(value.possums.markup_percent === 130 && ['none', 'running_sse', 'final_sse', 'final_metrics'].includes(value.possums.evidence));
        requireThat(amount(value.possums.reserved_microunits) === amount(value.possums.charged_microunits) + amount(value.possums.refunded_microunits));
      }
      if (value.possums.outcome === 'refunded') requireThat(amount(value.possums.charged_microunits) === 0n);
      amount(value.possums.charged_microunits); amount(value.possums.refunded_microunits);
      const cached = counts.cached_tokens ?? counts.prompt_tokens_details?.cached_tokens ?? 0;
      requireThat(Number.isSafeInteger(cached) && cached >= 0 && cached <= input);
      receipt = { finish: finish ?? 'stop', inputTokens: input, outputTokens: output, totalTokens: sum,
        chargedMicrounits: value.possums.charged_microunits, refundedMicrounits: value.possums.refunded_microunits,
        complete, outcome: value.possums.outcome, reservedMicrounits: value.possums.reserved_microunits, cachedTokens: cached,
        quotedCachedInputMicrounitsPerMillion: value.possums.quoted_cached_input_microunits_per_million_tokens, evidence: value.possums.evidence };
      const quotedInput = value.possums.quoted_input_microunits_per_million_tokens;
      const quotedOutput = value.possums.quoted_output_microunits_per_million_tokens;
      if (quotedInput !== undefined || quotedOutput !== undefined) {
        const inputRate = amount(quotedInput), outputRate = amount(quotedOutput);
        requireThat(inputRate > 0n && outputRate > 0n);
        const cachedRate = value.possums.quoted_cached_input_microunits_per_million_tokens == null ? inputRate : amount(value.possums.quoted_cached_input_microunits_per_million_tokens);
        requireThat(cachedRate <= inputRate);
        const computed = (130n * (BigInt(input - cached) * inputRate + BigInt(cached) * cachedRate + BigInt(output) * outputRate) + 100_000_000n - 1n) / 100_000_000n;
        const reserved = value.possums.reserved_microunits === undefined ? computed : amount(value.possums.reserved_microunits);
        const charged = value.possums.outcome === 'refunded' ? 0n : computed < reserved ? computed : reserved;
        requireThat(amount(value.possums.charged_microunits) === charged);
        receipt.quotedInputMicrounitsPerMillion = quotedInput;
        receipt.quotedOutputMicrounitsPerMillion = quotedOutput;
      }
      state = 'done';
    } else {
      constraint = 'choice';
      requireThat(value.choices.length <= 1 && !receipt && state !== 'done');
      if (value.usage != null) {
        constraint = 'usage';
        fields(value.usage, ['prompt_tokens', 'completion_tokens', 'total_tokens'], ['prompt_tokens_details', 'completion_tokens_details', 'cost_usd']);
        const u = value.usage;
        requireThat([u.prompt_tokens, u.completion_tokens, u.total_tokens].every(n => Number.isSafeInteger(n) && n >= 0) && u.prompt_tokens + u.completion_tokens === u.total_tokens);
        if (u.prompt_tokens_details?.cached_tokens != null) requireThat(Number.isSafeInteger(u.prompt_tokens_details.cached_tokens) && u.prompt_tokens_details.cached_tokens >= 0 && u.prompt_tokens_details.cached_tokens <= u.prompt_tokens);
      }
      if (value.choices.length === 0) { requireThat(value.usage != null); return; }
      constraint = 'choice';
      requireThat(state === 'role' || state === 'text');
      const choice = value.choices[0]; fields(choice, ['index', 'delta'], ['finish_reason']);
      requireThat(choice.index === 0);
      constraint = 'delta';
      fields(choice.delta, [], ['role', 'content', 'tool_calls', 'reasoning', 'reasoning_content']);
      if (choice.delta.role != null) requireThat(choice.delta.role === 'assistant');
      state = 'text';
      if (choice.delta.content != null) { requireThat(typeof choice.delta.content === 'string'); textDelta = choice.delta.content; }
      for (const key of ['reasoning', 'reasoning_content']) requireThat(choice.delta[key] == null || typeof choice.delta[key] === 'string');
      if (Object.hasOwn(choice.delta, 'tool_calls') && choice.delta.tool_calls != null) {
        constraint = 'tool';
        requireThat(Array.isArray(choice.delta.tool_calls));
        const indexes = new Set<number>();
        for (const delta of choice.delta.tool_calls) {
          requireThat(names);
          requireThat(delta && Number.isSafeInteger(delta.index) && delta.index >= 0 && !indexes.has(delta.index));
          indexes.add(delta.index);
          fields(delta, ['index'], ['id', 'type', 'function']);
          let call = calls[delta.index];
          if (!call) {
            requireThat(delta.index === calls.length);
            retain(toolRecordRetainedBytes);
            call = { id: '', name: '', arguments: '' }; calls.push(call);
          }
          // The gateway preserves SDK fragments: identity and arguments can
          // arrive separately. Completeness is checked at the terminal finish.
          if (delta.type != null) requireThat(delta.type === 'function');
          if (delta.id != null && delta.id !== '') {
            toolID(delta.id);
            requireThat((!call.id || call.id === delta.id) && (!ids.has(delta.id) || call.id === delta.id));
            if (!call.id) retain(toolStringRetainedBytesPerUnit * delta.id.length);
            call.id = delta.id; ids.add(delta.id);
          }
          if (delta.function != null) {
            fields(delta.function, [], ['name', 'arguments']);
            if (delta.function.name != null && delta.function.name !== '') {
              toolName(delta.function.name);
              requireThat(names.has(delta.function.name) && (!call.name || call.name === delta.function.name));
              if (!call.name) retain(toolStringRetainedBytesPerUnit * delta.function.name.length);
              call.name = delta.function.name;
            }
          }
          const fragment = delta.function?.arguments ?? '';
          requireThat(typeof fragment === 'string');
          retain(toolStringRetainedBytesPerUnit * fragment.length);
          call.arguments += fragment;
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
    // String models retain the legacy text-only API. Tools require an explicitly
    // qualified live entry AND declarations; no name-based model inference.
    if (typeof model === 'string') modelID = model;
    else {
      const entry = validateModels({ object: 'list', data: [model] })[0]; modelID = entry.id;
      if (options.tools !== undefined) {
        requireThat(entry.tool_protocol === 'openai-functions-v1');
        const tools = admitTools(options.tools);
        serialize(tools, LIMITS.chat);
        names = new Set(Array.from(tools, tool => tool.function.name));
      }
    }
    requireThat(options.tools === undefined || names);
    stage = 'stream'; constraint = 'envelope';
    for (;;) {
      op.check();
      // Channel bodies time HTTP bytes, not decrypted frames or visible text.
      let next: ReadableStreamReadResult<Uint8Array>;
      try { next = await op.wait(reader.read(), hasByteReadTimeout(body) ? null : LIMITS.idleMs); }
      catch (error) {
        if (error instanceof OperationFailure || error instanceof DiagnosticFailure) throw error;
        throw new DiagnosticFailure(atEOF() ? 'stream' : 'transport', atEOF() ? 'eof' : 'fetch', 'uncertain');
      }
      if (next.done) {
        constraint = 'utf8'; requireThat(decoder.decode() === '');
        // Only a Channel-produced, SDK-decrypted body may carry HTTP admission status.
        if (controlError) { constraint = 'json'; failure = gatewayError(parseJSON(utf8.encode(pending), LIMITS.error), hasByteReadTimeout(body) ? httpStatus : undefined); }
        else { constraint = 'envelope'; requireThat(pending === '' && data === undefined); }
        if (receipt?.complete === false && !failure) failure = new GatewayError('generation_failed', 'stream_finish_missing');
        if (failure) {
          terminal = true;
          if (receipt && atEOF()) {
            confirmedSettlements.set(failure, Object.freeze(receipt));
            failure.message = gatewayDescription(failure.reason, failure.detail, false, receipt);
          } else if (claimedRefunds.has(failure)) {
            confirmedRefunds.add(failure);
            failure.message = gatewayDescription(failure.reason, failure.detail, true);
          }
          throw failure;
        }
        constraint = state === 'role' || state === 'text' ? 'finish_missing' :
          state === 'usage' ? 'usage_missing' : state === 'done' ? 'done_missing' : 'eof';
        requireThat(atEOF() && receipt && receipt.complete !== false);
        try {
          if (finish === 'tool_calls') requireThat(names && calls.length > 0);
          if (calls.length && finish !== 'length') {
            requireThat(names);
            for (const call of calls) { toolID(call.id); toolName(call.name); objectArguments(call.arguments); }
          }
        } catch {
          failure = new GatewayError('generation_failed', 'tool_call_incomplete'); terminal = true;
          confirmedSettlements.set(failure, Object.freeze(receipt));
          failure.message = gatewayDescription(failure.reason, failure.detail, false, receipt);
          throw failure;
        }
        return Object.freeze(receipt);
      }
      constraint = 'envelope';
      requireThat(next.value.length <= LIMITS.frame);
      constraint = 'utf8'; pending += decoder.decode(next.value, { stream: true });
      constraint = 'envelope';
      if (!sawEvent && !failure && pending.trimStart().startsWith('{')) controlError = true;
      if (controlError) { requireThat(pending.length <= LIMITS.error); continue; }
      let end: number;
      while ((end = pending.indexOf('\n')) !== -1) {
        requireThat(end <= LIMITS.sseEvent);
        const current = pending.slice(0, end); pending = pending.slice(end + 1); await line(current);
      }
      requireThat(pending.length <= LIMITS.sseEvent);
    }
  } catch (error) {
    if (terminal && error === failure) throw failure;
    if (failure) throw new GatewayError(failure.reason, failure.detail, 'unknown', failure.status); // No authenticated EOF: refund is unproven.
    if (error instanceof DiagnosticFailure) throw error;
    if (error instanceof OperationFailure) throw new DiagnosticFailure(stage, error.constraint, 'uncertain');
    throw new DiagnosticFailure(stage, constraint, 'uncertain');
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
      throw new CatalogFailure('request', error instanceof ChannelError ? error.code : 'rejected');
    }
    try { return validateModels(value); }
    catch { throw new CatalogFailure('validation'); }
  }
  async balance(signal?: AbortSignal): Promise<BalanceSnapshot> {
    let value: any;
    try {
      requireThat(this.#bearer);
      value = await this.channel.balance(this.#bearer, signal);
    } catch (error) {
      if (error instanceof GatewayError) throw error;
      throw new BalanceFailure('request', error instanceof ChannelError ? error.code : 'rejected', error instanceof DiagnosticFailure ? error : undefined);
    }
    try {
      keys(value, ['available_microunits', 'in_flight', 'completed_requests']);
      amount(value.available_microunits); amount(value.completed_requests);
      requireThat(Number.isInteger(value.in_flight) && !Object.is(value.in_flight, -0) && value.in_flight >= 0 && value.in_flight <= 0xffffffff);
      return Object.freeze({ availableMicrounits: value.available_microunits,
        inFlight: value.in_flight, completedRequests: value.completed_requests });
    } catch { throw new BalanceFailure('validation'); }
  }
  async chat(model: string, messages: Chat['messages'], onDelta: (text: string) => void, newConversation = false,
    options: ChatOptions = {}): Promise<Receipt> {
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
      opts = fields(options, [], ['signal', 'tools', 'tool_choice', 'onPayload', 'onResponse', 'onEvent']);
      for (const key of ['onPayload', 'onResponse', 'onEvent'] as const) requireThat(opts[key] === undefined || typeof opts[key] === 'function');
      op = new Operation(null, opts.signal);
    } catch { throw new DiagnosticFailure('request', 'schema'); }
    let stage: FailureStage = 'request', constraint: FailureConstraint = 'encoding';
    try {
      op.check();
      let payload = snapshotInvocation({ model, messages, stream: true,
        ...(opts.tools !== undefined ? { tools: opts.tools } : {}), ...(opts.tool_choice !== undefined ? { tool_choice: opts.tool_choice } : {}) });
      if (opts.onPayload) {
        stage = 'hook'; constraint = 'unexpected';
        let replacement: unknown;
        try { replacement = await op.wait(Promise.resolve().then(() => opts.onPayload!(payload)), LIMITS.idleMs); }
        catch (error) { if (error instanceof OperationFailure) throw error; throw new DiagnosticFailure('hook', 'unexpected'); }
        stage = 'request'; constraint = 'encoding';
        // Returning undefined keeps the immutable original; replacements are
        // re-admitted with the same model and cannot smuggle transport options.
        payload = snapshotInvocation(replacement === undefined ? payload : replacement);
        requireThat(payload.model === model);
      }
      op.check();
      return new PreparedChat(preparedAuthority, { payload,
        options: Object.freeze({ signal: opts.signal, onResponse: opts.onResponse, onEvent: opts.onEvent }), onDelta,
        model, newConversation, dispatched: false, busy: false, closed: false });
    } catch (error) {
      if (error instanceof DiagnosticFailure || error instanceof JSONDepthError) throw error;
      if (error instanceof OperationFailure) throw new DiagnosticFailure(stage, error.constraint);
      throw new DiagnosticFailure(stage, constraint);
    } finally { op.close(); }
  }
  async chatPrepared(prepared: PreparedChat, beforeDispatch?: () => void): Promise<Receipt> {
    return this.#chatPrepared(prepared, this.#bearer, beforeDispatch);
  }
  async #chatPrepared(prepared: PreparedChat, bearer: string | undefined, beforeDispatch?: () => void): Promise<Receipt> {
    // Acquire synchronously, before any await. The state is module-owned rather
    // than a mutable caller-supplied boolean, including across client rotations.
    const state = preparedStates.get(prepared);
    if (!state || state.closed || state.dispatched || state.busy) throw new DiagnosticFailure('request', 'schema');
    state.busy = true;
    let op: Operation;
    try { requireThat(bearer); op = new Operation(null, state.options!.signal); }
    catch { state.busy = false; throw new DiagnosticFailure('request', 'schema'); }
    const { payload, options: opts, onDelta, model, newConversation } = state;
    let issued = false, hookThrown = false, stage: FailureStage = 'request', constraint: FailureConstraint = 'encoding';
    let status: number | undefined;
    try {
      op.check(); stage = 'catalog'; constraint = 'fetch';
      // Refresh, never cache a usable offline catalog or substitute a cheaper model.
      // One invocation uses one in-memory session even if another login overlaps.
      const catalog = await this.channel.models(bearer, op.controller.signal);
      constraint = 'schema';
      const entry = validateModels(catalog).find(entry => entry.id === model);
      requireThat(entry);
      const usesTools = payload!.tools !== undefined || payload!.messages.some(message => message.role === 'tool' || (message.role === 'assistant' && message.tool_calls !== undefined));
      requireThat(!usesTools || entry.tool_protocol === 'openai-functions-v1');
      op.check(); requireThat(!state.closed); issued = true; stage = 'submission'; constraint = 'fetch';
      const issuance: any = await this.channel.control('/v1/submissions', { model, new_conversation: newConversation }, bearer, op.controller.signal);
      constraint = 'schema';
      keys(issuance, ['submission']); requireThat(token(issuance.submission));
      op.check(); requireThat(!state.closed); stage = 'transport'; constraint = 'fetch';
      // The owner checks session/account/run and cancellation synchronously at
      // the last possible boundary, after all awaited setup and hooks.
      beforeDispatch?.();
      op.check(); requireThat(!state.closed);
      state.dispatched = true;
      const body = await this.channel.chat({ ...payload!, submission: issuance.submission }, bearer,
        { signal: op.controller.signal, onResponse: async info => {
          if (Number.isInteger(info.status) && info.status >= 100 && info.status <= 599) status = info.status;
          if (!opts!.onResponse) return;
          try { await opts!.onResponse!(info); }
          catch { hookThrown = true; throw new DiagnosticFailure('hook', 'unexpected', 'uncertain', status); }
        } });
      // A forced function constrains generation; 'none' never permits live calls.
      let tools = payload!.tool_choice === 'none' ? undefined : payload!.tools;
      if (typeof payload!.tool_choice === 'object') {
        const name = payload!.tool_choice.function.name; tools = tools?.filter(tool => tool.function.name === name);
      }
      stage = 'stream';
      return await consumeCompletion(body, entry, onDelta!, { signal: op.controller.signal, tools, onEvent: opts!.onEvent }, status);
    } catch (error) {
      if (error instanceof GatewayError) {
        // Preserve the same authenticated refund carrier; copying would lose its evidence.
        if (status !== undefined) Object.defineProperty(error, 'status', { value: status });
        throw error;
      }
      if (error instanceof CatalogFailure) throw error;
      if (hookThrown) throw new DiagnosticFailure('hook', 'unexpected', 'uncertain', status);
      if (error instanceof DiagnosticFailure) {
        if ((issued && error.code !== 'uncertain') || (error.status === undefined && status !== undefined)) {
          throw new DiagnosticFailure(error.stage, error.constraint, issued ? 'uncertain' : error.code, error.status ?? status);
        }
        throw error;
      }
      if (!issued && error instanceof JSONDepthError) throw error;
      if (error instanceof OperationFailure) throw new DiagnosticFailure(stage, error.constraint, issued ? 'uncertain' : 'rejected');
      throw new DiagnosticFailure(stage, constraint,
        issued || (error instanceof ChannelError && error.code === 'uncertain') ? 'uncertain' : 'rejected');
    } finally { op.close(); state.busy = false; if (state.dispatched) prepared.close(); }
  }
}
