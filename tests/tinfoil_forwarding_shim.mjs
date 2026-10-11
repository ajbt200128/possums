// Actual pinned shim + EHBP; only isolated synthetic identity/account/content.
// Native Node HTTP sees trailers; Fetch/client receipts do not depend on them.
import assert from 'node:assert/strict';
import https from 'node:https';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';
import path from 'node:path';
const dir=process.env.PHASE01_FIXTURE_DIR;
const meta=JSON.parse(await readFile(path.join(dir,'fixture.json'),'utf8'));
const require=createRequire(path.join(process.env.PHASE01_INSTALL,'package.json'));
const { Identity }=await import(pathToFileURL(require.resolve('ehbp')));
const identity=await Identity.unmarshalPublicConfig(Uint8Array.from(Buffer.from(meta.config,'hex')));
for (const optIn of [false,true]) {
  const headers={ 'content-type':'application/json',authorization:'Bearer '+'a'.repeat(43) };
  if(optIn) headers['X-Tinfoil-Request-Usage-Metrics']='true';
  const encrypted=await identity.encryptRequestWithContext(new Request(meta.origin+'/v1/chat/completions',{
    method:'POST',headers,body:JSON.stringify({model:'native-trailer',stream:true,messages:[{role:'user',content:'synthetic'}]}),
  }));
  const body=new Uint8Array(await encrypted.request.arrayBuffer());
  const response=await new Promise((resolve,reject)=>{
    const req=https.request(encrypted.request.url,{method:'POST',headers:Object.fromEntries(encrypted.request.headers),agent:false},res=>{
      const chunks=[];let size=0;
      res.on('data',chunk=>{size+=chunk.length;if(size>4096){req.destroy();reject(new Error('fixture_transport_bound'));}else chunks.push(chunk);});
      res.on('end',()=>resolve({status:res.statusCode,headers:res.headers,trailers:res.trailers,bytes:Buffer.concat(chunks)}));
      res.on('error',()=>reject(new Error('fixture_response_transport')));
    });
    req.on('error',()=>reject(new Error('fixture_request_transport')));
    req.end(body);
  });
  assert.equal(response.status,200);
  assert.equal(response.trailers['x-tinfoil-usage-metrics'],optIn?'prompt=2,completion=1,total=3,model=native-trailer':undefined);
  const decrypted=await identity.decryptResponseWithContext(new Response(response.bytes,{headers:response.headers}),encrypted.context);
  assert.equal(await decrypted.text(),'data: synthetic-native\n\n');
}
const stats=await (await fetch(meta.origin+'/fixture/stats')).json();
assert.equal(stats.calls,2);assert.equal(stats.encrypted,2);assert.equal(stats.authorized,2);
await fetch(meta.origin+'/fixture/stop');
console.log('PASS pinned shim/EHBP native HTTP opt-in trailer, authenticated body EOF, exactly two deliberate requests; Fetch trailer support not claimed');
