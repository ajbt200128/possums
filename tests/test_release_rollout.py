"""Offline source-gate integration with synthetic Git history and GitHub evidence."""
import contextlib
import copy
import io
import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from test_release_automation import ROOT, r, run


class RolloutTest(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.old_cwd = Path.cwd()
        os.chdir(self.directory.name)
        self.addCleanup(os.chdir, self.old_cwd)
        self.git("init", "-q", "-b", "master")
        self.git("config", "user.name", "Synthetic")
        self.git("config", "user.email", "synthetic@example.invalid")
        self.base = self.commit({"src/main.rs": "baseline", "docs/old.md": "old"})
        self.source = self.commit({"clients/pi/provider.ts": "synthetic"})
        self.prs = {self.source: self.pr(self.source)}
        self.files = {1: [{"filename": "clients/pi/provider.ts", "status": "added"}]}
        self.associations = {}
        self.calls = []
        self.origin = run(sha=self.source)
        self.publication = run("publish-image.yml", sha=self.base, event="workflow_dispatch")
        self.publication.update(id=40, run_attempt=1, display_title=f"Image source {self.base}")
        self.runs = [self.publication]
        self.jobs = {"total_count": 4, "jobs": [{"name": name, "conclusion": "success"}
                     for name in ("source-checks", "build (a)", "build (b)", "publish")]}

    def git(self, *args):
        return subprocess.check_output(["git", *args], stderr=subprocess.DEVNULL).decode().strip()

    def commit(self, changes):
        for name, content in changes.items():
            path = Path(name)
            if content is None:
                path.unlink()
            else:
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(content)
        self.git("add", "-A")
        self.git("commit", "-qm", "Synthetic change")
        return self.git("rev-parse", "HEAD")

    def pr(self, sha, number=1):
        return {"number": number, "merged": True, "merge_commit_sha": sha,
                "base": {"ref": "main", "repo": {"full_name": r.REPO}},
                "labels": [{"name": r.NO_ROLLOUT_LABEL}], "changed_files": 1}

    def api(self, path):
        self.calls.append(path)
        if path == "actions/workflows/check.yml":
            return {"id": 20}
        if path == "actions/runs/10":
            return self.origin
        if path.startswith("actions/workflows/check.yml/runs?"):
            return {"workflow_runs": [self.origin]}
        if path == "git/ref/heads/main":
            return {"object": {"sha": self.source}}
        if path.startswith("commits/"):
            sha = path.split("/")[1]
            return self.associations.get(sha, [{"number": self.prs[sha]["number"]}] if sha in self.prs else [])
        if path.startswith("pulls/"):
            number = int(path.split("/")[1])
            if "/files?" in path:
                page = int(path.split("page=")[-1])
                return self.files[number][(page - 1) * 100:page * 100]
            return next(pr for pr in self.prs.values() if pr["number"] == number)
        if path.startswith("actions/workflows/publish-image.yml/runs?"):
            return {"workflow_runs": self.runs}
        if path.startswith("actions/runs/") and path.endswith("jobs?per_page=100"):
            return self.jobs
        raise AssertionError("Unexpected synthetic API route")

    def select(self, manual=False, api=None):
        self.origin = run(sha=self.source)
        event = {"action": "completed", "repository": {"full_name": r.REPO}, "workflow_run": self.origin}
        Path("event.json").write_text(json.dumps(event))
        Path("output").write_text("")
        env = {"RELEASE_AUTOMATION_ENABLED": "true", "GITHUB_REPOSITORY": r.REPO,
               "GITHUB_EVENT_PATH": str(Path("event.json").resolve()),
               "GITHUB_OUTPUT": str(Path("output").resolve()),
               "GITHUB_EVENT_NAME": "workflow_dispatch" if manual else "workflow_run",
               # Automatic admission must use the originating source, not this implementation SHA.
               "GITHUB_SHA": self.source if manual else self.base, "GITHUB_REF": "refs/heads/main"}
        stdout = io.StringIO()
        with patch.dict(os.environ, env, clear=True), patch.object(r, "api", side_effect=api or self.api), \
                contextlib.redirect_stdout(stdout):
            r.select_source()
        return Path("output").read_text(), json.loads(stdout.getvalue())

    def assert_rollout(self, expected, reason=None):
        output, decision = self.select()
        self.assertIn(f"source={self.source}\n", output)
        self.assertIn(f"rollout={'true' if expected else 'false'}\n", output)
        self.assertEqual(decision["rollout"], expected)
        if reason:
            self.assertEqual(decision["code"], "RELEASE_ROLLOUT_" + reason.upper())

    def test_labeled_squash_skips_after_exact_checks(self):
        self.assert_rollout(False, "eligible")
        self.assertLess(self.calls.index("git/ref/heads/main"),
                        self.calls.index(f"commits/{self.source}/pulls?per_page=100"))
        self.assertEqual(self.calls[-1], "git/ref/heads/main")

    def test_unlabeled_and_manual_publish(self):
        self.prs[self.source]["labels"] = []
        self.assert_rollout(True, "ineligible")
        self.assertFalse(any("publish-image.yml/runs" in call for call in self.calls))
        self.prs[self.source]["labels"] = [{"name": r.NO_ROLLOUT_LABEL}]
        self.calls.clear()
        output, decision = self.select(manual=True)
        self.assertIn("rollout=true\n", output)
        self.assertEqual(decision["code"], "RELEASE_ROLLOUT_MANUAL")
        self.assertFalse(any(call.startswith(("commits/", "pulls/")) for call in self.calls))

    def test_mixed_gateway_and_all_protected_inputs(self):
        for name in ("src/main.rs", "flake.nix", "Cargo.lock", "attestation-helper/main.go",
                     ".github/workflows/check.yml", "scripts/release.py", "deploy/policy.json",
                     "tinfoil-config.yml", "tests/image_health.py", "tests/support/stream.rs",
                     "clients/pi/injected.rs", "docs/injected.rs", "clients/pi/Cargo.toml"):
            self.assertFalse(r.rollout_path(name), name)
        self.source = self.commit({"src/main.rs": "gateway", "docs/safe.md": "docs"})
        self.prs = {self.source: self.pr(self.source)}
        self.prs[self.source]["changed_files"] = 2
        self.files[1] = [{"filename": "src/main.rs", "status": "modified"},
                         {"filename": "docs/safe.md", "status": "added"}]
        self.assert_rollout(True, "ineligible")

    def test_rename_and_deletion_check_both_paths(self):
        self.source = self.commit({"src/main.rs": None, "docs/renamed.md": "baseline"})
        self.prs = {self.source: self.pr(self.source)}
        self.files[1] = [{"filename": "docs/renamed.md", "previous_filename": "src/main.rs", "status": "renamed"}]
        self.assert_rollout(True, "ineligible")
        # Even misleading PR file evidence cannot hide the mainline deletion.
        self.files[1] = [{"filename": "docs/renamed.md", "status": "added"}]
        self.assert_rollout(True, "ineligible")

    def test_eligible_deleted_document(self):
        self.source = self.commit({"docs/old.md": None})
        self.prs = {self.source: self.pr(self.source)}
        self.files[1] = [{"filename": "docs/old.md", "status": "removed"}]
        self.assert_rollout(True, "ineligible")  # earlier unlabeled/unknown PR must not disappear
        self.prs[self.git("rev-parse", "HEAD^")] = self.pr(self.git("rev-parse", "HEAD^"), 2)
        self.files[2] = [{"filename": "clients/pi/provider.ts", "status": "added"}]
        self.assert_rollout(False, "eligible")

    def test_missing_ambiguous_unmerged_wrong_source_and_branch(self):
        for association in ([], [{"number": 1}, {"number": 2}]):
            self.associations[self.source] = association
            self.assert_rollout(True, "ineligible")
        self.associations.clear()
        for field, value in (("merged", False), ("merge_commit_sha", self.base),
                             ("base", {"ref": "feature", "repo": {"full_name": r.REPO}})):
            original = copy.deepcopy(self.prs[self.source])
            self.prs[self.source][field] = value
            self.assert_rollout(True, "ineligible")
            self.prs[self.source] = original

    def test_multi_commit_gateway_revert_still_publishes(self):
        self.commit({"src/main.rs": "gateway changed"})
        self.source = self.commit({"src/main.rs": "baseline", "docs/safe.md": "docs"})
        self.prs = {self.source: self.pr(self.source)}
        self.files[1] = [{"filename": "docs/safe.md", "status": "added"}]
        self.assert_rollout(True, "ineligible")

    def test_two_parent_merge_with_multiple_pr_commits(self):
        self.git("checkout", "-qb", "feature", self.base)
        self.commit({"clients/pi/a.ts": "a"})
        self.commit({"docs/a.md": "a"})
        self.git("checkout", "-q", "master")
        self.git("merge", "--no-ff", "-qm", "Synthetic merge", "feature")
        self.source = self.git("rev-parse", "HEAD")
        self.prs[self.source] = self.pr(self.source, 2)
        self.prs[self.source]["changed_files"] = 2
        self.files[2] = [{"filename": "clients/pi/a.ts", "status": "added"},
                         {"filename": "docs/a.md", "status": "added"}]
        self.assert_rollout(False, "eligible")

    def test_complete_paginated_pr_not_just_mainline_delta(self):
        # Rebase linkage proves only the final commit: inspect the entire PR anyway.
        self.files[1] = [{"filename": f"docs/{i}.md", "status": "added"} for i in range(100)]
        self.files[1].append({"filename": "scripts/deploy.py", "status": "modified"})
        self.prs[self.source]["changed_files"] = 101
        self.assert_rollout(True, "ineligible")
        self.assertIn("pulls/1/files?per_page=100&page=2", self.calls)

    def test_missing_baseline_and_skipped_success_are_not_publication(self):
        self.runs = []
        self.assert_rollout(True, "baseline")
        self.runs = [self.publication]
        self.jobs["jobs"][-1]["conclusion"] = "skipped"
        self.assert_rollout(True, "baseline")

    def test_duplicate_baseline_jobs_do_not_prove_publication(self):
        self.jobs["jobs"].append({"name": "publish", "conclusion": "skipped"})
        self.jobs["total_count"] = 5
        self.assert_rollout(True, "baseline")

    def test_existing_event_and_checks_gates_precede_label_lookup(self):
        for path, code in (("actions/runs/10", "checks"),
                           (f"actions/workflows/check.yml/runs?head_sha={self.source}&per_page=100", "checks")):
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
            self.assertEqual(caught.exception.code, code)
            self.assertFalse(any(call.startswith(("commits/", "pulls/")) for call in self.calls))
            self.assertEqual(Path("output").read_text(), "")

    def test_lookup_and_malformed_evidence_stop_without_outputs(self):
        for bad in (r.Stop("command"), {"hostile": "SYNTHETIC-SECRET"}):
            def api(path, bad=bad):
                if path.startswith("commits/"):
                    if isinstance(bad, Exception):
                        raise bad
                    return bad
                return self.api(path)
            with self.assertRaises(r.Stop) as caught:
                self.select(api=api)
            self.assertEqual(caught.exception.code, "rollout_lookup")
            self.assertEqual(Path("output").read_text(), "")
            self.assertNotIn("SYNTHETIC-SECRET", json.dumps(r.diagnostic(caught.exception.code)))
        self.prs[self.source]["changed_files"] = 2
        with self.assertRaises(r.Stop) as caught:
            self.select()
        self.assertEqual(caught.exception.code, "rollout_lookup")

    def test_failure_at_each_rollout_api_boundary_is_operational(self):
        for prefix in ("pulls/1/files?", "pulls/1", "actions/workflows/publish-image.yml/runs?",
                       "actions/runs/40/attempts/"):
            def api(path, prefix=prefix):
                if path.startswith(prefix):
                    raise r.Stop("command")
                return self.api(path)
            with self.subTest(prefix=prefix), self.assertRaises(r.Stop) as caught:
                self.select(api=api)
            self.assertEqual(caught.exception.code, "rollout_lookup")
            self.assertEqual(Path("output").read_text(), "")

    def test_eligible_rename_checks_old_and_new_documents(self):
        self.git("checkout", "-q", self.base)
        self.source = self.commit({"docs/old.md": None, "docs/new.md": "old"})
        self.prs = {self.source: self.pr(self.source)}
        self.files[1] = [{"filename": "docs/new.md", "previous_filename": "docs/old.md", "status": "renamed"}]
        self.assert_rollout(False, "eligible")

    def test_large_pr_and_unknown_baseline_keep_publication(self):
        self.prs[self.source]["changed_files"] = 3001
        self.assert_rollout(True, "ineligible")
        self.prs[self.source]["changed_files"] = 1
        self.publication.update(head_sha=self.source, display_title=f"Image source {self.source}")
        self.assert_rollout(True, "baseline")
        self.publication.update(head_sha="f" * 40, display_title="Image source " + "f" * 40)
        self.assert_rollout(True, "baseline")

    def test_stale_after_lookup_cannot_skip_or_publish(self):
        reads = 0
        def api(path):
            nonlocal reads
            if path == "git/ref/heads/main":
                reads += 1
                if reads == 2:
                    return {"object": {"sha": self.base}}
            return self.api(path)
        with self.assertRaises(r.Stop) as caught:
            self.select(api=api)
        self.assertEqual(caught.exception.code, "stale")
        self.assertEqual(Path("output").read_text(), "")

    def test_workflow_integration_trust_and_explicit_build_gate(self):
        workflow = (ROOT / ".github/workflows/publish-image.yml").read_text()
        self.assertIn("ref: ${{ github.sha }}\n          fetch-depth: 0", workflow)
        self.assertIn("pull-requests: read", workflow)
        self.assertIn("rollout: ${{ steps.source.outputs.rollout }}", workflow)
        self.assertEqual(workflow.count("if: needs.source-checks.outputs.rollout == 'true'"), 2)
        self.assertIn("run: python3 scripts/release.py source", workflow)
        self.assertNotIn("pull_request_target", workflow)


if __name__ == "__main__":
    unittest.main()
