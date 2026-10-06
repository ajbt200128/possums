import { requireApiApproval } from '../../examples/phase01/approval.js';
import { ReferenceClient } from '../../examples/phase01/client.js';
import { LIMITS, Operation, base64, boundedReport, cleanup, collect, digest, hex, parseJSON, requireThat, ChannelError } from '../../examples/phase01/limits.js';

// Acquire only public bytes at the installed approval's exact destinations.
// All these bytes remain untrusted until ReferenceClient.verified authenticates
// provenance, hardware endorsements and the key used by the actual EHBP channel.
export async function connect(manifest: Uint8Array<ArrayBuffer>, signal?: AbortSignal): Promise<ReferenceClient> {
  const approval = requireApiApproval();
  requireThat(manifest.length <= LIMITS.provenance && await digest(manifest) === approval.manifest);
  const op = new Operation();
  const abort = () => op.close();
  signal?.addEventListener('abort', abort, { once: true });
  if (signal?.aborted) op.close();
  async function acquire(url: string, cap: number): Promise<Uint8Array<ArrayBuffer>> {
    let response: Response | undefined;
    try {
      response = await op.wait(fetch(new Request(url, {
        signal: op.controller.signal, credentials: 'omit', redirect: 'error',
        cache: 'no-store', referrerPolicy: 'no-referrer',
      })), LIMITS.operationMs);
      requireThat(response.ok && !response.redirected && response.url === url);
      return await collect(response.body, cap, op);
    } finally {
      if (response?.body && !response.body.locked) await cleanup(response.body.cancel());
    }
  }
  try {
    const doc = parseJSON(await acquire(approval.origin + '/.well-known/tinfoil-attestation', LIMITS.evidence), LIMITS.evidence);
    requireThat(doc.format === 'https://tinfoil.sh/predicate/sev-snp-guest/v2');
    const raw = await boundedReport(base64(doc.body, LIMITS.evidence), op);
    const tcb = raw.slice(0x180, 0x188);
    const kds = `https://kdsintf.amd.com/vcek/v1/Genoa/${hex(raw.slice(0x1a0, 0x1e0))}?blSPL=${tcb[0]}&teeSPL=${tcb[1]}&snpSPL=${tcb[6]}&ucodeSPL=${tcb[7]}`;
    const vcek = await acquire(kds, LIMITS.certificate);
    const cert = parseJSON(await acquire(approval.origin + '/.well-known/tinfoil-certificate', LIMITS.certificate), LIMITS.certificate);
    const provenance = parseJSON(await acquire(`https://api.github.com/repos/${approval.repository}/attestations/sha256:${approval.manifest}`, LIMITS.provenance), LIMITS.provenance);
    requireThat(provenance.attestations?.length === 1);
    const config = await acquire(approval.origin + '/.well-known/hpke-keys', LIMITS.key);
    const bundle = new TextEncoder().encode(JSON.stringify({
      domain: new URL(approval.origin).hostname, enclaveAttestationReport: doc,
      enclaveCert: cert.certificate, vcek: btoa(String.fromCharCode(...vcek)),
      digest: approval.manifest, releaseTag: approval.tag,
      sigstoreBundle: provenance.attestations[0].bundle,
    }));
    requireThat(bundle.length <= LIMITS.bundle && !op.controller.signal.aborted);
    const client = await ReferenceClient.verified(bundle, new Uint8Array(manifest), config);
    requireThat(!signal?.aborted);
    return client;
  } catch { throw new ChannelError(); }
  finally { signal?.removeEventListener('abort', abort); op.close(); }
}
