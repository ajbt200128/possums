"""Offline checks of the evaluated helper; a stub replaces Tor before execution."""
import json
import os
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


class TorDevelopmentTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        result = subprocess.run(
            ["devenv", "eval", "scripts.possums-tor.exec"],
            check=True, capture_output=True, text=True,
        )
        cls.script = json.loads(result.stdout)["scripts.possums-tor.exec"]
        binary = re.search(r"/nix/store/[^\s]+/bin/tor", cls.script)
        if binary is None:
            raise AssertionError("evaluated helper must select pinned Tor")
        cls.binary = binary.group()

    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix="possums-tor-helper-fixture-")
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name)
        self.args = self.root / "arguments.json"
        stub = self.root / "stub-tor"
        stub.write_text(
            f"#!{sys.executable}\n"
            "import json, os, sys\n"
            "with open(os.environ['FIXTURE_ARGS'], 'w') as output:\n"
            "    json.dump(sys.argv[1:], output)\n"
        )
        stub.chmod(0o700)
        self.helper = self.root / "helper.sh"
        self.helper.write_text(self.script.replace(self.binary, str(stub)))
        self.env = {
            "PATH": os.environ["PATH"], "HOME": str(self.root / "home"),
            "XDG_STATE_HOME": str(self.root / 'state with "quotes"'),
            "FIXTURE_ARGS": str(self.args),
        }
        self.state = Path(self.env["XDG_STATE_HOME"]) / "possums-tor-client"

    def run_helper(self, *args):
        return subprocess.run(
            ["bash", str(self.helper), *args], env=self.env,
            capture_output=True, text=True, timeout=5,
        )

    def test_shell_syntax(self):
        subprocess.run(["bash", "-n", str(self.helper)], check=True, capture_output=True)

    def test_private_persistent_state_and_argv_not_generated_config(self):
        self.assertEqual(self.run_helper().returncode, 0)
        self.assertEqual(self.state.stat().st_mode & 0o777, 0o700)
        self.assertEqual((self.state / "data").stat().st_mode & 0o777, 0o700)
        args = json.loads(self.args.read_text())
        self.assertEqual(args[args.index("--DataDirectory") + 1], str(self.state / "data"))
        self.assertEqual(args[args.index("--SocksPort") + 1], "127.0.0.1:19050")
        self.assertEqual(args[args.index("-f") + 1], "/dev/null")
        self.assertEqual(args[args.index("--defaults-torrc") + 1], "/dev/null")
        self.assertEqual(args[args.index("--Log") + 1], "err file /dev/null")
        self.assertFalse((self.state / "torrc").exists())
        marker = self.state / "data" / "synthetic-guard-marker"
        marker.write_text("synthetic")
        self.assertEqual(self.run_helper().returncode, 0)
        self.assertEqual(marker.read_text(), "synthetic")

    def test_explicit_config_untouched_without_state_creation(self):
        config = self.root / "owner config"
        config.write_text("synthetic owner configuration")
        self.assertEqual(self.run_helper("--config", str(config)).returncode, 0)
        self.assertEqual(json.loads(self.args.read_text()), ["-f", str(config)])
        self.assertEqual(config.read_text(), "synthetic owner configuration")
        self.assertFalse(self.state.exists())

    def test_environment_config_passed_through(self):
        self.env["POSSUMS_TOR_CONFIG"] = str(self.root / "synthetic-owner-config")
        self.assertEqual(self.run_helper().returncode, 0)
        self.assertEqual(json.loads(self.args.read_text()), ["-f", self.env["POSSUMS_TOR_CONFIG"]])
        self.assertFalse(self.state.exists())

    def test_bad_arguments_not_echoed(self):
        for args in [("HOSTILE_ARGUMENT_DO_NOT_ECHO",), ("--config",), ("--config", "")]:
            result = self.run_helper(*args)
            self.assertEqual(result.returncode, 2)
            self.assertIn("code=tor_development_configuration", result.stderr)
            self.assertNotIn("HOSTILE_ARGUMENT", result.stderr)
            self.assertFalse(self.args.exists())

    def test_symlink_state_or_data_refused_without_target_changes(self):
        target = self.root / "synthetic-target"
        target.mkdir(mode=0o755)
        self.state.parent.mkdir(parents=True)
        self.state.symlink_to(target, target_is_directory=True)
        result = self.run_helper()
        self.assertEqual(result.returncode, 2)
        self.assertNotIn(str(self.state), result.stderr)
        self.assertEqual(target.stat().st_mode & 0o777, 0o755)
        self.assertFalse(self.args.exists())
        self.state.unlink()
        self.state.mkdir(mode=0o700)
        (self.state / "data").symlink_to(target, target_is_directory=True)
        self.assertEqual(self.run_helper().returncode, 2)
        self.assertEqual(target.stat().st_mode & 0o777, 0o755)

    def test_relative_state_refused(self):
        self.env["XDG_STATE_HOME"] = "synthetic-relative"
        self.assertEqual(self.run_helper().returncode, 2)
        self.assertFalse(self.args.exists())


if __name__ == "__main__":
    unittest.main()
