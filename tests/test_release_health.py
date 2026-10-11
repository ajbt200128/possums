"""Offline measured-health configuration checks; no provider or Docker calls."""
import importlib.util
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("release_health", ROOT / "scripts/release.py")
assert spec is not None and spec.loader is not None
release = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = release
spec.loader.exec_module(release)

HEALTH = b'''    healthcheck:
      test: ["CMD", "/bin/possums", "--healthcheck"]
      interval: 10s
      timeout: 5s
      retries: 3
      start_period: 60s
'''


class HealthReleaseTest(unittest.TestCase):
    def test_measured_gateway_has_fixed_credential_free_probe(self):
        source = (ROOT / "tinfoil-config.yml").read_bytes()
        self.assertEqual(source.count(HEALTH), 1)
        self.assertIn(b"    attestation: true\n" + HEALTH + b"    pids_limit: 256\n", source)

    def test_release_image_replacement_preserves_health_and_runtime(self):
        source = (ROOT / "tinfoil-config.yml").read_bytes()
        image = "sha256:" + "c" * 64
        replaced = release.transform(source, image)
        self.assertEqual(replaced.count(HEALTH), 1)
        # Replacing the image a second time must change nothing else.
        old_line = next(line for line in source.splitlines() if line.startswith(b"    image: "))
        old_digest = old_line.decode().rsplit("@", 1)[1]
        self.assertEqual(release.transform(replaced, old_digest), source)


if __name__ == "__main__":
    unittest.main()
