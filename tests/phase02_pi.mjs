// Offline, synthetic-credential SDK/provider checks. PHASE02_TEST_BUILD must be a separately
// compiled test-entry.ts bundle with fixture capability, never the shipped package.
import assert from 'node:assert/strict';
import { isDeepStrictEqual } from 'node:util';
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
const file = process.env.PHASE02_TEST_BUILD;
const root = process.env.PHASE02_SCRATCH;
const piRoot = process.env.POSSUMS_PI_ROOT;
assert(file && root && piRoot, 'explicit scratch bundle, scratch state and pinned Pi paths required');
const bundle = await import(pathToFileURL(file).href);
// Direct provider unit fixtures model an already started trust session. The
// extension/native lifecycle fixtures below use the real startup handler.
const m = { ...bundle, PossumsProvider: class extends bundle.PossumsProvider {
 constructor(...args) { super(...args); this.newSession(); }
} };
const ai = await import(pathToFileURL(path.join(piRoot, 'node_modules/@earendil-works/pi-ai/dist/index.js')).href);
const coding = await import(pathToFileURL(path.join(piRoot, 'node_modules/@earendil-works/pi-coding-agent/dist/index.js')).href);
const { prepareCompaction } = await import(pathToFileURL(path.join(piRoot, 'node_modules/@earendil-works/pi-coding-agent/dist/core/compaction/compaction.js')).href);
const { AuthStorage: NativeAuthStorage } = await import(pathToFileURL(path.join(piRoot, 'node_modules/@earendil-works/pi-coding-agent/dist/core/auth-storage.js')).href);
const { isRetryableAssistantError, retryDelayMs, retryAssistantCall } = await import(pathToFileURL(path.join(piRoot, 'node_modules/@earendil-works/pi-ai/dist/utils/retry.js')).href);
fs.mkdirSync(root, { recursive: true });
const passed = [];
async function check(name, body) { await body(); passed.push(name); console.log('PASS '+name); }
const entry = tools => ({ object: 'model', id: 'synthetic', context_tokens: '20', max_output_tokens: '20',
  input_microunits_per_million_tokens: '1000000', output_microunits_per_million_tokens: '1000000', maximum_reservation_microunits: '52',
  ...(tools ? { tool_protocol: 'openai-functions-v1' } : {}) });
const receipt = finish => ({ finish, inputTokens: 2, outputTokens: 3, totalTokens: 5, chargedMicrounits: '7', refundedMicrounits: '45',
  quotedInputMicrounitsPerMillion: '1000000', quotedOutputMicrounitsPerMillion: '1000000' });
const event = delta => ({ object: 'chat.completion.chunk', model: 'synthetic', choices: [{ index: 0, delta, finish_reason: null }] });
const user = { role: 'user', content: 'Synthetic input', timestamp: 1 };
const recoveryKey = 'synthetic_not_a_usable_credential';
const envKey = 'synthetic_not_a_usable_env_credential';
const legacyMarker = 'possums-memory-only-session';
const requestMarker = 'possums-verified-memory-session';
const authInput = (credential, env = undefined, signal = new AbortController().signal) => ({ credential, signal,
 ctx: { env: async name => name === 'POSSUMS_RECOVERY_CREDENTIAL' ? env : undefined, fileExists: async () => false } });
const interaction = (key = recoveryKey, signal = new AbortController().signal) => ({ signal, prompt: async () => key, notify: () => {} });
const tool = { name: 'echo', description: 'Synthetic echo', parameters: ai.Type.Object({ value: ai.Type.String() }) };
function deferred() { let resolve; const promise = new Promise(r => { resolve=r; }); return {promise,resolve}; }
async function terminalGatewayError(code, detail, billing) {
 const body = new ReadableStream({ start(controller) {
  controller.enqueue(new TextEncoder().encode('data: '+JSON.stringify({error:{code,detail,billing,message:'PRIVATE_PROMPT'}})+'\n\n'));
  controller.close();
 } });
 try { await m.consumeCompletion(body,'synthetic',()=>{}); assert.fail('expected gateway error'); }
 catch (error) { assert(error instanceof m.GatewayError); return error; }
}
function freshFixture(candidate) {
 const fresh=new m.ReferenceClient(candidate.channel);
 for(const name of ['login','models','chat','balance'])if(Object.hasOwn(candidate,name))fresh[name]=(...args)=>candidate[name](...args);
 return fresh;
}
async function setup(plan, tools = true) {
  const key = new Uint8Array([0,0,32,...Array(32).fill(7),0,4,0,1,0,2]);
  const channel = await m.Channel.fixture('https://localhost:18443', key, '07'.repeat(32));
  const client = new m.ReferenceClient(channel);
  const requests = [];
  let sends = 0; let failModels = false; let abortSeen = false; let release;
  client.login = async () => {};
  client.balance = async () => ({availableMicrounits:'5000000',inFlight:0,completedRequests:'0'});
  client.models = async () => {
    if (failModels) throw new Error('private diagnostic must not escape');
    return m.validateModels({ object:'list', data:[entry(tools)] });
  };
  client.chat = async (_model, messages, onDelta, _newConversation, options) => {
    const mode = plan[sends++];
    requests.push({ messages: structuredClone(messages), newConversation: _newConversation, tools: options.tools });
    if (mode === 'held' || mode === 'held_tools') release=deferred();
    if (options.onPayload) await options.onPayload({model:'synthetic',stream:true,messages,...(options.tools?{tools:options.tools}:{})});
    await options.onResponse?.({status:200,contentType:'text/event-stream'});
    if(['fragmented_tools','late_fragmented_tools','fragmented_tools_uncertain','fragmented_tools_transient'].includes(mode)){
      const frame=value=>'data: '+JSON.stringify(value)+'\n\n';
      const identity=event({tool_calls:[{index:0,id:'call_one',type:'function',function:{name:'echo'}}]});
      const argumentsEvent=event({tool_calls:[{index:0,function:{arguments:mode==='fragmented_tools_transient'?'{"value":':'{"value":"ok"}'}}]});
      const fragments=mode==='late_fragmented_tools'?[event({tool_calls:[{index:0}]}),argumentsEvent,identity]:[identity,argumentsEvent];
      const finish={object:'chat.completion.chunk',model:'synthetic',choices:[{index:0,delta:{},finish_reason:'tool_calls'}]};
      const usage={object:'chat.completion.chunk',model:'synthetic',choices:[],usage:{prompt_tokens:2,completion_tokens:3,total_tokens:5},
        possums:{outcome:'settled',charged_microunits:'7',refunded_microunits:'45',quoted_input_microunits_per_million_tokens:'1000000',quoted_output_microunits_per_million_tokens:'1000000'}};
      const text=[event({role:'assistant'}),event({content:'Synthetic preamble'}),...fragments,...(mode==='fragmented_tools_transient'?[]:[finish])].map(frame).join('')+
        (mode==='fragmented_tools_transient'?frame({error:{code:'generation_failed',detail:'stream_transport_failed',billing:'unknown',message:'PRIVATE_PROMPT'}}):
          mode==='fragmented_tools_uncertain'?'':frame(usage)+'data: [DONE]\n\n');
      const body=new ReadableStream({start(controller){controller.enqueue(new TextEncoder().encode(text));controller.close();}});
      return m.consumeCompletion(body,entry(true),onDelta,{tools:options.tools,onEvent:options.onEvent,signal:options.signal});
    }
    await options.onEvent(event({role:'assistant'}));
    if (['tool_calls','stop_tools','tool_length','length_partial','late_tools','held_tools'].includes(mode)) {
      await options.onEvent(event({tool_calls:[{index:0,...(mode==='late_tools'?{}:{id:'call_one',type:'function'}),function:{...(mode==='late_tools'?{}:{name:'echo'}),arguments:'{"value":'}}]}));
      if (mode !== 'length_partial') await options.onEvent(event({tool_calls:[{index:0,...(mode==='late_tools'?{id:'call_one',type:'function'}:{}),function:{...(mode==='late_tools'?{name:'echo'}:{}),arguments:'"ok"}'}}]}));
    } else {
      const text = mode === 'empty' ? '   ' : 'progressive';
      await options.onEvent(event({content:text}));
      onDelta(text);
    }
    if (mode === 'held' || mode === 'held_tools') await release.promise;
    if (mode === 'uncertain') throw new m.ChannelError('uncertain');
    if (mode === 'refund') throw await terminalGatewayError('generation_failed', 'stream_idle_timeout', 'refunded');
    if (mode === 'unknown_bill') throw await terminalGatewayError('unavailable', 'inference_unavailable', 'unknown');
    if (mode === 'quiescing') throw new m.GatewayError('service_quiescing', undefined, 'unknown', 503);
    if (mode && typeof mode === 'object') throw await terminalGatewayError(mode.code, mode.detail, mode.billing ?? 'unknown');
    if (mode === 'sdk_decode') throw new m.GatewayError('generation_failed', 'sdk_stream_decode_failed', 'unknown');
    if (mode === 'hostile') throw new Error('possums_secret_key_PRIVATE_PROMPT');
    if (mode === 'abort') await new Promise((_resolve,reject)=>{ options.signal.addEventListener('abort',()=>{abortSeen=true;reject(new m.ChannelError('uncertain'));},{once:true}); });
    return receipt(['tool_calls','late_tools','held_tools'].includes(mode) ? 'tool_calls' : ['tool_length','length_partial','length'].includes(mode) ? 'length' : 'stop');
  };
  client.freshSession=()=>freshFixture(client);
  const provider = new m.PossumsProvider(async()=>client);
  const credential = await provider.auth.apiKey.login({signal:new AbortController().signal,prompt:async()=> 'synthetic_not_a_usable_credential'});
  const selected = provider.getModels()[0];
  return { provider, client, credential, selected, requests, sends:()=>sends, failModels:()=>{failModels=true;}, abortSeen:()=>abortSeen, release:()=>release?.resolve() };
}
async function drain(stream) { const events=[];for await(const event of stream)events.push(event); return {events,message:await stream.result()}; }
const context = tools => ai.normalizeContext({messages:[user],tools:tools?[tool]:[]});
const { diagnosticChecks } = await import('./phase02_diagnostics.mjs');
await diagnosticChecks(m, check);

