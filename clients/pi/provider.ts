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
import { ReferenceClient, BalanceFailure, CatalogFailure, GatewayError, gatewayDetailLabel, type BalanceSnapshot, type LiveModel, type CompletionEvent } from '../../examples/phase01/client.js';
import { ChannelError, DiagnosticFailure, OperationFailure, JSONDepthError, LIMITS, Operation, parseJSON, serialize, requireThat, type FailureStage } from '../../examples/phase01/limits.js';
import { ConnectionFailure, connectionFailure, catalogConnectionFailure, diagnosticDescription } from './diagnostics.js';
import { invocation } from './wire.js';

export const PROVIDER_ID = 'possums';
const API = 'openai-completions';
const ORIGIN = 'https://possum-phase0.possums.containers.tinfoil.dev';
const LEGACY_AUTH = 'possums-memory-only-session';
const REQUEST_AUTH = 'possums-verified-memory-session';
const recoveryAuth = envApiKeyAuth('Possums recovery credential', ['POSSUMS_RECOVERY_CREDENTIAL']);
const isRecoveryKey = (key: string | undefined): key is string => !!key && key !== LEGACY_AUTH && key !== REQUEST_AUTH;
type RequestOptions = StreamOptions & { reasoning?: unknown; reasoningEffort?: unknown; toolChoice?: unknown };

function safeNumber(value: string, stage: 'catalog' | 'settlement' = 'catalog'): number {
  const result = Number(value);
  if (!Number.isSafeInteger(result) || result < 0) throw new DiagnosticFailure(stage, 'precision');
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
// Persist only public selection metadata using Pi's generation-checked store.
// Costs/limits are historical, explicitly NOT current quotes or authorization.
function discoveryModels(value: unknown): readonly Model<typeof API>[] {
  try {
    const entries = parseJSON(serialize(value, LIMITS.catalog), LIMITS.catalog);
    requireThat(Array.isArray(entries) && entries.length <= LIMITS.models);
    const ids = new Set<string>();
    return Object.freeze(entries.map(entry => {
      requireThat(entry && entry.provider === PROVIDER_ID && entry.api === API && entry.baseUrl === ORIGIN);
      requireThat(typeof entry.id === 'string' && /^[A-Za-z0-9._:/-]{1,128}$/.test(entry.id) && !ids.has(entry.id));
      ids.add(entry.id);
      requireThat([entry.contextWindow, entry.maxTokens].every(n => Number.isSafeInteger(n) && n > 0) && entry.maxTokens <= entry.contextWindow);
      const cost = entry.cost;
      requireThat(cost && ['input', 'output', 'cacheRead', 'cacheWrite'].every(key => typeof cost[key] === 'number' && Number.isFinite(cost[key]) && cost[key] >= 0));
      return Object.freeze({ id: entry.id, provider: PROVIDER_ID, api: API, baseUrl: ORIGIN,
        name: `${entry.id} · last-known metadata · prices/limits not current · inference unavailable`,
        input: ['text'] as ['text'], reasoning: false, contextWindow: entry.contextWindow, maxTokens: entry.maxTokens,
        cost: Object.freeze({ input: cost.input, output: cost.output, cacheRead: cost.cacheRead, cacheWrite: cost.cacheWrite }) });
    }));
  } catch { return []; } // Untrusted cache cannot supply transport options or diagnostics.
}
function blank(selected: Model<typeof API>): AssistantMessage {
  return { role: 'assistant', provider: PROVIDER_ID, api: API, model: selected.id,
    timestamp: Date.now(), content: [], stopReason: 'pending',
    usage: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 0,
      cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } } };
}
const localFailures: Readonly<Record<string, string>> = Object.freeze({
  possums_balance_request_failed: 'Balance read failed. This command sent no inference; prior charges are unchanged or unknown.',
  possums_balance_validation_failed: 'Balance response failed validation. This command sent no inference; prior charges are unchanged or unknown.',
  possums_reconcile_busy: 'Wait until Pi finishes its run, retries and compaction before taking a snapshot.',
  possums_reconcile_pending: 'Account has unfinished reservations. Wait for them to finish before taking a snapshot.',
  possums_reconcile_started: 'Reconciliation already started. Use finish or cancel first.',
  possums_reconcile_not_started: 'No reconciliation window. Use /possums-reconcile start first.',
  possums_reconcile_scope_changed: 'Session or account changed. Start a fresh reconciliation window.',
  possums_compaction_failed: 'Native summary unavailable.',
  possums_summary_empty: 'Summary was empty.',
  possums_summary_tools: 'Summary attempted tool use; no tool executed.',
  possums_summary_length: 'Summary hit the output limit.',
  possums_catalog_unavailable: 'Model catalog unavailable. Use /login to refresh the session.',
  possums_incomplete_tool_round: 'Tool response incomplete; no tool executed. Charge settled. Not automatically continued. A new request may incur another charge.',
  possums_invalid_stream: 'Pi stream assembly violated the validated event ordering/identity constraint; no tool executed. Share this code for support.',
  possums_login_cancelled: 'Login cancelled. Use /login to start again.',
  possums_request_options_unsupported: 'Request options unsupported; remove custom options.',
  possums_run_replaced: 'Session changed; no tool executed. Any settled charge remains recorded.',
  possums_session_unavailable: 'Session unavailable. Use /login to authenticate.',
  possums_model_unavailable: 'Selected model is absent from the live authenticated catalog. Refresh models or select an available model deliberately. No model substituted; no inference request sent.',
  possums_tools_unsupported: 'Selected model is not qualified for tools. Use /possums-text-only or select a qualified model. No inference request sent.',
  possums_automatic_replay_blocked: 'Automatic replay blocked. Submit a deliberate new request if needed.',
  possums_constrained_sampling_unsupported: 'Constrained sampling unsupported; remove the option.',
  possums_custom_tools_unsupported: 'Custom tools unsupported; remove the option.',
  possums_duplicate_tool_call: 'Duplicate tool call in history; correct the history.',
  possums_images_unsupported: 'Images unsupported; remove images before sending.',
  possums_mixed_provider_history: 'Mixed provider history unsupported; start a new session.',
  possums_orphan_tool_result: 'Tool result has no matching call; correct the history.',
  possums_unresolved_tool_calls: 'Tool calls lack results; complete the history.',
});
// Pi 1.0.4 classifies errorMessage, with no provider classifier hook. Only these
// closed gateway operational failures opt into its native bounded retry policy.
// HTTP details do not expose upstream status; this is not a claim of HTTP 503.
const transientDetails = new Set(['tokenizer_send_failed', 'tokenizer_http_failed',
  'generation_send_failed', 'generation_http_failed', 'stream_transport_failed',
  'stream_idle_timeout', 'stream_deadline_exceeded']);
