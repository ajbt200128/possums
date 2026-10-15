// Offline qualification of the exact pinned gateway-error decoder body from a
// shipped Pi bundle. Declarations are extracted statically and never initialize
// the bundle's provider, authentication or network.
//
// Usage:
//   node tests/shipped_error_decoder.mjs <bundle.mjs> <typescript.js> [version-label]
//
// Evidence this test establishes (per artifact, from the artifact's own decoder,
// not from HEAD assumptions):
//   - accepted code/detail vocabulary and the locally authored description text
//   - unknown code/detail are rejected (no weakening of the closed contract)
//   - server-provided `message` is accepted as a string but never rendered or stored
//   - extra fields are either ignored (newer tolerant decoder) or rejected (older
//     strict decoder); the behavior is detected from the artifact
//   - candidate optional `interface_failure` diagnostic metadata is accepted and
//     ignored only where the artifact tolerates extra fields, and is never stored,
//     rendered, or allowed to authorize settlement/tool gates
//   - the `service_quiescing` pre-admission envelope keeps its exact shape and its
//     `not_submitted` billing is not reinterpreted as an accounting receipt
//   - duplicate_request outcome values stay bounded and never confirm settlement
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { pathToFileURL, fileURLToPath } from 'node:url';
import path from 'node:path';

const [bundlePath, compilerPath, labelArg] = process.argv.slice(2);
assert(bundlePath && compilerPath, 'usage: shipped_error_decoder.mjs <bundle.mjs> <typescript.js> [label]');
const label = labelArg ?? path.basename(path.dirname(bundlePath));

// --- Static extraction (same shape as tests/shipped_usage_decoder.mjs) --------
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
  assert(statement, `shipped error decoder declaration unavailable: ${name}`);
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
for (const name of ['gatewayError', 'GatewayError', 'gatewayDetailLabel', 'details']) select(name);
const code = ast.statements.filter(statement => selected.has(statement)).map(statement => statement.getText(ast)).join('\n');
// Only public, operator-selected shipped code, with no imports/provider activation.
const decoder = new Function(`${code}\nreturn {gatewayError, GatewayError, gatewayDetailLabel, details};`)();
assert(decoder.details instanceof Set, 'shipped detail vocabulary unavailable');

const sha256 = createHash('sha256').update(source).digest('hex');
const probe = fn => { try { return { ok: true, value: fn() }; } catch (error) { return { ok: false, error }; } };
const accept = (value, status) => {
  const result = probe(() => decoder.gatewayError(value, status));
  assert(result.ok, `expected envelope accepted: ${JSON.stringify(value)}`);
  return result.value;
};
const reject = (value, status) => {
  const result = probe(() => decoder.gatewayError(value, status));
  assert(!result.ok, `expected envelope rejected: ${JSON.stringify(value)}`);
};

// --- Closed vocabulary source of truth (src/failures.rs) ---------------------
const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const failuresSource = await readFile(path.join(repoRoot, 'src', 'failures.rs'), 'utf8');
const allBlock = failuresSource.match(/pub\(crate\) const ALL: \[Self; 56\] = \[([\s\S]*?)\];/);
assert(allBlock, 'src/failures.rs ALL array unavailable');
const variants = [...allBlock[1].matchAll(/Self::(\w+)/g)].map(match => match[1]);
const labelPairs = Object.fromEntries([...failuresSource.matchAll(/Self::(\w+) => "([a-z_]+)",/g)].map(match => [match[1], match[2]]));
const interfaceLabels = variants.map(variant => labelPairs[variant]);
assert.equal(variants.length, 56, 'expected 56 typed interface failures');
assert.equal(new Set(interfaceLabels).size, 56, 'expected 56 unique labels');

// --- Capability detection, from the artifact ---------------------------------
const legacyCodes = ['insufficient_credit', 'unauthorized', 'invalid_request', 'account_limit', 'duplicate_request', 'unavailable', 'generation_failed'];
const quiescingEnvelope = { error: { code: 'service_quiescing', stage: 'admission', constraint: 'service_quiescing', billing: 'not_submitted', message: 'synthetic' } };
const supportsQuiescing = probe(() => decoder.gatewayError({ error: { ...quiescingEnvelope.error } }, 503)).ok;
const toleratesExtraFields = probe(() => decoder.gatewayError({ error: { code: 'unavailable', __probe: 1 } })).ok;
const hasSettlementGetter = 'settlement' in decoder.GatewayError.prototype;

