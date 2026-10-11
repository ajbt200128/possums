"""Offline negative checks for the local image health runner; Docker never invoked."""
import importlib.util
import subprocess
import sys
import unittest
from pathlib import Path
from unittest.mock import patch


spec = importlib.util.spec_from_file_location("image_health", Path(__file__).with_name("image_health.py"))
assert spec is not None and spec.loader is not None
health = importlib.util.module_from_spec(spec)
spec.loader.exec_module(health)
SENTINEL = "hostile-credential-account-header"


class HealthRunnerTests(unittest.TestCase):
    def fails(self, code, function, *args):
        with self.assertRaises(health.QualificationError) as result:
            function(*args)
        self.assertEqual(str(result.exception), code)
        self.assertNotIn(SENTINEL, str(result.exception))

    def test_arguments_and_archive_identity_fail_closed(self):
        self.fails("arguments", health.main, [])
        self.fails("arguments", health.main, ["a"])
        self.fails("archive_identity", health.qualify, "same", "same")
        with patch.object(health, "checked", return_value=SENTINEL.encode()), patch.object(health.Path, "is_file", return_value=True):
            self.fails("image_identity", health.load, "archive", health.IMAGES[0])

    def test_image_user_cannot_be_overridden(self):
        with patch.object(health.Path, "is_file", return_value=True), patch.object(health, "checked", side_effect=[b"Loaded image: possums-gateway:phase0\n", b'"root"\n']):
            self.fails("image_user", health.load, "archive", health.IMAGES[0])
        with patch.object(health, "checked", return_value=b"not-an-id\n") as checked:
            self.fails("container_create", health.container, "synthetic", health.IMAGES[0], "none")
            self.assertNotIn("--user", checked.call_args.args[0])

    def test_production_probe_overrides_only_its_entrypoint(self):
        state = b'{"ExitCode":0,"Running":false}'
        with patch.object(health, "checked", side_effect=[b"a" * 64 + b"\n", b"probe\n", b"0\n", state]) as checked, patch.object(health, "docker", return_value=(0, b"", b"")):
            health.probe("probe", "none", 0, "probe_serving")
            production = checked.call_args_list[0].args[0]
            self.assertEqual(production[production.index("--entrypoint") + 1], "/bin/possums")
            self.assertEqual(production[-2:], [health.IMAGES[0], "--healthcheck"])
            self.assertNotIn("--user", production)

        with patch.object(health, "checked", return_value=b"a" * 64 + b"\n") as checked:
            for arguments in ([], ["--health-serving"], ["--health-quiescing"]):
                with self.subTest(arguments=arguments):
                    health.container("fixture", health.IMAGES[1], "none", arguments, fixture=True)
                    fixture = checked.call_args.args[0]
                    self.assertNotIn("--entrypoint", fixture)
                    self.assertEqual(fixture[-1 - len(arguments):], [health.IMAGES[1], *arguments])
                    self.assertIn("POSSUMS_ACCOUNTS_JSON=" + health.ACCOUNTS, fixture)
                    self.assertNotIn("--user", fixture)

    def test_bad_inspection_and_negative_deadline(self):
        with patch.object(health, "container"), patch.object(health, "checked", side_effect=[b"", b"1\n", SENTINEL.encode()]):
            self.fails("probe_isolated", health.probe, "synthetic", "none", 1, "probe_isolated")
        state = b'{"ExitCode":1,"Running":false,"StartedAt":"2026-10-10T00:00:00Z","FinishedAt":"2026-10-10T00:00:04Z"}'
        with patch.object(health, "container"), patch.object(health, "checked", side_effect=[b"", b"1\n", state]), patch.object(health, "docker", return_value=(0, b"", health.FAILURE)):
            self.fails("probe_isolated", health.probe, "synthetic", "none", 1, "probe_isolated")

    def test_elapsed_requires_valid_process_clock(self):
        healthy = {"StartedAt": "2026-10-10T00:00:00.000000000Z", "FinishedAt": "2026-10-10T00:00:02.900000000Z"}
        health.elapsed(healthy, "probe_isolated")
        self.fails("probe_isolated", health.elapsed, {**healthy, "FinishedAt": "invalid"}, "probe_isolated")

    def test_hostile_probe_logs_and_cleanup_on_failure(self):
        state = b'{"ExitCode":1,"Running":false}'
        with patch.object(health, "container"), patch.object(health, "checked", side_effect=[b"", b"1\n", state]), patch.object(health, "docker", return_value=(0, SENTINEL.encode(), health.FAILURE)):
            self.fails("probe_quiescing", health.probe, "synthetic", "none", 1, "probe_quiescing")
        with patch.object(health, "load"), patch.object(health, "container", side_effect=health.QualificationError("container_create")), patch.object(health, "docker", return_value=(0, b"", b"")) as docker:
            self.fails("container_create", health.qualify, "production", "fixture")
            self.assertEqual(docker.call_args.args[0][:2], ["rm", "--force"])
        with patch.object(health, "load"), patch.object(health, "container", side_effect=KeyboardInterrupt), patch.object(health, "docker", return_value=(0, b"", b"")) as docker:
            with self.assertRaises(KeyboardInterrupt):
                health.qualify("production", "fixture")
            self.assertEqual(docker.call_args.args[0][:2], ["rm", "--force"])

    def test_bounded_subprocess_output_and_timeout_hide_hostile_text(self):
        real_popen = subprocess.Popen
        for snippet, timeout in (("import sys;sys.stderr.write('" + SENTINEL + "'*1000)", 2),
                                 ("import time;time.sleep(1)", 0.01)):
            with self.subTest(timeout=timeout):
                def spawn(*args, script=snippet, **kwargs):
                    return real_popen([sys.executable, "-c", script], **kwargs)
                with patch.object(health.subprocess, "Popen", side_effect=spawn):
                    self.fails("docker_boundary", health.docker, ["unused"], "docker_boundary", timeout)

    def test_cli_diagnostic_is_closed(self):
        result = subprocess.run([sys.executable, str(Path(__file__).with_name("image_health.py")), SENTINEL],
                                capture_output=True, timeout=3, check=False)
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout, b"")
        self.assertIn(b"image_health_arguments:", result.stderr)
        self.assertNotIn(SENTINEL.encode(), result.stderr)


if __name__ == "__main__":
    unittest.main()
