// Run with scratch build/install/evidence/fixture paths; never reads account secrets.
import assert from 'node:assert/strict';
import { readFile, writeFile, realpath } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';
import { createHash, X509Certificate } from 'node:crypto';
import { gzipSync, gunzipSync } from 'node:zlib';
import path from 'node:path';
const { PHASE01_BUILD: build, PHASE01_INSTALL: install, PHASE01_EVIDENCE: evidence, PHASE01_FIXTURE_DIR: fixtureDir, PHASE01_BROWSER: browserPath } = process.env;
assert.ok(build && install && evidence && fixtureDir && browserPath);
const load = async p => JSON.parse(await readFile(p, 'utf8'));
const prod = await import(pathToFileURL(path.join(build, 'channel-node.mjs')));
const fixture = await import(pathToFileURL(path.join(build, 'fixture-node.mjs')));
const bundle = await load(path.join(evidence, 'bundle.json'));
const manifest = new Uint8Array(await readFile(path.join(evidence, 'manifest.bin')));
const key = new Uint8Array(await readFile(path.join(evidence, 'key.bin')));
const bytes = obj => new TextEncoder().encode(JSON.stringify(obj));
const originalFetch = globalThis.fetch;
let credentials = 0, content = 0, acquisitions = 0;
globalThis.fetch = async req => {
  assert.ok(req instanceof Request);
  assert.equal(req.credentials, 'omit'); assert.equal(req.redirect, 'error'); assert.equal(req.cache, 'no-store');
  assert.ok(req.signal);
  if (req.headers.has('Authorization')) credentials++;
  if (req.body) content++; else acquisitions++;
  throw new Error('network forbidden during offline qualification');
};
let passed = 0;
async function rejects(name, fn, code) {
  await assert.rejects(fn, code ? e => e.code === code : undefined, name); passed++;
}
const verified = await prod.qualifyWeb(bytes(bundle), manifest, key);
assert.equal(verified.scope, 'web-observation-only'); passed++;
for (const [name, mutate] of [
  ['malformed', b => { b.enclaveAttestationReport.body = '!!!'; }],
  ['report-signature', b => { const raw = gunzipSync(Buffer.from(b.enclaveAttestationReport.body, 'base64')); raw[0x2a0] ^= 1; b.enclaveAttestationReport.body = gzipSync(raw).toString('base64'); }],
  ['provenance-signature', b => { const signature = Buffer.from(b.sigstoreBundle.dsseEnvelope.signatures[0].sig, 'base64'); signature[0] ^= 1; b.sigstoreBundle.dsseEnvelope.signatures[0].sig = signature.toString('base64'); }],
  ['unapproved-release', b => { b.releaseTag = 'v0.0.7'; }],
  ['wrong-manifest', b => { b.digest = '0'.repeat(64); }],
  ['wrong-domain', b => { b.domain = 'phase01.invalid'; }],
  ['decompression-bomb', b => { b.enclaveAttestationReport.body = gzipSync(Buffer.alloc(1024*1024)).toString('base64'); }],
]) {
  const bad = structuredClone(bundle); mutate(bad);
  await rejects(name, () => prod.qualifyWeb(bytes(bad), manifest, key));
}
const wrongKey = key.slice(); wrongKey[3] ^= 1;
await rejects('endorsed-key', () => prod.qualifyWeb(bytes(bundle), manifest, wrongKey));
for (const bad of [key.slice(0,40), new Uint8Array(42), Uint8Array.from(key, (v,i) => i===2 ? 0x21 : v), Uint8Array.from(key, (v,i) => i===40 ? 1 : v)]) {
  assert.throws(() => prod.validateKeyConfig(bad, verified.hpkeKey)); passed++;
}
assert.throws(() => prod.checkApproval(Date.now(), Date.now()-1, false)); passed++;
assert.throws(() => prod.checkApproval(Date.now(), Date.now()+1000, true)); passed++;
// Actual pinned AMD certificate-validity rejection, not a made-up v2 quote-age test.
const { verifyAttestation } = await import(pathToFileURL(path.join(install, 'node_modules/@tinfoilsh/verifier/dist/index.js')));
const NativeDate = Date;
globalThis.Date = class extends NativeDate { constructor(...args) { super(...(args.length ? args : ['2100-01-01T00:00:00Z'])); } };
try { await rejects('AMD collateral validity', () => verifyAttestation(bundle.enclaveAttestationReport, bundle.vcek)); }
finally { globalThis.Date = NativeDate; }
await rejects('production API approval empty', () => prod.Channel.api());
assert.throws(()=>new prod.Channel(Symbol(),prod.WEB_APPROVAL.origin,{})); passed++;
await rejects('fixture disabled in production build', () => prod.Channel.fixture('https://localhost:18443', key, verified.hpkeKey));
await rejects('fixture cannot select production', () => fixture.Channel.fixture(prod.WEB_APPROVAL.origin, key, verified.hpkeKey));
assert.deepEqual([credentials,content,acquisitions],[0,0,0]);
const chat = { model: 'fixture', stream: true, submission: 's'.repeat(43), messages:[{role:'user',content:'qualification-content\ud800😀\n"\\\u0000'}] };
assert.throws(() => prod.encodeChat({...chat, messages:[{role:'user',content:'x'.repeat(prod.LIMITS.chat)}]})); passed++;
assert.throws(() => prod.encodeChat({...chat, tools:[]})); passed++;
assert.throws(() => prod.serialize(new Array(2**30),prod.LIMITS.chat)); passed++;
assert.throws(() => prod.encodeChat({...chat,model:true})); passed++;
assert.throws(() => prod.parseJSON(bytes('x'.repeat(5000)),4096)); passed++;
assert.throws(() => prod.parseJSON(new TextEncoder().encode('['.repeat(17)+'0'+']'.repeat(17)),4096)); passed++;
assert.throws(() => prod.parseJSON(new Uint8Array([0xff]),4096)); passed++;
assert.equal(prod.serialize({x:'\ud800😀\n"'},4096).length, bytes({x:'\ud800😀\n"'}).length); passed++;

