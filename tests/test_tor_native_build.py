"""Offline build-policy tests: synthetic tool output, no compiler or network work."""
import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("tor_native_build", ROOT / "scripts/tor_native_build.py")
assert spec is not None and spec.loader is not None
builder = importlib.util.module_from_spec(spec)
spec.loader.exec_module(builder)


class NativeBuildTests(unittest.TestCase):
    def test_distribution_targets_are_closed(self):
        self.assertEqual(len(builder.TARGETS), 4)
        for target in builder.TARGETS:
            self.assertEqual(builder.select_target(["--target", target], "aarch64-apple-darwin"), (target, True))
        for args in [["--target", "x86_64-unknown-linux-gnu"], ["--target", "../hostile-canary"],
                     ["--target", "x86_64-pc-windows-msvc"], ["--target"], ["--target", next(iter(builder.TARGETS)), "extra"]]:
            with self.subTest(args=args), self.assertRaises(builder.BuildFailure) as failure:
                builder.select_target(args, "aarch64-apple-darwin")
            self.assertNotIn("hostile-canary", str(failure.exception))

    def test_native_hosts_do_not_claim_distribution_qualification(self):
        for host in builder.LOCAL_TARGETS:
            self.assertEqual(builder.select_target([], host), (host, False))
        with self.assertRaises(builder.BuildFailure):
            builder.select_target([], "x86_64-pc-windows-msvc")

    def test_target_std_requires_installed_library_and_binds_bytes(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            with self.assertRaises(builder.BuildFailure):
                builder.target_std_digest(root)
            (root / "libstd-synthetic.rlib").write_bytes(b"public-runtime")
            first = builder.target_std_digest(root)
            self.assertEqual(first, builder.target_std_digest(root))
            (root / "libstd-synthetic.rlib").write_bytes(b"different-runtime")
            self.assertNotEqual(first, builder.target_std_digest(root))

    def test_musl_build_is_self_contained_and_clears_ambient_configuration(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            toolchain = root / "compiler"
            linker = toolchain / "lib/rustlib/aarch64-apple-darwin/bin/rust-lld"
            linker.parent.mkdir(parents=True)
            linker.touch()
            cache = root / "personal-cargo"
            (cache / "registry").mkdir(parents=True)
            (cache / "config.toml").write_text("hostile-canary")
            (cache / "credentials.toml").write_text("hostile-canary")
            scratch = root / "scratch"
            scratch.mkdir()
            with patch.dict(os.environ, {"CARGO_HOME": str(cache), "RUSTFLAGS": "hostile-canary", "NPM_TOKEN": "hostile-canary"}):
                env, details = builder.build_environment(scratch, toolchain, {"host": "aarch64-apple-darwin"}, "aarch64-unknown-linux-musl")
            self.assertEqual(details, {"kind": "rust-lld", "abi": "musl", "selfContained": True})
            self.assertIn("target-feature=+crt-static", env["RUSTFLAGS"])
            self.assertIn("link-self-contained=yes", env["RUSTFLAGS"])
            self.assertNotIn("hostile-canary", json.dumps(env))
            self.assertNotIn("NPM_TOKEN", env)
            self.assertEqual({p.name for p in (scratch / "cargo-home").iterdir()}, {"registry"})
            self.assertEqual(Path(env["HOME"]), scratch / "home")

    def test_missing_self_contained_linker_never_falls_back(self):
        with tempfile.TemporaryDirectory() as tmp, self.assertRaisesRegex(builder.BuildFailure, "possums_build_tor_linker"):
            builder.build_environment(Path(tmp), Path(tmp) / "missing", {"host": "aarch64-apple-darwin"}, "aarch64-unknown-linux-musl")

    def test_apple_uses_selected_sdk_and_explicit_floor(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            sdk = root / "sdk"
            sdk.mkdir()
            with patch.object(builder, "run", side_effect=[str(sdk), "26.4.1", "/usr/bin/clang", "Apple clang version 17.0.0 (public)"]):
                env, details = builder.build_environment(root, root, {"host": "aarch64-apple-darwin"}, "x86_64-apple-darwin")
            self.assertEqual(env["MACOSX_DEPLOYMENT_TARGET"], "14.0")
            self.assertEqual(env["CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER"], "/usr/bin/clang")
            self.assertEqual(details["sdk"], "26.4.1")
            self.assertNotIn(str(root), json.dumps(details))

    def test_apple_cross_os_build_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp, self.assertRaisesRegex(builder.BuildFailure, "possums_build_tor_sdk"):
            builder.build_environment(Path(tmp), Path(tmp), {"host": "x86_64-unknown-linux-gnu"}, "aarch64-apple-darwin")

    def test_tool_failure_never_echoes_output_or_arguments(self):
        result = subprocess.CompletedProcess(["synthetic"], 1, b"hostile-canary", b"hostile-canary")
        with patch.object(subprocess, "run", return_value=result), self.assertRaises(builder.BuildFailure) as failure:
            builder.run(["synthetic", "hostile-canary"])
        self.assertNotIn("hostile-canary", str(failure.exception))
        with patch.object(subprocess, "run", side_effect=OSError("hostile-canary")), self.assertRaises(builder.BuildFailure) as failure:
            builder.run(["synthetic"])
        self.assertNotIn("hostile-canary", str(failure.exception))

    def test_wrong_compiler_fails_before_build(self):
        with patch.object(builder, "run", return_value="release: 1.93.0\nhost: aarch64-apple-darwin\n"), self.assertRaisesRegex(builder.BuildFailure, "possums_build_tor_compiler"):
            builder.toolchain()

    def test_cli_unknown_failure_has_content_free_fallback(self):
        # Missing rustc exercises the actual script boundary, not an echo formatter.
        with tempfile.TemporaryDirectory() as tmp:
            result = subprocess.run([sys.executable, str(ROOT / "scripts/tor_native_build.py")],
                                    env={"PATH": tmp}, capture_output=True, check=False)
        self.assertEqual(result.returncode, 1)
        self.assertIn(b"possums_build_tor_tool_start", result.stderr)
        self.assertEqual(result.stdout, b"")
        self.assertNotIn(b"Traceback", result.stderr)


if __name__ == "__main__":
    unittest.main()
