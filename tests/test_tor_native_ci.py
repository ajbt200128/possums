"""Native CI control-flow tests; every acquisition/build/runtime call is mocked."""
import importlib.util
import json
import os
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("ci_pi_native", ROOT / "scripts/ci_pi_native.py")
assert spec is not None and spec.loader is not None
ci = importlib.util.module_from_spec(spec)
with patch.object(sys, "path", [str(ROOT / "scripts"), *sys.path]):
    spec.loader.exec_module(ci)
COMMIT = "a" * 40


class NativeCITests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.output = self.root / "output"
        self.packages = [self.root / "first", self.root / "second"]
        for package in self.packages:
            package.mkdir()
            (package / "possums-tor-native").write_bytes(b"synthetic-native")
            (package / "possums-tor-native").chmod(0o755)
            (package / "tor-native-build-report.json").write_text('{}')
        self.calls = []
        self.built = 0
        self.dirty = False
        self.dirty_after_tests = False
        for context in [patch.dict(os.environ, {"GITHUB_SHA": COMMIT, "SSL_CERT_FILE": "/public-certificate-fixture",
                                                "NPM_TOKEN": "hostile-canary", "CARGO_REGISTRIES_CRATES_IO_TOKEN": "hostile-canary"}),
                        patch.object(ci.native, "toolchain", return_value=({"host": "aarch64-apple-darwin"}, self.root,
                                                                         self.root / "rustc", self.root / "cargo")),
                        patch.object(ci.platform, "mac_ver", return_value=("14.8.1", (), ""))]:
            context.start()
            self.addCleanup(context.stop)

    def mock_stage(self, stage, command, *, env=None, cwd=ci.REPO):
        self.calls.append((stage, command, dict(env) if env else None, cwd))
        if stage == "source":
            return COMMIT if "rev-parse" in command else "changed" if self.dirty else ""
        if stage == "node":
            return "v24.13.0"
        if stage == "build":
            package = self.packages[self.built]
            self.built += 1
            return json.dumps({"torNativeBuild": str(package)})
        if stage == "host_tests" and self.dirty_after_tests:
            self.dirty = True
        return ""

    def test_apple_candidate_is_repeated_source_bound_and_not_os_floor_authorization(self):
        with patch.object(ci, "stage_run", side_effect=self.mock_stage):
            report = ci.prepare("aarch64-apple-darwin", self.output)
        self.assertEqual(report["repeatBuild"], "identical")
        self.assertEqual(report["macOS"], "14.8.1")
        self.assertNotIn("osFloorQualified", report)
        self.assertEqual(report["sourceCommit"], COMMIT)
        self.assertEqual(self.built, 2)
        self.assertEqual({p.name for p in self.output.iterdir()},
                         {"possums-tor-native", "tor-native-build-report.json", "tor-native-qualification.json"})
        self.assertTrue((self.output / "possums-tor-native").stat().st_mode & 0o111)
        for stage, command, env, cwd in self.calls:
            if stage in {"prefetch", "build", "host_tests"}:
                self.assertNotIn("hostile-canary", json.dumps(env))
                self.assertNotIn("NPM_TOKEN", env)
                self.assertNotIn("CARGO_REGISTRIES_CRATES_IO_TOKEN", env)
                self.assertEqual(env["RUSTUP_AUTO_INSTALL"], "0")
                if stage == "prefetch":
                    self.assertEqual(cwd.name, "source")
                    self.assertIn("--locked", command)
                else:
                    self.assertEqual(env["CARGO_NET_OFFLINE"], "true")
        self.assertFalse(any(stage == "image_preparation" for stage, *_ in self.calls))

    def test_linux_requires_explicit_image_preparation_and_clean_userspace_success(self):
        result = {"execution": "synthetic_loopback_passed"}
        with (patch.object(ci, "stage_run", side_effect=self.mock_stage),
              patch.object(ci.native, "toolchain", return_value=({"host": "aarch64-unknown-linux-gnu"}, self.root,
                                                               self.root / "rustc", self.root / "cargo")),
              patch.object(ci.qualification, "qualify", return_value=result) as qualified):
            report = ci.prepare("aarch64-unknown-linux-musl", self.output)
        qualified.assert_called_once_with("aarch64-unknown-linux-musl", self.packages[0])
        self.assertEqual(report["cleanUserspace"], result)
        pulls = [command for stage, command, *_ in self.calls if stage == "image_preparation"]
        self.assertEqual({command[-1] for command in pulls}, set(ci.qualification.IMAGES.values()))
        self.assertTrue(all("--config" in command and "unix:///var/run/docker.sock" in command for command in pulls))

    def test_bad_target_existing_output_wrong_commit_and_dirty_source_do_not_build(self):
        with patch.object(ci, "stage_run") as run:
            for target in ["hostile-canary", "aarch64-unknown-linux-gnu"]:
                with self.assertRaisesRegex(ci.native.BuildFailure, "ci_input"):
                    ci.prepare(target, self.output)
            self.output.mkdir()
            with self.assertRaisesRegex(ci.native.BuildFailure, "ci_input"):
                ci.prepare("aarch64-apple-darwin", self.output)
            run.assert_not_called()
        self.output.rmdir()
        self.dirty = True
        with patch.object(ci, "stage_run", side_effect=self.mock_stage), self.assertRaisesRegex(ci.native.BuildFailure, "ci_source"):
            ci.prepare("aarch64-apple-darwin", self.output)
        self.dirty = False
        with (patch.object(ci, "stage_run", side_effect=self.mock_stage), patch.dict(os.environ, {"GITHUB_SHA": "b" * 40}),
              self.assertRaisesRegex(ci.native.BuildFailure, "ci_source")):
            ci.prepare("aarch64-apple-darwin", self.output)
        self.assertEqual(self.built, 0)

    def test_wrong_node_or_runner_never_builds(self):
        def wrong_node(stage, command, **kwargs):
            return "v24.14.0" if stage == "node" else self.mock_stage(stage, command, **kwargs)

        with patch.object(ci, "stage_run", side_effect=wrong_node), self.assertRaisesRegex(ci.native.BuildFailure, "ci_node"):
            ci.prepare("aarch64-apple-darwin", self.output)
        with patch.object(ci, "stage_run", side_effect=self.mock_stage), self.assertRaisesRegex(ci.native.BuildFailure, "ci_runner"):
            ci.prepare("x86_64-apple-darwin", self.output)
        self.assertEqual(self.built, 0)

    def test_different_clean_builds_and_midrun_source_changes_cannot_emit_evidence(self):
        (self.packages[1] / "possums-tor-native").write_bytes(b"different")
        with patch.object(ci, "stage_run", side_effect=self.mock_stage), self.assertRaisesRegex(ci.native.BuildFailure, "ci_reproducibility"):
            ci.prepare("aarch64-apple-darwin", self.output)
        self.assertFalse(self.output.exists())
        (self.packages[1] / "possums-tor-native").write_bytes(b"synthetic-native")
        self.built = 0
        self.dirty_after_tests = True
        with patch.object(ci, "stage_run", side_effect=self.mock_stage), self.assertRaisesRegex(ci.native.BuildFailure, "ci_source_changed"):
            ci.prepare("aarch64-apple-darwin", self.output)
        self.assertFalse(self.output.exists())

    def test_build_json_failure_is_closed_and_stops(self):
        def malformed(stage, command, **kwargs):
            return "hostile-canary" if stage == "build" else self.mock_stage(stage, command, **kwargs)

        with patch.object(ci, "stage_run", side_effect=malformed), self.assertRaisesRegex(ci.native.BuildFailure, "ci_build_output") as failure:
            ci.prepare("aarch64-apple-darwin", self.output)
        self.assertNotIn("hostile-canary", str(failure.exception))
        self.assertFalse(self.output.exists())

    def test_known_tool_category_keeps_ci_stage(self):
        with (patch.object(ci.native, "run", side_effect=ci.native.BuildFailure("possums_build_tor_tool_exit: closed")),
              self.assertRaisesRegex(ci.native.BuildFailure, "ci_prefetch.*possums_build_tor_tool_exit")):
            ci.stage_run("prefetch", ["synthetic"])

    def test_workflow_is_manual_read_only_closed_four_runner_matrix_without_distribution(self):
        text = (ROOT / ".github/workflows/check-pi-native.yml").read_text()
        self.assertEqual(set(re.findall(r"^\s+- target: (\S+)$", text, re.M)), set(ci.native.TARGETS))
        self.assertEqual(set(re.findall(r"^\s+runner: (\S+)$", text, re.M)),
                         {"macos-14", "macos-15-intel", "ubuntu-24.04-arm", "ubuntu-24.04"})
        self.assertIn("on:\n  workflow_dispatch:", text)
        self.assertIn("permissions:\n  contents: read", text)
        self.assertIn("persist-credentials: false", text)
        self.assertIn("package-manager-cache: false", text)
        self.assertIn('diagnostic-endpoint: ""', text)
        for forbidden in ["pull_request", "push:", "secrets.", "id-token:", "npm publish", "upload-artifact", "continue-on-error"]:
            self.assertNotIn(forbidden, text)
        for action in re.findall(r"uses: (\S+)", text):
            self.assertRegex(action, r"^[\w/-]+@[0-9a-f]{40}$")

    def test_cli_rejects_missing_arguments_without_tools_or_raw_trace(self):
        result = subprocess.run([sys.executable, str(ROOT / "scripts/ci_pi_native.py")], capture_output=True, check=False)
        self.assertEqual(result.returncode, 1)
        self.assertIn(b"possums_pi_native_ci_input", result.stderr)
        self.assertEqual(result.stdout, b"")
        self.assertNotIn(b"Traceback", result.stderr)


if __name__ == "__main__":
    unittest.main()