// Native SDK storage is always injected beneath this test's isolated scratch root.
async function nativeRuntime(provider, credentials, modelsStore = new ai.InMemoryModelsStore()) {
 const runtime=await coding.ModelRuntime.create({credentials,modelsStore,modelsPath:null,allowModelNetwork:false,refreshOnCreate:false});
 runtime.registerNativeProvider(provider);
 await runtime.refresh({providers:['possums'],allowNetwork:false});
 return runtime;
}
async function authFixture(plan = []) {
 const s=await setup(plan);const trace={establish:0,keys:[],models:0,conversations:[]};
 const establish=async()=>{
  trace.establish++;
  const candidate=new m.ReferenceClient(s.client.channel);
  candidate.login=async key=>{trace.keys.push(key);};
  candidate.models=async()=>{trace.models++;return s.client.models();};
  candidate.balance=(...args)=>s.client.balance(...args);
  candidate.chat=(model,messages,onDelta,newConversation,options)=>{
   trace.conversations.push(newConversation);return s.client.chat(model,messages,onDelta,newConversation,options);
  };
  candidate.freshSession=()=>freshFixture(candidate);
  return candidate;
 };
 return {s,trace,establish};
}
function fixtureExtension(establish) {
 let provider;const handlers=new Map(),commands=new Map();
 m.extension({registerFlag:()=>{},getFlag:()=>undefined,registerProvider:value=>{provider=value;value.establish=establish;},
  on:(name,handler)=>handlers.set(name,handler),registerCommand:(name,command)=>commands.set(name,command)});
 return {provider,handlers,commands};
}
await check('native AuthStorage persists recovery key; cold extension start restores verified models without inference',async()=>{
 const fixture=await authFixture(['stop','stop']);
 const authPath=path.join(root,'native-auth','auth.json');
 const storage=NativeAuthStorage.create(authPath);
 const provider=new m.PossumsProvider(fixture.establish);
 const modelsStore=new ai.InMemoryModelsStore();
 const runtime=await nativeRuntime(provider,storage,modelsStore);
 assert.equal(await runtime.checkAuth('possums'),undefined);assert.equal(fixture.trace.establish,1,'public trust starts at session startup, not auth checks');
 const credential=await runtime.login('possums','api_key',interaction());
 assert.deepEqual(credential,{type:'api_key',key:recoveryKey});
 assert.deepEqual(JSON.parse(fs.readFileSync(authPath,'utf8')),{possums:credential});
 assert.equal(fs.statSync(authPath).mode&0o777,0o600);assert.equal(fs.statSync(path.dirname(authPath)).mode&0o777,0o700);
 const discovery=await modelsStore.read('possums');assert.equal(discovery.models.length,1);assert.match(discovery.models[0].name,/last-known/);assert(!JSON.stringify(discovery).includes(recoveryKey));
 provider.logout();
 const extension=fixtureExtension(fixture.establish);
 assert(!extension.commands.has('possums-logout'));
 const restored=await nativeRuntime(extension.provider,NativeAuthStorage.create(authPath),modelsStore);
 const registry=new coding.ModelRegistry(restored);
 const before=structuredClone(fixture.trace);
 assert(await restored.checkAuth('possums'));assert.match((await restored.getAvailable('possums'))[0].name,/last-known/);assert.deepEqual(extension.provider.catalog,[]);
 assert.deepEqual(fixture.trace,before,'availability is offline even with a saved key');
 await extension.handlers.get('session_start')({}, {modelRegistry:registry});
 assert.equal(registry.getAvailable().filter(value=>value.provider==='possums').length,1);
 assert.equal(fixture.trace.establish,2);assert.deepEqual(fixture.trace.keys,[recoveryKey,recoveryKey]);
 assert.equal(fixture.s.sends(),0);assert.deepEqual((await modelsStore.read('possums')).models.map(value=>value.id),['synthetic']);
 assert.equal((await registry.getProviderAuth('possums')).auth.apiKey,requestMarker);
 const selected=extension.provider.getModels()[0];
 extension.provider.beginRun();await drain(restored.streamSimple(selected,context(false)));
 await extension.handlers.get('session_start')({}, {modelRegistry:registry});
 extension.provider.beginRun();await drain(restored.streamSimple(selected,context(false)));
 assert.deepEqual(fixture.trace.conversations,[true,true],'/new resets conversation, not saved authentication');
 assert.equal(fixture.trace.establish,3,'each new session verifies again without a Pi restart');
 await restored.logout('possums');
 assert.equal(await NativeAuthStorage.create(authPath).read('possums'),undefined);
 assert.deepEqual(JSON.parse(fs.readFileSync(authPath,'utf8')),{});
 assert.deepEqual(extension.provider.catalog,[]);assert.deepEqual(await restored.getAvailable('possums'),[]);assert.equal(await registry.getProviderAuth('possums'),undefined);
});
await check('pinned SDK checks stay offline; saved key wins over env; legacy marker requires fresh login',async()=>{
 const fixture=await authFixture();const provider=new m.PossumsProvider(fixture.establish);
 const credentials=new ai.InMemoryCredentialStore();
 const models=ai.createModels({credentials,modelsStore:new ai.InMemoryModelsStore(),authContext:authInput(undefined,envKey).ctx});
 models.setProvider(provider);
 assert.equal((await models.checkAuth('possums')).source,'POSSUMS_RECOVERY_CREDENTIAL');
 assert.deepEqual(await models.getAvailable('possums'),[]);assert.equal(fixture.trace.establish,1);
 await credentials.modify('possums',async()=>({type:'api_key',key:recoveryKey}));
 assert.equal((await models.checkAuth('possums')).source,'stored credential');assert.equal(fixture.trace.establish,1);
 assert.equal((await models.refresh({providers:['possums'],allowNetwork:true})).errors.size,0);
 assert.deepEqual(fixture.trace.keys,[recoveryKey]);
 await models.logout('possums');await models.refresh({providers:['possums'],allowNetwork:false});
 assert.deepEqual(provider.catalog,[]);
 assert.equal((await models.checkAuth('possums')).source,'POSSUMS_RECOVERY_CREDENTIAL');
 await models.refresh({providers:['possums'],allowNetwork:true});assert.deepEqual(fixture.trace.keys,[recoveryKey,envKey]);
 for(const marker of [legacyMarker,requestMarker]) {
  await credentials.modify('possums',async()=>({type:'api_key',key:marker}));
  assert.equal(await models.checkAuth('possums'),undefined);
  await models.refresh({providers:['possums'],allowNetwork:true});
  assert.equal(await models.getAuth('possums'),undefined);assert.deepEqual(provider.catalog,[]);assert.deepEqual(await models.getAvailable('possums'),[]);
 }
 assert.deepEqual(fixture.trace.keys,[recoveryKey,envKey],'neither marker is sent or silently falls back to env');
 await models.login('possums','api_key',interaction());
 assert.deepEqual(await credentials.read('possums'),{type:'api_key',key:recoveryKey});
 assert.equal((await models.getAuth('possums')).auth.apiKey,requestMarker);assert.equal(fixture.s.sends(),0);
});
await check('local approval expiry and mismatched manifest fail before any evidence fetch',async()=>{
 const now=Date.now,fetch=globalThis.fetch;let calls=0;
 globalThis.fetch=async()=>{calls++;throw new Error('PRIVATE_URL_CREDENTIAL');};
 try {
  Date.now=()=>m.API_APPROVALS[0].expires-1000;
  await assert.rejects(m.connect(new Uint8Array()),error=>error instanceof m.ConnectionFailure && error.code==='manifest_mismatch');
  Date.now=()=>m.API_APPROVALS[0].expires;
  await assert.rejects(m.connect(new Uint8Array()),error=>error instanceof m.ConnectionFailure && error.code==='approval_expired');
  assert.equal(calls,0);
 } finally {Date.now=now;globalThis.fetch=fetch;}
});
await check('extension reports missing or invalid manifest paths without revealing them or prompting',async()=>{
 for(const manifest of ['PRIVATE_PATH',path.join(root,'PRIVATE_PATH_missing.json')]) {
  let provider,prompts=0;
  m.extension({registerFlag:()=>{},getFlag:()=>manifest,registerProvider:value=>{provider=value;},on:()=>{},registerCommand:()=>{}});
  provider.newSession();
  await assert.rejects(provider.auth.apiKey.login({...interaction(),prompt:async()=>{prompts++;return recoveryKey;}}),
   error=>error.code==='manifest_unavailable' && !error.message.includes('PRIVATE_PATH'));
  assert.equal(prompts,0);
 }
});
await check('connection categories survive restoration and login without exposing hostile errors or prompting',async()=>{
 for(const code of ['approval_expired','approval_unavailable','manifest_unavailable','manifest_mismatch','evidence_unavailable','verification_failed']) {
  let prompts=0;const notices=[];
  const provider=new m.PossumsProvider(async()=>{throw new m.ConnectionFailure(code);},failure=>notices.push(failure));
  for(const operation of [()=>provider.auth.apiKey.resolve(authInput({type:'api_key',key:recoveryKey})),
   ()=>provider.auth.apiKey.login({...interaction(),prompt:async()=>{prompts++;return recoveryKey;}})]) {
   await assert.rejects(operation(),error=>error instanceof m.ConnectionFailure && error.code===code && error.message.includes('No inference request was sent by this connection attempt'));
  }
  assert.equal(prompts,0);assert.equal(notices.length,2);assert.deepEqual(provider.getModels(),[]);
 }
 const hostile='PRIVATE_URL_CREDENTIAL\u001b[31m';
 const provider=new m.PossumsProvider(async()=>{throw new Error(hostile);},()=>{throw new Error(hostile);});
 await assert.rejects(provider.auth.apiKey.resolve(authInput({type:'api_key',key:recoveryKey})),error=>error.code==='verification_failed' && !error.message.includes(hostile));
 assert(!m.connectionFailure(new Error('possums_approval_expired')).message.includes('[possums_approval_expired]'));
});
await check('native refresh shows one actionable notice and offline status; full authentication clears it',async()=>{
 const fixture=await authFixture();
 const extension=fixtureExtension(async()=>{throw new m.ConnectionFailure('verification_failed');});
 const credentials=new ai.InMemoryCredentialStore();
 await credentials.modify('possums',async()=>({type:'api_key',key:recoveryKey}));
 const runtime=await nativeRuntime(extension.provider,credentials),registry=new coding.ModelRegistry(runtime),notices=[];
 const ctx={hasUI:true,ui:{notify:text=>notices.push(text)},modelRegistry:registry};
 await extension.handlers.get('session_start')({},ctx);
 await registry.refresh({providers:['possums'],allowNetwork:true});
 assert.equal(notices.length,1);assert.match(notices[0],/possums_verification_failed/);assert.match(notices[0],/new Pi session/);
 assert.match(notices[0],/Cached models do not authorize inference/);assert.deepEqual(extension.provider.getModels(),[]);
 await extension.commands.get('possums-status').handler('',ctx);
 assert.equal(notices.length,2);assert.equal(notices[1],notices[0]);
 assert.equal(fixture.trace.establish,0,'status does not connect');
 extension.provider.establish=fixture.establish;
 extension.provider.newSession();
 await extension.provider.verifySession();
 await registry.refresh({providers:['possums'],allowNetwork:true});
 await extension.commands.get('possums-status').handler('',ctx);
 assert.equal(notices.at(-1),m.approvalSummary());assert.equal(fixture.s.sends(),0);
 assert(!notices.join('\n').includes(recoveryKey));
});
const hostileConnection = 'PRIVATE_URL_CREDENTIAL_PROMPT\u001b[31m';
function assertConnectionFailure(error, code) {
 assert(error instanceof m.ConnectionFailure);assert.equal(error.code,code);
 assert.match(error.message,new RegExp('\\[possums_'+code+'\\]'));
 assert.match(error.message,/No inference request was sent by this connection attempt/);
 assert.match(error.message,/Cached models do not authorize inference/);
 assert(!error.message.includes(hostileConnection));assert.equal(error.cause,undefined);
 return true;
}
await check('native session and initial catalog failures reach status, deduplicate and clear only on full recovery',async()=>{
 for(const phase of ['login','load','validation','conversion']) {
  const fixture=await authFixture(),originalModels=fixture.s.client.models;let failing=true;
  const injectedModels=async()=>{
   if(failing && phase==='load')throw new Error(hostileConnection);
   if(failing && phase==='validation')return m.validateModels({object:'list',data:[{id:hostileConnection}]});
   if(failing && phase==='conversion')return [{...entry(true),id:hostileConnection,context_tokens:'9007199254740992'}];
   return originalModels();
  };
  fixture.s.client.models=injectedModels;
  const extension=fixtureExtension(async()=>{
   const candidate=await fixture.establish(),login=candidate.login;
   candidate.login=async(...args)=>{if(failing && phase==='login')throw new Error(hostileConnection);await login(...args);};
   return candidate;
  });
  const credentials=new ai.InMemoryCredentialStore();
  await credentials.modify('possums',async()=>({type:'api_key',key:recoveryKey}));
  const runtime=await nativeRuntime(extension.provider,credentials),registry=new coding.ModelRegistry(runtime),notices=[];
  const ctx={hasUI:true,ui:{notify:text=>notices.push(text)},modelRegistry:registry};
  const code=phase==='login'?'session_unavailable':phase==='conversion'?'catalog_conversion_failed':'catalog_unavailable';
  await extension.handlers.get('session_start')({},ctx);
  await registry.refresh({providers:['possums'],allowNetwork:true});
  assert.equal(notices.length,1);assert.match(notices[0],new RegExp('possums_'+code));
  assert.match(notices[0],/check connectivity/i);assert.deepEqual(extension.provider.getModels(),[]);
  await assert.rejects(extension.provider.auth.apiKey.login(interaction()),error=>assertConnectionFailure(error,code));
  assert.equal(notices.length,1,'interactive failure also preserves warning deduplication');
  const before=structuredClone(fixture.trace);
  await extension.commands.get('possums-status').handler('',ctx);
  assert.equal(notices.at(-1),notices[0]);assert.deepEqual(fixture.trace,before,'status stays offline');
  failing=false;
  const held=deferred(),entered=deferred();
  fixture.s.client.models=async()=>{entered.resolve();await held.promise;return originalModels();};
  const pending=registry.refresh({providers:['possums'],allowNetwork:true});await entered.promise;
  await extension.commands.get('possums-status').handler('',ctx);
  assert.equal(notices.at(-1),notices[0],'verified channel and login alone do not clear the failure');
  held.resolve();await pending;
  await extension.commands.get('possums-status').handler('',ctx);
  assert.equal(notices.at(-1),m.approvalSummary());assert.equal(extension.provider.getModels().length,1);
  // A full recovery permits a new warning of the same category.
  failing=true;fixture.s.client.models=injectedModels;
  const count=notices.length;
  await assert.rejects(extension.provider.auth.apiKey.login(interaction()),error=>assertConnectionFailure(error,code));
  assert.equal(notices.length,count+1);assert.equal(notices.at(-1),notices[0]);
  assert.equal(fixture.s.sends(),0);
  assert(!notices.join('').includes(hostileConnection));assert(!notices.join('').includes(recoveryKey));
 }
});
async function catalogFixture() {
 const key=new Uint8Array([0,0,32,...Array(32).fill(7),0,4,0,1,0,2]);
 const channel=await m.Channel.fixture('https://localhost:18443',key,'07'.repeat(32));
 // Only catalog GETs exercise fetch; session setup is synthetic and never sends a key.
 channel.challenge=async()=>({challenge:'c'.repeat(43),expires_in:600});
 channel.control=async()=>({token:'b'.repeat(43),token_type:'Bearer',expires_in:43200});
 return new m.ReferenceClient(channel);
}
await check('prepared invocation freezes hooks and payload across setup and verified-client replacement',async()=>{
 const client=await catalogFixture(), second=await catalogFixture();
 second.channel.control=async()=>({token:'z'.repeat(43),token_type:'Bearer',expires_in:43200});
 await client.login(recoveryKey);await second.login(envKey);
 const messages=[{role:'user',content:'Synthetic input'}], calls=[], deltas=[];
 let hooks=0, first=true;
 client.channel.models=async()=>{calls.push('catalog1');if(first){first=false;throw new m.CatalogFailure('http','rejected',401,{error:{code:'unauthorized'}});}return {object:'list',data:[entry()]};};
 second.channel.models=async bearer=>{assert.equal(bearer,'z'.repeat(43));calls.push('catalog2');return {object:'list',data:[entry()]};};
 second.channel.control=async (path,_payload,bearer)=>{assert.equal(bearer,'z'.repeat(43));calls.push(path);return {submission:'s'.repeat(43)};};
 second.channel.chat=async (payload,bearer,options)=>{
  assert.equal(bearer,'z'.repeat(43));calls.push('chat');assert.equal(payload.messages[0].content,'Frozen synthetic');
  await options.onResponse({status:200,contentType:'text/event-stream'});
  const frame=value=>'data: '+JSON.stringify(value)+'\n\n';
  const text=frame(event({role:'assistant'}))+frame(event({content:'safe'}))+
   frame({object:'chat.completion.chunk',model:'synthetic',choices:[{index:0,delta:{},finish_reason:'stop'}]})+
   frame({object:'chat.completion.chunk',model:'synthetic',choices:[],usage:{prompt_tokens:1,completion_tokens:1,total_tokens:2},possums:{outcome:'settled',charged_microunits:'2',refunded_microunits:'50'}})+'data: [DONE]\n\n';
  return new ReadableStream({start(controller){controller.enqueue(new TextEncoder().encode(text));controller.close();}});
 };
 const options={onPayload:()=>{hooks++;return {model:'synthetic',stream:true,messages:[{role:'user',content:'Frozen synthetic'}]};}};
 const prepared=await client.prepareChat('synthetic',messages,text=>deltas.push(text),false,options);
 messages[0].content='mutated';options.onPayload=()=>{throw Error(hostileConnection);};
 assert(Object.isFrozen(prepared));assert.equal(prepared.dispatched,false);
 await assert.rejects(client.chatPrepared(prepared),error=>error instanceof m.CatalogFailure && error.stage==='http' && error.reason==='unauthorized');
 assert.equal(prepared.dispatched,false);
 assert.equal((await second.chatPrepared(prepared)).finish,'stop');
 assert.deepEqual(calls,['catalog1','catalog2','/v1/submissions','chat']);assert.deepEqual(deltas,['safe']);assert.equal(hooks,1);
 assert.equal(prepared.dispatched,true);
 await assert.rejects(second.chatPrepared(prepared),error=>error instanceof m.DiagnosticFailure && error.stage==='request');
 assert.equal(calls.length,4);
 assert.equal((await second.chat('synthetic',[{role:'user',content:'Frozen synthetic'}],()=>{},false,{onPayload:()=>{hooks++;}})).finish,'stop');
 assert.equal(hooks,2);assert.deepEqual(calls.slice(4),['catalog2','/v1/submissions','chat']);
});
await check('ordinary chat retains original bearer when payload hook switches or overlaps login',async()=>{
 for(const switchInsideHook of [true,false]){
  const client=await catalogFixture(),trace=[],entered=deferred(),release=deferred();let sessions=0;
  client.channel.control=async (path,_payload,bearer)=>{
   if(path==='/v1/sessions')return {token:(++sessions===1?'a':'z').repeat(43),token_type:'Bearer',expires_in:43200};
   trace.push(['submission',bearer]);return {submission:'s'.repeat(43)};
  };
  await client.login(recoveryKey);
  client.channel.models=async bearer=>{trace.push(['catalog',bearer]);return {object:'list',data:[entry()]};};
  client.channel.chat=async (_payload,bearer)=>{
   trace.push(['chat',bearer]);
   const frame=value=>'data: '+JSON.stringify(value)+'\n\n';
   const text=frame(event({role:'assistant'}))+frame({object:'chat.completion.chunk',model:'synthetic',choices:[{index:0,delta:{},finish_reason:'stop'}]})+
    frame({object:'chat.completion.chunk',model:'synthetic',choices:[],usage:{prompt_tokens:1,completion_tokens:1,total_tokens:2},
     possums:{outcome:'settled',charged_microunits:'2',refunded_microunits:'50'}})+'data: [DONE]\n\n';
   return new ReadableStream({start(controller){controller.enqueue(new TextEncoder().encode(text));controller.close();}});
  };
  const pending=client.chat('synthetic',[{role:'user',content:'Synthetic input'}],()=>{},false,{onPayload:async()=>{
   if(switchInsideHook)await client.login(envKey);
   else {entered.resolve();await release.promise;}
  }});
  if(!switchInsideHook){await entered.promise;assert.deepEqual(trace,[]);await client.login(envKey);release.resolve();}
  try {
   assert.equal((await pending).finish,'stop');assert.equal(sessions,2);
   assert.deepEqual(trace,[['catalog','a'.repeat(43)],['submission','a'.repeat(43)],['chat','a'.repeat(43)]]);
  }finally{release.resolve();}
 }
});
await check('prepared setup failures, abort and concurrent use never double dispatch',async()=>{
 const client=await catalogFixture();await client.login(recoveryKey);
 const invocation=[{role:'user',content:'Synthetic input'}];let calls=0, hold=deferred();
 client.channel.models=async()=>{calls++;await hold.promise;return {object:'list',data:[entry()]};};
 client.channel.control=async()=>{calls++;return {submission:'s'.repeat(43)};};
 client.channel.chat=()=>{calls++;throw Error(hostileConnection);};
 const prepared=await client.prepareChat('synthetic',invocation,()=>{});
 const first=client.chatPrepared(prepared);
 await assert.rejects(client.chatPrepared(prepared),error=>error instanceof m.DiagnosticFailure && error.stage==='request');
 hold.resolve();await assert.rejects(first,error=>error instanceof m.DiagnosticFailure && error.stage==='transport' && error.code==='uncertain' && !error.message.includes(hostileConnection));
 assert.equal(calls,3);assert.equal(prepared.dispatched,true);
 await assert.rejects(client.chatPrepared(prepared));assert.equal(calls,3);
 const abort=new AbortController();abort.abort();
 await assert.rejects(client.prepareChat('synthetic',invocation,()=>{},false,{signal:abort.signal}),error=>error instanceof m.DiagnosticFailure);
 const during= new AbortController();
 const blocked=await client.prepareChat('synthetic',invocation,()=>{},false,{signal:during.signal});during.abort();
 await assert.rejects(client.chatPrepared(blocked),error=>error instanceof m.DiagnosticFailure && error.constraint==='interrupted');
 assert.equal(blocked.dispatched,false);blocked.close();assert.equal(calls,3);
 for(const bad of [{get signal(){throw Error(hostileConnection);}}, {onPayload:()=>({model:'other',stream:true,messages:invocation})},
  {onPayload:()=>({model:'synthetic',stream:true,messages:invocation,headers:{secret:hostileConnection}})}])
  await assert.rejects(client.prepareChat('synthetic',invocation,()=>{},false,bad),error=>error instanceof m.DiagnosticFailure && !error.message.includes(hostileConnection));
 assert.equal(calls,3);
});
await check('abort during payload hook or catalog cannot dispatch and leaves no active operation',async()=>{
 const client=await catalogFixture();await client.login(recoveryKey);
 const invocation=[{role:'user',content:'Synthetic input'}],enter=deferred(),release=deferred(),abort=new AbortController();
 let calls=0;
 const pending=client.prepareChat('synthetic',invocation,()=>{},false,{signal:abort.signal,onPayload:async()=>{enter.resolve();await release.promise;return undefined;}});
 await enter.promise;abort.abort();
 await assert.rejects(pending,error=>error instanceof m.DiagnosticFailure && error.stage==='hook' && error.constraint==='interrupted');
 release.resolve();assert.equal(calls,0);
 const waiting=deferred(),inside=deferred(),during=new AbortController();
 client.channel.models=async()=>{calls++;inside.resolve();await waiting.promise;return {object:'list',data:[entry()]};};
 client.channel.control=async()=>{calls++;return {submission:'s'.repeat(43)};};
 client.channel.chat=()=>{calls++;throw Error(hostileConnection);};
 const prepared=await client.prepareChat('synthetic',invocation,()=>{},false,{signal:during.signal});
 const attempt=client.chatPrepared(prepared);await inside.promise;during.abort();waiting.resolve();
 await assert.rejects(attempt,error=>error instanceof m.DiagnosticFailure && error.constraint==='interrupted');
 assert.equal(calls,1);assert.equal(prepared.dispatched,false);prepared.close();
});
await check('prepared control uncertainty is not dispatch; post-dispatch failures preserve receipt evidence',async()=>{
 const client=await catalogFixture();await client.login(recoveryKey);
 let catalogs=0,controls=0,chats=0;
 client.channel.models=async()=>{catalogs++;return {object:'list',data:[entry()]};};
 client.channel.control=async()=>{controls++;if(controls===1)throw new m.DiagnosticFailure('submission','fetch','uncertain',503);return {submission:'s'.repeat(43)};};
 client.channel.chat=()=>{chats++;throw Error(hostileConnection);};
 const invocation=[{role:'user',content:'Synthetic input'}];const prepared=await client.prepareChat('synthetic',invocation,()=>{});
 await assert.rejects(client.chatPrepared(prepared),error=>error instanceof m.DiagnosticFailure && error.stage==='submission' && error.code==='uncertain' && error.status===503);
 assert.equal(prepared.dispatched,false);assert.equal(chats,0);
 await assert.rejects(client.chatPrepared(prepared),error=>error instanceof m.DiagnosticFailure && error.stage==='transport' && error.code==='uncertain');
 assert.equal(prepared.dispatched,true);assert.deepEqual([catalogs,controls,chats],[2,2,1]);
 const refunded=await terminalGatewayError('generation_failed','stream_usage_missing','refunded');
 client.channel.chat=async()=>{chats++;return new ReadableStream({start(controller){controller.enqueue(new TextEncoder().encode('data: '+JSON.stringify({error:{code:'generation_failed',detail:'stream_usage_missing',billing:'refunded',message:hostileConnection}})+'\n\n'));controller.close();}});};
 const again=await client.prepareChat('synthetic',invocation,()=>{});
 await assert.rejects(client.chatPrepared(again),error=>error instanceof m.GatewayError && error.billing==='refunded' && error.reason===refunded.reason && !error.message.includes(hostileConnection));
 assert.equal(again.dispatched,true);assert.equal(chats,2);
 client.channel.chat=async(_payload,_bearer,options)=>{chats++;await options.onResponse({status:200,contentType:'text/event-stream'});return new ReadableStream({start(controller){controller.close();}});};
 const response=await client.prepareChat('synthetic',invocation,()=>{},false,{onResponse:()=>{throw Error(hostileConnection);}});
 await assert.rejects(client.chatPrepared(response),error=>error instanceof m.DiagnosticFailure && error.stage==='hook' && error.code==='uncertain' && !error.message.includes(hostileConnection));
 assert.equal(response.dispatched,true);assert.equal(chats,3);
 const missing=await client.prepareChat('synthetic',invocation,()=>{});
 await assert.rejects(client.chatPrepared(missing),error=>error instanceof m.DiagnosticFailure && error.stage==='stream' && error.constraint==='finish_missing' && error.code==='uncertain');
 assert.equal(missing.dispatched,true);assert.equal(chats,4);
});
await check('quiescing HTTP 503 reaches catalog and challenge presentations only after exact bounded EOF',async()=>{
 const wire={error:{code:'service_quiescing',stage:'admission',constraint:'service_quiescing',billing:'not_submitted',message:hostileConnection}};
 const key=new Uint8Array([0,0,32,...Array(32).fill(7),0,4,0,1,0,2]);
 const channel=await m.Channel.fixture('https://localhost:18443',key,'07'.repeat(32));
 const originalFetch=globalThis.fetch;let calls=[];
 try {
  globalThis.fetch=async request=>{calls.push(new URL(request.url).pathname);return Response.json(wire,{status:503});};
  const client=new m.ReferenceClient(channel);
  await assert.rejects(client.login(recoveryKey),error=>{
   assert(error instanceof m.GatewayError);assert.equal(error.reason,'service_quiescing');assert.equal(error.status,503);
   assert.equal(error.billing,'unknown');assert(!error.message.includes(hostileConnection));
   const shown=m.connectionFailure(error);assertConnectionFailure(shown,'service_quiescing');
   assert.match(shown.message,/Stage: admission; constraint: service_quiescing.*Observed HTTP status: 503/);
   return true;
  });
  assert.deepEqual(calls,['/v1/auth/challenge']);
  const catalog=await catalogFixture();await catalog.login(recoveryKey);calls=[];
  await assert.rejects(catalog.models(),error=>{
   assert(error instanceof m.CatalogFailure);assert.equal(error.reason,'service_quiescing');assert.equal(error.status,503);
   const shown=m.catalogConnectionFailure(error);assertConnectionFailure(shown,'service_quiescing');
   assert.match(shown.message,/Stage: catalog; constraint: http.*Gateway stage: admission; constraint: service_quiescing/);
   assert.match(shown.message,/Observed HTTP status: 503/);assert(!shown.message.includes('refunded'));
   return true;
  });
  assert.deepEqual(calls,['/v1/models']);
  for(const response of [
   new Response(new ReadableStream({start(controller){controller.enqueue(new TextEncoder().encode(JSON.stringify(wire)));controller.error(new Error(hostileConnection));}}),{status:503}),
   Response.json({error:{...wire.error,stage:'transport'}},{status:503}),
   Response.json(wire,{status:502}),
   Response.json({error:{...wire.error,extra:hostileConnection}},{status:503}),
  ]) {
   globalThis.fetch=async request=>{calls.push(new URL(request.url).pathname);return response;};
   await assert.rejects(catalog.models(),error=>error instanceof m.CatalogFailure && error.reason===undefined && !error.message.includes(hostileConnection));
  }
  assert.equal(calls.length,5,'one catalog GET per deliberate attempt, no diagnostic replay');
  globalThis.fetch=async request=>{calls.push(new URL(request.url).pathname);return Response.json(wire,{status:503});};
  // Plaintext HTTP bodies cannot be promoted through the encrypted EHBP boundary.
  calls=[];
  await assert.rejects(channel.control('/v1/sessions',{challenge:'c'.repeat(43),credential:'c'.repeat(43)}),error=>
   error instanceof m.DiagnosticFailure && error.constraint==='endpoint_binding' && error.status===503);
  await assert.rejects(channel.balance('b'.repeat(43)),error=>
   error instanceof m.DiagnosticFailure && error.constraint==='endpoint_binding' && error.status===503);
  await assert.rejects(channel.chat({model:'synthetic',stream:true,submission:'s'.repeat(43),messages:[{role:'user',content:'synthetic'}]},'b'.repeat(43)),error=>
   error instanceof m.DiagnosticFailure && error.constraint==='endpoint_binding' && error.status===503);
  assert.deepEqual(calls,['/v1/sessions','/v1/balance','/v1/chat/completions']);
 } finally {globalThis.fetch=originalFetch;}
});
await check('catalog diagnostics preserve bounded HTTP rejection details without hostile text or billing claims',async()=>{
 const originalFetch=globalThis.fetch,client=await catalogFixture();let calls=0;
 await client.login(recoveryKey);
 try {
  for(const [status,body,reason,detail] of [
   [401,JSON.stringify({error:{code:'unauthorized',message:hostileConnection,billing:'refunded'}}),'unauthorized',undefined],
   [429,JSON.stringify({error:{code:'account_limit',message:hostileConnection}}),'account_limit',undefined],
   [503,JSON.stringify({error:{code:'unavailable',detail:'catalog_failed',message:hostileConnection,billing:'unknown'}}),'unavailable','catalog_failed'],
   [503,JSON.stringify({error:{code:'unavailable',detail:'verification_failed',message:hostileConnection}}),'unavailable','verification_failed'],
   [503,JSON.stringify({error:{code:'unavailable',detail:hostileConnection}}),undefined,undefined],
   [503,'not JSON '+hostileConnection,undefined,undefined],
   [503,'x'.repeat(4097),undefined,undefined],
   [503,new ReadableStream({start(controller){controller.error(new Error(hostileConnection));}}),undefined,undefined],
   [302,hostileConnection,undefined,undefined],
  ]) {
   globalThis.fetch=async request=>{
    assert.equal(request.url,'https://localhost:18443/v1/models');assert.equal(request.method,'GET');
    assert.equal(request.headers.get('authorization'),'Bearer '+'b'.repeat(43));calls++;
    return new Response(body,{status,headers:{'content-type':'application/json'}});
   };
   await assert.rejects(client.models(),error=>{
    assert(error instanceof m.CatalogFailure);assert(Object.isFrozen(error));assert.equal(error.stage,'http');
    assert.equal(error.status,status);assert.equal(error.reason,reason);assert.equal(error.detail,detail);
    assert.equal(error.cause,undefined);assert.equal(error.billing,undefined);assert(!error.message.includes(hostileConnection));
    const failure=m.catalogConnectionFailure(error);assertConnectionFailure(failure,'catalog_http_rejected');
    assert.match(failure.message,new RegExp('Observed HTTP status: '+status));
    if(reason)assert(failure.message.includes('Gateway code: '+reason));
    if(detail)assert(failure.message.includes('['+detail+']'));
    assert(!failure.message.includes('refunded'));assert(!failure.message.includes('Charge unknown'));
    return true;
   });
  }
  assert.equal(calls,9,'one GET per deliberate catalog request; no replay');
 } finally {globalThis.fetch=originalFetch;}
});
await check('catalog request, body and validation stages remain distinct and challenge behavior is unchanged',async()=>{
 const originalFetch=globalThis.fetch,client=await catalogFixture();await client.login(recoveryKey);let calls=0;
 try {
  for(const [stage,response] of [
   ['request',()=>{throw new Error(hostileConnection);}],
   ['body',()=>new Response('not JSON '+hostileConnection)],
   ['body',()=>new Response('x'.repeat(256*1024+1))],
   ['body',()=>new Response(new ReadableStream({start(controller){controller.error(new Error(hostileConnection));}}))],
   ['validation',()=>Response.json({object:'list',data:[]})],
   ['validation',()=>Response.json({object:'list',data:[{...entry(true),maximum_reservation_microunits:'1'}]})],
  ]) {
   globalThis.fetch=async()=>{calls++;return response();};
   await assert.rejects(client.models(),error=>{
    assert(error instanceof m.CatalogFailure);assert.equal(error.stage,stage);assert.equal(error.status,undefined);
    assertConnectionFailure(m.catalogConnectionFailure(error),stage==='request'?'catalog_request_failed':stage==='body'?'catalog_body_invalid':'catalog_validation_failed');
    return true;
   });
  }
  const key=new Uint8Array([0,0,32,...Array(32).fill(7),0,4,0,1,0,2]);
  const channel=await m.Channel.fixture('https://localhost:18443',key,'07'.repeat(32));
  globalThis.fetch=async()=>{calls++;return Response.json({error:{code:'unavailable',message:hostileConnection}},{status:503});};
  await assert.rejects(channel.challenge(),error=>error instanceof m.ChannelError && !(error instanceof m.CatalogFailure));
  assert.equal(calls,7);
  assert.equal(m.catalogConnectionFailure(new Error('catalog_http_rejected '+hostileConnection)).code,'catalog_unavailable');
 } finally {globalThis.fetch=originalFetch;}
});
await check('native catalog HTTP rejection reaches transient status with observed safe gateway detail and no inference',async()=>{
 const originalFetch=globalThis.fetch,client=await catalogFixture();let calls=0;
 const extension=fixtureExtension(async()=>client),credentials=new ai.InMemoryCredentialStore();
 await credentials.modify('possums',async()=>({type:'api_key',key:recoveryKey}));
 const runtime=await nativeRuntime(extension.provider,credentials),registry=new coding.ModelRegistry(runtime),notices=[];
 const ctx={hasUI:true,ui:{notify:text=>notices.push(text)},modelRegistry:registry};
 try {
  globalThis.fetch=async request=>{
   assert.equal(request.method,'GET');assert.equal(request.url,'https://localhost:18443/v1/models');calls++;
   return Response.json({error:{code:'unavailable',detail:'catalog_failed',message:hostileConnection,billing:'refunded'}},{status:503});
  };
  await extension.handlers.get('session_start')({},ctx);await registry.refresh({providers:['possums'],allowNetwork:true});
  assert.equal(calls,2);assert.equal(notices.length,1,'same-category failures remain deduplicated');
  assert.match(notices[0],/possums_catalog_http_rejected/);assert.match(notices[0],/Observed HTTP status: 503/);
  assert.match(notices[0],/Gateway code: unavailable/);assert.match(notices[0],/\[catalog_failed\]/);
  assert(!notices[0].includes(hostileConnection));assert(!notices[0].includes('refunded'));assert.deepEqual(extension.provider.getModels(),[]);
  await extension.commands.get('possums-status').handler('',ctx);assert.equal(notices.at(-1),notices[0]);assert.equal(calls,2);
  globalThis.fetch=async request=>{assert.equal(request.method,'GET');calls++;return Response.json({object:'list',data:[entry(true)]});};
  await registry.refresh({providers:['possums'],allowNetwork:true});await extension.commands.get('possums-status').handler('',ctx);
  assert.equal(notices.at(-1),m.approvalSummary());assert.equal(extension.provider.getModels().length,1);
 } finally {globalThis.fetch=originalFetch;}
});
await check('native challenge quiescing warning preserves admission without catalog or inference',async()=>{
 const originalFetch=globalThis.fetch,key=new Uint8Array([0,0,32,...Array(32).fill(7),0,4,0,1,0,2]);
 const channel=await m.Channel.fixture('https://localhost:18443',key,'07'.repeat(32));
 const extension=fixtureExtension(async()=>new m.ReferenceClient(channel));extension.provider.newSession();
 const routes=[];
 try {
  globalThis.fetch=async request=>{routes.push(new URL(request.url).pathname);return Response.json({error:{code:'service_quiescing',stage:'admission',constraint:'service_quiescing',billing:'not_submitted',message:hostileConnection}},{status:503});};
  await assert.rejects(extension.provider.auth.apiKey.login(interaction()),error=>{
   assertConnectionFailure(error,'service_quiescing');assert.match(error.message,/Stage: admission; constraint: service_quiescing.*Observed HTTP status: 503/);return true;
  });
  assert.deepEqual(routes,['/v1/auth/challenge']);
  assert.deepEqual(extension.provider.getModels(),[]);
 } finally {globalThis.fetch=originalFetch;}
});
await check('native quiescing catalog warning is offline on status and sends no inference',async()=>{
 const originalFetch=globalThis.fetch,client=await catalogFixture(),routes=[];
 const extension=fixtureExtension(async()=>client),credentials=new ai.InMemoryCredentialStore();
 await credentials.modify('possums',async()=>({type:'api_key',key:recoveryKey}));
 const runtime=await nativeRuntime(extension.provider,credentials),registry=new coding.ModelRegistry(runtime),notices=[];
 const ctx={hasUI:true,ui:{notify:text=>notices.push(text)},modelRegistry:registry};
 try {
  globalThis.fetch=async request=>{
   routes.push(new URL(request.url).pathname);
   return Response.json({error:{code:'service_quiescing',stage:'admission',constraint:'service_quiescing',billing:'not_submitted',message:hostileConnection}},{status:503});
  };
  await extension.handlers.get('session_start')({},ctx);
  assert.equal(notices.length,1);assert.match(notices[0],/possums_service_quiescing/);
  assert.match(notices[0],/Gateway stage: admission; constraint: service_quiescing/);
  assert(!notices[0].includes(hostileConnection));assert(!notices[0].includes('refunded'));
  const before=routes.length;
  await extension.commands.get('possums-status').handler('',ctx);
  assert.equal(routes.length,before);assert(routes.every(route=>route==='/v1/models'));
  assert.deepEqual(extension.provider.getModels(),[]);
 } finally {globalThis.fetch=originalFetch;}
});
await check('native later catalog failures clear usable models, deduplicate, and clear status on accepted recovery',async()=>{
 const fixture=await authFixture(),extension=fixtureExtension(fixture.establish);
 const credentials=new ai.InMemoryCredentialStore();
 await credentials.modify('possums',async()=>({type:'api_key',key:recoveryKey}));
 const runtime=await nativeRuntime(extension.provider,credentials),registry=new coding.ModelRegistry(runtime),notices=[];
 const ctx={hasUI:true,ui:{notify:text=>notices.push(text)},modelRegistry:registry};
 await extension.handlers.get('session_start')({},ctx);
 assert.equal(extension.provider.getModels().length,1);assert.deepEqual(notices,[]);
 const original=fixture.s.client.models;
 fixture.s.client.models=async()=>{throw new Error(hostileConnection);};
 for(let i=0;i<2;i++)await registry.refresh({providers:['possums'],allowNetwork:true});
 assert.deepEqual(extension.provider.catalog,[]);assert.match(extension.provider.getModels()[0].name,/last-known/);assert.equal(notices.length,1);
 assert.match(notices[0],/possums_catalog_unavailable/);
 await extension.commands.get('possums-status').handler('',ctx);assert.equal(notices.at(-1),notices[0]);
 fixture.s.client.models=original;
 await registry.refresh({providers:['possums'],allowNetwork:true});
 await extension.commands.get('possums-status').handler('',ctx);assert.equal(notices.at(-1),m.approvalSummary());
 assert.equal(extension.provider.getModels().length,1);
 fixture.s.client.models=async()=>{throw new Error(hostileConnection);};
 const count=notices.length;
 await registry.refresh({providers:['possums'],allowNetwork:true});assert.equal(notices.length,count+1);
 assert(!notices.join('').includes(hostileConnection));assert.equal(fixture.s.sends(),0);
});
await check('cancelled, logged-out or replaced session/catalog failures never publish stale authentication diagnostics',async()=>{
 for(const phase of ['login','models'])for(const action of ['cancel','logout','replace']) {
  const fixture=await authFixture(),held=deferred(),entered=deferred(),notices=[],controller=new AbortController();let calls=0;
  const provider=new m.PossumsProvider(async()=>{
   const candidate=await fixture.establish();
   const original=candidate[phase];
   candidate[phase]=async(...args)=>{if(calls++===0){entered.resolve();await held.promise;throw new Error(hostileConnection);}return original(...args);};
   return candidate;
  },failure=>notices.push(failure));
  const pending=assert.rejects(provider.auth.apiKey.resolve(authInput({type:'api_key',key:recoveryKey},undefined,controller.signal)));
  await entered.promise;
  if(action==='cancel')controller.abort();else if(action==='logout')provider.logout();
  else await provider.auth.apiKey.resolve(authInput({type:'api_key',key:envKey}));
  const before=[...notices];held.resolve();await pending;
  assert.deepEqual(notices,before);assert.deepEqual(notices,action==='replace'?[undefined]:[]);
  assert.equal(provider.getModels().length,action==='replace'?1:0);assert.equal(fixture.s.sends(),0);
 }
});
await check('catalog diagnostics publish only with accepted current updates, including late success',async()=>{
 for(const fails of [false,true])for(const action of ['reject','cancel','logout','replace']) {
  const fixture=await authFixture(),notices=[],provider=new m.PossumsProvider(fixture.establish,failure=>notices.push(failure));
  await provider.auth.apiKey.login(interaction());notices.length=0;
  const held=deferred(),entered=deferred(),controller=new AbortController(),original=fixture.s.client.models;
  fixture.s.client.models=async()=>{entered.resolve();await held.promise;if(fails)throw new Error(hostileConnection);return [];};
  const pending=provider.refreshModels({credential:{type:'api_key',key:requestMarker},allowNetwork:true,signal:controller.signal,
   publish:async value=>{if(action==='reject')return false;value.update?.();return true;}});
  const settled=fails?assert.rejects(pending,error=>assertConnectionFailure(error,'catalog_unavailable')):pending;
  await entered.promise;
  if(action==='cancel')controller.abort();else if(action==='logout')provider.logout();
  else if(action==='replace'){fixture.s.client.models=original;await provider.auth.apiKey.login(interaction(envKey));}
  const before=[...notices];held.resolve();await settled;
  assert.deepEqual(notices,before);assert.equal(provider.catalog.length,action==='logout'?0:1);
  assert.equal(fixture.s.sends(),0);
 }
});
await check('throwing connection callbacks and native extension UI never change authentication or catalog outcomes',async()=>{
 for(const phase of ['login','models']) {
  const fixture=await authFixture();let failing=true;
  const establish=async()=>{const candidate=await fixture.establish(),original=candidate[phase];
   candidate[phase]=async(...args)=>{if(failing)throw new Error(hostileConnection);return original(...args);};return candidate;
  };
  const provider=new m.PossumsProvider(establish,()=>{throw new Error(hostileConnection);});
  const code=phase==='login'?'session_unavailable':'catalog_unavailable';
  await assert.rejects(provider.auth.apiKey.resolve(authInput({type:'api_key',key:recoveryKey})),error=>assertConnectionFailure(error,code));
  assert.deepEqual(provider.getModels(),[]);failing=false;
  await provider.auth.apiKey.login(interaction());assert.equal(provider.getModels().length,1);
  const refresh=()=>provider.refreshModels({credential:{type:'api_key',key:requestMarker},allowNetwork:true,signal:new AbortController().signal,publish:async value=>{value.update?.();return true;}});
  const original=fixture.s.client.models;fixture.s.client.models=async()=>{throw new Error(hostileConnection);};
  await assert.rejects(refresh(),error=>assertConnectionFailure(error,'catalog_unavailable'));assert.deepEqual(provider.catalog,[]);assert.deepEqual(provider.listed,[]);
  fixture.s.client.models=original;await refresh();assert.equal(provider.getModels().length,1);
  const extension=fixtureExtension(establish),credentials=new ai.InMemoryCredentialStore();
  await credentials.modify('possums',async()=>({type:'api_key',key:recoveryKey}));
  const runtime=await nativeRuntime(extension.provider,credentials),registry=new coding.ModelRegistry(runtime);
  const ctx={hasUI:true,ui:{notify:()=>{throw new Error(hostileConnection);}},modelRegistry:registry};
  failing=true;await extension.handlers.get('session_start')({},ctx);assert.deepEqual(extension.provider.getModels(),[]);
  failing=false;await registry.refresh({providers:['possums'],allowNetwork:true});assert.equal(extension.provider.getModels().length,1);
  fixture.s.client.models=async()=>{throw new Error(hostileConnection);};
  await registry.refresh({providers:['possums'],allowNetwork:true});assert.deepEqual(extension.provider.catalog,[]);assert.deepEqual(extension.provider.listed,[]);
  await extension.commands.get('possums-status').handler('',ctx);
  fixture.s.client.models=original;await registry.refresh({providers:['possums'],allowNetwork:true});assert.equal(extension.provider.getModels().length,1);
  assert.equal(fixture.s.sends(),0);
 }
});
await check('cancelled or obsolete verification failures never publish a stale diagnostic',async()=>{
 for(const action of ['cancel','logout']) {
  const held=deferred(),entered=deferred(),notices=[],controller=new AbortController();
  const provider=new m.PossumsProvider(async()=>{entered.resolve();await held.promise;throw new Error('PRIVATE_ERROR');},failure=>notices.push(failure));
  const pending=assert.rejects(provider.auth.apiKey.resolve(authInput({type:'api_key',key:recoveryKey},undefined,controller.signal)));
  await entered.promise;
  if(action==='cancel')controller.abort();else provider.logout();
  held.resolve();await pending;assert.deepEqual(notices,[]);
 }
});
await check('failed channel verification prevents both interactive prompt and saved/env credential transmission',async()=>{
 let prompts=0,sends=0;
 const provider=new m.PossumsProvider(async()=>({channel:{},login:async()=>{sends++;}}));
 await assert.rejects(provider.auth.apiKey.login({...interaction(),prompt:async()=>{prompts++;return recoveryKey;}}));
 for(const input of [authInput({type:'api_key',key:recoveryKey}),authInput(undefined,envKey)]) {
  await assert.rejects(provider.auth.apiKey.resolve(input));
 }
 assert.equal(prompts,0);assert.equal(sends,0);assert.deepEqual(provider.getModels(),[]);
});
await check('cancelled and replaced authentication cannot send late secrets or restore stale state',async()=>{
 for(const action of ['cancel','logout','replace']) {
  const fixture=await authFixture();const held=deferred(),entered=deferred();let calls=0;
  const provider=new m.PossumsProvider(async()=>{const candidate=await fixture.establish();if(calls++===0){entered.resolve();await held.promise;}return candidate;});
  const controller=new AbortController();
  const pending=provider.auth.apiKey.resolve(authInput({type:'api_key',key:recoveryKey},undefined,controller.signal));
  const rejected=assert.rejects(pending);await entered.promise;
  if(action==='cancel')controller.abort();
  else if(action==='logout')provider.logout();
  else {
   const replacement=provider.auth.apiKey.resolve(authInput({type:'api_key',key:envKey}));
   held.resolve();await replacement;
  }
  held.resolve();await rejected;
  assert.deepEqual(fixture.trace.keys,action==='replace'?[envKey]:[]);
  assert.equal(provider.getModels().length,action==='replace'?1:0);
 }
 for(const phase of ['prompt','login','catalog']) {
  const fixture=await authFixture();const held=deferred(),entered=deferred();
  const provider=new m.PossumsProvider(async()=>{const candidate=await fixture.establish();
   if(phase!=='prompt'){const name=phase==='login'?'login':'models';const original=candidate[name];candidate[name]=async(...args)=>{const value=await original(...args);entered.resolve();await held.promise;return value;};}
   return candidate;
  });
  const controller=new AbortController();
  const pending=provider.auth.apiKey.login({...interaction(recoveryKey,controller.signal),prompt:async()=>{if(phase==='prompt'){entered.resolve();await held.promise;}return recoveryKey;}});
  const rejected=assert.rejects(pending);await entered.promise;controller.abort();held.resolve();await rejected;
  assert.deepEqual(provider.getModels(),[]);assert.equal(fixture.trace.keys.length,phase==='prompt'?0:1);
  assert.equal(fixture.trace.models,phase==='catalog'?1:0);
 }
});
await check('native SDK cancelled login never persists or restores a late candidate',async()=>{
 const fixture=await authFixture();const held=deferred(),entered=deferred();
 const provider=new m.PossumsProvider(async()=>{const candidate=await fixture.establish();
  const models=candidate.models;candidate.models=async()=>{const entries=await models();entered.resolve();await held.promise;return entries;};
  return candidate;
 });
 const authPath=path.join(root,'cancelled-login','auth.json');
 const runtime=await nativeRuntime(provider,NativeAuthStorage.create(authPath));
 const controller=new AbortController();
 const pending=runtime.login('possums','api_key',interaction(recoveryKey,controller.signal));
 const rejected=assert.rejects(pending);await entered.promise;controller.abort();held.resolve();await rejected;
 await new Promise(setImmediate);
 assert.equal(await NativeAuthStorage.create(authPath).read('possums'),undefined);
 assert.deepEqual(provider.getModels(),[]);assert.equal(fixture.s.sends(),0);
});
await check('native logout/replacement revoke pending tools but retain settled receipt charges',async()=>{
 for(const action of ['logout','replace','abort']) {
  const fixture=await authFixture(['held_tools']);const provider=new m.PossumsProvider(fixture.establish);
  const authPath=path.join(root,'pending-'+action,'auth.json');
  const runtime=await nativeRuntime(provider,NativeAuthStorage.create(authPath));
  await runtime.login('possums','api_key',interaction());provider.beginRun();
  const controller=new AbortController();const stream=runtime.streamSimple(provider.getModels()[0],context(true),{signal:controller.signal});
  let changed=false,ends=0;
  for await(const event of stream){
   if(event.type==='toolcall_delta'&&!changed){changed=true;
    if(action==='logout')await runtime.logout('possums');
    else if(action==='replace')await runtime.login('possums','api_key',interaction(envKey));
    else controller.abort();
    fixture.s.release();
   }
   if(event.type==='toolcall_end')ends++;
  }
  const result=await stream.result();assert(changed);assert.equal(ends,0);assert.notEqual(result.stopReason,'toolUse');
  assert.equal(result.usage.cost.total,7/1e6);assert.equal(result.diagnostics[0].details.finish,'tool_calls');
  assert.equal(fixture.s.sends(),1);
  assert.equal((await NativeAuthStorage.create(authPath).read('possums'))?.key,action==='logout'?undefined:action==='replace'?envKey:recoveryKey);
 }
});
await check('late catalog success/failure cannot replace a newer authenticated account',async()=>{
 for(const fails of [false,true]) {
  const fixture=await authFixture(),notices=[];const provider=new m.PossumsProvider(fixture.establish,failure=>notices.push(failure));
  const runtime=await nativeRuntime(provider,new ai.InMemoryCredentialStore());
  await runtime.login('possums','api_key',interaction());
  const held=deferred(),entered=deferred();const original=fixture.s.client.models;
  fixture.s.client.models=async()=>{entered.resolve();await held.promise;if(fails)throw new Error('synthetic failure');return [];};
  const pending=runtime.refresh({providers:['possums'],allowNetwork:true});await entered.promise;
  fixture.s.client.models=original;
  await runtime.login('possums','api_key',interaction(envKey));const before=[...notices];held.resolve();await pending;
  await new Promise(setImmediate); // Let the aborted provider operation reach its late publication.
  assert.deepEqual(notices,before,'native rejected publication cannot report or clear a diagnostic');
  assert.equal(provider.getModels().length,1);assert.equal((await runtime.getAuth('possums')).auth.apiKey,requestMarker);
 }
});