// --- Accepted code/detail vocabulary and description text --------------------
const rendersReasonCode = probe(() => decoder.gatewayError({ error: { code: 'unavailable' } }).message.includes('[unavailable]')).value === true;
for (const reason of legacyCodes) {
  const failure = accept({ error: { code: reason } });
  assert.equal(failure.reason, reason);
  assert.equal(failure.detail, undefined);
  assert.equal(failure.code, 'uncertain');
  assert.equal(failure.billing, 'unknown');
  assert.equal(failure.settlement, undefined);
  // The description is locally authored and carries a content-free code/stage/constraint.
  assert.match(failure.message, /Possums:/);
  if (rendersReasonCode) assert.ok(failure.message.includes(`[${reason}]`), `expected [${reason}] in description`);
}
for (const detail of decoder.details) {
  const failure = accept({ error: { code: 'unavailable', detail } });
  assert.equal(failure.detail, detail);
  assert.ok(failure.message.includes(`[${detail}]`));
}
reject({ error: { code: 'not_a_gateway_code' } });
reject({ error: { code: 'unavailable', detail: 'not_a_gateway_detail' } });
reject({ error: { code: 42 } });
reject({ error: { detail: 'inference_unavailable' } });

// --- Hostile server message is accepted as a string but never rendered/stored -
const hostile = 'HOSTILE-REVIEW-MARKER-7f3c';
for (const reason of legacyCodes) {
  const failure = accept({ error: { code: reason, message: hostile } });
  assert.ok(!failure.message.includes(hostile), 'hostile message leaked into description');
  assert.ok(!JSON.stringify(failure).includes(hostile), 'hostile message leaked into serialization');
  for (const value of Object.values(failure)) assert.notEqual(value, hostile, 'hostile message stored on error');
  assert.ok(!Object.keys(failure).includes('message') || failure.message === undefined);
}
reject({ error: { code: 'unavailable', message: 123 } });

// --- Extra-field tolerance, detected from the artifact -----------------------
if (toleratesExtraFields) {
  const failure = accept({ error: { code: 'unavailable', __probe: 1 } });
  assert.ok(!Object.hasOwn(failure, '__probe'), 'unknown field copied onto GatewayError');
} else {
  reject({ error: { code: 'unavailable', __probe: 1 } });
}

// --- Tokenizer send refinements retain the exact shipped default envelope ----
for (const cause of ['tokenizer_transport_failed', 'tokenizer_send_deadline', 'tokenizer_connect_failed']) {
  const failure = accept({ error: {
    code: 'unavailable', detail: 'tokenizer_send_failed', billing: 'unknown',
    message: `[${cause}] Stage: tokenizer_send; constraint: ${cause}. Not replayed. Billing unknown; a new submission may incur another charge.`,
  } }, 503);
  assert.equal(failure.detail, 'tokenizer_send_failed');
  assert.equal(failure.billing, 'unknown');
  assert.equal(failure.settlement, undefined);
  assert.ok(!failure.message.includes(cause)); // Shipped Pi ignores gateway messages.
}

// --- Candidate optional `interface_failure` diagnostic metadata --------------
// Diagnosis only: it must never be stored, rendered, or authorize settlement/tools.
const envelopeFamilies = [
  ['unavailable', 'Gateway unavailable; the failing stage/constraint is reported in the code fields.'],
  ['unauthorized', 'Gateway rejected authentication; verify credentials before retrying.'],
  ['invalid_request', 'Request rejected by gateway; correct the request before retrying.'],
  ['insufficient_credit', 'Insufficient credit for the selected model reservation.'],
  ['account_limit', 'Account limit reached; wait before submitting a new request.'],
];
for (const [code, message] of envelopeFamilies) {
  // Legacy envelope without metadata is accepted by every artifact.
  const legacy = accept({ error: { code, message } });
  assert.equal(legacy.reason, code);
  assert.equal(legacy.settlement, undefined);

  for (const interfaceFailure of interfaceLabels) {
    const value = { error: { code, message, interface_failure: interfaceFailure } };
    if (toleratesExtraFields) {
      const failure = accept(value);
      assert.equal(failure.reason, code, 'metadata must not change the reason');
      assert.equal(failure.settlement, undefined, 'metadata must not authorize settlement');
      assert.equal(failure.billing, 'unknown', 'metadata must not assert billing');
      assert.ok(!failure.message.includes(interfaceFailure), 'metadata label leaked into description');
      assert.ok(!JSON.stringify(failure).includes(interfaceFailure), 'metadata label stored on error');
      assert.ok(!Object.hasOwn(failure, 'interface_failure'), 'metadata copied onto GatewayError');
    } else {
      // Older strict decoders reject any unknown field: the candidate envelope is
      // NOT backward compatible with them. Recorded as a compatibility gap.
      reject(value);
    }
  }
}

