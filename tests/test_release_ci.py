"""Offline guards for same-invocation CI output reuse, not release substitution."""
import os
import re
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CHECK = (ROOT / ".github/workflows/check.yml").read_text()


def job(name):
    match = re.search(rf"(?ms)^  {name}:\n(.*?)(?=^  [a-z][a-z-]*:\n|\Z)", CHECK)
    assert match is not None
    return match[1]


class CiImageHandoffTests(unittest.TestCase):
    def test_required_jobs_and_checks_remain(self):
        self.assertEqual(re.findall(r"^  ([a-z][a-z-]*):$", CHECK.split("jobs:\n")[1], re.M),
                         ["flake", "image", "image-startup"])
        self.assertIn("nix flake check --no-write-lock-file --print-build-logs", job("flake"))
        for suite in ("test_release*.py", "test_release_cache.py", "test_image_health.py", "test_image_tor.py"):
            self.assertIn(suite, job("flake"))
        self.assertIn("RELEASE_CACHE_GRAPH_CHECK=1", job("flake"))
        self.assertIn("../scripts/verify-serving/main_test.go", job("flake"))
        self.assertIn('-o "$RUNNER_TEMP/possums-verify-serving" ../scripts/verify-serving/main.go', job("flake"))
        self.assertIn("python3 scripts/deploy.py qualify-serving-tool", job("flake"))
        self.assertIn("python3 scripts/deploy.py qualify-serving-tool", job("image-startup"))
        self.assertIn('chmod 700 "$RUNNER_TEMP/possums-verify-serving"', job("image-startup"))

    def test_consumers_use_only_current_job_artifact_ids(self):
        for name in ("image", "image-startup"):
            consumer = job(name)
            self.assertIn("needs: flake", consumer)
            self.assertIn("artifact-ids: ${{ needs.flake.outputs.production-artifact }}", consumer)
            self.assertNotIn("nix build", consumer)
            self.assertNotIn("nix-installer", consumer)
            self.assertNotIn("run-id:", consumer)
            self.assertNotIn("github-token:", consumer)
            self.assertNotIn("pattern:", consumer)
        self.assertIn("artifact-ids: ${{ needs.flake.outputs.fixture-artifact }}", job("image-startup"))
        for kind in ("production", "fixture"):
            self.assertIn(f"{kind}-artifact: ${{{{ steps.{kind}.outputs.artifact-id }}}}", job("flake"))
            self.assertIn(f"name: ci-{kind}-${{{{ github.run_id }}}}-${{{{ github.run_attempt }}}}-${{{{ github.sha }}}}",
                          job("flake"))
        self.assertEqual(job("flake").count("if-no-files-found: error"), 2)
        self.assertEqual(job("flake").count("retention-days: 1"), 2)
        self.assertEqual(job("flake").count("compression-level: 0"), 2)

    def test_real_rootfs_and_health_qualification_remain(self):
        startup = job("image-startup")
        self.assertIn('chmod +x "$RUNNER_TEMP/ci-fixture/tor-image-probe"', startup)
        self.assertIn('python3 tests/image_tor.py "$RUNNER_TEMP/ci-production/production-image.tar.gz" '
                      '"$RUNNER_TEMP/ci-fixture/tor-image-probe"', startup)
        self.assertIn('python3 tests/image_health.py "$RUNNER_TEMP/ci-production/production-image.tar.gz" '
                      '"$RUNNER_TEMP/ci-fixture/fixture-image.tar.gz"', startup)
        for constraint in ("--network none", "--read-only", "--tmpfs /tmp:rw,size=64m,mode=1777",
                           "test \"$(docker logs", "startup-canary"):
            self.assertIn(constraint, startup)
        self.assertIn("linux/amd64", job("image"))

    def test_staging_copies_only_archives_and_public_verifiers(self):
        # Execute the actual staging script with a synthetic Nix boundary. No
        # Docker, credentials, network, application launch or store realization.
        match = re.search(r"(?ms)^      - name: Build CI images and key-free Tor probe once\n"
                          r"        run: \|\n(.*?)(?=^      -)", job("flake"))
        assert match is not None
        script = "\n".join(line[10:] for line in match[1].splitlines())
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / "bin"
            binary.mkdir()
            nix = binary / "nix"
            nix.write_text("#!/bin/sh\nset -eu\n"
                           'test "$1" = build && test "$2" = --no-write-lock-file\n'
                           'test "${FAIL_SYNTHETIC_NIX:-}" != true\n'
                           'case "$*" in\n'
                           '  *--no-link*) exit 0;;\n'
                           'esac\n'
                           'package=$3\n'
                           'test "$4" = --out-link\n'
                           'case "$package" in\n'
                           '  .#tor-image-probe) mkdir -p "$5/bin"; printf probe > "$5/bin/tor-image-probe";;\n'
                           '  .#gateway-image|.#gateway-smoke-image) printf "%s" "$package" > "$5";;\n'
                           '  *) exit 1;;\n'
                           'esac\n')
            nix.chmod(0o755)
            (root / "possums-verify-serving").write_bytes(b"serving-verifier")
            subprocess.run(["bash", "-e", "-c", script], check=True, capture_output=True,
                           env={**os.environ, "PATH": str(binary) + os.pathsep + os.environ["PATH"],
                                "RUNNER_TEMP": directory})
            production = root / "ci-production"
            fixture = root / "ci-fixture"
            self.assertEqual([p.name for p in production.iterdir()], ["production-image.tar.gz"])
            self.assertEqual({p.name for p in fixture.iterdir()},
                             {"fixture-image.tar.gz", "tor-image-probe", "possums-verify-serving"})
            self.assertEqual((production / "production-image.tar.gz").read_bytes(), b".#gateway-image")
            self.assertEqual((fixture / "fixture-image.tar.gz").read_bytes(), b".#gateway-smoke-image")
            self.assertEqual((fixture / "tor-image-probe").read_bytes(), b"probe")
            self.assertEqual((fixture / "possums-verify-serving").read_bytes(), b"serving-verifier")
            failed = root / "failed"
            failed.mkdir()
            result = subprocess.run(["bash", "-e", "-c", script], check=False, capture_output=True,
                                    env={**os.environ, "PATH": str(binary) + os.pathsep + os.environ["PATH"],
                                         "RUNNER_TEMP": str(failed), "FAIL_SYNTHETIC_NIX": "true"})
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(list(failed.iterdir()), [])

    def test_release_builders_stay_fresh_and_do_not_consume_ci_artifacts(self):
        publisher = (ROOT / ".github/workflows/publish-image.yml").read_text()
        self.assertIn("build: [a, b]", publisher)
        self.assertIn('nix build .#gateway-image --option substitute false --option builders ""', publisher)
        self.assertIn("python3 scripts/check-release-cache.py", publisher)
        self.assertNotIn("ci-production", publisher)
        self.assertNotIn("ci-fixture", publisher)


if __name__ == "__main__":
    unittest.main()
