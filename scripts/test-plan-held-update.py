import copy
import importlib.util
import json
import subprocess
import sys
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("plan-held-update.py")
spec = importlib.util.spec_from_file_location("planner", SCRIPT)
assert spec and spec.loader
planner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(planner)


def fixture():
    identity = {
        "tag": "v0.0.99", "manifest_sha256": "a" * 64,
        "measurement_sha256": "b" * 64, "hpke_key_sha256": "c" * 64,
    }
    return {
        "expected": identity, "observed": copy.deepcopy(identity),
        "production_tag": "v0.0.17", "latest_tag": "v0.0.17",
        "strategy": "blue_green", "review_state": "held_ready",
        "secret_references": sorted(planner.REFERENCES),
    }


class PlannerTests(unittest.TestCase):
    def rejects(self, doc):
        with self.assertRaises((planner.Rejected, ValueError)):
            planner.plan(json.dumps(doc).encode())

    def test_consistent_checklist_never_authorizes_promotion(self):
        result = planner.plan(json.dumps(fixture()).encode())
        self.assertFalse(result["promote_allowed"])
        self.assertEqual(result["secret_references"], sorted(planner.REFERENCES))
        self.assertIn("review_verifier", result["blockers"])

    def test_wrong_tag_measurement_manifest_or_key(self):
        for field in planner.IDENTITY_FIELDS:
            with self.subTest(field=field):
                doc = fixture()
                doc["observed"][field] = "v0.0.98" if field == "tag" else "d" * 64
                self.rejects(doc)

    def test_replacement_unknown_strategy_and_missing_readiness(self):
        for field, values in {
            "strategy": ["replace", "recreate", "unknown", None, {}],
            "review_state": ["starting", "failed", None, True, {}],
        }.items():
            for value in values:
                doc = fixture()
                doc[field] = value
                self.rejects(doc)
        doc = fixture()
        del doc["review_state"]
        self.rejects(doc)

    def test_latest_cannot_move_during_review(self):
        doc = fixture()
        doc["latest_tag"] = doc["expected"]["tag"]
        self.rejects(doc)
        doc = fixture()
        doc["production_tag"] = doc["latest_tag"] = doc["expected"]["tag"]
        self.rejects(doc)

    def test_reference_values_and_unknown_fields_rejected(self):
        for value in [[], ["TINFOIL_API_KEY"] * 3, {"TINFOIL_API_KEY": "SYNTHETIC_SECRET"}]:
            doc = fixture()
            doc["secret_references"] = value
            self.rejects(doc)
        doc = fixture()
        doc["raw_status"] = "SYNTHETIC_SECRET"
        self.rejects(doc)

    def test_hostile_input_never_echoed(self):
        for raw in [b'{"raw_error":"SYNTHETIC_SECRET"}', b'[]', b'null', b'"SYNTHETIC_SECRET"',
                    b'{"strategy":1,"strategy":2}', b'{' * 2000, b'x' * (planner.LIMIT + 1)]:
            result = subprocess.run([sys.executable, str(SCRIPT)], input=raw, capture_output=True)
            self.assertEqual(result.returncode, 1)
            self.assertEqual(result.stdout, b'{"result":"plan_rejected","promote_allowed":false}\n')
            self.assertEqual(result.stderr, b'')

    def test_no_execution_imports(self):
        # Planner has no network, subprocess or platform client dependencies.
        self.assertNotIn("subprocess", SCRIPT.read_text())
        self.assertNotIn("urllib", SCRIPT.read_text())


if __name__ == "__main__":
    unittest.main()