// --- service_quiescing exact pre-admission envelope --------------------------
if (supportsQuiescing) {
  const failure = accept({ error: { ...quiescingEnvelope.error } }, 503);
  assert.equal(failure.reason, 'service_quiescing');
  assert.equal(failure.detail, undefined);
  assert.equal(failure.status, 503);
  // `billing: "not_submitted"` is an admission hint, never an accounting receipt.
  assert.equal(failure.billing, 'unknown');
  assert.equal(failure.settlement, undefined);
  reject({ error: { ...quiescingEnvelope.error, stage: 'gateway' } }, 503);
  reject({ error: { ...quiescingEnvelope.error, constraint: 'other' } }, 503);
  reject({ error: { ...quiescingEnvelope.error, billing: 'refunded' } }, 503);
  reject({ error: { ...quiescingEnvelope.error } }, 200);
  reject({ error: { code: 'service_quiescing', stage: 'admission' } }, 503);
  // Optional diagnostic metadata is ignored where tolerated and never changes billing.
  const withMetadata = probe(() => decoder.gatewayError({ error: { ...quiescingEnvelope.error, interface_failure: 'auth_state_unknown' } }, 503));
  if (toleratesExtraFields) {
    assert(withMetadata.ok, 'tolerant decoder should ignore metadata on the quiescing envelope');
    assert.equal(withMetadata.value.billing, 'unknown');
    assert.equal(withMetadata.value.settlement, undefined);
    assert.ok(!withMetadata.value.message.includes('auth_state_unknown'));
  } else {
    assert(!withMetadata.ok, 'strict decoder should reject metadata on the quiescing envelope');
  }
} else {
  // Pre-forwarding artifacts predate the distinct envelope and reject the code.
  reject({ error: { ...quiescingEnvelope.error } }, 503);
}

// --- Duplicate outcomes stay bounded and never confirm settlement/tools ------
const duplicate = accept({ error: { code: 'duplicate_request', outcome: 'in_flight' } });
assert.equal(duplicate.reason, 'duplicate_request');
assert.equal(duplicate.settlement, undefined);
assert.equal(duplicate.billing, 'unknown');
for (const outcome of ['settled', 'refunded']) {
  const failure = accept({ error: { code: 'duplicate_request', outcome } });
  assert.equal(failure.settlement, undefined, 'duplicate outcome must not confirm a receipt');
  assert.equal(failure.billing, 'unknown', 'duplicate outcome must not confirm billing');
}
reject({ error: { code: 'duplicate_request', outcome: 'unknown_outcome' } });
reject({ error: { code: 'unavailable', outcome: 'settled' } });
reject({ error: { code: 'duplicate_request', outcome: 7 } });

// --- A claimed refund is not a confirmed refund ------------------------------
const claimed = accept({ error: { code: 'generation_failed', detail: 'stream_finish_missing', billing: 'refunded' } });
assert.equal(claimed.billing, 'unknown', 'refund claim alone must not confirm billing');
assert.equal(claimed.settlement, undefined);
assert.ok(!claimed.message.includes('Reservation refunded'), 'unconfirmed refund must not be rendered as confirmed');
reject({ error: { code: 'generation_failed', billing: 'settled' } });

// --- Evidence summary --------------------------------------------------------
const drift = {
  label,
  sha256,
  details: decoder.details.size,
  legacyCodes: legacyCodes.length,
  supportsQuiescing,
  toleratesExtraFields,
  hasSettlementGetter,
  rendersReasonCode,
  interfaceLabels: interfaceLabels.length,
  candidateMetadataAccepted: toleratesExtraFields,
};
console.log(`shipped error decoder passed (${label}): ${JSON.stringify(drift)}`);
