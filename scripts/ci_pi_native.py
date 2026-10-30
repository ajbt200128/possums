"""Explicit isolated native CI preparation; no publisher, client install or live probe."""
import json
import os
import platform
import re
import shutil
import sys
import tempfile
from pathlib import Path

import tor_native_build as native
import tor_native_qualification as qualification

REPO = Path(__file__).resolve().parents[1]


def stage_run(stage, command, *, env=None, cwd=REPO):
    try:
        return native.run(command, env=env, cwd=cwd)
    except native.BuildFailure as error:
        # Retain the closed tool failure category while naming its CI owner.
        raise native.BuildFailure("possums_pi_native_ci_" + stage + ": this native CI stage failed. " + str(error)) from None


def prepare(target, output):
    if target not in native.TARGETS or output.exists():
        raise native.BuildFailure("possums_pi_native_ci_input: supply an approved target and a new output directory. Existing output will not be overwritten.")
    commit = stage_run("source", ["git", "rev-parse", "HEAD"]).strip()
    expected = os.environ.get("GITHUB_SHA", commit)
    if (not re.fullmatch(r"[a-f0-9]{40}", commit) or commit != expected
            or stage_run("source", ["git", "status", "--porcelain", "--untracked-files=all"]).strip()):
        raise native.BuildFailure("possums_pi_native_ci_source: the checkout is dirty or differs from the exact requested commit. Check out the reviewed source; no native build was started.")
    if stage_run("node", ["node", "--version"]).strip() != "v24.13.0":
        raise native.BuildFailure("possums_pi_native_ci_node: Node 24.13.0 is required for native qualification. Select the exact pinned runtime; no native build was started.")
    rust, _, compiler, cargo = native.toolchain()
    if native.LOCAL_TARGETS[rust["host"]][:2] != native.TARGETS[target][:2]:
        raise native.BuildFailure("possums_pi_native_ci_runner: the compiler host does not match the requested architecture and OS. Use the native target runner; no cross-build execution claim is permitted.")
    with tempfile.TemporaryDirectory(prefix="possums-pi-native-ci-", dir="/tmp") as tmp:
        root = Path(tmp)
        source = root / "source"
        (source / "src").mkdir(parents=True)
        (root / "home").mkdir()
        for name in native.SOURCES:
            shutil.copyfile(REPO / "clients/pi/tor-native" / name, source / name)
        # Public dependency acquisition is separate from the locked offline build.
        # Do not discover Cargo credentials/config or walk the checkout's parents.
        certificate = os.environ.get("SSL_CERT_FILE") or os.environ.get("NIX_SSL_CERT_FILE")
        if not certificate or not Path(certificate).is_absolute() or not Path(certificate).is_file():
            raise native.BuildFailure("possums_pi_native_ci_certificate: the public build CA bundle is unavailable. Enter the pinned pi-native shell and retain its certificate bundle; do not disable TLS verification.")
        env = {"PATH": os.environ["PATH"], "HOME": str(root / "home"), "CARGO_HOME": str(root / "cargo-home"),
               "RUSTC": str(compiler), "RUSTUP_AUTO_INSTALL": "0", "LC_ALL": "C",
               "SSL_CERT_FILE": certificate, "CARGO_HTTP_CAINFO": certificate}
        stage_run("prefetch", [str(cargo), "fetch", "--locked", "--target", target,
                               "--manifest-path", str(source / "Cargo.toml")], env=env, cwd=source)
        env["CARGO_NET_OFFLINE"] = "true"
        packages = []
        for _ in range(2):
            built = stage_run("build", [sys.executable, str(REPO / "scripts/tor_native_build.py"), "--target", target], env=env)
            try:
                result = json.loads(built)
                packages.append(Path(result["torNativeBuild"]))
            except (json.JSONDecodeError, KeyError, TypeError):
                raise native.BuildFailure("possums_pi_native_ci_build_output: native build output could not be decoded or validated. Keep the target unqualified; check the pinned builder. No artifact was accepted.") from None
        first, second = packages
        digest = native.sha(first / "possums-tor-native")
        if digest != native.sha(second / "possums-tor-native"):
            raise native.BuildFailure("possums_pi_native_ci_reproducibility: independent scratch native builds differ. Keep the target unqualified; inspect the pinned public inputs and compiler path mapping.")
        # The existing full native suite includes synthetic TLS, errors and cleanup.
        test_env = dict(env, POSSUMS_TOR_NATIVE_TEST_HELPER=str(first / "possums-tor-native"))
        stage_run("host_tests", ["node", "--test", "tests/tor_native_transport.mjs"], env=test_env)
        evidence = {"target": target, "sourceCommit": commit, "helperSha256": digest,
                    "node": "24.13.0", "repeatBuild": "identical", "hostTransportTests": "passed",
                    "publicRuntimeProbes": False,
                    "sources": {name: native.sha(REPO / name) for name in
                                ("scripts/ci_pi_native.py", "tests/tor_native_transport.mjs")}}
        if target in qualification.TARGETS:
            config = root / "docker-config"
            config.mkdir()
            docker_env = {"PATH": os.environ["PATH"], "LC_ALL": "C"}
            for image in qualification.IMAGES.values():
                stage_run("image_preparation", ["docker", "--host", "unix:///var/run/docker.sock", "--config", str(config),
                                                "pull", image], env=docker_env)
            evidence["cleanUserspace"] = qualification.qualify(target, first)
        else:
            version = platform.mac_ver()[0]
            if not re.fullmatch(r"[0-9]+\.[0-9]+(?:\.[0-9]+)?", version):
                raise native.BuildFailure("possums_pi_native_ci_os: the observed macOS version could not be established. Keep OS-floor qualification open; report only a validated observed version.")
            evidence["macOS"] = version
        if (stage_run("source", ["git", "rev-parse", "HEAD"]).strip() != commit
                or stage_run("source", ["git", "status", "--porcelain", "--untracked-files=all"]).strip()):
            raise native.BuildFailure("possums_pi_native_ci_source_changed: source changed during qualification. Discard this candidate and check out the reviewed commit; no artifact was accepted.")
        output.mkdir(parents=True)
        for name in ("possums-tor-native", "tor-native-build-report.json"):
            shutil.copy2(first / name, output / name)
        (output / "tor-native-qualification.json").write_text(json.dumps(evidence, indent=2) + "\n")
        return evidence


if __name__ == "__main__":
    try:
        if len(sys.argv) != 3:
            raise native.BuildFailure("possums_pi_native_ci_input: supply an approved native target and a new scratch output directory. No work was started.")
        print(json.dumps(prepare(sys.argv[1], Path(sys.argv[2]))))
    except (native.BuildFailure, qualification.QualificationFailure, qualification.PortabilityFailure) as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
    except Exception:
        print("possums_pi_native_ci_unknown: isolated native CI preparation or reporting failed; cause unknown. Check the pinned runner, scratch storage and public inputs; keep the target unqualified.", file=sys.stderr)
        sys.exit(1)
