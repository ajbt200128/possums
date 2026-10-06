#!/bin/sh
set -eu
SOURCE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO=$(CDPATH= cd -- "$SOURCE/../.." && pwd)
PI_ROOT=${POSSUMS_PI_ROOT:?Set POSSUMS_PI_ROOT to the pinned Pi 0.99.2 release directory}
NODE=${POSSUMS_NODE:-$(command -v node)}
NPM=$(command -v npm)
[ "$("$NODE" --version)" = v24.13.0 ] || { printf '%s\n' 'Node 24.13.0 required' >&2; exit 1; }
for PACKAGE in pi-ai pi-coding-agent pi-agent-core; do
  [ -f "$PI_ROOT/node_modules/@earendil-works/$PACKAGE/package.json" ] || { printf '%s\n' 'Pinned Pi installation missing' >&2; exit 1; }
done
S=$(mktemp -d "${TMPDIR:-/tmp}/possums-pi-build-XXXXXX")
mkdir -p "$S/source/clients/pi" "$S/source/examples/phase01" "$S/home" "$S/tmp" "$S/cache" "$S/checks/source"
cp "$SOURCE"/*.ts "$SOURCE"/*.mjs "$SOURCE"/*.json "$S/source/clients/pi/"
cp "$REPO/examples/phase01"/*.ts "$S/source/examples/phase01/"
for TEST in phase01_client phase01_transport phase02_client phase02_pi; do
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
env -i HOME="$S/home" TMPDIR="$S/tmp" PATH="$(dirname "$NODE"):/usr/bin:/bin" PHASE02_OUT="$S/package" \
  "$NODE" build.mjs
mkdir -p "$S/package/node_modules/@earendil-works"
for PACKAGE in pi-ai pi-coding-agent pi-agent-core; do
  ln -s "$PI_ROOT/node_modules/@earendil-works/$PACKAGE" "$S/package/node_modules/@earendil-works/$PACKAGE"
done
printf '\nBuilt package: %s\n' "$S/package"
printf 'Local checks: POSSUMS_PI_ROOT=%s %s %s/source/clients/pi/check.mjs\n' "$PI_ROOT" "$NODE" "$S"
printf 'Load with pinned Pi: pi --no-session --no-extensions --no-tools -e %s/extension.mjs --possums-manifest /absolute/path/to/approved/tinfoil-deployment.json\n' "$S/package"
