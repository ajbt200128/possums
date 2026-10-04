import { Channel, type Chat } from './transport.js';
import { ChannelError, LIMITS, Operation, cleanup, parseJSON, requireThat } from './limits.js';

export type LiveModel = { object: 'model'; id: string; context_tokens: string; max_output_tokens: string;
  input_microunits_per_million_tokens: string; output_microunits_per_million_tokens: string; maximum_reservation_microunits: string };
export type Receipt = { finish: 'stop' | 'length'; inputTokens: number; outputTokens: number; totalTokens: number;
  chargedMicrounits: string; refundedMicrounits: string };
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
  keys(value, ['object', 'data']);
  requireThat(value.object === 'list' && Array.isArray(value.data) && value.data.length > 0 && value.data.length <= LIMITS.models);
  const ids = new Set<string>();
  for (const model of value.data) {
    keys(model, ['object', 'id', 'context_tokens', 'max_output_tokens', 'input_microunits_per_million_tokens', 'output_microunits_per_million_tokens', 'maximum_reservation_microunits']);
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

export class GatewayError extends ChannelError {
  constructor(readonly reason: string) { super('uncertain'); }
}
function gatewayError(value: any): never {
  keys(value, ['error']);
  requireThat(value.error && typeof value.error.code === 'string' &&
    ['insufficient_credit', 'unauthorized', 'invalid_request', 'account_limit', 'duplicate_request', 'unavailable', 'generation_failed'].includes(value.error.code));
  // Only fixed codes from an authenticated body, never raw diagnostics or replay.
  throw new GatewayError(value.error.code);
}

// Success requires the ordered, authenticated gateway receipt AND stream EOF.
// No answer is accumulated; interruption is uncertain and never causes a resend.
export async function consumeCompletion(body: ReadableStream<Uint8Array>, model: string, onDelta: (text: string) => void): Promise<Receipt> {
  const reader = body.getReader(), decoder = new TextDecoder('utf-8', { fatal: true }), op = new Operation(LIMITS.streamMs);
  let pending = '', data: string | undefined, state: 'role' | 'text' | 'usage' | 'done' | 'eof' = 'role';
  let finish: 'stop' | 'length' | undefined, receipt: Receipt | undefined, events = 0, total = 0, controlError = false;
  function event(payload: string): void {
    requireThat(++events <= LIMITS.sseEvents);
    if (payload === '[DONE]') { requireThat(state === 'done'); state = 'eof'; return; }
    requireThat(state !== 'eof');
    const value = parseJSON(new TextEncoder().encode(payload), LIMITS.sseEvent);
    if (value.error) gatewayError(value);
    requireThat(value.object === 'chat.completion.chunk' && value.model === model && Array.isArray(value.choices));
    if (state === 'usage') {
      keys(value, ['object', 'model', 'choices', 'usage', 'possums']);
      requireThat(value.choices.length === 0 && finish);
      keys(value.usage, ['prompt_tokens', 'completion_tokens', 'total_tokens']);
      const { prompt_tokens: input, completion_tokens: output, total_tokens: sum } = value.usage;
      requireThat([input, output, sum].every(n => Number.isSafeInteger(n) && n >= 0) && input + output === sum);
      keys(value.possums, ['outcome', 'charged_microunits', 'refunded_microunits']);
      requireThat(value.possums.outcome === 'settled');
      amount(value.possums.charged_microunits); amount(value.possums.refunded_microunits);
      receipt = { finish, inputTokens: input, outputTokens: output, totalTokens: sum,
        chargedMicrounits: value.possums.charged_microunits, refundedMicrounits: value.possums.refunded_microunits };
      state = 'done'; return;
    }
    keys(value, ['object', 'model', 'choices']);
    requireThat(value.choices.length === 1 && (state === 'role' || state === 'text'));
    const choice = value.choices[0]; keys(choice, ['index', 'delta', 'finish_reason']);
    requireThat(choice.index === 0);
    if (state === 'role') {
      keys(choice.delta, ['role']); requireThat(choice.delta.role === 'assistant' && choice.finish_reason === null);
      state = 'text'; return;
    }
    if (choice.finish_reason !== null) {
      keys(choice.delta, []); requireThat(choice.finish_reason === 'stop' || choice.finish_reason === 'length');
      finish = choice.finish_reason; state = 'usage'; return;
    }
    keys(choice.delta, ['content']); requireThat(typeof choice.delta.content === 'string');
    onDelta(choice.delta.content);
  }
  function line(value: string): void {
    if (value.endsWith('\r')) value = value.slice(0, -1);
    if (value.startsWith(':')) return;
    if (!value) { if (data !== undefined) { event(data); data = undefined; } return; }
    requireThat(state !== 'eof' && data === undefined && value.startsWith('data: '));
    data = value.slice(6);
  }
  function atEOF(): boolean { return state === 'eof'; }
  try {
    for (;;) {
      // Reasoning can pause visible output; the absolute stream deadline stays bounded.
      const next = await op.wait(reader.read(), LIMITS.streamMs);
      if (next.done) {
        requireThat(decoder.decode() === '');
        if (controlError) gatewayError(parseJSON(new TextEncoder().encode(pending), LIMITS.error));
        requireThat(pending === '' && data === undefined && atEOF() && receipt);
        return Object.freeze(receipt);
      }
      total += next.value.length;
      requireThat(next.value.length <= LIMITS.frame && total <= LIMITS.stream);
      pending += decoder.decode(next.value, { stream: true });
      if (pending.trimStart().startsWith('{')) controlError = true;
      if (controlError) { requireThat(pending.length <= LIMITS.error); continue; }
      let end: number;
      while ((end = pending.indexOf('\n')) !== -1) {
        requireThat(end <= LIMITS.sseEvent);
        line(pending.slice(0, end)); pending = pending.slice(end + 1);
      }
      requireThat(pending.length <= LIMITS.sseEvent);
    }
  } catch (error) { if (error instanceof GatewayError) throw error; throw new ChannelError('uncertain'); }
  finally { op.close(); await cleanup(reader.cancel()); }
}

export class ReferenceClient {
  #bearer: string | undefined;
  constructor(readonly channel: Channel) { Channel.requireVerified(channel); }
  static async verified(bundle: Uint8Array, manifest: Uint8Array<ArrayBuffer>, keyConfig: Uint8Array): Promise<ReferenceClient> {
    return new ReferenceClient(await Channel.api(bundle, manifest, keyConfig));
  }
  async login(credential: string): Promise<void> {
    this.#bearer = undefined;
    const challenge: any = await this.channel.challenge();
    keys(challenge, ['challenge', 'expires_in']); requireThat(token(challenge.challenge) && challenge.expires_in === 600);
    const session: any = await this.channel.control('/v1/sessions', { challenge: challenge.challenge, credential });
    keys(session, ['token', 'token_type', 'expires_in']);
    requireThat(token(session.token) && session.token_type === 'Bearer' && session.expires_in === 43200);
    this.#bearer = session.token;
  }
  async models(): Promise<readonly LiveModel[]> {
    requireThat(this.#bearer); return validateModels(await this.channel.models(this.#bearer));
  }
  async chat(model: string, messages: Chat['messages'], onDelta: (text: string) => void, newConversation = false): Promise<Receipt> {
    requireThat(this.#bearer);
    // Refresh, never cache a usable offline catalog or substitute a cheaper model.
    requireThat((await this.models()).some(entry => entry.id === model));
    const issuance: any = await this.channel.control('/v1/submissions', { model, new_conversation: newConversation }, this.#bearer);
    keys(issuance, ['submission']); requireThat(token(issuance.submission));
    const body = await this.channel.chat({ model, messages, stream: true, submission: issuance.submission }, this.#bearer);
    return consumeCompletion(body, model, onDelta);
  }
}
