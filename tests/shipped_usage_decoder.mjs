// Offline qualification of exact pinned decoder bodies from a shipped bundle.
// Extract declarations statically: never initialize its provider, auth or network.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { pathToFileURL } from 'node:url';
const [bundlePath, compilerPath] = process.argv.slice(2);
assert(bundlePath && compilerPath);
const ts = (await import(pathToFileURL(compilerPath).href)).default;
const source = await readFile(bundlePath, 'utf8');
const ast = ts.createSourceFile('shipped.mjs', source, ts.ScriptTarget.Latest, true, ts.ScriptKind.JS);
const declarations = new Map();
for (const statement of ast.statements) {
  if (ts.isFunctionDeclaration(statement) && statement.name) declarations.set(statement.name.text, statement);
  if (ts.isVariableStatement(statement)) {
    for (const declaration of statement.declarationList.declarations) {
      if (ts.isIdentifier(declaration.name)) declarations.set(declaration.name.text, statement);
    }
  }
}
const selected = new Set();
function select(name) {
  const statement = declarations.get(name);
  assert(statement, 'shipped decoder declaration unavailable');
  if (selected.has(statement)) return;
  selected.add(statement);
  function dependencies(node) {
    if (ts.isIdentifier(node) && declarations.has(node.text) &&
        !(ts.isPropertyAccessExpression(node.parent) && node.parent.name === node) &&
        !(ts.isPropertyAssignment(node.parent) && node.parent.name === node)) select(node.text);
    ts.forEachChild(node, dependencies);
  }
  dependencies(statement);
}
for (const name of ['consumeCompletion', 'GatewayError', 'validateModels']) select(name);
const code = ast.statements.filter(statement => selected.has(statement)).map(statement => statement.getText(ast)).join('\n');
// Only public, operator-selected shipped code, with no imports/provider activation.
const decoder = new Function(`${code}\nreturn {consumeCompletion, GatewayError, validateModels};`)();
const entry = { object:'model', id:'m', context_tokens:'20', max_output_tokens:'20', input_microunits_per_million_tokens:'1000000', output_microunits_per_million_tokens:'1000000', maximum_reservation_microunits:'52', tool_protocol:'openai-functions-v1' };
assert.equal(decoder.validateModels({object:'list', data:[entry]}).length, 1);
const event = value => `data: ${JSON.stringify({object:'chat.completion.chunk', model:'m', ...value})}\n\n`;
const wire = [
  event({choices:[{index:0, delta:{role:'assistant', content:'synthetic'}, finish_reason:null}]}),
  event({choices:[{index:0, delta:{}, finish_reason:'stop'}], usage:{prompt_tokens:2, completion_tokens:3, total_tokens:5}}),
  event({choices:[], possums:{outcome:'settled', status:'complete', reserved_microunits:'52', charged_microunits:'7', refunded_microunits:'45', markup_percent:130, quoted_input_microunits_per_million_tokens:'1000000', quoted_output_microunits_per_million_tokens:'1000000', quoted_cached_input_microunits_per_million_tokens:null, evidence:'final_metrics', usage:{prompt_tokens:2, completion_tokens:3, total_tokens:5, cached_tokens:1}, provider_cost_usd:'0.000005'}}),
  'data: [DONE]\n\n',
].join('');
function body(text, interrupted = false) {
  let sent = false;
  return new ReadableStream({pull(controller) {
    if (!sent) { sent = true; controller.enqueue(new TextEncoder().encode(text)); }
    else if (interrupted) controller.error(new Error('private-marker')); else controller.close();
  }}, {highWaterMark:0});
}
const receipt = await decoder.consumeCompletion(body(wire), entry, () => {});
assert.equal(receipt.chargedMicrounits, '7'); assert.equal(receipt.complete, true);
const partial = wire.replace('"status":"complete"', '"status":"incomplete"').replace('data: [DONE]\n\n', 'data: {"error":{"code":"generation_failed","detail":"stream_usage_invalid","message":"private-marker","billing":"unknown"}}\n\ndata: [DONE]\n\n');
await assert.rejects(decoder.consumeCompletion(body(partial), entry, () => {}), error => error instanceof decoder.GatewayError && error.settlement?.complete === false && error.billing === 'settled' && !error.message.includes('private-marker'));
await assert.rejects(decoder.consumeCompletion(body(partial, true), entry, () => {}), error => !error.settlement && !error.message.includes('private-marker'));
for (const cached of [undefined, 0, 1, 2]) {
  const charge = cached === undefined ? '7' : String(Math.ceil((5-cached)*1.3));
  const terminal = {outcome:'settled',status:'complete',reserved_microunits:'52',charged_microunits:charge,refunded_microunits:String(52-Number(charge)),markup_percent:130,
    quoted_input_microunits_per_million_tokens:'1000000',quoted_output_microunits_per_million_tokens:'1000000',quoted_cached_input_microunits_per_million_tokens:'0',evidence:'final_metrics',
    usage:{prompt_tokens:2,completion_tokens:3,total_tokens:5,...(cached===undefined?{}:{cached_tokens:cached})},provider_cost_usd:null};
  const prefix=event({choices:[{index:0,delta:{content:'synthetic'},finish_reason:'stop'}]});
  const entryWithCache={...entry,cached_input_microunits_per_million_tokens:'0'};
  assert.equal(decoder.validateModels({object:'list',data:[entryWithCache]})[0].cached_input_microunits_per_million_tokens,'0');
  const receipt=await decoder.consumeCompletion(body(prefix+event({choices:[],possums:terminal})+'data: [DONE]\n\n'),entryWithCache,()=>{});
  assert.equal(receipt.chargedMicrounits,charge);assert.equal(receipt.totalTokens,5);
  const capped={...terminal,reserved_microunits:'2',charged_microunits:'2',refunded_microunits:'0'};
  assert.equal((await decoder.consumeCompletion(body(prefix+event({choices:[],possums:capped})+'data: [DONE]\n\n'),entry,()=>{})).chargedMicrounits,'2');
  const partial=prefix+event({choices:[],possums:{...terminal,status:'incomplete'}})+'data: [DONE]\n\n';
  await assert.rejects(decoder.consumeCompletion(body(partial),entry,()=>{}),error=>error instanceof decoder.GatewayError && error.settlement?.chargedMicrounits===charge && error.settlement.complete===false);
  await assert.rejects(decoder.consumeCompletion(body(partial,true),entry,()=>{}),error=>!error.settlement);
  const malformed=structuredClone(terminal);malformed.usage.cached_tokens=3;
  await assert.rejects(decoder.consumeCompletion(body(prefix+event({choices:[],possums:malformed})+'data: [DONE]\n\n'),entry,()=>{}));
}
console.log(`shipped usage decoder passed (cache unknown/zero/partial/full, discounted/capped, partial/interrupted, malformed): sha256=${createHash('sha256').update(source).digest('hex')}`);
