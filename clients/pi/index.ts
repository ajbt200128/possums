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
  // Pi 1.0.4 applies CLI flag values after loading extension factories. Select
  // at the first actual trust establishment, then lock the authority: a later
  // flag change cannot silently switch a pinned session to latest or vice versa.
  let pinnedManifest: unknown, selected = false;
  const authority = () => {
    const configured = pi.getFlag('possums-manifest');
    if (!selected) { pinnedManifest = configured; selected = true; }
    else if (configured !== pinnedManifest) throw new ConnectionFailure('manifest_mismatch');
    return pinnedManifest;
  };
  const provider = new PossumsProvider(async signal => {
    const pinned = authority();
    return pinned === undefined ? connectPublished(signal) : connect(await manifest(pinned), signal);
  }, failure => {
    if (!failure) { notifiedCodes.clear(); return; }
    if (!ui || notifiedCodes.has(failure.code)) return;
    notifiedCodes.add(failure.code);
    try { ui.notify(failure.message, 'warning'); } catch { /* Transient UI only. */ }
  }, (previous, signal) => {
    const pinned = authority();
    return pinned === undefined ? recoverPublished(previous, signal)
      : manifest(pinned).then(bytes => recoverCompiled(previous, bytes, signal));
  });
  pi.registerProvider(provider);
  pi.on('session_start', async (_event, ctx) => {
    const epoch = ++sessionEpoch;
    notifiedCodes.clear();
    ui = ctx.hasUI ? ctx.ui : undefined;
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
  pi.on('session_shutdown', () => { sessionEpoch++; ui = undefined; provider.shutdown(); });
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
