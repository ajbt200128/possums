// Synthetic offline fixtures against the current reference/Pi decoders. No sockets.
// INTAKE_DIAGNOSTIC_BUILD is the scratch clients/pi/test-entry.ts bundle.
import assert from 'node:assert/strict';
import { pathToFileURL } from 'node:url';
const m = await import(pathToFileURL(process.env.INTAKE_DIAGNOSTIC_BUILD).href);
const hostile = 'PRIVATE_PROMPT_CREDENTIAL_URL_EXCEPTION';
const token = 'a'.repeat(43);
const model = { object: 'model', id: 'synthetic', context_tokens: '20', max_output_tokens: '20',
  input_microunits_per_million_tokens: '1000000', output_microunits_per_million_tokens: '1000000', maximum_reservation_microunits: '52', tool_protocol: 'openai-functions-v1' };
const diagnostic = (stage, constraint, code = 'INTAKE_REMOTE_UNAVAILABLE', status = 503, operation = 'intake_reserve') => ({code, operation, stage, constraint, status});
const wire = intake => ({error:{code:'unavailable', message:hostile, intake}});
const frame = value => `data: ${typeof value === 'string' ? value : JSON.stringify(value)}\n\n`;
const stream = (values, interrupted = false) => {
  let sent = false;
  return new ReadableStream({pull(controller) {
    if (!sent) { sent = true; controller.enqueue(new TextEncoder().encode(values.map(frame).join(''))); }
    else if (interrupted) controller.error(new Error(hostile)); else controller.close();
  }}, {highWaterMark:0});
};
const cutoffDiagnostics = [
  diagnostic('transaction_completion','commit_evidence','INTAKE_REMOTE_UNAVAILABLE',503,'intake_settle'),
  diagnostic('settlement','immutable_hold_expiry','INTAKE_HOLD_EXPIRED',409,'intake_settle'),
];
const transportConstraints = ['configuration','input','encoding_limit','encoding_unknown','client_unknown',
  'request_unknown','connect_unknown','transport_unknown','deadline','response_encoding','response_limit',
  'response_read_unknown','response_json'];