await check('native recovery credential; request auth stays non-secret',async()=>{
 const s=await setup(['stop']);
 assert.equal(s.credential.key,recoveryKey);
 assert.equal((await s.provider.auth.apiKey.resolve(authInput(s.credential))).auth.apiKey,requestMarker);
 assert.equal(await s.provider.auth.apiKey.resolve(authInput(undefined)),undefined);
 s.provider.logout();assert.equal(s.provider.catalog.length,0);assert.equal(s.provider.listed.length,0);
 assert.equal(await s.provider.auth.apiKey.resolve(authInput(undefined)),undefined);
});
await check('progress before receipt, hooks and submitted-rate cost',async()=>{
 const s=await setup(['held']);s.provider.beginRun();let payload=0,response=0,raw=0;
 const stream=s.provider.streamSimple(s.selected,context(false),{onPayload:()=>{payload++;},onResponse:()=>{response++;},onProviderStreamEvent:()=>{raw++;}});
 const events=[];let observed=false;
 for await(const event of stream){events.push(event);if(event.type==='text_delta'){observed=true;assert.equal(events.some(x=>x.type==='done'),false);s.release();}}
 const result=await stream.result();assert(observed);assert.equal(payload,1);assert.equal(response,1);assert.equal(raw,2);
 assert.equal(result.usage.totalTokens,5);assert.equal(result.usage.cost.total,7/1e6);assert.equal(result.usage.cost.input,2/1e6);
 assert(Math.abs(result.usage.cost.input+result.usage.cost.output-result.usage.cost.total)<1e-15);
});
await check('provider leaves subsequent invocation policy to native Pi after uncertain or length failure',async()=>{
 for(const mode of ['uncertain','length']){const s=await setup([mode,'stop']);s.provider.beginRun();const first=await drain(s.provider.streamSimple(s.selected,context(false)));
 const second=await drain(s.provider.streamSimple(s.selected,context(false)));assert.equal(s.sends(),2);assert.equal(second.message.stopReason,'stop');
 if(mode==='uncertain')assert.match(first.message.errorMessage,/charge unknown/);else {assert.equal(first.message.stopReason,'error');assert.equal(first.message.diagnostics[0].details.finish,'length');assert.equal(first.message.usage.cost.total,7/1e6);}}
});
await check('abort propagated without replay',async()=>{
 const s=await setup(['abort']);s.provider.beginRun();const controller=new AbortController();const stream=s.provider.streamSimple(s.selected,context(false),{signal:controller.signal});
 for await(const event of stream){if(event.type==='text_delta')controller.abort();}
 assert(s.abortSeen());assert.equal((await stream.result()).stopReason,'aborted');assert.equal(s.sends(),1);
});
await check('unqualified tools and image history rejected before invocation',async()=>{
 const s=await setup(['stop'],false);s.provider.beginRun();assert.equal((await drain(s.provider.streamSimple(s.selected,context(true)))).message.stopReason,'error');assert.equal(s.sends(),0);
 s.provider.beginRun();const image=ai.normalizeContext({messages:[{role:'user',timestamp:1,content:[{type:'image',data:'synthetic',mimeType:'image/png'}]}]});
 assert.match((await drain(s.provider.streamSimple(s.selected,image))).message.errorMessage,/\[possums_images_unsupported\].*remove images/);assert.equal(s.sends(),0);
});
await check('tool-only parallel history uses empty text without altering calls or results',async()=>{
 const s=await setup([]);
 const calls=['one','two'].map(value=>({type:'toolCall',id:'call_'+value,name:'echo',arguments:{value}}));
 const assistant={role:'assistant',provider:'possums',api:'openai-completions',model:s.selected.id,timestamp:2,stopReason:'toolUse',content:calls,
  usage:{input:0,output:0,cacheRead:0,cacheWrite:0,totalTokens:0,cost:{input:0,output:0,cacheRead:0,cacheWrite:0,total:0}}};
 const results=calls.map(call=>({role:'toolResult',toolCallId:call.id,toolName:'echo',content:[{type:'text',text:'Synthetic result'}],isError:false,timestamp:3}));
 const transcript=ai.normalizeContext({messages:[user,assistant,...results],tools:[tool]});
 const payload=m.snapshotInvocation(m.invocation(s.selected,transcript));
 assert.equal(payload.messages.length,4);assert.equal(payload.messages[1].role,'assistant');assert.equal(payload.messages[1].content,'');
 assert.deepEqual(payload.messages[1].tool_calls.map(call=>({id:call.id,name:call.function.name,arguments:JSON.parse(call.function.arguments)})),calls.map(call=>({id:call.id,name:call.name,arguments:call.arguments})));
 assert.deepEqual(payload.messages.slice(2).map(message=>({id:message.tool_call_id,content:message.content})),calls.map(call=>({id:call.id,content:'Synthetic result'})));
 assistant.content=[{type:'text',text:'Synthetic preamble'},...calls];
 const withText=m.snapshotInvocation(m.invocation(s.selected,transcript));assert.equal(withText.messages[1].content,'Synthetic preamble');
});
await check('actual pinned Pi SDK continues beyond 64 historical calls',async()=>{
 const {session,s,toolRuns}=await sdkSetup('sdk-long-history',[...Array(66).fill('tool_calls'),'stop'],true);
 const chat=s.client.chat;
 s.client.chat=async(model,messages,onDelta,newConversation,options)=>{
  m.snapshotInvocation({model,stream:true,messages,tools:options.tools});
  const id='prior_'+s.sends();
  return chat(model,messages,onDelta,newConversation,{...options,onEvent:async value=>{
   const copy=structuredClone(value);
   for(const choice of copy.choices??[])for(const call of choice.delta?.tool_calls??[])if(call.id)call.id=id;
   await options.onEvent(copy);
  }});
 };
 try{await session.prompt('Synthetic long tool task');assert.equal(toolRuns(),66);assert.equal(s.sends(),67);
  assert.equal(s.requests.at(-1).messages.filter(message=>message.role==='tool').length,66);
  assert.equal(session.messages.filter(message=>message.role==='assistant').at(-1).stopReason,'stop');}
 finally{session.dispose();}
});
await check('large synthetic Pi history and tool catalog use whole-request bounds',async()=>{
 const s=await setup([]);
 const calls=Array.from({length:66},(_,i)=>({type:'toolCall',id:'prior_'+i,name:'echo',arguments:{value:'a'.repeat(65*1024)}}));
 const assistant={role:'assistant',provider:'possums',api:'openai-completions',model:s.selected.id,timestamp:2,stopReason:'toolUse',content:calls,
  usage:{input:0,output:0,cacheRead:0,cacheWrite:0,totalTokens:0,cost:{input:0,output:0,cacheRead:0,cacheWrite:0,total:0}}};
 const results=calls.map(call=>({role:'toolResult',toolCallId:call.id,toolName:'echo',content:[{type:'text',text:'Synthetic'}],timestamp:3}));
 const tools=Array.from({length:65},(_,i)=>({...tool,name:i===0?'echo':'tool_'+i}));
 const transcript=ai.normalizeContext({messages:[user,assistant,...results,...Array.from({length:4097},()=>structuredClone(user))],tools});
 const invocation=m.invocation(s.selected,transcript);
 assert.equal(invocation.messages.length,4165);
 // Request admission accepts this history; the full snapshot's existing
 // 32,768-node lexical parser bound still rejects its larger wire structure.
 assert.throws(()=>m.snapshotInvocation(invocation));
 const payload=m.snapshotInvocation({...invocation,messages:Array.from(invocation.messages).slice(0,68)});
 assert.equal(payload.messages.length,68);assert.equal(payload.tools.length,65);
 assert.equal(payload.messages[1].tool_calls.length,66);
 assert.equal(JSON.parse(payload.messages[1].tool_calls[0].function.arguments).value.length,65*1024);
 assert(Object.isFrozen(payload.messages[1].tool_calls[0]));
});
await check('nested tool schema survives Pi conversion and the complete request snapshot',async()=>{
 let parameters={type:'object',properties:{value:{type:'string',enum:['synthetic']}}};
 for(let i=0;i<6;i++)parameters={type:'object',properties:{nested:parameters}};
 const s=await setup(['stop']);
 const transcript=ai.normalizeContext({messages:[user],tools:[{...tool,parameters}]});
 const payload=m.snapshotInvocation(m.invocation(s.selected,transcript));
 assert.deepEqual(payload.tools[0].function.parameters,parameters);
 assert(Object.isFrozen(payload.tools[0].function.parameters));
});
await check('repeated over-deep requests fail deterministically without inference or private content',async()=>{
 let parameters={type:'object',properties:{value:{type:'string',enum:['PRIVATE_SCHEMA_SENTINEL']}}};
 for(let i=0;i<14;i++)parameters={type:'object',properties:{nested:parameters}};
 const s=await setup(['stop']);const chat=s.client.chat;
 s.client.chat=async(model,messages,onDelta,newConversation,options)=>{
  m.snapshotInvocation({model,messages,stream:true,tools:options.tools});
  return chat(model,messages,onDelta,newConversation,options);
 };
 s.provider.beginRun();
 const transcript=ai.normalizeContext({messages:[user],tools:[{...tool,parameters}]});
 const first=await drain(s.provider.streamSimple(s.selected,transcript));
 assert.equal(first.message.stopReason,'error');assert.equal(s.sends(),0);
 assert.match(first.message.errorMessage,/\[possums_request_json_depth\].*Request encoding.*Simplify tool schemas or history.*No inference request sent.*Not replayed/);
 assert(!JSON.stringify(first).includes('PRIVATE_SCHEMA_SENTINEL'));
 assert.equal(first.message.diagnostics,undefined);
 const second=await drain(s.provider.streamSimple(s.selected,transcript));
 assert.equal(second.message.errorMessage,first.message.errorMessage);assert.equal(s.sends(),0);
});
await check('failed catalog refresh removes prior usable list',async()=>{
 const s=await setup(['stop']);assert.equal(s.provider.getModels().length,1);s.failModels();
 await assert.rejects(s.provider.refreshModels({credential:{type:'api_key',key:requestMarker},allowNetwork:true,signal:new AbortController().signal,publish:async value=>{value.update?.();return true;}}),/possums_catalog_unavailable/);
 assert.equal(s.provider.catalog.length,0);assert.equal(s.provider.listed.length,0);assert.match(s.provider.getModels()[0].name,/last-known/);
});
await check('length after tool deltas never authorizes tool execution',async()=>{
 const s=await setup(['tool_length']);s.provider.beginRun();const result=await drain(s.provider.streamSimple(s.selected,context(true)));
 assert.equal(result.message.stopReason,'error');assert.equal(result.events.some(x=>x.type==='toolcall_end'),false);assert.equal(s.sends(),1,result.message.errorMessage);
});
await check('late call identity maps only after an authenticated tool receipt',async()=>{
 const s=await setup(['late_tools']);s.provider.beginRun();const result=await drain(s.provider.streamSimple(s.selected,context(true)));
 assert.equal(result.message.stopReason,'toolUse');const call=result.message.content.find(value=>value.type==='toolCall');assert.equal(call.id,'call_one');assert.equal(call.name,'echo');assert.deepEqual(call.arguments,{value:'ok'});
 assert.equal(result.events.filter(value=>value.type==='toolcall_end').length,1);
});
await check('logout revokes pending tool execution while retaining a late settled receipt',async()=>{
 const s=await setup(['held_tools']);s.provider.beginRun();const stream=s.provider.streamSimple(s.selected,context(true));let loggedOut=false,ends=0;
 for await(const event of stream){if(event.type==='toolcall_delta'&&!loggedOut){loggedOut=true;s.provider.logout();s.release();}if(event.type==='toolcall_end')ends++;}
 const result=await stream.result();assert(loggedOut);assert.equal(ends,0);assert.equal(result.stopReason,'error');assert.match(result.errorMessage,/\[possums_run_replaced\].*settled charge remains recorded/);assert.equal(result.usage.cost.total,7/1e6);assert.equal(result.diagnostics[0].details.finish,'tool_calls');assert.equal(s.sends(),1);
});
await check('gateway failure descriptions preserve fixed reason and proven billing without exposing content',async()=>{
 for(const [mode, code, billing] of [['refund','generation_failed','refunded'],['unknown_bill','unavailable','unknown']]){
  const s=await setup([mode]);s.provider.beginRun();const result=await drain(s.provider.streamSimple(s.selected,context(false)));
  assert.match(result.message.errorMessage,/Transient service unavailable/);
  assert(result.message.errorMessage.includes(`[${code}]`));
  assert.match(result.message.errorMessage,new RegExp(code==='generation_failed'?'Generation failed.*stream idle timeout.*Reservation refunded':'Gateway unavailable.*inference unavailable.*Charge unknown'));
  assert.match(result.message.errorMessage,/Native automatic retry may incur another charge/);
  assert.doesNotMatch(result.message.errorMessage,/billing|Not replayed/i);
  assert.equal(result.message.diagnostics?.some(d=>d.type==='possums_billing_unknown')??false,billing==='unknown');
  assert.equal(result.events.some(e=>e.type==='toolcall_end'),false);assert.equal(s.sends(),1);
 }
 const s=await setup(['hostile']);s.provider.beginRun();const result=await drain(s.provider.streamSimple(s.selected,context(false)));
 assert.match(result.message.errorMessage,/\[possums_provider_unexpected\].*Stage: provider; constraint: unexpected.*cause is unknown.*Charge unknown.*Not replayed/);
 assert(!JSON.stringify(result).includes('PRIVATE_PROMPT'));
});
await check('pinned Pi classifier admits only closed gateway operational failures',async()=>{
 const transient=['tokenizer_send_failed','tokenizer_http_failed','generation_send_failed','generation_http_failed','stream_transport_failed','stream_idle_timeout','stream_deadline_exceeded'];
 const terminal=`inference_unavailable upstream_response_invalid verification_failed catalog_failed request_encoding_failed tool_profile_unqualified
 tokenizer_response_invalid tokenizer_upload_incomplete endpoint_binding_failed stream_content_type_invalid sdk_stream_decode_failed upstream_error_event
 stream_event_schema_invalid stream_choice_invalid stream_delta_unsupported tool_index_invalid tool_identity_invalid tool_name_not_allowed
 tool_choice_violated tool_call_incomplete tool_arguments_too_large stream_finish_invalid stream_finish_missing stream_usage_invalid
 stream_usage_unexpected stream_usage_missing stream_output_after_finish settlement_failed`.split(/\s+/);
 for(const detail of [...transient,...terminal,undefined]) {
  const s=await setup([{code:'generation_failed',detail}]);const {message}=await drain(s.provider.streamSimple(s.selected,context(false)));
  assert.equal(isRetryableAssistantError(message),transient.includes(detail),String(detail));assert.equal(s.sends(),1);
  assert(message.errorMessage.includes('[generation_failed]'));if(detail)assert(message.errorMessage.includes(`[${detail}]`));
  assert(message.diagnostics.some(d=>d.type==='possums_billing_unknown'));
  assert(!JSON.stringify(message).includes('PRIVATE_PROMPT'));
 }
 for(const code of ['unavailable','unauthorized','insufficient_credit','invalid_request','account_limit','duplicate_request']) {
  for(const detail of [undefined,'inference_unavailable','stream_idle_timeout']) {
   const s=await setup([{code,detail}]);const {message}=await drain(s.provider.streamSimple(s.selected,context(false)));
   assert.equal(isRetryableAssistantError(message),code==='unavailable'&&detail!=='stream_idle_timeout',code+'/'+detail);
   assert.equal(s.sends(),1);assert(message.errorMessage.includes(`[${code}]`));
  }
 }
 for(const [code,detail] of [['unavailable','PRIVATE_PROMPT_timeout'],['PRIVATE_PROMPT_service unavailable','stream_idle_timeout']]) {
  const s=await setup([]);s.client.chat=async()=>{throw new m.GatewayError(code,detail);};
  const {message}=await drain(s.provider.streamSimple(s.selected,context(false)));
  assert.equal(isRetryableAssistantError(message),false);assert(!JSON.stringify(message).includes('PRIVATE_PROMPT'));
 }
});
async function sdkSetup(name, plan, tools, compaction=false, qualified=true, restoreTools=false, retry={baseDelayMs:1,maxAgentDelayMs:8}, extraExtensions=[], toolHook=async()=>{}) {
 const s=await setup(plan,qualified);let toolRuns=0,provider,textOnlyCommand;const notices=[],compactions=[];
 // Keep native physical-model lookup and the selected host model consistent.
 const models=s.client.models;s.client.models=async()=> (await models()).map(model=>({...model,context_tokens:'64000'}));
 const settings=coding.SettingsManager.inMemory({compaction:{enabled:compaction,keepRecentTokens:4},retry,defaultTools:[],cacheWarming:'off',enableInstallTelemetry:false});
 const credentials=new ai.InMemoryCredentialStore();
 await credentials.modify('possums',async()=>({type:'api_key',key:recoveryKey}));
 const runtime=await coding.ModelRuntime.create({credentials,modelsStore:new ai.InMemoryModelsStore(),modelsPath:null,allowModelNetwork:false,refreshOnCreate:false});
 const cwd=path.join(root,name);fs.mkdirSync(cwd,{recursive:true});
 const loader=new coding.DefaultResourceLoader({cwd,agentDir:cwd,settingsManager:settings,noExtensions:true,noSkills:true,noPromptTemplates:true,noThemes:true,noContextFiles:true,systemPrompt:'Synthetic test',
  extensionFactories:[pi=>m.extension({...pi,on:(name,handler)=>pi.on(name,(event,ctx)=>{if(name==='session_before_compact'){compactions.push(event);ctx={...ctx,ui:{...ctx.ui,notify:text=>notices.push(text)}};}return handler(event,ctx);}),registerProvider:value=>{provider=value;value.establish=async()=>s.client;pi.registerProvider(value);},registerCommand:(name,command)=>{if(name==='possums-text-only')textOnlyCommand=command;if(name==='possums-reconcile'){const original=command;command={...original,handler:(args,ctx)=>original.handler(args,{...ctx,hasUI:true,ui:{...ctx.ui,notify:text=>notices.push(text)}})};}pi.registerCommand(name,command);}}),...(restoreTools?[pi=>pi.on('before_agent_start',()=>pi.setActiveTools(['echo']))]:[]),...extraExtensions]});
 await loader.reload();assert.deepEqual(loader.getExtensions().errors,[]);
 runtime.registerNativeProvider(provider);
 await runtime.refresh({providers:['possums'],allowNetwork:false});
 // Public verification and saved-auth restoration happen in actual session_start.
 // Large synthetic host window isolates Pi recovery behavior from the separately
 // tested gateway context budget; the fake ReferenceClient has no tokenizer.
 const selected={...s.selected,contextWindow:64000};
 const {session}=await coding.createAgentSession({cwd,agentDir:cwd,modelRuntime:runtime,model:selected,resourceLoader:loader,settingsManager:settings,
  thinkingLevel:'off',sessionManager:coding.SessionManager.inMemory(cwd),tools:tools?['echo']:[],customTools:[{...tool,label:'Echo',execute:async(...args)=>{toolRuns++;await toolHook(...args);return {content:[{type:'text',text:'ok'}],details:undefined};}}]});
 await session.bindExtensions({mode:'print'});
 if(tools)session.setActiveToolsByName(['echo']);assert.deepEqual(session.getActiveToolNames(),tools?['echo']:[]);
 return {session,s,runtime,provider,notices,compactions,settings,toolRuns:()=>toolRuns,textOnly:()=>textOnlyCommand.handler('',{ui:{notify:()=>{}}})};
}
await check('deferred renewal qualification: explicit source and active signal reach auth before stream; background signal differs',async()=>{
 const trace=[],signals=[];let runSignal;
 const f=await sdkSetup('sdk-renewal-signal',['stop','stop','stop'],false,false,true,false,{enabled:false},[pi=>{
  pi.on('input',(event,ctx)=>trace.push(['input',event.source,event.streamingBehavior,ctx.isIdle(),ctx.signal]));
  pi.on('before_agent_start',(_event,ctx)=>trace.push(['before',ctx.signal]));
  pi.on('agent_start',(_event,ctx)=>{runSignal=ctx.signal;signals.push(runSignal);trace.push(['start']);});
 }]);
 const resolve=f.provider.auth.apiKey.resolve,entered=deferred(),release=deferred();let hold=true;
 f.provider.auth.apiKey.resolve=async input=>{
  trace.push(['auth',input.signal===runSignal]);
  if(hold && input.signal===runSignal){entered.resolve();await release.promise;}
  return resolve(input);
 };
 try {
  const pending=f.session.prompt('Synthetic deferred renewal');await entered.promise;
  assert(runSignal instanceof AbortSignal);assert.equal(f.session.isIdle,false);assert.equal(f.s.sends(),0);
  await f.runtime.refresh({providers:['possums'],allowNetwork:true});
  assert(trace.some(event=>event[0]==='auth'&&!event[1]));assert.equal(f.s.sends(),0);
  const stopped=f.session.abort();await stopped;assert(runSignal.aborted);assert.equal(f.s.sends(),0);
  release.resolve();await pending;hold=false;
  await f.session.prompt('Synthetic RPC',{source:'rpc'});
  await f.session.sendUserMessage('Synthetic extension');
  assert.deepEqual(trace.filter(e=>e[0]==='input').map(e=>e.slice(1,4)),[
   ['interactive',undefined,true],['rpc',undefined,true],['extension',undefined,true],
  ]);
  assert(trace.filter(e=>e[0]==='before').every(e=>e[1]===undefined));
  assert.equal(new Set(signals).size,3);assert.equal(f.s.sends(),2);
 } finally {release.resolve();f.session.dispose();}
});
// Genuine Channel/ReferenceClient login with synthetic session endpoints; public
// verification is injectable, while the native event/auth/abort path is real.
async function renewalFixture(name, plan=['stop','stop'], extra=[], tools=false) {
 const f=await sdkSetup('sdk-renew-'+name,plan,tools,false,true,false,{baseDelayMs:1,maxAgentDelayMs:8},extra);
 const trace={verified:[],credentials:[],attempts:[],signals:[]},policies=new Map();
 let next='A',verifyHook=async()=>{},loginHook=async()=>{};
 const make=async tag=>{
  const config=new Uint8Array([0,0,32,...Array(32).fill(7),0,4,0,1,0,2]);
  const channel=await m.Channel.fixture('https://localhost:18443',config,'07'.repeat(32));
  const policy={tag,expires:Date.now()+60*60*1000};policies.set(tag,policy);
  Object.defineProperty(channel,'release',{get:()=>policy});
  const valid=()=>{if(Date.now()>=policy.expires)throw new m.ChannelError();};
  channel.challenge=async()=>{valid();return {challenge:'c'.repeat(43),expires_in:600};};
  channel.control=async(_path,payload,_bearer,signal)=>{
   valid();trace.credentials.push({tag,key:payload.credential});
   await loginHook(tag,payload.credential,signal);
   return {token:'b'.repeat(43),token_type:'Bearer',expires_in:43200};
  };
  const create=()=>{
   const client=new m.ReferenceClient(channel);
   client.models=async()=>{valid();return f.s.client.models();};
   client.chat=(...args)=>{valid();trace.attempts.push(tag);return f.s.client.chat(...args);};
   client.freshSession=create;return client;
  };
  return create();
 };
 f.provider.establish=async signal=>{
  const tag=next;trace.verified.push(tag);trace.signals.push(signal);
  await verifyHook(tag,signal);return make(tag);
 };
 f.provider.newSession();await f.provider.verifySession();
 await f.runtime.refresh({providers:['possums'],allowNetwork:true});
 assert.deepEqual(trace.verified,['A']);assert.equal(trace.credentials.length,1);
 return {...f,trace,expire:()=>{policies.get(f.provider.release.tag).expires=0;next='B';},
  authExpire:()=>Object.defineProperty(f.provider.client,'authExpiresAt',{value:0}),
  verifyHook:fn=>{verifyHook=fn;},loginHook:fn=>{loginHook=fn;},next:tag=>{next=tag;}};
}
await check('explicit interactive/RPC expiry renewal keeps transcript and conversation; valid turns do not verify',async()=>{
 for(const source of ['interactive','rpc']) {
  const f=await renewalFixture(source,['stop','stop','stop']);
  try {
   await f.session.prompt('Synthetic prior history',{source});
   const before=structuredClone(f.session.messages);assert.deepEqual(f.trace.verified,['A']);
   f.expire();await f.session.prompt('Synthetic fresh turn',{source});
   assert.deepEqual(f.trace.verified,['A','B']);assert.deepEqual(f.trace.attempts,['A','B']);
   assert.equal(f.provider.release.tag,'B');assert.equal(f.trace.credentials.length,2);
   assert.deepEqual(f.session.messages.slice(0,before.length),before);
   assert(f.s.requests[1].messages.some(message=>message.content==='Synthetic prior history'));
   assert.equal(f.s.requests[1].newConversation,false);
   await f.session.prompt('Synthetic still valid',{source});assert.deepEqual(f.trace.verified,['A','B']);
   assert.equal(f.trace.credentials.length,2);
  } finally {f.session.dispose();}
 }
});
await check('known auth-only expiry reauthenticates with the same verified trust and invalidates reconciliation',async()=>{
 const f=await renewalFixture('auth');
 try {
  f.provider.reconciliation={before:{availableMicrounits:'1',completedRequests:'0',inFlight:0},charged:0n,completed:0n,unknown:false};
  f.authExpire();await f.session.prompt('Synthetic auth renewal');
  assert.deepEqual(f.trace.verified,['A']);assert.deepEqual(f.trace.attempts,['A']);
  assert.equal(f.trace.credentials.length,2);assert.equal(f.provider.reconciliation,undefined);
  assert(f.provider.client.authExpiresAt>Date.now());
 } finally {f.session.dispose();}
});
await check('Stop during deferred verification/login aborts the real run and late results cannot publish or infer',async()=>{
 for(const stage of ['verify','login']) {
  const f=await renewalFixture('stop-'+stage),entered=deferred(),release=deferred();let signal;
  f.expire();
  const hold=async(_tag,...args)=>{signal=args.at(-1);entered.resolve();await release.promise;};
  if(stage==='verify')f.verifyHook(hold);else f.loginHook(hold);
  try {
   const pending=f.session.prompt('Synthetic stopped renewal');await entered.promise;
   assert(signal instanceof AbortSignal);assert.equal(f.session.isIdle,false);assert.equal(f.s.sends(),0);
   if(stage==='verify')assert.equal(f.trace.credentials.length,1,'no credential before new verification');
   // A separate refresh has a signal, but not this submission's permission.
   await f.runtime.refresh({providers:['possums'],allowNetwork:true});
   assert.deepEqual(f.trace.verified,['A','B']);
   await f.session.abort();await pending;assert(signal.aborted);assert.equal(f.s.sends(),0);
   release.resolve();await new Promise(setImmediate);
   assert.equal(f.provider.release.tag,'A');assert.deepEqual(f.provider.catalog,[]);assert.deepEqual(f.provider.listed,[]);
   assert.equal(f.s.sends(),0);
   f.verifyHook(async()=>{});f.loginHook(async()=>{});
   await f.session.prompt('Synthetic deliberate next turn');
   assert.equal(f.provider.release.tag,'B');assert.equal(f.s.sends(),1);
  } finally {release.resolve();f.session.dispose();}
 }
});
await check('failed renewal preserves closed stage, sends no credential/prompt and cannot renew on background or extension work',async()=>{
 for(const code of ['verification_failed','evidence_unavailable','approval_expired']) {
  const f=await renewalFixture('failure-'+code);f.expire();
  f.verifyHook(async()=>{throw new m.ConnectionFailure(code);});
  try {
   await f.session.prompt('Synthetic failed renewal');
   assert.equal(f.s.sends(),0);assert.equal(f.trace.credentials.length,1);
   assert.equal(f.trace.verified.length,2);assert.match(f.session.messages.at(-1).errorMessage,new RegExp('possums_'+code));
   await f.runtime.refresh({providers:['possums'],allowNetwork:true});
   await f.session.sendUserMessage('Synthetic extension cannot retry renewal');
   assert.equal(f.trace.verified.length,2);assert.equal(f.s.sends(),0);
   f.verifyHook(async()=>{});await f.session.prompt('Synthetic explicit retry');
   assert.equal(f.trace.verified.length,3);assert.equal(f.s.sends(),1);
  } finally {f.session.dispose();}
 }
});
await check('extension-origin prompts cannot initiate expiry renewal',async()=>{
 const f=await renewalFixture('extension');f.expire();
 try {
  await f.session.sendUserMessage('Synthetic extension prompt');
  assert.deepEqual(f.trace.verified,['A']);assert.equal(f.s.sends(),0);
  await f.session.prompt('Synthetic explicit prompt');assert.deepEqual(f.trace.verified,['A','B']);assert.equal(f.s.sends(),1);
 } finally {f.session.dispose();}
});
await check('expiry within native retries and tool continuation never swaps trust',async()=>{
 for(const mode of ['unknown_bill','tool_calls']) {
  const f=await renewalFixture('continuation-'+mode,[mode,'stop'],[],mode==='tool_calls');
  if(mode==='tool_calls')f.session.setActiveToolsByName(['echo']);
  const chat=f.s.client.chat;
  f.s.client.chat=async(...args)=>{try{return await chat(...args);}finally{f.expire();}};
  try {
   await f.session.prompt('Synthetic continuation');
   assert.deepEqual(f.trace.verified,['A']);assert.equal(f.s.sends(),1);
   assert.equal(f.toolRuns(),mode==='tool_calls'?1:0);
   assert.equal(f.session.messages.at(-1).stopReason,'error');
  } finally {f.session.dispose();}
 }
});
await check('queued steering/follow-up do not grant renewal; the later idle explicit submission can',async()=>{
 for(const behavior of ['steer','followUp']) {
  const f=await renewalFixture('queued-'+behavior,['held','stop']),entered=deferred();
  const chat=f.s.client.chat;
  f.s.client.chat=async(...args)=>{const pending=chat(...args);entered.resolve();const value=await pending;f.expire();return value;};
  try {
   const pending=f.session.prompt('Synthetic original run');await entered.promise;
   await f.session[behavior]('Synthetic queued input');f.s.release();await pending;
   assert.deepEqual(f.trace.verified,['A']);assert.equal(f.s.sends(),1);
   f.s.client.chat=chat;await f.session.prompt('Synthetic later explicit input');
   assert.deepEqual(f.trace.verified,['A','B']);assert.equal(f.s.sends(),2);
  } finally {f.s.release();f.session.dispose();}
 }
});
await check('logout/session/account replacement invalidates pending renewal and rejects stale publication',async()=>{
 for(const action of ['logout','session','account']) {
  const f=await renewalFixture('race-'+action),entered=deferred(),release=deferred();let heldSignal;
  // Use valid public trust + expired auth, so a deliberate account replacement
  // can authenticate immediately without borrowing the old run's permission.
  f.authExpire();f.loginHook(async(_tag,key,signal)=>{if(key===recoveryKey){heldSignal=signal;entered.resolve();await release.promise;}});
  try {
   const pending=f.session.prompt('Synthetic obsolete renewal');await entered.promise;
   if(action==='logout')await f.runtime.logout('possums');
   if(action==='session')f.provider.newSession();
   if(action==='account')await f.runtime.login('possums','api_key',interaction(envKey));
   assert(heldSignal.aborted);release.resolve();await pending;await new Promise(setImmediate);
   assert.equal(f.s.sends(),0);
   if(action==='account'){assert.equal(f.provider.recoveryKey,envKey);assert.equal(f.provider.getModels().length,1);}
   else {assert.deepEqual(f.provider.catalog,[]);assert.deepEqual(f.provider.listed,[]);}
  } finally {release.resolve();f.session.dispose();}
 }
});
await check('background refresh during input and pre-prompt/manual compaction cannot borrow renewal permission',async()=>{
 let f;
 f=await renewalFixture('preflight',['stop','stop','stop'],[pi=>pi.on('input',async(_event,ctx)=>{
  if(!f)return;
  await ctx.modelRegistry.refresh({providers:['possums'],allowNetwork:true});
  assert.deepEqual(f.trace.verified,['A']);
 })]);
 const chat=f.s.client.chat;
 try {
  f.s.client.chat=async(...args)=>({...await chat(...args),inputTokens:50000,totalTokens:50003});
  await f.session.prompt('Synthetic prior context '.repeat(100));
  await f.session.prompt('Synthetic high context '.repeat(100));f.s.client.chat=chat;
  f.expire();
  await assert.rejects(()=>f.session.compact(),/Compaction cancelled/);
  assert.deepEqual(f.trace.verified,['A']);assert.equal(f.s.sends(),2);
  f.settings.setCompactionEnabled(true);
  await f.session.prompt('Synthetic explicit after expired compaction');
  assert(f.compactions.some(event=>event.reason==='threshold'));
  assert.deepEqual(f.trace.verified,['A','B']);assert.equal(f.s.sends(),3);
 } finally {f.session.dispose();}
});
await check('nested extension custom run cannot steal a marked explicit submission',async()=>{
 let f,injected=false;
 f=await renewalFixture('nested-custom',['stop'],[pi=>pi.on('before_agent_start',async()=>{
  if(!f||injected)return;injected=true;
  await f.session.sendCustomMessage({customType:'synthetic',content:'Synthetic extension work',display:false},{triggerTurn:true});
 })]);
 f.expire();
 try {
  await f.session.prompt('Synthetic explicit pending input');
  assert.deepEqual(f.trace.verified,['A']);assert.equal(f.s.sends(),0);
 } finally {f.session.dispose();}
});
await check('overlapping idle input origins fail closed instead of transferring explicit permission',async()=>{
 for(const heldSource of ['interactive','extension']) {
  const entered=deferred(),release=deferred();let held=false;
  const f=await renewalFixture('overlap-'+heldSource,['stop'],[pi=>pi.on('input',async event=>{
   if(!held&&event.source===heldSource){held=true;entered.resolve();await release.promise;}
  })]);
  f.expire();
  try {
   const first=f.session.prompt('Synthetic first overlapping input',{source:heldSource});await entered.promise;
   await f.session.prompt('Synthetic other overlapping input',{source:heldSource==='interactive'?'extension':'interactive'});
   release.resolve();await first;
   assert.deepEqual(f.trace.verified,['A']);assert.equal(f.s.sends(),0);
   await f.session.prompt('Synthetic unambiguous submission');
   assert.deepEqual(f.trace.verified,['A','B']);assert.equal(f.s.sends(),1);
  } finally {release.resolve();f.session.dispose();}
 }
});
await check('expired returned verification context cannot authorize a credential or prompt',async()=>{
 const f=await renewalFixture('expired-candidate'),old=f.provider.verifiedTemplate;
 f.expire();f.provider.establish=async()=>old;
 try {
  await f.session.prompt('Synthetic invalid renewal');
  assert.equal(f.trace.credentials.length,1);assert.equal(f.s.sends(),0);
  assert.match(f.session.messages.at(-1).errorMessage,/possums_verification_failed/);
 } finally {f.session.dispose();}
});
await check('an earlier nested auth request on the active signal consumes the candidate without renewing',async()=>{
 let f,injected=false;
 f=await renewalFixture('nested-auth',['stop'],[pi=>pi.on('agent_start',async(_event,ctx)=>{
  if(!f||injected)return;injected=true;
  await ctx.modelRegistry.streamSimple(ctx.model,context(false),{signal:ctx.signal}).result();
 })]);
 f.expire();
 try {
  await f.session.prompt('Synthetic explicit after nested auth');
  assert.deepEqual(f.trace.verified,['A']);assert.equal(f.s.sends(),0);
  await f.session.prompt('Synthetic next explicit');
  assert.deepEqual(f.trace.verified,['A','B']);assert.equal(f.s.sends(),1);
 } finally {f.session.dispose();}
});
// Qualification only: these probes record a BLOCKER, not deployment-recovery
// permission. Pi is unmodified; only the synthetic provider instance is observed.
// The fixture's `nested` label is test scheduling, never a provider capability.
for(const [hook,round] of [
  ['message_start',0],['turn_start',0],['turn_start',1],
  ['context',0],['context',1],['context_with_system',0],['context_with_system',1],
  ['before_provider_request',0],['before_provider_request',1],['tool_execute',0],
]) {
 await check('permission qualification BLOCKED: same-signal '+hook+' round '+round,async()=>{
  let f,preparedMessages,nested=false,injected=false,nestedResult,hookFailure=false,turn=-1,starts=0,ends=0,hookCalls=0;
  const requests=[],auth=[];
  const inject=async(event,ctx,toolSignal)=>{
   if(injected)return;
   injected=true;hookCalls++;nested=true;
   try {
    // Use supported transcript projection, not transcript-text matching or IDs.
    // During execution, reuse the real preceding request captured by a
    // supported hook; an unfinished tool batch is not a valid model input.
    const messages=hook==='tool_execute'?preparedMessages:hook==='context_with_system'?event.messages:
     hook==='message_start'?[...f.session.messages,event.message]:f.session.messages;
    // The first turn_start precedes user-message delivery; it is a signal-only
    // control. Later boundaries use the actual native transcript projection.
    const transcript=hook==='turn_start'&&round===0?context(true):
     ai.normalizeContext({messages:coding.convertToLlm(messages)});
    const signal=toolSignal??ctx.signal;
    if(!(signal instanceof AbortSignal)||signal!==ctx.signal)throw new Error('qualification_signal_mismatch');
    nestedResult=await ctx.modelRegistry.streamSimple(ctx.model,transcript,{signal}).result();
   } catch {hookFailure=true;} finally {nested=false;}
  };
  const extensions=[pi=>{
   pi.on('context_with_system',event=>{preparedMessages=structuredClone(event.messages);});
   pi.on('agent_start',()=>{starts++;});
   pi.on('turn_end',()=>{ends++;});
   pi.on('turn_start',async(event,ctx)=>{turn++;if(hook==='turn_start'&&turn===round)await inject(event,ctx);});
   if(!['turn_start','tool_execute'].includes(hook))pi.on(hook,async(event,ctx)=>{
    if(turn===round&&(hook!=='message_start'||event.message.role==='user'))await inject(event,ctx);
   });
  }];
  const beforeInitial=round===0&&!['before_provider_request','tool_execute'].includes(hook);
  f=await sdkSetup('sdk-permission-'+hook+'-'+round,
   beforeInitial?['stop','tool_calls','stop']:['tool_calls','stop','stop'],true,false,true,false,{enabled:false},extensions,
   async(_id,_params,signal,_update,ctx)=>{if(hook==='tool_execute')await inject(undefined,ctx,signal);});
  const resolve=f.provider.auth.apiKey.resolve,stream=f.provider.streamSimple.bind(f.provider);
  f.provider.auth.apiKey.resolve=input=>{auth.push({nested,input});return resolve(input);};
  f.provider.streamSimple=(model,transcript,options)=>{
   requests.push({nested,model,transcript:structuredClone(transcript),signal:options.signal});
   return stream(model,transcript,options);
  };
  try {
   await f.session.prompt('Synthetic permission qualification');
   assert(!hookFailure,'qualification hook failed');assert(injected);assert.equal(hookCalls,1);
   assert.equal(nestedResult?.stopReason,'stop');assert.equal(f.session.messages.at(-1).stopReason,'stop');
   assert.equal(starts,1);assert.equal(ends,2);assert.equal(turn,1);
   assert.equal(f.toolRuns(),1);assert.equal(f.s.sends(),3);
   assert.equal(requests.length,3);assert.equal(auth.length,3);
   const nestedRequest=requests.find(request=>request.nested),native=requests.filter(request=>!request.nested);
   assert.equal(native.length,2);
   assert(requests.every(request=>request.signal===native[0].signal),'signal does not identify native requests');
   assert(auth.every(request=>request.input.signal===native[0].signal),'auth shares the active signal');
   // Full auth inputs, including context and stored synthetic credential, are
   // identical. No extra native-origin field arrives at this boundary.
   assert(auth.every(request=>isDeepStrictEqual(request.input,auth[0].input)),'auth boundary changed; requalify provenance');
   assert(native[1].transcript.messages.some(message=>message.role==='toolResult'));
   if(!(hook==='turn_start'&&round===0)) {
    assert(isDeepStrictEqual(nestedRequest.transcript,native[round].transcript),'matching transcript probe changed');
    assert(isDeepStrictEqual(nestedRequest.model,native[round].model),'matching model probe changed');
   }
   if(round===1)assert(nestedRequest.transcript.messages.some(message=>message.role==='toolResult'),
    'preceding tool result is also available to nested work');
  } finally {f.session.dispose();}
 });
}
await check('permission qualification BLOCKED: post-confirmation nested auth can consume existing expiry renewal',async()=>{
 for(const hook of ['message_start','context_with_system']) {
  let f,injected=false,nested=false,renewedByNested=false,hookFailure=false,result;
  f=await renewalFixture('post-confirmation-'+hook,['stop','stop'],[pi=>pi.on(hook,async(event,ctx)=>{
   if(injected||(hook==='message_start'&&event.message.role!=='user'))return;
   injected=true;nested=true;
   try {
    const messages=hook==='message_start'?[...f.session.messages,event.message]:event.messages;
    result=await ctx.modelRegistry.streamSimple(ctx.model,
     ai.normalizeContext({messages:coding.convertToLlm(messages)}),{signal:ctx.signal}).result();
   } catch {hookFailure=true;} finally {nested=false;}
  })]);
  f.expire();f.verifyHook(async()=>{renewedByNested=nested;});
  try {
   await f.session.prompt('Synthetic post-confirmation qualification');
   assert(!hookFailure);assert(injected);assert(renewedByNested,'pinned permission behavior changed; requalify');
   assert.equal(result?.stopReason,'stop');assert.deepEqual(f.trace.verified,['A','B']);
   assert.equal(f.trace.credentials.length,2);assert.equal(f.s.sends(),2);
   assert.equal(f.session.messages.at(-1).stopReason,'stop');
  } finally {f.session.dispose();}
 }
});
await check('late public verification after logout or session replacement cannot publish or send a credential',async()=>{
 for(const action of ['logout','session']) {
  const f=await renewalFixture('public-race-'+action),entered=deferred(),release=deferred();let oldSignal;
  f.expire();f.verifyHook(async(tag,signal)=>{if(tag==='B'){oldSignal=signal;entered.resolve();await release.promise;}});
  try {
   const pending=f.session.prompt('Synthetic obsolete public verification');await entered.promise;
   if(action==='logout')await f.runtime.logout('possums');
   else {f.next('C');f.provider.newSession();await f.provider.verifySession();}
   assert(oldSignal.aborted);release.resolve();await pending;await new Promise(setImmediate);
   assert.equal(f.provider.release.tag,action==='logout'?'A':'C');
   assert.equal(f.trace.credentials.length,1);assert.equal(f.s.sends(),0);
   assert.deepEqual(f.provider.catalog,[]);assert.deepEqual(f.provider.listed,[]);
  } finally {release.resolve();f.session.dispose();}
 }
});
await check('auth resolution retains its renewal epoch across the completion handoff',async()=>{
 const f=await renewalFixture('handoff'),entered=deferred(),release=deferred();
 const renew=f.provider.renewForSubmission.bind(f.provider);
 f.provider.renewForSubmission=async(...args)=>{const epoch=await renew(...args);entered.resolve();await release.promise;return epoch;};
 f.authExpire();
 try {
  const pending=f.session.prompt('Synthetic old account renewal');await entered.promise;
  await f.runtime.login('possums','api_key',interaction(envKey));release.resolve();await pending;
  assert.equal(f.s.sends(),0);assert.equal(f.provider.recoveryKey,envKey);
  assert.equal(f.provider.getModels().length,1);
 } finally {release.resolve();f.session.dispose();}
});
await check('renewal blocker: Pi 1.0.4 Stop does not cancel an awaited before_agent_start hook',async()=>{
 const entered=deferred(),release=deferred();let signal,idle;
 const f=await sdkSetup('sdk-renewal-boundary-probe',['stop'],false,false,true,false,
  {enabled:false},[pi=>pi.on('before_agent_start',async(_event,ctx)=>{
   signal=ctx.signal;idle=ctx.isIdle();entered.resolve();await release.promise;
  })]);
 try {
  const pending=f.session.prompt('Synthetic boundary probe');await entered.promise;
  assert.equal(signal,undefined);assert.equal(idle,true);assert.equal(f.s.sends(),0);
  await f.session.abort();assert.equal(f.s.sends(),0);
  release.resolve();await pending;
  // This is pinned runtime behavior, NOT permission for renewal to revive work.
  assert.equal(f.s.sends(),1);
 } finally {release.resolve();f.session.dispose();}
});
function balancePlan(client, snapshots) {
 let reads=0;client.balance=async()=>{assert(reads<snapshots.length,'unexpected balance read');return snapshots[reads++];};return ()=>reads;
}
const balanceSnapshot=(available='5000000',completed='0',inFlight=0)=>({availableMicrounits:available,completedRequests:completed,inFlight});
const reconciliationContext=s=>({isIdle:()=>true,signal:new AbortController().signal,
 modelRegistry:{getProviderAuth:()=>s.provider.auth.apiKey.resolve(authInput(s.credential))}});
