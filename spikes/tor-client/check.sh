#!/bin/sh
# Offline checks only. Dependencies must already be installed from this lockfile.
set -eu
cd "$(dirname "$0")"
node -e 'if (process.version !== "v24.13.0") { process.stderr.write("tor-spike-check: use the pinned Node 24.13.0 environment\n"); process.exit(1); }'
# Apply only the reviewed exact-source patch; no install/download or scripts.
node qualify-undici.mjs
for file in *.mjs test/*.mjs; do
  node --check "$file"
done
node --test test/*.test.mjs