const meta = await load(path.join(fixtureDir, 'fixture.json'));
const config = new Uint8Array(Buffer.from(meta.config,'hex'));
const channel = await fixture.Channel.fixture(meta.origin, config, meta.key);
const bearer = 'a'.repeat(43);
// The identical admission assertions run inside Node AND Chromium, through public
// encodeChat/chat/control/serialize entries. No test-only transport admission flag.
async function admissionRegressions({productionURL, fixtureURL, meta}) {
  const m = await import(productionURL), f = await import(fixtureURL);
  const config = Uint8Array.from(meta.config.match(/../g), v => parseInt(v, 16));
  const c = await f.Channel.fixture(meta.origin, config, meta.key);
  const bearer = 'a'.repeat(43);
  const chat = {model:'fixture',stream:true,submission:'s'.repeat(43),messages:[{role:'user',content:''}]};
  const session = {challenge:'c'.repeat(43),credential:'c'.repeat(43)};
  const submission = {model:'fixture',new_conversation:true};
  let checks = 0, getterCalls = 0;
  const check = (ok, name) => { if (!ok) throw new Error('admission: '+name); };
  function wide(seed) {
    const value = {...seed};
    for (let i = Object.keys(seed).length; i < 65540; i++) value['extra'+i] = null;
    return value;
  }
  const wideChat = wide(chat), wideMessage = wide(chat.messages[0]);
  const wideSession = wide(session), wideSubmission = wide(submission);
  // Resolve lazy Node web globals before trapping application allocation APIs.
  const requestPrototype = Request.prototype, subtle = crypto.subtle;
  void globalThis.fetch;
  async function beforeWork(name, run, descriptorCap = Infinity) {
    let collections = 0, stringify = 0, encryption = 0, network = 0, descriptors = 0;
    const saved = [];
    function replace(object, key, fn) { const previous = object[key]; saved.push(()=>{object[key]=previous;}); object[key]=fn; return previous; }
    const forbidden = () => { collections++; throw new Error('whole-object collection'); };
    for (const key of ['getOwnPropertyDescriptors','entries','keys','getOwnPropertyNames','getOwnPropertySymbols']) replace(Object,key,forbidden);
    replace(Reflect,'ownKeys',forbidden);
    const descriptor = replace(Object,'getOwnPropertyDescriptor',(...args)=>{descriptors++; return descriptor(...args);});
    replace(JSON,'stringify',()=>{stringify++; throw new Error('unchecked stringify');});
    replace(requestPrototype,'arrayBuffer',()=>{encryption++; throw new Error('EHBP request collection');});
    replace(subtle,'encrypt',()=>{encryption++; throw new Error('encryption');});
    replace(globalThis,'fetch',()=>{network++; throw new Error('network');});
    let rejected = false;
    try { await run(); } catch (e) { rejected = e.code === 'rejected'; }
    finally { for (const restore of saved.reverse()) restore(); }
    check(rejected, name+' rejection');
    check(collections === 0 && stringify === 0 && encryption === 0 && network === 0, name+' pre-work');
    check(descriptors <= descriptorCap, name+' incremental descriptors');
    check(getterCalls === 0, name+' accessor invocation');
    checks++;
  }
  await beforeWork('production API approval empty',()=>m.Channel.api());
  await beforeWork('production fixture disabled',()=>m.Channel.fixture(meta.origin,config,meta.key));
  await beforeWork('fixture cannot select production',()=>f.Channel.fixture(m.WEB_APPROVAL.origin,config,meta.key));
  await beforeWork('wide encodeChat',()=>m.encodeChat(wideChat),10);
  await beforeWork('wide public chat',()=>c.chat(wideChat,bearer),10);
  await beforeWork('wide nested message',()=>c.chat({...chat,messages:[wideMessage]},bearer),16);
  await beforeWork('wide session',()=>c.control('/v1/sessions',wideSession),6);
  await beforeWork('wide submission',()=>c.control('/v1/submissions',wideSubmission,bearer),6);
  await beforeWork('generic wide node limit',()=>m.serialize(wideChat,m.LIMITS.chat),2*m.LIMITS.nodes+2);
  const accessor = (seed, key, enumerable = true) => Object.defineProperty({...seed},key,{enumerable,configurable:true,get(){getterCalls++;return 'forbidden';}});
  for (const key of ['model','stream','submission','messages']) await beforeWork('chat accessor '+key,()=>c.chat(accessor(chat,key),bearer));
  for (const key of ['role','content']) await beforeWork('message accessor '+key,()=>c.chat({...chat,messages:[accessor(chat.messages[0],key)]},bearer));
  for (const key of ['challenge','credential']) await beforeWork('session accessor '+key,()=>c.control('/v1/sessions',accessor(session,key)));
  for (const key of ['model','new_conversation']) await beforeWork('submission accessor '+key,()=>c.control('/v1/submissions',accessor(submission,key),bearer));
  await beforeWork('hidden accessor',()=>c.chat(accessor(chat,'extra',false),bearer));
  await beforeWork('hidden unknown field',()=>c.chat(Object.defineProperty({...chat},'extra',{value:null}),bearer));
  await beforeWork('hidden required field',()=>c.chat(Object.defineProperty({...chat},'model',{value:'fixture',enumerable:false}),bearer));
  await beforeWork('symbol field',()=>c.chat({...chat,[Symbol('extra')]:null},bearer));
  const sparse = new Array(2); sparse[1] = chat.messages[0];
  await beforeWork('sparse messages',()=>c.chat({...chat,messages:sparse},bearer));
  const arrayAccessor = [chat.messages[0]];
  Object.defineProperty(arrayAccessor,'0',{get(){getterCalls++;return chat.messages[0];}});
  await beforeWork('array accessor',()=>c.chat({...chat,messages:arrayAccessor},bearer));
  await beforeWork('array unknown field',()=>c.chat({...chat,messages:Object.assign([chat.messages[0]],{extra:null})},bearer));
  await beforeWork('message count',()=>c.chat({...chat,messages:new Array(m.LIMITS.messages+1)},bearer));
  await beforeWork('malformed chat',()=>c.chat({...chat,stream:false},bearer));
  await beforeWork('malformed message',()=>c.chat({...chat,messages:[{role:'tool',content:''}]},bearer));
  await beforeWork('malformed session',()=>c.control('/v1/sessions',{...session,challenge:false}));
  await beforeWork('malformed submission',()=>c.control('/v1/submissions',{...submission,new_conversation:'true'},bearer));
  await beforeWork('oversized session',()=>c.control('/v1/sessions',{...session,credential:'c'.repeat(m.LIMITS.control)}));
  await beforeWork('oversized submission',()=>c.control('/v1/submissions',{...submission,model:'m'.repeat(m.LIMITS.control)},bearer));
  for (const value of [undefined,1,1n,()=>{},new Date(),new Uint8Array(1)]) await beforeWork('unsupported type',()=>m.serialize({value},m.LIMITS.control));
  await beforeWork('toJSON hook',()=>m.serialize({toJSON(){getterCalls++;return ''; }},m.LIMITS.control));
  await beforeWork('generic sparse array',()=>m.serialize(new Array(2),m.LIMITS.control));
  const nested = depth => {let v=null;while(depth--)v=[v];return v;};
  check(m.serialize(nested(m.LIMITS.depth),m.LIMITS.control).length>0,'depth inclusive'); checks++;
  await beforeWork('depth exceeded',()=>m.serialize(nested(m.LIMITS.depth+1),m.LIMITS.control));
  const nodes = Array(m.LIMITS.nodes-1).fill(null);
  check(m.serialize(nodes,m.LIMITS.chat).length>0,'nodes inclusive'); checks++;
  await beforeWork('nodes exceeded',()=>m.serialize([...nodes,null],m.LIMITS.chat));
  await beforeWork('aggregate nodes exceeded',()=>m.serialize([nodes],m.LIMITS.chat));
  await beforeWork('incremental aggregate bytes',()=>m.serialize({a:'a'.repeat(8),b:'b'.repeat(8),c:'c'.repeat(8)},16),4);
  for (const text of ['', 'ASCII', 'é漢😀\ud800X\udfff\n\r\t\b\f"\\\u0000']) {
    const value = {text}, expected = JSON.stringify(value), size = new TextEncoder().encode(expected).length;
    check(new TextDecoder().decode(m.serialize(value,size))===expected,'Unicode/escaping bytes'); checks++;
    await beforeWork('Unicode/escaping one byte over',()=>m.serialize(value,size-1));
  }
  check(m.serialize('x'.repeat(m.LIMITS.control-2),m.LIMITS.control).length===m.LIMITS.control,'control bytes inclusive'); checks++;
  await beforeWork('control bytes exceeded',()=>m.serialize('x'.repeat(m.LIMITS.control-1),m.LIMITS.control));
  const overhead = new TextEncoder().encode(JSON.stringify(chat)).length;
  const fullChat = {...chat,messages:[{role:'user',content:'x'.repeat(m.LIMITS.chat-overhead)}]};
  check(m.encodeChat(fullChat).length===m.LIMITS.chat,'chat bytes inclusive'); checks++;
  await beforeWork('chat bytes exceeded',()=>c.chat({...chat,messages:[{role:'user',content:fullChat.messages[0].content+'x'}]},bearer));
  check(m.encodeChat({...chat,messages:Array(m.LIMITS.messages).fill(chat.messages[0])}).length>0,'messages inclusive'); checks++;
  const frozen = Object.freeze({...chat,messages:Object.freeze([Object.freeze({...chat.messages[0]})])});
  check(m.encodeChat(frozen).length>0,'frozen data objects'); checks++;
  const originalStringify = JSON.stringify;
  JSON.stringify = value => {
    check(value!==chat && value.messages!==chat.messages && value.messages[0]!==chat.messages[0],'snapshot not unchecked aliases');
    check(Object.getPrototypeOf(value)===null && Object.getPrototypeOf(value.messages)===null,'no inherited toJSON');
    return originalStringify(value);
  };
  try { m.encodeChat(chat); checks++; } finally { JSON.stringify=originalStringify; }
  return {checks,wideProperties:65540,wholeObjectCollections:0,stringifyOnRejection:0,encryptionOnRejection:0,networkOnRejection:0,getterCalls};
}
const admissionNode = await admissionRegressions({productionURL:pathToFileURL(path.join(build,'channel-node.mjs')).href,fixtureURL:pathToFileURL(path.join(build,'fixture-node.mjs')).href,meta});
passed += admissionNode.checks;
// Actual TLS socket, real shim/middleware, no global agent or validation bypass.
globalThis.fetch = async req => {
  assert.equal(req.credentials,'omit'); assert.equal(req.redirect,'error'); assert.equal(req.cache,'no-store');
  assert.equal(new URL(req.url).origin,meta.origin);
  return originalFetch(req);
};
async function fixtureControl(name) {
  const res = await originalFetch(meta.origin+'/fixture/'+name,{redirect:'error',credentials:'omit',cache:'no-store',signal:AbortSignal.timeout(5000)});
  assert.ok(res.ok);
  return name === 'stats' ? res.json() : res.body?.cancel();
}
await fixtureControl('normal');
const publicFetch = p => originalFetch(meta.origin+p,{redirect:'error',credentials:'omit',cache:'no-store',signal:AbortSignal.timeout(5000)});
const observedKey = new Uint8Array(await (await publicFetch('/.well-known/hpke-keys')).arrayBuffer());
assert.deepEqual(observedKey,config); passed++;
const observedEvidence = await (await publicFetch('/.well-known/tinfoil-attestation')).json();
assert.equal(observedEvidence.format,'https://tinfoil.sh/predicate/dummy/v2'); passed++;
assert.equal((await publicFetch('/.well-known/tinfoil-certificate')).status,200); passed++;
assert.equal((await publicFetch('/not-allowed')).status,404); passed++;
const cors = await originalFetch(meta.origin+'/v1/chat/completions',{method:'OPTIONS',headers:{Origin:'https://phase01.invalid','Access-Control-Request-Headers':'authorization,content-type,ehbp-encapsulated-key'},credentials:'omit',redirect:'error',cache:'no-store',signal:AbortSignal.timeout(5000)});
assert.equal(cors.status,204); assert.equal(cors.headers.get('Access-Control-Allow-Origin'),'https://phase01.invalid'); passed++;
const models = await channel.models(bearer); assert.equal(models.object,'list'); passed++;
const response = await channel.chat(chat,bearer);
const reader = response.getReader();
assert.equal(new TextDecoder().decode((await reader.read()).value),'data: first\n\n');
assert.equal((await fixtureControl('stats')).released,0); passed++;
await fixtureControl('release');
let suffix=''; for (;;) { const r=await reader.read(); if(r.done)break; suffix+=new TextDecoder().decode(r.value); }
assert.equal(suffix,'data: last\n\n'); passed++;
let stats=await fixtureControl('stats'); assert.equal(stats.calls,2); assert.equal(stats.authorized,2); assert.equal(stats.encrypted,1);
for(const mode of ['plaintext','redirect']) {
  await fixtureControl(mode);
  const before=(await fixtureControl('stats')).encrypted;
  const start=performance.now();
  await rejects(mode,()=>channel.chat(chat,bearer),'uncertain');
  assert.ok(performance.now()-start<2000,'plaintext error was drained or cleanup stalled');
  assert.equal((await fixtureControl('stats')).encrypted,before+1,'automatic replay');
}
await fixtureControl('normal');
assert.deepEqual(await channel.control('/v1/sessions',{challenge:'c'.repeat(43),credential:'c'.repeat(43)}),{ok:true}); passed++;
assert.deepEqual(await channel.control('/v1/submissions',{model:'fixture',new_conversation:true},bearer),{ok:true}); passed++;
for(const model of ['oversized-event','invalid-utf8']) {
  await rejects(model,async()=>{const r=(await channel.chat({...chat,model},bearer)).getReader();for(;;){if((await r.read()).done)break;}},'uncertain');
}
// Hostile fetch streams exercise cancellation/ceilings before SDK buffering. The
// crypto and verified flags are NOT mocked. A never-ending plaintext 422 is never read.
let read=0,cancel=0;
globalThis.fetch = async () => new Response(new ReadableStream({pull(){read++;},cancel(){cancel++;return new Promise(()=>{});}},{highWaterMark:0}),{status:422});
let start=performance.now();
await rejects('no plaintext clone/drain',()=>channel.chat(chat,bearer),'uncertain');
assert.equal(read,0); assert.equal(cancel,1); assert.ok(performance.now()-start<1000);
globalThis.fetch = async () => new Response(new ReadableStream({pull(c){c.enqueue(new Uint8Array(65537));},cancel(){cancel++;}},{highWaterMark:0}));
await rejects('oversized catalog',()=>channel.models(bearer));
globalThis.fetch = async () => new Response(new ReadableStream({pull(){},cancel(){cancel++;}},{highWaterMark:0}));
start=performance.now();
await rejects('idle deadline',()=>channel.models(bearer));
assert.ok(performance.now()-start>=14000 && performance.now()-start<18000); passed++;
globalThis.fetch = async () => new Response(new ReadableStream({start(c){c.enqueue(new Uint8Array([4,0,0,0]));},cancel(){cancel++;}},{highWaterMark:0}),{headers:{'Ehbp-Response-Nonce':'0'.repeat(64)}});
await rejects('frame ceiling before SDK buffer',async()=>{const r=(await channel.chat(chat,bearer)).getReader();await r.read();},'uncertain');
let calls=0;
globalThis.fetch = async () => {calls++; throw new Error('transport');};
await rejects('post-send network no replay',()=>channel.chat(chat,bearer),'uncertain'); assert.equal(calls,1);
assert.ok(cancel>=3);
let acquiredBytes=0,acquisitionCancel=0;
globalThis.fetch=async req=>{
  assert.equal(req.url,prod.WEB_APPROVAL.origin+'/.well-known/tinfoil-attestation');
  assert.equal(req.credentials,'omit');assert.equal(req.redirect,'error');assert.equal(req.cache,'no-store');
  assert.equal(req.headers.has('Authorization'),false);assert.equal(req.body,null);
  return new Response(new ReadableStream({pull(c){acquiredBytes+=4096;c.enqueue(new Uint8Array(4096));},cancel(){acquisitionCancel++;}},{highWaterMark:0}),{headers:{'Content-Length':'1'}});
};
await rejects('fragmented oversized evidence misleading length',()=>prod.observeWeb(manifest));
assert.equal(acquisitionCancel,1);assert.equal(acquiredBytes,prod.LIMITS.evidence+4096);
globalThis.fetch=originalFetch;

