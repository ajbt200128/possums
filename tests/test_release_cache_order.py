"""Offline checks for pinned proxy ordering, never cache/service probes."""
import contextlib
import importlib.util
import io
import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("cache_order", ROOT / "scripts/ci-cache-order.py")
assert spec is not None and spec.loader is not None
cache_order = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cache_order)


class CacheOrderTests(unittest.TestCase):
    def test_changes_only_exact_proxy_priority(self):
        private = "access-tokens = SYNTHETIC-PRIVATE\n"
        upstream = "substituters = https://cache.nixos.org https://synthetic.invalid/cache\n"
        comment = "# " + cache_order.PROXY
        config = private + upstream + comment + cache_order.PROXY + "fallback = true\n"
        self.assertEqual(cache_order.upstream_first(config),
                         private + upstream + comment + cache_order.PROXY.replace("priority=1\n", "priority=50\n")
                         + "fallback = true\n")

    def test_missing_duplicate_or_changed_proxy_is_rejected(self):
        for config in ("", "# " + cache_order.PROXY, cache_order.PROXY * 2,
                       cache_order.PROXY.replace("37515", "37516"),
                       cache_order.PROXY.replace("trusted=1", "trusted=0"),
                       cache_order.PROXY.replace("priority=1", "priority=50")):
            with self.subTest(config_present=bool(config)), self.assertRaises(cache_order.ProxyEntryMismatch):
                cache_order.upstream_first(config)

    def test_updates_xdg_config_preserving_mode(self):
        with tempfile.TemporaryDirectory() as directory:
            config = Path(directory) / "nix/nix.conf"
            config.parent.mkdir()
            config.write_text(cache_order.PROXY)
            config.chmod(0o600)
            output = io.StringIO()
            with patch.dict(os.environ, {"XDG_CONFIG_HOME": directory}), contextlib.redirect_stdout(output):
                self.assertEqual(cache_order.main(), 0)
            self.assertEqual(json.loads(output.getvalue())["code"], "CI_CACHE_UPSTREAM_FIRST")
            self.assertEqual(config.read_text(), cache_order.PROXY.replace("priority=1\n", "priority=50\n"))
            self.assertEqual(config.stat().st_mode & 0o777, 0o600)

    def test_unknown_or_mismatched_config_does_not_expose_values(self):
        with tempfile.TemporaryDirectory() as directory:
            config = Path(directory) / "nix/nix.conf"
            config.parent.mkdir()
            config.write_text("SYNTHETIC-PRIVATE")
            output = io.StringIO()
            with patch.dict(os.environ, {"XDG_CONFIG_HOME": directory}), contextlib.redirect_stdout(output):
                self.assertEqual(cache_order.main(), 0)
            self.assertEqual(json.loads(output.getvalue())["code"], "CI_CACHE_ORDER_UNQUALIFIED")
            self.assertIs(json.loads(output.getvalue())["applied"], False)
            self.assertNotIn("SYNTHETIC", output.getvalue())
            self.assertEqual(config.read_text(), "SYNTHETIC-PRIVATE")
        output = io.StringIO()
        with patch.object(Path, "read_text", side_effect=FileNotFoundError), contextlib.redirect_stdout(output):
            self.assertEqual(cache_order.main(), 0)
        self.assertEqual(json.loads(output.getvalue())["code"], "CI_CACHE_ORDER_UNQUALIFIED")
        for operation in ("read_text", "write_text"):
            output = io.StringIO()
            with patch.object(Path, "read_text", return_value=cache_order.PROXY), \
                    patch.object(Path, operation, side_effect=OSError("SYNTHETIC-PRIVATE")), \
                    contextlib.redirect_stdout(output):
                self.assertEqual(cache_order.main(), 1)
            self.assertEqual(json.loads(output.getvalue())["code"], "CI_CACHE_CONFIG_UNKNOWN")
            self.assertNotIn("SYNTHETIC", output.getvalue())

    def test_both_builders_set_order_before_building_without_weakening_freshness(self):
        for workflow in ("check.yml", "publish-image.yml"):
            source = (ROOT / ".github/workflows" / workflow).read_text()
            configure = source.index("run: python3 scripts/ci-cache-order.py")
            self.assertLess(source.index("source-revision: 5a7f052c8f0c6511bbef3570eeed4d39242171ef"), configure)
            self.assertLess(configure, source.index("nix build"))
            self.assertIn("use-flakehub: false", source)
            self.assertIn("use-gha-cache: true", source)
            self.assertNotIn("diff-store:", source)
        publisher = (ROOT / ".github/workflows/publish-image.yml").read_text()
        self.assertIn("build: [a, b]", publisher)
        self.assertIn("python3 scripts/check-release-cache.py", publisher)
        self.assertIn('--option substitute false --option builders ""', publisher)
        self.assertLess(publisher.index("python3 scripts/check-release-cache.py"),
                        publisher.index('--option substitute false --option builders ""'))


if __name__ == "__main__":
    unittest.main()
