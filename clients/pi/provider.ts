import {
  createAssistantMessageEventStream, parseStreamingJson,
  type Api, type ApiStreamOptions, type AssistantMessageEventStream, type StreamOptions,
  type AssistantMessage, type Model, type Provider, type SimpleStreamOptions,
  type TranscriptContext, type ToolCall,
} from '@earendil-works/pi-ai';
import { Channel } from '../../examples/phase01/transport.js';
import { ReferenceClient, GatewayError, gatewayDetailLabel, type LiveModel, type CompletionEvent } from '../../examples/phase01/client.js';
import { ChannelError } from '../../examples/phase01/limits.js';
import { ReplayGuard } from './replay.js';
import { invocation } from './wire.js';

export const PROVIDER_ID = 'possums';
const API = 'openai-completions';
const ORIGIN = 'https://possum-phase0.possums.containers.tinfoil.dev';
const MEMORY_AUTH = 'possums-memory-only-session';
type RequestOptions = StreamOptions & { reasoning?: unknown; reasoningEffort?: unknown; toolChoice?: unknown };

function safeNumber(value: string): number {
  const result = Number(value);
  if (!Number.isSafeInteger(result) || result < 0) throw new Error('possums_catalog_out_of_range');
  return result;
}
function dollars(microunits: string): string {
  const value = BigInt(microunits);
  return `${value / 1_000_000n}.${(value % 1_000_000n).toString().padStart(6, '0')}`;
}
function model(entry: LiveModel): Model<typeof API> {
  return {
    id: entry.id, provider: PROVIDER_ID, api: API, baseUrl: ORIGIN,
    name: `${entry.id} · ${entry.tool_protocol ? 'tools' : 'text only'} · max $${dollars(entry.maximum_reservation_microunits)} (indicative)`,
    input: ['text'], reasoning: false,
    contextWindow: safeNumber(entry.context_tokens), maxTokens: safeNumber(entry.max_output_tokens),
    cost: { input: safeNumber(entry.input_microunits_per_million_tokens) * 1.3 / 1_000_000,
      output: safeNumber(entry.output_microunits_per_million_tokens) * 1.3 / 1_000_000, cacheRead: 0, cacheWrite: 0 },
  };
}
function blank(selected: Model<typeof API>): AssistantMessage {
  return { role: 'assistant', provider: PROVIDER_ID, api: API, model: selected.id,
    timestamp: Date.now(), content: [], stopReason: 'pending',
    usage: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 0,
      cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } } };
}
const localFailures: Readonly<Record<string, string>> = Object.freeze({
  possums_catalog_out_of_range: 'Model catalog exceeds supported limits.',
  possums_catalog_unavailable: 'Model catalog unavailable. Use /login to refresh the session.',
  possums_incomplete_tool_round: 'Tool response incomplete; no tool executed. Charge settled. Not automatically continued. A new request may incur another charge.',
  possums_invalid_stream: 'Response incomplete; no tool executed.',
  possums_login_cancelled: 'Login cancelled. Use /login to start again.',
  possums_request_options_unsupported: 'Request options unsupported; remove custom options.',
  possums_run_replaced: 'Session changed; no tool executed. Any settled charge remains recorded.',
  possums_session_unavailable: 'Session unavailable. Use /login to authenticate.',
  possums_tools_unsupported: 'Selected model is not qualified for tools. Use /possums-text-only or select a qualified model. No inference request sent.',
  possums_automatic_replay_blocked: 'Automatic replay blocked. Submit a deliberate new request if needed.',
  possums_invalid_tool_receipt: 'Tool receipt invalid; no tool executed.',
  possums_constrained_sampling_unsupported: 'Constrained sampling unsupported; remove the option.',
  possums_custom_tools_unsupported: 'Custom tools unsupported; remove the option.',
  possums_duplicate_tool_call: 'Duplicate tool call in history; correct the history.',
  possums_images_unsupported: 'Images unsupported; remove images before sending.',
  possums_mixed_provider_history: 'Mixed provider history unsupported; start a new session.',
  possums_orphan_tool_result: 'Tool result has no matching call; correct the history.',
  possums_unresolved_tool_calls: 'Tool calls lack results; complete the history.',
});
function safeFailure(error: unknown): string {
  if (error instanceof GatewayError) {
    const reason: Readonly<Record<string, string>> = {
      insufficient_credit: 'Insufficient credit for this model’s maximum reservation.',
      unauthorized: 'Session expired; use /login to authenticate again.',
      invalid_request: 'Request rejected by gateway.', account_limit: 'Account limit reached.',
      duplicate_request: 'Duplicate submission rejected.', unavailable: 'Gateway unavailable.',
      generation_failed: 'Generation failed.',
    };
    const label = gatewayDetailLabel(error.detail);
    return `Possums: ${reason[error.reason] ?? 'Gateway request failed.'}${label ? ` ${label}.` : ''} ` +
      (error.billing === 'refunded' ? 'Reservation refunded.' : 'Charge unknown. A new request may incur another charge.') + ' Not replayed.';
  }
  if (error instanceof ChannelError && error.code === 'uncertain') return 'Possums: request interrupted; charge unknown. Not replayed. A new request may incur another charge.';
  // Only exact internal literals are displayable. Never echo a hook exception.
  if (error instanceof Error && Object.hasOwn(localFailures, error.message)) return `[${error.message}] ${localFailures[error.message]}`;
  return 'Possums: request rejected. Not replayed.';
}

