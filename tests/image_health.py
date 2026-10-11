"""Local, synthetic OCI-image health qualification; never contacts a provider.

Privacy boundary: only fixed codes are printed. Docker itself retains loaded images and
container metadata on the local engine; no claim of whole-host transience is made.
"""
import contextlib
import datetime as dt
import json
import os
import re
import selectors
import subprocess
import sys
import time
import uuid
from pathlib import Path


ACCOUNTS = '[{"id":"startup-canary","credential_sha256":"g80hPkaKL7rbUpYj1pvTaDr7jxOh8KvDFn00a94wNxk","demo_microunits":1000}]'
FAILURE = b"healthcheck_failed: local readiness unavailable\n"
OPTIONS = ["--read-only", "--tmpfs", os.path.join(os.sep, "tmp") + ":rw,size=64m,mode=1777", "--cap-drop", "ALL", "--security-opt", "no-new-privileges"]
IMAGES = ("possums-gateway:phase0", "possums-gateway-smoke:phase0")
MAX_OUTPUT = 8192


class QualificationError(Exception):
    pass


def require(condition, stage):
    if not condition:
        raise QualificationError(stage)


def docker(arguments, stage, timeout=15):
    """Bound Docker output, process lifetime and diagnostics, including hostile stderr."""
    process = None
    streams = selectors.DefaultSelector()
    output = {"stdout": bytearray(), "stderr": bytearray()}
    try:
        process = subprocess.Popen(["docker", *arguments], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        assert process.stdout is not None and process.stderr is not None
        for name, pipe in (("stdout", process.stdout), ("stderr", process.stderr)):
            streams.register(pipe, selectors.EVENT_READ, name)
        deadline = time.monotonic() + timeout
        while streams.get_map():
            ready = streams.select(max(0, deadline - time.monotonic()))
            require(bool(ready) and time.monotonic() <= deadline, stage)
            for key, _ in ready:
                chunk = os.read(key.fd, 4096)
                if not chunk:
                    streams.unregister(key.fileobj)
                    continue
                output[key.data].extend(chunk)
                require(sum(map(len, output.values())) <= MAX_OUTPUT, stage)
        require(process.wait(timeout=max(0, deadline - time.monotonic())) is not None, stage)
        return process.returncode, bytes(output["stdout"]), bytes(output["stderr"])
    except (OSError, ValueError, subprocess.TimeoutExpired):
        raise QualificationError(stage) from None
    finally:
        streams.close()
        if process is not None:
            if process.poll() is None:
                process.kill()
                with contextlib.suppress(subprocess.TimeoutExpired):
                    process.wait(timeout=1)
            assert process.stdout is not None and process.stderr is not None
            process.stdout.close()
            process.stderr.close()


def checked(arguments, stage, timeout=15):
    code, out, err = docker(arguments, stage, timeout)
    require(code == 0 and not err, stage)
    return out


def load(archive, image):
    require(Path(archive).is_file(), "archive_unavailable")
    result = checked(["load", "--input", str(archive)], "image_load", timeout=120)
    require(result == f"Loaded image: {image}\n".encode(), "image_identity")
    user = checked(["image", "inspect", "--format", "{{json .Config.User}}", image], "image_inspect")
    require(user == b'"65532:65532"\n', "image_user")


def container(name, image, network, arguments=(), fixture=False):
    command = ["create", "--name", name, "--network", network, *OPTIONS]
    if fixture:
        command += ["--env", "POSSUMS_ACCOUNTS_JSON=" + ACCOUNTS]
    if image == IMAGES[0]:
        command += ["--entrypoint", "/bin/possums"]
    command += [image, *arguments]
    require(bool(re.fullmatch(rb"[0-9a-f]{64}\n", checked(command, "container_create"))), "container_create")


def started(name):
    out = checked(["start", name], "container_start")
    require(out == (name + "\n").encode(), "container_start")


def fixture_ready(name):
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        state = checked(["inspect", "--format", "{{json .State.Running}}", name], "fixture_state")
        require(state == b"true\n", "fixture_state")
        code, out, err = docker(["logs", name], "fixture_logs")
        require(code == 0 and not err and out in (b"", b"http://127.0.0.1:8080\n"), "fixture_logs")
        if out:
            return
        time.sleep(0.1)
    raise QualificationError("fixture_ready")


def elapsed(state, stage):
    try:
        started_at = dt.datetime.fromisoformat(state["StartedAt"].replace("Z", "+00:00"))
        finished_at = dt.datetime.fromisoformat(state["FinishedAt"].replace("Z", "+00:00"))
        duration = (finished_at - started_at).total_seconds()
        require(0 <= duration <= 3, stage)
    except (KeyError, TypeError, ValueError, AttributeError):
        raise QualificationError(stage) from None


def probe(name, network, expected_code, stage):
    container(name, IMAGES[0], network, ["--healthcheck"])
    # Inspect stdout and stderr separately after completion, never print logs.
    checked(["start", name], stage)
    raw = checked(["wait", name], stage)
    require(raw == f"{expected_code}\n".encode(), stage)
    state_bytes = checked(["inspect", "--format", "{{json .State}}", name], stage)
    try:
        state = json.loads(state_bytes)
        require(type(state) is dict and state.get("ExitCode") == expected_code and state.get("Running") is False, stage)
    except (ValueError, TypeError):
        raise QualificationError(stage) from None
    code, out, err = docker(["logs", name], stage)
    require(code == 0 and out == b"" and err == (b"" if expected_code == 0 else FAILURE), stage)
    if expected_code != 0:
        elapsed(state, stage)


def qualify(production_archive, fixture_archive):
    require(Path(production_archive) != Path(fixture_archive), "archive_identity")
    load(production_archive, IMAGES[0])
    load(fixture_archive, IMAGES[1])
    owned = []
    failure = None
    try:
        for mode, expected in (("serving", 0), ("quiescing", 1)):
            fixture = "possums-health-fixture-" + uuid.uuid4().hex
            owned.append(fixture)
            container(fixture, IMAGES[1], "none", ["--health-" + mode], fixture=True)
            started(fixture)
            fixture_ready(fixture)
            name = "possums-health-probe-" + uuid.uuid4().hex
            owned.append(name)
            probe(name, "container:" + fixture, expected, "probe_" + mode)
        name = "possums-health-probe-" + uuid.uuid4().hex
        owned.append(name)
        probe(name, "none", 1, "probe_isolated")
    except BaseException as exc:
        failure = exc
        raise
    finally:
        cleanup_failed = False
        for name in reversed(owned):
            try:
                code, _, _ = docker(["rm", "--force", name], "container_cleanup")
                cleanup_failed |= code != 0
            except QualificationError:
                cleanup_failed = True
        if cleanup_failed and failure is None:
            raise QualificationError("container_cleanup")


def main(argv):
    require(len(argv) == 2, "arguments")
    qualify(*argv)


if __name__ == "__main__":
    try:
        main(sys.argv[1:])
    except (QualificationError, KeyboardInterrupt) as exc:
        code = str(exc) if isinstance(exc, QualificationError) else "interrupted"
        print(f"image_health_{code}: local image qualification unavailable; check the isolated CI job and retry qualification before publication", file=sys.stderr)
        sys.exit(1)
    except Exception:
        print("image_health_unexpected: local image qualification outcome unknown; check the isolated CI job before publication", file=sys.stderr)
        sys.exit(1)
