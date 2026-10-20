#!/usr/bin/env python3
"""Prefer the public upstream before the existing pinned loopback cache proxy."""
import json
import os
from pathlib import Path

PROXY = ("extra-substituters = http://127.0.0.1:37515?trusted=1&compression=zstd"
         "&parallel-compression=true&priority=1\n")


class ProxyEntryMismatch(Exception):
    pass


def upstream_first(config):
    # Match the pinned daemon's exact entry; preserve every other setting and
    # proxy parameter rather than reconstructing potentially private config.
    lines = config.splitlines(keepends=True)
    if lines.count(PROXY) != 1:
        raise ProxyEntryMismatch
    replacement = PROXY.replace("priority=1\n", "priority=50\n")
    return "".join(replacement if line == PROXY else line for line in lines)


def main():
    try:
        directory = Path(os.environ.get("XDG_CONFIG_HOME", Path.home() / ".config"))
        path = directory / "nix/nix.conf"
        updated = upstream_first(path.read_text())
        path.write_text(updated)
    except ProxyEntryMismatch:
        print(json.dumps({"code": "CI_CACHE_CONFIG_MISMATCH", "stage": "cache-priority",
                          "constraint": "pinned_proxy_entry",
                          "action": "Check the pinned cache setup before building; proxy priority was not changed."}))
        return 1
    except Exception:
        print(json.dumps({"code": "CI_CACHE_CONFIG_UNKNOWN", "stage": "cache-priority",
                          "constraint": "configuration_io_cause_unknown",
                          "action": "Check runner configuration privately before building; configuration outcome is unknown."}))
        return 1
    print(json.dumps({"code": "CI_CACHE_UPSTREAM_FIRST", "stage": "cache-priority",
                      "proxy_priority": 50}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
