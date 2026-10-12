"""Offline integration of the discretionary label with the actual source gate."""
import contextlib
import copy
import io
import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from test_release_automation import ROOT, R, S, r, run


class RolloutTest(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.directory_path = Path(self.directory.name)
        self.origin = run(sha=S)
        self.associated = [{"number": 1}]
        self.pr = {"merged": True, "merge_commit_sha": S,
                   "base": {"ref": "main", "repo": {"full_name": r.REPO}},
                   "labels": [{"name": r.NO_ROLLOUT_LABEL}]}
        self.calls = []

    def api(self, path):
        self.calls.append(path)
        if path == "actions/workflows/check.yml":
            return {"id": 20}
        if path == "actions/runs/10":
            return self.origin
        if path.startswith("actions/workflows/check.yml/runs?"):
            return {"workflow_runs": [self.origin]}
        if path == "git/ref/heads/main":
            return {"object": {"sha": S}}
        if path == f"commits/{S}/pulls?per_page=100":
            return self.associated
        if path == "pulls/1":
            return self.pr
        raise AssertionError("Unexpected synthetic API route")

    def select(self, manual=False, api=None):
        event = {"action": "completed", "repository": {"full_name": r.REPO}, "workflow_run": self.origin}
        event_path, output_path = self.directory_path / "event.json", self.directory_path / "output"
        event_path.write_text(json.dumps(event))
        output_path.write_text("")
        env = {"RELEASE_AUTOMATION_ENABLED": "true", "GITHUB_REPOSITORY": r.REPO,
               "GITHUB_EVENT_PATH": str(event_path), "GITHUB_OUTPUT": str(output_path),
               "GITHUB_EVENT_NAME": "workflow_dispatch" if manual else "workflow_run",
               # Automatic lookup uses the originating source, not the implementation SHA.
               "GITHUB_SHA": S if manual else R, "GITHUB_REF": "refs/heads/main"}
        stdout = io.StringIO()
        with patch.dict(os.environ, env, clear=True), patch.object(r, "api", side_effect=api or self.api), \
                contextlib.redirect_stdout(stdout), patch.object(r, "git") as git:
            r.select_source()
            git.assert_not_called()
        return output_path.read_text(), json.loads(stdout.getvalue())

    def assert_rollout(self, expected, reason):
        output, decision = self.select()
        self.assertEqual(output, f"source={S}\nrollout={'true' if expected else 'false'}\n")
        self.assertEqual(decision["rollout"], expected)
        self.assertEqual(decision["code"], "RELEASE_ROLLOUT_" + reason.upper())

    def test_labeled_origin_skips_after_exact_checks_without_diff_or_baseline(self):
        self.assert_rollout(False, "labeled")
        self.assertLess(self.calls.index("git/ref/heads/main"),
                        self.calls.index(f"commits/{S}/pulls?per_page=100"))
        self.assertEqual(self.calls[-1], "git/ref/heads/main")
        self.assertEqual([call for call in self.calls if call.startswith(("commits/", "pulls/"))],
                         [f"commits/{S}/pulls?per_page=100", "pulls/1"])

    def test_unlabeled_and_manual_publish(self):
        self.pr["labels"] = []
        self.assert_rollout(True, "normal")
        self.pr["labels"] = [{"name": r.NO_ROLLOUT_LABEL}]
        self.calls.clear()
        output, decision = self.select(manual=True)
        self.assertIn("rollout=true\n", output)
        self.assertEqual(decision["code"], "RELEASE_ROLLOUT_MANUAL")
        self.assertFalse(any(call.startswith(("commits/", "pulls/")) for call in self.calls))

    def test_missing_ambiguous_unmerged_wrong_source_and_branch_publish(self):
        for association in ([], [{"number": 1}, {"number": 2}]):
            self.associated = association
            self.assert_rollout(True, "normal")
        self.associated = [{"number": 1}]
        for field, value in (("merged", False), ("merge_commit_sha", R),
                             ("base", {"ref": "feature", "repo": {"full_name": r.REPO}}),
                             ("base", {"ref": "main", "repo": {"full_name": "other/repository"}})):
            original = copy.deepcopy(self.pr)
            self.pr[field] = value
            self.assert_rollout(True, "normal")
            self.pr = original

    def test_lookup_and_malformed_evidence_stop_without_outputs(self):
        for bad in (r.Stop("command"), {"hostile": "SYNTHETIC-SECRET"}, [{"number": "hostile"}]):
            def api(path, bad=bad):
                if path.startswith("commits/"):
                    if isinstance(bad, Exception):
                        raise bad
                    return bad
                return self.api(path)
            with self.assertRaises(r.Stop) as caught:
                self.select(api=api)
            self.assertEqual(caught.exception.code, "rollout_lookup")
            self.assertEqual((self.directory_path / "output").read_text(), "")
            self.assertNotIn("SYNTHETIC-SECRET", json.dumps(r.diagnostic(caught.exception.code)))
        self.pr["labels"] = [{"hostile": "SYNTHETIC-SECRET"}]
        with self.assertRaises(r.Stop) as caught:
            self.select()
        self.assertEqual(caught.exception.code, "rollout_lookup")

    def test_pr_lookup_failure_is_operational(self):
        def api(path):
            if path == "pulls/1":
                raise r.Stop("command")
            return self.api(path)
        with self.assertRaises(r.Stop) as caught:
            self.select(api=api)
        self.assertEqual(caught.exception.code, "rollout_lookup")
        self.assertEqual((self.directory_path / "output").read_text(), "")

    def test_existing_event_and_checks_gates_precede_label_lookup(self):
        for path in ("actions/runs/10", f"actions/workflows/check.yml/runs?head_sha={S}&per_page=100"):
            def api(request, path=path):
                response = self.api(request)
                if request == path:
                    assert isinstance(response, dict)
                    if "workflow_runs" in response:
                        return {"workflow_runs": []}
                    return {**response, "conclusion": "failure"}
                return response
            self.calls.clear()
            with self.subTest(path=path), self.assertRaises(r.Stop) as caught:
                self.select(api=api)
            self.assertEqual(caught.exception.code, "checks")
            self.assertFalse(any(call.startswith(("commits/", "pulls/")) for call in self.calls))
            self.assertEqual((self.directory_path / "output").read_text(), "")

    def test_stale_after_lookup_cannot_skip_or_publish(self):
        reads = 0
        def api(path):
            nonlocal reads
            if path == "git/ref/heads/main":
                reads += 1
                if reads == 2:
                    return {"object": {"sha": R}}
            return self.api(path)
        with self.assertRaises(r.Stop) as caught:
            self.select(api=api)
        self.assertEqual(caught.exception.code, "stale")
        self.assertEqual((self.directory_path / "output").read_text(), "")

    def test_workflow_integration_trust_and_explicit_build_gate(self):
        workflow = (ROOT / ".github/workflows/publish-image.yml").read_text()
        self.assertIn("ref: ${{ github.sha }}\n          persist-credentials: false", workflow)
        self.assertIn("pull-requests: read", workflow)
        self.assertIn("rollout: ${{ steps.source.outputs.rollout }}", workflow)
        self.assertEqual(workflow.count("if: needs.source-checks.outputs.rollout == 'true'"), 2)
        self.assertIn("run: python3 scripts/release.py source", workflow)
        self.assertNotIn("pull_request_target", workflow)


if __name__ == "__main__":
    unittest.main()
