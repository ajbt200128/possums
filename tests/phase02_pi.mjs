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
  let sends = 0; let failModels = false; let abortSeen = false; let release;
  client.login = async () => {};
  client.models = async () => {
    if (failModels) throw new Error('private diagnostic must not escape');
    return m.validateModels({ object:'list', data:[entry(tools)] });
  };
  client.chat = async (_model, messages, onDelta, _newConversation, options) => {
    const mode = plan[sends++];
    if (mode === 'held' || mode === 'held_tools') release=deferred();
    if (options.onPayload) await options.onPayload({model:'synthetic',stream:true,messages,...(options.tools?{tools:options.tools}:{})});
    await options.onResponse?.({status:200,contentType:'text/event-stream'});
    await options.onEvent(event({role:'assistant'}));
    if (['tool_calls','stop_tools','tool_length','length_partial','late_tools','held_tools'].includes(mode)) {
      await options.onEvent(event({tool_calls:[{index:0,...(mode==='late_tools'?{}:{id:'call_one',type:'function'}),function:{...(mode==='late_tools'?{}:{name:'echo'}),arguments:'{"value":'}}]}));
      if (mode !== 'length_partial') await options.onEvent(event({tool_calls:[{index:0,...(mode==='late_tools'?{id:'call_one',type:'function'}:{}),function:{...(mode==='late_tools'?{name:'echo'}:{}),arguments:'"ok"}'}}]}));
    } else {
      await options.onEvent(event({content:'progressive'}));
      onDelta('progressive');
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
  return { provider, client, credential, selected, sends:()=>sends, failModels:()=>{failModels=true;}, abortSeen:()=>abortSeen, release:()=>release?.resolve() };
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
  const fixture=await authFixture();const provider=new m.PossumsProvider(fixture.establish);
  const runtime=await nativeRuntime(provider,new ai.InMemoryCredentialStore());
  await runtime.login('possums','api_key',interaction());
  const held=deferred(),entered=deferred();const original=fixture.s.client.models;
  fixture.s.client.models=async()=>{entered.resolve();await held.promise;if(fails)throw new Error('synthetic failure');return [];};
  const pending=runtime.refresh({providers:['possums'],allowNetwork:true});await entered.promise;
  fixture.s.client.models=original;
  await runtime.login('possums','api_key',interaction(envKey));held.resolve();await pending;
  await new Promise(setImmediate); // Let the aborted provider operation reach its late publication.
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
 const s=await setup(plan,qualified);let toolRuns=0,provider,textOnlyCommand;
 const settings=coding.SettingsManager.inMemory({compaction:{enabled:compaction,keepRecentTokens:4},retry:{baseDelayMs:1,maxAgentDelayMs:2},defaultTools:[],cacheWarming:'off',enableInstallTelemetry:false});
 const runtime=await coding.ModelRuntime.create({credentials:new ai.InMemoryCredentialStore(),modelsStore:new ai.InMemoryModelsStore(),modelsPath:null,allowModelNetwork:false,refreshOnCreate:false});
 const cwd=path.join(root,name);fs.mkdirSync(cwd,{recursive:true});
 const loader=new coding.DefaultResourceLoader({cwd,agentDir:cwd,settingsManager:settings,noExtensions:true,noSkills:true,noPromptTemplates:true,noThemes:true,noContextFiles:true,systemPrompt:'Synthetic test',
  extensionFactories:[pi=>m.extension({...pi,registerProvider:value=>{provider=value;value.establish=async()=>s.client;pi.registerProvider(value);},registerCommand:(name,command)=>{if(name==='possums-text-only')textOnlyCommand=command;pi.registerCommand(name,command);}}),...(restoreTools?[pi=>pi.on('before_agent_start',()=>pi.setActiveTools(['echo']))]:[])]});
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
 return {session,s,runtime,provider,toolRuns:()=>toolRuns,textOnly:()=>textOnlyCommand.handler('',{ui:{notify:()=>{}}})};
}
await check('actual Pi SDK and shipped extension hooks run one receipted invocation per model turn',async()=>{
 const {session,s,toolRuns}=await sdkSetup('sdk-tools',['tool_calls','stop'],true);
 try{await session.prompt('Synthetic tool task');assert.equal(toolRuns(),1,session.messages.filter(value=>value.role==='assistant').at(-1)?.errorMessage);assert.equal(s.sends(),2);assert.equal(session.messages.filter(value=>value.role==='assistant').at(-1).content.find(value=>value.type==='text').text,'progressive');assert.equal(session.sessionManager.getSessionFile(),undefined);}
 finally{session.dispose();}
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
fs.writeFileSync(path.join(root,'results.json'),JSON.stringify({pi:'0.99.2',passed,scope:'Synthetic provider/client mocks and actual Pi SDK; no production model qualification or gateway evidence'},null,2)+'\n');