function safeFailure(error: unknown, nativeRetry = false, stage: FailureStage = 'provider'): string {
  if (error instanceof CatalogFailure) return catalogConnectionFailure(error).message;
  if (error instanceof BalanceFailure && error.observation) return `${diagnosticDescription(error.observation)} This balance command sent no inference; prior billing is unchanged or unknown.`;
  if (error instanceof DiagnosticFailure || error instanceof OperationFailure) {
    const failure = error instanceof DiagnosticFailure ? error : new DiagnosticFailure(stage, error.constraint, 'uncertain');
    return `Possums: ${diagnosticDescription(failure)} Billing status: Charge unknown. Not replayed. A new request may incur another charge.`;
  }
  if (error instanceof JSONDepthError) return '[possums_request_json_depth] Stage: request; constraint: json_depth. Request encoding exceeds the JSON nesting limit. Simplify tool schemas or history. No inference request sent. Not replayed.';
  if (error instanceof ConnectionFailure) return `${error.message} No automatic retry for this connection/billing check.`;
  if (error instanceof GatewayError) {
    const reason: Readonly<Record<string, string>> = {
      insufficient_credit: 'Insufficient credit for this model’s maximum reservation.',
      unauthorized: 'Gateway rejected authentication; use /login again. Expiry is not established.',
      invalid_request: 'Request rejected by gateway.', account_limit: 'Account limit reached.',
      duplicate_request: 'Duplicate submission rejected.', unavailable: 'Gateway unavailable.',
      generation_failed: 'Generation failed.',
    };
    const label = gatewayDetailLabel(error.detail);
    const known = Object.hasOwn(reason, error.reason);
    const detailStages: Readonly<Record<string, string>> = {
      settlement_failed: 'settlement', sdk_stream_decode_failed: 'gateway_sdk_decoding',
      endpoint_binding_failed: 'gateway_verified_transport', verification_failed: 'gateway_verification',
      catalog_failed: 'gateway_catalog', request_encoding_failed: 'gateway_request_encoding',
    };
    const detail = label ? error.detail! : '';
    const gatewayStage = error.reason === 'unauthorized' ? 'gateway_authentication' :
      Object.hasOwn(detailStages, detail) ? detailStages[detail] :
      detail.startsWith('stream_') || detail.startsWith('tool_') ? 'gateway_stream' : 'gateway_admission';
    const action = error.reason === 'insufficient_credit' ? 'Add credit before submitting this model again.' : error.reason === 'invalid_request' ? 'Review supported request options and context limits.' : error.reason === 'account_limit' ? 'Wait for pending requests before submitting again.' : error.reason === 'unauthorized' ? 'Check authentication, not trust pins.' : 'If this persists, share only the content-free code/stage/constraint for support.';
    const description = `${known ? `[${error.reason}] ${reason[error.reason]}` : '[possums_gateway_unclassified] Gateway returned an unclassified category; underlying cause unknown.'}${label ? ` ${label}.` : error.detail === 'unclassified' ? ' [possums_gateway_detail_unclassified] Gateway detail is unclassified; underlying cause unknown.' : ''} Stage: ${gatewayStage}; constraint: ${error.detail === 'unclassified' ? 'unclassified' : label ? error.detail : known ? error.reason : 'unclassified'}.${error.status === undefined ? '' : ` Observed HTTP status: ${error.status}.`} ${action}`;
    const transient = (error.reason === 'unavailable' && (error.detail === undefined || error.detail === 'inference_unavailable')) ||
      (error.reason === 'generation_failed' && error.detail !== undefined && transientDetails.has(error.detail));
    if (nativeRetry && transient) {
      return `Possums: Transient service unavailable. ${description} ` +
        (error.billing === 'refunded' ? 'Reservation refunded.' : 'Charge unknown.') + ' Native automatic retry may incur another charge.';
    }
    // "Billing" explicitly excludes terminal gateway diagnostics from Pi's broad
    // classifier, even if a detail contains e.g. "timeout". Summaries never retry.
    return `Possums: ${description} Billing status: ` +
      (error.billing === 'refunded' ? 'Reservation refunded.' : 'Charge unknown. A new request may incur another charge.') + ' Not replayed.';
  }
  if (error instanceof ChannelError && error.code === 'uncertain') return `Possums: [possums_${stage}_uncertain] Stage: ${stage}; constraint: outcome_unknown. Request interrupted; charge unknown. Cause unknown; check connectivity and share only this code for support. Not replayed. A new request may incur another charge.`;
  // Only exact internal literals are displayable. Never echo a hook exception.
  if (error instanceof Error && Object.hasOwn(localFailures, error.message)) return `[${error.message}] Stage: ${stage}; constraint: ${error.message.slice('possums_'.length)}. ${localFailures[error.message]}`;
  return `Possums: ${diagnosticDescription(new DiagnosticFailure(stage, 'unexpected', 'uncertain'))} Billing status: Charge unknown. Not replayed. A new request may incur another charge.`;
}