await check('actual Pi SDK retains every closed component failure with unknown billing and no native retry',async()=>{
 const f=await sdkSetup('sdk-component-diagnostics',[],false);let calls=0;
 try {
  for(const [stage,constraint] of [['trust','expired'],['request','encoding'],['catalog','schema'],['submission','http'],
    ['transport','endpoint_binding'],['transport','frames'],['transport','decryption'],['transport','idle'],['transport','deadline'],
    ['stream','utf8'],['stream','json'],['stream','choice'],['stream','delta'],['stream','tool'],['stream','finish_missing'],
    ['stream','usage_missing'],['stream','done_missing'],['stream','eof'],['settlement','receipt'],['provider','unexpected'],['hook','unexpected']]) {
   f.s.client.chat=async()=>{calls++;throw new m.DiagnosticFailure(stage,constraint,'uncertain',stage==='submission'?401:undefined);};
   await f.session.prompt('Synthetic failure fixture');
   const result=f.session.messages.at(-1);
   assert.equal(result.stopReason,'error');
   assert(result.errorMessage.includes(`[possums_${stage}_${constraint}]`),result.errorMessage);
   assert(result.errorMessage.includes(`Stage: ${stage}; constraint: ${constraint}`));
   assert.match(result.errorMessage,/Charge unknown.*Not replayed.*another charge/);
   assert(!isRetryableAssistantError(result));
   assert(result.diagnostics.some(value=>value.type==='possums_billing_unknown'));
  }
  assert.equal(calls,21);assert.equal(f.toolRuns(),0);assert.equal(f.s.sends(),0);
 } finally {f.session.dispose();}
});
await check('post-receipt display precision failure retains settled accounting instead of inventing unknown billing',async()=>{
 const s=await setup([]),charged='18446744073709551615';let calls=0;
 s.client.chat=async(_model,_messages,onDelta,_fresh,options)=>{
  calls++;
  const values=[event({role:'assistant'}),{object:'chat.completion.chunk',model:'synthetic',choices:[{index:0,delta:{},finish_reason:'stop'}]},
   {object:'chat.completion.chunk',model:'synthetic',choices:[],usage:{prompt_tokens:2,completion_tokens:3,total_tokens:5},possums:{outcome:'settled',charged_microunits:charged,refunded_microunits:'0'}}];
  const body=new ReadableStream({start(controller){controller.enqueue(new TextEncoder().encode(values.map(value=>'data: '+JSON.stringify(value)+'\n\n').join('')+'data: [DONE]\n\n'));controller.close();}});
  return m.consumeCompletion(body,'synthetic',onDelta,{onEvent:options.onEvent,signal:options.signal});
 };
 const result=(await drain(s.provider.streamSimple(s.selected,context(false)))).message;
 assert.equal(calls,1);assert.equal(result.stopReason,'error');
 assert.match(result.errorMessage,/possums_settlement_precision.*Stage: settlement.*supported safe-integer.*Authenticated receipt: charge settled/);
 assert.doesNotMatch(result.errorMessage,/Charge unknown|Model catalog exceeds|No complete validated receipt/);
 assert.equal(result.diagnostics[0].type,'possums_settled_receipt');assert.equal(result.diagnostics[0].details.chargedMicrounits,charged);
});
await check('actual native auth wrapper preserves the closed provider cause and transient diagnostic',async()=>{
 const s=await setup([]);const notices=[];
 const provider=new m.PossumsProvider(async()=>{
  const client=freshFixture(s.client);client.login=async()=>{throw new m.DiagnosticFailure('authentication','http','rejected',401);};
  client.freshSession=()=>client;return client;
 },failure=>{if(failure)notices.push(failure.message);});
 const credentials=new ai.InMemoryCredentialStore();await credentials.modify('possums',async()=>({type:'api_key',key:recoveryKey}));
 const runtime=await nativeRuntime(provider,credentials);
 await assert.rejects(runtime.getAuth('possums'),error=>{
  assert.match(error.message,/API key auth failed for provider possums: \[possums_authentication_http\].*Stage: authentication.*Observed HTTP status: 401/);
  assert.doesNotMatch(error.message,/PRIVATE_PROMPT|synthetic_not_a_usable/);
  assert(error.cause instanceof m.ConnectionFailure);return true;
 });
 assert.match(notices.at(-1),/possums_authentication_http.*Stage: authentication.*Observed HTTP status: 401.*No inference request/);
 assert.doesNotMatch(notices.join(' '),/Session expired|PRIVATE_PROMPT|synthetic_not_a_usable/);assert.equal(s.sends(),0);
});
await check('credential selection and entry failures never echo native hostile exceptions or initiate inference',async()=>{
 const s=await setup([]),notices=[];
 const provider=new m.PossumsProvider(async()=>s.client,failure=>{if(failure)notices.push(failure.message);});
 const hostile={...authInput(undefined),ctx:{env:async()=>{throw new Error('HOSTILE_CREDENTIAL_STACK_URL');},fileExists:async()=>false}};
 for(const action of ['check','resolve'])await assert.rejects(provider.auth.apiKey[action](hostile),error=>error instanceof m.ConnectionFailure&&error.code==='credential_resolution_failed');
 await assert.rejects(provider.auth.apiKey.login({...interaction(),prompt:async()=>{throw new Error('HOSTILE_CREDENTIAL_STACK_URL');}}),error=>error instanceof m.ConnectionFailure&&error.code==='credential_entry_failed');
 assert.equal(await provider.auth.apiKey.resolve(authInput(undefined)),undefined);
 assert.match(notices.at(-1),/possums_credential_missing.*Use \/login/);
 assert(!notices.join(' ').includes('HOSTILE_CREDENTIAL_STACK_URL'));assert.equal(s.sends(),0);
});
await check('actual SDK retry backoff abort drops errorMessage at retryAssistantCall, retaining the closed callback diagnostic',async()=>{
 const s=await setup(['unknown_bill']);const result=(await drain(s.provider.streamSimple(s.selected,context(false)))).message;
 const abort=new AbortController();let observed;
 const final=await retryAssistantCall(async()=>result,{enabled:true,maxRetries:1,baseDelayMs:1000},abort.signal,
  {onRetryScheduled:(_attempt,_max,_delay,message)=>{observed=message;abort.abort();}});
 assert.equal(final.stopReason,'aborted');assert.equal(final.errorMessage,undefined);
 assert.match(observed,/\[unavailable\].*Stage: gateway_admission.*Charge unknown.*another charge/);
 assert.equal(s.sends(),1);
});
await check('encrypted balance HTTP 503 body and EOF failures reach actual Pi reconciliation presentation',async()=>{
 const ehbp=await import(pathToFileURL(path.resolve(path.dirname(file),'../../source/clients/pi/node_modules/ehbp/dist/esm/index.js')).href);
 const server=await ehbp.Identity.generate(),encoder=new TextEncoder();
 const channel=await m.Channel.fixture('https://localhost:18443',await server.marshalConfig(),await server.getPublicKeyHex());
 channel.challenge=async()=>({challenge:'c'.repeat(43),expires_in:600});
 channel.control=async()=>({token:'b'.repeat(43),token_type:'Bearer',expires_in:43200});
 const client=new m.ReferenceClient(channel),provider=new m.PossumsProvider(async()=>client);
 await client.login(recoveryKey);
 const originalFetch=globalThis.fetch,paths=[];
 const wire={error:{code:'service_quiescing',stage:'admission',constraint:'service_quiescing',billing:'not_submitted',message:hostileConnection}};
 try {
  for(const [body,interrupted,stage,constraint] of [
   ['{"error":"'+hostileConnection+'"',false,'balance','json'],
   [JSON.stringify(wire),true,'transport','fetch'],
  ]) {
   globalThis.fetch=async request=>{
    paths.push(new URL(request.url).pathname);
    assert.equal(request.method,'POST');assert.equal(request.headers.get('authorization'),'Bearer '+'b'.repeat(43));
    const encapsulated=ehbp.hexToBytes(request.headers.get('Ehbp-Encapsulated-Key'));
    const recipient=await server.suite.SetupRecipient(server.getPrivateKey(),encapsulated,{info:encoder.encode(ehbp.HPKE_REQUEST_INFO)});
    const sent=new Uint8Array(await request.arrayBuffer());
    assert.deepEqual(JSON.parse(new TextDecoder().decode(await recipient.Open(sent.slice(4)))),{});
    const nonce=crypto.getRandomValues(new Uint8Array(32));
    const secret=new Uint8Array(await recipient.Export(encoder.encode(ehbp.EXPORT_LABEL),ehbp.EXPORT_LENGTH));
    const keys=await ehbp.deriveResponseKeys(secret,encapsulated,nonce);
    const cipher=await ehbp.encryptChunk(keys,0,encoder.encode(body));
    const frame=new Uint8Array(4+cipher.length);new DataView(frame.buffer).setUint32(0,cipher.length,false);frame.set(cipher,4);
    let sentFrame=false;
    return new Response(new ReadableStream({pull(controller){
     if(!sentFrame){sentFrame=true;controller.enqueue(frame);}
     else if(interrupted)controller.error(new Error(hostileConnection));else controller.close();
    }},{highWaterMark:0}),{status:503,headers:{'Ehbp-Response-Nonce':ehbp.bytesToHex(nonce)}});
   };
   await assert.rejects(client.balance(),error=>{
    assert(error instanceof m.BalanceFailure);assert.equal(error.code,'uncertain');
    assert(error.observation instanceof m.DiagnosticFailure);
    assert.equal(error.observation.stage,stage);assert.equal(error.observation.constraint,constraint);
    assert.equal(error.observation.status,503);assert.equal(error.reason,undefined);
    assert.equal(error.billing,undefined);assert.equal(error.cause,undefined);
    const shown=provider.reconciliationFailure(error);
    assert.match(shown,new RegExp(`possums_${stage}_${constraint}.*Stage: ${stage}; constraint: ${constraint}.*Observed HTTP status: 503`));
    assert.match(shown,/prior billing is unchanged or unknown/);
    assert.doesNotMatch(shown,/PRIVATE_URL_CREDENTIAL_PROMPT|synthetic_not_a_usable|service_quiescing|Reservation refunded|not_submitted|\x1b|stack/i);
    assert(!JSON.stringify(error).includes(hostileConnection));
    return true;
   });
   assert.equal(paths.length,interrupted?2:1,'presentation cannot issue additional requests');
  }
  assert.deepEqual(paths,['/v1/balance','/v1/balance']);
 }finally{globalThis.fetch=originalFetch;}
});
await check('rotated recipient rejects old EHBP request; plaintext submission 422 observes only local binding status',async()=>{
 const ehbp=await import(pathToFileURL(path.resolve(path.dirname(file),'../../source/clients/pi/node_modules/ehbp/dist/esm/index.js')).href);
 const old=await ehbp.Identity.generate(),rotated=await ehbp.Identity.generate();
 const channel=await m.Channel.fixture('https://localhost:18443',await old.marshalConfig(),await old.getPublicKeyHex());
 const originalFetch=globalThis.fetch;let calls=0,reads=0;
 try {
  for(const body of [JSON.stringify({type:hostileConnection,error:{code:'unauthorized',billing:'refunded'}}),
   'unreadable '+hostileConnection,new ReadableStream({pull(controller){reads++;controller.error(Error(hostileConnection));}}, {highWaterMark:0})]){
   globalThis.fetch=async request=>{
    calls++;assert.equal(new URL(request.url).pathname,'/v1/submissions');
    const encapsulated=ehbp.hexToBytes(request.headers.get('Ehbp-Encapsulated-Key'));
    const recipient=await rotated.suite.SetupRecipient(rotated.getPrivateKey(),encapsulated,{info:new TextEncoder().encode(ehbp.HPKE_REQUEST_INFO)});
    const encrypted=new Uint8Array(await request.arrayBuffer());
    await assert.rejects(recipient.Open(encrypted.slice(4)),'rotated key cannot open the old request');
    return new Response(body,{status:422,headers:{'content-type':'application/problem+json'}});
   };
   await assert.rejects(channel.control('/v1/submissions',{model:'synthetic',new_conversation:false},'b'.repeat(43)),error=>{
    assert(error instanceof m.DiagnosticFailure);assert.equal(error.stage,'submission');assert.equal(error.constraint,'endpoint_binding');
    assert.equal(error.status,422);assert.equal(error.code,'uncertain');assert.equal(error.billing,undefined);
    assert(!error.message.includes(hostileConnection));return true;
   });
  }
  assert.equal(reads,0,'missing nonce must not parse the untrusted plaintext body');
  for(const nonce of ['bad','a'.repeat(62)]){
   globalThis.fetch=async()=>{calls++;return new Response(hostileConnection,{status:422,headers:{'Ehbp-Response-Nonce':nonce}});};
   await assert.rejects(channel.control('/v1/submissions',{model:'synthetic',new_conversation:false},'b'.repeat(43)),error=>
    error instanceof m.DiagnosticFailure && error.stage==='submission' && error.constraint==='endpoint_binding' && error.status===422 && !error.message.includes(hostileConnection));
  }
  globalThis.fetch=async()=>{calls++;return new Response(hostileConnection,{status:401});};
  await assert.rejects(channel.control('/v1/submissions',{model:'synthetic',new_conversation:false},'b'.repeat(43)),error=>
   error instanceof m.DiagnosticFailure && error.constraint==='endpoint_binding' && error.status===401);
  assert.equal(calls,6,'no retry, trust probe or inference from a diagnostic');
 }finally{globalThis.fetch=originalFetch;}
});
await check('authenticated submission rejection retains its original HTTP, body and typed quiescing classification',async()=>{
 const ehbp=await import(pathToFileURL(path.resolve(path.dirname(file),'../../source/clients/pi/node_modules/ehbp/dist/esm/index.js')).href);
 const server=await ehbp.Identity.generate(),encoder=new TextEncoder();
 const channel=await m.Channel.fixture('https://localhost:18443',await server.marshalConfig(),await server.getPublicKeyHex());
 const originalFetch=globalThis.fetch;let calls=0;
 try {
  for(const mode of ['complete','malformed','wrong_envelope','interrupted','quiescing']){
   globalThis.fetch=async request=>{
    calls++;assert.equal(new URL(request.url).pathname,'/v1/submissions');
    const encapsulated=ehbp.hexToBytes(request.headers.get('Ehbp-Encapsulated-Key'));
    const recipient=await server.suite.SetupRecipient(server.getPrivateKey(),encapsulated,{info:encoder.encode(ehbp.HPKE_REQUEST_INFO)});
    const nonce=crypto.getRandomValues(new Uint8Array(32));
    const secret=new Uint8Array(await recipient.Export(encoder.encode(ehbp.EXPORT_LABEL),ehbp.EXPORT_LENGTH));
    const keys=await ehbp.deriveResponseKeys(secret,encapsulated,nonce);
    const text=mode==='malformed'?'{':JSON.stringify(mode==='wrong_envelope'?{}:mode==='quiescing'?{
     error:{code:'service_quiescing',stage:'admission',constraint:'service_quiescing',billing:'not_submitted',message:hostileConnection}}:{error:{code:'unauthorized',message:hostileConnection}});
    const cipher=await ehbp.encryptChunk(keys,0,encoder.encode(text));
    const frame=new Uint8Array(4+cipher.length);new DataView(frame.buffer).setUint32(0,cipher.length,false);frame.set(cipher,4);
    let once=false;
    return new Response(new ReadableStream({pull(controller){if(!once){once=true;controller.enqueue(frame);}else if(mode==='interrupted')controller.error(Error(hostileConnection));else controller.close();}}, {highWaterMark:0}),
     {status:mode==='quiescing'?503:422,headers:{'Ehbp-Response-Nonce':ehbp.bytesToHex(nonce)}});
   };
   const result=channel.control('/v1/submissions',{model:'synthetic',new_conversation:false},'b'.repeat(43));
   if(mode==='wrong_envelope'){
    assert.deepEqual(await result,{},'the caller, not a 422 shortcut, rejects a missing submission token');
   }else await assert.rejects(result,error=>{
    if(mode==='quiescing'){
     assert(error instanceof m.GatewayError);assert.equal(error.reason,'service_quiescing');assert.equal(error.status,503);
     assert.equal(error.billing,'unknown');
    }else{
     assert(error instanceof m.DiagnosticFailure);assert.equal(error.stage,mode==='interrupted'?'transport':'submission');assert.equal(error.status,422);
     assert.equal(error.constraint,mode==='complete'?'http':mode==='malformed'?'body':'fetch');
     assert.equal(error.code,'uncertain');
    }
    assert(!error.message.includes(hostileConnection));return true;
   });
  }
  assert.equal(calls,5);
 }finally{globalThis.fetch=originalFetch;}
});
await check('actual Pi reconciliation commands compare exact receipts and balance without inference or persistent reports',async()=>{
 for(const [name,plan,tools,completed] of [['tools',['tool_calls','stop'],true,'2'],['refund-retry',['refund','stop'],false,'2']]) {
  const f=await sdkSetup('sdk-reconcile-'+name,plan,tools);const charged=tools?'14':'7';
  const reads=balancePlan(f.s.client,[balanceSnapshot(),balanceSnapshot((5000000n-BigInt(charged)).toString(),completed)]);
  try {
   await f.session.prompt('/possums-reconcile start');assert.equal(f.s.sends(),0);assert.equal(f.session.messages.length,0);assert.equal(reads(),1);
   assert.match(f.notices.at(-1),/Reconciliation started/);
   await f.session.prompt('Synthetic accounting task');const before=f.session.messages.length;
   await f.session.prompt('/possums-reconcile finish');assert.equal(f.s.sends(),2);assert.equal(reads(),2);assert.equal(f.session.messages.length,before);
   assert.match(f.notices.at(-1),new RegExp('Reconciliation matched: balance debit \\$0\\.0000'+(tools?'14':'07')));
   assert.equal(f.session.sessionManager.getSessionFile(),undefined);
   assert(!JSON.stringify(f.session.sessionManager.getEntries()).includes('Reconciliation matched'));
  } finally {f.session.dispose();}
 }
});
await check('reconciliation keeps u64 precision, detects unrelated activity, unknown charges and mismatches',async()=>{
 const cases=[
  {name:'wide',before:balanceSnapshot('18446744073709551615','9007199254740993'),after:balanceSnapshot('18446744073709551608','9007199254740994'),plan:['stop'],expected:/Reconciliation matched/},
  {name:'outside',after:balanceSnapshot('4999993','2'),plan:['stop'],expected:/Reconciliation unavailable/},
  {name:'unknown',after:balanceSnapshot('4999993','2'),plan:['unknown_bill','stop'],expected:/Reconciliation unavailable/},
  {name:'reset',before:balanceSnapshot('5000000','10'),after:balanceSnapshot('4999993','0'),plan:['stop'],expected:/Reconciliation unavailable/},
  {name:'mismatch',after:balanceSnapshot('4999994','1'),plan:['stop'],expected:/Reconciliation mismatch/},
 ];
 for(const c of cases) {
  const s=await setup(c.plan);balancePlan(s.client,[c.before??balanceSnapshot(),c.after]);const ctx=reconciliationContext(s);
  await s.provider.reconcile('start',ctx);s.provider.beginRun();for(const _ of c.plan)await drain(s.provider.streamSimple(s.selected,context(false)));
  assert.match(await s.provider.reconcile('finish',ctx),c.expected);
 }
});
await check('reconciliation pending, busy, cancel, session replacement and failed reads never infer or claim a match',async()=>{
 const s=await setup(['stop']);const ctx=reconciliationContext(s);let reads=balancePlan(s.client,[balanceSnapshot('5000000','0',1)]);
 await assert.rejects(()=>s.provider.reconcile('start',{...ctx,isIdle:()=>false}),/possums_reconcile_busy/);assert.equal(reads(),0);
 await assert.rejects(()=>s.provider.reconcile('start',ctx),/possums_reconcile_pending/);assert.equal(reads(),1);assert.equal(s.sends(),0);
 reads=balancePlan(s.client,[balanceSnapshot(),balanceSnapshot('4999948','0',1),balanceSnapshot('4999993','1')]);
 await s.provider.reconcile('start',ctx);await assert.rejects(()=>s.provider.reconcile('start',ctx),/possums_reconcile_started/);assert.equal(reads(),1);
 await assert.rejects(()=>s.provider.reconcile('finish',ctx),/possums_reconcile_pending/);assert.equal(reads(),2);
 s.provider.beginRun();await drain(s.provider.streamSimple(s.selected,context(false)));assert.match(await s.provider.reconcile('finish',ctx),/Reconciliation matched/);
 balancePlan(s.client,[balanceSnapshot()]);await s.provider.reconcile('start',ctx);assert.match(await s.provider.reconcile('cancel',ctx),/discarded/);
 await assert.rejects(()=>s.provider.reconcile('finish',ctx),/possums_reconcile_not_started/);
 balancePlan(s.client,[balanceSnapshot()]);await s.provider.reconcile('start',ctx);s.provider.newSession();
 await assert.rejects(()=>s.provider.reconcile('finish',ctx),/possums_reconcile_not_started/);
 assert.equal(s.sends(),1);
 const race=await setup([]),entered=deferred(),gate=deferred(),raceCtx=reconciliationContext(race);
 race.client.balance=async()=>{entered.resolve();await gate.promise;return balanceSnapshot();};
 const pending=race.provider.reconcile('start',raceCtx);await entered.promise;
 await race.provider.reconcile('cancel',raceCtx);gate.resolve();
 await assert.rejects(()=>pending,/possums_reconcile_scope_changed/);
 await assert.rejects(()=>race.provider.reconcile('finish',raceCtx),/possums_reconcile_not_started/);assert.equal(race.sends(),0);
 const f=await sdkSetup('sdk-reconcile-hostile',[],false);
 f.s.client.balance=async()=>{throw new m.BalanceFailure('request');};
 try {await f.session.prompt('/possums-reconcile start');assert.equal(f.s.sends(),0);assert.equal(f.session.messages.length,0);assert.match(f.notices.at(-1),/possums_balance_request_failed/);}
 finally {f.session.dispose();}
});
await check('trust starts closed, caches failed verification and never refreshes on login/logout/catalog within a session',async()=>{
 const fixture=await authFixture([]);let fail=true,checks=0,prompts=0;
 const provider=new bundle.PossumsProvider(async()=>{checks++;if(fail)throw new m.ConnectionFailure('verification_failed');return fixture.establish();});
 const input={...interaction(),prompt:async()=>{prompts++;return recoveryKey;}};
 await assert.rejects(()=>provider.auth.apiKey.login(input),error=>error instanceof m.ConnectionFailure&&error.code==='session_unavailable');
 assert.equal(checks,0);assert.equal(prompts,0);
 provider.newSession();
 await assert.rejects(()=>provider.verifySession(),error=>error instanceof m.ConnectionFailure&&error.code==='verification_failed');
 fail=false;
 for(let i=0;i<2;i++){
  provider.logout();
  await assert.rejects(()=>provider.auth.apiKey.login(input),error=>error instanceof m.ConnectionFailure&&error.code==='verification_failed');
  assert.deepEqual(provider.getModels(),[]);
 }
 assert.equal(checks,1);assert.equal(prompts,0);assert.equal(fixture.trace.keys.length,0);
 provider.newSession();await provider.verifySession();
 await provider.auth.apiKey.login(input);provider.logout();await provider.auth.apiKey.login(input);
 assert.equal(checks,2);assert.equal(prompts,2);assert.equal(fixture.trace.keys.length,2);
 provider.shutdown();
 await assert.rejects(()=>provider.verifySession(),error=>error instanceof Error&&error.message==='possums_run_replaced');
 assert.equal(checks,2);
});
await check('actual Pi runtime new/resume verifies once; stopped A never refreshes or replays and new session selects B',async()=>{
 const sources={A:await setup(['stop'],false),B:await setup(['stop'],false)};
 for(const [tag,source] of Object.entries(sources))Object.defineProperty(source.client.channel,'release',
  {value:Object.freeze({tag,expires:Date.now()+12*60*60*1000})});
 let active='A',available=true,provider;
 const verifications=[],attempts=[],starts=[];
 const credentials=new ai.InMemoryCredentialStore();
 await credentials.modify('possums',async()=>({type:'api_key',key:recoveryKey}));
 const cwd=path.join(root,'sdk-release-lifecycle'),agentDir=path.join(cwd,'agent');
 fs.mkdirSync(cwd,{recursive:true});
 const createRuntime=async({sessionManager,sessionStartEvent})=>{
  const modelRuntime=await coding.ModelRuntime.create({credentials,modelsStore:new ai.InMemoryModelsStore(),modelsPath:null,allowModelNetwork:false,refreshOnCreate:false});
  const services=await coding.createAgentSessionServices({cwd,agentDir,modelRuntime,
   settingsManager:coding.SettingsManager.inMemory({defaultTools:[],compaction:{enabled:false},retry:{enabled:true,maxRetries:2,baseDelayMs:1}}),
   resourceLoaderOptions:{noExtensions:true,noSkills:true,noPromptTemplates:true,noThemes:true,
    skillsOverride:()=>({skills:[],diagnostics:[]}),
    promptsOverride:()=>({prompts:[],diagnostics:[]}),themesOverride:()=>({themes:[],diagnostics:[]}),agentsFilesOverride:()=>({agentsFiles:[]}),
    extensionFactories:[pi=>{
     const register=pi.registerProvider.bind(pi);
     pi.registerProvider=value=>{provider=value;value.establish=async()=>{
      const tag=active;verifications.push(tag);
      if(!available)throw new m.ConnectionFailure('evidence_unavailable');
      const source=sources[tag],candidate=new m.ReferenceClient(source.client.channel);
      candidate.login=async()=>{};
      candidate.models=async()=>(await source.client.models()).map(item=>({...item,context_tokens:'64000'}));
      candidate.chat=(...args)=>{attempts.push(tag);if(tag!==active||!available)throw new m.ChannelError('uncertain');return source.client.chat(...args);};
      candidate.freshSession=()=>freshFixture(candidate);return candidate;
     };register(value);};
     m.extension(pi);pi.on('session_start',event=>{starts.push(event.reason);});
    }]}});
  const result=await coding.createAgentSessionFromServices({services,sessionManager,sessionStartEvent,
   model:{...sources.A.selected,contextWindow:64000},thinkingLevel:'off',tools:[]});
  return {...result,services,diagnostics:services.diagnostics};
 };
 const host=await coding.createAgentSessionRuntime(createRuntime,{cwd,agentDir,
  sessionManager:coding.SessionManager.create(cwd,path.join(cwd,'sessions'))});
 host.setRebindSession(session=>session.bindExtensions({mode:'print'}));
 try{
  await host.session.bindExtensions({mode:'print'});
  assert.deepEqual(verifications,['A']);assert.equal(provider.release.tag,'A');
  await host.session.prompt('Synthetic A request');assert.deepEqual(attempts,['A']);
  const original=host.session.sessionFile,oldSession=host.session;
  assert(original);
  await host.services.modelRuntime.logout('possums');
  await host.services.modelRuntime.login('possums','api_key',interaction(envKey));
  assert.deepEqual(verifications,['A'],'account replacement reuses fixed public trust');
  active='B';available=false;
  await host.session.prompt('Synthetic stopped A request');
  assert.deepEqual(attempts,['A','A']);assert.deepEqual(verifications,['A']);
  assert.match(host.session.messages.at(-1).errorMessage,/charge unknown.*Not replayed/);
  available=true;
  assert.deepEqual(await host.newSession(),{cancelled:false});
  assert.notEqual(host.session,oldSession);assert.deepEqual(verifications,['A','B']);
  assert.equal(provider.release.tag,'B');
  await host.session.prompt('Synthetic B request');assert.deepEqual(attempts,['A','A','B']);
  assert.deepEqual(await host.switchSession(original),{cancelled:false});
  assert.deepEqual(verifications,['A','B','B']);assert.equal(provider.release.tag,'B');
  assert(starts.includes('new')&&starts.includes('resume'));
  assert.deepEqual(attempts,['A','A','B'],'resume never replays interrupted A generation');
 }finally{await host.dispose();}
});
await check('actual Pi SDK and shipped extension hooks run one receipted invocation per model turn',async()=>{
 const {session,s,toolRuns}=await sdkSetup('sdk-tools',['tool_calls','stop'],true);
 try{await session.prompt('Synthetic tool task');assert.equal(toolRuns(),1,session.messages.filter(value=>value.role==='assistant').at(-1)?.errorMessage);assert.equal(s.sends(),2);assert.equal(s.requests[1].messages.find(message=>message.role==='assistant').content,'');assert.equal(session.messages.filter(value=>value.role==='assistant').at(-1).content.find(value=>value.type==='text').text,'progressive');assert.equal(session.sessionManager.getSessionFile(),undefined);}
 finally{session.dispose();}
});
await check('actual Pi SDK accepts steering during held text and tool responses, and queued follow-up',async()=>{
 for(const [mode,method] of [['held','steer'],['held_tools','steer'],['held','followUp']]) {
  const f=await sdkSetup('sdk-'+method+'-'+mode,[mode,'stop'],mode==='held_tools');
  const retries=[];let queued;
  const unsubscribe=f.session.subscribe(event=>{
   if(event.type==='auto_retry_start')retries.push(event);
   if(event.type==='message_update' && event.assistantMessageEvent.type===(mode==='held_tools'?'toolcall_delta':'text_delta') && !queued) {
    queued=f.session[method]('Synthetic queued '+method).finally(()=>f.s.release());
   }
  });
  try {
   await f.session.prompt('Synthetic held task');await queued;assert(queued);
   assert.equal(f.s.sends(),2);assert.equal(f.toolRuns(),mode==='held_tools'?1:0);assert.equal(retries.length,0);
   assert(f.s.requests[1].messages.some(message=>message.role==='user'&&message.content.includes('Synthetic queued '+method)));
   if(mode==='held_tools')assert(f.s.requests[1].messages.some(message=>message.role==='tool'&&message.tool_call_id==='call_one'));
   assert.equal(f.session.messages.at(-1).stopReason,'stop');
  } finally {unsubscribe();f.session.dispose();}
 }
});
const rawAssistants = session => session.sessionManager.getEntries().filter(entry=>entry.type==='message'&&entry.message.role==='assistant').map(entry=>entry.message);
await check('Pi presents quiescing without refund or native retry',async()=>{
 const f=await sdkSetup('sdk-quiescing',['quiescing','stop'],false);
 const events=[];const unsubscribe=f.session.subscribe(event=>{if(event.type.startsWith('auto_retry_'))events.push(event);});
 try {
  await f.session.prompt('Synthetic quiescing request');
  assert.equal(f.s.sends(),1);assert.deepEqual(events,[]);
  const message=f.session.messages.at(-1);
  assert.match(message.errorMessage,/\[service_quiescing\].*Stage: admission; constraint: service_quiescing.*Charge unknown.*Not replayed/);
  assert(!message.errorMessage.includes('PRIVATE_PROMPT'));
  assert(!message.errorMessage.includes('Reservation refunded'));
  assert.equal(message.diagnostics?.some(d=>d.type==='possums_reservation_refunded')??false,false);
 } finally {unsubscribe();f.session.dispose();}
});
await check('actual Pi SDK retries validated transient failures with native events and retains raw diagnostics',async()=>{
 for(const mode of ['refund','unknown_bill']) {
  const f=await sdkSetup('sdk-retry-'+mode,[mode,'stop'],false);const events=[];
  const unsubscribe=f.session.subscribe(event=>{if(event.type.startsWith('auto_retry_'))events.push(event);});
  try {
   await f.session.prompt('Synthetic transient task');assert.equal(f.s.sends(),2);
   assert.deepEqual(events.map(event=>event.type),['auto_retry_start','auto_retry_end']);
   assert.equal(events[0].attempt,1);assert.equal(events[0].maxAttempts,3);assert.equal(events[1].success,true);
   const raw=rawAssistants(f.session);assert.equal(raw.length,2);assert.equal(raw[0].stopReason,'error');assert.equal(raw[1].stopReason,'stop');
   assert.match(raw[0].errorMessage,/Transient service unavailable.*Native automatic retry may incur another charge/);
   assert.match(raw[0].errorMessage,mode==='refund'?/\[generation_failed\].*\[stream_idle_timeout\].*Reservation refunded/:/\[unavailable\].*\[inference_unavailable\].*Charge unknown/);
   assert.equal(raw[0].diagnostics?.some(d=>d.type==='possums_billing_unknown')??false,mode==='unknown_bill');
   assert.equal(raw[0].diagnostics?.some(d=>d.type==='possums_reservation_refunded')??false,mode==='refund');
   assert(!JSON.stringify(raw).includes('PRIVATE_PROMPT'));
   assert.equal(f.session.messages.filter(message=>message.role==='assistant').length,1,'native projection omits the failed attempt, not raw history');
   assert.deepEqual(f.s.requests[0].messages,f.s.requests[1].messages);
  } finally {unsubscribe();f.session.dispose();}
 }
});
await check('actual Pi SDK exhausts exactly the default three retries and resets budget after success',async()=>{
 const f=await sdkSetup('sdk-retry-budget',[...Array(4).fill('unknown_bill'),'refund','stop','refund','stop'],false);const events=[];
 const unsubscribe=f.session.subscribe(event=>{if(event.type.startsWith('auto_retry_'))events.push(event);});
 try {
  assert.equal(coding.SettingsManager.inMemory().getRetrySettings().maxRetries,3);
  assert.equal(coding.SettingsManager.inMemory().getRetrySettings().baseDelayMs,2000);
  assert.deepEqual([1,2,3].map(attempt=>retryDelayMs(coding.SettingsManager.inMemory().getRetrySettings(),attempt)),[2000,4000,8000]);
  assert.equal(f.settings.getRetrySettings().maxRetries,3);
  await f.session.prompt('Synthetic exhausted task');assert.equal(f.s.sends(),4);
  assert.deepEqual(events.filter(event=>event.type==='auto_retry_start').map(event=>event.attempt),[1,2,3]);
  assert.deepEqual(events.filter(event=>event.type==='auto_retry_start').map(event=>event.delayMs),[1,2,4]);
  assert(events.filter(event=>event.type==='auto_retry_start').every(event=>event.maxAttempts===3));
  assert.equal(events.at(-1).type,'auto_retry_end');assert.equal(events.at(-1).success,false);assert.equal(events.at(-1).attempt,3);
  assert.equal(rawAssistants(f.session).length,4);
  assert(rawAssistants(f.session).every(message=>message.stopReason==='error'&&message.diagnostics.some(d=>d.type==='possums_billing_unknown')));
  assert.match(f.session.messages.at(-1).errorMessage,/Charge unknown.*another charge/);
  for(let i=0;i<2;i++) {
   events.length=0;await f.session.prompt('Synthetic fresh task '+i);assert.equal(f.s.sends(),6+i*2);
   assert.deepEqual(events.filter(event=>event.type==='auto_retry_start').map(event=>event.attempt),[1]);assert.equal(events.at(-1).success,true);
  }
 } finally {unsubscribe();f.session.dispose();}
});
await check('actual Pi SDK aborts native retry backoff without another submission',async()=>{
 // Only the synthetic fixture changes delay; shipped extension never changes settings.
 const f=await sdkSetup('sdk-retry-abort',['unknown_bill','stop'],false,false,true,false,{baseDelayMs:1000,maxAgentDelayMs:1000});const events=[];let abort;
 const unsubscribe=f.session.subscribe(event=>{
  if(event.type.startsWith('auto_retry_'))events.push(event);
  if(event.type==='auto_retry_start')abort=new Promise(resolve=>setImmediate(()=>resolve(f.session.abort())));
 });
 try {
  await f.session.prompt('Synthetic abort during backoff');await abort;assert(abort);
  assert.equal(f.s.sends(),1);assert.deepEqual(events.map(event=>event.type),['auto_retry_start','auto_retry_end']);
  assert.equal(events.at(-1).success,false);assert.equal(events.at(-1).attempt,1);
  const raw=rawAssistants(f.session);assert.equal(raw.length,1);assert.match(raw[0].errorMessage,/Charge unknown.*another charge/);
  assert(raw[0].diagnostics.some(d=>d.type==='possums_billing_unknown'));
 } finally {unsubscribe();f.session.dispose();}
});
await check('actual Pi SDK never executes incomplete tool fragments from a transient failed attempt',async()=>{
 const f=await sdkSetup('sdk-retry-fragments',['fragmented_tools_transient','stop'],true);const events=[];
 const unsubscribe=f.session.subscribe(event=>{if(event.type.startsWith('auto_retry_'))events.push(event);});
 try {
  await f.session.prompt('Synthetic interrupted tool task');assert.equal(f.s.sends(),2);assert.equal(f.toolRuns(),0);
  assert.deepEqual(events.map(event=>event.type),['auto_retry_start','auto_retry_end']);assert.equal(events.at(-1).success,true);
  const raw=rawAssistants(f.session);assert.equal(raw[0].stopReason,'error');assert(raw[0].content.some(block=>block.type==='toolCall'));
  assert(raw[0].diagnostics.some(d=>d.type==='possums_billing_unknown'));assert(!raw[0].diagnostics.some(d=>d.type==='possums_settled_receipt'));
  assert.equal(f.s.requests[1].messages.some(message=>message.role==='tool'||message.tool_calls),false);
  assert(!JSON.stringify(raw).includes('PRIVATE_PROMPT'));
 } finally {unsubscribe();f.session.dispose();}
});
await check('actual Pi SDK completes split identity/arguments and late identity through the real client decoder',async()=>{
 for(const mode of ['fragmented_tools','late_fragmented_tools']){
  const {session,s,toolRuns}=await sdkSetup('sdk-'+mode,[mode,'stop'],true);
  try{await session.prompt('Synthetic fragmented tool test');assert.equal(toolRuns(),1);assert.equal(s.sends(),2);
   const messages=session.messages.filter(value=>value.role==='assistant');
   assert.equal(messages[0].stopReason,'toolUse');assert(messages[0].diagnostics.some(d=>d.type==='possums_settled_receipt'));
   assert.deepEqual(messages[0].content.find(value=>value.type==='toolCall').arguments,{value:'ok'});
   assert.equal(messages.at(-1).stopReason,'stop');
  }finally{session.dispose();}
 }
});
await check('actual Pi SDK cannot execute or replay split tool fragments without a terminal receipt',async()=>{
 const {session,s,toolRuns}=await sdkSetup('sdk-fragmented-uncertain',['fragmented_tools_uncertain'],true);
 try{await session.prompt('Synthetic incomplete fragmented tool test');assert.equal(toolRuns(),0);assert.equal(s.sends(),1);
  const last=session.messages.filter(value=>value.role==='assistant').at(-1);
  assert.equal(last.stopReason,'error');assert(last.diagnostics.some(d=>d.type==='possums_billing_unknown'));
  assert(!last.diagnostics.some(d=>d.type==='possums_settled_receipt'));
 }finally{session.dispose();}
});
await check('actual Pi SDK native logout prevents a pending tool from executing and preserves its charge',async()=>{
 const {session,s,runtime,toolRuns}=await sdkSetup('sdk-native-logout',['held_tools'],true);
 let logout;
 const unsubscribe=session.subscribe(event=>{
  if(event.type==='message_update'&&event.assistantMessageEvent.type==='toolcall_delta'&&!logout) {
   logout=runtime.logout('possums').finally(()=>s.release());
  }
 });
 try {
  await session.prompt('Synthetic pending logout');await logout;assert(logout);
  assert.equal(toolRuns(),0);assert.equal(s.sends(),1);
  const result=session.messages.filter(value=>value.role==='assistant').at(-1);
  assert.equal(result.stopReason,'error');assert.equal(result.usage.cost.total,7/1e6);
  assert.equal(result.diagnostics[0].details.finish,'tool_calls');
  assert.deepEqual(await runtime.listCredentials(),[]);
 } finally {unsubscribe();session.dispose();}
});
await check('actual Pi SDK executes stop-with-tool once and continues with its matching result',async()=>{
 const {session,s,toolRuns}=await sdkSetup('sdk-stop-tools',['stop_tools','stop'],true);
 try {await session.prompt('Synthetic stop-tool task');assert.equal(toolRuns(),1);assert.equal(s.sends(),2);
  const assistants=session.messages.filter(value=>value.role==='assistant');
  assert.equal(assistants[0].stopReason,'toolUse');assert.equal(assistants[0].diagnostics[0].details.finish,'stop');
  assert.equal(session.messages.find(value=>value.role==='toolResult')?.toolCallId,'call_one');
 } finally {session.dispose();}
});
await check('actual Pi SDK does not execute, retry or replay partial length-tool calls',async()=>{
 const {session,s,toolRuns}=await sdkSetup('sdk-length-partial',['length_partial'],true,true);
 try {await session.prompt('Synthetic partial tool task');assert.equal(toolRuns(),0);assert.equal(s.sends(),1);
  const result=session.messages.filter(value=>value.role==='assistant').at(-1);
  assert.equal(result.stopReason,'error');assert.equal(result.diagnostics[0].details.finish,'length');assert.equal(result.usage.cost.total,7/1e6);
  assert.match(result.errorMessage,/no tool executed.*Charge settled.*Not automatically continued/);
 } finally {session.dispose();}
});
await check('default Pi retry and compaction cannot replay an uncertain invocation',async()=>{
 const {session,s}=await sdkSetup('sdk-uncertain',['uncertain'],false,true);
 try{await session.prompt('Synthetic uncertain task');assert.equal(s.sends(),1);assert.match(session.messages.filter(value=>value.role==='assistant').at(-1).errorMessage,/charge unknown/);}
 finally{session.dispose();}
});
await check('actual Pi SDK leaves auth, credit, validation, integrity, settlement and hostile failures terminal',async()=>{
 const cases=['hostile',...['unauthorized','insufficient_credit','invalid_request','account_limit','duplicate_request'].map(code=>({code,detail:'stream_idle_timeout'})),
  ...['request_encoding_failed','verification_failed','tool_identity_invalid','stream_usage_invalid','settlement_failed'].map(detail=>({code:'generation_failed',detail}))];
 for(const [index,mode] of cases.entries()) {
  const f=await sdkSetup('sdk-terminal-'+index,[mode,'stop'],true);const events=[];
  const unsubscribe=f.session.subscribe(event=>{if(event.type.startsWith('auto_retry_'))events.push(event);});
  try {
   await f.session.prompt('Synthetic terminal failure');assert.equal(f.s.sends(),1);assert.equal(f.toolRuns(),0);assert.deepEqual(events,[]);
   assert.equal(f.session.messages.at(-1).stopReason,'error');assert(!JSON.stringify(rawAssistants(f.session)).includes('PRIVATE_PROMPT'));
  } finally {unsubscribe();f.session.dispose();}
 }
});
await check('actual Pi SDK does not retry or compact after detailed gateway decode failure',async()=>{
 const {session,s,toolRuns}=await sdkSetup('sdk-detailed-error',['sdk_decode'],true,true);
 try {await session.prompt('Synthetic decode failure');const result=session.messages.filter(value=>value.role==='assistant').at(-1);
  assert.equal(s.sends(),1);assert.equal(toolRuns(),0);
  assert.match(result.errorMessage,/Generation failed.*sdk stream decode failed.*Charge unknown.*Not replayed/);
  assert.equal(result.diagnostics?.[0]?.type,'possums_billing_unknown');
 } finally {session.dispose();}
});
await check('terminal partial mapping retains charged output without Pi early-length regeneration',async()=>{
 const {session,s}=await sdkSetup('sdk-length',['stop','length'],false,true);
 try{await session.prompt('Synthetic previous input '.repeat(100));await session.prompt('Synthetic next input');assert.equal(s.sends(),2);const assistants=session.messages.filter(value=>value.role==='assistant');assert.equal(assistants.length,2);assert.equal(assistants.at(-1).stopReason,'error');assert.equal(assistants.at(-1).diagnostics[0].details.finish,'length');assert.equal(session.getSessionStats().cost,14/1e6);}
 finally{session.dispose();}
});
await check('text-only command succeeds in isolation; a later tool-restoring extension fails before invocation',async()=>{
 for(const restore of [true,false]){
  const {session,s,toolRuns,textOnly}=await sdkSetup('sdk-text-only-'+restore,['stop'],true,false,false,restore);
  try{await textOnly();assert.deepEqual(session.getActiveToolNames(),[]);await session.prompt('Synthetic text-only input');const result=session.messages.filter(value=>value.role==='assistant').at(-1);
   assert.equal(toolRuns(),0);assert.equal(s.sends(),restore?0:1);assert.equal(result.stopReason,restore?'error':'stop');if(restore)assert.match(result.errorMessage,/\[possums_tools_unsupported\].*qualified for tools/);
  }finally{session.dispose();}
 }
});

