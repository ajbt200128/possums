"""Offline source-driver contract tests; mocked builds are not source qualification."""
import copy
import importlib.util
import io
import json
import os
import runpy
import shutil
import subprocess
import sys
import tempfile
import time
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
from unittest.mock import Mock, patch

REPO = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO / "scripts"))
_spec = importlib.util.spec_from_file_location("pi_source_ci_under_test", REPO / "scripts/ci_pi_source.py")
assert _spec is not None and _spec.loader is not None
ci = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(ci)
HOSTILE = "PRIVATE_TOKEN=https://user:password@private.invalid/prompt"
COMMIT = "a" * 40


class SourceCITests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="pi-ci-offline-test-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.ca = self.root / "public-ca.pem"
        self.ca.write_text("synthetic public certificate fixture; never used for TLS")

    def safe_failure(self, call, stage, constraint="validation"):
        with self.assertRaises(ci.SourceFailure) as raised:
            call()
        error = raised.exception
        self.assertEqual((error.stage, error.constraint), (stage, constraint))
        self.assertEqual(error.code, "possums_pi_source_ci_" + stage)
        self.assertIn("stage=" + stage, str(error))
        self.assertIn("constraint=" + constraint, str(error))
        self.assertNotIn(HOSTILE, str(error))
        self.assertIsNone(error.__context__)
        return error

    def isolated_env(self):
        root = self.root / "environment"
        root.mkdir()
        with patch.dict(os.environ, {"SSL_CERT_FILE": str(self.ca), "PATH": os.defpath}, clear=True):
            return ci.environment(root)

    def sdk_fixture(self):
        directory = self.root / "sdk"
        shutil.copytree(REPO / "clients/pi/ci-sdk", directory)
        return directory

    def mutate_lock(self, mutation):
        directory = self.sdk_fixture()
        path = directory / "package-lock.json"
        lock = json.loads(path.read_text())
        mutation(lock)
        path.write_text(json.dumps(lock))
        self.safe_failure(lambda: ci.validate_sdk(directory), "sdk_lock")

    def test_environment_replaces_inherited_credentials_and_settings(self):
        root = self.root / "fresh"
        root.mkdir()
        hostile: dict[str, str] = dict.fromkeys(("HOME", "HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY",
                   "NODE_OPTIONS", "NPM_TOKEN", "AWS_ACCESS_KEY_ID", "GITHUB_TOKEN", "GH_TOKEN", "SSH_AUTH_SOCK",
                   "GIT_CONFIG_COUNT", "CARGO_REGISTRIES_CRATES_IO_TOKEN", "RUSTFLAGS", "GOFLAGS", "XDG_CONFIG_HOME"), HOSTILE)
        with patch.dict(os.environ, dict(hostile, SSL_CERT_FILE=str(self.ca), PATH=os.defpath), clear=True):
            env = ci.environment(root)
        self.assertFalse(set(hostile).difference({"HOME"}) & set(env))
        self.assertNotIn(HOSTILE, json.dumps(env))
        for key in ("HOME", "TMPDIR", "CARGO_HOME", "GOMODCACHE", "GOCACHE", "npm_config_cache"):
            directory = Path(env[key])
            self.assertEqual(directory.parent, root)
            self.assertEqual(list(directory.iterdir()), [])
        for key in ("npm_config_userconfig", "npm_config_globalconfig"):
            self.assertEqual(Path(env[key]).read_text(), "")
        for key in ("GIT_CONFIG_GLOBAL", "GIT_CONFIG_SYSTEM"):
            self.assertEqual(env[key], "/dev/null")
        for key, value in {"GOAUTH": "off", "GOENV": "off", "GOWORK": "off", "GOTOOLCHAIN": "local",
                           "RUSTUP_AUTO_INSTALL": "0", "npm_config_fetch_retries": "0"}.items():
            self.assertEqual(env[key], value)
        self.assertEqual(env["npm_config_registry"], "https://registry.npmjs.org")
        self.assertEqual(env["GOPROXY"], "https://proxy.golang.org")

    def test_missing_or_relative_certificate_fails_closed(self):
        for certificate in ("", "relative.pem", str(self.root / "missing"), str(self.root)):
            with self.subTest(certificate=certificate), patch.dict(os.environ, {"SSL_CERT_FILE": certificate}, clear=True):
                self.safe_failure(lambda: ci.environment(self.root), "certificate")

    def test_nix_certificate_fallback(self):
        root = self.root / "fresh"
        root.mkdir()
        with patch.dict(os.environ, {"NIX_SSL_CERT_FILE": str(self.ca)}, clear=True):
            self.assertEqual(ci.environment(root)["SSL_CERT_FILE"], str(self.ca))

    def test_exact_clean_snapshot(self):
        with patch.dict(os.environ, {"GITHUB_SHA": COMMIT}, clear=True), patch.object(ci, "run", side_effect=[COMMIT + "\n", ""]) as run:
            self.assertEqual(ci.snapshot({}), COMMIT)
        self.assertIn("--untracked-files=all", run.call_args_list[1].args[1])
        self.assertIn("core.fsmonitor=false", run.call_args_list[1].args[1])

    def test_snapshot_rejects_drift_dirty_and_malformed(self):
        for commit, status, expected in ((COMMIT, "?? private", COMMIT), (COMMIT, "", "b" * 40), ("not-a-commit", "", COMMIT)):
            with self.subTest(commit=commit, status=status), patch.dict(os.environ, {"GITHUB_SHA": expected}, clear=True), patch.object(ci, "run", side_effect=[commit, status]):
                self.safe_failure(lambda: ci.snapshot({}), "source")

    def tool_outputs(self):
        return ["v24.13.0", "11.6.2", "go version go1.27.2 linux/amd64", "rustc 1.92.0 (public)", "cargo 1.92.0 (public)"]

    def test_exact_tool_pins_supported_host(self):
        with patch.dict(os.environ, {"POSSUMS_PI_NATIVE_TARGET": "x86_64-unknown-linux-musl"}, clear=True), patch.object(ci.platform, "machine", return_value="x86_64"), patch.object(ci.sys, "platform", "linux"), patch.object(ci, "run", side_effect=self.tool_outputs()):
            self.assertEqual(ci.tools({}), "x86_64-unknown-linux-musl")

    def test_each_tool_pin_mismatch(self):
        for index in range(5):
            outputs = self.tool_outputs()
            outputs[index] = HOSTILE
            with self.subTest(tool=index), patch.object(ci, "run", side_effect=outputs):
                self.safe_failure(lambda: ci.tools({}), "tools_" + ("node", "npm", "go", "rustc", "cargo")[index])

    def test_target_missing_unknown_or_wrong_host(self):
        for target in ("", HOSTILE, "aarch64-apple-darwin"):
            with self.subTest(target=target), patch.dict(os.environ, {"POSSUMS_PI_NATIVE_TARGET": target}, clear=True), patch.object(ci.platform, "machine", return_value="x86_64"), patch.object(ci.sys, "platform", "linux"), patch.object(ci, "run", side_effect=self.tool_outputs()):
                self.safe_failure(lambda: ci.tools({}), "tools")

    def test_checked_in_sdk_has_120_locked_dependencies(self):
        directory = self.sdk_fixture()
        self.assertEqual(len(json.loads((directory / "package-lock.json").read_text())["packages"]) - 1, 120)
        ci.validate_sdk(directory)

    def test_hostile_lock_shapes(self):
        baseline = json.loads((REPO / "clients/pi/ci-sdk/package-lock.json").read_text())
        for packages in (None, [], "hostile", {"": []}, {"": baseline["packages"][""], "node_modules/bad": []}):
            with self.subTest(packages=type(packages).__name__):
                directory = self.root / "shape"
                directory.mkdir(exist_ok=True)
                shutil.copyfile(REPO / "clients/pi/ci-sdk/package.json", directory / "package.json")
                lock = dict(baseline, packages=packages)
                (directory / "package-lock.json").write_text(json.dumps(lock))
                self.safe_failure(lambda directory=directory: ci.validate_sdk(directory), "sdk_lock")

    def test_lock_rejects_urls_links_and_integrity(self):
        baseline = json.loads((REPO / "clients/pi/ci-sdk/package-lock.json").read_text())
        name = next(name for name in baseline["packages"] if name)
        mutations = [("resolved", value) for value in ("http://registry.npmjs.org/a", "https://evil.invalid/a", "file:/tmp/a", "git+https://registry.npmjs.org/a", "https://registry.npmjs.org/a?token=secret", "https://user:pass@registry.npmjs.org/a", "https://registry.npmjs.org/a#secret")]
        mutations += [("link", True), ("integrity", "sha1-AAAA"), ("integrity", "sha512-" + "A" * 85 + "B=="), ("integrity", None)]
        for field, value in mutations:
            with self.subTest(field=field, value=value):
                directory = self.root / "url"
                directory.mkdir(exist_ok=True)
                shutil.copyfile(REPO / "clients/pi/ci-sdk/package.json", directory / "package.json")
                lock = copy.deepcopy(baseline)
                lock["packages"][name][field] = value
                (directory / "package-lock.json").write_text(json.dumps(lock))
                self.safe_failure(lambda directory=directory: ci.validate_sdk(directory), "sdk_lock")

    def test_lock_rejects_traversal(self):
        self.mutate_lock(lambda lock: lock["packages"].update({"node_modules/../escape": copy.deepcopy(next(v for k, v in lock["packages"].items() if k))}))

    def test_lock_requires_direct_sdk_pins(self):
        self.mutate_lock(lambda lock: lock["packages"]["node_modules/@earendil-works/pi-ai"].update(version="1.3.1"))

    def test_lock_requires_root_sdk_pins(self):
        self.mutate_lock(lambda lock: lock["packages"][""].update(dependencies={}))

    def test_manifest_rejects_extra_scripts_and_wrong_pins(self):
        directory = self.sdk_fixture()
        path = directory / "package.json"
        baseline = json.loads(path.read_text())
        for replacement in (dict(baseline, scripts={"install": HOSTILE}), dict(baseline, dependencies={}), dict(baseline, engines={"node": "24"}), [], None):
            path.write_text(json.dumps(replacement))
            self.safe_failure(lambda: ci.validate_sdk(directory), "sdk_lock")

    def test_invalid_sdk_json_is_closed(self):
        directory = self.sdk_fixture()
        (directory / "package-lock.json").write_text(HOSTILE)
        self.safe_failure(lambda: ci.validate_sdk(directory), "sdk_lock")

    def test_builder_result_accepts_one_existing_absolute_directory(self):
        self.assertEqual(ci.builder_result(HOSTILE + "\n" + json.dumps({"output": str(self.root)}), "output"), self.root)

    def test_builder_result_rejects_missing_duplicate_relative_symlink(self):
        link = self.root / "link"
        link.symlink_to(self.root, target_is_directory=True)
        for text in (HOSTILE, '{"output":1}', '{"output":"relative"}', json.dumps({"output": str(link)}), json.dumps({"output": str(self.root / "missing")}), '\n'.join([json.dumps({"output": str(self.root)})] * 2)):
            with self.subTest(text=text):
                self.safe_failure(lambda text=text: ci.builder_result(text, "output"), "build_output")

    def test_run_real_exit_does_not_expose_stdout_stderr(self):
        self.safe_failure(lambda: ci.run("checks", [sys.executable, "-c", f"import sys;print({HOSTILE!r});print({HOSTILE!r},file=sys.stderr);sys.exit(2)"], self.isolated_env(), timeout=3), "checks", "exit")

    def test_run_start_error_is_closed_without_exception_context(self):
        with patch.object(ci.subprocess, "Popen", side_effect=OSError(HOSTILE)):
            self.safe_failure(lambda: ci.run("native_build", ["never"], {}), "native_build", "start")

    def test_run_read_failure_preserves_typed_constraint(self):
        process = Mock(returncode=0)
        process.communicate.side_effect = OSError(HOSTILE)
        with patch.object(ci.subprocess, "Popen", return_value=process), patch.object(ci.os, "killpg") as kill:
            self.safe_failure(lambda: ci.run("checks", ["never"], {}), "checks", "read")
        kill.assert_called_once_with(process.pid, ci.signal.SIGKILL)
        self.assertEqual(process.communicate.call_count, 2)
        process.stdout.close.assert_called_once()
        process.stderr.close.assert_called_once()

    def test_real_read_failure_terminates_and_reaps_owned_child(self):
        env = self.isolated_env()
        child = subprocess.Popen([sys.executable, "-c", "import time;time.sleep(30)"], env=env,
                                 stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
        communicate = child.communicate
        calls = 0
        def fault(**kwargs):
            nonlocal calls
            calls += 1
            if calls == 1:
                raise OSError(HOSTILE)
            return communicate(**kwargs)
        try:
            with patch.object(ci.subprocess, "Popen", return_value=child), patch.object(child, "communicate", side_effect=fault):
                self.safe_failure(lambda: ci.run("checks", ["never"], env, timeout=3), "checks", "read")
            self.assertIsNotNone(child.poll())
            self.assertLess(child.returncode, 0)
        finally:
            if child.poll() is None:
                os.killpg(child.pid, ci.signal.SIGKILL)
            child.communicate(timeout=3)

    def test_cleanup_interrupt_preserves_primary_and_closes_remaining_pipe(self):
        for constraint in ("exit", "timeout", "read"):
            process = Mock(returncode=2)
            process.communicate.side_effect = ([(b"", b"")] if constraint == "exit" else
                                               [subprocess.TimeoutExpired("never", 1), (b"", b"")] if constraint == "timeout" else
                                               [OSError(HOSTILE), (b"", b"")])
            process.stdout.close.side_effect = KeyboardInterrupt(HOSTILE)
            with self.subTest(constraint=constraint), patch.object(ci.subprocess, "Popen", return_value=process), patch.object(ci.os, "killpg"):
                self.safe_failure(lambda: ci.run("checks", ["never"], {}), "checks", constraint)
            process.stderr.close.assert_called_once()

    def test_stop_interrupt_preserves_primary_failure(self):
        process = Mock(returncode=None)
        process.communicate.side_effect = [OSError(HOSTILE), KeyboardInterrupt(HOSTILE)]
        with patch.object(ci.subprocess, "Popen", return_value=process), patch.object(ci.os, "killpg", side_effect=KeyboardInterrupt(HOSTILE)):
            self.safe_failure(lambda: ci.run("checks", ["never"], {}), "checks", "read")
        process.stderr.close.assert_called_once()

    def test_interrupt_stops_owned_process_and_propagates(self):
        process = Mock(returncode=None)
        process.communicate.side_effect = [KeyboardInterrupt(), (b"", b"")]
        with patch.object(ci.subprocess, "Popen", return_value=process), patch.object(ci.os, "killpg") as kill, self.assertRaises(KeyboardInterrupt):
            ci.run("checks", ["never"], {})
        kill.assert_called_once_with(process.pid, ci.signal.SIGKILL)
        process.stdout.close.assert_called_once()
        process.stderr.close.assert_called_once()

    def test_run_real_decode_error_is_closed(self):
        self.safe_failure(lambda: ci.run("report", [sys.executable, "-c", "import os;os.write(1,b'\\xff')"], self.isolated_env(), timeout=3), "report", "decode")

    def test_run_cleanup_does_not_replace_primary_failure(self):
        for returncode, constraint in ((2, "exit"), (0, "cleanup")):
            process = Mock(returncode=returncode)
            process.communicate.return_value = (HOSTILE.encode(), HOSTILE.encode())
            process.stdout.close.side_effect = OSError(HOSTILE)
            process.stderr.close.side_effect = OSError(HOSTILE)
            with self.subTest(returncode=returncode), patch.object(ci.subprocess, "Popen", return_value=process) as popen:
                self.safe_failure(lambda: ci.run("checks", ["never"], {}), "checks", constraint)
                self.assertTrue(popen.call_args.kwargs["start_new_session"])
                self.assertEqual(process.stderr.close.call_count, 1)

    @unittest.skipUnless(os.name == "posix", "process-group termination requires POSIX")
    def test_real_child_grandchild_timeout_is_bounded(self):
        # The outer watchdog also bounds a regression in the driver's pipe cleanup.
        program = (f"import sys;sys.path.insert(0,{str(REPO / 'scripts')!r});import ci_pi_source as c;"
                   "c.run('checks',[sys.executable,'-c',"
                   + repr("import subprocess,sys,time;subprocess.Popen([sys.executable,'-c','import time;time.sleep(3)']);time.sleep(3)")
                   + "],dict(__import__('os').environ),timeout=0.1)")
        started = time.monotonic()
        result = subprocess.run([sys.executable, "-c", program], capture_output=True, timeout=8, env=self.isolated_env())
        self.assertLess(time.monotonic() - started, 2.5)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b"constraint=timeout", result.stderr)
        self.assertNotIn(HOSTILE.encode(), result.stderr)

    def test_source_failure_unknown_inputs_are_closed(self):
        error = ci.SourceFailure(HOSTILE, HOSTILE)
        self.assertEqual((error.stage, error.constraint), ("unknown", "validation"))
        self.assertNotIn(HOSTILE, str(error))

    def cli(self, argv, failure=None, exit_code=1):
        stdout, stderr = io.StringIO(), io.StringIO()
        def tracer(frame, event, arg):
            if failure is not None and event == "call" and frame.f_code.co_name == "prepare" and frame.f_code.co_filename == str(REPO / "scripts/ci_pi_source.py"):
                # Use this execution's own exception type, just as prepare does.
                if isinstance(failure, ci.SourceFailure):
                    raise frame.f_globals["SourceFailure"](failure.stage, failure.constraint)
                raise failure
            return tracer
        with patch.object(sys, "argv", argv), redirect_stdout(stdout), redirect_stderr(stderr):
            previous = sys.gettrace()
            sys.settrace(tracer)
            try:
                with self.assertRaises(SystemExit) as raised:
                    runpy.run_path(str(REPO / "scripts/ci_pi_source.py"), run_name="__main__")
            finally:
                sys.settrace(previous)
        self.assertEqual(raised.exception.code, exit_code)
        self.assertEqual(stdout.getvalue(), "")
        self.assertNotIn(HOSTILE, stderr.getvalue())
        self.assertNotIn("Traceback", stderr.getvalue())
        return stderr.getvalue()

    def test_cli_arguments_rejected_before_work(self):
        self.assertIn("possums_pi_source_ci_input", self.cli(["driver", HOSTILE], RuntimeError(HOSTILE)))

    def test_cli_unexpected_failure_is_generic_closed(self):
        self.assertIn("possums_pi_source_ci_unknown", self.cli(["driver"], RuntimeError(HOSTILE)))

    def test_cli_interrupt_is_closed_exit_130(self):
        self.cli(["driver"], KeyboardInterrupt(HOSTILE), exit_code=130)

    def test_cli_known_failure_retains_stage_constraint(self):
        output = self.cli(["driver"], ci.SourceFailure("checks", "exit"))
        self.assertIn("stage=checks; constraint=exit", output)
        self.assertIn("subprocess boundary", output)

    def prepare_fixture(self, fail_stage=None, late_drift=False):
        calls = []
        outputs = {}
        for stage, key in (("verifier_build", "verifierBuild"), ("native_build", "torNativeBuild"), ("client_build", "output")):
            directory = self.root / stage
            directory.mkdir()
            outputs[stage] = json.dumps({key: str(directory)})
        def fake_run(stage, command, env, cwd=ci.REPO, timeout=600):
            calls.append((stage, command, dict(env), Path(cwd), timeout))
            self.assertNotIn(HOSTILE, json.dumps(env))
            if stage == fail_stage:
                raise ci.SourceFailure(stage, "exit")
            return outputs.get(stage, "")
        with patch.dict(os.environ, {"SSL_CERT_FILE": str(self.ca), "PATH": os.defpath, "GITHUB_TOKEN": HOSTILE, "HTTPS_PROXY": HOSTILE}, clear=True), patch.object(ci, "run", side_effect=fake_run), patch.object(ci, "tools", return_value="x86_64-unknown-linux-musl"), patch.object(ci, "snapshot", side_effect=[COMMIT, "b" * 40 if late_drift else COMMIT]) as snapshots:
            if fail_stage or late_drift:
                self.safe_failure(ci.prepare, fail_stage or "source", "exit" if fail_stage else "validation")
                receipt = None
            else:
                receipt = ci.prepare()
        return receipt, calls, snapshots.call_count

    def test_mocked_prepare_acquires_then_builds_offline_without_uploads(self):
        receipt, calls, snapshots = self.prepare_fixture()
        self.assertEqual([c[0] for c in calls], ["npm_acquisition", "npm_acquisition", "go_acquisition", "cargo_acquisition", "verifier_build", "native_build", "client_build", "report", "checks"])
        self.assertEqual(snapshots, 2)
        for stage, command, env, _cwd, _timeout in calls:
            self.assertFalse({"GITHUB_TOKEN", "HTTPS_PROXY", "NPM_TOKEN", "SSH_AUTH_SOCK"} & env.keys())
            self.assertFalse({"publish", "upload", "install", "--universal"} & set(command))
            if stage == "npm_acquisition":
                self.assertEqual(command[:2], ["npm", "ci"])
                for flag in ("--ignore-scripts", "--no-audit", "--no-fund", "--workspaces=false"):
                    self.assertIn(flag, command)
            if stage == "cargo_acquisition":
                self.assertIn("--locked", command)
            if stage.endswith("build") or stage in ("report", "checks"):
                for key, value in {"GOPROXY": "off", "GOSUMDB": "off", "CARGO_NET_OFFLINE": "true", "npm_config_offline": "true"}.items():
                    self.assertEqual(env[key], value)
        report = next(c[1] for c in calls if c[0] == "report")
        self.assertIn("clientReport(publicJSON", " ".join(report))
        checks = next(c[1] for c in calls if c[0] == "checks")
        self.assertTrue(checks[-1].endswith("source/clients/pi/check.mjs"))
        assert receipt is not None
        self.assertEqual(receipt["sourceCommit"], COMMIT)
        self.assertEqual(receipt["universalCandidate"], "not_established")
        self.assertEqual(receipt["packedChecks"], "synthetic_only")
        self.assertEqual(receipt["installation"], "not_attempted")
        self.assertEqual(receipt["publication"], "not_attempted")
        self.assertLess(len(json.dumps(receipt)), 1024)
        self.assertNotIn(str(self.root), json.dumps(receipt))
        self.assertFalse(Path(calls[0][2]["HOME"]).exists())

    def test_late_snapshot_change_withholds_success_receipt(self):
        receipt, calls, snapshots = self.prepare_fixture(late_drift=True)
        self.assertIsNone(receipt)
        self.assertEqual(snapshots, 2)
        self.assertEqual(calls[-1][0], "checks")

    def test_check_failure_withholds_receipt_and_cleans_scratch(self):
        receipt, calls, snapshots = self.prepare_fixture(fail_stage="checks")
        self.assertIsNone(receipt)
        self.assertEqual(snapshots, 1)
        self.assertFalse(Path(calls[0][2]["HOME"]).exists())

    def test_acquisition_failure_latches_before_build(self):
        receipt, calls, _ = self.prepare_fixture(fail_stage="cargo_acquisition")
        self.assertIsNone(receipt)
        self.assertEqual(calls[-1][0], "cargo_acquisition")
        self.assertFalse(any(c[0].endswith("build") for c in calls))

    def test_sdk_handoff_uses_validated_bytes_not_a_later_read(self):
        directory = self.sdk_fixture()
        expected = {name: (directory / name).read_bytes() for name in ("package.json", "package-lock.json")}
        documents = ci.validate_sdk(directory)
        (directory / "package-lock.json").write_text(HOSTILE)
        self.assertEqual(documents, expected)

    def test_scratch_cleanup_preserves_known_primary(self):
        for cleanup in (OSError(HOSTILE), KeyboardInterrupt(HOSTILE)):
            temporary = Mock(name=str(self.root))
            temporary.name = str(self.root)
            temporary.cleanup.side_effect = cleanup
            with self.subTest(cleanup=type(cleanup).__name__), patch.object(ci.tempfile, "TemporaryDirectory", return_value=temporary), patch.object(ci, "qualify", side_effect=ci.SourceFailure("checks", "exit")):
                self.safe_failure(ci.prepare, "checks", "exit")
            temporary.cleanup.assert_called_once()

    def test_scratch_cleanup_failure_withholds_success(self):
        temporary = Mock()
        temporary.name = str(self.root)
        temporary.cleanup.side_effect = OSError(HOSTILE)
        with patch.object(ci.tempfile, "TemporaryDirectory", return_value=temporary), patch.object(ci, "qualify", return_value={"sourceChecks": "passed"}):
            self.safe_failure(ci.prepare, "cleanup")

    def test_local_storage_failure_does_not_retain_raw_context(self):
        temporary = Mock()
        temporary.name = str(self.root)
        with patch.object(ci.tempfile, "TemporaryDirectory", return_value=temporary), patch.object(ci, "qualify", side_effect=OSError(HOSTILE)):
            self.safe_failure(ci.prepare, "storage")

    def test_read_only_workflow_contract(self):
        workflow = (REPO / ".github/workflows/check-pi-source.yml").read_text()
        for literal in ("pull_request:", "workflow_dispatch:", "contents: read", "ubuntu-24.04",
                        "persist-credentials: false", "node-version: 24.13.0", "package-manager-cache: false",
                        "diagnostic-endpoint: \"\"", "scripts/ci_pi_source.py", "test_pi_source_*.py",
                        "8e8c483db84b4bee98b60c0593521ed34d9990e8", "2028fbc5c25fe9cf00d9f06a71cc4710d4507903",
                        "21a544727d0c62386e78b4befe52d19ad12692e3"):
            self.assertIn(literal, workflow)
        for literal in ("id-token:", "secrets.", "upload-artifact", "actions/cache", "npm publish", "environment:", "pull_request_target:"):
            self.assertNotIn(literal, workflow)


if __name__ == "__main__":
    unittest.main()
