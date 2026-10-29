"""Inspect native runtime linkage using platform tools; never execute the binary."""
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


class PortabilityFailure(Exception):
    """Static messages only; never include an inspected value or tool output."""


def tool(command):
    try:
        result = subprocess.run(command, env={"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "LC_ALL": "C"},
                                capture_output=True, check=False)
        if result.returncode == 0:
            return result.stdout.decode("utf-8")
    except (OSError, UnicodeError):
        pass
    raise PortabilityFailure("possums_build_tor_inspection_tool: executable inspection could not complete; cause unknown. Provide the platform readelf or Apple lipo/otool tools. Do not accept the artifact without inspection.")


def elf_linkage(text, machine):
    if not ("ELF Header:" in text and "Program Headers:" in text
            and re.search(r"^\s*Class:\s+ELF64\s*$", text, re.M)
            and re.search(r"^\s*Data:\s+2's complement, little endian\s*$", text, re.M)
            and re.search(r"^\s*Type:\s+(EXEC|DYN)\b", text, re.M)
            and re.search(r"^\s*Machine:\s+" + re.escape(machine) + r"\s*$", text, re.M)):
        raise PortabilityFailure("possums_build_tor_format: executable format or architecture differs from the selected target. Rebuild with the approved target toolchain.")
    if (re.search(r"^\s*INTERP\s", text, re.M) or re.search(r"\((NEEDED|RPATH|RUNPATH)\)", text)
            or "/nix/store/" in text):
        raise PortabilityFailure("possums_build_tor_linkage: Linux output has a runtime loader, shared-library dependency or search path. Rebuild as self-contained static musl; do not copy build libraries into the package.")
    return {"format": "elf64", "linkage": "static", "execution": "not_tested"}


def macho_linkage(text):
    commands = re.split(r"(?m)^Load command \d+\s*$", text)[1:]
    kinds = [re.search(r"^\s*cmd (LC_\w+)\s*$", block, re.M) for block in commands]
    if not commands or any(kind is None for kind in kinds):
        raise PortabilityFailure("possums_build_tor_format: Mach-O load commands could not be validated. Rebuild and inspect with platform tools.")
    floor = None
    loader = False
    for block, match in zip(commands, kinds, strict=True):
        assert match is not None
        kind = match[1]
        if kind in {"LC_RPATH", "LC_DYLD_ENVIRONMENT"} or "/nix/store/" in block:
            raise PortabilityFailure("possums_build_tor_linkage: Mach-O runtime search paths or environment overrides are not allowed. Rebuild with platform SDK libraries only.")
        if kind in {"LC_LOAD_DYLIB", "LC_LOAD_WEAK_DYLIB", "LC_REEXPORT_DYLIB", "LC_LOAD_UPWARD_DYLIB", "LC_LAZY_LOAD_DYLIB", "LC_LOAD_DYLINKER"}:
            name = re.search(r"^\s*name (\S+) \(offset \d+\)\s*$", block, re.M)
            if not name or ".." in name[1].split("/") or not name[1].startswith(("/usr/lib/", "/System/Library/Frameworks/")):
                raise PortabilityFailure("possums_build_tor_linkage: Mach-O requires a non-platform library or loader. Rebuild with platform SDK libraries only; no bundled loader workaround is allowed.")
            if kind == "LC_LOAD_DYLINKER":
                if loader or name[1] != "/usr/lib/dyld":
                    raise PortabilityFailure("possums_build_tor_linkage: the expected platform loader was not established. Rebuild with the platform toolchain.")
                loader = True
        if kind == "LC_BUILD_VERSION":
            minimum = re.search(r"^\s*minos ([0-9.]+)\s*$", block, re.M)
            if floor is not None or not re.search(r"^\s*platform (1|MACOS)\s*$", block, re.M) or not minimum:
                raise PortabilityFailure("possums_build_tor_deployment: the macOS deployment target was not established. Rebuild for the proposed 14.0 qualification floor.")
            floor = minimum[1]
        if kind == "LC_VERSION_MIN_MACOSX":
            raise PortabilityFailure("possums_build_tor_deployment: legacy deployment metadata is outside this candidate contract. Rebuild for macOS 14.0 with the selected SDK.")
    if not loader or floor != "14.0":
        raise PortabilityFailure("possums_build_tor_deployment: macOS loader or minimum version differs from the candidate contract. Rebuild for 14.0; this check does not prove execution on that OS.")
    return {"format": "macho64", "linkage": "platform_only", "deploymentTarget": floor, "execution": "not_tested"}


def inspect(target, artifact):
    if target not in TARGETS:
        raise PortabilityFailure("possums_build_tor_inspection_target: inspection requires one of the four approved target triples. No artifact was executed.")
    if not stat.S_ISREG(artifact.lstat().st_mode) or not artifact.stat().st_mode & 0o111:
        raise PortabilityFailure("possums_build_tor_artifact: expected a regular executable file, not a link or non-executable. Restore the matching build artifact.")
    with artifact.open("rb") as handle:
        magic = handle.read(4)
    format_, arch = TARGETS[target]
    if format_ == "elf":
        if magic != b"\x7fELF":
            raise PortabilityFailure("possums_build_tor_format: expected a Linux ELF executable. Rebuild for the approved target.")
        return elf_linkage(tool(["readelf", "-W", "-h", "-l", "-d", str(artifact)]), arch)
    if magic != b"\xcf\xfa\xed\xfe" or tool(["/usr/bin/lipo", "-archs", str(artifact)]).strip() != arch:
        raise PortabilityFailure("possums_build_tor_format: expected a thin Mach-O executable of the selected architecture. Rebuild for the approved target.")
    if not re.search(r"\bEXECUTE\b", tool(["/usr/bin/otool", "-hv", str(artifact)])):
        raise PortabilityFailure("possums_build_tor_format: the selected Mach-O is not an executable. Restore the matching artifact.")
    return macho_linkage(tool(["/usr/bin/otool", "-l", str(artifact)]))


if __name__ == "__main__":
    try:
        if len(sys.argv) != 3:
            raise PortabilityFailure("possums_build_tor_inspection_input: supply an approved target and artifact path. No artifact was executed.")
        print(json.dumps(inspect(sys.argv[1], Path(sys.argv[2]))))
    except PortabilityFailure as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
    except Exception:
        print("possums_build_tor_inspection_unknown: local executable inspection failed; cause unknown. Check the artifact and inspection tools; no artifact was executed.", file=sys.stderr)
        sys.exit(1)