const syntheticAssistant = (content = [{type:'text',text:'Synthetic answer'}]) => ({role:'assistant',provider:'possums',api:'openai-completions',model:'synthetic',timestamp:2,content,stopReason:'stop',usage:{input:2,output:3,cacheRead:0,cacheWrite:0,totalTokens:5,cost:{input:0,output:0,cacheRead:0,cacheWrite:0,total:0}}});
function seedCompaction(manager, split = false, previous = false) {
 const id=manager.appendMessage({...user,content:'Synthetic old request'});
 if(previous)manager.appendCompaction('SYNTHETIC PREVIOUS SUMMARY',id,100,{readFiles:['prior.txt'],modifiedFiles:[]});
 for(const [name,file] of [['read','read.txt'],['write','written.txt']]) {
  manager.appendMessage({...syntheticAssistant([{type:'toolCall',id:'file_'+name,name,arguments:{path:file,content:'Synthetic file'}}]),stopReason:'toolUse'});
  manager.appendMessage({role:'toolResult',toolCallId:'file_'+name,toolName:name,content:[{type:'text',text:'Synthetic result'}],timestamp:3,isError:false});
 }
 manager.appendMessage(syntheticAssistant());
 manager.appendMessage({...user,content:'Synthetic recent request '.repeat(20)});
 if(split)manager.appendMessage(syntheticAssistant([{type:'text',text:'Synthetic retained answer '.repeat(20)}]));
 return prepareCompaction(manager.getBranch(),{enabled:true,reserveTokens:10,keepRecentTokens:4});
}
async function compactionFixture(plan, split = false, previous = false) {
 const fixture=await authFixture(plan), extension=fixtureExtension(fixture.establish);
 const runtime=await nativeRuntime(extension.provider,new ai.InMemoryCredentialStore());
 await extension.handlers.get('session_start')({}, {modelRegistry:new coding.ModelRegistry(runtime)});
 await runtime.login('possums','api_key',interaction());
 const manager=coding.SessionManager.inMemory(root), preparation=seedCompaction(manager,split,previous),controller=new AbortController(),notices=[];
 assert(preparation);assert.equal(preparation.isSplitTurn,split);
 const ctx={model:extension.provider.getModels()[0],modelRegistry:new coding.ModelRegistry(runtime),thinkingLevel:'off',ui:{notify:text=>notices.push(text)}};
 const event={type:'session_before_compact',preparation,branchEntries:manager.getBranch(),reason:'manual',willRetry:false,signal:controller.signal,customInstructions:'Synthetic focus'};
 return {...fixture,...extension,runtime,manager,event,ctx,controller,notices,run:()=>extension.handlers.get('session_before_compact')(event,ctx)};
}
await check('reconciliation includes paid native compaction summaries without reading or persisting summary content',async()=>{
 const f=await compactionFixture(['stop','stop'],true);const ctx={...f.ctx,isIdle:()=>true,signal:f.controller.signal};
 balancePlan(f.s.client,[balanceSnapshot(),balanceSnapshot('4999986','2')]);
 await f.provider.reconcile('start',ctx);const result=await f.run();assert(result.compaction);
 assert.match(await f.provider.reconcile('finish',ctx),/Reconciliation matched: balance debit \$0\.000014/);assert.equal(f.s.sends(),2);
});
await check('native one/two-summary prompts, previous checkpoint, files, advisory hints and combined usage',async()=>{
 for(const split of [false,true]) {
  const f=await compactionFixture(['stop','stop'],split,true),nativeRequests=[],hints=[];
  const expected=await coding.compact(f.event.preparation,f.ctx.model,requestMarker,{},f.event.customInstructions,f.event.signal,'off',async(model,context,options)=>{
   nativeRequests.push(structuredClone(m.invocation(model,context).messages));hints.push(options.maxTokens);
   const stream=ai.createAssistantMessageEventStream(),message={...syntheticAssistant([{type:'text',text:'progressive'}]),usage:{...syntheticAssistant().usage,cost:{input:2/1e6,output:7/1e6-2/1e6,cacheRead:0,cacheWrite:0,total:7/1e6}}};
   stream.push({type:'done',reason:'stop',message});stream.end();return stream;
  });
  const result=await f.run();assert.deepEqual(result.compaction,expected);assert.equal(f.s.sends(),split?2:1);
  assert.deepEqual(f.s.requests.map(r=>r.messages),nativeRequests);assert.deepEqual(hints,split?[8,5]:[8]);
  assert(nativeRequests[0].some(msg=>msg.content.includes('<previous-summary>\nSYNTHETIC PREVIOUS SUMMARY')));
  assert(nativeRequests[0].some(msg=>msg.content.includes('Additional focus: Synthetic focus')));
  assert.deepEqual(result.compaction.details,{readFiles:['prior.txt','read.txt'],modifiedFiles:['written.txt']});
  assert.equal(result.compaction.usage.cost.total,(split?14:7)/1e6);assert.equal(result.compaction.usage.totalTokens,split?10:5);
  assert(f.s.requests.every(r=>!r.tools && r.newConversation));assert.deepEqual(f.notices,[]);
  f.provider.beginRun();const ordinary=await drain(f.provider.streamSimple(f.ctx.model,context(false),{maxTokens:8}));
  assert.match(ordinary.message.errorMessage,/possums_request_options_unsupported/);assert.equal(f.s.sends(),split?2:1);
 }
});
await check('summary success/failure preserves native continuation and ordinary conversation state',async()=>{
 for(const summary of ['stop','refund']) {
  const ready=await compactionFixture([summary,'stop']);ready.provider.beginRun();await ready.run();
  assert.equal((await drain(ready.provider.streamSimple(ready.ctx.model,context(false)))).message.stopReason,'stop');
  assert.deepEqual(ready.s.requests.map(r=>r.newConversation),[true,true]);
  const f=await compactionFixture(['tool_calls',summary,'stop','stop']);f.provider.beginRun();
  const first=(await drain(f.provider.streamSimple(f.ctx.model,context(true)))).message;
  assert.equal(first.stopReason,'toolUse');await f.run();
  const continuation=ai.normalizeContext({messages:[user,first,{role:'toolResult',toolCallId:'call_one',toolName:'echo',content:[{type:'text',text:'ok'}],timestamp:3}],tools:[tool]});
  assert.equal((await drain(f.provider.streamSimple(f.ctx.model,continuation))).message.stopReason,'stop');
  assert.equal((await drain(f.provider.streamSimple(f.ctx.model,continuation))).message.stopReason,'stop');
  assert.deepEqual(f.s.requests.map(r=>r.newConversation),[true,true,false,false]);assert.equal(f.s.sends(),4);
 }
});
await check('summary errors, length, empty text and tool attempts fail closed with safe stages and no replay',async()=>{
 for(const [mode,pattern] of [['refund',/Generation failed.*stream idle timeout.*Reservation refunded/],['unknown_bill',/Gateway unavailable.*inference unavailable.*Charge unknown/],['sdk_decode',/sdk stream decode failed/],['hostile',/possums_provider_unexpected/],['length',/possums_summary_length/],['empty',/possums_summary_empty/],['tool_calls',/possums_summary_tools/],['stop_tools',/possums_summary_tools/]]) {
  const f=await compactionFixture([mode,'stop'],true),observed=[];const perform=f.provider.perform.bind(f.provider);
  f.provider.perform=(...args)=>{const stream=perform(...args);observed.push(drain(stream));return stream;};
  const result=await f.run();
  assert((await Promise.all(observed)).every(({events})=>events.every(event=>!event.type.startsWith('toolcall_'))));
  assert.deepEqual(result,{cancel:true});assert.equal(f.s.sends(),1);assert.match(f.notices[0],/summary 1/);assert.match(f.notices[0],pattern);
  assert(!f.notices.join('').includes('PRIVATE_PROMPT'));assert.match(f.notices[0],/No checkpoint saved.*Not replayed/);
  if(['length','empty','tool_calls','stop_tools'].includes(mode))assert.match(f.notices[0],/Observed settled charges: \$0.000007/);
  assert.equal((await drain(f.provider.streamSimple(f.ctx.model,context(false)))).message.stopReason,'stop');assert.equal(f.s.sends(),2);
 }
});
await check('failed second summary reports first paid summary and observed second-call billing without a ledger',async()=>{
 for(const [mode,amount] of [['refund','0.000007'],['unknown_bill','0.000007'],['length','0.000014']]) {
  const f=await compactionFixture(['stop',mode],true);assert.deepEqual(await f.run(),{cancel:true});assert.equal(f.s.sends(),2);
  assert.match(f.notices[0],/summary 2/);assert(f.notices[0].includes('Observed settled charges: $'+amount));
  if(mode==='unknown_bill')assert.match(f.notices[0],/Additional charge unknown/);
  if(mode==='refund')assert.match(f.notices[0],/stream idle timeout.*Reservation refunded/);
  assert.equal(f.manager.getEntries().filter(e=>e.type==='compaction').length,0);
 }
});
await check('summary abort/session/logout/auth replacement/run replacement rejects late receipt and stops split sends',async()=>{
 for(const action of ['abort','session','logout','replace','run']) {
  const f=await compactionFixture(['held','stop'],true),entered=deferred();const original=f.s.client.chat;
  f.s.client.chat=(model,messages,onDelta,fresh,options)=>original(model,messages,text=>{onDelta(text);entered.resolve();},fresh,options);
  const pending=f.run();await entered.promise;
  if(action==='abort')f.controller.abort();
  if(action==='session')f.provider.newSession();
  if(action==='logout')await f.runtime.logout('possums');
  if(action==='replace')await f.runtime.login('possums','api_key',interaction(envKey));
  if(action==='run')f.provider.beginRun();
  f.s.release();assert.deepEqual(await pending,{cancel:true});assert.equal(f.s.sends(),1);
  assert.match(f.notices[0],/possums_run_replaced.*Observed settled charges: \$0.000007/);
 }
 const f=await compactionFixture(['stop']);f.controller.abort();assert.deepEqual(await f.run(),{cancel:true});assert.equal(f.s.sends(),0);
 const aborted=await compactionFixture(['abort','stop'],true),entered=deferred(),original=aborted.s.client.chat;
 aborted.s.client.chat=(model,messages,onDelta,fresh,options)=>original(model,messages,text=>{onDelta(text);entered.resolve();},fresh,options);
 const pending=aborted.run();await entered.promise;await new Promise(setImmediate);aborted.controller.abort();
 assert.deepEqual(await pending,{cancel:true});assert.equal(aborted.s.sends(),1);assert.match(aborted.notices[0],/charge unknown/);
});
await check('auth wait invalidation and throwing transient UI cannot enable default summary fallback',async()=>{
 for(const action of ['abort','session','logout','replace']) {
  const f=await compactionFixture(['stop']),held=deferred(),entered=deferred(),original=f.ctx.modelRegistry.getProviderAuth.bind(f.ctx.modelRegistry);
  f.ctx.modelRegistry.getProviderAuth=async(...args)=>{const auth=await original(...args);entered.resolve();await held.promise;return auth;};
  const pending=f.run();await entered.promise;
  if(action==='abort')f.controller.abort();if(action==='session')f.provider.newSession();if(action==='logout')await f.runtime.logout('possums');if(action==='replace')await f.runtime.login('possums','api_key',interaction(envKey));
  held.resolve();assert.deepEqual(await pending,{cancel:true});assert.equal(f.s.sends(),0);
 }
 const f=await compactionFixture(['hostile']);f.ctx.ui.notify=()=>{throw new Error('PRIVATE_PROMPT');};
 assert.deepEqual(await f.run(),{cancel:true});assert.equal(f.s.sends(),1);
});
await check('willRetry recovery cancels before authentication; other providers remain untouched',async()=>{
 const f=await compactionFixture(['stop']);let auth=0;f.ctx.modelRegistry.getProviderAuth=async()=>{auth++;throw new Error('PRIVATE_PROMPT');};
 f.event.willRetry=true;f.event.reason='overflow';assert.deepEqual(await f.run(),{cancel:true});assert.match(f.notices[0],/possums_automatic_replay_blocked/);
 assert.equal(auth,0);assert.equal(f.s.sends(),0);
 for(const reason of ['manual','threshold','overflow']){f.ctx.model={...f.ctx.model,provider:'another'};f.event.reason=reason;assert.equal(await f.run(),undefined);}
 assert.equal(auth,0);assert.equal(f.notices.length,1);
});
await check('private summary scope closes after native success or failure',async()=>{
 for(const mode of ['stop','refund']) {
  const f=await compactionFixture([mode]);let scope;const perform=f.provider.perform.bind(f.provider);
  f.provider.perform=(...args)=>{scope=args[3];return perform(...args);};
  await f.run();assert.equal(typeof scope,'function');assert.throws(scope,/possums_run_replaced/);
  assert.equal(f.s.sends(),1);
 }
});
await check('actual Pi overflow-retry hook cancellation prevents summary and replay',async()=>{
 const f=await sdkSetup('sdk-compact-recovery',['stop'],false,true);
 try {
  seedCompaction(f.session.sessionManager,true);const before=f.session.sessionManager.getEntries();
  assert.equal(await f.session._runAutoCompaction('overflow',true),false);
  assert.equal(f.compactions.length,1);assert.equal(f.compactions[0].willRetry,true);assert.equal(f.s.sends(),0);
  assert.deepEqual(f.session.sessionManager.getEntries(),before);assert.match(f.notices[0],/possums_automatic_replay_blocked/);
 } finally {f.session.dispose();}
});
await check('actual Pi manual compaction checkpoints native summaries/files/usage and prunes via native projection',async()=>{
 for(const split of [false,true]) {
  const f=await sdkSetup('sdk-compact-'+split,['stop','stop'],true);
  try {
   seedCompaction(f.session.sessionManager,split,true);const before=f.session.sessionManager.getBranch();
   const result=await f.session.compact('Synthetic focus');const saved=f.session.sessionManager.getEntries().filter(e=>e.type==='compaction').at(-1);
   assert.equal(f.compactions.length,1);assert.equal(f.compactions[0].reason,'manual');assert.equal(f.compactions[0].willRetry,false);
   assert.equal(f.s.sends(),split?2:1);assert.equal(saved.summary,result.summary);assert.equal(saved.firstKeptEntryId,f.compactions[0].preparation.firstKeptEntryId);
   assert.equal(result.usage.cost.total,(split?14:7)/1e6);assert.deepEqual(result.details,{readFiles:['prior.txt','read.txt'],modifiedFiles:['written.txt']});
   assert.equal(f.session.getSessionStats().cost,(split?14:7)/1e6);assert.equal(f.toolRuns(),0);assert.equal(f.notices.length,0);
   const projected=f.session.sessionManager.buildSessionProjection().messages;
   assert(projected.some(msg=>msg.role==='compactionSummary'));assert(!JSON.stringify(projected).includes('Synthetic old request'));
   assert(f.session.sessionManager.getEntries().length>before.length,'raw native history retained; projection is pruned');
  } finally {f.session.dispose();}
 }
});
await check('actual Pi failed native compaction never saves checkpoint or persists transient diagnostics',async()=>{
 for(const mode of ['refund','length','empty','tool_calls','abort']) {
  const f=await sdkSetup('sdk-compact-failure-'+mode,['stop',mode],true);
  try {
   seedCompaction(f.session.sessionManager,true);const before=f.session.sessionManager.getEntries();let cancel;
   if(mode==='abort') {const original=f.s.client.chat;f.s.client.chat=(model,messages,onDelta,fresh,options)=>original(model,messages,text=>{onDelta(text);if(f.s.sends()===2)cancel=setImmediate(()=>f.session.abortCompaction());},fresh,options);}
   await assert.rejects(f.session.compact(),/Compaction cancelled/);if(cancel)clearImmediate(cancel);
   assert.equal(f.s.sends(),2);assert.deepEqual(f.session.sessionManager.getEntries(),before);assert.equal(f.toolRuns(),0);
   assert.equal(f.notices.length,1);assert.match(f.notices[0],/summary 2.*Observed settled charges: \$0.0000(07|14)/);
  } finally {f.session.dispose();}
 }
});
await check('actual Pi threshold compaction is native and never retries a completed turn',async()=>{
 const f=await sdkSetup('sdk-compact-threshold',['stop','stop','stop'],false,true);
 try {
  // Synthetic usage crosses the host threshold, not the mock gateway's window.
  const original=f.s.client.chat;f.s.client.chat=async(...args)=>{const receipt=await original(...args);return f.s.sends()===2?{...receipt,inputTokens:50000,totalTokens:50003,chargedMicrounits:'65004',refundedMicrounits:'0'}:receipt;};
  await f.session.prompt('Synthetic prior threshold input');
  await f.session.prompt('Synthetic threshold request '.repeat(100));
  assert.equal(f.compactions.length,1);assert.equal(f.compactions[0].reason,'threshold');assert.equal(f.compactions[0].willRetry,false);
  assert.equal(f.s.sends(),3);assert.equal(f.session.sessionManager.getEntries().filter(e=>e.type==='compaction').length,1);
  assert.equal(f.session.messages.filter(e=>e.role==='assistant').length,1);assert.equal(f.notices.length,0);
 } finally {f.session.dispose();}
});

fs.writeFileSync(path.join(root,'results.json'),JSON.stringify({pi:JSON.parse(fs.readFileSync(path.join(piRoot,'node_modules/@earendil-works/pi-ai/package.json'),'utf8')).version,passed,scope:'Synthetic provider/client mocks and actual Pi SDK; no production model qualification or gateway evidence'},null,2)+'\n');
