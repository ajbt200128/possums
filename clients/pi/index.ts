import { fileURLToPath } from 'node:url';
import { dirname, isAbsolute, join } from 'node:path';
import { open, type FileHandle } from 'node:fs/promises';
import { readFileSync } from 'node:fs';
import type { ExtensionAPI } from '@earendil-works/pi-coding-agent';
import { LIMITS } from '../../examples/phase01/limits.js';
import { connect } from './bootstrap.js';
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
  if (typeof file !== 'string' || !isAbsolute(file)) throw new Error('possums_manifest_required');
  let handle: FileHandle | undefined;
  try {
    handle = await open(file, 'r');
    const info = await handle.stat();
    if (!info.isFile() || info.size > LIMITS.provenance) throw new Error('possums_invalid_manifest');
    const buffer = new Uint8Array(LIMITS.provenance + 1);
    const { bytesRead } = await handle.read(buffer, 0, buffer.length, 0);
    if (bytesRead !== info.size || bytesRead > LIMITS.provenance) throw new Error('possums_invalid_manifest');
    return buffer.slice(0, bytesRead);
  } catch { throw new Error('possums_invalid_manifest'); }
  finally { await handle?.close(); }
}

export default function possums(pi: ExtensionAPI): void {
  requirePinnedRuntime();
  pi.registerFlag('possums-manifest', { type: 'string', description: 'Independently hash-approved Possums release manifest (public file, never a credential)' });
  const provider = new PossumsProvider(async signal => connect(await manifest(
    pi.getFlag('possums-manifest') ?? fileURLToPath(new URL('./tinfoil-deployment.json', import.meta.url)),
  ), signal));
  pi.registerProvider(provider);
  pi.on('session_start', async (_event, ctx) => {
    provider.newSession();
    await ctx.modelRegistry.refresh({ providers: [PROVIDER_ID], allowNetwork: true });
  });
  pi.on('session_shutdown', () => provider.logout());
  pi.on('before_agent_start', (_event, ctx) => {
    if (ctx.model?.provider === PROVIDER_ID) provider.beginRun();
  });
  pi.on('agent_settled', () => provider.endRun());
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
