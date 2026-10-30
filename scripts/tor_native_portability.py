"""Inspect native runtime linkage using platform tools; never execute the binary."""
import hashlib
import json
import os
import re
import stat
import subprocess
import sys
from pathlib import Path

TARGETS = {
    "aarch64-apple-darwin": ("macho", "arm64"),
    "x86_64-apple-darwin": ("macho", "x86_64"),
    "aarch64-unknown-linux-musl": ("elf", "AArch64"),
    "x86_64-unknown-linux-musl": ("elf", "Advanced Micro Devices X86-64"),
}


MAX_ARTIFACT_BYTES = 64 * 1024 * 1024
HASH_CHUNK_BYTES = 1024 * 1024
INSPECTION_TIMEOUT_SECONDS = 30
TIMEOUT_CODE = "possums_build_tor_inspection_timeout"
TIMEOUT_MESSAGE = "possums_build_tor_inspection_timeout: static inspection exceeded its time limit. Check the platform inspection tools and artifact; do not accept it without completed inspection. No artifact was executed."
ARTIFACT_MESSAGE = "possums_build_tor_artifact: inspection requires the same stable regular executable, at most 64 MiB, throughout inspection. Restore the matching artifact and inspect again; no artifact was executed."
HASH_MESSAGE = "possums_build_tor_artifact: inspected bytes differ from the expected SHA-256. Restore the matching artifact and inspect again; no artifact was executed."
UNKNOWN_CODE = "possums_build_tor_inspection_unknown"
UNKNOWN_MESSAGE = "possums_build_tor_inspection_unknown: local executable inspection failed; cause unknown. Check the artifact and inspection tools; no artifact was executed."
FAILURE_CODES = frozenset({
    "possums_build_tor_inspection_tool",
    TIMEOUT_CODE,
    "possums_build_tor_format",
    "possums_build_tor_linkage",
    "possums_build_tor_deployment",
    "possums_build_tor_inspection_target",
    "possums_build_tor_artifact",
    "possums_build_tor_inspection_input",
    UNKNOWN_CODE,
})


class PortabilityFailure(Exception):
    """Explicit closed code and static message, never inspected values or output."""

    def __init__(self, code, message=UNKNOWN_MESSAGE):
        if type(code) is not str or code not in FAILURE_CODES:
            code, message = UNKNOWN_CODE, UNKNOWN_MESSAGE
        self.code = code
        super().__init__(message)


