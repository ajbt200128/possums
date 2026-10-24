// Synthetic/offline checks against the pinned Pi runtime and real client path.
import assert from 'node:assert/strict';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
const m = await import(pathToFileURL(process.env.PHASE02_TEST_BUILD).href);
const ai = await import(pathToFileURL(path.join(process.env.POSSUMS_PI_ROOT, 'node_modules/@earendil-works/pi-ai/dist/index.js')).href);
const base = { object:'model',id:'synthetic',context_tokens:'100',max_output_tokens:'100',
 input_microunits_per_million_tokens:'1000000',output_microunits_per_million_tokens:'1000000',maximum_reservation_microunits:'260',tool_protocol:'openai-functions-v1',reasoning:true };
const metadata = (enable, disable, effort_map) => ({ params:{'/v1/chat/completions':{...(enable?{enable}:{}),...(disable?{disable}:{})}},...(effort_map?{effort_map}:{}) });
const mapped = { ...base,reasoning_params:metadata({chat_template_kwargs:{thinking:true,reasoning_effort:'$EFFORT'}},{chat_template_kwargs:{thinking:false}},{low:'low',medium:'high',high:'xhigh'}) };
const toggle = { ...base,reasoning_params:metadata({chat_template_kwargs:{enable_thinking:true}},{chat_template_kwargs:{enable_thinking:false}}) };
const always = { ...base,reasoning_params:metadata({chat_template_kwargs:{reasoning_effort:'$EFFORT'}},undefined,{low:'low',medium:'high',high:'max'}) };
const unmapped = { ...base,reasoning_params:metadata({reasoning_effort:'$EFFORT'}) };
const frame = value => 'data: '+JSON.stringify(value)+'\n\n';
const event = delta => ({object:'chat.completion.chunk',model:'synthetic',choices:[{index:0,delta,finish_reason:null}]});
const finish = kind => ({object:'chat.completion.chunk',model:'synthetic',choices:[{index:0,delta:{},finish_reason:kind}]});
const money = {object:'chat.completion.chunk',model:'synthetic',choices:[],usage:{prompt_tokens:2,completion_tokens:3,total_tokens:5},possums:{outcome:'settled',status:'complete',charged_microunits:'7',refunded_microunits:'253'}};
function body(deltas, kind='stop', interrupted=false) {
 const text = deltas.map(delta=>frame(event(delta))).join('')+(interrupted?'':frame(finish(kind))+frame(money)+'data: [DONE]\n\n');
 let sent=false;
 return new ReadableStream({pull(controller){if(!sent){sent=true;controller.enqueue(new TextEncoder().encode(text));}else if(interrupted)controller.error(Error('PRIVATE_HOSTILE_EXCEPTION'));else controller.close();}},{highWaterMark:0});
}
const user={role:'user',content:'Synthetic input',timestamp:1};
const ctx=ai.normalizeContext({messages:[user]});
async function setup(entry=mapped, deltas=[{reasoning:'thought'},{content:'answer'}], kind='stop', interrupted=false) {
 const config=new Uint8Array([0,0,32,...Array(32).fill(7),0,4,0,1,0,2]);
 const channel=await m.Channel.fixture('https://localhost:18443',config,'07'.repeat(32));
 const requests=[]; let current=entry, submissions=0;
 channel.challenge=async()=>({challenge:'A'.repeat(43),expires_in:600});
 channel.control=async route=>route==='/v1/sessions'?{token:'B'.repeat(43),token_type:'Bearer',expires_in:43200}:(submissions++,{submission:'C'.repeat(43)});
 channel.models=async()=>({object:'list',data:[current]});
 channel.chat=async payload=>{requests.push(structuredClone(payload));return body(deltas,kind,interrupted);};
 const client=new m.ReferenceClient(channel);
 const provider=new m.PossumsProvider(async()=>client);provider.newSession();
 await provider.auth.apiKey.login({signal:new AbortController().signal,prompt:async()=>'synthetic_not_a_usable_credential'});
 const selected=provider.getModels()[0];
 return {provider,selected,requests,client,change:value=>{current=value;},submissions:()=>submissions};
}
async function drain(stream){const events=[];for await(const event of stream)events.push(event);return {events,message:await stream.result()};}
const opts=payload=>({...(payload.reasoning_effort!==undefined?{reasoning_effort:payload.reasoning_effort}:{}),...(payload.chat_template_kwargs!==undefined?{chat_template_kwargs:payload.chat_template_kwargs}:{})});
function balanced(events){for(const kind of ['thinking','text']){const starts=events.filter(e=>e.type===kind+'_start'),ends=events.filter(e=>e.type===kind+'_end');assert.deepEqual(starts.map(e=>e.contentIndex),ends.map(e=>e.contentIndex));for(const start of starts){const end=ends.find(e=>e.contentIndex===start.contentIndex);assert(events.indexOf(start)<events.indexOf(end));}}}
for(const [entry,expected] of [[mapped,['off','low','medium','high']],[always,['low','medium','high']],[toggle,['off','high']]]){
 const native={...m.thinkingMetadata(entry)};
 assert.deepEqual(ai.getSupportedThinkingLevels(native),expected);
}
for(const entry of [unmapped,{...base}, {...base,reasoning_params:{params:{'/v1/chat/completions':{enable:{chat_template_kwargs:{unknown:'PRIVATE'}}}}}}]){
 const clean=m.validateModels({object:'list',data:[entry]})[0];assert.equal(m.thinkingMetadata(clean).reasoning,false);
 assert.deepEqual(m.simpleThinking(clean,'high'),{});
 assert.throws(()=>m.admitReasoning(clean,{reasoning_effort:'high'}));
}
assert.deepEqual(m.simpleThinking(mapped,'medium'),{chat_template_kwargs:{thinking:true,reasoning_effort:'high'}});
assert.deepEqual(m.simpleThinking(always,undefined),{});
assert.equal(m.thinkingMetadata(always).thinkingLevelMap.off,null);
assert.deepEqual(m.simpleThinking(mapped,undefined),{chat_template_kwargs:{thinking:false}});
for(const value of [{chat_template_kwargs:{thinking:'PRIVATE'}},{chat_template_kwargs:{thinking:true,extra:true}},{reasoning_effort:'$EFFORT'}])assert.throws(()=>m.admitReasoning(mapped,value));
for(const [simple,options,expected] of [[true,undefined,{chat_template_kwargs:{thinking:false}}],[false,undefined,{}],[true,{reasoning:'medium'},{chat_template_kwargs:{thinking:true,reasoning_effort:'high'}}],[false,{chatTemplateKwargs:{thinking:false}},{chat_template_kwargs:{thinking:false}}]]){
 const f=await setup();const result=await drain(simple?f.provider.streamSimple(f.selected,ctx,options):f.provider.stream(f.selected,ctx,options));
 assert.deepEqual(opts(f.requests[0]),expected);assert.equal(result.message.stopReason,'stop');balanced(result.events);
 assert.deepEqual(result.message.content.map(b=>b.type),['thinking','text']);
}
// Default old gateway/client-facing catalog lacks optional metadata: defaults still dispatch.
const {reasoning:_reasoning,...legacy}=base;
for(const entry of [legacy,unmapped,always]){
 const f=await setup(entry);await drain(f.provider.streamSimple(f.selected,ctx));assert.deepEqual(opts(f.requests[0]),{});
}
// Null and empty reasoning are valid and create no synthetic content block.
{
 const f=await setup(mapped,[{reasoning:null,content:null},{reasoning:'',reasoning_content:'',content:''}]);
 const result=await drain(f.provider.streamSimple(f.selected,ctx));assert.deepEqual(result.message.content,[]);balanced(result.events);
 assert.equal(result.message.stopReason,'stop');
}
// Full-event hook precedes display; both aliases choose only the native first nonempty field.
{
 const f=await setup(mapped,[{reasoning:null,reasoning_content:''},{reasoning:'duplicate',reasoning_content:'one',content:'two'},{reasoning:'three'},{content:'four'}]);
 const result=await drain(f.provider.streamSimple(f.selected,ctx));balanced(result.events);
 assert.deepEqual(result.message.content.map(b=>b.type==='thinking'?b.thinking:b.text),['one','two','three','four']);
}
for(const interrupted of [false,true]){
 const f=await setup(mapped,[{reasoning:'one'},{content:'two'},{reasoning:'three'},{tool_calls:[{index:0,id:'call_one',type:'function',function:{name:'echo',arguments:'{}'}}]}],'tool_calls',interrupted);
 const result=await drain(f.provider.streamSimple(f.selected,ctx));balanced(result.events);
 assert.equal(result.events.at(-1).type,'done');
 assert.equal(result.message.content.some(b=>b.type==='toolCall'),!interrupted);
 if(interrupted){assert(!result.events.some(e=>e.type==='toolcall_end'));assert(result.message.diagnostics.some(d=>d.type==='possums_turn_warning'));assert(!JSON.stringify(result.message).includes('PRIVATE_HOSTILE_EXCEPTION'));}
}
// Payload hooks and latest catalog (including session renewal) cannot bypass admission.
for(const change of ['hook','catalog']){
 const f=await setup();if(change==='catalog')f.change(unmapped);
 const result=await drain(f.provider.streamSimple(f.selected,ctx,{reasoning:'medium',...(change==='hook'?{onPayload:payload=>({...payload,chat_template_kwargs:{thinking:true,reasoning_effort:'PRIVATE_HOSTILE_VALUE'}})}:{})}));
 assert.equal(f.requests.length,0);assert.equal(f.submissions(),0);assert(result.message.diagnostics.some(d=>d.type==='possums_turn_warning'));
 assert.match(result.message.diagnostics.find(d=>d.type==='possums_turn_warning').details.message,/possums_request_reasoning/);
 assert(!JSON.stringify(result.message).includes('PRIVATE_HOSTILE_VALUE'));
}
// Expiry renewal resolves the level against the renewed authenticated metadata,
// not the old Pi model snapshot (no inference retry or replay).
{
 const f=await setup();
 f.change({...mapped,reasoning_params:metadata({chat_template_kwargs:{thinking:true,reasoning_effort:'$EFFORT'}},{chat_template_kwargs:{thinking:false}},{low:'low',medium:'max',high:'xhigh'})});
 Object.defineProperty(f.provider.client,'authExpiresAt',{value:0});
 const result=await drain(f.provider.streamSimple(f.selected,ctx,{reasoning:'medium'}));
 assert.equal(result.message.stopReason,'stop');assert.deepEqual(opts(f.requests[0]),{chat_template_kwargs:{thinking:true,reasoning_effort:'max'}});
 assert.equal(f.requests.length,1);
}
{
 const top={...base,reasoning_params:metadata({reasoning_effort:'$EFFORT'},undefined,{low:'low',medium:'medium',high:'high'})};
 const f=await setup(top);await drain(f.provider.stream(f.selected,ctx,{reasoningEffort:'medium'}));
 assert.deepEqual(opts(f.requests[0]),{reasoning_effort:'medium'});
}
// Signed thinking is never replayed as reasoning or text, including tool-only turns.
{
 const selected=(await setup()).selected;
 const assistant={role:'assistant',api:'openai-completions',provider:'possums',model:'synthetic',timestamp:1,stopReason:'toolUse',usage:{input:0,output:0,cacheRead:0,cacheWrite:0,totalTokens:0,cost:{input:0,output:0,cacheRead:0,cacheWrite:0,total:0}},content:[{type:'thinking',thinking:'PRIVATE_REASONING_REPLAY',thinkingSignature:'reasoning_content'},{type:'toolCall',id:'call_one',name:'echo',arguments:{}}]};
 const request=m.invocation(selected,ai.normalizeContext({messages:[user,assistant,{role:'toolResult',toolCallId:'call_one',toolName:'echo',content:[{type:'text',text:'ok'}],isError:false,timestamp:2}]}));
 assert(!JSON.stringify(request).includes('PRIVATE_REASONING_REPLAY'));assert.equal(request.messages.find(message=>message.role==='assistant').content,'');
}
console.log('PASS catalog-driven thinking: closed metadata/options; raw omission/native off; mapped/toggle/unavailable levels; balanced display/tools/interruption; hooks/fresh catalog admission; no reasoning replay (pinned Pi, offline)');
