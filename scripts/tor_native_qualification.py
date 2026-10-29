"""Offline Linux qualification; requires pre-provisioned digest-pinned images."""
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from tor_native_build import SOURCES, sha
from tor_native_portability import PortabilityFailure, inspect

# Official Node 24.13.0 index digests, checked against Docker Hub tag metadata.
# Multi-architecture indices; selected image AND engine must match the runner.
IMAGES = {
    "glibc": "node@sha256:4660b1ca8b28d6d1906fd644abe34b2ed81d15434d26d845ef0aced307cf4b6f",
    "musl": "node@sha256:cd6fb7efa6490f039f3471a189214d5f548c11df1ff9e5b181aa49e22c14383e",
}
TARGETS = {
    "aarch64-unknown-linux-musl": ("arm64", "aarch64"),
    "x86_64-unknown-linux-musl": ("x64", "x86_64"),
}
FIXTURES = ("tests/tor_native_smoke.mjs", "clients/pi/tor-native-transport.mjs")
REPO = Path(__file__).resolve().parents[1]


class QualificationFailure(Exception):
    """Only authored diagnostic messages, never tool output or exception text."""


def run(command, stage):
    # Stage labels are closed call-site constants, never tool/user input.
    assert stage in {"engine", "image", "create", "execute", "outcome", "cleanup"}
    code = "possums_pi_native_qualification_" + stage
    try:
        result = subprocess.run(command, env={"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "LC_ALL": "C"},
                                capture_output=True, timeout=120, check=False)
    except subprocess.TimeoutExpired:
        raise QualificationFailure(code + "_deadline: the local tool exceeded its deadline at this qualification stage. Inspect the isolated fixture runner before another deliberate attempt; the operation outcome is unknown.") from None
    except OSError:
        raise QualificationFailure(code + "_start: the local tool could not start at this qualification stage; cause unknown. Provide Docker and the pinned inspection tools.") from None
    if result.returncode:
        raise QualificationFailure(code + "_exit: the local tool exited unsuccessfully at this qualification stage; cause unknown. Check the isolated runner and pre-provisioned images; raw tool output is withheld.")
    try:
        return result.stdout.decode("utf-8")
    except UnicodeError:
        raise QualificationFailure(code + "_decode: local tool output could not be decoded at this qualification stage. Check the pinned tools; keep the target unqualified.") from None


def validate(target: str, package: Path) -> tuple[Path, str, str]:
    if target not in TARGETS or sys.platform != "linux" or platform.machine() != TARGETS[target][1]:
        raise QualificationFailure("possums_pi_native_qualification_runner: use the selected architecture's native Linux runner. Emulated or cross-built output alone does not qualify execution.")
    report_path = package / "tor-native-build-report.json"
    if report_path.stat().st_size > 65536:
        raise QualificationFailure("possums_pi_native_qualification_report: build report exceeds the reviewed input bound. Use the matching native build report.")
    try:
        report = json.loads(report_path.read_text())
    except (OSError, UnicodeError, json.JSONDecodeError):
        raise QualificationFailure("possums_pi_native_qualification_report: the local build report could not be read or decoded. Restore the matching public build report; keep the target unqualified.") from None
    artifact = package / "possums-tor-native"
    arch = TARGETS[target][0]
    sources = {"clients/pi/tor-native/" + name for name in SOURCES} | {
        "scripts/build-tor-native.sh", "scripts/tor_native_build.py", "scripts/tor_native_portability.py"}
    expected = {name: sha(REPO / name) for name in sources}
    digest = sha(artifact)
    if not (report.get("target") == target and report.get("distributionTarget") is True
            and report.get("platform") == "linux" and report.get("arch") == arch
            and report.get("rust") == "1.92.0" and report.get("tor") == "0.47.0"
            and report.get("sources") == expected
            and report.get("artifacts") == {"possums-tor-native": {"sha256": digest, "size": artifact.stat().st_size}}
            and re.fullmatch(r"[a-f0-9]{64}", report.get("targetStdSha256", ""))
            and report.get("linker") == {"kind": "rust-lld", "abi": "musl", "selfContained": True}):
        raise QualificationFailure("possums_pi_native_qualification_inputs: target, source, compiler contract or artifact correspondence failed. Rebuild the exact source with the explicit distribution target; no runtime check was started.")
    # Independently inspect the bytes, not just the producer's claimed linkage.
    if report.get("portability") != inspect(target, artifact):
        raise QualificationFailure("possums_pi_native_qualification_linkage: observed linkage differs from the report. Rebuild and independently inspect the candidate; no runtime check was started.")
    return artifact, digest, arch


def qualify(target, package):
    artifact, digest, arch = validate(target, package)
    with tempfile.TemporaryDirectory(prefix="possums-native-qualification-", dir="/tmp") as tmp:
        root = Path(tmp)
        config = root / "docker-config"
        config.mkdir()
        # Do not discover personal Docker contexts/auth or accept a remote engine.
        docker = ["docker", "--host", "unix:///var/run/docker.sock", "--config", str(config)]
        expected_engine = {"arm64": {"linux/aarch64", "linux/arm64"}, "x64": {"linux/x86_64", "linux/amd64"}}[arch]
        if run([*docker, "info", "--format", "{{.OSType}}/{{.Architecture}}"], "engine").strip() not in expected_engine:
            raise QualificationFailure("possums_pi_native_qualification_engine: the local Docker engine architecture does not match the selected target. Use a native matching Linux runner; no emulation fallback is allowed.")
        stage = root / "fixture"
        stage.mkdir(mode=0o755)
        for name in FIXTURES:
            destination = stage / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(REPO / name, destination)
            destination.chmod(0o644)
        shutil.copyfile(artifact, stage / "possums-tor-native")
        (stage / "possums-tor-native").chmod(0o755)
        for libc, image in IMAGES.items():
            image_arch = "arm64" if arch == "arm64" else "amd64"
            if run([*docker, "image", "inspect", "--format", "{{.Os}}/{{.Architecture}}", image], "image").strip() != "linux/" + image_arch:
                raise QualificationFailure("possums_pi_native_qualification_image: the pre-provisioned image architecture is wrong. Provision the pinned image on the matching runner; no image pull or emulation was attempted.")
            # Own a named fixture even when create/start delivery is interrupted.
            name = root.name + "-" + libc
            primary = None
            try:
                run([*docker, "create", "--name", name, "--pull=never", "--network=none", "--read-only",
                     "--cap-drop=ALL", "--security-opt=no-new-privileges", "--user=65532:65532",
                     "--tmpfs", "/tmp:rw,noexec,nosuid,nodev,size=64m,mode=1777",  # nosec B108  # Container-only memory filesystem, not host storage.
                     "--mount", f"type=bind,src={stage},dst=/fixture,readonly", "--workdir=/fixture",
                     "--env", "POSSUMS_TOR_NATIVE_TEST_HELPER=/fixture/possums-tor-native",
                     "--env", "POSSUMS_TOR_NATIVE_TEST_ARCH=" + arch,
                     "--env", "POSSUMS_TOR_NATIVE_TEST_LIBC=" + libc,
                     "--entrypoint=/usr/local/bin/node", image, "--test", "tests/tor_native_smoke.mjs"], "create")
                run([*docker, "start", "--attach", name], "execute")
                # Attached output alone is not exit-status evidence.
                if run([*docker, "inspect", "--format", "{{.State.ExitCode}}/{{.State.Running}}", name], "outcome").strip() != "0/false":
                    raise QualificationFailure("possums_pi_native_qualification_runtime: the synthetic helper round trip did not complete successfully. Keep the target unqualified; no public traffic was permitted.")
            except BaseException as error:
                primary = error
                raise
            finally:
                # Failed create delivery has an unknown outcome. Remove only
                # our owned name, never enumerate or modify unrelated containers.
                try:
                    run([*docker, "rm", "--force", name], "cleanup")
                except QualificationFailure:
                    if primary is None:
                        raise QualificationFailure("possums_pi_native_qualification_cleanup: fixture cleanup could not be confirmed; cause unknown. Check the isolated runner's fixture state; no public traffic was permitted.") from None
                    print("possums_pi_native_qualification_cleanup: cleanup could not be confirmed after a primary failure. Check the isolated runner's fixture state; the primary failure is preserved.", file=sys.stderr)
        return {"target": target, "platform": "linux", "arch": arch, "node": "24.13.0",
                "helperSha256": digest, "userspaces": dict(IMAGES),
                "sources": {name: sha(REPO / name) for name in (*FIXTURES, "scripts/tor_native_qualification.py")},
                "execution": "synthetic_loopback_passed", "publicNetwork": False,
                "scope": "clean glibc/musl helper launch and SOCKS round trip; not full client, TLS, Tor or inference qualification"}


if __name__ == "__main__":
    try:
        if len(sys.argv) != 3:
            raise QualificationFailure("possums_pi_native_qualification_input: supply one approved Linux musl target and its package directory. No runtime check was started.")
        print(json.dumps(qualify(sys.argv[1], Path(sys.argv[2]))))
    except (QualificationFailure, PortabilityFailure) as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
    except Exception:
        print("possums_pi_native_qualification_unknown: local qualification preparation or evidence validation failed; cause unknown. Check the isolated runner and matching public artifacts; keep the target unqualified.", file=sys.stderr)
        sys.exit(1)