export class PossumsProvider implements Provider {
  readonly id = PROVIDER_ID;
  readonly name = 'Possums (verified, session-only)';
  readonly baseUrl = ORIGIN;
  private client: ReferenceClient | undefined;
  private catalog: readonly LiveModel[] = [];
  private listed: readonly Model<typeof API>[] = [];
  private readonly guard = new ReplayGuard();
  private newConversation = true;
  private epoch = 0;

  constructor(private readonly establish: (signal: AbortSignal) => Promise<ReferenceClient>) {}

  readonly auth = { apiKey: {
    name: 'Possums recovery credential (memory only)',
    check: async (input: { credential?: { key?: string } }) => this.client && input.credential?.key === MEMORY_AUTH ? { type: 'api_key' as const, source: 'verified memory session' } : undefined,
    resolve: async (input: { credential?: { key?: string } }) => this.client && input.credential?.key === MEMORY_AUTH ? { auth: { apiKey: MEMORY_AUTH }, source: 'verified memory session' } : undefined,
    login: async (interaction: { signal: AbortSignal; prompt: (prompt: { type: 'secret'; message: string; signal: AbortSignal }) => Promise<string> }) => {
      this.logout();
      const epoch = this.epoch;
      // Verify public evidence and the actual channel before requesting a secret.
      const candidate = await this.establish(interaction.signal);
      Channel.requireVerified(candidate.channel);
      if (epoch !== this.epoch || interaction.signal.aborted) throw new Error('possums_login_cancelled');
      const credential = await interaction.prompt({ type: 'secret', message: 'Possums recovery credential (not saved)', signal: interaction.signal });
      if (epoch !== this.epoch || interaction.signal.aborted) throw new Error('possums_login_cancelled');
      await candidate.login(credential, interaction.signal);
      const entries = await candidate.models(interaction.signal);
      const listed = entries.map(model);
      if (epoch !== this.epoch || interaction.signal.aborted) throw new Error('possums_login_cancelled');
      this.client = candidate; this.catalog = entries; this.listed = listed;
      // Pi persists this harmless marker, never the recovery or bearer credential.
      return { type: 'api_key' as const, key: MEMORY_AUTH };
    },
  } };

  getModels(): readonly Model<typeof API>[] { return this.listed; }
  beginRun(): void { this.epoch++; this.guard.beginRun(); }
  endRun(): void { this.guard.close(); }
  newSession(): void { this.epoch++; this.guard.close(); this.newConversation = true; }
  logout(): void {
    this.epoch++; this.guard.close(); this.client = undefined; this.catalog = []; this.listed = [];
    this.newConversation = true;
  }

