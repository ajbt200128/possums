// Offline, synthetic-credential SDK/provider checks. PHASE02_TEST_BUILD must be a separately
// compiled test-entry.ts bundle with fixture capability, never the shipped package.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
const file = process.env.PHASE02_TEST_BUILD;
const root = process.env.PHASE02_SCRATCH;
const piRoot = process.env.POSSUMS_PI_ROOT;
assert(file && root && piRoot, 'explicit scratch bundle, scratch state and pinned Pi paths required');
const m = await import(pathToFileURL(file).href);
const ai = await import(pathToFileURL(path.join(piRoot, 'node_modules/@earendil-works/pi-ai/dist/index.js')).href);
const coding = await import(pathToFileURL(path.join(piRoot, 'node_modules/@earendil-works/pi-coding-agent/dist/index.js')).href);
const { prepareCompaction } = await import(pathToFileURL(path.join(piRoot, 'node_modules/@earendil-works/pi-coding-agent/dist/core/compaction/compaction.js')).href);
const { AuthStorage: NativeAuthStorage } = await import(pathToFileURL(path.join(piRoot, 'node_modules/@earendil-works/pi-coding-agent/dist/core/auth-storage.js')).href);
fs.mkdirSync(root, { recursive: true });
const passed = [];
async function check(name, body) { await body(); passed.push(name); console.log('PASS '+name); }
const origin = 'https://possum-phase0.possums.containers.tinfoil.dev';
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
async function setup(plan, tools = true) {
  const key = new Uint8Array([0,0,32,...Array(32).fill(7),0,4,0,1,0,2]);
  const channel = await m.Channel.fixture('https://localhost:18443', key, '07'.repeat(32));
  const client = new m.ReferenceClient(channel);
  const requests = [];
  let sends = 0; let failModels = false; let abortSeen = false; let release;
  client.login = async () => {};
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
    if(['fragmented_tools','late_fragmented_tools','fragmented_tools_uncertain'].includes(mode)){
      const frame=value=>'data: '+JSON.stringify(value)+'\n\n';
      const identity=event({tool_calls:[{index:0,id:'call_one',type:'function',function:{name:'echo'}}]});
      const argumentsEvent=event({tool_calls:[{index:0,function:{arguments:'{"value":"ok"}'}}]});
      const fragments=mode==='late_fragmented_tools'?[event({tool_calls:[{index:0}]}),argumentsEvent,identity]:[identity,argumentsEvent];
      const finish={object:'chat.completion.chunk',model:'synthetic',choices:[{index:0,delta:{},finish_reason:'tool_calls'}]};
      const usage={object:'chat.completion.chunk',model:'synthetic',choices:[],usage:{prompt_tokens:2,completion_tokens:3,total_tokens:5},
        possums:{outcome:'settled',charged_microunits:'7',refunded_microunits:'45',quoted_input_microunits_per_million_tokens:'1000000',quoted_output_microunits_per_million_tokens:'1000000'}};
      const text=[event({role:'assistant'}),event({content:'Synthetic preamble'}),...fragments,finish].map(frame).join('')+
        (mode==='fragmented_tools_uncertain'?'':frame(usage)+'data: [DONE]\n\n');
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
    if (mode === 'unknown_bill') throw new m.GatewayError('unavailable', 'inference_unavailable', 'unknown');
    if (mode === 'sdk_decode') throw new m.GatewayError('generation_failed', 'sdk_stream_decode_failed', 'unknown');
    if (mode === 'hostile') throw new Error('possums_secret_key_PRIVATE_PROMPT');
    if (mode === 'abort') await new Promise((_resolve,reject)=>{ options.signal.addEventListener('abort',()=>{abortSeen=true;reject(new m.ChannelError('uncertain'));},{once:true}); });
    return receipt(['tool_calls','late_tools','held_tools'].includes(mode) ? 'tool_calls' : ['tool_length','length_partial','length'].includes(mode) ? 'length' : 'stop');
  };
  const provider = new m.PossumsProvider(async()=>client);
  const credential = await provider.auth.apiKey.login({signal:new AbortController().signal,prompt:async()=> 'synthetic_not_a_usable_credential'});
  const selected = provider.getModels()[0];
  return { provider, client, credential, selected, requests, sends:()=>sends, failModels:()=>{failModels=true;}, abortSeen:()=>abortSeen, release:()=>release?.resolve() };
}
async function drain(stream) { const events=[];for await(const event of stream)events.push(event); return {events,message:await stream.result()}; }
const context = tools => ai.normalizeContext({messages:[user],tools:tools?[tool]:[]});

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
  candidate.chat=(model,messages,onDelta,newConversation,options)=>{
   trace.conversations.push(newConversation);return s.client.chat(model,messages,onDelta,newConversation,options);
  };
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
 assert.equal(await runtime.checkAuth('possums'),undefined);assert.equal(fixture.trace.establish,0);
 const credential=await runtime.login('possums','api_key',interaction());
 assert.deepEqual(credential,{type:'api_key',key:recoveryKey});
 assert.deepEqual(JSON.parse(fs.readFileSync(authPath,'utf8')),{possums:credential});
 assert.equal(fs.statSync(authPath).mode&0o777,0o600);assert.equal(fs.statSync(path.dirname(authPath)).mode&0o777,0o700);
 assert.equal(await modelsStore.read('possums'),undefined);
 provider.logout();
 const extension=fixtureExtension(fixture.establish);
 assert(!extension.commands.has('possums-logout'));
 const restored=await nativeRuntime(extension.provider,NativeAuthStorage.create(authPath),modelsStore);
 const registry=new coding.ModelRegistry(restored);
 const before=structuredClone(fixture.trace);
 assert(await restored.checkAuth('possums'));assert.deepEqual(await restored.getAvailable('possums'),[]);
 assert.deepEqual(fixture.trace,before,'availability is offline even with a saved key');
 await extension.handlers.get('session_start')({}, {modelRegistry:registry});
 assert.equal(registry.getAvailable().filter(value=>value.provider==='possums').length,1);
 assert.equal(fixture.trace.establish,2);assert.deepEqual(fixture.trace.keys,[recoveryKey,recoveryKey]);
 assert.equal(fixture.s.sends(),0);assert.equal(await modelsStore.read('possums'),undefined);
 assert.equal((await registry.getProviderAuth('possums')).auth.apiKey,requestMarker);
 const selected=extension.provider.getModels()[0];
 extension.provider.beginRun();await drain(restored.streamSimple(selected,context(false)));
 await extension.handlers.get('session_start')({}, {modelRegistry:registry});
 extension.provider.beginRun();await drain(restored.streamSimple(selected,context(false)));
 assert.deepEqual(fixture.trace.conversations,[true,true],'/new resets conversation, not saved authentication');
 assert.equal(fixture.trace.establish,2);
 await restored.logout('possums');
 assert.equal(await NativeAuthStorage.create(authPath).read('possums'),undefined);
 assert.deepEqual(JSON.parse(fs.readFileSync(authPath,'utf8')),{});
 assert.deepEqual(extension.provider.getModels(),[]);assert.equal(await registry.getProviderAuth('possums'),undefined);
});
await check('pinned SDK checks stay offline; saved key wins over env; legacy marker requires fresh login',async()=>{
 const fixture=await authFixture();const provider=new m.PossumsProvider(fixture.establish);
 const credentials=new ai.InMemoryCredentialStore();
 const models=ai.createModels({credentials,modelsStore:new ai.InMemoryModelsStore(),authContext:authInput(undefined,envKey).ctx});
 models.setProvider(provider);
 assert.equal((await models.checkAuth('possums')).source,'POSSUMS_RECOVERY_CREDENTIAL');
 assert.deepEqual(await models.getAvailable('possums'),[]);assert.equal(fixture.trace.establish,0);
 await credentials.modify('possums',async()=>({type:'api_key',key:recoveryKey}));
 assert.equal((await models.checkAuth('possums')).source,'stored credential');assert.equal(fixture.trace.establish,0);
 assert.equal((await models.refresh({providers:['possums'],allowNetwork:true})).errors.size,0);
 assert.deepEqual(fixture.trace.keys,[recoveryKey]);
 await models.logout('possums');await models.refresh({providers:['possums'],allowNetwork:false});
 assert.deepEqual(provider.getModels(),[]);
 assert.equal((await models.checkAuth('possums')).source,'POSSUMS_RECOVERY_CREDENTIAL');
 await models.refresh({providers:['possums'],allowNetwork:true});assert.deepEqual(fixture.trace.keys,[recoveryKey,envKey]);
 for(const marker of [legacyMarker,requestMarker]) {
  await credentials.modify('possums',async()=>({type:'api_key',key:marker}));
  assert.equal(await models.checkAuth('possums'),undefined);
  await models.refresh({providers:['possums'],allowNetwork:true});
  assert.equal(await models.getAuth('possums'),undefined);assert.deepEqual(provider.getModels(),[]);
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
 assert.equal(notices.length,1);assert.match(notices[0],/possums_verification_failed/);assert.match(notices[0],/\/reload/);
 assert.match(notices[0],/Cached models do not authorize inference/);assert.deepEqual(extension.provider.getModels(),[]);
 await extension.commands.get('possums-status').handler('',ctx);
 assert.equal(notices.length,2);assert.equal(notices[1],notices[0]);
 assert.equal(fixture.trace.establish,0,'status does not connect');
 extension.provider.establish=fixture.establish;
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
  assert.match(notices[0],/Check connectivity/);assert.deepEqual(extension.provider.getModels(),[]);
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
 assert.deepEqual(extension.provider.getModels(),[]);assert.equal(notices.length,1);
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
   if(calls++===0)candidate[phase]=async()=>{entered.resolve();await held.promise;throw new Error(hostileConnection);};
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
  assert.deepEqual(notices,before);assert.equal(provider.getModels().length,action==='logout'?0:1);
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
  await assert.rejects(refresh(),error=>assertConnectionFailure(error,'catalog_unavailable'));assert.deepEqual(provider.getModels(),[]);
  fixture.s.client.models=original;await refresh();assert.equal(provider.getModels().length,1);
  const extension=fixtureExtension(establish),credentials=new ai.InMemoryCredentialStore();
  await credentials.modify('possums',async()=>({type:'api_key',key:recoveryKey}));
  const runtime=await nativeRuntime(extension.provider,credentials),registry=new coding.ModelRegistry(runtime);
  const ctx={hasUI:true,ui:{notify:()=>{throw new Error(hostileConnection);}},modelRegistry:registry};
  failing=true;await extension.handlers.get('session_start')({},ctx);assert.deepEqual(extension.provider.getModels(),[]);
  failing=false;await registry.refresh({providers:['possums'],allowNetwork:true});assert.equal(extension.provider.getModels().length,1);
  fixture.s.client.models=async()=>{throw new Error(hostileConnection);};
  await registry.refresh({providers:['possums'],allowNetwork:true});assert.deepEqual(extension.provider.getModels(),[]);
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
  else await provider.auth.apiKey.resolve(authInput({type:'api_key',key:envKey}));
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
 s.provider.logout();assert.equal(s.provider.getModels().length,0);
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
await check('uncertain generation and length cannot authorize another invocation',async()=>{
 for(const mode of ['uncertain','length']){const s=await setup([mode]);s.provider.beginRun();const first=await drain(s.provider.streamSimple(s.selected,context(false)));
 const second=await drain(s.provider.streamSimple(s.selected,context(false)));assert.equal(s.sends(),1);assert.match(second.message.errorMessage,/\[possums_automatic_replay_blocked\].*Automatic replay blocked/);
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
await check('nested tool schema survives Pi conversion and the complete request snapshot',async()=>{
 let parameters={type:'object',properties:{value:{type:'string',enum:['synthetic']}}};
 for(let i=0;i<6;i++)parameters={type:'object',properties:{nested:parameters}};
 const s=await setup(['stop']);
 const transcript=ai.normalizeContext({messages:[user],tools:[{...tool,parameters}]});
 const payload=m.snapshotInvocation(m.invocation(s.selected,transcript));
 assert.deepEqual(payload.tools[0].function.parameters,parameters);
 assert(Object.isFrozen(payload.tools[0].function.parameters));
});
await check('over-deep request encoding keeps a private actionable error and cannot replay',async()=>{
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
 assert.match(second.message.errorMessage,/\[possums_automatic_replay_blocked\]/);assert.equal(s.sends(),0);
});
await check('failed catalog refresh removes prior usable list',async()=>{
 const s=await setup(['stop']);assert.equal(s.provider.getModels().length,1);s.failModels();
 await assert.rejects(s.provider.refreshModels({credential:{type:'api_key',key:requestMarker},allowNetwork:true,signal:new AbortController().signal,publish:async value=>{value.update?.();return true;}}),/possums_catalog_unavailable/);
 assert.equal(s.provider.getModels().length,0);
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
  assert.match(result.message.errorMessage,new RegExp(code==='generation_failed'?'Generation failed.*stream idle timeout.*Reservation refunded.*Not replayed':'Gateway unavailable.*inference unavailable.*Charge unknown.*Not replayed'));
  assert.equal(result.message.diagnostics?.some(d=>d.type==='possums_billing_unknown')??false,billing==='unknown');
  assert.equal(result.events.some(e=>e.type==='toolcall_end'),false);assert.equal(s.sends(),1);
 }
 const s=await setup(['hostile']);s.provider.beginRun();const result=await drain(s.provider.streamSimple(s.selected,context(false)));
 assert.equal(result.message.errorMessage,'Possums: request rejected. Not replayed.');
 assert(!JSON.stringify(result).includes('PRIVATE_PROMPT'));
});
async function sdkSetup(name, plan, tools, compaction=false, qualified=true, restoreTools=false) {
 const s=await setup(plan,qualified);let toolRuns=0,provider,textOnlyCommand;const notices=[],compactions=[];
 // Keep native physical-model lookup and the selected host model consistent.
 const models=s.client.models;s.client.models=async()=> (await models()).map(model=>({...model,context_tokens:'64000'}));
 const settings=coding.SettingsManager.inMemory({compaction:{enabled:compaction,keepRecentTokens:4},retry:{baseDelayMs:1,maxAgentDelayMs:2},defaultTools:[],cacheWarming:'off',enableInstallTelemetry:false});
 const runtime=await coding.ModelRuntime.create({credentials:new ai.InMemoryCredentialStore(),modelsStore:new ai.InMemoryModelsStore(),modelsPath:null,allowModelNetwork:false,refreshOnCreate:false});
 const cwd=path.join(root,name);fs.mkdirSync(cwd,{recursive:true});
 const loader=new coding.DefaultResourceLoader({cwd,agentDir:cwd,settingsManager:settings,noExtensions:true,noSkills:true,noPromptTemplates:true,noThemes:true,noContextFiles:true,systemPrompt:'Synthetic test',
  extensionFactories:[pi=>m.extension({...pi,on:(name,handler)=>pi.on(name,(event,ctx)=>{if(name==='session_before_compact'){compactions.push(event);ctx={...ctx,ui:{...ctx.ui,notify:text=>notices.push(text)}};}return handler(event,ctx);}),registerProvider:value=>{provider=value;value.establish=async()=>s.client;pi.registerProvider(value);},registerCommand:(name,command)=>{if(name==='possums-text-only')textOnlyCommand=command;pi.registerCommand(name,command);}}),...(restoreTools?[pi=>pi.on('before_agent_start',()=>pi.setActiveTools(['echo']))]:[])]});
 await loader.reload();assert.deepEqual(loader.getExtensions().errors,[]);
 runtime.registerNativeProvider(provider);
 await runtime.refresh({providers:['possums'],allowNetwork:false});
 await runtime.login('possums','api_key',interaction());
 // Large synthetic host window isolates Pi recovery behavior from the separately
 // tested gateway context budget; the fake ReferenceClient has no tokenizer.
 const selected={...provider.getModels()[0],contextWindow:64000};
 const {session}=await coding.createAgentSession({cwd,agentDir:cwd,modelRuntime:runtime,model:selected,resourceLoader:loader,settingsManager:settings,
  thinkingLevel:'off',sessionManager:coding.SessionManager.inMemory(cwd),tools:tools?['echo']:[],customTools:[{...tool,label:'Echo',execute:async()=>{toolRuns++;return {content:[{type:'text',text:'ok'}],details:undefined};}}]});
 await session.bindExtensions({mode:'print'});
 if(tools)session.setActiveToolsByName(['echo']);assert.deepEqual(session.getActiveToolNames(),tools?['echo']:[]);
 return {session,s,runtime,provider,notices,compactions,settings,toolRuns:()=>toolRuns,textOnly:()=>textOnlyCommand.handler('',{ui:{notify:()=>{}}})};
}
await check('actual Pi SDK and shipped extension hooks run one receipted invocation per model turn',async()=>{
 const {session,s,toolRuns}=await sdkSetup('sdk-tools',['tool_calls','stop'],true);
 try{await session.prompt('Synthetic tool task');assert.equal(toolRuns(),1,session.messages.filter(value=>value.role==='assistant').at(-1)?.errorMessage);assert.equal(s.sends(),2);assert.equal(s.requests[1].messages.find(message=>message.role==='assistant').content,'');assert.equal(session.messages.filter(value=>value.role==='assistant').at(-1).content.find(value=>value.type==='text').text,'progressive');assert.equal(session.sessionManager.getSessionFile(),undefined);}
 finally{session.dispose();}
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
 await runtime.login('possums','api_key',interaction());
 const manager=coding.SessionManager.inMemory(root), preparation=seedCompaction(manager,split,previous),controller=new AbortController(),notices=[];
 assert(preparation);assert.equal(preparation.isSplitTurn,split);
 const ctx={model:extension.provider.getModels()[0],modelRegistry:new coding.ModelRegistry(runtime),thinkingLevel:'off',ui:{notify:text=>notices.push(text)}};
 const event={type:'session_before_compact',preparation,branchEntries:manager.getBranch(),reason:'manual',willRetry:false,signal:controller.signal,customInstructions:'Synthetic focus'};
 return {...fixture,...extension,runtime,manager,event,ctx,controller,notices,run:()=>extension.handlers.get('session_before_compact')(event,ctx)};
}
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
await check('summary success/failure preserves ready run, receipted continuation and ordinary conversation state',async()=>{
 for(const summary of ['stop','refund']) {
  const ready=await compactionFixture([summary,'stop']);ready.provider.beginRun();await ready.run();
  assert.equal((await drain(ready.provider.streamSimple(ready.ctx.model,context(false)))).message.stopReason,'stop');
  assert.deepEqual(ready.s.requests.map(r=>r.newConversation),[true,true]);
  const f=await compactionFixture(['tool_calls',summary,'stop']);f.provider.beginRun();
  const first=(await drain(f.provider.streamSimple(f.ctx.model,context(true)))).message;
  assert.equal(first.stopReason,'toolUse');await f.run();
  const continuation=ai.normalizeContext({messages:[user,first,{role:'toolResult',toolCallId:'call_one',toolName:'echo',content:[{type:'text',text:'ok'}],timestamp:3}],tools:[tool]});
  assert.equal((await drain(f.provider.streamSimple(f.ctx.model,continuation))).message.stopReason,'stop');
  assert.equal((await drain(f.provider.streamSimple(f.ctx.model,continuation))).message.stopReason,'error');
  assert.deepEqual(f.s.requests.map(r=>r.newConversation),[true,true,false]);assert.equal(f.s.sends(),3);
 }
});
await check('summary errors, length, empty text and tool attempts fail closed with safe stages and no replay',async()=>{
 for(const [mode,pattern] of [['refund',/Generation failed.*stream idle timeout.*Reservation refunded/],['unknown_bill',/Gateway unavailable.*inference unavailable.*Charge unknown/],['sdk_decode',/sdk stream decode failed/],['hostile',/request rejected/],['length',/possums_summary_length/],['empty',/possums_summary_empty/],['tool_calls',/possums_summary_tools/],['stop_tools',/possums_summary_tools/]]) {
  const f=await compactionFixture([mode,'stop'],true),observed=[];const perform=f.provider.perform.bind(f.provider);
  f.provider.perform=(...args)=>{const stream=perform(...args);observed.push(drain(stream));return stream;};
  const result=await f.run();
  assert((await Promise.all(observed)).every(({events})=>events.every(event=>!event.type.startsWith('toolcall_'))));
  assert.deepEqual(result,{cancel:true});assert.equal(f.s.sends(),1);assert.match(f.notices[0],/summary 1/);assert.match(f.notices[0],pattern);
  assert(!f.notices.join('').includes('PRIVATE_PROMPT'));assert.match(f.notices[0],/No checkpoint saved.*Not replayed/);
  if(['length','empty','tool_calls','stop_tools'].includes(mode))assert.match(f.notices[0],/Observed settled charges: \$0.000007/);
  assert.equal((await drain(f.provider.streamSimple(f.ctx.model,context(false)))).message.stopReason,'error');assert.equal(f.s.sends(),1);
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

fs.writeFileSync(path.join(root,'results.json'),JSON.stringify({pi:'1.0.4',passed,scope:'Synthetic provider/client mocks and actual Pi SDK; no production model qualification or gateway evidence'},null,2)+'\n');
