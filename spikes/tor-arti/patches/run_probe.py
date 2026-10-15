#!/usr/bin/env python3
"""Test-only patch on checksum-pinned actual Arti, in disposable scratch.

Run inside the pinned Nix native environment. Uses only an already-installed
Rust 1.99.0; installs nothing. No Tor client/bootstrap, subscriber or log artifact.
"""
import hashlib
import os
import shutil
import subprocess
import tarfile
import tempfile
import urllib.request
from pathlib import Path

VERSION = "0.47.0"
CHECKSUM = "43e6531f26fe6f44d71dabdc63b7845bae81547cc738aa942f0e3e482f776388"
FEATURES = "tokio,rustls,onion-service-service,onion-service-cli-extra,memquota,vanguards,static-sqlite"
FAILURES = {
    "toolchain": "arti_probe_toolchain: scratch compiler lookup failed; cause unknown; check the existing stable toolchain locally; do not install automatically.",
    "source": "arti_probe_source: public source acquisition/extraction failed; cause unknown; review local public-source availability; no Tor operation ran.",
    "patch": "arti_probe_patch: scratch patch preparation failed; cause unknown; review the checksum-pinned source and patch locally; no Tor operation ran.",
    "test": "arti_probe_test: real-library synthetic tests did not complete successfully; review the public-source build locally; no Tor operation ran.",
    "check": "arti_probe_check: patched library compilation did not complete successfully; review the public-source build locally; no Tor operation ran.",
}


def run():
    stage = "toolchain"
    try:
        here = Path(__file__).resolve().parent
        spike = here.parent
        compiler = subprocess.run(
            ["rustup", "which", "--toolchain", "stable", "rustc"],
            check=True, capture_output=True, text=True,
        ).stdout.strip()
        binaries = Path(compiler).parent
        env = os.environ.copy()
        env.update(RUSTC=compiler, RUSTDOC=str(binaries / "rustdoc"))
        env["PATH"] = str(binaries) + os.pathsep + env["PATH"]
        version = subprocess.run([compiler, "--version"], check=True,
                                 capture_output=True, text=True).stdout
        if not version.startswith("rustc 1.99.0 "):
            print("arti_probe_toolchain_mismatch: existing stable is not tested Rust 1.99.0; use the documented existing scratch compiler; do not install automatically.")
            return 1
        stage = "source"
        with tempfile.TemporaryDirectory(prefix="possums-arti-probe-") as scratch:
            scratch = Path(scratch)
            archives = list((Path.home() / ".cargo/registry/cache").glob(
                f"*/arti-client-{VERSION}.crate"))
            if len(archives) == 1:
                data = archives[0].read_bytes()
            else:
                with urllib.request.urlopen(
                    f"https://static.crates.io/crates/arti-client/arti-client-{VERSION}.crate",
                    timeout=30,
                ) as response:
                    data = response.read(4 * 1024 * 1024)
            if hashlib.sha256(data).hexdigest() != CHECKSUM:
                print("arti_probe_source_checksum: public archive does not match the pinned checksum; stop and reacquire approved public source; no patch/build/Tor operation ran.")
                return 1
            archive = scratch / "source.crate"
            archive.write_bytes(data)
            with tarfile.open(archive) as source:
                source.extractall(scratch, filter="data")
            stage = "patch"
            package = scratch / f"arti-client-{VERSION}"
            subprocess.run(["patch", "--batch", "--fuzz=0", "-p1", "-i",
                            str(here / "protocol-lifecycle-probe.patch")],
                           cwd=package, check=True)
            shutil.copyfile(here / "lifecycle_probe.rs", package / "src/lifecycle_probe.rs")
            manifest = package / "Cargo.toml"
            # Probe-only dependencies; no production manifest/cache edits.
            with manifest.open("a") as output:
                output.write('\n[workspace]\n')
                output.write(f'\n[dev-dependencies.possums-tor-arti-spike]\npath = "{spike}"\n')
                output.write('\n[dev-dependencies.tokio-util]\nversion = "=0.7.20"\nfeatures = ["rt"]\n')
            shutil.copyfile(spike / "Cargo.lock", package / "Cargo.lock")
            # Resolution for upstream unit-test dependencies is scratch-only.
            for command_stage, command in [
                ("test", ["cargo", "test", "--manifest-path", str(manifest),
                          "--no-default-features", "--features", FEATURES, "--lib",
                          "protostatus::lifecycle_probe"]),
                ("check", ["cargo", "check", "--locked", "--manifest-path", str(manifest),
                           "--no-default-features", "--features", FEATURES, "--lib"]),
            ]:
                stage = command_stage
                subprocess.run(command, env=env, check=True)
        print("arti_probe_checked: real 0.47.0 monitor tests passed; callback-only shutdown remains blocked.")
        return 0
    except (OSError, ValueError, subprocess.SubprocessError, tarfile.TarError):
        # No exception text, paths, response bodies or diagnostic artifact.
        print(FAILURES[stage])
        return 1


if __name__ == "__main__":
    raise SystemExit(run())
