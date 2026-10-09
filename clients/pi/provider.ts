import { compact, type ExtensionContext, type SessionBeforeCompactEvent, type SessionBeforeCompactResult } from '@earendil-works/pi-coding-agent';
import type { StreamFn } from '@earendil-works/pi-agent-core';
import {
  createAssistantMessageEventStream, parseStreamingJson, envApiKeyAuth,
  type ApiKeyAuth,
  type Api, type ApiStreamOptions, type AssistantMessageEventStream, type StreamOptions,
  type AssistantMessage, type Model, type Provider, type SimpleStreamOptions,
  type TranscriptContext, type ToolCall,
} from '@earendil-works/pi-ai';
import { Channel } from '../../examples/phase01/transport.js';
import { ReferenceClient, CatalogFailure, GatewayError, gatewayDetailLabel, type LiveModel, type CompletionEvent } from '../../examples/phase01/client.js';
import { ChannelError, JSONDepthError, LIMITS, Operation } from '../../examples/phase01/limits.js';
import { ConnectionFailure, connectionFailure, catalogConnectionFailure } from './diagnostics.js';
import { ReplayGuard } from './replay.js';
import { invocation } from './wire.js';

export const PROVIDER_ID = 'possums';
const API = 'openai-completions';
const ORIGIN = 'https://possum-phase0.possums.containers.tinfoil.dev';
const LEGACY_AUTH = 'possums-memory-only-session';
const REQUEST_AUTH = 'possums-verified-memory-session';
const recoveryAuth = envApiKeyAuth('Possums recovery credential', ['POSSUMS_RECOVERY_CREDENTIAL']);
const isRecoveryKey = (key: string | undefined): key is string => !!key && key !== LEGACY_AUTH && key !== REQUEST_AUTH;
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
function catalogModels(entries: readonly LiveModel[]): readonly Model<typeof API>[] {
  try { return entries.map(model); }
  catch { throw new CatalogFailure('conversion'); }
}
function blank(selected: Model<typeof API>): AssistantMessage {
  return { role: 'assistant', provider: PROVIDER_ID, api: API, model: selected.id,
    timestamp: Date.now(), content: [], stopReason: 'pending',
    usage: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 0,
      cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } } };
}
const localFailures: Readonly<Record<string, string>> = Object.freeze({
  possums_compaction_failed: 'Native summary unavailable.',
  possums_summary_empty: 'Summary was empty.',
  possums_summary_tools: 'Summary attempted tool use; no tool executed.',
  possums_summary_length: 'Summary hit the output limit.',
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
  if (error instanceof JSONDepthError) return '[possums_request_json_depth] Request encoding exceeds the JSON nesting limit. Simplify tool schemas or history. No inference request sent. Not replayed.';
  if (error instanceof ConnectionFailure) return error.message;
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
  readonly name = 'Possums (verified)';
  readonly baseUrl = ORIGIN;
  private client: ReferenceClient | undefined;
  private catalog: readonly LiveModel[] = [];
  private listed: readonly Model<typeof API>[] = [];
  private readonly guard = new ReplayGuard();
  private newConversation = true;
  private epoch = 0;
  private authEpoch = 0;
  private recoveryKey: string | undefined;
  private restoring: Promise<void> | undefined;
  private trust: Promise<ReferenceClient> | undefined;
  private verifiedTemplate: ReferenceClient | undefined;
  private trustController = new AbortController();
  private trustEpoch = 0;
  private sessionClosed = true;

  constructor(
    private readonly establish: (signal: AbortSignal) => Promise<ReferenceClient>,
    private readonly reportConnection: (failure: ConnectionFailure | undefined) => void = () => {},
  ) {}

  private trustedTemplate(): Promise<ReferenceClient> {
    if (this.sessionClosed) return Promise.reject(new ConnectionFailure('session_unavailable'));
    if (!this.trust) {
      const epoch = this.trustEpoch;
      const signal = this.trustController.signal;
      this.trust = (async () => {
        try {
          const candidate = await this.establish(signal);
          Channel.requireVerified(candidate.channel);
          if (epoch !== this.trustEpoch || signal.aborted) throw new Error('possums_run_replaced');
          this.verifiedTemplate = candidate;
          return candidate;
        } catch (error) { throw connectionFailure(error); }
      })();
      // Cache failures too: login/model refresh cannot silently select another release.
      void this.trust.catch(() => {});
    }
    return this.trust;
  }

  async verifySession(): Promise<void> {
    const epoch = this.trustEpoch;
    try { await this.trustedTemplate(); }
    catch (error) {
      if (epoch !== this.trustEpoch || this.sessionClosed) throw new Error('possums_run_replaced');
      const failure = connectionFailure(error);
      try { this.reportConnection(failure); } catch { /* Transient UI only. */ }
      throw failure;
    }
  }

  get release(): Readonly<{ tag: string; expires: number }> | undefined {
    return this.verifiedTemplate?.channel.release;
  }

  private async establishVerified(signal: AbortSignal, epoch: number): Promise<ReferenceClient> {
    const op = new Operation(LIMITS.operationMs, signal);
    try {
      const template = await op.wait(this.trustedTemplate(), LIMITS.operationMs);
      this.requireCurrent(epoch, signal);
      // Public trust is shared; mutable bearer state is never shared between accounts.
      return template.freshSession();
    } catch (error) {
      this.requireCurrent(epoch, signal);
      const failure = connectionFailure(error);
      try { this.reportConnection(failure); } catch { /* UI reporting cannot change authentication. */ }
      throw failure;
    } finally { op.close(); }
  }

  readonly auth: { apiKey: ApiKeyAuth } = { apiKey: {
    name: recoveryAuth.name,
    // Only select configured material here: availability/listing must not connect.
    check: async input => {
      const result = await recoveryAuth.resolve(input);
      return isRecoveryKey(result?.auth.apiKey) ? { type: 'api_key', source: result.source } : undefined;
    },
    resolve: async input => {
      const epoch = this.authEpoch;
      const result = await recoveryAuth.resolve(input);
      this.requireCurrent(epoch, input.signal);
      const key = result?.auth.apiKey;
      if (!isRecoveryKey(key)) return undefined;
      if (key !== this.recoveryKey) {
        this.logout();
        this.recoveryKey = key;
      }
      const current = this.authEpoch;
      if (!this.client) {
        if (!this.restoring) {
          const pending = (async () => {
            const candidate = await this.establishVerified(input.signal, current);
            await this.authenticate(candidate, key, current, input.signal);
          })();
          this.restoring = pending;
          // An obsolete attempt must not clear a replacement's in-flight work.
          void pending.finally(() => { if (this.restoring === pending) this.restoring = undefined; }).catch(() => {});
        }
        try { await this.restoring; }
        catch (error) {
          if (error instanceof ConnectionFailure) throw error;
          throw new Error('possums_session_unavailable');
        }
      }
      this.requireCurrent(current, input.signal);
      // Never expose a recovery key or bearer token to Pi's request hooks.
      return { auth: { apiKey: REQUEST_AUTH }, source: result?.source };
    },
    login: async interaction => {
      this.logout();
      const epoch = this.authEpoch;
      // Verify public evidence and the actual channel before requesting a secret.
      const candidate = await this.establishVerified(interaction.signal, epoch);
      const credential = await recoveryAuth.login!(interaction);
      this.requireCurrent(epoch, interaction.signal);
      if (!isRecoveryKey(credential.key)) throw new Error('possums_session_unavailable');
      await this.authenticate(candidate, credential.key, epoch, interaction.signal);
      // Native Pi login persists this ApiKeyCredential in its existing auth store.
      return credential;
    },
  } };

  private requireCurrent(epoch: number, signal: AbortSignal): void {
    if (epoch !== this.authEpoch || signal.aborted) throw new Error('possums_login_cancelled');
  }

  private async authenticate(candidate: ReferenceClient, key: string, epoch: number, signal: AbortSignal): Promise<void> {
    let stage: ConnectionFailure['code'] = 'session_unavailable';
    try {
      await candidate.login(key, signal);
      this.requireCurrent(epoch, signal);
      stage = 'catalog_unavailable';
      const entries = await candidate.models(signal);
      const listed = catalogModels(entries);
      this.requireCurrent(epoch, signal);
      this.recoveryKey = key;
      this.client = candidate; this.catalog = entries; this.listed = listed;
      // Public verification alone must not reset failed-session warning deduplication.
      try { this.reportConnection(undefined); } catch { /* UI reporting cannot change authentication. */ }
    } catch (error) {
      this.requireCurrent(epoch, signal);
      const failure = stage === 'catalog_unavailable' ? catalogConnectionFailure(error) : new ConnectionFailure(stage);
      try { this.reportConnection(failure); } catch { /* UI reporting cannot change authentication. */ }
      throw failure;
    }
  }

  getModels(): readonly Model<typeof API>[] { return this.listed; }
  beginRun(): void { this.epoch++; this.guard.beginRun(); }
  endRun(): void { this.guard.close(); }
  newSession(): void {
    this.shutdown();
    this.sessionClosed = false;
    this.trustController = new AbortController();
    void this.trustedTemplate().catch(() => {});
  }
  shutdown(): void {
    this.sessionClosed = true;
    this.trustController.abort();
    this.trustEpoch++;
    this.trust = undefined;
    this.verifiedTemplate = undefined;
    this.logout();
  }
  logout(): void {
    this.epoch++; this.authEpoch++; this.guard.close();
    this.client = undefined; this.catalog = []; this.listed = [];
    this.recoveryKey = undefined; this.restoring = undefined;
    this.newConversation = true;
  }

  async refreshModels(context: Parameters<NonNullable<Provider['refreshModels']>>[0]): Promise<void> {
    if (!context.allowNetwork) {
      // Pi supplies the stored credential in the offline phase, including after
      // native /logout. No ambient lookup or network is needed to revoke state.
      const key = context.credential?.type === 'api_key' ? context.credential.key : undefined;
      await context.publish({ update: () => {
        if (!isRecoveryKey(key) || key !== this.recoveryKey) this.logout();
      } });
      return;
    }
    // The network phase receives resolve()'s effective, non-secret credential.
    if (context.credential?.type !== 'api_key' || context.credential.key !== REQUEST_AUTH || !this.client) return;
    const client = this.client;
    const epoch = this.authEpoch;
    const current = () => epoch === this.authEpoch && !context.signal.aborted;
    try {
      const entries = await client.models(context.signal);
      const listed = catalogModels(entries);
      await context.publish({ update: () => {
        if (current()) {
          this.catalog = entries; this.listed = listed;
          try { this.reportConnection(undefined); } catch { /* UI reporting cannot change catalog state. */ }
        }
      } });
    } catch (error) {
      const failure = catalogConnectionFailure(error);
      // Neither a stale success nor a stale failure may mutate a newer account.
      await context.publish({ update: () => {
        if (current()) {
          this.catalog = []; this.listed = [];
          try { this.reportConnection(failure); } catch { /* UI reporting cannot change catalog state. */ }
        }
      } });
      throw failure;
    }
  }

  stream<T extends Api>(selected: Model<T>, context: TranscriptContext, options?: ApiStreamOptions<T>): AssistantMessageEventStream {
    return this.perform(selected as Model<typeof API>, context, options as RequestOptions);
  }

  streamSimple(selected: Model<Api>, context: TranscriptContext, options?: SimpleStreamOptions): AssistantMessageEventStream {
    return this.perform(selected as Model<typeof API>, context, options);
  }

  // Native compaction is separate from run authorization: it must not consume a
  // ready run or a receipted tool continuation, even when its second call fails.
  async compact(event: SessionBeforeCompactEvent, ctx: ExtensionContext): Promise<SessionBeforeCompactResult> {
    const selected = ctx.model;
    const epoch = this.epoch;
    const authEpoch = this.authEpoch;
    let closed = false;
    let stage = 'authorization';
    let failure: string | undefined;
    let settled = 0n;
    let unknown = false;
    const requireScope = () => {
      if (closed || epoch !== this.epoch || authEpoch !== this.authEpoch || event.signal.aborted) {
        throw new Error('possums_run_replaced');
      }
    };
    try {
      if (event.willRetry) throw new Error('possums_automatic_replay_blocked');
      requireScope();
      const auth = await ctx.modelRegistry.getProviderAuth(PROVIDER_ID);
      requireScope();
      if (!selected || selected.provider !== PROVIDER_ID || auth?.auth.apiKey !== REQUEST_AUTH || !this.client) {
        throw new Error('possums_session_unavailable');
      }
      let calls = 0;
      const streamFn: StreamFn = async (requested, context, options) => {
        requireScope();
        if (requested !== selected) throw new Error('possums_request_options_unsupported');
        stage = `summary ${++calls}`;
        // Only this closed-over native callback drops the SDK's advisory cap.
        // The gateway still reserves/grants the selected model's full allowance.
        const { maxTokens: _nativeHint, ...rest } = options ?? {};
        const stream = this.perform(selected as Model<typeof API>, context, { ...rest, signal: event.signal }, requireScope);
        const result = await stream.result();
        for (const diagnostic of result.diagnostics ?? []) {
          if (diagnostic.type === 'possums_settled_receipt' && typeof diagnostic.details?.chargedMicrounits === 'string') {
            settled += BigInt(diagnostic.details.chargedMicrounits);
          }
          if (diagnostic.type === 'possums_billing_unknown') unknown = true;
        }
        // perform authors these messages locally; never display native exceptions.
        if (result.stopReason !== 'stop') {
          failure = result.errorMessage;
          throw new Error('possums_compaction_failed');
        }
        requireScope();
        return stream;
      };
      // No retry policy: Pi 1.0.4 makes one attempt per native summary (one or
      // two sequential summaries). Native code owns prompts, files and usage.
      const headers = Object.fromEntries(Object.entries(auth.auth.headers ?? {}).filter((entry): entry is [string, string] => typeof entry[1] === 'string'));
      const result = await compact(event.preparation, selected, auth.auth.apiKey, headers,
        event.customInstructions, event.signal, ctx.thinkingLevel, streamFn, auth.env);
      requireScope();
      return { compaction: result };
    } catch (error) {
      const billing = `${settled > 0n ? ` Observed settled charges: $${dollars(settled.toString())}; not undone.` : ''}${unknown ? ' Additional charge unknown.' : ''}`;
      // ExtensionRunner swallows thrown hooks and would then use the default
      // summarizer. Even a failing UI must not turn this into a fallback/replay.
      try { ctx.ui.notify(`Possums compaction (${stage}): ${failure ?? safeFailure(error)}${billing} No checkpoint saved. Not replayed. Use /compact deliberately or /new.`, 'warning'); }
      catch { /* Transient UI only; never log or persist failed summaries. */ }
      return { cancel: true };
    } finally { closed = true; }
  }

  private perform(selected: Model<typeof API>, context: TranscriptContext, options: RequestOptions = {}, summary?: () => void): AssistantMessageEventStream {
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
        if (summary) summary();
        else this.guard.claim(selected.id, context.messages);
        if (!this.client || options.signal?.aborted) throw new Error('possums_session_unavailable');
        if (selected.provider !== PROVIDER_ID || selected.api !== API || selected.baseUrl !== ORIGIN || options.fetch || options.maxTokens !== undefined || options.samplingParams || options.temperature !== undefined || options.reasoning !== undefined || options.reasoningEffort !== undefined) {
          throw new Error('possums_request_options_unsupported');
        }
        const payload = invocation(selected, context);
        if (summary && payload.tools?.length) throw new Error('possums_summary_tools');
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
        }, summary ? true : this.newConversation, {
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
                const block: ToolCall = { type: 'toolCall', id: call.id ?? '', name: call.function?.name ?? '', arguments: {} };
                pending = { block, contentIndex: output.content.length, json: '' };
                args.set(call.index, pending); output.content.push(block);
                if (!summary) events.push({ type: 'toolcall_start', contentIndex: pending.contentIndex, partial: output });
              }
              if (call.id !== undefined) pending.block.id = call.id;
              if (call.function?.name !== undefined) pending.block.name = call.function.name;
              const fragment = call.function?.arguments ?? '';
              pending.json += fragment;
              pending.block.arguments = parseStreamingJson(pending.json);
              if (fragment && !summary) events.push({ type: 'toolcall_delta', contentIndex: pending.contentIndex, delta: fragment, partial: output });
            }
          },
        });
        if (!summary && epoch === this.epoch) this.newConversation = false;
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
        if (epoch !== this.epoch || options.signal?.aborted) throw new Error('possums_run_replaced');
        if (summary) {
          summary();
          if (args.size || receipt.finish === 'tool_calls') throw new Error('possums_summary_tools');
          if (receipt.finish !== 'stop') throw new Error('possums_summary_length');
          if (!output.content.some(block => block.type === 'text' && block.text.trim())) throw new Error('possums_summary_empty');
        }
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
        if (!summary && epoch === this.epoch) this.guard.complete(selected.id, toolUse ? 'tool_calls' : receipt.finish, [...args.values()].map(call => ({ id: call.block.id, name: call.block.name })));
        if (output.stopReason === 'error') events.push({ type: 'error', reason: 'error', error: output });
        else events.push({ type: 'done', reason: output.stopReason, message: output });
      } catch (error) {
        if (!summary && epoch === this.epoch) this.guard.close();
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
