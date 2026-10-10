import { Channel, hasByteReadTimeout, type ResponseOptions } from './transport.js';
import { ChannelError, JSONDepthError, LIMITS, Operation, cleanup, parseJSON, requireThat, serialize, utf8 } from './limits.js';
import { admitTools, fields, freezeJSON, objectArguments, snapshotInvocation, toolID, toolName,
  type Chat, type Invocation, type Tool, type ToolChoice } from './tools.js';

// Events are provisional display data, NEVER permission to execute a tool.
// Execution needs caller policy checks AND a settled receipt + DONE + authenticated EOF.
export type CompletionEvent = Readonly<{
  object: 'chat.completion.chunk'; model: string;
  choices: readonly Readonly<{ index: 0; delta: Readonly<{ role?: 'assistant'; content?: string;
    tool_calls?: readonly Readonly<{ index: number; id?: string; type?: 'function';
      function?: Readonly<{ name?: string; arguments?: string }> }>[] }>;
    finish_reason: Receipt['finish'] | null }>[];
  usage?: Readonly<{ prompt_tokens: number; completion_tokens: number; total_tokens: number }>;
  possums?: Readonly<{ outcome: 'settled'; charged_microunits: string; refunded_microunits: string;
    quoted_input_microunits_per_million_tokens?: string; quoted_output_microunits_per_million_tokens?: string }>;
}>;
export type CompletionOptions = { signal?: AbortSignal; tools?: Tool[];
  onEvent?: (event: CompletionEvent) => void | Promise<void> };
export type ChatOptions = ResponseOptions & CompletionOptions & { tool_choice?: ToolChoice;
  onPayload?: (payload: Readonly<Invocation>) => unknown | Promise<unknown> };

export type LiveModel = { object: 'model'; id: string; context_tokens: string; max_output_tokens: string;
  input_microunits_per_million_tokens: string; output_microunits_per_million_tokens: string; maximum_reservation_microunits: string;
  tool_protocol?: 'openai-functions-v1' };
export type Receipt = { finish: 'stop' | 'length' | 'tool_calls'; inputTokens: number; outputTokens: number; totalTokens: number;
  chargedMicrounits: string; refundedMicrounits: string;
  quotedInputMicrounitsPerMillion?: string; quotedOutputMicrounitsPerMillion?: string };
