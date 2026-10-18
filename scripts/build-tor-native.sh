#!/bin/sh
# Build only the maintained SOCKS byte connector; never launch Tor or inference.
set -eu
REPO=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
S=$(mktemp -d /tmp/possums-tor-native-build-XXXXXX)
mkdir -p "$S/source/src" "$S/package"
cp "$REPO/clients/pi/tor-native/Cargo.toml" "$REPO/clients/pi/tor-native/Cargo.lock" "$S/source/"
cp "$REPO/clients/pi/tor-native/src/main.rs" "$S/source/src/"
# The pinned environment supplies Rust 1.92.0; no native SDK lifecycle scripts.
nix develop --offline --no-write-lock-file "$REPO" --command sh -c '
  set -eu
  test "$(rustc --version | cut -d " " -f 2)" = 1.92.0
  unset CARGO_BUILD_TARGET
  cd "$1/source"
  cargo build --offline --locked --release --manifest-path "$1/source/Cargo.toml" --target-dir "$1/target"
' sh "$S"
cp "$S/target/release/possums-tor-native" "$S/package/possums-tor-native"
node --input-type=module - "$REPO" "$S/package" <<'JS'
import { readFile, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { platform, arch } from 'node:process';
import path from 'node:path';
const [repo, output] = process.argv.slice(2);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const sources = {};
for (const name of ['Cargo.toml', 'Cargo.lock', 'src/main.rs']) {
  const key = 'clients/pi/tor-native/' + name;
  sources[key] = hash(await readFile(path.join(repo, key)));
}
const report = { rust: '1.92.0', tor: '0.47.0', platform, arch, sources,
  artifacts: { 'possums-tor-native': { sha256: hash(await readFile(path.join(output, 'possums-tor-native'))) } } };
await writeFile(path.join(output, 'tor-native-build-report.json'), JSON.stringify(report, null, 2) + '\n');
console.log(JSON.stringify({ torNativeBuild: output, platform, arch }));
JS
