#!/bin/sh
set -eu
SOURCE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO=$(CDPATH= cd -- "$SOURCE/../.." && pwd)
cache_input_error() {
  printf '%s\n' 'possums_pi_build_cache_input: invalid build-only npm cache input. Use an existing absolute nonsymlink directory owned by the caller containing only public scratch cache, and POSSUMS_PI_NPM_OFFLINE=0 or 1.' >&2
  exit 1
}
OFFLINE=${POSSUMS_PI_NPM_OFFLINE-0}
case "$OFFLINE" in 0|1) ;; *) cache_input_error ;; esac
NODE=${POSSUMS_NODE:-$(command -v node)}
# This is a build-only handoff. The caller must supply only public scratch cache,
# never a personal cache/configuration; ownership does not establish its contents.
if [ "${POSSUMS_PI_NPM_CACHE+x}" = x ]; then
  "$NODE" --input-type=module - "$POSSUMS_PI_NPM_CACHE" 2>/dev/null <<'JS' || cache_input_error
import fs from 'node:fs';
import path from 'node:path';
const cache = process.argv[2];
try {
  if (!path.isAbsolute(cache)) process.exit(1);
  const stat = fs.lstatSync(cache.replace(/\/+$/, '') || '/');
  if (!stat.isDirectory() || stat.isSymbolicLink() || stat.uid !== process.getuid()) process.exit(1);
} catch {
  process.exit(1);
}
JS
fi
PI_ROOT=${POSSUMS_PI_ROOT:?Set POSSUMS_PI_ROOT to the pinned Pi 1.3.0 release directory}
NPM=$(command -v npm)
[ "$("$NODE" --version)" = v24.13.0 ] || { printf '%s\n' 'Node 24.13.0 required' >&2; exit 1; }
for PACKAGE in pi-ai pi-coding-agent pi-agent-core; do
  [ -f "$PI_ROOT/node_modules/@earendil-works/$PACKAGE/package.json" ] || { printf '%s\n' 'Pinned Pi installation missing' >&2; exit 1; }
done
# nix develop removes its own TMPDIR on exit; keep reviewable artifacts.
S=$(mktemp -d /tmp/possums-pi-build-XXXXXX)
mkdir -p "$S/source/clients/pi" "$S/source/clients/pi/core" "$S/home" "$S/tmp" "$S/cache" "$S/checks/source"
cp "$SOURCE"/*.ts "$SOURCE"/*.mts "$SOURCE"/*.mjs "$SOURCE"/*.json "$SOURCE/build.sh" "$S/source/clients/pi/"
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
NATIVE=${POSSUMS_TOR_NATIVE_BUILD:-}
if [ -z "$NATIVE" ]; then
  case "$(uname -s):$(uname -m)" in
    Darwin:arm64) NATIVE_TARGET=aarch64-apple-darwin ;;
    Darwin:x86_64) NATIVE_TARGET=x86_64-apple-darwin ;;
    Linux:aarch64|Linux:arm64) NATIVE_TARGET=aarch64-unknown-linux-musl ;;
    Linux:x86_64) NATIVE_TARGET=x86_64-unknown-linux-musl ;;
    *) printf '%s\n' 'possums_build_tor_platform: no approved native helper target exists for this source-build host. Use a supported macOS/Linux arm64/x64 build runner.' >&2; exit 1 ;;
  esac
  sh "$REPO/scripts/build-tor-native.sh" --target "$NATIVE_TARGET" > "$S/tor-native-build-output"
  NATIVE=$("$NODE" -e 'const fs=require("node:fs"); const lines=fs.readFileSync(process.argv[1],"utf8").trim().split("\n"); process.stdout.write(JSON.parse(lines.at(-1)).torNativeBuild)' "$S/tor-native-build-output")
fi
mkdir -p "$S/source/clients/pi/tor-native/src" "$S/tor-native"
cp "$REPO/clients/pi/tor-native/Cargo.toml" "$REPO/clients/pi/tor-native/Cargo.lock" "$S/source/clients/pi/tor-native/"
cp "$REPO/clients/pi/tor-native/src/main.rs" "$S/source/clients/pi/tor-native/src/"
cp "$NATIVE/possums-tor-native" "$NATIVE/tor-native-build-report.json" "$S/tor-native/"
mkdir "$S/verifier"
cp "$V3/verifier.wasm" "$V3/wasm_exec.cjs" "$V3/GO-LICENSE" "$V3/verifier-build-report.json" "$S/verifier/"
cp "$REPO/clients/pi/core"/*.ts "$S/source/clients/pi/core/"
cp "$REPO/clients/pi/core"/*.json "$S/source/clients/pi/core/"
for TEST in intake_diagnostics phase01_client phase01_transport phase01_stream_limits tinfoil_forwarding_client phase02_client phase02_pi phase02_thinking phase02_diagnostics phase02_cache phase02_manifest_cache phase02_discovery phase02_v3 phase02_v3_package phase02_peer_recovery pi_release_metadata pi_native_artifacts pi_distribution pi_pack tor_channel tor_selection tor_diagnostics tor_client_failures tor_identity_verifier_wasm tor_setup; do
  cp "$REPO/tests/$TEST.mjs" "$S/checks/source/"
done
mkdir -p "$S/checks/source/fixtures"
cp "$REPO/tests/fixtures/tor_grant_rfc8032.json" "$S/checks/source/fixtures/"
: > "$S/user.npmrc"
: > "$S/global.npmrc"
cd "$S/source/clients/pi"
CACHE=${POSSUMS_PI_NPM_CACHE-"$S/cache"}
set --
if [ "$OFFLINE" = 1 ]; then set -- --offline; fi
env -i HOME="$S/home" TMPDIR="$S/tmp" PATH="$(dirname "$NODE"):$(dirname "$NPM"):/usr/bin:/bin" \
  npm_config_userconfig="$S/user.npmrc" npm_config_globalconfig="$S/global.npmrc" npm_config_logs_max=0 npm_config_update_notifier=false \
  "$NPM" ci --legacy-peer-deps --ignore-scripts --no-audit --no-fund --registry=https://registry.npmjs.org --cache="$CACHE" "$@"
mkdir -p node_modules/@earendil-works
for PACKAGE in pi-ai pi-coding-agent pi-agent-core; do
  ln -s "$PI_ROOT/node_modules/@earendil-works/$PACKAGE" "node_modules/@earendil-works/$PACKAGE"
done
ln -s ../node_modules "$S/source/clients/pi/core/node_modules"
env -i HOME="$S/home" TMPDIR="$S/tmp" PATH="$(dirname "$NODE"):/usr/bin:/bin" PHASE02_OUT="$S/package" POSSUMS_VERIFIER_BUILD="$S/verifier" POSSUMS_TOR_NATIVE_BUILD="$S/tor-native" \
  "$NODE" build.mjs
# build.mjs already links the runtime peers into the scratch package.
printf '\nBuilt package: %s\n' "$S/package"
printf 'Local checks: POSSUMS_PI_ROOT=%s %s %s/source/clients/pi/check.mjs\n' "$PI_ROOT" "$NODE" "$S"
printf 'Load with pinned Pi: pi --no-session --no-extensions --no-tools -e %s/extension.mjs\n' "$S/package"
