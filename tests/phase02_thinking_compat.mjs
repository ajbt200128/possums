// Manual offline exact shipped-artifact decoder check. No default extension is
// registered, no secrets read, and no network or inference initiated.
import assert from 'node:assert/strict';
import { readFile, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
const artifacts=JSON.parse(process.env.POSSUMS_OLD_ARTIFACTS ?? '[]');
assert(artifacts.length>0 && process.env.PHASE02_COMPAT_SCRATCH,'explicit public artifact list and scratch directory required');
// This exact default response is asserted from the candidate route in api_models.rs.
const wire={object:'list',data:[{object:'model',id:'synthetic',context_tokens:'100',max_output_tokens:'100',input_microunits_per_million_tokens:'1000000',output_microunits_per_million_tokens:'1000000',maximum_reservation_microunits:'260'}]};
for(const [index,artifact] of artifacts.entries()){
 const source=await readFile(artifact,'utf8');
 assert(source.includes('function validateModels(') && source.includes('function encodeChat('));
 const scratch=path.join(process.env.PHASE02_COMPAT_SCRATCH,`old-artifact-${index}.mjs`);
 // Add only test exports; leave the decoder, helpers, and SDK dependency bytes intact.
 await writeFile(scratch,source+'\nexport { validateModels as oldValidateModels, encodeChat as oldEncodeChat };\n');
 const old=await import(pathToFileURL(scratch).href);
 assert.deepEqual(old.oldValidateModels(wire),wire.data);
 const opted=structuredClone(wire);opted.data[0].reasoning=true;
 let strict=false;try{old.oldValidateModels(opted);}catch{strict=true;}
 const chat=JSON.parse(new TextDecoder().decode(old.oldEncodeChat({model:'synthetic',stream:true,submission:'C'.repeat(43),messages:[{role:'user',content:'synthetic'}]})));
 assert.equal(chat.reasoning_effort,undefined);assert.equal(chat.chat_template_kwargs,undefined);
 console.log(JSON.stringify({artifactSha256:createHash('sha256').update(source).digest('hex'),strictCatalogDecoder:strict,defaultCandidateCatalog:'passed',defaultOldRequestOmission:'passed',scope:'offline exact decoder/encoder only; no authentication/stream/live inference'}));
}
