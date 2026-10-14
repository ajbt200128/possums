// Actual emitted package + Pi 1.0.4 SDK + real packaged Worker/Go WASM.
// Public acquisition is synthetic, with deliberately invalid evidence; no
// credentials, application sockets or inference. Positive crypto remains open.
import assert from 'node:assert/strict';
import { mkdir, readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
const root = process.env.POSSUMS_PI_ROOT, pkg = process.env.PHASE02_PACKAGE;
assert(root && pkg && path.isAbsolute(root) && path.isAbsolute(pkg));
const coding = await import(pathToFileURL(path.join(root, 'node_modules/@earendil-works/pi-coding-agent/dist/index.js')));
const ai = await import(pathToFileURL(path.join(root, 'node_modules/@earendil-works/pi-ai/dist/index.js')));
const extension = (await import(pathToFileURL(path.join(pkg, 'extension.mjs')))).default;
const config = Buffer.from('cvm-version: 0.14.12\ncontainers: []\n');
const manifest = { config: config.toString('base64'), hashes: { version: 'v0.14.12' },
  cmdline: `tinfoil-config-hash=${createHash('sha256').update(config).digest('hex')}` };
const urls = [], fetch = globalThis.fetch;
globalThis.fetch = async req => {
  urls.push(req.url);
  assert.equal(req.method, 'GET'); assert.equal(req.body, null); assert.equal(req.headers.has('authorization'), false);
  let body, status = 200, headers = {};
  if (req.url.endsWith('/releases/latest')) body = { tag_name: 'v0.0.99' };
  else if (req.url.includes('/releases/download/')) { status = 302; headers.location = 'https://release-assets.githubusercontent.com/synthetic'; }
  else if (req.url === 'https://release-assets.githubusercontent.com/synthetic') body = manifest;
  else if (/\/tinfoil-attestation\?nonce=[0-9a-f]{64}$/.test(req.url)) body = {};
  else if (req.url.endsWith('/tinfoil-certificate')) body = { certificate: 'synthetic; never reaches certificate appraisal' };
  else if (req.url.endsWith('/hpke-keys')) body = new Uint8Array([0, 0, 32, ...Array(32).fill(7), 0, 4, 0, 1, 0, 2]);
  else throw new Error('HOSTILE unexpected application request');
  const res = new Response(body instanceof Uint8Array ? body : body === undefined ? null : JSON.stringify(body), { status, headers });
  Object.defineProperty(res, 'url', { value: req.url }); return res;
};
const cwd = path.resolve(pkg, '../checks/packaged-host'); await mkdir(cwd, { recursive: true });
const settings = coding.SettingsManager.inMemory({ defaultTools: [], cacheWarming: 'off', enableInstallTelemetry: false, compaction: { enabled: false } });
const runtime = await coding.ModelRuntime.create({ credentials: new ai.InMemoryCredentialStore(), modelsStore: new ai.InMemoryModelsStore(),
  modelsPath: null, allowModelNetwork: false, refreshOnCreate: false });
let provider, status;
const loader = new coding.DefaultResourceLoader({ cwd, agentDir: cwd, settingsManager: settings, noExtensions: true,
  noSkills: true, noPromptTemplates: true, noThemes: true, noContextFiles: true, systemPrompt: 'Synthetic offline qualification',
  extensionFactories: [pi => extension({ ...pi,
    registerProvider: value => { provider = value; pi.registerProvider(value); },
    registerCommand: (name, command) => { if (name === 'possums-status') status = command; pi.registerCommand(name, command); },
  })] });
let session;
try {
  await loader.reload(); assert.deepEqual(loader.getExtensions().errors, []); assert.equal(urls.length, 0);
  runtime.registerNativeProvider(provider);
  ({ session } = await coding.createAgentSession({ cwd, agentDir: cwd, modelRuntime: runtime, resourceLoader: loader,
    settingsManager: settings, sessionManager: coding.SessionManager.inMemory(cwd), tools: [],
    model: { provider: 'possums', id: 'synthetic', api: 'openai-completions', baseUrl: 'https://gateway.possums.dev',
      name: 'Synthetic', input: ['text'], reasoning: false, contextWindow: 64000, maxTokens: 20,
      cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 } } }));
  await session.bindExtensions({ mode: 'print' });
  assert.equal(provider.currentFailure.code, 'verification_failed');
  assert.match(provider.currentFailure.message, /possums_v3_envelope_rejected/);
  assert(!provider.currentFailure.message.includes('artifact_unavailable')); assert.deepEqual(provider.getModels(), []);
  const before = urls.length; let notice;
  await status.handler('', { hasUI: true, ui: { notify: text => { notice = text; } } });
  assert.match(notice, /possums_v3_envelope_rejected/); assert.equal(urls.length, before);
  assert.equal(urls.length, 6); assert.equal(session.messages.length, 0);
  const report = JSON.parse(await readFile(path.join(pkg, 'build-report.json'), 'utf8'));
  for (const name of ['verifier.wasm', 'wasm_exec.cjs', 'verifier-worker.mjs', 'verifier.mjs', 'verifier.d.mts', 'GO-LICENSE'])
    assert.equal(createHash('sha256').update(await readFile(path.join(pkg, name))).digest('hex'), report.artifacts[name].sha256);
  console.log(JSON.stringify({ suite: 'pi-v3-packaged-host', pi: '1.0.4', checks: 18,
    scope: 'actual emitted extension registration/session_start/status and packaged real WASM rejection; synthetic public acquisition only', inference: 0 }));
} finally { session?.dispose(); globalThis.fetch = fetch; }
