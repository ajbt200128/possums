// Credential-free SDK/provider checks. PHASE02_TEST_BUILD must be a separately
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

await check('session-only auth marker; logout disables resolution',async()=>{
 const s=await setup(['stop']);
 assert.equal(s.credential.key,'possums-memory-only-session');
 assert(await s.provider.auth.apiKey.resolve({credential:s.credential}));
 assert.equal(await s.provider.auth.apiKey.resolve({credential:undefined}),undefined);
 s.provider.logout();assert.equal(await s.provider.auth.apiKey.resolve({credential:s.credential}),undefined);
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
 await assert.rejects(s.provider.refreshModels({allowNetwork:true,signal:new AbortController().signal,publish:async value=>{value.update?.();return true;}}),/possums_catalog_unavailable/);
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
 const credential=await provider.auth.apiKey.login({signal:new AbortController().signal,prompt:async()=> 'synthetic_not_a_usable_credential'});
 runtime.registerNativeProvider(provider);await runtime.setRuntimeApiKey('possums',credential.key);
 // Large synthetic host window isolates Pi recovery behavior from the separately
 // tested gateway context budget; the fake ReferenceClient has no tokenizer.
 const selected={...provider.getModels()[0],contextWindow:64000};
 const {session}=await coding.createAgentSession({cwd,agentDir:cwd,modelRuntime:runtime,model:selected,resourceLoader:loader,settingsManager:settings,
  thinkingLevel:'off',sessionManager:coding.SessionManager.inMemory(cwd),tools:tools?['echo']:[],customTools:[{...tool,label:'Echo',execute:async()=>{toolRuns++;return {content:[{type:'text',text:'ok'}],details:undefined};}}]});
 await session.bindExtensions({mode:'print'});
 if(tools)session.setActiveToolsByName(['echo']);assert.deepEqual(session.getActiveToolNames(),tools?['echo']:[]);
 return {session,s,toolRuns:()=>toolRuns,textOnly:()=>textOnlyCommand.handler('',{ui:{notify:()=>{}}})};
}
await check('actual Pi SDK and shipped extension hooks run one receipted invocation per model turn',async()=>{
 const {session,s,toolRuns}=await sdkSetup('sdk-tools',['tool_calls','stop'],true);
 try{await session.prompt('Synthetic tool task');assert.equal(toolRuns(),1,session.messages.filter(value=>value.role==='assistant').at(-1)?.errorMessage);assert.equal(s.sends(),2);assert.equal(session.messages.filter(value=>value.role==='assistant').at(-1).content.find(value=>value.type==='text').text,'progressive');assert.equal(session.sessionManager.getSessionFile(),undefined);}
 finally{session.dispose();}
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