export type BalanceSnapshot = Readonly<{ availableMicrounits: string; inFlight: number; completedRequests: string }>;
export class BalanceFailure extends ChannelError {
  constructor(readonly stage: 'request' | 'validation', code: ChannelError['code'] = 'rejected') {
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
    fields(model, ['object', 'id', 'context_tokens', 'max_output_tokens', 'input_microunits_per_million_tokens', 'output_microunits_per_million_tokens', 'maximum_reservation_microunits'], ['tool_protocol']);
    if (Object.hasOwn(model, 'tool_protocol')) requireThat(model.tool_protocol === 'openai-functions-v1');
    requireThat(model.object === 'model' && typeof model.id === 'string' && /^[A-Za-z0-9._:/-]{1,128}$/.test(model.id) && !ids.has(model.id));
    ids.add(model.id);
    const context = amount(model.context_tokens), output = amount(model.max_output_tokens);
    const inputPrice = amount(model.input_microunits_per_million_tokens), outputPrice = amount(model.output_microunits_per_million_tokens);
    requireThat(context > 0n && output > 0n && output <= context && inputPrice > 0n && outputPrice > 0n);
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
  tool_choice_violated tool_call_incomplete tool_arguments_too_large stream_finish_invalid stream_finish_missing
  stream_usage_invalid stream_usage_unexpected stream_usage_missing stream_output_after_finish settlement_failed`.split(/\s+/));
export function gatewayDetailLabel(detail: string | undefined): string | undefined {
  return detail !== undefined && details.has(detail) ? `[${detail}] ${detail.replaceAll('_', ' ')}` : undefined;
}
const claimedRefunds = new WeakSet<GatewayError>();
const confirmedRefunds = new WeakSet<GatewayError>();
function gatewayDescription(reason: string, detail: string | undefined, refunded: boolean): string {
  const reasons: Readonly<Record<string, string>> = {
    insufficient_credit: 'Insufficient credit for this model’s maximum reservation.',
    unauthorized: 'Session expired; use /login to authenticate again.',
    invalid_request: 'Request rejected by gateway.', account_limit: 'Account limit reached.',
    duplicate_request: 'Duplicate submission rejected.', unavailable: 'Gateway unavailable.',
    generation_failed: 'Generation failed.',
  };
  const label = gatewayDetailLabel(detail);
  return `Possums: ${Object.hasOwn(reasons, reason) ? reasons[reason] : 'Gateway request failed.'}` +
    (label ? ` ${label}.` : '') + (refunded ? ' Reservation refunded. Not replayed.' :
      ' Charge unknown. Not replayed. A new request may incur another charge.');
}
export class GatewayError extends ChannelError {
  constructor(readonly reason: string, readonly detail?: string, _claimedBilling: 'refunded' | 'unknown' = 'unknown') {
    super('uncertain'); this.message = gatewayDescription(reason, detail, false);
  }
  get billing(): 'refunded' | 'unknown' { return confirmedRefunds.has(this) ? 'refunded' : 'unknown'; }
}
function gatewayError(value: any): GatewayError {
  keys(value, ['error']);
  fields(value.error, ['code'], ['detail', 'message', 'billing', 'outcome']);
  requireThat(typeof value.error.code === 'string' &&
    ['insufficient_credit', 'unauthorized', 'invalid_request', 'account_limit', 'duplicate_request', 'unavailable', 'generation_failed'].includes(value.error.code));
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
        const failure = gatewayError(response);
        this.reason = failure.reason; this.detail = failure.detail;
      } catch { /* An unreadable/unknown error body does not erase the HTTP status. */ }
    }
    Object.freeze(this);
  }
}

// Success requires the ordered, authenticated gateway receipt AND stream EOF.
// No answer is accumulated; interruption is uncertain and never causes a resend.
export async function consumeCompletion(body: ReadableStream<Uint8Array>, model: string | LiveModel,
  onDelta: (text: string) => void, options: CompletionOptions = {}): Promise<Receipt> {
  const reader = body.getReader(), decoder = new TextDecoder('utf-8', { fatal: true }), op = new Operation(null, options.signal);
  let pending = '', data: string | undefined, state: 'role' | 'text' | 'usage' | 'done' | 'eof' | 'error' = 'role';
  let finish: Receipt['finish'] | undefined, receipt: Receipt | undefined, events = 0, total = 0, controlError = false;
  let failure: GatewayError | undefined, terminal = false;
  let modelID: string, names: Set<string> | undefined;
  const calls: { id: string; name: string; arguments: string; bytes: number }[] = [], ids = new Set<string>();
  let argumentBytes = 0;
  async function event(payload: string): Promise<void> {
    requireThat(++events <= LIMITS.sseEvents);
    if (payload === '[DONE]') { requireThat(state === 'done'); state = 'eof'; return; }
    requireThat(state !== 'eof' && state !== 'error');
    const value = parseJSON(utf8.encode(payload), LIMITS.sseEvent);
    requireThat(value && typeof value === 'object');
    if (Object.hasOwn(value, 'error')) { requireThat(!receipt); failure = gatewayError(value); state = 'error'; return; }
    requireThat(value.object === 'chat.completion.chunk' && value.model === modelID && Array.isArray(value.choices));
    let textDelta: string | undefined;
    if (state === 'usage') {
      keys(value, ['object', 'model', 'choices', 'usage', 'possums']);
      requireThat(value.choices.length === 0 && finish);
      keys(value.usage, ['prompt_tokens', 'completion_tokens', 'total_tokens']);
      const { prompt_tokens: input, completion_tokens: output, total_tokens: sum } = value.usage;
      requireThat([input, output, sum].every(n => Number.isSafeInteger(n) && n >= 0) && input + output === sum);
      fields(value.possums, ['outcome', 'charged_microunits', 'refunded_microunits'],
        ['quoted_input_microunits_per_million_tokens', 'quoted_output_microunits_per_million_tokens']);
      requireThat(value.possums.outcome === 'settled');
      amount(value.possums.charged_microunits); amount(value.possums.refunded_microunits);
      receipt = { finish, inputTokens: input, outputTokens: output, totalTokens: sum,
        chargedMicrounits: value.possums.charged_microunits, refundedMicrounits: value.possums.refunded_microunits };
      const quotedInput = value.possums.quoted_input_microunits_per_million_tokens;
      const quotedOutput = value.possums.quoted_output_microunits_per_million_tokens;
      if (quotedInput !== undefined || quotedOutput !== undefined) {
        const inputRate = amount(quotedInput), outputRate = amount(quotedOutput);
        requireThat(inputRate > 0n && outputRate > 0n);
        const charged = (130n * (BigInt(input) * inputRate + BigInt(output) * outputRate) + 100_000_000n - 1n) / 100_000_000n;
        requireThat(amount(value.possums.charged_microunits) === charged);
        receipt.quotedInputMicrounitsPerMillion = quotedInput;
        receipt.quotedOutputMicrounitsPerMillion = quotedOutput;
      }
      state = 'done';
    } else {
      keys(value, ['object', 'model', 'choices']);
      requireThat(value.choices.length === 1 && (state === 'role' || state === 'text'));
      const choice = value.choices[0]; keys(choice, ['index', 'delta', 'finish_reason']);
      requireThat(choice.index === 0);
      if (state === 'role') {
        keys(choice.delta, ['role']); requireThat(choice.delta.role === 'assistant' && choice.finish_reason === null);
        state = 'text';
      } else if (choice.finish_reason !== null) {
        keys(choice.delta, []);
        requireThat(choice.finish_reason === 'tool_calls' || choice.finish_reason === 'stop' || choice.finish_reason === 'length');
        if (choice.finish_reason === 'tool_calls') requireThat(names && calls.length > 0);
        if (calls.length && choice.finish_reason !== 'length') {
          requireThat(names);
          for (const call of calls) {
            toolID(call.id); toolName(call.name); objectArguments(call.arguments);
          }
        }
        finish = choice.finish_reason; state = 'usage';
      } else if (Object.hasOwn(choice.delta, 'tool_calls')) {
        keys(choice.delta, ['tool_calls']);
        requireThat(names && Array.isArray(choice.delta.tool_calls) && choice.delta.tool_calls.length > 0 && choice.delta.tool_calls.length <= LIMITS.tools);
        const indexes = new Set<number>();
        for (const delta of choice.delta.tool_calls) {
          requireThat(delta && Number.isInteger(delta.index) && delta.index >= 0 && delta.index < LIMITS.tools && !indexes.has(delta.index));
          indexes.add(delta.index);
          fields(delta, ['index'], ['id', 'type', 'function']);
          let call = calls[delta.index];
          if (!call) {
            requireThat(delta.index === calls.length);
            call = { id: '', name: '', arguments: '', bytes: 0 }; calls.push(call);
          }
          // The gateway preserves SDK fragments: identity and arguments can
          // arrive separately. Completeness is checked at the terminal finish.
          if (Object.hasOwn(delta, 'type')) requireThat(delta.type === 'function');
          if (Object.hasOwn(delta, 'id')) {
            toolID(delta.id);
            requireThat((!call.id || call.id === delta.id) && (!ids.has(delta.id) || call.id === delta.id));
            call.id = delta.id; ids.add(delta.id);
          }
          if (Object.hasOwn(delta, 'function')) {
            fields(delta.function, [], ['name', 'arguments']);
            if (Object.hasOwn(delta.function, 'name')) {
              toolName(delta.function.name);
              requireThat(names.has(delta.function.name) && (!call.name || call.name === delta.function.name));
              call.name = delta.function.name;
            }
          }
          const fragment = delta.function && Object.hasOwn(delta.function, 'arguments') ? delta.function.arguments : '';
          requireThat(typeof fragment === 'string' && fragment.length <= LIMITS.toolArguments && call.arguments.length + fragment.length <= LIMITS.toolArguments);
          call.arguments += fragment;
          const size = utf8.encode(call.arguments).length;
          argumentBytes += size - call.bytes; call.bytes = size;
          requireThat(size <= LIMITS.toolArguments && argumentBytes <= LIMITS.totalToolArguments);
        }
      } else {
        keys(choice.delta, ['content']); requireThat(typeof choice.delta.content === 'string');
        textDelta = choice.delta.content;
      }
    }
    // Validate the WHOLE event before exposing any of its deltas. Hooks cannot
    // mutate parser state, and awaiting them bounds queueing/backpressure.
    freezeJSON(value);
    op.check();
    if (textDelta !== undefined) await op.wait(Promise.resolve(onDelta(textDelta)), LIMITS.idleMs);
    if (options.onEvent) { op.check(); await op.wait(Promise.resolve(options.onEvent(value)), LIMITS.idleMs); }
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
    for (;;) {
      op.check();
      // Channel bodies time HTTP bytes, not decrypted frames or visible text.
      const next = await op.wait(reader.read(), hasByteReadTimeout(body) ? null : LIMITS.idleMs);
      if (next.done) {
        requireThat(decoder.decode() === '');
        if (controlError) failure = gatewayError(parseJSON(utf8.encode(pending), LIMITS.error));
        else requireThat(pending === '' && data === undefined);
        if (failure) {
          terminal = true;
          if (claimedRefunds.has(failure)) {
            confirmedRefunds.add(failure);
            failure.message = gatewayDescription(failure.reason, failure.detail, true);
          }
          throw failure;
        }
        requireThat(atEOF() && receipt);
        return Object.freeze(receipt);
      }
      total += next.value.length;
      requireThat(next.value.length <= LIMITS.frame && total <= LIMITS.stream);
      pending += decoder.decode(next.value, { stream: true });
      if (!events && !failure && pending.trimStart().startsWith('{')) controlError = true;
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
    if (failure) throw new GatewayError(failure.reason, failure.detail); // No authenticated EOF: refund is unproven.
    throw new ChannelError('uncertain');
  }
  finally { op.close(); await cleanup(reader.cancel()); }
}

export class ReferenceClient {
  #bearer: string | undefined;
  constructor(readonly channel: Channel) { Channel.requireVerified(channel); }
  static async verified(bundle: Uint8Array, manifest: Uint8Array<ArrayBuffer>, keyConfig: Uint8Array): Promise<ReferenceClient> {
    return new ReferenceClient(await Channel.api(bundle, manifest, keyConfig));
  }
  static async published(bundle: Uint8Array, manifest: Uint8Array<ArrayBuffer>, keyConfig: Uint8Array, signal?: AbortSignal): Promise<ReferenceClient> {
    return new ReferenceClient(await Channel.published(bundle, manifest, keyConfig, signal));
  }
  get release() { return this.channel.release; }
  freshSession(): ReferenceClient { return new ReferenceClient(this.channel); }
  async login(credential: string, signal?: AbortSignal): Promise<void> {
    this.#bearer = undefined;
    const op = new Operation(2 * LIMITS.operationMs, signal);
    let started = false;
    try {
      op.check(); started = true;
      const challenge: any = await this.channel.challenge(op.controller.signal);
      keys(challenge, ['challenge', 'expires_in']); requireThat(token(challenge.challenge) && challenge.expires_in === 600);
      op.check();
      const session: any = await this.channel.control('/v1/sessions', { challenge: challenge.challenge, credential }, undefined, op.controller.signal);
      keys(session, ['token', 'token_type', 'expires_in']);
      requireThat(token(session.token) && session.token_type === 'Bearer' && session.expires_in === 43200);
      op.check(); this.#bearer = session.token;
    } catch (error) {
      if (started && op.controller.signal.aborted) throw new ChannelError('uncertain');
      throw error;
    } finally { op.close(); }
  }
  async models(signal?: AbortSignal): Promise<readonly LiveModel[]> {
    let value: unknown;
    try {
      requireThat(this.#bearer); value = await this.channel.models(this.#bearer, signal);
    } catch (error) {
      if (error instanceof CatalogFailure) throw error;
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
      throw new BalanceFailure('request', error instanceof ChannelError ? error.code : 'rejected');
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
    requireThat(this.#bearer && typeof newConversation === 'boolean' && typeof onDelta === 'function');
    // Copy option descriptors once. No arbitrary request options / fetch / headers.
    const opts = fields(options, [], ['signal', 'tools', 'tool_choice', 'onPayload', 'onResponse', 'onEvent']);
    for (const key of ['onPayload', 'onResponse', 'onEvent']) requireThat(opts[key] === undefined || typeof opts[key] === 'function');
    const bearer = this.#bearer, op = new Operation(null, opts.signal);
    let issued = false, consuming = false;
    try {
      op.check();
      let payload = snapshotInvocation({ model, messages, stream: true,
        ...(opts.tools !== undefined ? { tools: opts.tools } : {}), ...(opts.tool_choice !== undefined ? { tool_choice: opts.tool_choice } : {}) });
      if (opts.onPayload) {
        const replacement = await op.wait(Promise.resolve(opts.onPayload(payload)), LIMITS.idleMs);
        // Returning undefined keeps the immutable original; replacements are
        // re-admitted with the same model and cannot smuggle transport options.
        payload = snapshotInvocation(replacement === undefined ? payload : replacement);
        requireThat(payload.model === model);
      }
      op.check();
      // Refresh, never cache a usable offline catalog or substitute a cheaper model.
      // One invocation uses one in-memory session even if another login overlaps.
      const entry = validateModels(await this.channel.models(bearer, op.controller.signal)).find(entry => entry.id === model);
      requireThat(entry);
      const usesTools = payload.tools !== undefined || payload.messages.some(message => message.role === 'tool' || (message.role === 'assistant' && message.tool_calls !== undefined));
      requireThat(!usesTools || entry.tool_protocol === 'openai-functions-v1');
      op.check(); issued = true;
      const issuance: any = await this.channel.control('/v1/submissions', { model, new_conversation: newConversation }, bearer, op.controller.signal);
      keys(issuance, ['submission']); requireThat(token(issuance.submission));
      op.check();
      const body = await this.channel.chat({ ...payload, submission: issuance.submission }, bearer,
        { signal: op.controller.signal, onResponse: opts.onResponse });
      // A forced function constrains generation; 'none' never permits live calls.
      let tools = payload.tool_choice === 'none' ? undefined : payload.tools;
      if (typeof payload.tool_choice === 'object') {
        const name = payload.tool_choice.function.name; tools = tools?.filter(tool => tool.function.name === name);
      }
      consuming = true;
      return await consumeCompletion(body, entry, onDelta, { signal: op.controller.signal, tools, onEvent: opts.onEvent });
    } catch (error) {
      if (consuming && error instanceof GatewayError) throw error;
      if (!issued && error instanceof JSONDepthError) throw error;
      throw new ChannelError(issued || (error instanceof ChannelError && error.code === 'uncertain') ? 'uncertain' : 'rejected');
    } finally { op.close(); }
  }
}
