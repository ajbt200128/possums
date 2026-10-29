"""Synthetic policy/ownership tests; no Docker, compiler, download or network."""
import contextlib
import copy
import importlib.util
import io
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("native_qualification", ROOT / "scripts/tor_native_qualification.py")
assert spec is not None and spec.loader is not None
q = importlib.util.module_from_spec(spec)
with patch.object(sys, "path", [str(ROOT / "scripts"), *sys.path]):
    spec.loader.exec_module(q)
TARGET = "aarch64-unknown-linux-musl"
PORTABILITY = {"format": "elf64", "linkage": "static", "execution": "not_tested"}


class QualificationTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.package = Path(self.tmp.name)
        artifact = self.package / "possums-tor-native"
        artifact.write_bytes(b"synthetic-public-helper")
        artifact.chmod(0o755)
        self.report = {"target": TARGET, "distributionTarget": True, "platform": "linux", "arch": "arm64",
                       "rust": "1.92.0", "tor": "0.47.0", "targetStdSha256": "a" * 64,
                       "linker": {"kind": "rust-lld", "abi": "musl", "selfContained": True},
                       "portability": PORTABILITY,
                       "sources": {name: q.sha(ROOT / name) for name in
                                   [*("clients/pi/tor-native/" + name for name in q.SOURCES),
                                    "scripts/build-tor-native.sh", "scripts/tor_native_build.py", "scripts/tor_native_portability.py"]},
                       "artifacts": {"possums-tor-native": {"sha256": q.sha(artifact), "size": artifact.stat().st_size}}}
        for context in [patch.object(q.sys, "platform", "linux"), patch.object(q.platform, "machine", return_value="aarch64"),
                        patch.object(q, "inspect", return_value=PORTABILITY)]:
            context.start()
            self.addCleanup(context.stop)
        self.save(self.report)

    def save(self, report):
        (self.package / "tor-native-build-report.json").write_text(json.dumps(report))

    @staticmethod
    def tool(command, stage):
        return {"engine": "linux/aarch64", "image": "linux/arm64", "outcome": "0/false"}.get(stage, "")

    def test_success_requires_both_digest_pinned_images_and_closed_runtime(self):
        calls = []

        def run(command, stage):
            calls.append((command, stage))
            if stage == "create":
                self.assertIn("--pull=never", command)
                self.assertIn("--network=none", command)
                self.assertIn("--read-only", command)
                self.assertIn("--user=65532:65532", command)
                self.assertIn("--cap-drop=ALL", command)
                self.assertIn("--security-opt=no-new-privileges", command)
                self.assertNotIn("--platform", command)
                self.assertNotIn("--privileged", command)
                self.assertIn("unix:///var/run/docker.sock", command)
                mount = command[command.index("--mount") + 1]
                stage_path = Path(mount.split("src=", 1)[1].split(",", 1)[0])
                self.assertEqual({str(p.relative_to(stage_path)) for p in stage_path.rglob("*") if p.is_file()},
                                 {*q.FIXTURES, "possums-tor-native"})
            return self.tool(command, stage)

        with patch.object(q, "run", side_effect=run):
            report = q.qualify(TARGET, self.package)
        self.assertEqual(report["userspaces"], q.IMAGES)
        self.assertEqual(report["execution"], "synthetic_loopback_passed")
        self.assertFalse(report["publicNetwork"])
        self.assertEqual([stage for _, stage in calls].count("create"), 2)
        self.assertEqual([stage for _, stage in calls].count("cleanup"), 2)
        for image in q.IMAGES.values():
            self.assertRegex(image, r"^node@sha256:[a-f0-9]{64}$")
            self.assertTrue(any(image in command for command, stage in calls if stage == "create"))

    def test_wrong_runner_engine_or_image_stops_before_create(self):
        with patch.object(q.platform, "machine", return_value="x86_64"), patch.object(q, "run") as invoked:
            with self.assertRaisesRegex(q.QualificationFailure, "qualification_runner"):
                q.qualify(TARGET, self.package)
            invoked.assert_not_called()
        for failed_stage, response in [("engine", "linux/amd64"), ("image", "linux/amd64")]:
            calls = []

            def run(command, stage, calls=calls, response=response, failed_stage=failed_stage):
                calls.append(stage)
                return response if stage == failed_stage else self.tool(command, stage)

            with patch.object(q, "run", side_effect=run), self.assertRaisesRegex(q.QualificationFailure, "qualification_" + failed_stage):
                q.qualify(TARGET, self.package)
            self.assertNotIn("create", calls)

    def test_report_tampering_and_local_host_output_are_rejected_before_docker(self):
        mutations = [{"distributionTarget": False}, {"target": "aarch64-unknown-linux-gnu"}, {"sources": {}},
                     {"arch": "x64"}, {"rust": "1.93.0"}, {"artifacts": {}},
                     {"targetStdSha256": "hostile-canary"}, {"linker": {"kind": "host"}},
                     {"portability": {"execution": "not_tested", "linkage": "unchecked_local_host"}}]
        for mutation in mutations:
            with self.subTest(mutation=mutation):
                report = copy.deepcopy(self.report)
                report.update(mutation)
                self.save(report)
                with patch.object(q, "run") as invoked, self.assertRaises(q.QualificationFailure) as failure:
                    q.qualify(TARGET, self.package)
                invoked.assert_not_called()
                self.assertNotIn("hostile-canary", str(failure.exception))

    def test_claimed_linkage_cannot_bypass_independent_inspection(self):
        with patch.object(q, "inspect", side_effect=q.PortabilityFailure("closed-inspector-failure")) as inspected:
            with patch.object(q, "run") as invoked, self.assertRaises(q.PortabilityFailure):
                q.qualify(TARGET, self.package)
            inspected.assert_called_once_with(TARGET, self.package / "possums-tor-native")
            invoked.assert_not_called()

    def test_nonzero_running_or_missing_exit_evidence_never_qualifies(self):
        for state in ["1/false", "0/true", "hostile-canary", ""]:
            stages = []

            def run(command, stage, stages=stages, state=state):
                stages.append(stage)
                return state if stage == "outcome" else self.tool(command, stage)

            with patch.object(q, "run", side_effect=run), self.assertRaisesRegex(q.QualificationFailure, "qualification_runtime"):
                q.qualify(TARGET, self.package)
            self.assertEqual(stages.count("create"), 1)
            self.assertEqual(stages.count("cleanup"), 1)

    def test_primary_failure_survives_cleanup_failure_without_next_runtime(self):
        for failed_stage in ["create", "execute", "outcome"]:
            stages = []

            def run(command, stage, stages=stages, failed_stage=failed_stage):
                stages.append(stage)
                if stage in {failed_stage, "cleanup"}:
                    raise q.QualificationFailure("primary-closed" if stage == failed_stage else "cleanup-closed")
                return self.tool(command, stage)

            warning = io.StringIO()
            with (patch.object(q, "run", side_effect=run), contextlib.redirect_stderr(warning),
                  self.assertRaisesRegex(q.QualificationFailure, "primary-closed")):
                q.qualify(TARGET, self.package)
            self.assertEqual(stages.count("create"), 1)
            self.assertEqual(stages.count("cleanup"), 1)
            self.assertIn("primary failure is preserved", warning.getvalue())

    def test_cleanup_failure_blocks_success(self):
        def run(command, stage):
            if stage == "cleanup":
                raise q.QualificationFailure("closed-cleanup")
            return self.tool(command, stage)

        with patch.object(q, "run", side_effect=run), self.assertRaisesRegex(q.QualificationFailure, "qualification_cleanup"):
            q.qualify(TARGET, self.package)

    def test_malformed_report_has_typed_private_fallback(self):
        (self.package / "tor-native-build-report.json").write_text("hostile-canary")
        with self.assertRaisesRegex(q.QualificationFailure, "qualification_report") as failure:
            q.qualify(TARGET, self.package)
        self.assertNotIn("hostile-canary", str(failure.exception))

    def test_real_tool_boundary_preserves_stage_and_suppresses_hostile_output(self):
        for stage in ["engine", "image", "create", "execute", "outcome", "cleanup"]:
            result = subprocess.CompletedProcess([], 1, b"hostile-canary", b"hostile-canary")
            with (patch.object(subprocess, "run", return_value=result) as invoked,
                  patch.dict(os.environ, {"DOCKER_HOST": "hostile-canary", "NPM_TOKEN": "hostile-canary"}),
                  self.assertRaisesRegex(q.QualificationFailure, "qualification_" + stage + "_exit") as failure):
                q.run(["docker", "synthetic"], stage)
            self.assertNotIn("hostile-canary", str(failure.exception))
            self.assertNotIn("DOCKER_HOST", invoked.call_args.kwargs["env"])
            self.assertNotIn("NPM_TOKEN", invoked.call_args.kwargs["env"])
        for fault, suffix in [(OSError("hostile-canary"), "start"),
                              (subprocess.TimeoutExpired("hostile-canary", 1), "deadline")]:
            with patch.object(subprocess, "run", side_effect=fault):
                with self.assertRaisesRegex(q.QualificationFailure, "execute_" + suffix) as failure:
                    q.run(["docker", "synthetic"], "execute")
            self.assertNotIn("hostile-canary", str(failure.exception))


if __name__ == "__main__":
    unittest.main()