// Browser dependency/crypto qualification only, not packet-06 UI/device harness.
const {chromium}=await import(pathToFileURL(path.join(install,'node_modules/playwright/index.mjs')));
const fixtureCert=new X509Certificate(await readFile(path.join(fixtureDir,'ca.pem')));
const fixtureSPKI=createHash('sha256').update(fixtureCert.publicKey.export({type:'spki',format:'der'})).digest('base64');
// Isolated browser synthetic-leaf trust only; never a production TLS bypass.
const browser=await chromium.launch({executablePath:browserPath,headless:true,args:['--ignore-certificate-errors-spki-list='+fixtureSPKI]});
try {
  assert.equal(browser.version(),'149.0.7827.55');
  const context=await browser.newContext({serviceWorkers:'block'});
  await context.grantPermissions(['local-network-access'],{origin:'https://phase01.invalid'});
  await context.addCookies([{name:'phase01_ambient',value:'must-not-send',url:meta.origin,secure:true,sameSite:'None'}]);
  const source=await readFile(path.join(build,'channel-browser.mjs'),'utf8');
  const fixtureSource=await readFile(path.join(build,'fixture-browser.mjs'),'utf8');
  let unexpected=0;
  await context.route('**/*',async route=>{
    const url=route.request().url();
    if(url==='https://phase01.invalid/') return route.fulfill({contentType:'text/html',body:'<!doctype html><title>qualification</title>'});
    if(url==='https://phase01.invalid/channel.mjs') return route.fulfill({contentType:'text/javascript',body:source});
    if(url==='https://phase01.invalid/fixture.mjs') return route.fulfill({contentType:'text/javascript',body:fixtureSource});
    if([meta.origin+'/v1/models',meta.origin+'/v1/chat/completions'].includes(url)) return route.continue();
    unexpected++; await route.abort();
  });
  const page=await context.newPage(); await page.goto('https://phase01.invalid/');
  const admissionBrowser = await page.evaluate(admissionRegressions,{productionURL:'/channel.mjs',fixtureURL:'/fixture.mjs',meta});
  assert.deepEqual(admissionBrowser,admissionNode); passed+=admissionBrowser.checks;
  const result=await page.evaluate(async({bundle,manifest,key})=>{
    let persistenceCalls=0;
    for(const name of ['getItem','setItem','removeItem','clear']) Storage.prototype[name]=()=>{persistenceCalls++;throw new Error('persistence forbidden');};
    indexedDB.open=()=>{persistenceCalls++;throw new Error('persistence forbidden');};
    caches.open=()=>{persistenceCalls++;throw new Error('persistence forbidden');};
    const m=await import('/channel.mjs');
    const p=await m.qualifyWeb(new TextEncoder().encode(JSON.stringify(bundle)),new Uint8Array(manifest),new Uint8Array(key));
    let rejected=false;
    const bad=new Uint8Array(key);bad[3]^=1;
    try {await m.qualifyWeb(new TextEncoder().encode(JSON.stringify(bundle)),new Uint8Array(manifest),bad);}catch{rejected=true;}
    return {scope:p.scope,rejected,persistenceCalls,localStorage:localStorage.length,sessionStorage:sessionStorage.length};
  },{bundle,manifest:Array.from(manifest),key:Array.from(key)});
  assert.deepEqual(result,{scope:'web-observation-only',rejected:true,persistenceCalls:0,localStorage:0,sessionStorage:0});
  assert.equal(unexpected,0); passed+=2;
  const releasedBefore=(await fixtureControl('stats')).released;
  const first=await page.evaluate(async({meta,chat,bearer})=>{
    const m=await import('/fixture.mjs');
    const cfg=Uint8Array.from(meta.config.match(/../g),v=>parseInt(v,16));
    const c=await m.Channel.fixture(meta.origin,cfg,meta.key);
    const models=await c.models(bearer);
    const reader=(await c.chat(chat,bearer)).getReader();
    globalThis.fixtureReader=reader;
    return {object:models.object,first:new TextDecoder().decode((await reader.read()).value)};
  },{meta,chat,bearer});
  assert.deepEqual(first,{object:'list',first:'data: first\n\n'});
  assert.equal((await fixtureControl('stats')).released,releasedBefore);
  await fixtureControl('release');
  const last=await page.evaluate(async()=>{let s='';for(;;){const r=await globalThis.fixtureReader.read();if(r.done)break;s+=new TextDecoder().decode(r.value);}delete globalThis.fixtureReader;return s;});
  assert.equal(last,'data: last\n\n');assert.equal(unexpected,0);passed++;
  await writeFile(path.join(build,'browser-identity.json'),JSON.stringify({version:browser.version(),executable:await realpath(browserPath),sha256:createHash('sha256').update(await readFile(browserPath)).digest('hex')}));
} finally {await browser.close(); await fixtureControl('stop');}
console.log(JSON.stringify({passed,failedVerificationCredentialSends:credentials,failedVerificationContentSends:content,progressiveAuthenticatedDecryption:true,automaticReplay:false,browserVerifier:true,admissionNode,admissionChromium:true}));
