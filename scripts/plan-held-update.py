#!/usr/bin/env python3
"""Offline paid-only checklist lint. Never contacts a service or authorizes promotion.

Input is the locally authored schema documented in docs/blue-green-deployments.md,
NOT a Tinfoil API response or cryptographic verification result.
"""
import json
import re
import sys
from typing import Any

LIMIT = 16384
REFERENCES = {"POSSUMS_ACCOUNTS_JSON", "TINFOIL_API_KEY", "OTEL_EXPORTER_OTLP_HEADERS"}
IDENTITY_FIELDS = {"tag", "manifest_sha256", "measurement_sha256", "hpke_key_sha256"}


class Rejected(Exception):
    pass


def require(condition):
    if not condition:
        raise Rejected()


def object_without_duplicates(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result)
        result[key] = value
    return result


def identity(value: Any):
    require(isinstance(value, dict) and set(value) == IDENTITY_FIELDS)
    require(type(value["tag"]) is str and len(value["tag"]) <= 64)
    require(re.fullmatch(r"v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", value["tag"]))
    for field in IDENTITY_FIELDS - {"tag"}:
        require(type(value[field]) is str and re.fullmatch(r"[0-9a-f]{64}", value[field]))


def plan(raw: bytes):
    require(len(raw) <= LIMIT)
    try:
        doc: Any = json.loads(raw, object_pairs_hook=object_without_duplicates)
    except ValueError:
        raise Rejected() from None
    require(isinstance(doc, dict) and set(doc) == {
        "expected", "observed", "production_tag", "latest_tag", "strategy",
        "review_state", "secret_references",
    })
    identity(doc["expected"])
    identity(doc["observed"])
    require(doc["expected"] == doc["observed"])
    # Latest remains on the serving paid release throughout candidate review.
    require(type(doc["production_tag"]) is str and doc["production_tag"] == doc["latest_tag"])
    require(re.fullmatch(r"v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", doc["production_tag"]))
    require(doc["production_tag"] != doc["expected"]["tag"])
    require(doc["strategy"] == "blue_green" and doc["review_state"] == "held_ready")
    refs: Any = doc["secret_references"]
    require(isinstance(refs, list) and all(type(ref) is str for ref in refs))
    require(len(refs) == len(REFERENCES) and set(refs) == REFERENCES)
    # Neither matching hashes nor asserted readiness proves authenticity or safety.
    return {
        "result": "checklist_consistent_not_verified",
        "candidate_tag": doc["expected"]["tag"],
        "secret_references": sorted(refs),
        "promote_allowed": False,
        "blockers": ["review_verifier", "owner_drain", "session_affinity", "overlap_accounting", "operator_approval"],
    }


def main():
    try:
        require(len(sys.argv) == 1)
        result = plan(sys.stdin.buffer.read(LIMIT + 1))
    except (Rejected, ValueError, TypeError, RecursionError):
        # Never echo input, paths, URLs, platform errors or secret values.
        print('{"result":"plan_rejected","promote_allowed":false}')
        return 1
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