export class PossumsProvider implements Provider {
  readonly id = PROVIDER_ID;
  readonly name = 'Possums (verified)';
  readonly baseUrl = ORIGIN;
  private client: ReferenceClient | undefined;
  private catalog: readonly LiveModel[] = [];
  private listed: readonly Model<typeof API>[] = [];
  private discovery: readonly Model<typeof API>[] = [];
  private failure: ConnectionFailure | undefined;
  private activeRunSignal: AbortSignal | undefined;
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
  private reconciliation: { before: BalanceSnapshot; charged: bigint; completed: bigint; unknown: boolean } | undefined;
  private reconciliationEpoch = 0;
  private pendingInput: boolean | undefined;
  private renewalArmed = false;
  private renewalSignal: AbortSignal | undefined;
  private renewalCandidateSignal: AbortSignal | undefined;
  private renewalController: AbortController | undefined;
  private renewalRequired = false;
  private renewalFailure: ConnectionFailure | undefined;

  constructor(
    private readonly establish: (signal: AbortSignal) => Promise<ReferenceClient>,
    private readonly reportConnection: (failure: ConnectionFailure | undefined) => void = () => {},
  ) {}

  get currentFailure(): ConnectionFailure | undefined { return this.failure; }
  private observeFailure(failure: ConnectionFailure | undefined): void {
    this.failure = failure;
    this.reportConnection(failure);
  }

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
      try { this.observeFailure(failure); } catch { /* Transient UI only. */ }
      throw failure;
    }
  }

  get release(): Readonly<{ tag: string; expires: number }> | undefined {
    return this.verifiedTemplate?.channel.release;
  }

  private async establishVerified(signal: AbortSignal, epoch: number): Promise<ReferenceClient> {
    const op = new Operation(LIMITS.bootstrapMs, signal);
    try {
      const template = await op.wait(this.trustedTemplate(), LIMITS.bootstrapMs);
      this.requireCurrent(epoch, signal);
      // Public trust is shared; mutable bearer state is never shared between accounts.
      return template.freshSession();
    } catch (error) {
      this.requireCurrent(epoch, signal);
      const failure = connectionFailure(error);
      try { this.observeFailure(failure); } catch { /* UI reporting cannot change authentication. */ }
      throw failure;
    } finally { op.close(); }
  }

  readonly auth: { apiKey: ApiKeyAuth } = { apiKey: {
    name: recoveryAuth.name,
    // Only select configured material here: availability/listing must not connect.
    check: async input => {
      const epoch = this.authEpoch;
      let result;
      try { result = await recoveryAuth.resolve(input); }
      catch {
        this.requireCurrent(epoch, input.signal);
        const failure = new ConnectionFailure('credential_resolution_failed');
        try { this.observeFailure(failure); } catch { /* Transient UI only. */ }
        throw failure;
      }
      this.requireCurrent(epoch, input.signal);
      return isRecoveryKey(result?.auth.apiKey) ? { type: 'api_key', source: result.source } : undefined;
    },
    resolve: async input => {
      // Consume once, before any await or expiry decision. Refresh/compaction
      // signals cannot borrow an explicit run's permission; retries get none.
      const diagnosticAttempt = this.activeRunSignal !== undefined && this.activeRunSignal === input.signal;
      const mayRenew = this.renewalSignal !== undefined && this.renewalSignal === input.signal;
      if (mayRenew) this.renewalSignal = undefined;
      if (this.renewalCandidateSignal === input.signal) this.renewalCandidateSignal = undefined;
      const epoch = this.authEpoch;
      let result;
      try { result = await recoveryAuth.resolve(input); }
      catch {
        this.requireCurrent(epoch, input.signal);
        const failure = new ConnectionFailure('credential_resolution_failed');
        try { this.observeFailure(failure); } catch { /* Transient UI only. */ }
        throw failure;
      }
      this.requireCurrent(epoch, input.signal);
      const key = result?.auth.apiKey;
      if (!isRecoveryKey(key)) {
        try { this.observeFailure(new ConnectionFailure('credential_missing')); } catch { /* Transient UI only. */ }
        return undefined;
      }
      if (key !== this.recoveryKey) {
        this.logout();
        this.recoveryKey = key;
      }
      let current = this.authEpoch;
      if (mayRenew && (this.renewalRequired ||
        (this.release !== undefined && Date.now() >= this.release.expires) ||
        (this.client?.authExpiresAt !== undefined && Date.now() >= this.client.authExpiresAt))) {
        current = await this.renewForSubmission(key, input.signal);
      } else if (this.renewalRequired) {
        // A failed/unfinished renewal cannot fall through to ordinary restore
        // on background discovery, an automatic retry or a tool continuation.
        throw this.renewalFailure ?? new ConnectionFailure('session_unavailable');
      }
      this.requireCurrent(current, input.signal);
      // Reporting an already observed setup failure in this run performs no
      // acquisition/auth/catalog retry. Deliberate model refresh and existing
      // authorized expiry renewal keep their separate, unchanged paths.
      if (diagnosticAttempt && this.failure && (!this.client || this.catalog.length === 0)) throw this.failure;
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
          throw error instanceof ConnectionFailure ? error : connectionFailure(error);
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
      let credential;
      try { credential = await recoveryAuth.login!(interaction); }
      catch {
        this.requireCurrent(epoch, interaction.signal);
        const failure = new ConnectionFailure('credential_entry_failed');
        try { this.observeFailure(failure); } catch { /* Transient UI only. */ }
        throw failure;
      }
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

  private async authenticate(candidate: ReferenceClient, key: string, epoch: number, signal: AbortSignal,
    renewedTemplate?: ReferenceClient): Promise<void> {
    let stage: ConnectionFailure['code'] = 'session_unavailable';
    this.requireCurrent(epoch, signal);
    this.recoveryKey = key; // Scope an observed setup failure without granting authorization.
    try {
      await candidate.login(key, signal);
      this.requireCurrent(epoch, signal);
      stage = 'catalog_unavailable';
      const entries = await candidate.models(signal);
      const listed = catalogModels(entries);
      this.requireCurrent(epoch, signal);
      // Publish public trust, account bearer and catalog in one synchronous
      // current-epoch step, never while verification/login is still pending.
      if (renewedTemplate) {
        this.trust = Promise.resolve(renewedTemplate);
        this.verifiedTemplate = renewedTemplate;
        this.renewalRequired = false;
        this.renewalFailure = undefined;
      }
      this.client = candidate; this.catalog = entries; this.listed = listed;
      this.discovery = discoveryModels(listed);
      // Public verification alone must not reset failed-session warning deduplication.
      try { this.observeFailure(undefined); } catch { /* UI reporting cannot change authentication. */ }
    } catch (error) {
      this.requireCurrent(epoch, signal);
      const failure = stage === 'catalog_unavailable' ? catalogConnectionFailure(error) : error instanceof DiagnosticFailure ? connectionFailure(error) : new ConnectionFailure(stage);
      try { this.observeFailure(failure); } catch { /* UI reporting cannot change authentication. */ }
      throw failure;
    }
  }

  private async renewForSubmission(key: string, signal: AbortSignal): Promise<number> {
    this.renewalController?.abort();
    const controller = new AbortController();
    this.renewalController = controller;
    const op = new Operation(null, AbortSignal.any([signal, controller.signal]));
    const epoch = ++this.authEpoch;
    this.epoch++;
    this.client = undefined; this.catalog = []; this.listed = []; this.restoring = undefined;
    this.renewalRequired = true; this.renewalFailure = undefined;
    // A changed verified instance or bearer cannot certify the old balance window.
    this.reconciliationEpoch++; this.reconciliation = undefined;
    try {
      let template = this.verifiedTemplate;
      if (!template || (template.release && Date.now() >= template.release.expires)) {
        this.trustController.abort();
        this.trustEpoch++;
        this.trustController = new AbortController();
        try {
          template = await op.wait(this.establish(op.controller.signal), LIMITS.bootstrapMs);
          this.requireCurrent(epoch, op.controller.signal);
          Channel.requireVerified(template.channel);
          // Existing compiled-manifest establishment still enforces its own
          // administrative expiry. Never change pins or extend certificates.
          if (template.release && Date.now() >= template.release.expires) throw new ConnectionFailure('verification_failed');
        } catch (error) {
          this.requireCurrent(epoch, op.controller.signal);
          const failure = connectionFailure(error);
          try { this.observeFailure(failure); } catch { /* Transient UI only. */ }
          throw failure;
        }
      }
      this.requireCurrent(epoch, op.controller.signal);
      await op.wait(this.authenticate(template.freshSession(), key, epoch, op.controller.signal, template), 3 * LIMITS.operationMs);
      return epoch;
    } catch (error) {
      if (epoch === this.authEpoch) this.renewalFailure = error instanceof ConnectionFailure ? error : new ConnectionFailure('session_unavailable');
      throw error;
    } finally {
      op.close();
      if (this.renewalController === controller) this.renewalController = undefined;
    }
  }

  getModels(): readonly Model<typeof API>[] { return this.listed.length ? this.listed : this.discovery; }
  markInput(explicit: boolean, activeSignal?: AbortSignal): void {
    // Overlapping/handled idle inputs have no durable Pi submission ID. Deny
    // ambiguous correlation rather than lend human permission to another input.
    this.pendingInput = this.pendingInput === undefined ? explicit : false;
    if (!activeSignal) this.renewalArmed = false;
  }
  beginRun(): void {
    this.epoch++;
    this.renewalArmed = this.pendingInput === true;
    this.pendingInput = undefined;
  }
  bindRun(signal?: AbortSignal): void {
    this.activeRunSignal = signal;
    this.renewalSignal = undefined;
    this.renewalCandidateSignal = this.renewalArmed && signal && !signal.aborted ? signal : undefined;
    this.renewalArmed = false;
  }
  confirmRunInput(user: boolean, signal?: AbortSignal): void {
    // Native prompt runs emit an initial user message before request auth;
    // sendCustomMessage can start a nested run without input/before_agent_start.
    // Only the first non-system message can confirm the marked candidate.
    if (this.renewalCandidateSignal && this.renewalCandidateSignal === signal) {
      this.renewalSignal = user ? signal : undefined;
      this.renewalCandidateSignal = undefined;
    }
  }
  endRun(): void { this.activeRunSignal = undefined; this.renewalSignal = undefined; this.renewalCandidateSignal = undefined; this.renewalArmed = false; }
  settleRun(): void { this.endRun(); this.pendingInput = undefined; }
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
    this.renewalController?.abort();
    this.renewalController = undefined;
    this.renewalRequired = false; this.renewalFailure = undefined;
    this.settleRun();
    this.epoch++; this.authEpoch++;
    this.client = undefined; this.catalog = []; this.listed = [];
    this.failure = undefined;
    this.recoveryKey = undefined; this.restoring = undefined;
    this.newConversation = true;
    this.reconciliationEpoch++;
    this.reconciliation = undefined;
  }

  private async balance(ctx: ExtensionContext): Promise<BalanceSnapshot> {
    const auth = await ctx.modelRegistry.getProviderAuth(PROVIDER_ID);
    const epoch = this.authEpoch;
    if (auth?.auth.apiKey !== REQUEST_AUTH || !this.client) {
      throw new Error('possums_reconcile_scope_changed');
    }
    const client = this.client;
    try {
      const snapshot = await client.balance(ctx.signal);
      if (epoch !== this.authEpoch || client !== this.client || ctx.signal?.aborted) {
        throw new Error('possums_reconcile_scope_changed');
      }
      return snapshot;
    } catch (error) {
      if (error instanceof BalanceFailure && !error.observation) throw new Error(error.message);
      throw error;
    }
  }

  async reconcile(action: string, ctx: ExtensionContext): Promise<string> {
    if (action === 'cancel') {
      this.reconciliationEpoch++;
      this.reconciliation = undefined;
      return 'Reconciliation window discarded. No inference or accounting change.';
    }
    if (action !== 'start' && action !== 'finish') return 'Use /possums-reconcile start, finish or cancel. These commands do not initiate inference.';
    if (!ctx.isIdle()) throw new Error('possums_reconcile_busy');
    const window = this.reconciliation;
    const epoch = this.reconciliationEpoch;
    if (action === 'start' && window) throw new Error('possums_reconcile_started');
    if (action === 'finish' && !window) throw new Error('possums_reconcile_not_started');
    const snapshot = await this.balance(ctx);
    if (!ctx.isIdle()) throw new Error('possums_reconcile_busy');
    if (snapshot.inFlight !== 0) throw new Error('possums_reconcile_pending');
    if (epoch !== this.reconciliationEpoch || window !== this.reconciliation) throw new Error('possums_reconcile_scope_changed');
    if (action === 'start') {
      this.reconciliation = { before: snapshot, charged: 0n, completed: 0n, unknown: false };
      return `Reconciliation started. Available demo credit: $${dollars(snapshot.availableMicrounits)}. Run your chosen Possums task, then /possums-reconcile finish. New inference, retries and compaction may charge credit.`;
    }
    if (!window) throw new Error('possums_reconcile_not_started');
    this.reconciliation = undefined;
    const completed = BigInt(snapshot.completedRequests) - BigInt(window.before.completedRequests);
    if (window.unknown || completed !== window.completed) {
      return 'Reconciliation unavailable: an outcome is unknown, accounting changed outside the observed window, or the ledger reset. No confirmed match or refund is inferred.';
    }
    const debit = BigInt(window.before.availableMicrounits) - BigInt(snapshot.availableMicrounits);
    if (debit !== window.charged) return 'Reconciliation mismatch: balance change differs from authenticated receipt charges. No refund or upstream-invoice agreement is inferred.';
    return `Reconciliation matched: balance debit $${dollars(window.charged.toString())} equals authenticated receipt charges across ${completed} completed requests (including confirmed refunds). Available demo credit: $${dollars(snapshot.availableMicrounits)}. This does not verify upstream invoices.`;
  }

  reconciliationFailure(error: unknown): string { return `${safeFailure(error, false, 'balance')} This command sent no inference; it does not establish prior accounting outcomes.`; }

  private observeReconciliation(output: AssistantMessage): void {
    const window = this.reconciliation;
    if (!window) return;
    const receipts = output.diagnostics?.filter(diagnostic => diagnostic.type === 'possums_settled_receipt') ?? [];
    const refunds = output.diagnostics?.filter(diagnostic => diagnostic.type === 'possums_reservation_refunded') ?? [];
    if (receipts.length === 1 && refunds.length === 0) {
      const charged = receipts[0].details?.chargedMicrounits;
      if (typeof charged !== 'string' || !/^(0|[1-9][0-9]{0,19})$/.test(charged)) { window.unknown = true; return; }
      window.charged += BigInt(charged);
      window.completed++;
    } else if (refunds.length === 1 && receipts.length === 0) {
      window.completed++;
    } else if (receipts.length || refunds.length || output.stopReason === 'error' || output.stopReason === 'aborted') {
      window.unknown = true;
    }
    if (output.diagnostics?.some(diagnostic => diagnostic.type === 'possums_billing_unknown')) window.unknown = true;
  }

  async refreshModels(context: Parameters<NonNullable<Provider['refreshModels']>>[0]): Promise<void> {
    if (!context.allowNetwork) {
      // Pi supplies the stored credential in the offline phase, including after
      // native /logout. No ambient lookup or network is needed to revoke state.
      const key = context.credential?.type === 'api_key' ? context.credential.key : undefined;
      const epoch = this.authEpoch;
      const stored = discoveryModels(context.stored?.models ?? []);
      const persist = this.client && this.listed.length && key === this.recoveryKey ? { models: this.discovery } : undefined;
      await context.publish({ ...(persist ? { persist } : {}), update: () => {
        if (epoch !== this.authEpoch || context.signal.aborted) return;
        if (!isRecoveryKey(key) || key !== this.recoveryKey) this.logout();
        if (!this.discovery.length) this.discovery = stored;
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
      if (!current()) return;
      const discovery = discoveryModels(listed);
      await context.publish({ persist: { models: discovery }, update: () => {
        if (current()) {
          this.catalog = entries; this.listed = listed; this.discovery = discovery;
          try { this.observeFailure(undefined); } catch { /* UI reporting cannot change catalog state. */ }
        }
      } });
    } catch (error) {
      const failure = catalogConnectionFailure(error);
      // Neither a stale success nor a stale failure may mutate a newer account.
      await context.publish({ update: () => {
        if (current()) {
          this.catalog = []; this.listed = [];
          try { this.observeFailure(failure); } catch { /* UI reporting cannot change catalog state. */ }
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

  // Native compaction has a separate scope and no retry policy. It must not
  // alter ordinary conversation state, even when its second call fails.
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
      try { ctx.ui.notify(`Possums compaction (${stage}): ${failure ?? safeFailure(error, false, 'compaction')}${billing} No checkpoint saved. Not replayed. Use /compact deliberately or /new.`, 'warning'); }
      catch { /* Transient UI only; never log or persist failed summaries. */ }
      return { cancel: true };
    } finally { closed = true; }
  }

  private perform(selected: Model<typeof API>, context: TranscriptContext, options: RequestOptions = {}, summary?: () => void): AssistantMessageEventStream {
    // A direct/nested stream that bypasses native auth must not leave renewal
    // permission available after generation has already been attempted.
    if (options.signal === this.renewalSignal) this.renewalSignal = undefined;
    if (options.signal === this.renewalCandidateSignal) this.renewalCandidateSignal = undefined;
    const events = createAssistantMessageEventStream();
    const output = blank(selected);
    const epoch = this.epoch;
    const reconciliation = this.reconciliation;
    const args = new Map<number, { block: ToolCall; contentIndex: number; json: string }>();
    let started = false;
    let failureStage: FailureStage = 'request';
    let submitted = false;
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
        if (this.failure && (!this.client || this.catalog.length === 0)) throw this.failure;
        if (!this.client || options.signal?.aborted) throw new Error('possums_session_unavailable');
        if (selected.provider !== PROVIDER_ID || selected.api !== API || selected.baseUrl !== ORIGIN || options.fetch || options.maxTokens !== undefined || options.samplingParams || options.temperature !== undefined || options.reasoning !== undefined || options.reasoningEffort !== undefined) {
          throw new Error('possums_request_options_unsupported');
        }
        const payload = invocation(selected, context);
        if (summary && payload.tools?.length) throw new Error('possums_summary_tools');
        if (options.toolChoice !== undefined) payload.tool_choice = options.toolChoice as NonNullable<typeof payload.tool_choice>;
        const entry = this.catalog.find(entry => entry.id === selected.id);
        if (!entry) throw new Error('possums_model_unavailable');
        if (payload.tools?.length && entry.tool_protocol !== 'openai-functions-v1') throw new Error('possums_tools_unsupported');
        failureStage = 'provider';
        submitted = true; // The reference client carries more precise pre-send failures.
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
        failureStage = 'settlement';
        const quoted = receipt.quotedInputMicrounitsPerMillion !== undefined && receipt.quotedOutputMicrounitsPerMillion !== undefined;
        // Record authenticated accounting before local display/precision conversion.
        output.diagnostics = [{ type: 'possums_settled_receipt', timestamp: Date.now(), details: {
          chargedMicrounits: receipt.chargedMicrounits, refundedMicrounits: receipt.refundedMicrounits,
          quotedCostComponents: quoted, finish: receipt.finish,
        } }];
        if (!summary && epoch === this.epoch) this.newConversation = false;
        if (!started) throw new Error('possums_invalid_stream');
        endText();
        const charged = safeNumber(receipt.chargedMicrounits, 'settlement') / 1_000_000;
        // The submitted rate is authoritative. Assign the sub-microunit rounding
        // remainder to output so the displayed components equal the settled total.
        const inputMicrounits = quoted ? BigInt(receipt.inputTokens) * BigInt(receipt.quotedInputMicrounitsPerMillion!) * 130n / 100_000_000n : 0n;
        const inputCost = safeNumber(inputMicrounits.toString(), 'settlement') / 1_000_000;
        output.usage = { input: receipt.inputTokens, output: receipt.outputTokens,
          cacheRead: 0, cacheWrite: 0, totalTokens: receipt.totalTokens,
          cost: { input: inputCost, output: quoted ? charged - inputCost : 0, cacheRead: 0, cacheWrite: 0, total: charged } };
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
          output.errorMessage = 'Possums: [possums_output_limit] Stage: completion; constraint: output_limit. Partial answer reached the model output limit; charge settled. Not automatically continued. Submit a deliberate follow-up if needed; a new request may incur another charge.';
        }
        if (output.stopReason === 'error') events.push({ type: 'error', reason: 'error', error: output });
        else events.push({ type: 'done', reason: output.stopReason, message: output });
      } catch (error) {
        output.stopReason = options.signal?.aborted ? 'aborted' : 'error';
        if (!output.diagnostics && error instanceof GatewayError && error.billing === 'refunded') {
          output.diagnostics = [{ type: 'possums_reservation_refunded', timestamp: Date.now(), details: { outcome: 'refunded' } }];
        }
        if (!output.diagnostics?.some(diagnostic => diagnostic.type === 'possums_settled_receipt') &&
          ((error instanceof GatewayError && error.billing === 'unknown') ||
            (!(error instanceof GatewayError) && !(error instanceof ConnectionFailure) && ((submitted && !(error instanceof ChannelError && error.code === 'rejected')) || options.signal?.aborted || (error instanceof ChannelError && error.code === 'uncertain'))))) {
          output.diagnostics = [{ type: 'possums_billing_unknown', timestamp: Date.now(), details: { receipt: false } }];
        }
        output.errorMessage = output.diagnostics?.some(diagnostic => diagnostic.type === 'possums_settled_receipt') ?
          `${safeFailure(error, false, failureStage).replace(/ Billing status: Charge unknown\..*$/, '')} Authenticated receipt: charge settled; not undone. Not replayed.` : options.signal?.aborted && !(error instanceof GatewayError) && !(error instanceof ConnectionFailure) ?
          'Possums: [possums_stream_interrupted] Stage: stream; constraint: interrupted. Delivery interrupted; charge unknown. Check connectivity and share only this code for support. Not replayed. A new request may incur another charge.' :
          safeFailure(error, !summary && epoch === this.epoch && !options.signal?.aborted, failureStage);
        events.push({ type: 'error', reason: output.stopReason, error: output });
      } finally {
        if (reconciliation && reconciliation === this.reconciliation) this.observeReconciliation(output);
        events.end();
      }
    })();
    return events;
  }
}
