import { API_APPROVALS, PUBLISHER, requireApiApproval, requireReleaseTag } from '../../examples/phase01/approval.js';
import { ConnectionFailure, connectionFailure } from './diagnostics.js';
import { ReferenceClient } from '../../examples/phase01/client.js';
import { LIMITS, Operation, base64, boundedReport, cleanup, collect, digest, hex, parseJSON, requireThat } from '../../examples/phase01/limits.js';

// Public acquisition only: callers below construct every destination locally.
// No SDK acquisition retries, credentials, cache or metadata URLs. Only the
// public manifest download permits one explicit GitHub-to-asset-CDN redirect.
async function acquire(url: string, cap: number, op: Operation): Promise<Uint8Array<ArrayBuffer>> {
  let response: Response | undefined;
  try {
    op.check();
    response = await op.wait(fetch(new Request(url, {
      signal: op.controller.signal, credentials: 'omit', redirect: 'error',
      cache: 'no-store', referrerPolicy: 'no-referrer',
    })), LIMITS.operationMs);
    requireThat(response.ok && !response.redirected && response.url === url);
    return await collect(response.body, cap, op);
  } catch { throw new ConnectionFailure('evidence_unavailable'); }
  finally {
    if (response?.body && !response.body.locked) await cleanup(response.body.cancel());
  }
}

async function downloadManifest(tag: string, op: Operation): Promise<Uint8Array<ArrayBuffer>> {
  const url = `https://github.com/${PUBLISHER.repository}/releases/download/${tag}/tinfoil-deployment.json`;
  let response: Response | undefined;
  try {
    op.check();
    response = await op.wait(fetch(new Request(url, {
      signal: op.controller.signal, credentials: 'omit', redirect: 'manual',
      cache: 'no-store', referrerPolicy: 'no-referrer',
    })), LIMITS.operationMs);
    requireThat(response.status === 302 && !response.redirected && response.url === url);
    const target = new URL(response.headers.get('location') ?? '');
    requireThat(target.protocol === 'https:' && target.hostname === 'release-assets.githubusercontent.com'
      && !target.port && !target.username && !target.password && !target.hash);
    if (response.body) await cleanup(response.body.cancel());
    return await acquire(target.href, LIMITS.provenance, op);
  } catch { throw new ConnectionFailure('evidence_unavailable'); }
  finally { if (response?.body && !response.body.locked) await cleanup(response.body.cancel()); }
}

// Preserve the independent compiled/reference entry point and its admin expiry.
export async function connect(manifest: Uint8Array<ArrayBuffer>, signal?: AbortSignal): Promise<ReferenceClient> {
  if (API_APPROVALS.length === 1 && Date.now() >= API_APPROVALS[0].expires) throw new ConnectionFailure('approval_expired');
  let approval;
  try { approval = requireApiApproval(); }
  catch { throw new ConnectionFailure('approval_unavailable'); }
  if (manifest.length > LIMITS.provenance || await digest(manifest) !== approval.manifest) throw new ConnectionFailure('manifest_mismatch');
  const op = new Operation(LIMITS.bootstrapMs, signal);
  try { return await serving(approval, new Uint8Array(manifest), op, false); }
  catch (error) { throw connectionFailure(error); }
  finally { op.close(); }
}

// Once per new Pi trust session. Latest is discovery, never approval; neither
// target_commitish nor asset URLs nor any caller-supplied policy are consulted.
export async function connectPublished(signal?: AbortSignal): Promise<ReferenceClient> {
  const op = new Operation(LIMITS.bootstrapMs, signal);
  try {
    const latest = parseJSON(await acquire(`https://api.github.com/repos/${PUBLISHER.repository}/releases/latest`, LIMITS.provenance, op), LIMITS.provenance);
    const tag = latest.tag_name;
    requireReleaseTag(tag);
    const manifest = await downloadManifest(tag, op);
    // Exact downloaded bytes must be the signed subject, not reserialized JSON.
    const hash = await digest(manifest);
    return await serving({ ...PUBLISHER, tag, manifest: hash }, manifest, op, true);
  } catch (error) { throw connectionFailure(error); }
  finally { op.close(); }
}

type Candidate = { origin: string; repository: string; tag: string; manifest: string };
async function serving(candidate: Candidate, manifest: Uint8Array<ArrayBuffer>, op: Operation, published: boolean): Promise<ReferenceClient> {
  try {
    const doc = parseJSON(await acquire(candidate.origin + '/.well-known/tinfoil-attestation', LIMITS.evidence, op), LIMITS.evidence);
    requireThat(doc.format === 'https://tinfoil.sh/predicate/sev-snp-guest/v2');
    const raw = await boundedReport(base64(doc.body, LIMITS.evidence), op);
    // Untrusted routing fields; hardware verification authenticates Genoa/TCB/chip.
    const tcb = raw.slice(0x180, 0x188);
    const kds = `https://kdsintf.amd.com/vcek/v1/Genoa/${hex(raw.slice(0x1a0, 0x1e0))}?blSPL=${tcb[0]}&teeSPL=${tcb[1]}&snpSPL=${tcb[6]}&ucodeSPL=${tcb[7]}`;
    const vcek = await acquire(kds, LIMITS.certificate, op);
    const cert = parseJSON(await acquire(candidate.origin + '/.well-known/tinfoil-certificate', LIMITS.certificate, op), LIMITS.certificate);
    const provenance = parseJSON(await acquire(`https://api.github.com/repos/${candidate.repository}/attestations/sha256:${candidate.manifest}`, LIMITS.provenance, op), LIMITS.provenance);
    requireThat(provenance.attestations?.length === 1);
    const config = await acquire(candidate.origin + '/.well-known/hpke-keys', LIMITS.key, op);
    const bundle = new TextEncoder().encode(JSON.stringify({
      domain: new URL(candidate.origin).hostname, enclaveAttestationReport: doc,
      enclaveCert: cert.certificate, vcek: btoa(String.fromCharCode(...vcek)),
      digest: candidate.manifest, releaseTag: candidate.tag,
      sigstoreBundle: provenance.attestations[0].bundle,
    }));
    requireThat(bundle.length <= LIMITS.bundle);
    op.check();
    const client = published
      ? await ReferenceClient.published(bundle, manifest, config, op.controller.signal)
      : await ReferenceClient.verified(bundle, manifest, config);
    op.check();
    return client;
  } catch (error) { throw connectionFailure(error); }
}
