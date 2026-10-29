import { isAbsolute } from 'node:path';
import { fileURLToPath } from 'node:url';
import { open, type FileHandle } from 'node:fs/promises';
import type { ExtensionAPI, ExtensionContext } from '@earendil-works/pi-coding-agent';
import { LIMITS } from './core/limits.js';
import { connect, connectPublished, recoverCompiled, recoverPublished, connectTorPublished, recoverTorPublished } from './bootstrap.js';
import { ConnectionFailure, connectionFailure, catalogConnectionFailure, approvalSummary } from './diagnostics.js';
import { prepareTorConnection, type TorConnection } from './tor-connection.js';
import { createTorSelection, type RouteSelection } from './tor-selection.js';
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
  pi.registerFlag('possums-tor-onion', { type: 'string', description: 'Explicit canonical v3 onion host; latest published Tor verification only (no Tor launch)' });
  pi.registerFlag('possums-tor-proxy', { type: 'string', description: 'Existing numeric loopback SOCKS listener, host:port (default 127.0.0.1:19050; IPv6 brackets allowed)' });
  let ui: ExtensionContext['ui'] | undefined;
  let sessionEpoch = 0;
  const notifiedCodes = new Set<ConnectionFailure['code']>();
  const authority = createTorSelection(() => ({ manifest: pi.getFlag('possums-manifest'),
    onion: pi.getFlag('possums-tor-onion'), proxy: pi.getFlag('possums-tor-proxy') }));
  let locked: RouteSelection | undefined, scope: TorConnection | undefined;
  let closing: Promise<void> | undefined, lifecycleFailure: ConnectionFailure | undefined;
  const torScope = (selection: Extract<RouteSelection, { route: 'tor' }>): TorConnection => {
    if (!scope) {
      // Only a valid explicit local Tor selection may read proxy-only secrets.
      // Native configuration validates optional pairing/UTF-8 bounds; no secret
      // is placed in flags, argv, diagnostics or persisted extension state.
      try {
        scope = prepareTorConnection({ helperPath: fileURLToPath(new URL('./possums-tor-native', import.meta.url)),
          host: selection.host, port: selection.port,
          username: process.env.POSSUMS_TOR_PROXY_USERNAME, password: process.env.POSSUMS_TOR_PROXY_PASSWORD }, selection.onion);
      } catch (error) { throw connectionFailure(error); }
    }
    return scope;
  };
  const closeScope = (): Promise<void> => {
    if (scope) {
      const owned = scope;
      scope = undefined;
      closing = (async () => {
        try { await owned.close(); }
        catch { throw new ConnectionFailure('tor_cleanup_unknown'); }
      })();
    }
    return closing ?? Promise.resolve();
  };
  const provider = new PossumsProvider(async signal => {
    if (lifecycleFailure) throw lifecycleFailure;
    const selection = locked = authority();
    if (selection.route === 'tor') return connectTorPublished(torScope(selection), signal);
    return selection.manifest === undefined ? connectPublished(signal) : connect(await manifest(selection.manifest), signal);
  }, failure => {
    if (!failure) { notifiedCodes.clear(); return; }
    if (!ui || notifiedCodes.has(failure.code)) return;
    notifiedCodes.add(failure.code);
    try { ui.notify(failure.message, 'warning'); } catch { /* Transient UI only. */ }
  }, (previous, signal) => {
    if (lifecycleFailure) throw lifecycleFailure;
    const selection = authority();
    if (selection.route === 'tor') {
      // Recovery may use ONLY the serving scope; never create a replacement
      // transport here or replay an uncertain generation.
      if (!scope) throw new ConnectionFailure('tor_transport_diagnostic');
      return recoverTorPublished(previous, scope, signal);
    }
    return selection.manifest === undefined ? recoverPublished(previous, signal)
      : manifest(selection.manifest).then(bytes => recoverCompiled(previous, bytes, signal));
  });
  pi.registerProvider(provider);
  pi.on('session_start', async (_event, ctx) => {
    const epoch = ++sessionEpoch;
    notifiedCodes.clear();
    ui = ctx.hasUI ? ctx.ui : undefined;
    provider.shutdown();
    try {
      await closeScope();
      if (epoch !== sessionEpoch) return;
      lifecycleFailure = undefined;
      // newSession starts async trust synchronously. Recreate a closed Tor
      // scope from the locked selection BEFORE it can start that work.
      if (locked?.route === 'tor') {
        const selection = authority();
        if (selection.route === 'tor') torScope(selection);
      }
    } catch (error) {
      if (epoch !== sessionEpoch) return;
      lifecycleFailure = connectionFailure(error);
    }
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
    const epoch = ++sessionEpoch;
    provider.shutdown();
    try { await closeScope(); }
    catch (error) {
      if (epoch === sessionEpoch) {
        lifecycleFailure = connectionFailure(error);
        try { ui?.notify(lifecycleFailure.message, 'warning'); } catch { /* Transient UI only. */ }
      }
    } finally { if (epoch === sessionEpoch) ui = undefined; }
  });
  pi.registerCommand('possums-status', {
    description: 'Show the last safe connection failure or session-pinned release (offline)',
    handler: async (_args, ctx) => {
      if (!ctx.hasUI) return;
      const failure = lifecycleFailure ?? provider.currentFailure;
      try { ctx.ui.notify(failure?.message ?? approvalSummary(provider.release), failure ? 'warning' : 'info'); }
      catch { /* Transient UI only. */ }
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
    const billingUnknown = message.diagnostics?.some(item => item.type === 'possums_billing_unknown');
    if (typeof warning !== 'string' && !billingUnknown) return;
    try {
      if (typeof warning === 'string') ctx.ui.notify(warning, 'warning');
      if (billingUnknown) ctx.ui.notify('Possums: [possums_billing_unknown] Response accounting unavailable; charge unknown. Pi cost zeros are unavailable placeholders, not free. Not replayed; a new request may incur another charge.', 'warning');
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
