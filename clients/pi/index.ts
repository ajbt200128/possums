import { isAbsolute } from 'node:path';
import { open, type FileHandle } from 'node:fs/promises';
import type { ExtensionAPI, ExtensionContext } from '@earendil-works/pi-coding-agent';
import { LIMITS } from '../../examples/phase01/limits.js';
import { connect, connectPublished, recoverCompiled, recoverPublished } from './bootstrap.js';
import { ConnectionFailure, catalogConnectionFailure, approvalSummary } from './diagnostics.js';
import { PossumsProvider, PROVIDER_ID } from './provider.js';

async function manifest(file: unknown): Promise<Uint8Array<ArrayBuffer>> {
  try {
    if (typeof file !== 'string' || !isAbsolute(file)) throw new Error();
    let handle: FileHandle | undefined;
    try {
      handle = await open(file, 'r');
      const info = await handle.stat();
      if (!info.isFile() || info.size > LIMITS.provenance) throw new Error();
      const buffer = new Uint8Array(LIMITS.provenance + 1);
      const { bytesRead } = await handle.read(buffer, 0, buffer.length, 0);
      if (bytesRead !== info.size || bytesRead > LIMITS.provenance) throw new Error();
      return buffer.slice(0, bytesRead);
    } finally { await handle?.close(); }
  } catch { throw new ConnectionFailure('manifest_unavailable'); }
}

export default function possums(pi: ExtensionAPI): void {
  pi.registerFlag('possums-manifest', { type: 'string', description: 'Independently hash-approved Possums release manifest (public file, never a credential)' });
  let ui: ExtensionContext['ui'] | undefined;
  let sessionEpoch = 0;
  const notifiedCodes = new Set<ConnectionFailure['code']>();
  let selected = false, selectedManifest: unknown;
  const authority = (): unknown => {
    const current = pi.getFlag('possums-manifest');
    if (selected && current !== selectedManifest) throw new ConnectionFailure('manifest_mismatch');
    selected = true;
    return selectedManifest = current;
  };
  const provider = new PossumsProvider(async signal => {
    const selection = authority();
    return selection === undefined ? connectPublished(signal) : connect(await manifest(selection), signal);
  }, failure => {
    if (!failure) { notifiedCodes.clear(); return; }
    if (!ui || notifiedCodes.has(failure.code)) return;
    notifiedCodes.add(failure.code);
    try { ui.notify(failure.message, 'warning'); } catch { /* Transient UI only. */ }
  }, (previous, signal) => {
    const selection = authority();
    return selection === undefined ? recoverPublished(previous, signal)
      : manifest(selection).then(bytes => recoverCompiled(previous, bytes, signal));
  });
  pi.registerProvider(provider);
  pi.on('session_start', async (_event, ctx) => {
    const epoch = ++sessionEpoch;
    notifiedCodes.clear();
    ui = ctx.hasUI ? ctx.ui : undefined;
    provider.shutdown();
    provider.newSession();
    try { await provider.verifySession(); }
    catch { return; } // Already reported as a closed, transient connection diagnostic.
    if (epoch !== sessionEpoch) return;
    try { await ctx.modelRegistry.refresh({ providers: [PROVIDER_ID], allowNetwork: true }); }
    catch (error) {
      if (epoch !== sessionEpoch) return;
      const failure = provider.currentFailure ?? catalogConnectionFailure(error);
      try { ui?.notify(failure.message, 'warning'); } catch { /* Transient UI only. */ }
    }
  });
  pi.on('session_shutdown', async () => {
    ++sessionEpoch;
    provider.shutdown();
    ui = undefined;
  });
  pi.registerCommand('possums-status', {
    description: 'Show the last safe connection failure or session-pinned release (offline)',
    handler: async (_args, ctx) => {
      if (!ctx.hasUI) return;
      const failure = provider.currentFailure;
      try { ctx.ui.notify(failure?.message ?? approvalSummary(provider.release), failure ? 'warning' : 'info'); }
      catch { /* Transient UI only. */ }
    },
  });
  pi.registerCommand('possums-reconcile', {
    description: 'Start, finish or cancel a private receipt/balance check (no inference)',
    handler: async (args, ctx) => {
      if (!ctx.hasUI) return;
      try {
        const result = await provider.reconcile(args.trim(), ctx);
        ctx.ui.notify(result, result.startsWith('Reconciliation unavailable:') || result.startsWith('Reconciliation mismatch:') ? 'warning' : 'info');
      } catch (error) {
        try { ctx.ui.notify(provider.reconciliationFailure(error), 'warning'); }
        catch { /* Transient diagnostics never fall back to logging or inference. */ }
      }
    },
  });
  pi.on('before_agent_start', (_event, ctx) => {
    if (ctx.model?.provider === PROVIDER_ID) provider.beginRun();
    else provider.settleRun();
  });
  pi.on('agent_start', (_event, ctx) => { provider.bindRun(ctx.model?.provider === PROVIDER_ID ? ctx.signal : undefined); });
  pi.on('message_end', (event, ctx) => {
    const message = event.message;
    if (!ctx.hasUI || message.role !== 'assistant' || message.provider !== PROVIDER_ID) return;
    const warning = message.diagnostics?.find(item => item.type === 'possums_turn_warning')?.details?.message;
    const charged = message.diagnostics?.find(item => item.type === 'possums_settled_receipt' || item.type === 'possums_gateway_charge')?.details?.chargedMicrounits;
    if (!warning && !message.diagnostics?.some(item => item.type === 'possums_billing_unknown' || item.type === 'possums_settled_receipt' || item.type === 'possums_gateway_charge')) return;
    try {
      if (typeof warning === 'string') ctx.ui.notify(warning, 'warning');
      const amount = typeof charged === 'string' && /^(0|[1-9][0-9]{0,19})$/.test(charged) ? BigInt(charged) : undefined;
      const tokensUnavailable = message.diagnostics?.find(item => item.type === 'possums_accounting_metadata')?.details?.usageAvailable === false;
      ctx.ui.notify(amount === undefined
        ? 'Possums: [possums_billing_unknown] Response accounting unavailable; charge unknown. Pi cost zeros are unavailable placeholders, not free. Not replayed; a new request may incur another charge.'
        : `Possums: gateway-reported charge $${amount / 1_000_000n}.${(amount % 1_000_000n).toString().padStart(6, '0')}. ${tokensUnavailable ? 'Token counts unavailable; native token zeros are placeholders. ' : ''}Input/cache/output charges unavailable; Pi component zeros are placeholders, not free. Receipt alone does not establish response completion or authorize tools.`, 'warning');
    } catch { /* Transient UI only; no logging, reset, renewal or inference. */ }
  });
  pi.on('agent_end', () => { provider.endRun(); });
  pi.on('agent_settled', () => { provider.settleRun(); });
  pi.on('cache_warming_decision', (_event, ctx) => {
    if (ctx.model?.provider === PROVIDER_ID) return { action: 'stop' as const };
  });
  pi.on('session_before_compact', (event, ctx) => {
    if (ctx.model?.provider === PROVIDER_ID) return provider.compact(event, ctx);
  });
  pi.registerCommand('possums-text-only', {
    description: 'Explicitly disable active Pi tools for a text-only Possums model',
    handler: async (_args, ctx) => {
      pi.setActiveTools([]);
      ctx.ui.notify('Pi tools disabled for this session. No model was substituted.', 'info');
    },
  });
}
