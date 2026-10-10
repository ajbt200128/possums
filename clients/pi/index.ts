import { fileURLToPath } from 'node:url';
import { dirname, isAbsolute, join } from 'node:path';
import { open, type FileHandle } from 'node:fs/promises';
import { readFileSync } from 'node:fs';
import type { ExtensionAPI, ExtensionContext } from '@earendil-works/pi-coding-agent';
import { LIMITS } from '../../examples/phase01/limits.js';
import { connect, connectPublished } from './bootstrap.js';
import { ConnectionFailure, approvalSummary } from './diagnostics.js';
import { PossumsProvider, PROVIDER_ID } from './provider.js';

export const PI_PIN = '1.0.4';
function requirePinnedRuntime(): void {
  try {
    for (const name of ['pi-ai', 'pi-coding-agent', 'pi-agent-core']) {
      const entry = fileURLToPath(import.meta.resolve(`@earendil-works/${name}`));
      const metadata = JSON.parse(readFileSync(join(dirname(entry), '..', 'package.json'), 'utf8'));
      if (metadata.version !== PI_PIN) throw new Error();
    }
  } catch { throw new Error('possums_requires_pi_1_0_4'); }
}
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
  requirePinnedRuntime();
  pi.registerFlag('possums-manifest', { type: 'string', description: 'Independently hash-approved Possums release manifest (public file, never a credential)' });
  let ui: ExtensionContext['ui'] | undefined;
  let lastFailure: ConnectionFailure | undefined;
  const notifiedCodes = new Set<ConnectionFailure['code']>();
  const provider = new PossumsProvider(async signal => {
    const pinnedManifest = pi.getFlag('possums-manifest');
    return pinnedManifest === undefined
      ? connectPublished(signal)
      : connect(await manifest(pinnedManifest), signal);
  }, failure => {
    lastFailure = failure;
    if (!failure) { notifiedCodes.clear(); return; }
    if (!ui || notifiedCodes.has(failure.code)) return;
    notifiedCodes.add(failure.code);
    try { ui.notify(failure.message, 'warning'); } catch { /* Transient UI only. */ }
  });
  pi.registerProvider(provider);
  pi.on('session_start', async (_event, ctx) => {
    ui = ctx.hasUI ? ctx.ui : undefined;
    provider.newSession();
    try { await provider.verifySession(); }
    catch { return; } // Already reported as a closed, transient connection diagnostic.
    await ctx.modelRegistry.refresh({ providers: [PROVIDER_ID], allowNetwork: true });
  });
  pi.on('session_shutdown', () => { ui = undefined; provider.shutdown(); });
  pi.registerCommand('possums-status', {
    description: 'Show the last safe connection failure or session-pinned release (offline)',
    handler: async (_args, ctx) => {
      if (!ctx.hasUI) return;
      try { ctx.ui.notify(lastFailure?.message ?? approvalSummary(provider.release), lastFailure ? 'warning' : 'info'); }
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
  // Source belongs to input, not before_agent_start (extensions reach that too).
  // These hooks only mark; all renewal I/O waits for native active-run auth.
  pi.on('input', (event, ctx) => {
    provider.markInput(ctx.model?.provider === PROVIDER_ID && ctx.isIdle() &&
      event.streamingBehavior === undefined && (event.source === 'interactive' || event.source === 'rpc'), ctx.signal);
  });
  pi.on('before_agent_start', (_event, ctx) => {
    if (ctx.model?.provider === PROVIDER_ID) provider.beginRun();
    else provider.settleRun();
  });
  pi.on('agent_start', (_event, ctx) => { provider.bindRun(ctx.model?.provider === PROVIDER_ID ? ctx.signal : undefined); });
  pi.on('message_start', (event, ctx) => {
    if (event.message.role !== 'system') provider.confirmRunInput(event.message.role === 'user', ctx.signal);
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
