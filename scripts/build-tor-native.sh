#!/bin/sh
# Run in the pinned devenv shell. No toolchain installation or Tor launch.
# No arguments builds for the local host; --target accepts four release triples.
set -eu
REPO=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
exec python3 "$REPO/scripts/tor_native_build.py" "$@"