const matrix = [
  diagnostic('writer_gate','writer_wait'),
  diagnostic('transaction_completion','commit_evidence'),
  diagnostic('binding','principal_binding','INTAKE_BINDING_CHANGED',200,'intake_refresh'),
  diagnostic('token_metadata','original_expiry','INTAKE_AUTHENTICATION_UNCONFIRMED',null,'intake_refresh'),
  diagnostic('auth_methods','password_method','INTAKE_AUTHENTICATION_UNCONFIRMED',200,'intake_auth_methods'),
  diagnostic('settlement','immutable_hold_expiry','INTAKE_HOLD_EXPIRED',409,'intake_settle'),
  diagnostic('settlement','first_terminal_wins','INTAKE_TERMINAL_MISMATCH',200,'intake_refund'),
  diagnostic('generation','work_deadline','INTAKE_HOLD_WORK_DEADLINE',null,'generation'),
  diagnostic('admission','usable_hold','INTAKE_HOLD_UNUSABLE',null),
  ...cutoffDiagnostics,
  ...['active_reservation','expected_not_after_authoritative_deadline','strictly_later_deadline','renewal_observation']
    .map(constraint => diagnostic('renewal',constraint,'INTAKE_PROTOCOL_UNCONFIRMED',409,'intake_renew')),
  ...transportConstraints.map(constraint => diagnostic(constraint === 'response_json' ? 'decode' : 'transport', constraint,
    constraint === 'response_json' ? 'INTAKE_PROTOCOL_UNCONFIRMED' : constraint === 'input' ? 'INTAKE_INPUT_INVALID' : 'INTAKE_REQUEST_UNAVAILABLE',
    constraint.startsWith('response_') ? 200 : null)),
];
const originalFetch = globalThis.fetch;
let unexpectedWork = 0;
globalThis.fetch = async () => { unexpectedWork++; throw new Error('offline_fixture_forbids_network'); };
try {
  for (const intake of matrix) {
    const failure = m.gatewayError(wire(intake),503);
    assert.deepEqual(failure.intakeDiagnostic,intake);
    assert(Object.isFrozen(failure.intakeDiagnostic));
    assert.throws(()=>{failure.intakeDiagnostic.status=201;});
    intake.status = intake.status; // A later caller mutation cannot affect the carrier.
    assert.equal(failure.billing,'unknown'); assert.equal(failure.settlement,undefined);
    assert(failure.message.includes(intake.code)); assert(failure.message.includes(intake.constraint));
    assert(!failure.message.includes(hostile)); assert.equal(failure.cause,undefined);
    const catalog = new m.CatalogFailure('http','rejected',503,wire(intake));
    const connection = m.catalogConnectionFailure(catalog);
    assert(connection.message.includes(intake.code)); assert(connection.message.includes(intake.constraint));
    assert.deepEqual(connection.intakeDiagnostic,intake);
    assert(!connection.message.includes(hostile));
    await assert.rejects(m.consumeCompletion(stream([wire(intake)]),'synthetic',()=>{}), error =>
      error instanceof m.GatewayError && error.intakeDiagnostic.constraint === intake.constraint && error.billing === 'unknown' && !error.message.includes(hostile));
  }
  const valid = matrix[0];
  for (const intake of [null, {}, {...valid,extra:hostile}, {...valid,code:hostile}, {...valid,operation:hostile},
    {...valid,stage:hostile}, {...valid,constraint:hostile}, {...valid,status:99}, {...valid,status:600},
    {...valid,status:503.5}, {...valid,status:'503'}, {...valid,status:undefined}, {...valid,code:'x'.repeat(100000)}]) {
    const failure = m.gatewayError(wire(intake));
    assert.equal(failure.intakeDiagnostic,undefined); assert.equal(failure.billing,'unknown');
    assert(!failure.message.includes(hostile)); assert(!JSON.stringify(failure).includes(hostile));
  }
  const unknown = m.gatewayError({error:{code:hostile,intake:valid,billing:'refunded'}});
  assert.equal(unknown.reason,'unclassified'); assert.equal(unknown.billing,'unknown'); assert(!unknown.message.includes(hostile));
  assert.throws(()=>m.gatewayError({error:{code:hostile,message:hostile}}));
  const legacy = m.gatewayError({error:{code:'unavailable',message:hostile}});
  assert.equal(legacy.intakeDiagnostic,undefined); assert.match(legacy.message,/Gateway unavailable/); assert(!legacy.message.includes(hostile));
  assert.equal(unexpectedWork,0);
  console.log('PASS closed projection, hostile/extra/invalid status, legacy decoder, no diagnostic work');

  // Actual Channel helpers: inspect the plaintext POST before EHBP encrypts it;
  // use controlled SDK response decoding rather than weakening response binding.
  const encrypt = m.Identity.prototype.encryptRequestWithContext;
  const decrypt = m.Identity.prototype.decryptResponseWithContext;
  let postHeaders = 0;
  m.Identity.prototype.encryptRequestWithContext = function(request) {
    assert.equal(request.headers.get('x-possums-intake-diagnostics'),'v1'); postHeaders++;
    return encrypt.call(this,request);
  };
  m.Identity.prototype.decryptResponseWithContext = async function() { return Response.json(wire(matrix[2])); };
  const channel = await m.Channel.fixture('https://localhost:18443',new Uint8Array([0,0,32,...Array(32).fill(7),0,4,0,1,0,2]),'07'.repeat(32));
  let requests = 0;
  globalThis.fetch = async request => {
    requests++; assert.equal(request.headers.get('x-possums-intake-diagnostics'),'v1');
    if (request.method === 'GET') return Response.json(wire(matrix[2]),{status:503});
    return new Response(new Uint8Array([0,0,0,16,...Array(16).fill(0)]),{status:503,headers:{'Ehbp-Response-Nonce':'0'.repeat(64)}});
  };
  try {
    await assert.rejects(channel.models(token), error => error instanceof m.CatalogFailure && error.intakeDiagnostic.code === matrix[2].code);
    for (const [path,payload,bearer,stage] of [
      ['/v1/sessions',{challenge:token,credential:token},undefined,'authentication'],
      ['/v1/submissions',{model:'synthetic',new_conversation:false},token,'submission'],
    ]) await assert.rejects(channel.control(path,payload,bearer), error => {
      assert(error instanceof m.DiagnosticFailure); assert(error instanceof m.IntakeFailure);
      assert(!(error instanceof m.GatewayError)); // No new setup-recovery category.
      assert.equal(error.stage,stage); assert.equal(error.constraint,'http'); assert.equal(error.code,'uncertain');
      assert.equal(error.intakeDiagnostic.code,matrix[2].code);
      const connection = m.connectionFailure(error);
      assert(connection.message.includes(matrix[2].code)); assert(!connection.message.includes(hostile)); return true;
    });
    assert.equal(requests,3); assert.equal(postHeaders,2);
  } finally { m.Identity.prototype.encryptRequestWithContext=encrypt; m.Identity.prototype.decryptResponseWithContext=decrypt; }
  console.log('PASS actual Channel GET/control opt-in and diagnostic propagation without recovery expansion');

  // Actual ReferenceClient preparation/decoder and Pi warning-only provider path.
  // Only transport results are synthetic; no financial or completion authorization
  // comes from the diagnostic itself. No token/balance recomputation is introduced.
  for (const [index,intake] of matrix.entries()) {
    const fixture = await m.Channel.fixture('https://localhost:18443',new Uint8Array([0,0,32,...Array(32).fill(7),0,4,0,1,0,2]),'07'.repeat(32));
    let sends=0, controls=0;
    fixture.challenge=async()=>({challenge:token,expires_in:600});
    fixture.models=async()=>({object:'list',data:[model]});
    fixture.control=async path=>{controls++; return path==='/v1/sessions'?{token,token_type:'Bearer',expires_in:43200}:{submission:token};};
    const paid = intake.code === 'INTAKE_TERMINAL_MISMATCH';
    const terminal = {object:'chat.completion.chunk',model:'synthetic',choices:[],possums:{outcome:'settled',status:'incomplete',charged_microunits:'7',refunded_microunits:'45'}};
    const provisional = {object:'chat.completion.chunk',model:'synthetic',choices:[{index:0,delta:{role:'assistant',content:'retained synthetic text',tool_calls:[{index:0,id:'synthetic_call',type:'function',function:{name:'echo',arguments:'{}'}}]},finish_reason:null}]};
    const detail = cutoffDiagnostics.includes(intake) ? 'stream_deadline_exceeded' : 'settlement_failed';
    fixture.chat=async()=>{sends++; return stream([provisional,...(paid?[terminal]:[]),{error:{code:'generation_failed',detail,billing:'unknown',message:hostile,intake}},...(paid?['[DONE]']:[])],index===6);};
    const provider = new m.PossumsProvider(async()=>new m.ReferenceClient(fixture));
    provider.newSession();
    await provider.auth.apiKey.login({signal:new AbortController().signal,prompt:async()=>token});
    provider.beginRun();
    const selected=provider.getModels()[0];
    const response=provider.streamSimple(selected,{messages:[{role:'user',content:'synthetic',timestamp:1}],tools:[]});
    for await (const _event of response) { /* drain native events */ }
    const message=await response.result();
    assert.equal(message.stopReason,'stop'); assert.equal(message.errorMessage,undefined);
    assert(!message.content.some(block=>block.type==='toolCall'));
    const warning=message.diagnostics.find(item=>item.type==='possums_turn_warning');
    assert(warning,JSON.stringify(message)); assert(warning.details.message.includes(intake.code),warning.details.message);
    assert(warning.details.message.includes(intake.constraint)); assert.equal(warning.details.backendGenerationComplete,false); assert.equal(warning.details.toolsAuthorized,false);
    if (cutoffDiagnostics.includes(intake)) assert(warning.details.message.includes('[stream_deadline_exceeded]'));
    assert(!JSON.stringify(message).includes(hostile));
    assert.equal(sends,1); assert.equal(controls,2); // One login/submission; no diagnostic work/replay.
    if(paid) {
      assert.equal(message.usage.cost.total,7/1e6);
      const receipt=message.diagnostics.find(item=>item.type==='possums_settled_receipt');
      assert.equal(receipt.details.chargedMicrounits,'7'); assert.equal(receipt.details.complete,false);
    } else assert(message.diagnostics.some(item=>item.type==='possums_billing_unknown'));
    provider.logout();
  }
  console.log('PASS actual reference/Pi warnings, mismatch monetary receipt and interrupted error preservation, no tools/replay');
} finally { globalThis.fetch=originalFetch; }