  async refreshModels(context: Parameters<NonNullable<Provider['refreshModels']>>[0]): Promise<void> {
    if (!context.allowNetwork || !this.client) return;
    try {
      const entries = await this.client.models(context.signal);
      const listed = entries.map(model);
      await context.publish({ update: () => { this.catalog = entries; this.listed = listed; } });
    } catch {
      // Retain no usable stale catalog, regardless of Pi's default refresh policy.
      this.catalog = []; this.listed = [];
      throw new Error('possums_catalog_unavailable');
    }
  }

  stream<T extends Api>(selected: Model<T>, context: TranscriptContext, options?: ApiStreamOptions<T>): AssistantMessageEventStream {
    return this.perform(selected as Model<typeof API>, context, options as RequestOptions);
  }

  streamSimple(selected: Model<Api>, context: TranscriptContext, options?: SimpleStreamOptions): AssistantMessageEventStream {
    return this.perform(selected as Model<typeof API>, context, options);
  }

  private perform(selected: Model<typeof API>, context: TranscriptContext, options: RequestOptions = {}): AssistantMessageEventStream {
    const events = createAssistantMessageEventStream();
    const output = blank(selected);
    const epoch = this.epoch;
    const args = new Map<number, { block: ToolCall; contentIndex: number; json: string }>();
    let started = false;
    let textIndex: number | undefined;
    const endText = () => {
      if (textIndex === undefined) return;
      const block = output.content[textIndex];
      if (block.type === 'text') events.push({ type: 'text_end', contentIndex: textIndex, content: block.text, partial: output });
      textIndex = undefined;
    };
    void (async () => {
      try {
        this.guard.claim(selected.id, context.messages);
        if (!this.client || options.signal?.aborted) throw new Error('possums_session_unavailable');
        if (selected.provider !== PROVIDER_ID || selected.api !== API || selected.baseUrl !== ORIGIN || options.fetch || options.maxTokens !== undefined || options.samplingParams || options.temperature !== undefined || options.reasoning !== undefined || options.reasoningEffort !== undefined) {
          throw new Error('possums_request_options_unsupported');
        }
        const payload = invocation(selected, context);
        if (options.toolChoice !== undefined) payload.tool_choice = options.toolChoice as NonNullable<typeof payload.tool_choice>;
        const entry = this.catalog.find(entry => entry.id === selected.id);
        if (!entry || (payload.tools?.length && entry.tool_protocol !== 'openai-functions-v1')) throw new Error('possums_tools_unsupported');
        const receipt = await this.client.chat(selected.id, payload.messages, text => {
          if (!started) throw new Error('possums_invalid_stream');
          if (textIndex === undefined) {
            textIndex = output.content.length;
            output.content.push({ type: 'text', text: '' });
            events.push({ type: 'text_start', contentIndex: textIndex, partial: output });
          }
          const block = output.content[textIndex];
          if (block.type !== 'text') throw new Error('possums_invalid_stream');
          block.text += text;
          events.push({ type: 'text_delta', contentIndex: textIndex, delta: text, partial: output });
        }, this.newConversation, {
          signal: options.signal, tools: payload.tools, tool_choice: payload.tool_choice,
          onPayload: options.onPayload ? value => options.onPayload!(value, selected) : undefined,
          onResponse: options.onResponse ? value => options.onResponse!({ status: value.status, headers: value.contentType ? { 'content-type': value.contentType } : {} }, selected) : undefined,
          onEvent: async (value: CompletionEvent) => {
            await options.onProviderStreamEvent?.(structuredClone(value), selected);
            const delta = value.choices?.[0]?.delta;
            if (delta?.role === 'assistant' && !started) {
              started = true;
              events.push({ type: 'start', partial: output });
            }
            for (const call of delta?.tool_calls ?? []) {
              endText();
              let pending = args.get(call.index);
              if (!pending) {
                const block: ToolCall = { type: 'toolCall', id: call.id ?? '', name: call.function.name ?? '', arguments: {} };
                pending = { block, contentIndex: output.content.length, json: '' };
                args.set(call.index, pending); output.content.push(block);
                events.push({ type: 'toolcall_start', contentIndex: pending.contentIndex, partial: output });
              }
              if (call.id !== undefined) pending.block.id = call.id;
              if (call.function.name !== undefined) pending.block.name = call.function.name;
              const fragment = call.function.arguments;
              pending.json += fragment;
              pending.block.arguments = parseStreamingJson(pending.json);
              if (fragment) events.push({ type: 'toolcall_delta', contentIndex: pending.contentIndex, delta: fragment, partial: output });
            }
          },
        });
        if (epoch === this.epoch) this.newConversation = false;
        if (!started) throw new Error('possums_invalid_stream');
        endText();
        const charged = safeNumber(receipt.chargedMicrounits) / 1_000_000;
        const quoted = receipt.quotedInputMicrounitsPerMillion !== undefined && receipt.quotedOutputMicrounitsPerMillion !== undefined;
        // The submitted rate is authoritative. Assign the sub-microunit rounding
        // remainder to output so the displayed components equal the settled total.
        const inputMicrounits = quoted ? BigInt(receipt.inputTokens) * BigInt(receipt.quotedInputMicrounitsPerMillion!) * 130n / 100_000_000n : 0n;
        const inputCost = safeNumber(inputMicrounits.toString()) / 1_000_000;
        output.usage = { input: receipt.inputTokens, output: receipt.outputTokens,
          cacheRead: 0, cacheWrite: 0, totalTokens: receipt.totalTokens,
          cost: { input: inputCost, output: quoted ? charged - inputCost : 0, cacheRead: 0, cacheWrite: 0, total: charged } };
        output.diagnostics = [{ type: 'possums_settled_receipt', timestamp: Date.now(), details: {
          chargedMicrounits: receipt.chargedMicrounits, refundedMicrounits: receipt.refundedMicrounits,
          quotedCostComponents: quoted, finish: receipt.finish,
        } }];
        if (epoch !== this.epoch) throw new Error('possums_run_replaced');
        if (args.size && receipt.finish === 'length') throw new Error('possums_incomplete_tool_round');
        const toolUse = receipt.finish === 'tool_calls' || (receipt.finish === 'stop' && args.size > 0);
        for (const pending of args.values()) {
          if (!pending.block.id || !pending.block.name) throw new Error('possums_invalid_stream');
          pending.block.arguments = JSON.parse(pending.json);
          events.push({ type: 'toolcall_end', contentIndex: pending.contentIndex, toolCall: pending.block, partial: output });
        }
        // Pi omits a recoverable length attempt before the compaction hook.
        // A non-retryable terminal error preserves this paid partial answer;
        // the authenticated finish remains recorded in the receipt diagnostic.
        output.stopReason = toolUse ? 'toolUse' : 'stop';
        if (receipt.finish === 'length') {
          output.stopReason = 'error';
          output.errorMessage = 'Possums: partial answer; charge settled. Not automatically continued. A deliberate new request may incur another charge.';
        }
        if (epoch === this.epoch) this.guard.complete(selected.id, toolUse ? 'tool_calls' : receipt.finish, [...args.values()].map(call => ({ id: call.block.id, name: call.block.name })));
        if (output.stopReason === 'error') events.push({ type: 'error', reason: 'error', error: output });
        else events.push({ type: 'done', reason: output.stopReason, message: output });
      } catch (error) {
        if (epoch === this.epoch) this.guard.close();
        output.stopReason = options.signal?.aborted ? 'aborted' : 'error';
        if (!output.diagnostics?.some(diagnostic => diagnostic.type === 'possums_settled_receipt') &&
          ((error instanceof GatewayError && error.billing === 'unknown') ||
            (!(error instanceof GatewayError) && (options.signal?.aborted || (error instanceof ChannelError && error.code === 'uncertain'))))) {
          output.diagnostics = [{ type: 'possums_billing_unknown', timestamp: Date.now(), details: { receipt: false } }];
        }
        output.errorMessage = output.diagnostics?.some(diagnostic => diagnostic.type === 'possums_settled_receipt') ?
          safeFailure(error) : options.signal?.aborted && !(error instanceof GatewayError) ?
          'Possums: interrupted; charge unknown. Not replayed. A new request may incur another charge.' : safeFailure(error);
        events.push({ type: 'error', reason: output.stopReason, error: output });
      } finally { events.end(); }
    })();
    return events;
  }
}
