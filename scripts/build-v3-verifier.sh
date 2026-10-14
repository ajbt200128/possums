#!/bin/sh
set -eu
REPO=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
[ "$(go version)" = 'go version go1.27.2 '"$(go env GOHOSTOS)/$(go env GOHOSTARCH)" ] || { printf '%s\n' 'Go 1.27.2 required' >&2; exit 1; }
[ "$(node --version)" = v24.13.0 ] || { printf '%s\n' 'Node 24.13.0 required' >&2; exit 1; }
# nix develop removes its own TMPDIR on exit; retain the scratch artifacts.
OUT=$(mktemp -d /tmp/possums-v3-build-XXXXXX)
cd "$REPO/attestation-helper"
go mod verify
CGO_ENABLED=0 go build -mod=readonly -trimpath -buildvcs=false -ldflags='-s -w' -o "$OUT/possums-attestation" .
CGO_ENABLED=0 GOOS=js GOARCH=wasm go build -mod=readonly -trimpath -buildvcs=false -ldflags='-s -w' -o "$OUT/verifier.wasm" ./cmd/verifier-wasm
cp "$(go env GOROOT)/lib/wasm/wasm_exec.js" "$OUT/wasm_exec.cjs"
cp "$REPO/clients/pi/GO-LICENSE" "$OUT/GO-LICENSE"
cp "$REPO/clients/pi/verifier.mjs" "$REPO/clients/pi/verifier-worker.mjs" "$REPO/clients/pi/verifier.d.mts" "$OUT/"
node --input-type=module - "$OUT" "$REPO" <<'JS'
import { readFile, writeFile, readdir } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import path from 'node:path';
const [out, repo] = process.argv.slice(2);
const hash = async file => createHash('sha256').update(await readFile(file)).digest('hex');
const artifacts = {};
for (const name of (await readdir(out)).sort()) artifacts[name] = { sha256: await hash(path.join(out, name)) };
const sources = {};
for (const directory of ['attestation-helper', 'clients/pi']) {
  async function visit(relative) {
    for (const entry of await readdir(path.join(repo, relative), { withFileTypes: true })) {
      const file = path.join(relative, entry.name);
      if (entry.isDirectory() && directory === 'attestation-helper') await visit(file);
      const publicSource = directory === 'attestation-helper'
        ? entry.name.endsWith('.go') || ['go.mod', 'go.sum'].includes(entry.name)
        : ['verifier.mjs', 'verifier-worker.mjs', 'verifier.d.mts'].includes(entry.name);
      if (entry.isFile() && publicSource) sources[file] = await hash(path.join(repo, file));
    }
  }
  await visit(directory);
}
for (const file of ['scripts/build-v3-verifier.sh', 'clients/pi/GO-LICENSE', 'tests/v3_verifier_wasm.mjs']) sources[file] = await hash(path.join(repo, file));
await writeFile(path.join(out, 'verifier-build-report.json'), JSON.stringify({
  go: '1.27.2', node: process.version, sdk: 'v0.17.0-rc.1.0.20261009012920-31c57af7d7b4', sources, artifacts,
}, null, 2) + '\n');
console.log(JSON.stringify({ verifierBuild: out, scope: 'scratch native/WASM artifacts only; no installation or network verification' }));
JS
