#!/usr/bin/python3
"""Darwin operator-only full-prepare/SDK probe; no key bytes leave the child.
Every private run first qualifies this exact executable/profile/output parser
using public synthetic inputs, actual denied IO, and hostile panic containment.
"""
import argparse
import hashlib
import os
from pathlib import Path
import re
import resource
import selectors
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parent.parent
BINARY = ROOT / "target/tor-offline-e2e-helper"
SHA256 = "7c776fb09dafe8145d45406f3438bd1e3e53d6ec7cc2c533953b8794c910461f"
NSS = ROOT / "target/missing_nss.dylib"
NSS_SHA256 = "59b1c8fbc17985117e4e907fae729555c9c9ec2c7cdae43a8339b0d77a972389"
ENTRY = "tor_runtime::enabled::tests::offline_e2e::operator_full_path"
CODES = frozenset((
    "PREPARE_CONFIGURATION", "PREPARE_STATE", "IDENTITY_INVALID", "IDENTITY_REJECTED",
    "GRANT_REJECTED", "CAUSE_UNKNOWN", "INPUT_FILE", "INPUT_PERMISSIONS", "INPUT_HOSTNAME",
    "NOT_AUTHORIZED", "ARGUMENTS", "DISABLED", "SANDBOX", "SANDBOX_QUALIFIED", "RUNTIME",
    "SDK_CLIENT", "SDK_KEYSTORE", "SDK_IDENTITY", "CLEANUP", "PREPARE_AND_SDK_OK",
    "NSS_INJECTED", "NSS_NOT_INJECTED", "PREPARE_PANIC", "RUNTIME_PANIC",
    "SDK_CLIENT_PANIC", "SDK_KEYSTORE_PANIC", "CLEANUP_PANIC", "SDK_BUILDER_PANIC", "SDK_CONFIG_PANIC",
))


def classify(status, stdout, stderr):
    if status != 0 or stderr:
        return "EXECUTION"
    pattern = (rb'\nrunning 1 test\ntest ' + re.escape(ENTRY.encode())
               + rb' \.\.\. POSSUMS_OFFLINE_E2E=([A-Z_]+)\nok\n\n'
               + rb'test result: ok\. 1 passed; 0 failed; 0 ignored; 0 measured; '
               + rb'\d+ filtered out; finished in \d+\.\d+s\n\n')
    match = re.fullmatch(pattern, stdout)
    if match is None:
        return "EXECUTION"
    code = match.group(1).decode("ascii")
    return code if code in CODES else "EXECUTION"


def invoke(mode, denied, paths=None, missing_nss=False):
    # Production prepare creates exclusive owner-only scratch beneath hardcoded
    # /tmp. Permit only that SDK runtime prefix, not generic /tmp or HOME writes.
    profile = ('(version 1) (allow default) (deny network* (with no-log)) '
               '(deny file-write* (with no-log)) '
               '(allow file-write* (regex #"^/private/tmp/possums-tor-[A-Za-z0-9]+(/.*)?$"))')
    env = {"HOME": "", "POSSUMS_OFFLINE_MODE": mode,
           "POSSUMS_OFFLINE_DENIED_PATH": str(denied)}
    if paths is not None:
        env.update(POSSUMS_OFFLINE_ACK="PRODUCTION_B_LOCAL_ONLY",
                   POSSUMS_OFFLINE_NATIVE_PATH=str(paths[0]),
                   POSSUMS_OFFLINE_HOSTNAME_PATH=str(paths[1]))
    command = ["/usr/bin/sandbox-exec", "-p", profile]
    if missing_nss:
        if paths is not None or mode == "private":
            return "NOT_AUTHORIZED"
        # SIP strips DYLD from sandbox-exec itself. env sets this public-only
        # fault injector immediately before exec of the non-SIP Rust child.
        command += ["/usr/bin/env", "DYLD_INSERT_LIBRARIES=" + str(NSS), "HOME=/tmp"]
    command += [str(BINARY), "--ignored", "--exact", ENTRY, "--nocapture", "--test-threads=1"]
    child = subprocess.Popen(command,
                             env=env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                             stderr=subprocess.PIPE)
    assert child.stdout is not None and child.stderr is not None
    streams = (child.stdout, child.stderr)
    buffers = {stream.fileno(): bytearray() for stream in streams}
    selector = selectors.DefaultSelector()
    try:
        for stream in streams:
            os.set_blocking(stream.fileno(), False)
            selector.register(stream, selectors.EVENT_READ)
        deadline = time.monotonic() + 20
        while selector.get_map():
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                return "EXECUTION"
            for key, _ in selector.select(min(remaining, 0.2)):
                data = os.read(key.fd, 4096)
                if not data:
                    selector.unregister(key.fileobj)
                else:
                    buffers[key.fd].extend(data)
                    if sum(map(len, buffers.values())) > 65536:
                        return "EXECUTION"
        status = child.wait(timeout=max(0.01, deadline - time.monotonic()))
        return classify(status, bytes(buffers[child.stdout.fileno()]), bytes(buffers[child.stderr.fileno()]))
    finally:
        selector.close()
        if child.poll() is None:
            child.kill()
        child.wait(timeout=5)
        for stream in streams:
            stream.close()
        for buffer in buffers.values():
            buffer[:] = b'\0' * len(buffer)


