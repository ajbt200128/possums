// Synthetic/offline receipt and native grouping qualification, no Fetch trailers.
import assert from 'node:assert/strict';
import { pathToFileURL } from 'node:url';
import path from 'node:path';
const m = await import(pathToFileURL(process.env.PHASE02_TEST_BUILD));
const entry = { object:'model', id:'m', context_tokens:'20', max_output_tokens:'20', input_microunits_per_million_tokens:'1000000', output_microunits_per_million_tokens:'1000000', maximum_reservation_microunits:'52', cached_input_microunits_per_million_tokens:'0', tool_protocol:'openai-functions-v1' };
const tool = { type:'function', function:{name:'lookup',parameters:{type:'object'}} };
const event = value => 'data: '+JSON.stringify({object:'chat.completion.chunk',model:'m',...value})+'\n\n';
const choice = (delta, finish = null, usage) => event({choices:[{index:0,delta,finish_reason:finish}],...(usage ? {usage} : {})});
const terminal = (status, charged='4', reserved='52', outcome='settled') => event({choices:[],possums:{
  outcome,status, reserved_microunits:reserved,charged_microunits:charged,refunded_microunits:(BigInt(reserved)-BigInt(charged)).toString(),markup_percent:130,
  quoted_input_microunits_per_million_tokens:'1000000', quoted_output_microunits_per_million_tokens:'1000000',quoted_cached_input_microunits_per_million_tokens:'0',
  evidence:'running_sse',usage:{prompt_tokens:3,completion_tokens:2,total_tokens:5,cached_tokens:2},provider_cost_usd:'999',
}});
const done = 'data: [DONE]\n\n';
function stream(text, fault = false) {
  const bytes = new TextEncoder().encode(text); let offset=0;
  return new ReadableStream({pull(c) { if(offset < bytes.length){c.enqueue(bytes.slice(offset, offset+7));offset+=7;} else if(fault)c.error(new Error('private-error-marker')); else c.close(); }},{highWaterMark:0});
}
let text='';
const complete = await m.consumeCompletion(stream(choice({role:'assistant',content:'native'},null,{prompt_tokens:1,completion_tokens:1,total_tokens:2})+choice({content:' grouping'},'stop')+terminal('complete')+done),entry,t=>text+=t);
assert.equal(text,'native grouping'); assert.equal(complete.complete,true);assert.equal(complete.chargedMicrounits,'4');assert.equal(complete.cachedTokens,2);
for (const calls of [null,[]]) {
  const receipt = await m.consumeCompletion(stream(choice({role:'assistant',content:'text only',tool_calls:calls},'stop')+terminal('complete')+done),entry,()=>{});
  assert.equal(receipt.complete,true); // No declared tools needed for an empty native array.
}
const capped = await m.consumeCompletion(stream(choice({content:'capped'},'stop')+terminal('complete','2','2')+done),entry,()=>{});
assert.equal(capped.chargedMicrounits,'2');
const fragment = choice({role:'assistant',tool_calls:[{index:0,id:'call',type:'function',function:{name:'lookup',arguments:'{'}}]});
let executed = false;
await assert.rejects(async()=> {
  await m.consumeCompletion(stream(fragment+terminal('incomplete')+event({error:{code:'generation_failed',detail:'stream_transport_failed',message:'private-error-marker',billing:'unknown'}}).replace(/"object":"chat.completion.chunk","model":"m",/, '')+done),entry,()=>{}, {tools:[tool]});
  executed=true;
},error=>error instanceof m.GatewayError && error.billing==='settled' && error.settlement?.complete===false && error.settlement.chargedMicrounits==='4' && !error.message.includes('private-error-marker'));
assert.equal(executed,false);
for(const body of [stream(fragment),stream(fragment+terminal('incomplete')+done,true)]) {
  await assert.rejects(()=>m.consumeCompletion(body,entry,()=>{},{tools:[tool]}),error=>error instanceof m.ChannelError && (!('billing' in error) || error.billing==='unknown') && !error.message.includes('private-error-marker'));
}
assert.equal((await m.consumeCompletion(stream(fragment+choice({},'tool_calls')+terminal('complete')+done),entry,()=>{},{tools:[tool]})).chargedMicrounits,'4');
for(const altered of [terminal('complete','3'),terminal('complete').replace('"cached_tokens":2','"cached_tokens":4'),terminal('complete').replace('"markup_percent":130','"markup_percent":150')]) {
  assert.equal((await m.consumeCompletion(stream(choice({},'stop')+altered+done),entry,()=>{})).complete,true);
}
console.log('PASS native grouping, authoritative gateway accounting, provider cost separation, paid partial tools, truncated receipt unknown, caller validation after confirmed money');