def tool(command, fd=None):
    try:
        result = subprocess.run(command, env={"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "LC_ALL": "C"},
                                capture_output=True, check=False, timeout=INSPECTION_TIMEOUT_SECONDS,
                                pass_fds=() if fd is None else (fd,))
        if result.returncode == 0:
            return result.stdout.decode("utf-8")
    except subprocess.TimeoutExpired:
        raise PortabilityFailure(TIMEOUT_CODE, TIMEOUT_MESSAGE) from None
    except (OSError, UnicodeError):
        pass
    raise PortabilityFailure("possums_build_tor_inspection_tool", "possums_build_tor_inspection_tool: executable inspection could not complete; cause unknown. Provide the platform readelf or Apple lipo/otool tools. Do not accept the artifact without inspection.")


def elf_linkage(text, machine):
    if not ("ELF Header:" in text and "Program Headers:" in text
            and re.search(r"^\s*Class:\s+ELF64\s*$", text, re.M)
            and re.search(r"^\s*Data:\s+2's complement, little endian\s*$", text, re.M)
            and re.search(r"^\s*Type:\s+(EXEC|DYN)\b", text, re.M)
            and re.search(r"^\s*Machine:\s+" + re.escape(machine) + r"\s*$", text, re.M)):
        raise PortabilityFailure("possums_build_tor_format", "possums_build_tor_format: executable format or architecture differs from the selected target. Rebuild with the approved target toolchain.")
    if (re.search(r"^\s*INTERP\s", text, re.M) or re.search(r"\((NEEDED|RPATH|RUNPATH)\)", text)
            or "/nix/store/" in text):
        raise PortabilityFailure("possums_build_tor_linkage", "possums_build_tor_linkage: Linux output has a runtime loader, shared-library dependency or search path. Rebuild as self-contained static musl; do not copy build libraries into the package.")
    return {"format": "elf64", "linkage": "static", "execution": "not_tested"}


def macho_linkage(text):
    commands = re.split(r"(?m)^Load command \d+\s*$", text)[1:]
    kinds = [re.search(r"^\s*cmd (LC_\w+)\s*$", block, re.M) for block in commands]
    if not commands or any(kind is None for kind in kinds):
        raise PortabilityFailure("possums_build_tor_format", "possums_build_tor_format: Mach-O load commands could not be validated. Rebuild and inspect with platform tools.")
    floor = None
    loader = False
    for block, match in zip(commands, kinds, strict=True):
        assert match is not None
        kind = match[1]
        if kind in {"LC_RPATH", "LC_DYLD_ENVIRONMENT"} or "/nix/store/" in block:
            raise PortabilityFailure("possums_build_tor_linkage", "possums_build_tor_linkage: Mach-O runtime search paths or environment overrides are not allowed. Rebuild with platform SDK libraries only.")
        if kind in {"LC_LOAD_DYLIB", "LC_LOAD_WEAK_DYLIB", "LC_REEXPORT_DYLIB", "LC_LOAD_UPWARD_DYLIB", "LC_LAZY_LOAD_DYLIB", "LC_LOAD_DYLINKER"}:
            name = re.search(r"^\s*name (\S+) \(offset \d+\)\s*$", block, re.M)
            if not name or ".." in name[1].split("/") or not name[1].startswith(("/usr/lib/", "/System/Library/Frameworks/")):
                raise PortabilityFailure("possums_build_tor_linkage", "possums_build_tor_linkage: Mach-O requires a non-platform library or loader. Rebuild with platform SDK libraries only; no bundled loader workaround is allowed.")
            if kind == "LC_LOAD_DYLINKER":
                if loader or name[1] != "/usr/lib/dyld":
                    raise PortabilityFailure("possums_build_tor_linkage", "possums_build_tor_linkage: the expected platform loader was not established. Rebuild with the platform toolchain.")
                loader = True
        if kind == "LC_BUILD_VERSION":
            minimum = re.search(r"^\s*minos ([0-9.]+)\s*$", block, re.M)
            if floor is not None or not re.search(r"^\s*platform (1|MACOS)\s*$", block, re.M) or not minimum:
                raise PortabilityFailure("possums_build_tor_deployment", "possums_build_tor_deployment: the macOS deployment target was not established. Rebuild for the proposed 14.0 qualification floor.")
            floor = minimum[1]
        if kind == "LC_VERSION_MIN_MACOSX":
            raise PortabilityFailure("possums_build_tor_deployment", "possums_build_tor_deployment: legacy deployment metadata is outside this candidate contract. Rebuild for macOS 14.0 with the selected SDK.")
    if not loader or floor != "14.0":
        raise PortabilityFailure("possums_build_tor_deployment", "possums_build_tor_deployment: macOS loader or minimum version differs from the candidate contract. Rebuild for 14.0; this check does not prove execution on that OS.")
    return {"format": "macho64", "linkage": "platform_only", "deploymentTarget": floor, "execution": "not_tested"}


def identity(info):
    return (info.st_dev, info.st_ino, info.st_mode, info.st_size, info.st_mtime_ns, info.st_ctime_ns)


def regular_executable(info):
    return stat.S_ISREG(info.st_mode) and info.st_mode & 0o111 and info.st_size <= MAX_ARTIFACT_BYTES


def require_stable(fd, artifact, initial):
    try:
        current = os.fstat(fd)
        selected = artifact.lstat()
    except OSError:
        raise PortabilityFailure("possums_build_tor_artifact", ARTIFACT_MESSAGE) from None
    if not regular_executable(current) or identity(current) != identity(initial) or identity(selected) != identity(initial):
        raise PortabilityFailure("possums_build_tor_artifact", ARTIFACT_MESSAGE)


def fingerprint(fd):
    os.lseek(fd, 0, os.SEEK_SET)
    digest = hashlib.sha256()
    magic = b""
    total = 0
    while True:
        chunk = os.read(fd, min(HASH_CHUNK_BYTES, MAX_ARTIFACT_BYTES + 1 - total))
        if not chunk:
            return magic, digest.hexdigest()
        if len(magic) < 4:
            magic = (magic + chunk)[:4]
        total += len(chunk)
        if total > MAX_ARTIFACT_BYTES:
            raise PortabilityFailure("possums_build_tor_artifact", ARTIFACT_MESSAGE)
        digest.update(chunk)


def inspect_fd(target, fd, magic):
    # SDK tools reopen only this inherited descriptor, never the mutable asset path.
    descriptor = f"/dev/fd/{fd}" if sys.platform == "darwin" else f"/proc/self/fd/{fd}"
    format_, arch = TARGETS[target]
    if format_ == "elf":
        if magic != b"\x7fELF":
            raise PortabilityFailure("possums_build_tor_format", "possums_build_tor_format: expected a Linux ELF executable. Rebuild for the approved target.")
        return elf_linkage(tool(["readelf", "-W", "-h", "-l", "-d", descriptor], fd), arch)
    if magic != b"\xcf\xfa\xed\xfe" or tool(["/usr/bin/lipo", "-archs", descriptor], fd).strip() != arch:
        raise PortabilityFailure("possums_build_tor_format", "possums_build_tor_format: expected a thin Mach-O executable of the selected architecture. Rebuild for the approved target.")
    if not re.search(r"\bEXECUTE\b", tool(["/usr/bin/otool", "-hv", descriptor], fd)):
        raise PortabilityFailure("possums_build_tor_format", "possums_build_tor_format: the selected Mach-O is not an executable. Restore the matching artifact.")
    return macho_linkage(tool(["/usr/bin/otool", "-l", descriptor], fd))


def inspect(target, artifact, expected_sha256=None):
    if target not in TARGETS:
        raise PortabilityFailure("possums_build_tor_inspection_target", "possums_build_tor_inspection_target: inspection requires one of the four approved target triples. No artifact was executed.")
    if expected_sha256 is not None and (type(expected_sha256) is not str or re.fullmatch(r"[0-9a-f]{64}", expected_sha256) is None):
        raise PortabilityFailure("possums_build_tor_inspection_input", "possums_build_tor_inspection_input: expected SHA-256 must be exactly 64 lowercase hexadecimal characters. Supply the validated artifact digest; no artifact was executed.")
    initial = artifact.lstat()
    if not regular_executable(initial):
        raise PortabilityFailure("possums_build_tor_artifact", ARTIFACT_MESSAGE)
    try:
        fd = os.open(artifact, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW)
    except OSError:
        raise PortabilityFailure("possums_build_tor_artifact", ARTIFACT_MESSAGE) from None
    primary_failed = False
    try:
        require_stable(fd, artifact, initial)
        magic, digest = fingerprint(fd)
        require_stable(fd, artifact, initial)
        if expected_sha256 is not None and digest != expected_sha256:
            raise PortabilityFailure("possums_build_tor_artifact", HASH_MESSAGE)
        evidence = inspect_fd(target, fd, magic)
        require_stable(fd, artifact, initial)
        _, final_digest = fingerprint(fd)
        require_stable(fd, artifact, initial)
        if final_digest != digest:
            raise PortabilityFailure("possums_build_tor_artifact", HASH_MESSAGE if expected_sha256 is not None else ARTIFACT_MESSAGE)
        return evidence
    except BaseException:
        primary_failed = True
        raise
    finally:
        try:
            os.close(fd)
        except OSError:
            if not primary_failed:
                raise PortabilityFailure(UNKNOWN_CODE, UNKNOWN_MESSAGE) from None


def main(argv):
    json_mode = bool(argv and argv[0] == "--json")
    if json_mode:
        argv = argv[1:]
    try:
        if len(argv) == 4 and argv[2] == "--sha256":
            evidence = inspect(argv[0], Path(argv[1]), expected_sha256=argv[3])
        elif len(argv) == 2:
            evidence = inspect(argv[0], Path(argv[1]))
        else:
            raise PortabilityFailure("possums_build_tor_inspection_input", "possums_build_tor_inspection_input: supply an approved target and artifact path. No artifact was executed.")
        print(json.dumps({"ok": True, "evidence": evidence} if json_mode else evidence))
        return 0
    except PortabilityFailure as error:
        if json_mode:
            print(json.dumps({"ok": False, "code": error.code}))
        else:
            print(str(error), file=sys.stderr)
    except Exception:
        if json_mode:
            print(json.dumps({"ok": False, "code": UNKNOWN_CODE}))
        else:
            print(UNKNOWN_MESSAGE, file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