def projection_selftest():
    valid = (f"\nrunning 1 test\ntest {ENTRY} ... POSSUMS_OFFLINE_E2E=CAUSE_UNKNOWN\nok\n\n"
             "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 123 filtered out; finished in 0.00s\n\n").encode()
    if classify(0, valid, b'') != "CAUSE_UNKNOWN":
        return False
    for status, stdout, stderr in [(1, valid, b''), (0, valid, b'PUBLIC_HOSTILE'),
                                   (0, valid + b'PUBLIC_HOSTILE', b''),
                                   (0, valid.replace(b'CAUSE_UNKNOWN', b'PRIVATE_VALUE'), b'')]:
        if classify(status, stdout, stderr) != "EXECUTION":
            return False
    return True


def emit(code):
    print("code=tor_offline_e2e_" + code.lower())
    return 0 if code in ("PREPARE_AND_SDK_OK", "BASELINE_CONFIRMED") else 2


class Parser(argparse.ArgumentParser):
    def error(self, message):
        raise ValueError


def main():
    parser = Parser(description=__doc__)
    parser.add_argument("mode", choices=("baseline", "qualify", "private"))
    parser.add_argument("--native-path", type=Path)
    parser.add_argument("--hostname-path", type=Path)
    parser.add_argument("--ack-private-access", action="store_true")
    parser.add_argument("--synthetic-missing-nss", action="store_true")
    args = parser.parse_args()
    if sys.platform != "darwin" or hashlib.sha256(BINARY.read_bytes()).hexdigest() != SHA256:
        return emit("ARTIFACT")
    if args.synthetic_missing_nss and (args.mode == "private" or hashlib.sha256(NSS.read_bytes()).hexdigest() != NSS_SHA256):
        return emit("ARTIFACT")
    if not projection_selftest():
        return emit("PROJECTION")
    os.umask(0o077)
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    with tempfile.TemporaryDirectory(prefix="possums-r41-public-") as root:
        denied = Path(root) / "must-not-exist"
        for mode, expected in (("probe", "SANDBOX_QUALIFIED"), ("panic", "CAUSE_UNKNOWN")):
            if invoke(mode, denied, missing_nss=args.synthetic_missing_nss) != expected or denied.exists():
                return emit("QUALIFICATION")
        if args.synthetic_missing_nss and invoke("nss_probe", denied, missing_nss=True) != "NSS_INJECTED":
            return emit("QUALIFICATION")
        baseline = invoke("synthetic", denied, missing_nss=args.synthetic_missing_nss)
        if args.mode == "baseline":
            return emit("BASELINE_CONFIRMED" if baseline == "PREPARE_CONFIGURATION" else baseline)
        if baseline != "PREPARE_AND_SDK_OK":
            return emit(baseline)
        if args.mode == "qualify":
            return emit(baseline)
        if not args.ack_private_access:
            return emit("NOT_AUTHORIZED")
        paths = (args.native_path, args.hostname_path)
        if any(path is None or not path.is_absolute() for path in paths):
            return emit("ARGUMENTS")
        return emit(invoke("private", denied, paths))


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception:
        sys.exit(emit("CAUSE_UNKNOWN"))
