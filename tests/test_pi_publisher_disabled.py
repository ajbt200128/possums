"""Reserved disabled boundary only; not artifact/publication admission tests."""
import json
import subprocess
import textwrap
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


class DisabledPiPublisherTests(unittest.TestCase):
    def setUp(self):
        self.workflow = (ROOT / ".github/workflows/publish-pi.yml").read_text()

    def test_only_manual_event_and_hard_false_source_gate(self):
        self.assertIn("on:\n  workflow_dispatch:\npermissions: {}", self.workflow)
        self.assertIn("    if: ${{ false && startsWith(github.ref, 'refs/tags/pi-v') }}", self.workflow)
        self.assertEqual(self.workflow.count("    if:"), 1)
        self.assertEqual(self.workflow.count("  publish:\n"), 1)
        for value in ("push:", "workflow_call:", "workflow_run:", "pull_request:", "inputs:", "vars.", "secrets."):
            self.assertNotIn(value, self.workflow)

    def test_protected_identity_is_dormant_and_has_no_tool_or_upload_step(self):
        self.assertIn("environment: pi-release", self.workflow)
        self.assertIn("concurrency:\n  group: pi-npm-publication\n  cancel-in-progress: false", self.workflow)
        self.assertIn("    permissions:\n      contents: read\n      id-token: write", self.workflow)
        self.assertIn("runs-on: ubuntu-24.04", self.workflow)
        self.assertIn("NOT a functional or enabled publisher", self.workflow)
        self.assertEqual(self.workflow.count("      - name:"), 1)
        for value in ("uses:", "npm publish", "npm stage", "NODE_AUTH_TOKEN", "NPM_TOKEN", "curl ", "gh ", "upload-artifact"):
            self.assertNotIn(value, self.workflow)

    def test_even_accidental_job_activation_only_runs_a_closed_blocker(self):
        before, separator, marker = self.workflow.partition("        run: |\n")
        self.assertTrue(separator)
        self.assertNotIn("run:", before)
        for line in marker.splitlines():
            self.assertTrue(line.startswith("          "))
        result = subprocess.run(["/bin/sh", "-c", textwrap.dedent(marker)],
                                env={"PATH": "/usr/bin:/bin"}, capture_output=True, timeout=3, check=False)
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout, b"")
        self.assertIn(b"possums_pi_publish_blocked: stage=admission", result.stderr)
        self.assertIn(b"constraint=operator_prerequisites", result.stderr)
        self.assertIn(b"No publication was attempted", result.stderr)
        self.assertNotIn(b"Traceback", result.stderr)

    def test_rights_are_not_fabricated_to_prepare_this_boundary(self):
        metadata = json.loads((ROOT / "clients/pi/release.json").read_text())
        self.assertIsNone(metadata["license"])
        self.assertIs(metadata["redistributionApproved"], False)


if __name__ == "__main__":
    unittest.main()
