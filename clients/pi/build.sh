#!/bin/sh
set -eu
SOURCE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO=$(CDPATH= cd -- "$SOURCE/../.." && pwd)
PI_ROOT=${POSSUMS_PI_ROOT:?Set POSSUMS_PI_ROOT to the pinned Pi 1.0.4 release directory}
NODE=${POSSUMS_NODE:-$(command -v node)}
NPM=$(command -v npm)
[ "$("$NODE" --version)" = v24.13.0 ] || { printf '%s\n' 'Node 24.13.0 required' >&2; exit 1; }
for PACKAGE in pi-ai pi-coding-agent pi-agent-core; do
  [ -f "$PI_ROOT/node_modules/@earendil-works/$PACKAGE/package.json" ] || { printf '%s\n' 'Pinned Pi installation missing' >&2; exit 1; }
done
# nix develop removes its own TMPDIR on exit; keep reviewable artifacts.
S=$(mktemp -d /tmp/possums-pi-build-XXXXXX)
mkdir -p "$S/source/clients/pi" "$S/source/examples/phase01" "$S/home" "$S/tmp" "$S/cache" "$S/checks/source"
cp "$SOURCE"/*.ts "$SOURCE"/*.mts "$SOURCE"/*.mjs "$SOURCE"/*.json "$S/source/clients/pi/"
# Build the same pinned offline core, unless the parent supplied a scratch build.
V3=${POSSUMS_VERIFIER_BUILD:-}
if [ -z "$V3" ]; then
  sh "$REPO/scripts/build-v3-verifier.sh" > "$S/verifier-build-output"
  V3=$("$NODE" -e 'const fs=require("node:fs"); const lines=fs.readFileSync(process.argv[1],"utf8").trim().split("\n"); process.stdout.write(JSON.parse(lines.at(-1)).verifierBuild)' "$S/verifier-build-output")
fi
"$NODE" --input-type=module - "$REPO" "$V3" <<'JS'
import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import path from 'node:path';
const [repo, build] = process.argv.slice(2);
const report = JSON.parse(await readFile(path.join(build, 'verifier-build-report.json'), 'utf8'));
for (const [name, expected] of Object.entries(report.sources)) {
  if (!name.startsWith('attestation-helper/')) continue;
  const actual = createHash('sha256').update(await readFile(path.join(repo, name))).digest('hex');
  if (actual !== expected) throw new Error('verifier source changed; rebuild before packaging');
}
JS
mkdir "$S/verifier"
cp "$V3/verifier.wasm" "$V3/wasm_exec.cjs" "$V3/GO-LICENSE" "$V3/verifier-build-report.json" "$S/verifier/"
cp "$REPO/examples/phase01"/*.ts "$S/source/examples/phase01/"
# Retain the exact pre-migration acquisition fixture for legacy-v2 regression
# tests only. The authorizing extension never imports this scratch-only file.
cp "$REPO/tests/fixtures/pi-v2-bootstrap.ts" "$S/source/clients/pi/legacy-bootstrap.ts"
"$NODE" -e 'const fs=require("node:fs"),crypto=require("node:crypto"); if(crypto.createHash("sha256").update(fs.readFileSync(process.argv[1])).digest("hex")!=="74a0d6e01fdbb60b9a4d959889f6a0eae62bdfc8d5cfc28efc1b47144eae9f61") throw new Error("possums_build_legacy_fixture: retained bootstrap bytes changed; restore the pinned fixture before building")' "$S/source/clients/pi/legacy-bootstrap.ts"
for TEST in phase01_client phase01_transport phase01_stream_limits tinfoil_forwarding_client phase02_client phase02_pi phase02_thinking phase02_diagnostics phase02_release phase02_cache phase02_discovery phase02_v3 phase02_v3_package; do
  cp "$REPO/tests/$TEST.mjs" "$S/checks/source/"
done
: > "$S/user.npmrc"
: > "$S/global.npmrc"
cd "$S/source/clients/pi"
env -i HOME="$S/home" TMPDIR="$S/tmp" PATH="$(dirname "$NODE"):$(dirname "$NPM"):/usr/bin:/bin" \
  npm_config_userconfig="$S/user.npmrc" npm_config_globalconfig="$S/global.npmrc" \
  "$NPM" ci --legacy-peer-deps --ignore-scripts --no-audit --no-fund --registry=https://registry.npmjs.org --cache="$S/cache"
mkdir -p node_modules/@earendil-works
for PACKAGE in pi-ai pi-coding-agent pi-agent-core; do
  ln -s "$PI_ROOT/node_modules/@earendil-works/$PACKAGE" "node_modules/@earendil-works/$PACKAGE"
done
ln -s ../../clients/pi/node_modules "$S/source/examples/phase01/node_modules"
env -i HOME="$S/home" TMPDIR="$S/tmp" PATH="$(dirname "$NODE"):/usr/bin:/bin" PHASE02_OUT="$S/package" POSSUMS_VERIFIER_BUILD="$S/verifier" \
  "$NODE" build.mjs
# build.mjs already links the runtime peers into the scratch package.
printf '\nBuilt package: %s\n' "$S/package"
printf 'Local checks: POSSUMS_PI_ROOT=%s %s %s/source/clients/pi/check.mjs\n' "$PI_ROOT" "$NODE" "$S"
printf 'Load with pinned Pi: pi --no-session --no-extensions --no-tools -e %s/extension.mjs\n' "$S/package"
